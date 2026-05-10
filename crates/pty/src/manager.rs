use claude_tabs_core::traits::provider::PtySize;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, error, info};

// macOS/Linux POSIX_SPAWN_SETSID — not exposed by libc-rs on darwin.
#[cfg(target_os = "macos")]
const POSIX_SPAWN_SETSID: libc::c_int = 0x0400;
#[cfg(target_os = "linux")]
const POSIX_SPAWN_SETSID: libc::c_int = 128;

extern "C" {
    fn posix_spawn_file_actions_addchdir_np(
        actions: *mut libc::posix_spawn_file_actions_t,
        path: *const libc::c_char,
    ) -> libc::c_int;

    fn ptsname_r(
        fd: libc::c_int,
        buf: *mut libc::c_char,
        buflen: libc::size_t,
    ) -> libc::c_int;
}

struct PtyInstance {
    master: OwnedFd,
    writer: Option<File>,
    pid: libc::pid_t,
    exited: Option<i32>,
    size: PtySize,
}

pub struct PtyManager {
    instances: Arc<Mutex<HashMap<String, PtyInstance>>>,
}

#[derive(Debug, thiserror::Error)]
pub enum PtyError {
    #[error("Spawn failed: {0}")]
    SpawnFailed(String),
    #[error("Session not found: {0}")]
    NotFound(String),
    #[error("Write failed: {0}")]
    WriteFailed(String),
    #[error("Resize failed: {0}")]
    ResizeFailed(String),
}

fn errno_err(prefix: &str) -> PtyError {
    PtyError::SpawnFailed(format!("{}: {}", prefix, io::Error::last_os_error()))
}

fn open_pty_pair() -> Result<(OwnedFd, CString), PtyError> {
    unsafe {
        let raw = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        if raw < 0 {
            return Err(errno_err("posix_openpt"));
        }
        let master = OwnedFd::from_raw_fd(raw);

        if libc::grantpt(master.as_raw_fd()) != 0 {
            return Err(errno_err("grantpt"));
        }
        if libc::unlockpt(master.as_raw_fd()) != 0 {
            return Err(errno_err("unlockpt"));
        }

        let mut buf = [0u8; 256];
        let rc = ptsname_r(
            master.as_raw_fd(),
            buf.as_mut_ptr() as *mut libc::c_char,
            buf.len(),
        );
        if rc != 0 {
            return Err(PtyError::SpawnFailed(format!("ptsname_r: errno {}", rc)));
        }
        let slave_path = CStr::from_ptr(buf.as_ptr() as *const libc::c_char).to_owned();

        let flags = libc::fcntl(master.as_raw_fd(), libc::F_GETFD);
        if flags >= 0 {
            libc::fcntl(master.as_raw_fd(), libc::F_SETFD, flags | libc::FD_CLOEXEC);
        }

        Ok((master, slave_path))
    }
}

