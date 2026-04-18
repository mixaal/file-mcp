use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::state::AppState;
use crate::tools::{text_err, text_ok, ToolResult};
use crate::util::run_git;

pub async fn run(state: Arc<Mutex<AppState>>, args: &Value) -> ToolResult {
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

    let n: usize = args
        .get("n")
        .and_then(|v| v.as_u64())
        .unwrap_or(10)
        .min(200) as usize;

    let n_str = n.to_string();
    match run_git(
        &git_cmd,
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
