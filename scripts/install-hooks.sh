#!/usr/bin/env bash
# Install git hooks for this workspace.
set -euo pipefail

HOOK_DIR="$(git rev-parse --git-dir)/hooks"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

ln -sf "$SCRIPT_DIR/pre-commit" "$HOOK_DIR/pre-commit"
echo "✓ pre-commit hook installed"
