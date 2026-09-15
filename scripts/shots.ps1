# Renders the reference fixtures from fixed cameras into -Out (default:
# ./shots) so two builds can be compared side by side. Fixtures live outside
# the repo in $env:RBX_FIXTURES (default: ../rbx-native-fixtures/places).
# Native Windows equivalent of shots.sh — untested on a real Windows machine
# (see README's Platform support table).
param(
    [string]$Out = "shots"
)
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Fix = if ($env:RBX_FIXTURES) { $env:RBX_FIXTURES } else { "../rbx-native-fixtures/places" }

# No ionice/nice equivalent attempted here — see check.ps1's own note.
cargo build --release -p rbx_viewer 2>&1 | Select-String -Pattern '^(error|warning)'

$Viewer = Join-Path "target" (Join-Path "release" "rbxview.exe")

function Invoke-Shot {
    param(
        [string]$Name,
        [string[]]$ViewerArgs
    )
    Write-Output "-- $Name"
    & $Viewer @ViewerArgs --screenshot (Join-Path $Out "$Name.png") --size 960x540
    if ($LASTEXITCODE -ne 0) { Write-Output "FAILED: $Name" }
}

Invoke-Shot -Name "testplace_spawn" -ViewerArgs @(
    "assets/tests/TestPlace.rbxl", "--eye", "14,8,14", "--look-at", "0,1,0"
)
Invoke-Shot -Name "testplace_wide" -ViewerArgs @(
    "assets/tests/TestPlace.rbxl", "--eye", "60,30,60", "--look-at", "0,0,0"
)
Invoke-Shot -Name "testrust_overview" -ViewerArgs @(
    (Join-Path $Fix "testrust_17675488706.rbxl")
)
Invoke-Shot -Name "marked_overview" -ViewerArgs @(
    (Join-Path $Fix "marked.rbxl")
)
Invoke-Shot -Name "marked_ground" -ViewerArgs @(
    (Join-Path $Fix "marked.rbxl"), "--eye", "40,12,40", "--look-at", "0,4,0"
)
# Same camera as the user's Studio capture of this place (CFrame position and
# orientation converted to eye/look-at), so the two can be compared directly.
Invoke-Shot -Name "demo_global" -ViewerArgs @(
    (Join-Path $Fix "DEMO_LIGHTING_MATERIALS.rbxl"),
    "--eye", "-121.423,54.07,-126.14", "--look-at", "-64.1,7.5,-58.7"
)
Invoke-Shot -Name "demo_night" -ViewerArgs @(
    (Join-Path $Fix "DEMO_LIGHTING_MATERIALS.rbxl"),
    "--eye", "-121.423,54.07,-126.14", "--look-at", "-64.1,7.5,-58.7", "--clock-time", "0"
)
