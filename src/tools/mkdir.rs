use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::state::AppState;
use crate::tools::{text_err, text_ok, ToolResult};
use crate::util::{
    check_is_regular, excluded_dirs_for, is_excluded_path, path_depth, safe_path, validate_put_path,
};

pub async fn run(state: Arc<Mutex<AppState>>, args: &Value) -> ToolResult {
    let (project_dir, max_depth, language) = {
        let st = state.lock().await;
        match st.project_dir.clone() {
            Some(d) => (d, st.max_depth, st.language.clone().unwrap_or_default()),
            None => {
                return Ok(text_err(
                    "404: no active project — call create_project or use_project first.",
                ))
            }
        }
    };

    let excluded = excluded_dirs_for(&language);

    let path_str = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (-32602i32, "Missing required argument: path".to_string()))?;

    // ── 400: excluded build-artifact directories ──────────────────────────────
    if is_excluded_path(path_str, excluded) {
        return Ok(text_err(format!(
            "400: '{path_str}' is inside a build-artifact directory \
             and cannot be created directly."
        )));
    }

    // ── 400: '.meta/' is reserved for project metadata; see put.rs for rationale.
    if is_excluded_path(path_str, &[".meta"]) {
        return Ok(text_err(format!(
            "400: '{path_str}' is inside '.meta/' which is reserved for project metadata."
        )));
    }

    // Reuse the same per-component character rules as put (._-a-zA-Z0-9).
    // validate_put_path already rejects absolute paths, '..' and bad chars.
    if let Err(reason) = validate_put_path(path_str) {
        return Ok(text_err(format!("400: invalid path — {reason}")));
    }

    let depth = path_depth(path_str);
    if depth > max_depth {
        return Ok(text_err(format!(
            "400: directory depth {depth} exceeds project max_depth {max_depth}."
        )));
    }

    let target = match safe_path(&project_dir, path_str) {
        Some(p) => p,
        None => {
            return Ok(text_err(format!(
                "400: path '{path_str}' escapes the project root."
            )))
        }
    };

    // Reject symlinks or special files anywhere on the path.
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

    if target.exists() {
        if target.is_dir() {
            return Ok(text_ok(format!("Directory '{path_str}' already exists.")));
        }
        return Ok(text_err(format!(
            "400: '{path_str}' already exists as a file."
        )));
    }

    tokio::fs::create_dir_all(&target)
        .await
        .map_err(|e| (-32603i32, format!("Failed to create directory: {e}")))?;

    Ok(text_ok(format!("Directory '{path_str}' created.")))
}
