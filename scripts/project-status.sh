#!/usr/bin/env bash
#
# Automated project state reporter for the octopus workspace.
# Run from the repository root.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "============================================"
echo "  Octopus Project Status"
echo "============================================"
echo

echo "--- Git ---"
git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "(not a git repository)"
echo "Status:"
git status --short || true
echo

echo "--- Active Task ---"
if [ -f STATUS.md ]; then
    grep -E "^\\*\\*Active Task\\*\\*" STATUS.md || true
else
    echo "STATUS.md not found"
fi
echo

echo "--- Compilation ---"
if cargo check --workspace 2>&1 | tail -5; then
    echo "Result: cargo check --workspace ✅"
else
    echo "Result: cargo check --workspace ❌"
fi
echo

echo "--- Tests ---"
if cargo test -p octopus-cli 2>&1 | tail -10; then
    echo "Result: cargo test -p octopus-cli ✅"
else
    echo "Result: cargo test -p octopus-cli ❌"
fi
echo

echo "--- Task Inventory ---"
echo "Active tasks:"
find tasks -maxdepth 1 -type f -name '*.md' ! -name '_template.md' -printf '  %f\n' 2>/dev/null || true
echo "Completed tasks:"
find tasks/completed -maxdepth 1 -type f -name '*.md' -printf '  %f\n' 2>/dev/null || true
echo

echo "============================================"
echo "  End of report"
echo "============================================"
