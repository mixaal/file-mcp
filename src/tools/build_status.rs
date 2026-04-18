use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::state::{AppState, BuildJob};
use crate::tools::{text_err, text_ok, ToolResult};

pub async fn run(state: Arc<Mutex<AppState>>, args: &Value) -> ToolResult {
    let job_id = args
        .get("job_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (-32602i32, "Missing required argument: job_id".to_string()))?
        .to_string();

    let mut st = state.lock().await;

    match st.jobs.remove(&job_id) {
        None => Ok(text_err(format!("404: unknown job_id '{job_id}'."))),

        Some(BuildJob::Running) => {
            // Put it back — not done yet.
            st.jobs.insert(job_id, BuildJob::Running);
            Ok(text_ok("Build is still running..."))
        }

        Some(BuildJob::Done { exit_code, stdout, stderr }) => {
            // Job consumed — caller has the result, no need to keep it.
            if exit_code == 0 {
                let msg = if stdout.trim().is_empty() {
                    "Build succeeded.".to_string()
                } else {
                    format!("Build succeeded.\n{stdout}")
                };
                Ok(text_ok(msg))
            } else {
                let mut msg = format!("Build failed (exit code {exit_code}).");
                if !stdout.trim().is_empty() {
                    msg.push_str(&format!("\n\nstdout:\n{stdout}"));
                }
                if !stderr.trim().is_empty() {
                    msg.push_str(&format!("\n\nstderr:\n{stderr}"));
                }
                Ok(text_err(msg))
            }
        }
    }
}
