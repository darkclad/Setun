#!/bin/sh
# Rebuild the browser front end: web/setun.wasm from crates/web.
# Serve with:  python -m http.server 8386 --directory web
set -e
cd "$(dirname "$0")/.."
cargo build --release -p setun-web --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/setun_web.wasm web/setun.wasm
ls -l web/setun.wasm
