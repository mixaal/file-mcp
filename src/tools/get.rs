use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::state::AppState;
use crate::tools::{text_err, text_ok, ToolResult};
use crate::util::{check_is_regular, safe_path};

pub async fn run(state: Arc<Mutex<AppState>>, args: &Value) -> ToolResult {
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

    let path_str = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (-32602i32, "Missing required argument: path".to_string()))?;

    let target = match safe_path(&project_dir, path_str) {
        Some(p) => p,
        None => {
            return Ok(text_err(format!(
                "404: path '{path_str}' is not allowed (absolute paths and escaping paths are rejected)."
            )))
        }
    };

    if !target.exists() {
        return Ok(text_err(format!("404: not found: {path_str}")));
    }

    if let Err(reason) = check_is_regular(&target) {
        return Ok(text_err(format!("403: {reason}")));
    }

    if target.is_dir() {
        let mut entries = Vec::new();
        let mut dir = tokio::fs::read_dir(&target)
            .await
            .map_err(|e| (-32603i32, format!("Failed to read directory: {e}")))?;
        while let Some(entry) = dir
            .next_entry()
            .await
            .map_err(|e| (-32603i32, e.to_string()))?
        {
            // Use symlink_metadata so we never follow symlinks.
            let Ok(meta) = entry.path().symlink_metadata() else { continue };
            let ft = meta.file_type();
            if ft.is_symlink() || (!ft.is_file() && !ft.is_dir()) {
                continue; // skip symlinks and special files silently
            }
            let kind = if ft.is_dir() { "dir" } else { "file" };
            entries.push(format!("{} ({kind})", entry.file_name().to_string_lossy()));
        }
        entries.sort();
        return Ok(text_ok(format!(
            "Directory listing for '{path_str}':\n{}",
            entries.join("\n")
        )));
    }

    let content = tokio::fs::read_to_string(&target)
        .await
        .map_err(|e| (-32603i32, format!("Failed to read file: {e}")))?;

    Ok(text_ok(content))
}
