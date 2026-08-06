#!/bin/sh
# Build the wasm module into www/pkg, where index.html expects it.
set -eu
command -v wasm-pack >/dev/null 2>&1 || cargo install wasm-pack --locked
exec wasm-pack build --target web --out-dir www/pkg "$@"
