use serde_json::Value;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::constants::{MAX_DEPTH_HARD_CAP, MAX_FILES_HARD_CAP};
use crate::state::{AppState, ProjectSize};
use crate::tools::{text_err, text_ok, ToolResult};
use crate::util::validate_name;

pub async fn run(state: Arc<Mutex<AppState>>, args: &Value) -> ToolResult {
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (-32602i32, "Missing required argument: name".to_string()))?
        .to_string();

    if let Err(reason) = validate_name(&name) {
        return Ok(text_err(format!("400: invalid project name — {reason}")));
    }

    let base_dir = {
        let st = state.lock().await;
        st.base_dir.clone()
    };

    let project_dir = base_dir.join(&name);

    if !project_dir.is_dir() {
        return Ok(text_err(format!("Project not found: {name}")));
    }

    let meta_path = project_dir.join(".meta/project.json");
    if !meta_path.exists() {
        return Ok(text_err(format!(
            "Directory '{name}' exists but has no .meta/project.json — was it created by this server?"
        )));
    }

    let raw = tokio::fs::read_to_string(&meta_path)
        .await
        .map_err(|e| (-32603i32, format!("Failed to read project meta: {e}")))?;

    let meta: Value = serde_json::from_str(&raw)
        .map_err(|e| (-32603i32, format!("Failed to parse project meta: {e}")))?;

    // SECURITY NOTE: .meta/project.json is treated as user-owned configuration —
    // the user has the right to hand-edit max_files / max_depth to raise quotas
    // beyond what create_project chose. put/mkdir block writes into '.meta/' so
    // that an MCP client cannot lift its own limits, but a local edit is fine.
    // The values are clamped to MAX_FILES_HARD_CAP / MAX_DEPTH_HARD_CAP here so
    // an accidental or malicious absurd number cannot disable the checks entirely.
    let language = meta["language"].as_str().unwrap_or("unknown").to_string();
    let max_files = (meta["max_files"].as_u64().unwrap_or(200) as usize).min(MAX_FILES_HARD_CAP);
    let max_depth = (meta["max_depth"].as_u64().unwrap_or(3) as usize).min(MAX_DEPTH_HARD_CAP);
    let size = ProjectSize::from_str(meta["size"].as_str().unwrap_or("medium"))
        .unwrap_or_default();

    {
        let mut st = state.lock().await;
        st.project_dir = Some(project_dir);
        st.language = Some(language.clone());
        st.max_files = max_files;
        st.max_depth = max_depth;
        st.size = size;
        st.project_name = Some(name.clone());
    }

    Ok(text_ok(format!(
        "Active project set to '{name}' (language={language}, max_files={max_files}, max_depth={max_depth})."
    )))
}
