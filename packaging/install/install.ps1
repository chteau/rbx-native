# RbxNative installer for Windows (x64).
#
#   irm https://rbxnative.chteau.bzh/install.ps1 | iex
#
# A version:
#   $env:RBXNATIVE_VERSION = '0.2.0'
#   irm https://rbxnative.chteau.bzh/install.ps1 | iex
# Uninstalling (keeps settings, places and saved keys):
#   $s = irm https://rbxnative.chteau.bzh/install.ps1
#   & ([scriptblock]::Create($s)) -Uninstall
# (-Version 0.2.0 works the same way.)
#
# Installs for the current user only, no admin, to the same folder as the
# setup (%LOCALAPPDATA%\Programs\RbxNative) with a Start menu shortcut. The
# download is checked against the release's SHA256SUMS, and that file
# against its minisign signature when minisign is installed. RbxNative keeps
# itself up to date from there.
param(
    [string]$Version = $env:RBXNATIVE_VERSION,
    [switch]$Uninstall
)

function Install-RbxNative {
    param([string]$Version, [bool]$Uninstall)

    $ErrorActionPreference = 'Stop'
    # Windows PowerShell's progress bar makes downloads many times slower.
    $ProgressPreference = 'SilentlyContinue'
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $repo = 'chteau/rbx-native'
    # The release signing key, the same one the editor's updater trusts
    # (crates/rbx_studio/src/updater/verify.rs).
    $publicKey = 'RWTH57MZqQ9zSpsJv14IQQqWjzpg9EdLyKUQ610z7gC7zShLMJb5kuJ+'
    $target = 'x86_64-pc-windows-msvc'
    $dir = Join-Path $env:LOCALAPPDATA 'Programs\RbxNative'
    $exe = Join-Path $dir 'RbxNative.exe'
    $shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'RbxNative.lnk'
    $setupUninstaller = Join-Path $dir 'unins000.exe'

    function Assert-NotRunning {
        $running = Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($dir, [StringComparison]::OrdinalIgnoreCase) }
        if ($running) { throw 'RbxNative is running. Close it and run this again.' }
    }

    if ($Uninstall) {
        if (Test-Path $setupUninstaller) {
            Write-Host 'RbxNative was installed by the setup, running its uninstaller.'
            Start-Process $setupUninstaller -ArgumentList '/SILENT' -Wait
            return
        }
        $removed = $false
        Assert-NotRunning
        if (Test-Path $shortcut) { Remove-Item $shortcut -Force; Write-Host "Removed $shortcut"; $removed = $true }
        if (Test-Path $dir) { Remove-Item $dir -Recurse -Force; Write-Host "Removed $dir"; $removed = $true }
        if ($removed) { Write-Host 'RbxNative is uninstalled. Your settings and places are still there.' }
        else { Write-Host "RbxNative isn't installed here." }
        return
    }

    if (-not [Environment]::Is64BitOperatingSystem) { throw 'RbxNative needs 64-bit Windows.' }
    if (Test-Path $setupUninstaller) {
        throw "RbxNative was installed by the setup in $dir and updates itself. To reinstall, run the setup again."
    }

    $Version = "$Version".TrimStart('v')
    # RBX_UPDATE_API points at a local mock, as it does for the updater.
    $root = if ($env:RBX_UPDATE_API) { $env:RBX_UPDATE_API } else { 'https://api.github.com' }
    $api = if ($Version) { "$root/repos/$repo/releases/tags/v$Version" } else { "$root/repos/$repo/releases/latest" }
    try { $release = Invoke-RestMethod -Uri $api -Headers @{ 'User-Agent' = 'rbxnative-install' } }
    catch { throw "Couldn't read the release from GitHub ($api): $($_.Exception.Message)" }
    $tag = $release.tag_name
    $Version = $tag.TrimStart('v')
    $asset = "RbxNative-$Version-$target.zip"
    function Get-AssetUrl([string]$name) {
        $found = $release.assets | Where-Object { $_.name -eq $name } | Select-Object -First 1
        if (-not $found) { throw "Release $tag has no $name" }
        $found.browser_download_url
    }
    $null = Get-AssetUrl $asset

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("rbxnative-install-" + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        Write-Host "Downloading RbxNative $Version ($asset)"
        Invoke-WebRequest -UseBasicParsing -Uri (Get-AssetUrl $asset) -OutFile (Join-Path $tmp $asset)
        Invoke-WebRequest -UseBasicParsing -Uri (Get-AssetUrl 'SHA256SUMS') -OutFile (Join-Path $tmp 'SHA256SUMS')

        if (Get-Command minisign -ErrorAction SilentlyContinue) {
            Invoke-WebRequest -UseBasicParsing -Uri (Get-AssetUrl 'SHA256SUMS.minisig') -OutFile (Join-Path $tmp 'SHA256SUMS.minisig')
            & minisign -Vq -P $publicKey -m (Join-Path $tmp 'SHA256SUMS') -x (Join-Path $tmp 'SHA256SUMS.minisig')
            if ($LASTEXITCODE -ne 0) { throw 'SHA256SUMS is not signed by the RbxNative release key. Nothing was installed.' }
            Write-Host 'Signature OK (minisign)'
        } else {
            Write-Host "minisign isn't installed, so only the checksum is checked."
        }
        $line = Get-Content (Join-Path $tmp 'SHA256SUMS') | Where-Object { ($_ -split '\s+', 2)[1] -replace '^\*', '' -eq $asset } | Select-Object -First 1
        if (-not $line) { throw "SHA256SUMS has no line for $asset" }
        $expected = ($line -split '\s+')[0].ToLowerInvariant()
        $actual = (Get-FileHash -Algorithm SHA256 (Join-Path $tmp $asset)).Hash.ToLowerInvariant()
        if ($actual -ne $expected) { throw "$asset doesn't match its checksum. Nothing was installed." }
        Write-Host 'Checksum OK'

        Expand-Archive -Path (Join-Path $tmp $asset) -DestinationPath (Join-Path $tmp 'unpacked')
        $package = Get-ChildItem (Join-Path $tmp 'unpacked') -Directory | Select-Object -First 1
        if (-not $package -or -not (Test-Path (Join-Path $package.FullName 'RbxNative.exe'))) { throw 'The package has no RbxNative.exe' }

        Assert-NotRunning
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
        foreach ($item in Get-ChildItem $package.FullName) {
            $dest = Join-Path $dir $item.Name
            # A loaded network-simulator driver can't be overwritten, and
            # these never change between releases.
            if ($item.Name -like 'WinDivert*' -and (Test-Path $dest)) { continue }
            Copy-Item $item.FullName $dest -Recurse -Force
        }

        $shell = New-Object -ComObject WScript.Shell
        $link = $shell.CreateShortcut($shortcut)
        $link.TargetPath = $exe
        $link.WorkingDirectory = $dir
        $link.Description = 'RbxNative'
        $link.Save()
    } finally {
        Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }

    Write-Host ''
    Write-Host "RbxNative $Version is installed in $dir."
    Write-Host 'Open it from the Start menu.'
}

Install-RbxNative -Version $Version -Uninstall $Uninstall.IsPresent
