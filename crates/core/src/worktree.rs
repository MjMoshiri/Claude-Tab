use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeInfo {
    pub path: String,
    pub branch: String,
    pub repo_path: String,
}

#[derive(Debug, thiserror::Error)]
pub enum WorktreeError {
    #[error("Not a git repository: {0}")]
    NotGitRepo(String),
    #[error("Git error: {0}")]
    GitError(String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

impl Serialize for WorktreeError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

/// Check if the given path is inside a git repository.
pub fn is_git_repo(path: &str) -> bool {
    Command::new("git")
        .args(["-C", path, "rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Create a git worktree under `<repo>/.claude/worktrees/<branch>`.
/// If `branch_name` is None, a name is auto-generated as `claude-tabs-<short-uuid>`.
pub fn create_worktree(repo_path: &str, branch_name: Option<&str>) -> Result<WorktreeInfo, WorktreeError> {
    if !is_git_repo(repo_path) {
        return Err(WorktreeError::NotGitRepo(repo_path.to_string()));
    }

    let branch = branch_name
        .map(String::from)
        .unwrap_or_else(|| {
            let short_id = &uuid::Uuid::new_v4().to_string()[..8];
            format!("claude-tabs-{}", short_id)
        });

    let worktree_dir = Path::new(repo_path).join(".claude").join("worktrees").join(&branch);
    let worktree_path = worktree_dir.to_string_lossy().to_string();

    // Ensure parent directory exists
    if let Some(parent) = worktree_dir.parent() {
        std::fs::create_dir_all(parent)?;
    }

    debug!(repo = %repo_path, branch = %branch, path = %worktree_path, "Creating git worktree");

    let output = Command::new("git")
        .args(["-C", repo_path, "worktree", "add", "-b", &branch, &worktree_path])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(WorktreeError::GitError(stderr.to_string()));
    }

    info!(branch = %branch, path = %worktree_path, "Git worktree created");

    Ok(WorktreeInfo {
        path: worktree_path,
        branch,
        repo_path: repo_path.to_string(),
    })
}

/// Remove a git worktree by its path.
pub fn remove_worktree(worktree_path: &str) -> Result<(), WorktreeError> {
    // Find the repo root from the worktree
    let output = Command::new("git")
        .args(["-C", worktree_path, "rev-parse", "--git-common-dir"])
        .output()?;

    if !output.status.success() {
        return Err(WorktreeError::GitError("Failed to find git common dir".to_string()));
    }

    let common_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // The common dir is the .git directory of the main repo
    let repo_root = Path::new(&common_dir)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| common_dir.clone());

    debug!(worktree = %worktree_path, repo = %repo_root, "Removing git worktree");

    let output = Command::new("git")
        .args(["-C", &repo_root, "worktree", "remove", worktree_path, "--force"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(WorktreeError::GitError(stderr.to_string()));
    }

    // Also delete the branch
    let branch_name = Path::new(worktree_path)
        .file_name()
        .map(|f| f.to_string_lossy().to_string());

    if let Some(branch) = branch_name {
        let _ = Command::new("git")
            .args(["-C", &repo_root, "branch", "-D", &branch])
            .output();
    }

    info!(worktree = %worktree_path, "Git worktree removed");
    Ok(())
}
