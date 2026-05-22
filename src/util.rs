use std::path::{Component, Path, PathBuf};

/// Resolve `requested` relative to `base`, rejecting any path that would
/// escape `base` (absolute paths, leading `..`, embedded `..` that pops
/// above `base`).  Returns `None` on any violation.
pub fn safe_path(base: &Path, requested: &str) -> Option<PathBuf> {
    let req = Path::new(requested);
    if req.is_absolute() {
        return None;
    }

    let mut parts: Vec<&std::ffi::OsStr> = Vec::new();
    for component in req.components() {
        match component {
            Component::Normal(c) => parts.push(c),
            Component::ParentDir => {
                if parts.pop().is_none() {
                    return None;
                }
            }
            Component::CurDir => {}
            _ => return None,
        }
    }

    let mut result = base.to_path_buf();
    for part in parts {
        result.push(part);
    }
    Some(result)
}

/// Validate that every component of a put path contains only `[._\-a-zA-Z0-9]`.
/// Returns `Err` with the offending component if validation fails.
/// Empty paths and paths with `..` / absolute roots are also rejected here.
pub fn validate_put_path(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    if p.is_absolute() {
        return Err("absolute paths are not allowed".to_string());
    }
    if path.is_empty() {
        return Err("path must not be empty".to_string());
    }
    for component in p.components() {
        match component {
            Component::Normal(c) => {
                let s = c.to_string_lossy();
                let bad: Vec<char> = s
                    .chars()
                    .filter(|ch| !ch.is_ascii_alphanumeric() && !"._-".contains(*ch))
                    .collect();
                if !bad.is_empty() {
                    return Err(format!(
                        "component '{}' contains disallowed characters: {:?} (only a-zA-Z0-9._- are permitted)",
                        s, bad
                    ));
                }
                if s.is_empty() {
                    return Err("empty path component".to_string());
                }
            }
            Component::ParentDir => {
                return Err("'..'' components are not allowed in put paths".to_string())
            }
            Component::CurDir => {}
            _ => return Err("absolute path components are not allowed".to_string()),
        }
    }
    Ok(())
}

/// Validate a project name: only `[_\-a-zA-Z0-9]` (no dot).
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("name must not be empty".to_string());
    }
    let bad: Vec<char> = name
        .chars()
        .filter(|c| !c.is_ascii_alphanumeric() && *c != '_' && *c != '-')
        .collect();
    if !bad.is_empty() {
        return Err(format!(
            "name contains disallowed characters: {:?} (only a-zA-Z0-9_- are permitted)",
            bad
        ));
    }
    Ok(())
}

/// Verify that `path` is a plain regular file or a plain directory.
/// Uses `symlink_metadata` so symlinks are **never** followed.
/// Returns `Err` for symlinks, device files, FIFOs, sockets, or anything else exotic.
pub fn check_is_regular(path: &std::path::Path) -> Result<(), String> {
    let meta = path
        .symlink_metadata()
        .map_err(|e| format!("cannot stat '{}': {e}", path.display()))?;
    let ft = meta.file_type();
    if ft.is_symlink() {
        return Err(format!(
            "'{}' is a symbolic link — only regular files and directories are allowed",
            path.display()
        ));
    }
    if !ft.is_file() && !ft.is_dir() {
        return Err(format!(
            "'{}' is a special file (device/FIFO/socket) — only regular files and directories are allowed",
            path.display()
        ));
    }
    Ok(())
}

/// Return the number of path components in `path` (only `Normal` segments count).
/// E.g. "src/main.rs" → 2,  "src/tools/mod.rs" → 3.
pub fn path_depth(path: &str) -> usize {
    Path::new(path)
        .components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .count()
}

/// Returns the top-level build-artifact directories for a given language.
/// These directories must not be written to via put/mkdir, and are not
/// counted toward the project file limit.
pub fn excluded_dirs_for(language: &str) -> &'static [&'static str] {
    match language.to_lowercase().as_str() {
        "rust" => &["target"],
        "java" => &["target", "build", ".gradle"],
        "python" => &[".venv", "venv", "__pycache__", "dist", "build"],
        "go" => &["vendor"],
        "javascript" | "js" | "node" | "typescript" | "ts" => &["node_modules", "dist", "build"],
        "c" | "c++" | "cpp" => &["build"],
        "godot" | "godot3d" | "godot-3d" | "godot2d" | "godot-2d" => &[".godot"],
        _ => &[],
    }
}

