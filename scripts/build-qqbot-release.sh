#!/usr/bin/env bash
set -euo pipefail

# Build a qqbot release tarball.
# Usage:
#   ./scripts/build-qqbot-release.sh [output-dir]

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${1:-$ROOT/dist}"
RELEASE_DIR="$OUT_DIR/qqbot-linux-x86_64"

mkdir -p "$RELEASE_DIR"

echo "Building qqbot and qqbot-core..."
cd "$ROOT"
cargo build --release -p qqbot -p qqbot-core

echo "Building summary plugin..."
cargo build --release -p summary --target wasm32-unknown-unknown

echo "Copying binaries and plugin..."
cp "$ROOT/target/release/qqbot" "$RELEASE_DIR/"
cp "$ROOT/target/release/qqbot-core" "$RELEASE_DIR/"
mkdir -p "$RELEASE_DIR/plugins"
cp "$ROOT/target/wasm32-unknown-unknown/release/summary.wasm" "$RELEASE_DIR/plugins/"

echo "Creating tarball..."
cd "$OUT_DIR"
tar czf "qqbot-linux-x86_64.tar.gz" "qqbot-linux-x86_64"

echo "Release ready: $OUT_DIR/qqbot-linux-x86_64.tar.gz"
