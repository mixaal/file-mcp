use std::sync::Arc;
use tokio::sync::Mutex;

use crate::state::AppState;
use crate::tools::{text_err, text_ok, ToolResult};

pub async fn run(state: Arc<Mutex<AppState>>) -> ToolResult {
    let st = state.lock().await;
    match &st.project_name {
        Some(name) => Ok(text_ok(name.clone())),
        None => Ok(text_err("no active project — call create_project or use_project first.")),
    }
}
