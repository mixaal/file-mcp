use std::process::Stdio;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::sync::{Mutex, oneshot};
use uuid::Uuid;

use crate::constants::SHELL_BIN;
use crate::state::{AppState, BuildJob};
use crate::tools::{ToolResult, text_err, text_ok};

pub async fn run(state: Arc<Mutex<AppState>>) -> ToolResult {
    let (project_dir, script) = {
        let st = state.lock().await;
        let project_dir = match st.project_dir.clone() {
            Some(d) => d,
            None => {
                return Ok(text_err(
                    "404: no active project — call create_project or use_project first.",
                ));
            }
        };

        let already_running = st
            .jobs
            .values()
            .any(|j| matches!(j, BuildJob::Running { .. }));
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

    // BIG SECURITY NOTE: we are intentionally allowing arbitrary code execution here, since the whole point of this tool is to run the user's build.sh.
    // DO NOT add any features that would allow an attacker to write files into the project directory or otherwise modify the build.sh script,
    // without proper authentication and authorization in place. If you need to add such features, add them in a way that does not run the risk
    // of unauthorized code execution (e.g. by having a separate authenticated endpoint that can only write to a safe location, and having build.sh read from there).
    let mut child = tokio::process::Command::new(SHELL_BIN)
        .arg(&script)
        .current_dir(&project_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| (-32603i32, format!("Failed to spawn build.sh: {e}")))?;

    let (kill_tx, kill_rx) = oneshot::channel::<()>();
    let job_id = Uuid::new_v4().to_string();

    {
        let mut st = state.lock().await;
        st.jobs.insert(
            job_id.clone(),
            BuildJob::Running { kill_tx: Some(kill_tx) },
        );
    }

    let state_bg = state.clone();
    let job_id_bg = job_id.clone();
    tokio::spawn(async move {
        // Drain the pipes concurrently so a chatty build can't block on a full pipe buffer.
        let mut stdout_pipe = child.stdout.take();
        let mut stderr_pipe = child.stderr.take();

        let stdout_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            if let Some(ref mut s) = stdout_pipe {
                let _ = s.read_to_end(&mut buf).await;
            }
            buf
        });
        let stderr_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            if let Some(ref mut s) = stderr_pipe {
                let _ = s.read_to_end(&mut buf).await;
            }
            buf
        });

        // Child stays owned here for the whole select, so child.id() remains
        // valid (the kernel can't reuse the PID until we reap via wait()).
        let wait_result = tokio::select! {
            status = child.wait() => status,
            _ = kill_rx => {
                if let Some(pid) = child.id() {
                    unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
                }
                child.wait().await
            }
        };

        let stdout_bytes = stdout_task.await.unwrap_or_default();
        let stderr_bytes = stderr_task.await.unwrap_or_default();

        let job = match wait_result {
            Ok(status) => BuildJob::Done {
                exit_code: status.code().unwrap_or(-1),
                stdout: String::from_utf8_lossy(&stdout_bytes).into_owned(),
                stderr: String::from_utf8_lossy(&stderr_bytes).into_owned(),
            },
            Err(e) => BuildJob::Done {
                exit_code: -1,
                stdout: String::from_utf8_lossy(&stdout_bytes).into_owned(),
                stderr: format!(
                    "Failed to wait for build.sh: {e}\n{}",
                    String::from_utf8_lossy(&stderr_bytes)
                ),
            },
        };

        let mut st = state_bg.lock().await;
        st.jobs.insert(job_id_bg, job);
    });

    Ok(text_ok(format!("Build started. job_id: {job_id}")))
}
