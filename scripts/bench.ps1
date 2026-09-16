# Times scene reload and rendering on this machine's real GPU and writes both a
# table and target/bench/bench.json. Every argument is forwarded to the harness
# (`--help` lists them); the fixture pair below is only the default.
#
# Deliberately NOT called from check.ps1: this needs a GPU, takes minutes, and
# reports a number rather than a pass or a fail. BENCHMARKS.md at the repo root
# holds the recorded baseline.
#
# Native Windows equivalent of bench.sh — no PowerShell analogue of Linux's
# ionice/nice is attempted here, see check.ps1's own note. Untested on a real
# Windows machine (see README's Platform support table).
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

$Out = Join-Path "target" "bench"
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Fix = if ($env:RBX_FIXTURES) { $env:RBX_FIXTURES } else { "../rbx-native-fixtures/places" }

cargo build --release --example bench -p rbx_viewer 2>&1 | Select-String -Pattern '^(error|warning)'
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$Bench = Join-Path "target" (Join-Path "release" (Join-Path "examples" "bench.exe"))
& $Bench `
    --fixture (Join-Path "assets" (Join-Path "tests" "TestPlace.rbxl")) `
    --fixture (Join-Path $Fix "marked.rbxl") `
    --json (Join-Path $Out "bench.json") `
    @args
exit $LASTEXITCODE
