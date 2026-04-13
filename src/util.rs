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

/// Recursively count regular files under `dir` (synchronous — call via spawn_blocking).
pub fn count_files_sync(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    entries.filter_map(|e| e.ok()).fold(0, |acc, entry| {
        let p = entry.path();
        if p.is_file() { acc + 1 } else if p.is_dir() { acc + count_files_sync(&p) } else { acc }
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
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}
