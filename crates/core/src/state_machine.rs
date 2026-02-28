use crate::session::SessionStore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Active,
    Running,
    YourTurn,
    Paused,
    Idle,
    Completed,
}

impl SessionState {
    pub fn display_name(&self) -> &str {
        match self {
            SessionState::Active => "Active",
            SessionState::Running => "Running",
            SessionState::YourTurn => "Your Turn",
            SessionState::Paused => "Paused",
            SessionState::Idle => "Idle",
            SessionState::Completed => "Completed",
        }
    }

    pub fn color(&self) -> &str {
        match self {
            SessionState::Active => "#4caf50",
            SessionState::Running => "#2196f3",
            SessionState::YourTurn => "#ff9800",
            SessionState::Paused => "#ff5722",
            SessionState::Idle => "#808080",
            SessionState::Completed => "#FFD60A",
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            SessionState::Active => "active",
            SessionState::Running => "running",
            SessionState::YourTurn => "your_turn",
            SessionState::Paused => "paused",
            SessionState::Idle => "idle",
            SessionState::Completed => "completed",
        }
    }
}

impl std::fmt::Display for SessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transition {
    pub session_id: String,
    pub from: SessionState,
    pub to: SessionState,
    pub trigger: String,
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Error type for state transition failures.
#[derive(Debug, thiserror::Error)]
pub enum TransitionError {
    #[error("Session not found: {0}")]
    SessionNotFound(String),
    #[error("Transition from '{from}' to '{to}' is not allowed (trigger: {trigger})")]
    InvalidTransition {
        from: SessionState,
        to: SessionState,
        trigger: String,
    },
    #[error("No state change needed (already in '{0}')")]
    NoChange(SessionState),
}

pub struct StateMachine {
    session_store: Arc<SessionStore>,
}

impl StateMachine {
    pub fn new(session_store: Arc<SessionStore>) -> Self {
        Self { session_store }
    }

    /// Check whether a transition from one state to another is valid.
    pub fn is_valid_transition(from: SessionState, to: SessionState) -> bool {
        use SessionState::*;
        matches!(
            (from, to),
            // Active -> Running (user submits prompt)
            (Active, Running)
            // Running -> YourTurn (permission/elicitation)
            | (Running, YourTurn)
            // Running -> Paused (interrupt)
            | (Running, Paused)
            // Running -> Completed (Stop hook — Claude finished its turn)
            | (Running, Completed)
            // Running -> Active (stale recovery — no hooks for 5 min)
            | (Running, Active)
            // Completed -> Running (user submits prompt)
            | (Completed, Running)
            // Completed -> Active (user visits tab / SessionStart)
            | (Completed, Active)
            // YourTurn -> Running (user submits prompt)
            | (YourTurn, Running)
            // Paused -> Running (user submits prompt)
            | (Paused, Running)
            // Any -> Idle (timeout)
            | (Active, Idle)
            | (Running, Idle)
            | (YourTurn, Idle)
            | (Paused, Idle)
            | (Completed, Idle)
            // Idle -> Active (visit/focus)
            | (Idle, Active)
            // Any -> Active (SessionStart)
            | (YourTurn, Active)
            | (Paused, Active)
        )
    }

    /// Transition a session's state.
    ///
    /// 1. Looks up the session's current state
    /// 2. Validates the transition
    /// 3. Updates the session state in the store
    /// 4. Returns the completed `Transition` or a `TransitionError`
    pub async fn transition_session(
        &self,
        session_id: &str,
        to: SessionState,
        trigger: &str,
    ) -> Result<Transition, TransitionError> {
        let session = self
            .session_store
            .get(session_id)
            .await
            .ok_or_else(|| TransitionError::SessionNotFound(session_id.to_string()))?;

        let from = session.state;

        if from == to {
            return Err(TransitionError::NoChange(to));
        }

        if !Self::is_valid_transition(from, to) {
            return Err(TransitionError::InvalidTransition {
                from,
                to,
                trigger: trigger.to_string(),
            });
        }

        self.session_store.update_state(session_id, to).await;

        let transition = Transition {
            session_id: session_id.to_string(),
            from,
            to,
            trigger: trigger.to_string(),
            metadata: HashMap::new(),
        };

        debug!(
            session_id = %session_id,
            from = %from,
            to = %to,
            trigger = %trigger,
            "State transition completed"
        );

        Ok(transition)
    }

    /// Returns a reference to the internal session store.
    pub fn session_store(&self) -> &Arc<SessionStore> {
        &self.session_store
    }
}
