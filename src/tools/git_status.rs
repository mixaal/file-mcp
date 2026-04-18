use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::constants::GIT_BIN;
use crate::state::AppState;
use crate::tools::{text_err, text_ok, ToolResult};
use crate::util::run_git;

pub async fn run(state: Arc<Mutex<AppState>>) -> ToolResult {
    let project_dir = {
        let st = state.lock().await;
        match st.project_dir.clone() {
            Some(d) => d,
            None => {
                return Ok(text_err(
                    "404: no active project — call create_project or use_project first.",
                ))
            }
        }
    };

    match run_git(Path::new(GIT_BIN), &project_dir, &["status"]).await {
        Ok(out) => Ok(text_ok(out)),
        Err(e) => Ok(text_err(format!("git status failed: {e}"))),
    }
}
