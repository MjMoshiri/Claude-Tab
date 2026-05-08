use crate::workflow::compiler::{compile, CompileError};
use crate::workflow::ir::Workflow;
use crate::workflow::parser::{parse, ParseError};
use crate::workflow::registry::WorkflowRegistry;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("io error reading {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("parse error in {0}: {1}")]
    Parse(PathBuf, ParseError),
    #[error("compile error in {0}: {1}")]
    Compile(PathBuf, CompileError),
    #[error("file too large: {0} ({1} bytes, max 256KB)")]
    TooLarge(PathBuf, u64),
}

pub struct LoadResult {
    pub loaded: Vec<(Workflow, String)>, // (workflow, source_hash)
    pub errors: Vec<LoadError>,
}

const MAX_FILE_BYTES: u64 = 256 * 1024;

pub fn scan_directory(dir: &Path) -> LoadResult {
    let mut loaded = Vec::new();
    let mut errors = Vec::new();

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            errors.push(LoadError::Io(dir.to_path_buf(), e));
            return LoadResult { loaded, errors };
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        match load_one(&path) {
            Ok(pair) => loaded.push(pair),
            Err(e) => errors.push(e),
        }
    }

    LoadResult { loaded, errors }
}

pub fn load_one(path: &Path) -> Result<(Workflow, String), LoadError> {
    let meta = std::fs::metadata(path).map_err(|e| LoadError::Io(path.to_path_buf(), e))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(LoadError::TooLarge(path.to_path_buf(), meta.len()));
    }
    let src = std::fs::read_to_string(path).map_err(|e| LoadError::Io(path.to_path_buf(), e))?;
    let wf = parse(&src).map_err(|e| LoadError::Parse(path.to_path_buf(), e))?;
    compile(&wf).map_err(|e| LoadError::Compile(path.to_path_buf(), e))?;

    let hash = hex::encode(Sha256::digest(src.as_bytes()));
    Ok((wf, hash))
}

pub fn populate_registry(dir: &Path, reg: &WorkflowRegistry) -> Vec<LoadError> {
    let result = scan_directory(dir);
    for (wf, _hash) in result.loaded {
        reg.upsert(wf);
    }
    result.errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(dir: &Path, name: &str, contents: &str) {
        std::fs::write(dir.join(name), contents).unwrap();
    }

    const VALID: &str = r#"---
id: w1
name: Test
inputs:
  - name: task
    description: x
    required: true
    type: string
---

## Stage: a
prompt: |
  do {{task}}
completion:
  type: criteria
  criteria: done
next: done
"#;

    #[test]
    fn loads_valid_workflow() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "w1.md", VALID);
        let result = scan_directory(tmp.path());
        assert_eq!(result.loaded.len(), 1);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn bad_file_does_not_poison_others() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "ok.md", VALID);
        write(tmp.path(), "bad.md", "no frontmatter here");
        let result = scan_directory(tmp.path());
        assert_eq!(result.loaded.len(), 1);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn populates_registry() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "w1.md", VALID);
        let reg = WorkflowRegistry::new();
        let errs = populate_registry(tmp.path(), &reg);
        assert!(errs.is_empty());
        assert!(reg.get("w1").is_some());
    }
}
