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
