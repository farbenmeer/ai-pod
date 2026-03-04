pub mod lifecycle;
pub mod notify;

use axum::{Router, extract::Query, routing::get, routing::post};
use serde::Deserialize;
use std::net::SocketAddr;

async fn health_handler() -> &'static str {
    "ok"
}

#[derive(Deserialize)]
struct NotifyParams {
    project: Option<String>,
}

async fn notify_handler(Query(params): Query<NotifyParams>) -> &'static str {
    let title = params.project.as_deref().unwrap_or("Claude Code");
    notify::send_notification(title, "ai-pod: task complete");
    "ok"
}

pub async fn run_server(port: u16) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/notify", post(notify_handler));

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("Notification server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
