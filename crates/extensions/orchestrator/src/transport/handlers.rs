use crate::dispatcher::Dispatcher;
use axum::{extract::{Path, State}, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

pub type AppState = Arc<Dispatcher>;

#[derive(Deserialize)]
pub struct TurnEnd {
    pub session_id: String,
    pub transcript_path: String,
}

pub async fn turn_end(State(d): State<AppState>, Json(p): Json<TurnEnd>) -> impl IntoResponse {
    if let Err(e) = d.on_turn_end(&p.session_id, &PathBuf::from(p.transcript_path)).await {
        tracing::warn!("turn_end error: {e}");
    }
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
pub struct SessionStart {
    pub session_id: String,
    pub source: String,
}

pub async fn session_start(State(d): State<AppState>, Json(p): Json<SessionStart>) -> impl IntoResponse {
    if let Err(e) = d.on_session_start(&p.session_id, &p.source).await {
        tracing::warn!("session_start error: {e}");
    }
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
pub struct AttachReq {
    pub session_id: String,
    pub workflow_id: String,
    pub inputs: serde_json::Value,
}

#[derive(Serialize)]
pub struct AttachResp {
    pub run_id: String,
    pub current_stage_id: String,
}

pub async fn attach(
    State(d): State<AppState>,
    Json(p): Json<AttachReq>,
) -> Result<Json<AttachResp>, (StatusCode, String)> {
    use crate::runtime::run::{RunStatus, WorkflowRun};
    use std::collections::BTreeMap;

    let workflow = d
        .registry
        .get(&p.workflow_id)
        .ok_or((StatusCode::NOT_FOUND, format!("unknown workflow {}", p.workflow_id)))?;
    let stage_one = workflow
        .stage_order
        .first()
        .ok_or((StatusCode::BAD_REQUEST, "workflow has no stages".into()))?
        .clone();

    let inputs: BTreeMap<String, serde_json::Value> = serde_json::from_value(p.inputs)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("bad inputs: {e}")))?;

    let run = WorkflowRun {
        run_id: uuid::Uuid::new_v4().to_string(),
        session_id: p.session_id.clone(),
        workflow_id: p.workflow_id.clone(),
        inputs: inputs.clone(),
        current_stage_id: stage_one.clone(),
        status: RunStatus::Running,
        started_at: chrono::Utc::now().timestamp_millis(),
        ended_at: None,
    };

    d.store.create_run(&run).map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
    d.store.record_history(&run.run_id, &stage_one, "entered", None).ok();

    let stage = workflow.stages.get(&stage_one).expect("validated");
    let rendered = crate::runtime::render::render(stage, &inputs, None)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    d.injector
        .write_user_prompt(&p.session_id, &rendered)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(AttachResp {
        run_id: run.run_id,
        current_stage_id: stage_one,
    }))
}

pub async fn state(
    State(d): State<AppState>,
    Path(sid): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let run = d.store.load_by_session(&sid).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    match run {
        Some(r) => Ok(Json(serde_json::json!({"run": r}))),
        None => Err(StatusCode::NOT_FOUND),
    }
}

#[derive(Deserialize)]
pub struct ControlReq {
    pub action: String,
}

pub async fn control(
    State(d): State<AppState>,
    Path(sid): Path<String>,
    Json(p): Json<ControlReq>,
) -> Result<StatusCode, (StatusCode, String)> {
    use crate::runtime::run::RunStatus;
    let run = d
        .store
        .load_by_session(&sid)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "no run".into()))?;
    match p.action.as_str() {
        "pause" => d
            .store
            .set_status(&run.run_id, RunStatus::Paused)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
        "resume" => d
            .store
            .set_status(&run.run_id, RunStatus::Running)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
        "cancel" => d
            .store
            .set_status(&run.run_id, RunStatus::Paused)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
        "skip" => {
            let workflow = d
                .registry
                .get(&run.workflow_id)
                .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "missing workflow".into()))?;
            match crate::runtime::run::advance(&run, &workflow, crate::runtime::run::Verdict::Complete) {
                crate::runtime::run::AdvanceOutcome::Inject { stage_id } => {
                    d.store
                        .advance_run(&run.run_id, &stage_id)
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
                    let next = workflow.stages.get(&stage_id).expect("validated");
                    let rendered = crate::runtime::render::render(next, &run.inputs, None)
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
                    d.injector
                        .write_user_prompt(&sid, &rendered)
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
                }
                crate::runtime::run::AdvanceOutcome::Done => d
                    .store
                    .set_status(&run.run_id, RunStatus::Done)
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
                _ => {}
            }
        }
        other => return Err((StatusCode::BAD_REQUEST, format!("unknown action {other}"))),
    }
    Ok(StatusCode::NO_CONTENT)
}
