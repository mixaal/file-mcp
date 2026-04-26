use serde_json::Value;
use std::sync::Arc;
use tokio::io::AsyncBufReadExt;
use tokio::sync::Mutex;

use crate::constants::MAX_GET_FILE_SZ;
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

    let path_str = match args.get("path").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return Ok(text_err(
                "400: missing path param relative to project (required)",
            ))
        }
    };

    let start_line = parse_line_arg(args.get("start_line"), "start_line")?;
    let end_line = parse_line_arg(args.get("end_line"), "end_line")?;
    if let (Some(s), Some(e)) = (start_line, end_line) {
        if s > e {
            return Ok(text_err(format!(
                "400: start_line {s} is greater than end_line {e}."
            )));
        }
    }
    let range_requested = start_line.is_some() || end_line.is_some();

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
        if range_requested {
            return Ok(text_err(
                "400: start_line/end_line only apply to files, not directories.",
            ));
        }
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

    if !range_requested {
        let size = tokio::fs::metadata(&target)
            .await
            .map_err(|e| (-32603i32, format!("Failed to stat file: {e}")))?
            .len();
        if size > MAX_GET_FILE_SZ {
            return Ok(text_err(format!(
                "413: file '{path_str}' is {size} bytes, exceeds max {MAX_GET_FILE_SZ}. \
                 Use start_line/end_line to read a slice."
            )));
        }

        let content = tokio::fs::read_to_string(&target)
            .await
            .map_err(|e| (-32603i32, format!("Failed to read file: {e}")))?;

        return Ok(text_ok(content));
    }

    // Ranged read: stream line-by-line so the file's size on disk does not
    // matter — only the returned slice is capped at MAX_GET_FILE_SZ bytes.
    let start = start_line.unwrap_or(1);
    let end = end_line.unwrap_or(u64::MAX);

    let file = tokio::fs::File::open(&target)
        .await
        .map_err(|e| (-32603i32, format!("Failed to open file: {e}")))?;
    let mut reader = tokio::io::BufReader::new(file).lines();

    let mut out = String::new();
    let mut lineno: u64 = 0;
    let max_bytes: usize = MAX_GET_FILE_SZ as usize;
    let mut truncated = false;

    while let Some(line) = reader
        .next_line()
        .await
        .map_err(|e| (-32603i32, format!("Failed to read file: {e}")))?
    {
        lineno += 1;
        if lineno < start {
            continue;
        }
        if lineno > end {
            break;
        }
        // +1 for the '\n' we will append.
        if out.len() + line.len() + 1 > max_bytes {
            truncated = true;
            break;
        }
        out.push_str(&line);
        out.push('\n');
    }

    if lineno < start {
        return Ok(text_err(format!(
            "404: start_line {start} is past end of file (file has {lineno} line(s))."
        )));
    }

    if truncated {
        out.push_str(&format!(
            "... (truncated at {max_bytes} bytes; shorten the range)\n"
        ));
    }

    Ok(text_ok(out))
}

fn parse_line_arg(v: Option<&Value>, name: &'static str) -> Result<Option<u64>, (i32, String)> {
    let Some(v) = v else { return Ok(None) };
    if v.is_null() {
        return Ok(None);
    }
    let n = v
        .as_i64()
        .ok_or_else(|| (-32602i32, format!("{name} must be a positive integer (>= 1)")))?;
    if n < 1 {
        return Err((
            -32602i32,
            format!("{name} must be a positive integer (>= 1)"),
        ));
    }
    Ok(Some(n as u64))
}
