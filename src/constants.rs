/// Number of entries returned per page in ls and tree.
pub const PAGE_SIZE: usize = 100;

/// Default number of commits shown by git_log when `n` is not supplied.
pub const GIT_LOG_DEFAULT_N: u64 = 10;

/// Maximum number of commits git_log will return (clamps the caller's `n`).
pub const GIT_LOG_MAX_N: u64 = 200;

/// Maximum byte-length of a git ref accepted by git_diff.
pub const GIT_REF_MAX_LEN: usize = 200;

/// Shell used to execute build.sh (and run.sh in the future).
pub const SHELL_BIN: &str = "/bin/bash";

/// Absolute path to the git binary used for all git sub-commands.
pub const GIT_BIN: &str = "/usr/bin/git";

/// Absolute path to the cargo binary used for Rust project scaffolding.
/// Resolved from $HOME at build time to pick up the rustup default location.
pub const CARGO_BIN: &str = concat!(env!("HOME"), "/.cargo/bin/cargo");

/// Absolute path to the go binary used for Go project scaffolding.
pub const GO_BIN: &str = "/snap/bin/go";

/// Maximum size (bytes) of a file returned by `get`. Larger files are rejected.
pub const MAX_GET_FILE_SZ: u64 = 512 * 1024;

/// Maximum size (bytes) of content accepted by `put`. Larger payloads are rejected.
pub const MAX_PUT_FILE_SZ: usize = 512 * 1024;

/// Hard ceiling on `max_files` loaded from `.meta/project.json`. Defends against
/// a hand-edited meta file requesting absurd quotas. Well above the Large preset (1000).
pub const MAX_FILES_HARD_CAP: usize = 5000;

/// Hard ceiling on `max_depth` loaded from `.meta/project.json`. Comfortably above
/// the Large / Java preset (7).
pub const MAX_DEPTH_HARD_CAP: usize = 10;
