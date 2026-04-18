use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::state::AppState;

pub mod build_start;
pub mod build_status;
pub mod create_project;
pub mod git_diff;
pub mod git_diff_staged;
pub mod git_log;
pub mod git_status;
pub mod godot;
pub mod get;
pub mod get_project_info;
pub mod ls;
pub mod mkdir;
pub mod put;
pub mod pwd;
pub mod tree;
pub mod use_project;

pub type ToolResult = Result<Value, (i32, String)>;

// ── helpers used by every tool ──────────────────────────────────────────────

pub fn text_ok(msg: impl Into<String>) -> Value {
    json!({"content": [{"type": "text", "text": msg.into()}]})
}

pub fn text_err(msg: impl Into<String>) -> Value {
    json!({"content": [{"type": "text", "text": msg.into()}], "isError": true})
}

// ── tools/list ───────────────────────────────────────────────────────────────

pub fn list() -> Value {
    json!({
        "tools": [
            {
                "name": "create_project",
                "description": "Create a new project for the given language. Initialises the project skeleton, creates a .meta/project.json, runs git init, and makes the project active. Subsequent file operations are restricted to this directory.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Project name (used as the directory name under the base projects directory)"
                        },
                        "language": {
                            "type": "string",
                            "description": "Programming language: rust, go, java, python, c, cpp, javascript, typescript, godot3d (FPS template), godot2d (platformer template), or any other identifier"
                        },
                        "size": {
                            "type": "string",
                            "enum": ["small", "medium", "large"],
                            "description": "Project size preset — small: 50 files / depth 2 (java 4), medium: 200 files / depth 3 (java 4) [default], large: 1000 files / depth 5 (java 7)"
                        }
                    },
                    "required": ["name", "language"]
                }
            },
            {
                "name": "pwd",
                "description": "Return the current working directory as the opaque token 'poc' (real path is not exposed).",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "use_project",
                "description": "Activate an existing project by name. All subsequent file operations are restricted to that project directory.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Name of the project directory to activate"
                        }
                    },
                    "required": ["name"]
                }
            },
            {
                "name": "get",
                "description": "Read a file (or list a directory) within the active project. Returns 404-equivalent if no project is active. Absolute paths and paths escaping the project root are rejected.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Relative path within the project directory (e.g. 'src/main.rs')"
                        }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "put",
                "description": "Write a file within the active project and commit it with git. Returns 404-equivalent if no project is active. Absolute paths and paths escaping the project root are rejected.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Relative path within the project directory"
                        },
                        "content": {
                            "type": "string",
                            "description": "Complete file content to write"
                        },
                        "message": {
                            "type": "string",
                            "description": "Git commit message. Only a-zA-Z0-9, hyphens, and spaces are kept; consecutive spaces are collapsed; result is truncated to 120 chars."
                        }
                    },
                    "required": ["path", "content", "message"]
                }
            },
            {
                "name": "mkdir",
                "description": "Create a directory (and any missing parents) within the active project. Path components must contain only a-zA-Z0-9._- ; '/' may be used to create nested paths in one call (e.g. 'src/tools'). Absolute paths and paths escaping the project root are rejected.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Relative directory path to create (e.g. 'src/tools')"
                        }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "ls",
                "description": "List the contents of a directory within the active project. Returns up to 100 entries per page; use 'offset' to paginate.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Relative path to list (defaults to project root '.')"
                        },
                        "offset": {
                            "type": "integer",
                            "description": "Zero-based entry offset for paging (default 0)",
                            "minimum": 0
                        },
                        "show_git": {
                            "type": "boolean",
                            "description": "Include the .git directory in the listing (default false)"
                        }
                    }
                }
            },
            {
                "name": "tree",
                "description": "Show the directory tree of the active project. Returns up to 100 lines per page; use 'offset' to paginate.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Root path for the tree (defaults to project root '.')"
                        },
                        "depth": {
                            "type": "integer",
                            "description": "Maximum traversal depth (defaults to the project's max_depth)",
                            "minimum": 0
                        },
                        "offset": {
                            "type": "integer",
                            "description": "Zero-based line offset for paging (default 0)",
                            "minimum": 0
                        },
                        "show_git": {
                            "type": "boolean",
                            "description": "Include the .git directory in the tree (default false)"
                        }
                    }
                }
            },
            {
                "name": "get_project_info",
                "description": "Return metadata about the active project: language, size preset, max_files, and max_depth. Returns 404-equivalent if no project is active.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "git_status",
                "description": "Show the working-tree status of the active project (equivalent to `git status`). Returns 404-equivalent if no project is active.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "git_log",
                "description": "Show the last N commits of the active project as an oneline graph (equivalent to `git log --oneline --decorate --graph -n N`). Returns 404-equivalent if no project is active.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "n": {
                            "type": "integer",
                            "description": "Number of commits to show (default 10, max 200)",
                            "minimum": 1
                        }
                    }
                }
            },
            {
                "name": "git_diff",
                "description": "Show the diff of the working tree against a git ref (equivalent to `git diff <ref>`). Defaults to HEAD when no ref is supplied. Returns 404-equivalent if no project is active.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "ref": {
                            "type": "string",
                            "description": "Git ref to diff against (e.g. 'HEAD', 'main', a commit SHA). Defaults to HEAD."
                        }
                    }
                }
            },
            {
                "name": "git_diff_staged",
                "description": "Show the diff of staged changes that would go into the next commit (equivalent to `git diff --staged`). Returns 404-equivalent if no project is active.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "build_start",
                "description": "Start build.sh in the background and return a job_id immediately. Use build_status to poll for the result. Returns 409 if a build is already running. Returns 404-equivalent if no project is active or build.sh is missing.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "build_status",
                "description": "Poll the result of a background build started with build_start. Returns 'still running' while the build is in progress. Returns stdout/stderr and exit code when done (the result is consumed on first read). Returns 404 for unknown job_id.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "job_id": {
                            "type": "string",
                            "description": "The job_id returned by build_start"
                        }
                    },
                    "required": ["job_id"]
                }
            }
        ]
    })
}

// ── tools/call dispatcher ────────────────────────────────────────────────────

pub async fn call(state: Arc<Mutex<AppState>>, params: &Value) -> ToolResult {
    let name = params
        .get("name")
        .and_then(|n| n.as_str())
        .ok_or_else(|| (-32602i32, "Missing required parameter: name".to_string()))?;

    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    match name {
        "create_project" => create_project::run(state, &args).await,
        "pwd" => pwd::run(state).await,
        "use_project" => use_project::run(state, &args).await,
        "get" => get::run(state, &args).await,
        "mkdir" => mkdir::run(state, &args).await,
        "ls" => ls::run(state, &args).await,
        "tree" => tree::run(state, &args).await,
        "put" => put::run(state, &args).await,
        "get_project_info" => get_project_info::run(state).await,
        "build_start" => build_start::run(state).await,
        "build_status" => build_status::run(state, &args).await,
        "git_status" => git_status::run(state).await,
        "git_log" => git_log::run(state, &args).await,
        "git_diff" => git_diff::run(state, &args).await,
        "git_diff_staged" => git_diff_staged::run(state).await,
        _ => Err((-32601, format!("Unknown tool: {name}"))),
    }
}