fn set_winsize(fd: RawFd, size: PtySize) -> Result<(), PtyError> {
    let ws = libc::winsize {
        ws_row: size.rows,
        ws_col: size.cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    if unsafe { libc::ioctl(fd, libc::TIOCSWINSZ as _, &ws) } != 0 {
        return Err(PtyError::ResizeFailed(
            io::Error::last_os_error().to_string(),
        ));
    }
    Ok(())
}

/// Resolve `command` to an absolute path using the PATH from `env` (the env
/// we will hand to the child), falling back to the parent's PATH, then to
/// a minimal default. This is needed because `posix_spawnp` resolves PATH
/// from the *caller's* environment — which is minimal when the .app is
/// launched from Finder/Dock.
fn resolve_command(
    command: &str,
    env: &HashMap<String, String>,
) -> Result<CString, PtyError> {
    if command.contains('/') {
        return CString::new(command)
            .map_err(|e| PtyError::SpawnFailed(e.to_string()));
    }

    let path_str = env
        .get("PATH")
        .cloned()
        .or_else(|| std::env::var("PATH").ok())
        .unwrap_or_else(|| "/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin".to_string());

    for dir in path_str.split(':').filter(|d| !d.is_empty()) {
        let candidate = Path::new(dir).join(command);
        if let Ok(meta) = std::fs::metadata(&candidate) {
            if meta.is_file() {
                return CString::new(candidate.as_os_str().as_bytes())
                    .map_err(|e| PtyError::SpawnFailed(e.to_string()));
            }
        }
    }

    Err(PtyError::SpawnFailed(format!(
        "command not found in PATH: {} (PATH={})",
        command, path_str
    )))
}

fn spawn_child(
    command_abs: &CStr,
    argv0: &str,
    args: &[String],
    working_dir: Option<&str>,
    env: &HashMap<String, String>,
    slave_path: &CStr,
) -> Result<libc::pid_t, PtyError> {
    let argv0_c =
        CString::new(argv0).map_err(|e| PtyError::SpawnFailed(e.to_string()))?;
    let mut argv_c: Vec<CString> = Vec::with_capacity(args.len() + 1);
    argv_c.push(argv0_c);
    for a in args {
        argv_c
            .push(CString::new(a.as_str()).map_err(|e| PtyError::SpawnFailed(e.to_string()))?);
    }
    let mut argv_ptrs: Vec<*mut libc::c_char> =
        argv_c.iter().map(|s| s.as_ptr() as *mut _).collect();
    argv_ptrs.push(std::ptr::null_mut());

    let mut env_map: HashMap<String, String> = std::env::vars().collect();
    for (k, v) in env {
        env_map.insert(k.clone(), v.clone());
    }
    let env_c: Vec<CString> = env_map
        .iter()
        .filter_map(|(k, v)| CString::new(format!("{}={}", k, v)).ok())
        .collect();
    let mut envp_ptrs: Vec<*mut libc::c_char> =
        env_c.iter().map(|s| s.as_ptr() as *mut _).collect();
    envp_ptrs.push(std::ptr::null_mut());

    let cwd_c = working_dir
        .map(|d| CString::new(d).map_err(|e| PtyError::SpawnFailed(e.to_string())))
        .transpose()?;

    unsafe {
        let mut attr: libc::posix_spawnattr_t = std::mem::zeroed();
        if libc::posix_spawnattr_init(&mut attr) != 0 {
            return Err(errno_err("posix_spawnattr_init"));
        }

        let mut flags: libc::c_int = libc::POSIX_SPAWN_SETSIGDEF | POSIX_SPAWN_SETSID;
        #[cfg(target_os = "macos")]
        {
            flags |= libc::POSIX_SPAWN_CLOEXEC_DEFAULT;
        }
        libc::posix_spawnattr_setflags(&mut attr, flags as libc::c_short);

        let mut sigs: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut sigs);
        for s in &[
            libc::SIGCHLD,
            libc::SIGHUP,
            libc::SIGINT,
            libc::SIGQUIT,
            libc::SIGTERM,
            libc::SIGALRM,
        ] {
            libc::sigaddset(&mut sigs, *s);
        }
        libc::posix_spawnattr_setsigdefault(&mut attr, &sigs);

        let mut actions: libc::posix_spawn_file_actions_t = std::mem::zeroed();
        if libc::posix_spawn_file_actions_init(&mut actions) != 0 {
            libc::posix_spawnattr_destroy(&mut attr);
            return Err(errno_err("posix_spawn_file_actions_init"));
        }

        // Open slave on stdin (no O_NOCTTY → becomes CTTY after SETSID).
        let rc = libc::posix_spawn_file_actions_addopen(
            &mut actions,
            0,
            slave_path.as_ptr(),
            libc::O_RDWR,
            0,
        );
        if rc != 0 {
            libc::posix_spawn_file_actions_destroy(&mut actions);
            libc::posix_spawnattr_destroy(&mut attr);
            return Err(PtyError::SpawnFailed(format!(
                "addopen(slave): errno {}",
                rc
            )));
        }
        libc::posix_spawn_file_actions_adddup2(&mut actions, 0, 1);
        libc::posix_spawn_file_actions_adddup2(&mut actions, 0, 2);

        if let Some(ref cwd) = cwd_c {
            let rc = posix_spawn_file_actions_addchdir_np(&mut actions, cwd.as_ptr());
            if rc != 0 {
                libc::posix_spawn_file_actions_destroy(&mut actions);
                libc::posix_spawnattr_destroy(&mut attr);
                return Err(PtyError::SpawnFailed(format!(
                    "addchdir_np: errno {}",
                    rc
                )));
            }
        }

        let mut pid: libc::pid_t = 0;
        let rc = libc::posix_spawn(
            &mut pid,
            command_abs.as_ptr(),
            &actions,
            &attr,
            argv_ptrs.as_mut_ptr(),
            envp_ptrs.as_mut_ptr(),
        );

        libc::posix_spawn_file_actions_destroy(&mut actions);
        libc::posix_spawnattr_destroy(&mut attr);

        if rc != 0 {
            return Err(PtyError::SpawnFailed(format!(
                "posix_spawn({}): errno {} ({})",
                command_abs.to_string_lossy(),
                rc,
                io::Error::from_raw_os_error(rc)
            )));
        }

        Ok(pid)
    }
}

