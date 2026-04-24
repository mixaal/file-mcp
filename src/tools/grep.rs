use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::constants::{
    GREP_BIN, GREP_MAX_CONTEXT, GREP_PATTERN_MAX_LEN, MAX_GREP_OUTPUT_BYTES,
};
use crate::state::AppState;
use crate::tools::{text_err, text_ok, ToolResult};
use crate::util::{check_is_regular, excluded_dirs_for, safe_path};

pub async fn run(state: Arc<Mutex<AppState>>, args: &Value) -> ToolResult {
    let (project_dir, language) = {
        let st = state.lock().await;
        match st.project_dir.clone() {
            Some(d) => (d, st.language.clone().unwrap_or_default()),
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

    let regex = args
        .get("regex")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (-32602i32, "Missing required argument: regex".to_string()))?;

    if regex.is_empty() {
        return Ok(text_err("400: regex must not be empty."));
    }
    if regex.len() > GREP_PATTERN_MAX_LEN {
        return Ok(text_err(format!(
            "400: regex is {} bytes, exceeds max {GREP_PATTERN_MAX_LEN}.",
            regex.len()
        )));
    }

    let lines_after = parse_context(args.get("lines_after"), "lines_after")?;
    let lines_before = parse_context(args.get("lines_before"), "lines_before")?;

    let output_line_numbers = args
        .get("output_line_numbers")
        .or_else(|| args.get("output_lines_numbers"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

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

    let is_dir = target.is_dir();

    // Assemble grep argv. Use -E (ERE) for a well-defined, bounded regex
    // flavour, and always -H so single-file output still prints the filename
    // prefix (matches the recursive format).
    let mut argv: Vec<String> = Vec::new();
    argv.push("-E".to_string());
    argv.push("-H".to_string());
    if output_line_numbers {
        argv.push("-n".to_string());
    }
    if lines_after > 0 {
        argv.push(format!("-A{lines_after}"));
    }
    if lines_before > 0 {
        argv.push(format!("-B{lines_before}"));
    }
    if is_dir {
        argv.push("-r".to_string());
        argv.push("--exclude-dir=.git".to_string());
        argv.push("--exclude-dir=.meta".to_string());
        for excluded in excluded_dirs_for(&language) {
            argv.push(format!("--exclude-dir={excluded}"));
        }
    }
    // `--` terminates options so a pattern starting with '-' is safe.
    argv.push("--".to_string());
    argv.push(regex.to_string());
    argv.push(target.to_string_lossy().into_owned());

    let output = tokio::process::Command::new(GREP_BIN)
        .args(&argv)
        .current_dir(&project_dir)
        .output()
        .await
        .map_err(|e| (-32603i32, format!("failed to spawn grep: {e}")))?;

    // grep exit codes: 0 = match, 1 = no match, 2 = error.
    let code = output.status.code().unwrap_or(-1);
    match code {
        0 => {
            let stdout = truncate_output(&output.stdout);
            // Strip the absolute project_dir prefix from filenames so clients
            // only see relative paths.
            let prefix = format!("{}/", project_dir.to_string_lossy());
            let cleaned = stdout.replace(&prefix, "");
            Ok(text_ok(cleaned))
        }
        1 => Ok(text_ok(format!("0 matches for /{regex}/ in '{path_str}'."))),
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            Ok(text_err(format!("grep failed (exit {code}): {stderr}")))
        }
    }
}

fn parse_context(v: Option<&Value>, name: &'static str) -> Result<u64, (i32, String)> {
    let Some(v) = v else { return Ok(0) };
    if v.is_null() {
        return Ok(0);
    }
    let n = v
        .as_i64()
        .ok_or_else(|| (-32602i32, format!("{name} must be a non-negative integer")))?;
    if n < 0 {
        return Err((
            -32602i32,
            format!("{name} must be a non-negative integer"),
        ));
    }
    let n = n as u64;
    Ok(n.min(GREP_MAX_CONTEXT))
}

fn truncate_output(bytes: &[u8]) -> String {
    let mut s = String::from_utf8_lossy(bytes).into_owned();
    if s.len() > MAX_GREP_OUTPUT_BYTES {
        let mut cut = MAX_GREP_OUTPUT_BYTES;
        while cut > 0 && !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push_str(&format!(
            "\n... (truncated at {MAX_GREP_OUTPUT_BYTES} bytes)"
        ));
    }
    s
}
