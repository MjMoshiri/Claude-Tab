use super::{JudgeError, JudgeRunner};
use async_trait::async_trait;
use tokio::process::Command;

pub struct ClaudeCli;

#[async_trait]
impl JudgeRunner for ClaudeCli {
    async fn run(&self, prompt: &str, model: &str) -> Result<String, JudgeError> {
        let model_arg = match model {
            "claude-haiku-4-5" | "haiku" => "haiku",
            "claude-sonnet-4-6" | "sonnet" => "sonnet",
            "claude-opus-4-7" | "opus" => "opus",
            other => other,
        };
        let output = Command::new("claude")
            .arg("-p")
            .arg("--model")
            .arg(model_arg)
            .arg(prompt)
            .output()
            .await
            .map_err(|e| JudgeError::Subprocess(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(JudgeError::Subprocess(stderr));
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}
