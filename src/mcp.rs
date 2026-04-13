use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::state::AppState;
use crate::tools;

/// Entry point: accepts a parsed JSON body (single request or batch array)
/// and returns the response value, or `None` for pure-notification inputs.
pub async fn handle_request(state: Arc<Mutex<AppState>>, body: Value) -> Option<Value> {
    if let Some(batch) = body.as_array() {
        let mut responses = Vec::new();
        for item in batch {
            if let Some(resp) = dispatch(state.clone(), item.clone()).await {
                responses.push(resp);
            }
        }
        return if responses.is_empty() {
            None
        } else {
            Some(Value::Array(responses))
        };
    }
    dispatch(state, body).await
}

async fn dispatch(state: Arc<Mutex<AppState>>, req: Value) -> Option<Value> {
    let id = req.get("id").cloned();
    let is_notification = id.is_none();
    let id = id.unwrap_or(Value::Null);

    let method = match req.get("method").and_then(|m| m.as_str()) {
        Some(m) => m.to_string(),
        None => {
            if is_notification {
                return None;
            }
            return Some(err_response(id, -32600, "Invalid Request: missing method"));
        }
    };

    let params = req.get("params").cloned().unwrap_or(Value::Null);

    // Notifications are handled but produce no response.
    if is_notification {
        // Nothing to act on for now; just absorb.
        return None;
    }

    let result: Result<Value, (i32, String)> = match method.as_str() {
        "initialize" => Ok(initialize(&params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tools::list()),
        "tools/call" => tools::call(state, &params).await,
        other => Err((-32601, format!("Method not found: {other}"))),
    };

    Some(match result {
        Ok(v) => json!({"jsonrpc": "2.0", "result": v, "id": id}),
        Err((code, msg)) => err_response(id, code, &msg),
    })
}

fn err_response(id: Value, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "error": {"code": code, "message": message},
        "id": id
    })
}

fn initialize(_params: &Value) -> Value {
    json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": "file-mcp",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}
