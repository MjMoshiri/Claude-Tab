pub mod claude_cli;
pub mod completion;
pub mod notes;

use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum JudgeError {
    #[error("subprocess failed: {0}")]
    Subprocess(String),
    #[error("malformed response: {0}")]
    Malformed(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[async_trait]
pub trait JudgeRunner: Send + Sync {
    async fn run(&self, prompt: &str, model: &str) -> Result<String, JudgeError>;
}
