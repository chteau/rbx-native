# Builds rbxview's browser version into crates/rbx_viewer/web/pkg — see
# build-web.sh for the prerequisites and why it is `cargo rustc`.
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'target' }
cargo rustc -p rbx_viewer --lib --release --target wasm32-unknown-unknown --crate-type cdylib
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
wasm-bindgen --target web --no-typescript --out-dir crates/rbx_viewer/web/pkg "$target/wasm32-unknown-unknown/release/rbx_viewer.wasm"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Write-Output "built crates/rbx_viewer/web - serve it with: rbxview --serve crates/rbx_viewer/web [place.rbxl]"
