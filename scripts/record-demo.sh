#!/usr/bin/env bash
# Build airprowl and record docs/demo.gif via vhs, all inside Docker.
# No local Rust/vhs needed — just Docker.
set -euo pipefail
cd "$(dirname "$0")/.."

mkdir -p docs
docker build -f Dockerfile.demo -t airprowl-demo .
docker run --rm -v "$PWD/docs:/out" airprowl-demo
echo "✓ wrote docs/demo.gif"
