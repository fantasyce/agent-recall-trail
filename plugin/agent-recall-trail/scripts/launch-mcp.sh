#!/bin/sh
set -eu

# Preserve an explicitly configured PATH installation. GUI and restricted SSH
# hosts may omit the default installer's user-local executable directory.
if command -v art >/dev/null 2>&1; then
    exec art "$@"
fi
if [ -n "${HOME:-}" ] && [ -x "$HOME/.local/bin/art" ]; then
    exec "$HOME/.local/bin/art" "$@"
fi

printf '%s\n' 'ART executable unavailable: install ART or configure an absolute MCP command path.' >&2
exit 127
