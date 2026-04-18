use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::constants::PAGE_SIZE;
use crate::state::AppState;
use crate::tools::{text_err, text_ok, ToolResult};
use crate::util::safe_path;

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
        .unwrap_or(".");

    let offset = args
        .get("offset")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;

    let show_git = args
        .get("show_git")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let target = if path_str == "." || path_str.is_empty() {
        project_dir.clone()
    } else {
        match safe_path(&project_dir, path_str) {
            Some(p) => p,
            None => {
                return Ok(text_err(format!(
                    "404: path '{path_str}' is not allowed."
                )))
            }
        }
    };

    if !target.exists() {
        return Ok(text_err(format!("404: not found: {path_str}")));
    }
    if !target.is_dir() {
        return Ok(text_err(format!("400: '{path_str}' is a file, not a directory.")));
    }

    let mut entries: Vec<String> = {
        let mut dir = tokio::fs::read_dir(&target)
            .await
            .map_err(|e| (-32603i32, format!("Failed to read directory: {e}")))?;

        let mut list = Vec::new();
        while let Some(entry) = dir
            .next_entry()
            .await
            .map_err(|e| (-32603i32, e.to_string()))?
        {
            // symlink_metadata never follows symlinks — skip anything exotic.
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if !show_git && name_str == ".git" {
                continue;
            }
            let Ok(meta) = entry.path().symlink_metadata() else { continue };
            let ft = meta.file_type();
            if ft.is_symlink() || (!ft.is_file() && !ft.is_dir()) {
                continue;
            }
            let suffix = if ft.is_dir() { "/" } else { "" };
            list.push(format!("{name_str}{suffix}"));
        }
        list.sort();
        list
    };

    let total = entries.len();
    let page: Vec<String> = entries.drain(..).skip(offset).take(PAGE_SIZE).collect();
    let end = offset + page.len();

    let header = format!(
        "ls '{path_str}'  [{}-{} of {}{}]",
        offset + 1,
        end,
        total,
        if end < total { ", use offset to page" } else { "" },
    );

    Ok(text_ok(format!("{header}\n{}", page.join("\n"))))
}
