# file-mcp

A small MCP (Model Context Protocol) server that lets a model scaffold, read,
write, build, and git-manage a single project directory at a time. Written in
Rust on axum + tokio.

## Transports

Stdio is the default — no network exposure.

```bash
cargo build --release
./target/release/file-mcp            # stdio (default)
./target/release/file-mcp --http     # HTTP on 127.0.0.1:$PORT (default 3000)
```

The `--http` listener binds to loopback only. There is **no authentication** on
the HTTP transport; it assumes a single-user, single-host setup.

## Environment

| Variable | Purpose                                                    |
|----------|------------------------------------------------------------|
| `PRJ_DIR`| Base directory under which projects live (default: CWD).   |
| `PORT`   | Port for `--http` (default 3000).                          |

## Tools

File / project:
`create_project`, `use_project`, `pwd`, `get`, `put`, `mkdir`, `ls`, `tree`,
`get_project_info`

Git:
`git_status`, `git_log`, `git_diff`, `git_diff_staged`

Build (see SECURITY NOTE below):
`build_start`, `build_status`, `build_kill`

See `src/tools/mod.rs` for the full JSON-Schema descriptions the server
returns from `tools/list`.

## SECURITY NOTE: build.sh is intentional arbitrary code execution

`create_project` scaffolds a project-root `build.sh` (and `run.sh`). The
`build_start` tool launches `build.sh` via `/bin/bash` inside the active
project directory. **Anything in that shell script runs with the server's
privileges.**

This is by design — the whole point of the tool is to build real projects.
The sensitive consequence is that any client who can write to `build.sh`
(i.e. via `put`) has full RCE on the host. The mitigations we rely on are:

- Default transport is stdio (no network reach).
- `--http` binds `127.0.0.1` only.
- Runs as the user that started the server — don't run as root.
- Don't expose either transport to an untrusted principal.

If you plan to relax these assumptions, gate `build_start` behind
authentication or sandbox the spawned shell before doing so.

### Tools the generated scripts rely on (via `$PATH`)

The `build.sh` / `run.sh` emitted by `create_project` shell out to these
language toolchains. Users should sanity-check their `$PATH` — these resolve
at script-execution time, so a hijacked `$PATH` means hijacked build:

| Language | build.sh uses                         | run.sh uses                     |
|----------|---------------------------------------|---------------------------------|
| Rust     | `cargo`                               | `cargo`                         |
| Go       | `go`                                  | `go`                            |
| Java     | `mvn`                                 | `mvn`                           |
| Python   | `python3`, `.venv/bin/pip` (relative) | `.venv/bin/python` / `python3`  |
| C        | `make` (Makefile uses `gcc`, `rm`)    | the built binary (relative)     |
| C++      | `cmake`, optionally `ninja`           | built binary (relative)         |
| JS/Node  | `npm`                                 | `npm`                           |
| TS       | `npm`, `npx` (`tsc` via `npx`)        | `node`                          |
| Godot    | `$GODOT` env or `godot`               | same                            |

The Rust-side spawns of `cargo` / `go` / `git` in `create_project` itself are
pinned to absolute paths (see "Hardcoded paths" below) — only the generated
scripts depend on `$PATH`.

## .meta/ directory

Each project has a `.meta/project.json` that records:

```json
{ "name": "...", "language": "...", "size": "...", "max_files": N, "max_depth": M }
```

This file is **user-editable**: if you want to raise the quotas beyond what
`create_project` chose, hand-edit it. On `use_project` the values are clamped
to hard caps defined in `src/constants.rs`:

- `MAX_FILES_HARD_CAP` (5000)
- `MAX_DEPTH_HARD_CAP` (10)

Values above the cap are silently reduced. This protects against an absurd or
malicious number disabling the file-count / depth checks.

MCP clients cannot rewrite `.meta/` — `put` and `mkdir` reject any path whose
top-level component is `.meta`. That keeps a client from lifting its own
limits via the tool API.

## Limits and caps

Tunable in `src/constants.rs` (rebuild to change):

| Constant                | Value     | What it bounds                               |
|-------------------------|-----------|----------------------------------------------|
| `MAX_GET_FILE_SZ`       | 512 KiB   | Largest file `get` will return.              |
| `MAX_PUT_FILE_SZ`       | 512 KiB   | Largest payload `put` will accept.           |
| `MAX_FILES_HARD_CAP`    | 5000      | Upper clamp on `.meta` `max_files`.          |
| `MAX_DEPTH_HARD_CAP`    | 10        | Upper clamp on `.meta` `max_depth` + `tree`. |
| `MAX_TREE_LINES`        | 10_000    | Hard stop on `tree` traversal output.        |
| `MAX_GIT_OUTPUT_BYTES`  | 1 MiB     | Truncation point for `run_git` stdout/stderr.|
| `GIT_LOG_MAX_N`         | 200       | Max `-n` for `git_log`.                      |
| `GIT_REF_MAX_LEN`       | 200       | Max length of a ref accepted by `git_diff`.  |
| `PAGE_SIZE`             | 100       | Entries per page in `ls` / `tree`.           |

## Hardcoded paths

The following binaries are pinned to absolute paths at compile time (in
`src/constants.rs`) rather than looked up via `$PATH`. This avoids `$PATH`
hijacking in the server process itself:

| Constant     | Default                       |
|--------------|-------------------------------|
| `SHELL_BIN`  | `/bin/bash`                   |
| `GIT_BIN`    | `/usr/bin/git`                |
| `CARGO_BIN`  | `$HOME/.cargo/bin/cargo`*     |
| `GO_BIN`     | `/snap/bin/go`                |

\* `CARGO_BIN` uses `env!("HOME")` at build time, so it's resolved for whoever
compiled the binary.

If any of these live in a different place on your host (e.g. distro-packaged
`cargo` at `/usr/bin/cargo`, Go installed from tarball at `/usr/local/go/bin/go`,
`git` at `/usr/local/bin/git`), edit `src/constants.rs` and rebuild. There is
no runtime override.

## Running with the MCP Inspector

The repo ships a helper script that builds, starts the server on `--http`,
and launches the inspector:

```bash
./run-model-inspector.sh
```

The script waits for the server to answer a `ping` before opening the
inspector UI. Ctrl-C tears down both.
