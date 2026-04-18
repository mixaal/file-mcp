use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::state::AppState;
use crate::tools::{text_err, text_ok, ToolResult};
use crate::util::run_git;

pub async fn run(state: Arc<Mutex<AppState>>) -> ToolResult {
    let (project_dir, git_cmd) = {
        let st = state.lock().await;
        match st.project_dir.clone() {
            Some(d) => (d, st.git_cmd.clone()),
            None => {
                return Ok(text_err(
                    "404: no active project — call create_project or use_project first.",
                ))
            }
        }
    };

    match run_git(&git_cmd, &project_dir, &["status"]).await {
        Ok(out) => Ok(text_ok(out)),
        Err(e) => Ok(text_err(format!("git status failed: {e}"))),
    }
}
