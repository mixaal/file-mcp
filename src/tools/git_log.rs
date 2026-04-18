use serde_json::Value;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::constants::{GIT_BIN, GIT_LOG_DEFAULT_N, GIT_LOG_MAX_N};
use crate::state::AppState;
use crate::tools::{text_err, text_ok, ToolResult};
use crate::util::run_git;

pub async fn run(state: Arc<Mutex<AppState>>, args: &Value) -> ToolResult {
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

    let n: usize = args
        .get("n")
        .and_then(|v| v.as_u64())
        .unwrap_or(GIT_LOG_DEFAULT_N)
        .min(GIT_LOG_MAX_N) as usize;

    let n_str = n.to_string();
    match run_git(
        Path::new(GIT_BIN),
        &project_dir,
        &[
            "log",
            "--oneline",
            "--decorate",
            "--graph",
            "-n",
            &n_str,
        ],
    )
    .await
    {
        Ok(out) if out.is_empty() => Ok(text_ok("No commits yet.")),
        Ok(out) => Ok(text_ok(out)),
        Err(e) => Ok(text_err(format!("git log failed: {e}"))),
    }
}