fn dup_to_file(fd: RawFd) -> Result<File, PtyError> {
    let new_fd = unsafe { libc::dup(fd) };
    if new_fd < 0 {
        return Err(PtyError::SpawnFailed(
            io::Error::last_os_error().to_string(),
        ));
    }
    unsafe {
        let f = libc::fcntl(new_fd, libc::F_GETFD);
        if f >= 0 {
            libc::fcntl(new_fd, libc::F_SETFD, f | libc::FD_CLOEXEC);
        }
    }
    Ok(unsafe { File::from_raw_fd(new_fd) })
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            instances: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn spawn(
        &self,
        session_id: &str,
        command: &str,
        args: &[String],
        working_dir: Option<&str>,
        env: &HashMap<String, String>,
        size: PtySize,
    ) -> Result<Box<dyn Read + Send>, PtyError> {
        let command_abs = resolve_command(command, env).map_err(|e| {
            error!(session_id = %session_id, command = %command, error = %e, "command resolution failed");
            e
        })?;

        let (master, slave_path) = open_pty_pair()?;

        let pid = spawn_child(&command_abs, command, args, working_dir, env, &slave_path)
            .map_err(|e| {
                error!(session_id = %session_id, command = %command, working_dir = ?working_dir, error = %e, "posix_spawn child failed");
                e
            })?;

        info!(
            session_id = %session_id,
            pid = pid,
            command = %command_abs.to_string_lossy(),
            "PTY spawned (posix_spawn)"
        );

        // master TIOCSWINSZ returns ENOTTY before slave opens on macOS, so
        // apply initial winsize after the child has the slave.
        if let Err(e) = set_winsize(master.as_raw_fd(), size) {
            error!(session_id = %session_id, error = %e, "initial winsize failed (non-fatal)");
        }

        let reader = dup_to_file(master.as_raw_fd())?;
        let writer = dup_to_file(master.as_raw_fd())?;

        let instance = PtyInstance {
            master,
            writer: Some(writer),
            pid,
            exited: None,
            size,
        };
        self.instances
            .lock()
            .insert(session_id.to_string(), instance);

        Ok(Box::new(reader))
    }

    pub fn write_data(&self, session_id: &str, data: &[u8]) -> Result<(), PtyError> {
        let mut instances = self.instances.lock();
        let instance = instances
            .get_mut(session_id)
            .ok_or_else(|| PtyError::NotFound(session_id.to_string()))?;

        match &mut instance.writer {
            Some(writer) => {
                writer
                    .write_all(data)
                    .map_err(|e| PtyError::WriteFailed(e.to_string()))?;
                writer
                    .flush()
                    .map_err(|e| PtyError::WriteFailed(e.to_string()))?;
                Ok(())
            }
            None => Err(PtyError::WriteFailed(format!(
                "Writer unavailable for session {}",
                session_id
            ))),
        }
    }

    pub fn resize(&self, session_id: &str, size: PtySize) -> Result<(), PtyError> {
        let mut instances = self.instances.lock();
        let instance = instances
            .get_mut(session_id)
            .ok_or_else(|| PtyError::NotFound(session_id.to_string()))?;
        set_winsize(instance.master.as_raw_fd(), size)?;
        instance.size = size;
        debug!(session_id = %session_id, rows = size.rows, cols = size.cols, "PTY resized");
        Ok(())
    }

    pub fn close(&self, session_id: &str) -> Result<(), PtyError> {
        let mut instances = self.instances.lock();
        if let Some(mut instance) = instances.remove(session_id) {
            instance.writer.take();
            unsafe {
                libc::kill(instance.pid, libc::SIGTERM);
            }
            info!(session_id = %session_id, "PTY closed");
            Ok(())
        } else {
            Err(PtyError::NotFound(session_id.to_string()))
        }
    }

    pub fn is_alive(&self, session_id: &str) -> bool {
        let mut instances = self.instances.lock();
        if let Some(instance) = instances.get_mut(session_id) {
            if instance.exited.is_some() {
                return false;
            }
            let mut status: libc::c_int = 0;
            let rc = unsafe { libc::waitpid(instance.pid, &mut status, libc::WNOHANG) };
            if rc == 0 {
                true
            } else if rc == instance.pid {
                instance.exited = Some(status);
                false
            } else {
                let err = io::Error::last_os_error();
                debug!(session_id = %session_id, error = %err, "Error checking PTY status, assuming alive");
                true
            }
        } else {
            false
        }
    }

    pub fn session_count(&self) -> usize {
        self.instances.lock().len()
    }
}

impl Default for PtyManager {
    fn default() -> Self {
        Self::new()
    }
}
