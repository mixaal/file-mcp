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

/// Maximum size (bytes) of a file returned by `get`. Larger files are rejected.
pub const MAX_GET_FILE_SZ: u64 = 512 * 1024;

/// Maximum size (bytes) of content accepted by `put`. Larger payloads are rejected.
pub const MAX_PUT_FILE_SZ: usize = 512 * 1024;
