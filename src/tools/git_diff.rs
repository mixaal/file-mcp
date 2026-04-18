use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::constants::GIT_REF_MAX_LEN;
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

    let git_ref = args
        .get("ref")
        .and_then(|v| v.as_str())
        .unwrap_or("HEAD")
        .to_string();

    // Allowlist: only characters valid in git refs.
    let valid = !git_ref.is_empty()
        && git_ref.len() <= GIT_REF_MAX_LEN
        && git_ref
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '~' | '^'));
    if !valid {
        return Ok(text_err(
            "400: invalid ref — only a-zA-Z0-9 and - _ . / ~ ^ are allowed, max 200 chars.",
        ));
    }

    match run_git(&git_cmd, &project_dir, &["diff", &git_ref]).await {
        Ok(out) if out.is_empty() => Ok(text_ok(format!("No differences against '{git_ref}'."))),
        Ok(out) => Ok(text_ok(out)),
        Err(e) => Ok(text_err(format!("git diff failed: {e}"))),
    }
}
