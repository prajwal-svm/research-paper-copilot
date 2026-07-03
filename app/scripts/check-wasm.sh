#!/bin/sh
# CI check (v5 platform-parity): the core must build for BOTH targets on
# every change — native with the full feature set, and wasm32-wasip1 with
# native-only capabilities excluded at compile time (sandbox, docker,
# keychain, ureq, pdfium, ML stack, sqlite, wasmtime host).
set -e
cd "$(dirname "$0")/.."
echo "== native (default features)"
cargo check -p copilot-core
echo "== wasm32-wasip1 (--no-default-features)"
cargo check -p copilot-core --no-default-features --target wasm32-wasip1
echo "both targets build ✓"
