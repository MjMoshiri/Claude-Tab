use crate::runtime::run::{RunStatus, WorkflowRun};
use chrono::Utc;
use rusqlite::{params, Connection};
use std::collections::BTreeMap;
use std::sync::Mutex;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("session already has a workflow run: {0}")]
    SessionAlreadyAttached(String),
    #[error("run not found for session {0}")]
    NotFound(String),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub struct RunStore {
    conn: Mutex<Connection>,
}

impl RunStore {
    pub fn new(conn: Connection) -> Self {
        Self { conn: Mutex::new(conn) }
    }

    pub fn create_run(&self, run: &WorkflowRun) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        let inputs_json = serde_json::to_string(&run.inputs)?;
        let result = conn.execute(
            "INSERT INTO workflow_runs
              (run_id, session_id, workflow_id, inputs_json, current_stage_id, status, started_at, ended_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                run.run_id, run.session_id, run.workflow_id,
                inputs_json, run.current_stage_id,
                run_status_to_str(run.status), run.started_at, run.ended_at,
            ],
        );
        match result {
            Ok(_) => Ok(()),
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Err(StoreError::SessionAlreadyAttached(run.session_id.clone()))
            }
            Err(e) => Err(e.into()),
        }
    }

    pub fn load_by_session(&self, session_id: &str) -> Result<Option<WorkflowRun>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT run_id, session_id, workflow_id, inputs_json, current_stage_id,
                    status, started_at, ended_at
             FROM workflow_runs WHERE session_id = ?",
        )?;
        let row = stmt
            .query_row(params![session_id], |row| {
                let inputs_json: String = row.get(3)?;
                let inputs: BTreeMap<String, serde_json::Value> =
                    serde_json::from_str(&inputs_json).unwrap_or_default();
                Ok(WorkflowRun {
                    run_id: row.get(0)?,
                    session_id: row.get(1)?,
                    workflow_id: row.get(2)?,
                    inputs,
                    current_stage_id: row.get(4)?,
                    status: str_to_run_status(&row.get::<_, String>(5)?),
                    started_at: row.get(6)?,
                    ended_at: row.get(7)?,
                })
            });
        match row {
            Ok(r) => Ok(Some(r)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn advance_run(&self, run_id: &str, new_stage_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE workflow_runs SET current_stage_id = ? WHERE run_id = ?",
            params![new_stage_id, run_id],
        )?;
        Ok(())
    }

    pub fn set_status(&self, run_id: &str, status: RunStatus) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        let ended_at = if matches!(status, RunStatus::Done | RunStatus::Error | RunStatus::Escalated) {
            Some(Utc::now().timestamp_millis())
        } else {
            None
        };
        conn.execute(
            "UPDATE workflow_runs SET status = ?, ended_at = COALESCE(?, ended_at) WHERE run_id = ?",
            params![run_status_to_str(status), ended_at, run_id],
        )?;
        Ok(())
    }

    pub fn record_history(
        &self,
        run_id: &str,
        stage_id: &str,
        verdict: &str,
        judge_log: Option<&str>,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp_millis();
        conn.execute(
            "INSERT INTO stage_history (run_id, stage_id, entered_at, verdict, judge_log)
             VALUES (?, ?, ?, ?, ?)",
            params![run_id, stage_id, now, verdict, judge_log],
        )?;
        Ok(())
    }
}

fn run_status_to_str(s: RunStatus) -> &'static str {
    match s {
        RunStatus::Running => "running",
        RunStatus::Paused => "paused",
        RunStatus::Done => "done",
        RunStatus::Escalated => "escalated",
        RunStatus::Error => "error",
    }
}

fn str_to_run_status(s: &str) -> RunStatus {
    match s {
        "paused" => RunStatus::Paused,
        "done" => RunStatus::Done,
        "escalated" => RunStatus::Escalated,
        "error" => RunStatus::Error,
        _ => RunStatus::Running,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claude_tabs_storage::migrations::run_migrations;
    use rusqlite::Connection;

    fn store() -> RunStore {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        // workflows row needs to exist for FK
        conn.execute(
            "INSERT INTO workflows (id, name, source_path, source_hash, compiled_json, updated_at)
             VALUES ('w1', 'W', '/tmp/w1.md', 'abc', '{}', 0)",
            [],
        ).unwrap();
        RunStore::new(conn)
    }

    fn make_run(session: &str) -> WorkflowRun {
        WorkflowRun {
            run_id: format!("run-{session}"),
            session_id: session.into(),
            workflow_id: "w1".into(),
            inputs: BTreeMap::new(),
            current_stage_id: "a".into(),
            status: RunStatus::Running,
            started_at: 1,
            ended_at: None,
        }
    }

    #[test]
    fn create_and_load_round_trip() {
        let s = store();
        let run = make_run("s1");
        s.create_run(&run).unwrap();
        let back = s.load_by_session("s1").unwrap().unwrap();
        assert_eq!(back.current_stage_id, "a");
        assert_eq!(back.status, RunStatus::Running);
    }

    #[test]
    fn duplicate_session_rejected() {
        let s = store();
        let run = make_run("s1");
        s.create_run(&run).unwrap();
        let err = s.create_run(&run).unwrap_err();
        assert!(matches!(err, StoreError::SessionAlreadyAttached(_)));
    }

    #[test]
    fn advance_updates_stage() {
        let s = store();
        let run = make_run("s1");
        s.create_run(&run).unwrap();
        s.advance_run(&run.run_id, "b").unwrap();
        let back = s.load_by_session("s1").unwrap().unwrap();
        assert_eq!(back.current_stage_id, "b");
    }
}
