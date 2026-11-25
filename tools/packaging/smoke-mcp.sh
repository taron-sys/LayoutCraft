#!/usr/bin/env bash
# Starts a built layoutcraft-mcp, sends "initialize" and checks that it answers as LayoutCraft.
# Usage: smoke-mcp.sh <path to layoutcraft-mcp[.exe]>
set -euo pipefail
bin="${1:?usage: smoke-mcp.sh <layoutcraft-mcp>}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
request='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"release-smoke-test","version":"0"}}}'
answer="$(printf '%s\n' "$request" | "$bin" --root "$work")"
case "$answer" in
  *'"serverInfo"'*'"layoutcraft"'*) printf 'smoke test ok: %s\n' "$bin" ;;
  *)
    printf 'error: unexpected answer from %s: %s\n' "$bin" "$answer" >&2
    exit 1
    ;;
esac
