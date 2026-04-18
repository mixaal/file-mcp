use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::constants::PAGE_SIZE;
use crate::state::AppState;
use crate::tools::{text_err, text_ok, ToolResult};
use crate::util::safe_path;

pub async fn run(state: Arc<Mutex<AppState>>, args: &Value) -> ToolResult {
    let (project_dir, max_depth) = {
        let st = state.lock().await;
        match st.project_dir.clone() {
            Some(d) => (d, st.max_depth),
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

    let depth_limit = args
        .get("depth")
        .and_then(|v| v.as_u64())
        .map(|d| d as usize)
        .unwrap_or(max_depth);

    let offset = args
        .get("offset")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;

    let show_git = args
        .get("show_git")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let root = if path_str == "." || path_str.is_empty() {
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

    if !root.exists() {
        return Ok(text_err(format!("404: not found: {path_str}")));
    }
    if !root.is_dir() {
        return Ok(text_err(format!("400: '{path_str}' is a file, not a directory.")));
    }

    // Build the full tree synchronously (fs traversal is fast enough).
    let root_clone = root.clone();
    let mut lines: Vec<String> = tokio::task::spawn_blocking(move || {
        let mut out = Vec::new();
        collect(&root_clone, "", 0, depth_limit, show_git, &mut out);
        out
    })
    .await
    .map_err(|e| (-32603i32, format!("Tree traversal error: {e}")))?;

    let total = lines.len();
    let page: Vec<String> = lines.drain(..).skip(offset).take(PAGE_SIZE).collect();
    let end = offset + page.len();

    let header = format!(
        "tree '{path_str}'  [{}-{} of {}{}]",
        offset + 1,
        end,
        total,
        if end < total { ", use offset to page" } else { "" },
    );

    Ok(text_ok(format!("{header}\n{}", page.join("\n"))))
}

fn collect(dir: &Path, prefix: &str, depth: usize, max_depth: usize, show_git: bool, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };

    // Collect only regular files and plain directories; skip symlinks and special files.
    let mut entries: Vec<PathBuf> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path();
            if !show_git && p.file_name().map(|n| n == ".git").unwrap_or(false) {
                return None;
            }
            let Ok(meta) = p.symlink_metadata() else { return None };
            let ft = meta.file_type();
            if ft.is_symlink() || (!ft.is_file() && !ft.is_dir()) {
                return None;
            }
            Some(p)
        })
        .collect();
    entries.sort();

    let count = entries.len();
    for (i, path) in entries.iter().enumerate() {
        let is_last = i == count - 1;
        let connector = if is_last { "└── " } else { "├── " };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        // Safe because we already checked it's not a symlink.
        let Ok(meta) = path.symlink_metadata() else { continue };
        let is_dir = meta.file_type().is_dir();

        out.push(format!(
            "{prefix}{connector}{name}{}",
            if is_dir { "/" } else { "" }
        ));

        if is_dir && depth < max_depth {
            let child_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });
            collect(path, &child_prefix, depth + 1, max_depth, show_git, out);
        }
    }
}
