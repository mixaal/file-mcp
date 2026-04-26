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

    let path_str = match args.get("path").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return Ok(text_err(
                "400: missing path param relative to project (required)",
            ))
        }
    };

    let old_str = match args.get("old_str").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return Ok(text_err(
                "400: missing required argument 'old_str' (string). The exact substring to replace — \
                 matching is BYTE-BY-BYTE, including whitespace, tabs vs spaces, and newlines.",
            ))
        }
    };

    // new_str may be "" (meaning: delete old_str).
    let new_str = match args.get("new_str").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return Ok(text_err(
                "400: missing required argument 'new_str' (string). Use \"\" to delete old_str.",
            ))
        }
    };

    let message_raw = match args.get("message").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return Ok(text_err(
                "400: missing required argument 'message' (string, git commit message).",
            ))
        }
    };

    // Accept both the correct spelling and the common misspelling.
    let occurrence_val = args.get("occurrence").or_else(|| args.get("occurence"));
    let occurrence = match occurrence_val.and_then(|v| v.as_i64()) {
        Some(n) => n,
        None => {
            let detail = match occurrence_val {
                None => "argument is missing",
                Some(v) if v.is_string() => "argument must be an integer, not a string",
                Some(_) => "argument must be an integer",
            };
            return Ok(text_err(format!(
                "400: invalid 'occurrence' — {detail}. Choose one: \
                 0 = require old_str to be unique (error if it appears more than once), \
                 -1 = replace every occurrence, \
                 n>=1 = replace only the n-th occurrence (1-indexed)."
            )));
        }
    };

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
        let hint = diagnose_no_match(&original, old_str);
        return Ok(text_err(format!(
            "404: old_str not found in '{path_str}' (0 occurrences). \
             Matching is BYTE-BY-BYTE — whitespace, tabs vs spaces, and line endings must match exactly.{hint} \
             Re-fetch the file with `get` and copy the bytes verbatim."
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

/// When old_str doesn't match, try a few common whitespace/line-ending
/// substitutions and report which one would have matched. Returned string
/// is either empty or starts with " Hint: ".
fn diagnose_no_match(haystack: &str, needle: &str) -> String {
    // 1. Tabs in needle but file uses spaces.
    if needle.contains('\t') {
        for width in [8usize, 4, 2] {
            let candidate = needle.replace('\t', &" ".repeat(width));
            if haystack.contains(&candidate) {
                return format!(
                    " Hint: your old_str uses TABS but the file uses {width} SPACES at the matching location."
                );
            }
        }
    }

    // 2. Spaces in needle but file uses tabs. Try collapsing runs of N spaces
    //    into a tab (only meaningful when the needle has multi-space runs).
    for width in [8usize, 4, 2] {
        let pat = " ".repeat(width);
        if !needle.contains(&pat) {
            continue;
        }
        let candidate = needle.replace(&pat, "\t");
        if candidate != needle && haystack.contains(&candidate) {
            return format!(
                " Hint: your old_str uses {width} SPACES but the file uses TABS at the matching location."
            );
        }
    }

    // 3. Line-ending mismatch.
    if needle.contains("\r\n") {
        let candidate = needle.replace("\r\n", "\n");
        if haystack.contains(&candidate) {
            return " Hint: your old_str uses CRLF line endings but the file uses LF.".to_string();
        }
    } else if needle.contains('\n') && haystack.contains("\r\n") {
        let candidate = needle.replace('\n', "\r\n");
        if haystack.contains(&candidate) {
            return " Hint: your old_str uses LF line endings but the file uses CRLF.".to_string();
        }
    }

    // 4. Trailing whitespace on lines differs.
    let trim_trailing = |s: &str| -> String {
        s.split('\n')
            .map(|l| l.trim_end_matches([' ', '\t']))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let needle_trim = trim_trailing(needle);
    if needle_trim != needle {
        let hay_trim = trim_trailing(haystack);
        if hay_trim.contains(&needle_trim) {
            return " Hint: trailing whitespace on one or more lines differs between old_str and the file."
                .to_string();
        }
    } else {
        let hay_trim = trim_trailing(haystack);
        if hay_trim != haystack && hay_trim.contains(needle) {
            return " Hint: the file has trailing whitespace on one or more lines that your old_str does not.".to_string();
        }
    }

    String::new()
}

#[cfg(test)]
mod tests {
    use super::diagnose_no_match;

    #[test]
    fn detects_tab_in_needle_spaces_in_file() {
        let file = "    let x = 1;\n";
        let needle = "\tlet x = 1;\n";
        let hint = diagnose_no_match(file, needle);
        assert!(hint.contains("TABS"), "got: {hint}");
        assert!(hint.contains("SPACES"), "got: {hint}");
    }

    #[test]
    fn detects_spaces_in_needle_tabs_in_file() {
        let file = "\tlet x = 1;\n";
        let needle = "    let x = 1;\n";
        let hint = diagnose_no_match(file, needle);
        assert!(hint.contains("SPACES"), "got: {hint}");
        assert!(hint.contains("TABS"), "got: {hint}");
    }

    #[test]
    fn detects_crlf_vs_lf() {
        let file = "a\nb\nc\n";
        let needle = "a\r\nb\r\n";
        let hint = diagnose_no_match(file, needle);
        assert!(hint.contains("CRLF"), "got: {hint}");
    }

    #[test]
    fn empty_when_no_obvious_cause() {
        let hint = diagnose_no_match("hello world", "completely unrelated");
        assert!(hint.is_empty(), "got: {hint}");
    }
}
