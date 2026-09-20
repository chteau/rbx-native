# Publishes one or more images to a secret GitHub gist and prints ready-to-paste
# Markdown links (each file's raw-content URL) for a PR description or review
# comment. Native Windows equivalent of publish-screenshot.sh — see that
# script's own header for the full rationale (gh gist create itself refuses
# binary files; the trick is cloning the gist as a real git repo and pushing
# the image as an ordinary blob instead). Untested on a real Windows machine
# (see README's Platform support table).
#
# Needs only `gh` and `git` — uses PowerShell's own ConvertFrom-Json rather
# than a separate jq dependency.
#
# "Secret" (gh gist create's own default, kept here) means unlisted, not
# private: anyone who gets the link can view it. Fine for an ordinary viewport
# screenshot; don't publish anything sensitive this way.
param(
    [Parameter(Mandatory = $true, ValueFromRemainingArguments = $true)]
    [string[]]$Images
)
$ErrorActionPreference = "Stop"

if ($Images.Count -eq 0) {
    Write-Error "usage: publish-screenshot.ps1 <image> [image...]"
    exit 2
}
foreach ($image in $Images) {
    if (-not (Test-Path -PathType Leaf $image)) {
        Write-Error "not a file: $image"
        exit 1
    }
}

$workDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $workDir | Out-Null
try {
    $stamp = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    $placeholder = Join-Path $workDir "README.md"
    # ASCII on purpose: Windows PowerShell 5.1 reads a BOM-less script as
    # Windows-1252, where the last byte of an em dash is a curly double quote
    # that ends the string and breaks the parse of everything after it.
    "rbx-native PR screenshot(s), published $stamp - see the other file(s) in this gist." |
        Out-File -FilePath $placeholder -Encoding utf8

    $gistUrl = gh gist create --desc "rbx-native PR screenshot(s) - $stamp" $placeholder
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $gistId = ($gistUrl -split "/")[-1]

    $cloneDir = Join-Path $workDir "clone"
    gh gist clone $gistId $cloneDir | Out-Null
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    foreach ($image in $Images) {
        Copy-Item $image (Join-Path $cloneDir (Split-Path $image -Leaf))
    }

    Push-Location $cloneDir
    try {
        git add -A
        git commit -q -m "Add screenshot(s)"
        git push -q origin HEAD
    }
    finally {
        Pop-Location
    }

    Write-Output "Gist: $gistUrl"
    Write-Output ""
    $filesJson = gh api "gists/$gistId" --jq ".files" | ConvertFrom-Json
    foreach ($image in $Images) {
        $name = Split-Path $image -Leaf
        $rawUrl = $filesJson.$name.raw_url
        Write-Output "![$name]($rawUrl)"
    }
}
finally {
    Remove-Item -Recurse -Force $workDir
}
