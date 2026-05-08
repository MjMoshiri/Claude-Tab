use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use thiserror::Error;

pub const MAX_JUDGE_PROMPT_BYTES: usize = 24 * 1024;
pub const MAX_WORKFLOW_FILE_BYTES: u64 = 256 * 1024;
pub const MAX_ADVANCES_PER_HOUR: usize = 20;
const WINDOW: Duration = Duration::from_secs(3600);

#[derive(Debug, Error, PartialEq)]
pub enum GuardrailError {
    #[error("loop break: run `{0}` exceeded {1} advances/hour")]
    LoopBreak(String, usize),
}

#[derive(Default)]
pub struct AdvanceCounter {
    inner: Mutex<HashMap<String, Vec<Instant>>>,
}

impl AdvanceCounter {
    pub fn new() -> Self { Self::default() }

    pub fn record_advance(&self, run_id: &str) -> Result<(), GuardrailError> {
        let mut map = self.inner.lock().unwrap();
        let now = Instant::now();
        let entry = map.entry(run_id.to_string()).or_default();
        entry.retain(|t| now.duration_since(*t) < WINDOW);
        if entry.len() >= MAX_ADVANCES_PER_HOUR {
            return Err(GuardrailError::LoopBreak(run_id.to_string(), MAX_ADVANCES_PER_HOUR));
        }
        entry.push(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_max() {
        let c = AdvanceCounter::new();
        for _ in 0..MAX_ADVANCES_PER_HOUR {
            c.record_advance("r1").unwrap();
        }
    }

    #[test]
    fn blocks_after_max() {
        let c = AdvanceCounter::new();
        for _ in 0..MAX_ADVANCES_PER_HOUR {
            c.record_advance("r1").unwrap();
        }
        let err = c.record_advance("r1").unwrap_err();
        assert!(matches!(err, GuardrailError::LoopBreak(_, _)));
    }

    #[test]
    fn counters_isolated_per_run() {
        let c = AdvanceCounter::new();
        for _ in 0..MAX_ADVANCES_PER_HOUR {
            c.record_advance("r1").unwrap();
        }
        c.record_advance("r2").unwrap();
    }
}
