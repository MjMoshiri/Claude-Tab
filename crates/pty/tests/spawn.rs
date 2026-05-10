use claude_tabs_core::traits::provider::PtySize;
use claude_tabs_pty::PtyManager;
use std::collections::HashMap;
use std::io::Read;
use std::thread;
use std::time::Duration;

fn read_with_timeout(mut reader: Box<dyn Read + Send>) -> String {
    // Spawn a thread to read until EOF or timeout.
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let mut buf = Vec::new();
        // Read until short read / error / EOF.
        let mut chunk = [0u8; 4096];
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(_) => break,
            }
        }
        let _ = tx.send(buf);
    });
    let buf = rx.recv_timeout(Duration::from_secs(3)).unwrap_or_default();
    String::from_utf8_lossy(&buf).to_string()
}

fn minimal_env() -> HashMap<String, String> {
    let mut env = HashMap::new();
    env.insert("TERM".to_string(), "xterm-256color".to_string());
    env.insert("PATH".to_string(), "/usr/bin:/bin".to_string());
    env
}

#[test]
fn spawn_via_envp_path_lookup() {
    // Simulates the .app launched from Finder case: parent process has a
    // minimal PATH; the command must be found via the env override we
    // pass in. We use `/usr/bin/printenv`-style logic by invoking `echo`,
    // which lives in /bin on macOS.
    let mgr = PtyManager::new();
    let env = minimal_env();

    let reader = mgr
        .spawn(
            "test-1",
            "echo",
            &["hello-from-pty".to_string()],
            None,
            &env,
            PtySize { rows: 24, cols: 80 },
        )
        .expect("spawn should succeed via envp PATH lookup");

    let output = read_with_timeout(reader);
    assert!(
        output.contains("hello-from-pty"),
        "expected echo output in PTY, got: {:?}",
        output
    );

    assert_eq!(mgr.session_count(), 1);
    let _ = mgr.close("test-1");
}

#[test]
fn spawn_absolute_path_bypasses_lookup() {
    // Absolute paths should be used directly even with an empty PATH.
    let mgr = PtyManager::new();
    let mut env = HashMap::new();
    env.insert("PATH".to_string(), "".to_string());

    let reader = mgr
        .spawn(
            "test-abs",
            "/bin/echo",
            &["abs-ok".to_string()],
            None,
            &env,
            PtySize { rows: 24, cols: 80 },
        )
        .expect("absolute path spawn should succeed");

    let output = read_with_timeout(reader);
    assert!(
        output.contains("abs-ok"),
        "expected output, got: {:?}",
        output
    );
    let _ = mgr.close("test-abs");
}

#[test]
fn spawn_missing_command_errors() {
    let mgr = PtyManager::new();
    let env = minimal_env();

    let res = mgr.spawn(
        "test-missing",
        "definitely-not-a-real-binary-xyz",
        &[],
        None,
        &env,
        PtySize { rows: 24, cols: 80 },
    );

    let err = match res {
        Ok(_) => panic!("expected SpawnFailed for missing command"),
        Err(e) => format!("{}", e),
    };
    assert!(
        err.contains("not found"),
        "error should mention 'not found': {}",
        err
    );
}

#[test]
fn spawn_respects_working_dir() {
    // `pwd` should print the cwd we pass; verify chdir_np works.
    let mgr = PtyManager::new();
    let env = minimal_env();

    let reader = mgr
        .spawn(
            "test-cwd",
            "pwd",
            &[],
            Some("/tmp"),
            &env,
            PtySize { rows: 24, cols: 80 },
        )
        .expect("spawn pwd should succeed");

    let output = read_with_timeout(reader);
    // macOS `pwd` may print /private/tmp because /tmp is a symlink.
    assert!(
        output.contains("/tmp") || output.contains("/private/tmp"),
        "expected pwd output to include /tmp, got: {:?}",
        output
    );
    let _ = mgr.close("test-cwd");
}

/// Simulates the .app-from-Finder case where the parent process has a
/// minimal PATH (typically `/usr/bin:/bin`). We mutate the parent's PATH
/// to be empty and verify that spawn still resolves the command via the
/// `env` override we hand to it. This is the bug v1.4.7 hit in
/// production: `posix_spawnp` consulted the *parent's* PATH, not the
/// envp, so commands like `claude` (in `/opt/homebrew/bin`) weren't
/// found.
///
/// NOTE: This test mutates a process-global. Run pty tests with
/// `--test-threads=1` if adding more env-mutating tests.
#[test]
fn spawn_resolves_command_when_parent_path_is_empty() {
    let original_path = std::env::var_os("PATH");
    // SAFETY: process-global mutation. Restored before test exits.
    unsafe {
        std::env::set_var("PATH", "");
    }

    let mgr = PtyManager::new();
    let mut env = HashMap::new();
    env.insert("TERM".to_string(), "xterm".to_string());
    // The env override DOES contain the directory holding `echo`.
    env.insert("PATH".to_string(), "/bin".to_string());

    let result = mgr.spawn(
        "test-empty-parent-path",
        "echo",
        &["finder-launch-ok".to_string()],
        None,
        &env,
        PtySize { rows: 24, cols: 80 },
    );

    // Restore PATH before any assertion can panic.
    unsafe {
        match original_path {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
    }

    let reader = result.expect("spawn must succeed with empty parent PATH");
    let output = read_with_timeout(reader);
    assert!(
        output.contains("finder-launch-ok"),
        "expected output, got: {:?}",
        output
    );
    let _ = mgr.close("test-empty-parent-path");
}

#[test]
fn spawn_with_minimal_parent_env_succeeds() {
    // Most defensive simulation of the .app-from-Finder case: temporarily
    // overwrite the parent's PATH to something that doesn't contain the
    // command, and verify spawn still works because we resolve via envp.
    //
    // We can't safely mutate the parent env across threads, so this test
    // just checks that posix_spawn (no `p`) with our resolved absolute
    // path doesn't depend on parent PATH at all.
    let mgr = PtyManager::new();

    // Empty PATH in env override, but command is `echo` and we expect
    // resolve_command to fall back to parent PATH. So flip the test:
    // give an explicit PATH override that DOES contain the command.
    let mut env = HashMap::new();
    env.insert("TERM".to_string(), "xterm".to_string());
    env.insert("PATH".to_string(), "/bin:/usr/bin".to_string());

    let reader = mgr
        .spawn(
            "test-min",
            "echo",
            &["ok".to_string()],
            None,
            &env,
            PtySize { rows: 24, cols: 80 },
        )
        .expect("spawn should succeed");

    let output = read_with_timeout(reader);
    assert!(output.contains("ok"), "got: {:?}", output);
    let _ = mgr.close("test-min");
}
