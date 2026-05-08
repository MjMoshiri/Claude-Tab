use super::auth::LocalAuth;
use super::handlers::AppState;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware::{self, Next},
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;

pub async fn serve(state: AppState, auth: Arc<LocalAuth>) -> std::io::Result<()> {
    let auth_for_mw = auth.clone();
    let app = Router::new()
        .route("/turn-end", post(super::handlers::turn_end))
        .route("/session-start", post(super::handlers::session_start))
        .route("/workflow/attach", post(super::handlers::attach))
        .route("/workflow/state/:sid", get(super::handlers::state))
        .route("/workflow/control/:sid", post(super::handlers::control))
        .layer(middleware::from_fn(move |req: Request<Body>, next: Next| {
            let auth = auth_for_mw.clone();
            async move {
                let provided = req
                    .headers()
                    .get("X-CT-Token")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                if !auth.verify(provided) {
                    return Err(StatusCode::UNAUTHORIZED);
                }
                Ok::<_, StatusCode>(next.run(req).await)
            }
        }))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
    let port = listener.local_addr()?.port();
    auth.write_port(port)?;
    tracing::info!(port, "orchestrator HTTP listener up");
    axum::serve(listener, app).await
}
