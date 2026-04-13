use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::Value;
use std::sync::Arc;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::Mutex,
};

mod mcp;
mod state;
mod tools;
mod util;

use state::AppState;

type SharedState = Arc<Mutex<AppState>>;

#[tokio::main]
async fn main() {
    let app_state: SharedState = Arc::new(Mutex::new(AppState::new()));

    if std::env::args().any(|a| a == "--stdio") {
        run_stdio(app_state).await;
    } else {
        run_http(app_state).await;
    }
}

async fn run_http(app_state: SharedState) {
    let app = Router::new()
        .route("/mcp", post(mcp_handler))
        .with_state(app_state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("0.0.0.0:{port}");

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("Cannot bind to {addr}: {e}"));

    eprintln!("file-mcp listening on {addr}");
    axum::serve(listener, app).await.expect("server error");
}

async fn run_stdio(state: SharedState) {
    let mut reader = BufReader::new(tokio::io::stdin());
    let mut stdout = tokio::io::stdout();
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break, // EOF
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let parsed: Value = match serde_json::from_str(trimmed) {
                    Ok(v) => v,
                    Err(e) => {
                        let resp = serde_json::json!({
                            "jsonrpc": "2.0",
                            "error": {"code": -32700, "message": format!("Parse error: {e}")},
                            "id": null
                        });
                        write_line(&mut stdout, &resp).await;
                        continue;
                    }
                };

                if let Some(response) = mcp::handle_request(state.clone(), parsed).await {
                    write_line(&mut stdout, &response).await;
                }
                // notifications (None) get no response
            }
            Err(e) => {
                eprintln!("stdin read error: {e}");
                break;
            }
        }
    }
}

async fn write_line(stdout: &mut tokio::io::Stdout, value: &Value) {
    let mut out = serde_json::to_string(value).unwrap();
    out.push('\n');
    let _ = stdout.write_all(out.as_bytes()).await;
    let _ = stdout.flush().await;
}

async fn mcp_handler(State(state): State<SharedState>, body: String) -> Response {
    let parsed: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            let resp = serde_json::json!({
                "jsonrpc": "2.0",
                "error": {"code": -32700, "message": format!("Parse error: {e}")},
                "id": null
            });
            return (StatusCode::OK, Json(resp)).into_response();
        }
    };

    match mcp::handle_request(state, parsed).await {
        Some(response) => (StatusCode::OK, Json(response)).into_response(),
        // Pure notification (no id) — acknowledge with 202, no body.
        None => StatusCode::ACCEPTED.into_response(),
    }
}
