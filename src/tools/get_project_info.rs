use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::state::AppState;
use crate::tools::{text_err, ToolResult};

pub async fn run(state: Arc<Mutex<AppState>>) -> ToolResult {
    let (project_dir, fallback) = {
        let st = state.lock().await;
        match st.project_dir.clone() {
            None => {
                return Ok(text_err(
                    "404: no active project — call create_project or use_project first.",
                ))
            }
            Some(d) => {
                let fb = json!({
                    "name":      st.project_name.clone().unwrap_or_default(),
                    "language":  st.language.clone().unwrap_or_default(),
                    "size":      st.size.as_str(),
                    "max_files": st.max_files,
                    "max_depth": st.max_depth,
                });
                (d, fb)
            }
        }
    };

    // Prefer the persisted .meta/project.json so we always reflect the canonical record.
    let meta_path = project_dir.join(".meta/project.json");
    let info: Value = if meta_path.exists() {
        match tokio::fs::read_to_string(&meta_path).await {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or(fallback),
            Err(_) => fallback,
        }
    } else {
        fallback
    };

    Ok(json!({
        "content": [
            {
                "type": "text",
                "text": serde_json::to_string_pretty(&info).unwrap()
            }
        ]
    }))
}
