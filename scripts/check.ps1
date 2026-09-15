# Full quality gate: formatting, lints (warnings fatal), tests. Native Windows
# equivalent of check.sh — no PowerShell analogue of Linux's ionice/nice CPU/IO
# priority hint is attempted here; it's a desktop-friendliness nicety on Linux,
# not a correctness requirement, so it's simply left out on this platform.
#
# Untested on a real Windows machine (see README's Platform support table) —
# report exactly what breaks rather than silently working around it, the same
# standing invitation the rest of this project's Windows story already makes.
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

cargo fmt --all -- --check
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

cargo clippy --workspace --all-targets -- -D warnings
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$logFile = Join-Path ([System.IO.Path]::GetTempPath()) "rbx-native-test.log"
cargo test --workspace @args 2>&1 |
    Tee-Object -FilePath $logFile |
    Select-String -Pattern '^(test result|running|error|warning: unused)'
$testExitCode = $LASTEXITCODE

$total = 0
Select-String -Path $logFile -Pattern '^test result: ok\. (\d+) passed' | ForEach-Object {
    $total += [int]$_.Matches[0].Groups[1].Value
}
Write-Output "TOTAL PASSED: $total"

if ($testExitCode -ne 0) { exit $testExitCode }
