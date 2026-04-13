use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::state::AppState;
use crate::tools::{text_err, text_ok, ToolResult};
use crate::util::{
    check_is_regular, count_files_sync, path_depth, run_git, safe_path, sanitize_message,
    validate_put_path,
};

pub async fn run(state: Arc<Mutex<AppState>>, args: &Value) -> ToolResult {
    let (project_dir, git_cmd, max_files, max_depth) = {
        let st = state.lock().await;
        match st.project_dir.clone() {
            Some(d) => (d, st.git_cmd.clone(), st.max_files, st.max_depth),
            None => {
                return Ok(text_err(
                    "404: no active project — call create_project or use_project first.",
                ))
            }
        }
    };

    let path_str = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (-32602i32, "Missing required argument: path".to_string()))?;

    let content = args
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (-32602i32, "Missing required argument: content".to_string()))?;

    let message_raw = args
        .get("message")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (-32602i32, "Missing required argument: message".to_string()))?;

    // ── 400: filename character validation ────────────────────────────────────
    if let Err(reason) = validate_put_path(path_str) {
        return Ok(text_err(format!("400: invalid path — {reason}")));
    }

    // ── 400: depth limit ─────────────────────────────────────────────────────
    let depth = path_depth(path_str);
    if depth > max_depth {
        return Ok(text_err(format!(
            "400: path depth {depth} exceeds project max_depth {max_depth}."
        )));
    }

    let message = sanitize_message(message_raw);
    if message.is_empty() {
        return Ok(text_err(
            "400: commit message is empty after sanitisation (only a-zA-Z0-9- and spaces are kept).",
        ));
    }

    let target = match safe_path(&project_dir, path_str) {
        Some(p) => p,
        None => {
            return Ok(text_err(format!(
                "400: path '{path_str}' escapes the project root."
            )))
        }
    };

    // Parent directory must already exist — we do NOT create directories via put.
    if let Some(parent) = target.parent() {
        if !parent.exists() {
            return Ok(text_err(format!(
                "400: parent directory does not exist for '{path_str}'. \
                 Create directories explicitly before writing files."
            )));
        }
    }

    // ── 403: reject symlinks / special files ─────────────────────────────────
    // Check the target itself if it already exists, and also every existing
    // ancestor up to the project root, so we can't be tricked by a symlinked
    // intermediate directory.
    {
        let mut check = target.clone();
        loop {
            if check.exists() || check.symlink_metadata().is_ok() {
                if let Err(reason) = check_is_regular(&check) {
                    return Ok(text_err(format!("403: {reason}")));
                }
            }
            if check == project_dir {
                break;
            }
            match check.parent() {
                Some(p) => check = p.to_path_buf(),
                None => break,
            }
        }
    }

    // ── 400: file count limit (only for new files) ────────────────────────────
    if !target.exists() {
        let dir = project_dir.clone();
        let current = tokio::task::spawn_blocking(move || count_files_sync(&dir))
            .await
            .unwrap_or(0);
        if current >= max_files {
            return Ok(text_err(format!(
                "400: project file limit reached ({current}/{max_files}). \
                 Increase the size preset or remove unused files."
            )));
        }
    }

    tokio::fs::write(&target, content)
        .await
        .map_err(|e| (-32603i32, format!("Failed to write file: {e}")))?;

    // Stage file (handles new untracked files), then commit with -a.
    let rel = target
        .strip_prefix(&project_dir)
        .unwrap_or(&target)
        .to_string_lossy()
        .into_owned();

    run_git(&git_cmd, &project_dir, &["add", &rel])
        .await
        .map_err(|e| (-32603i32, format!("git add failed: {e}")))?;

    match run_git(&git_cmd, &project_dir, &["commit", "-a", "-m", &message]).await {
        Ok(out) => Ok(text_ok(format!(
            "File '{path_str}' written and committed (message='{message}').\n{out}"
        ))),
        Err(e) if e.contains("nothing to commit") => Ok(text_ok(format!(
            "File '{path_str}' written (no effective changes to commit)."
        ))),
        Err(e) => Ok(text_err(format!(
            "File '{path_str}' written but git commit failed: {e}"
        ))),
    }
}
