use rusqlite::Connection;
use tracing::info;

pub fn run_migrations(conn: &Connection) -> Result<(), rusqlite::Error> {
    // Drop old tables to rebuild from scratch
    conn.execute_batch(
        "
        DROP TABLE IF EXISTS session_metadata;
        DROP TABLE IF EXISTS directory_preferences;
        ",
    )?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS directory_preferences (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_path TEXT NOT NULL UNIQUE,
            pinned INTEGER DEFAULT 0,
            hidden INTEGER DEFAULT 0,
            display_name TEXT
        );

        CREATE TABLE IF NOT EXISTS session_metadata (
            claude_session_id TEXT PRIMARY KEY,
            project_path TEXT NOT NULL DEFAULT '',
            custom_title TEXT,
            user_set_title INTEGER DEFAULT 0,
            generated_title TEXT,
            hidden INTEGER DEFAULT 0,
            previous_session_id TEXT,
            last_known_state TEXT,
            last_state_change_at TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE INDEX IF NOT EXISTS idx_session_metadata_updated ON session_metadata(updated_at DESC);
        CREATE INDEX IF NOT EXISTS idx_session_metadata_project ON session_metadata(project_path);
        CREATE INDEX IF NOT EXISTS idx_session_previous ON session_metadata(previous_session_id);
        ",
    )?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS workflows (
          id            TEXT PRIMARY KEY,
          name          TEXT NOT NULL,
          description   TEXT,
          source_path   TEXT NOT NULL,
          source_hash   TEXT NOT NULL,
          compiled_json TEXT NOT NULL,
          updated_at    INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS workflow_runs (
          run_id            TEXT PRIMARY KEY,
          session_id        TEXT NOT NULL UNIQUE,
          workflow_id       TEXT NOT NULL REFERENCES workflows(id),
          inputs_json       TEXT NOT NULL,
          current_stage_id  TEXT NOT NULL,
          status            TEXT NOT NULL,
          started_at        INTEGER NOT NULL,
          ended_at          INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_runs_session ON workflow_runs(session_id);

        CREATE TABLE IF NOT EXISTS stage_history (
          id          INTEGER PRIMARY KEY AUTOINCREMENT,
          run_id      TEXT NOT NULL REFERENCES workflow_runs(run_id),
          stage_id    TEXT NOT NULL,
          entered_at  INTEGER NOT NULL,
          exited_at   INTEGER,
          verdict     TEXT,
          judge_log   TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_history_run ON stage_history(run_id);
        ",
    )?;

    info!("Storage schema initialized");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn migration_creates_orchestrator_tables() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        assert!(tables.contains(&"workflows".to_string()));
        assert!(tables.contains(&"workflow_runs".to_string()));
        assert!(tables.contains(&"stage_history".to_string()));
    }

    #[test]
    fn migration_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap();
    }
}
