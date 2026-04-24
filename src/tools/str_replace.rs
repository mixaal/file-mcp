use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

use std::path::Path;

use crate::constants::{GIT_BIN, MAX_PUT_FILE_SZ};
use crate::state::AppState;
use crate::tools::{text_err, text_ok, ToolResult};
use crate::util::{
    check_is_regular, excluded_dirs_for, is_excluded_path, path_depth, run_git, safe_path,
    sanitize_message, validate_put_path,
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

    let old_str = args
        .get("old_str")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (-32602i32, "Missing required argument: old_str".to_string()))?;

    // new_str may be "" (meaning: delete old_str).
    let new_str = args
        .get("new_str")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (-32602i32, "Missing required argument: new_str".to_string()))?;

    let message_raw = args
        .get("message")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (-32602i32, "Missing required argument: message".to_string()))?;

    // Accept both the correct spelling and the common misspelling.
    let occurrence = args
        .get("occurrence")
        .or_else(|| args.get("occurence"))
        .and_then(|v| v.as_i64())
        .ok_or_else(|| (-32602i32, "Missing required argument: occurrence".to_string()))?;

    if old_str.is_empty() {
        return Ok(text_err("400: old_str must not be empty."));
    }

    // Mirror the same path/message/exclusion checks put uses, so str_replace
    // cannot be used to sidestep them.
    if is_excluded_path(path_str, excluded) {
        return Ok(text_err(format!(
            "400: '{path_str}' is inside a build-artifact directory \
             and cannot be written directly."
        )));
    }

    if is_excluded_path(path_str, &[".meta"]) {
        return Ok(text_err(format!(
            "400: '{path_str}' is inside '.meta/' which is reserved for project metadata."
        )));
    }

    if let Err(reason) = validate_put_path(path_str) {
        return Ok(text_err(format!("400: invalid path — {reason}")));
    }

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

    if !target.exists() {
        return Ok(text_err(format!("404: not found: {path_str}")));
    }

    if let Err(reason) = check_is_regular(&target) {
        return Ok(text_err(format!("403: {reason}")));
    }

    if target.is_dir() {
        return Ok(text_err(format!(
            "400: '{path_str}' is a directory; str_replace only operates on files."
        )));
    }

    // Walk ancestors to refuse any symlinked intermediate directory.
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

    let original = tokio::fs::read_to_string(&target)
        .await
        .map_err(|e| (-32603i32, format!("Failed to read file: {e}")))?;

    // Collect byte offsets of every non-overlapping match of old_str.
    let matches: Vec<usize> = original.match_indices(old_str).map(|(i, _)| i).collect();
    let count = matches.len();

    if count == 0 {
        return Ok(text_err(format!(
            "404: old_str not found in '{path_str}' (0 occurrences)."
        )));
    }

    let updated: String = match occurrence {
        0 => {
            if count != 1 {
                return Ok(text_err(format!(
                    "409: old_str is not unique in '{path_str}' ({count} occurrences) — \
                     pass occurrence=-1 to replace all, or 1..{count} to pick one."
                )));
            }
            let start = matches[0];
            let mut out = String::with_capacity(original.len() + new_str.len());
            out.push_str(&original[..start]);
            out.push_str(new_str);
            out.push_str(&original[start + old_str.len()..]);
            out
        }
        -1 => original.replace(old_str, new_str),
        n if n >= 1 => {
            let idx = n as usize;
            if idx > count {
                return Ok(text_err(format!(
                    "400: occurrence {idx} out of range — file has {count} occurrence(s) of old_str."
                )));
            }
            let start = matches[idx - 1];
            let mut out = String::with_capacity(original.len() + new_str.len());
            out.push_str(&original[..start]);
            out.push_str(new_str);
            out.push_str(&original[start + old_str.len()..]);
            out
        }
        _ => {
            return Ok(text_err(format!(
                "400: invalid occurrence {occurrence} — use 0 (unique), -1 (all), or 1..n."
            )));
        }
    };

    if updated.len() > MAX_PUT_FILE_SZ {
        return Ok(text_err(format!(
            "413: resulting content is {} bytes, exceeds max {MAX_PUT_FILE_SZ}.",
            updated.len()
        )));
    }

    if updated == original {
        return Ok(text_ok(format!(
            "File '{path_str}' unchanged (new_str is identical to old_str)."
        )));
    }

    tokio::fs::write(&target, &updated)
        .await
        .map_err(|e| (-32603i32, format!("Failed to write file: {e}")))?;

    let rel = target
        .strip_prefix(&project_dir)
        .unwrap_or(&target)
        .to_string_lossy()
        .into_owned();

    run_git(Path::new(GIT_BIN), &project_dir, &["add", &rel])
        .await
        .map_err(|e| (-32603i32, format!("git add failed: {e}")))?;

    let replaced = match occurrence {
        -1 => count,
        0 => 1,
        n => n as usize,
    };

    match run_git(Path::new(GIT_BIN), &project_dir, &["commit", "-a", "-m", &message]).await {
        Ok(out) => Ok(text_ok(format!(
            "File '{path_str}' updated ({replaced} replacement(s)) and committed (message='{message}').\n{out}"
        ))),
        Err(e) if e.contains("nothing to commit") => Ok(text_ok(format!(
            "File '{path_str}' updated ({replaced} replacement(s)) — no effective changes to commit."
        ))),
        Err(e) => Ok(text_err(format!(
            "File '{path_str}' updated but git commit failed: {e}"
        ))),
    }
}
