#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PORT="${PORT:-3000}"
SERVER_URL="http://localhost:${PORT}/mcp"

# ── build ─────────────────────────────────────────────────────────────────────
echo "==> Building file-mcp..."
cargo build --manifest-path "${SCRIPT_DIR}/Cargo.toml"

# ── start server ──────────────────────────────────────────────────────────────
echo "==> Starting file-mcp on port ${PORT}..."
"${SCRIPT_DIR}/target/debug/file-mcp" &
SERVER_PID=$!
trap 'echo; echo "==> Stopping file-mcp (PID ${SERVER_PID})..."; kill "${SERVER_PID}" 2>/dev/null || true' EXIT INT TERM

# ── wait until the server is accepting requests ───────────────────────────────
echo -n "    Waiting for server"
READY=0
for _ in $(seq 1 20); do
    if curl -sf -X POST "${SERVER_URL}" \
            -H "Content-Type: application/json" \
            -d '{"jsonrpc":"2.0","method":"ping","id":1}' \
            >/dev/null 2>&1; then
        READY=1
        break
    fi
    echo -n "."
    sleep 0.3
done
echo

if [[ "${READY}" -eq 0 ]]; then
    echo "ERROR: server did not become ready in time — check output above." >&2
    exit 1
fi

echo "    Server is up at ${SERVER_URL}"

# ── launch inspector ──────────────────────────────────────────────────────────
echo
echo "==> Launching MCP Inspector (via npx)..."
echo
echo "    When the browser opens, configure the connection as:"
echo "      Transport : Streamable HTTP"
echo "      URL       : ${SERVER_URL}"
echo
echo "    Press Ctrl-C to stop everything."
echo

npx --yes @modelcontextprotocol/inspector
