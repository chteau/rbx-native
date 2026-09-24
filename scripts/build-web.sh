#!/usr/bin/env bash
# Builds rbxview's browser version into crates/rbx_viewer/web/pkg, next to the
# page that loads it. Needs the wasm target and a wasm-bindgen CLI of the same
# version as the `wasm-bindgen` crate in Cargo.lock:
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version <that version> --locked
# Then serve it (the page cannot reach Roblox's CDNs itself — see
# crates/rbx_viewer/src/serve.rs):
#   rbxview --serve crates/rbx_viewer/web [place.rbxl]
set -euo pipefail
cd "$(dirname "$0")/.."
target="${CARGO_TARGET_DIR:-target}"
# `cargo rustc` rather than a `cdylib` in Cargo.toml: every native build of
# the crate would otherwise link a shared library nobody loads.
cargo rustc -p rbx_viewer --lib --release --target wasm32-unknown-unknown --crate-type cdylib
wasm-bindgen --target web --no-typescript \
  --out-dir crates/rbx_viewer/web/pkg \
  "$target/wasm32-unknown-unknown/release/rbx_viewer.wasm"
echo "built crates/rbx_viewer/web — serve it with: rbxview --serve crates/rbx_viewer/web [place.rbxl]"