/// Returns true when the first component of `path` matches an excluded directory.
/// Only the top-level component is checked — `src/target/foo` is not excluded,
/// but `target/foo` is.
pub fn is_excluded_path(path: &str, excluded: &[&str]) -> bool {
    Path::new(path)
        .components()
        .find_map(|c| {
            if let Component::Normal(n) = c {
                Some(n.to_string_lossy().into_owned())
            } else {
                None
            }
        })
        .map(|first| excluded.iter().any(|e| *e == first.as_str()))
        .unwrap_or(false)
}

/// Directories never counted toward the project file limit, regardless of
/// language. `.git` alone holds hundreds of objects/hooks/refs that would
/// otherwise dwarf the real source tree.
pub const ALWAYS_EXCLUDED_DIRS: &[&str] = &[".git"];

/// Recursively count regular files under `dir`, skipping any top-level
/// subdirectory whose name appears in `excluded` (synchronous — call via spawn_blocking).
/// `.git` is always skipped at every level via [`ALWAYS_EXCLUDED_DIRS`].
pub fn count_files_sync(dir: &Path, excluded: &[&str]) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    entries.filter_map(|e| e.ok()).fold(0, |acc, entry| {
        let p = entry.path();
        if p.is_file() {
            acc + 1
        } else if p.is_dir() {
            let skip = p
                .file_name()
                .map(|n| {
                    let name = n.to_string_lossy();
                    ALWAYS_EXCLUDED_DIRS.iter().any(|e| *e == name.as_ref())
                        || excluded.iter().any(|e| *e == name.as_ref())
                })
                .unwrap_or(false);
            // Language-specific excluded dirs only apply at the top level, but
            // ALWAYS_EXCLUDED_DIRS (e.g. `.git`) is reapplied at every depth.
            if skip { acc } else { acc + count_files_sync(&p, &[]) }
        } else {
            acc
        }
    })
}

/// Keep only `a-zA-Z0-9- ` (space allowed), collapse runs of spaces to one,
/// trim leading/trailing whitespace, then truncate to 120 characters.
pub fn sanitize_message(msg: &str) -> String {
    let filtered: String = msg
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == ' ')
        .collect();
    // Collapse consecutive spaces and trim.
    let collapsed = filtered.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.len() > 120 { collapsed[..120].to_string() } else { collapsed }
}

/// Run a git sub-command in `dir`.  Returns stdout on success, stderr on failure.
///
/// FIXME: `Command::output()` uses `Stdio::piped()` internally and buffers the
/// full stdout/stderr into memory before returning. The truncation below only
/// bounds the response we hand back — peak memory during git execution is still
/// proportional to the raw output size. To cap peak memory we'd need to spawn
/// with explicit piped stdio and stream-read with an early stop / kill once the
/// cap is reached. Deferred as too heavy for the current threat model.
pub async fn run_git(
    git_cmd: &Path,
    dir: &Path,
    args: &[&str],
) -> Result<String, String> {
    let output = tokio::process::Command::new(git_cmd)
        .args(args)
        .current_dir(dir)
        .output()
        .await
        .map_err(|e| format!("failed to spawn git: {e}"))?;

    if output.status.success() {
        Ok(truncate_output(&output.stdout))
    } else {
        Err(truncate_output(&output.stderr))
    }
}

/// Lossy-convert raw bytes to String and cap at `MAX_GIT_OUTPUT_BYTES`, appending
/// a sentinel if truncation happened. Cuts at a UTF-8 char boundary.
fn truncate_output(bytes: &[u8]) -> String {
    let mut s = String::from_utf8_lossy(bytes).into_owned();
    if s.len() > crate::constants::MAX_GIT_OUTPUT_BYTES {
        let mut cut = crate::constants::MAX_GIT_OUTPUT_BYTES;
        while cut > 0 && !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push_str(&format!(
            "\n... (truncated at {} bytes)",
            crate::constants::MAX_GIT_OUTPUT_BYTES
        ));
    }
    s
}
