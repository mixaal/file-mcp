use std::process::Stdio;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::constants::SHELL_BIN;
use crate::state::{AppState, BuildJob};
use crate::tools::{text_err, text_ok, ToolResult};

pub async fn run(state: Arc<Mutex<AppState>>) -> ToolResult {
    let (project_dir, script) = {
        let st = state.lock().await;
        let project_dir = match st.project_dir.clone() {
            Some(d) => d,
            None => {
                return Ok(text_err(
                    "404: no active project — call create_project or use_project first.",
                ))
            }
        };

        let already_running = st.jobs.values().any(|j| matches!(j, BuildJob::Running { .. }));
        if already_running {
            return Ok(text_err(
                "409: a build is already running — poll with build_status.",
            ));
        }

        let script = project_dir.join("build.sh");
        (project_dir, script)
    };

    if !script.is_file() {
        return Ok(text_err("404: build.sh not found in project root."));
    }

    let child = tokio::process::Command::new(SHELL_BIN)
        .arg(&script)
        .current_dir(&project_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| (-32603i32, format!("Failed to spawn build.sh: {e}")))?;

    let pid = child.id().unwrap_or(0);
    let job_id = Uuid::new_v4().to_string();

    {
        let mut st = state.lock().await;
        st.jobs.insert(job_id.clone(), BuildJob::Running { pid });
    }

    let state_bg = state.clone();
    let job_id_bg = job_id.clone();
    tokio::spawn(async move {
        let job = match child.wait_with_output().await {
            Ok(out) => BuildJob::Done {
                exit_code: out.status.code().unwrap_or(-1),
                stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            },
            Err(e) => BuildJob::Done {
                exit_code: -1,
                stdout: String::new(),
                stderr: format!("Failed to wait for build.sh: {e}"),
            },
        };

        let mut st = state_bg.lock().await;
        st.jobs.insert(job_id_bg, job);
    });

    Ok(text_ok(format!("Build started. job_id: {job_id}")))
}
