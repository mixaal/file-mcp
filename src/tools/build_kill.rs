use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::state::{AppState, BuildJob};
use crate::tools::{ToolResult, text_err, text_ok};

pub async fn run(state: Arc<Mutex<AppState>>, args: &Value) -> ToolResult {
    let job_id = args
        .get("job_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (-32602i32, "Missing required argument: job_id".to_string()))?
        .to_string();

    let mut st = state.lock().await;

    match st.jobs.get_mut(&job_id) {
        None => Ok(text_err(format!("404: unknown job_id '{job_id}'."))),
        Some(BuildJob::Done { .. }) => {
            Ok(text_ok("Build has already finished — nothing to kill."))
        }
        Some(BuildJob::Running { kill_tx }) => match kill_tx.take() {
            Some(tx) => {
                // Receiver lives inside the job's background task; the only way
                // send() can fail is if that task has already dropped it, which
                // means the process is already exiting.
                let _ = tx.send(());
                Ok(text_ok(
                    "SIGTERM requested. Poll build_status for the final result.",
                ))
            }
            None => Ok(text_ok(
                "Kill already requested — still waiting for the process to exit.",
            )),
        },
    }
}
