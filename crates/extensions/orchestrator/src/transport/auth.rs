use rand::RngCore;
use std::path::PathBuf;

pub struct LocalAuth {
    pub token: String,
    pub config_dir: PathBuf,
}

impl LocalAuth {
    pub fn generate(config_dir: PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(&config_dir)?;
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let token = hex::encode(bytes);

        let token_path = config_dir.join("orchestrator.token");
        std::fs::write(&token_path, &token)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&token_path)?.permissions();
            perms.set_mode(0o600);
            std::fs::set_permissions(&token_path, perms)?;
        }

        Ok(Self { token, config_dir })
    }

    pub fn write_port(&self, port: u16) -> std::io::Result<()> {
        std::fs::write(self.config_dir.join("orchestrator.port"), port.to_string())
    }

    pub fn cleanup(&self) {
        let _ = std::fs::remove_file(self.config_dir.join("orchestrator.token"));
        let _ = std::fs::remove_file(self.config_dir.join("orchestrator.port"));
    }

    pub fn verify(&self, provided: &str) -> bool {
        constant_time_eq(self.token.as_bytes(), provided.as_bytes())
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

pub fn default_config_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".claude-tabs")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn generates_64_char_hex_token() {
        let tmp = TempDir::new().unwrap();
        let auth = LocalAuth::generate(tmp.path().to_path_buf()).unwrap();
        assert_eq!(auth.token.len(), 64);
        assert!(auth.token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn writes_token_file() {
        let tmp = TempDir::new().unwrap();
        let auth = LocalAuth::generate(tmp.path().to_path_buf()).unwrap();
        let on_disk = std::fs::read_to_string(tmp.path().join("orchestrator.token")).unwrap();
        assert_eq!(on_disk, auth.token);
    }

    #[cfg(unix)]
    #[test]
    fn token_file_has_mode_0600() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let _auth = LocalAuth::generate(tmp.path().to_path_buf()).unwrap();
        let mode = std::fs::metadata(tmp.path().join("orchestrator.token"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn verify_constant_time() {
        let tmp = TempDir::new().unwrap();
        let auth = LocalAuth::generate(tmp.path().to_path_buf()).unwrap();
        assert!(auth.verify(&auth.token));
        assert!(!auth.verify("wrong"));
        assert!(!auth.verify(""));
    }
}
