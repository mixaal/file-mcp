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

    let st = state.lock().await;

    let pid = match st.jobs.get(&job_id) {
        None => return Ok(text_err(format!("404: unknown job_id '{job_id}'."))),
        Some(BuildJob::Done { .. }) => {
            return Ok(text_ok("Build has already finished — nothing to kill."))
        }
        Some(BuildJob::Running { pid }) => *pid,
    };

    if pid == 0 {
        return Ok(text_err("Cannot kill: process PID is unavailable."));
    }

    // SIGTERM — gives the shell script a chance to clean up.
    let rc = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
    if rc != 0 {
        let errno = std::io::Error::last_os_error();
        return Ok(text_err(format!(
            "Failed to send SIGTERM to PID {pid}: {errno}"
        )));
    }

    Ok(text_ok(format!(
        "SIGTERM sent to PID {pid}. Poll build_status for the final result."
    )))
}
