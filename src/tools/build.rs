use std::sync::Arc;
use tokio::sync::Mutex;

use crate::constants::SHELL_BIN;
use crate::state::AppState;
use crate::tools::{text_err, text_ok, ToolResult};

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

    let script = project_dir.join("build.sh");
    if !script.is_file() {
        return Ok(text_err("404: build.sh not found in project root."));
    }

    let output = tokio::process::Command::new(SHELL_BIN)
        .arg(&script)
        .current_dir(&project_dir)
        .output()
        .await
        .map_err(|e| (-32603i32, format!("Failed to spawn build.sh: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.success() {
        let msg = if stdout.trim().is_empty() {
            "Build succeeded.".to_string()
        } else {
            format!("Build succeeded.\n{stdout}")
        };
        return Ok(text_ok(msg));
    }

    let code = output.status.code().unwrap_or(-1);
    let mut msg = format!("Build failed (exit code {code}).");
    if !stdout.trim().is_empty() {
        msg.push_str(&format!("\n\nstdout:\n{stdout}"));
    }
    if !stderr.trim().is_empty() {
        msg.push_str(&format!("\n\nstderr:\n{stderr}"));
    }
    Ok(text_err(msg))
}
