#requires -Version 5.1
<#
.SYNOPSIS
  Build and package ocr-tool for Windows (or another host) as a zip.
  The GGUF model is deliberately NOT included.
#>
[CmdletBinding()]
param(
    [string]$Target = "windows-x64",
    [switch]$NoBuild
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$Version = (Select-String -Path (Join-Path $Root "Cargo.toml") -Pattern '^version = "([^"]+)"' |
    Select-Object -First 1).Matches[0].Groups[1].Value
if (-not $Version) { throw "cannot read workspace version from Cargo.toml" }

Push-Location $Root
try {
    if (-not $NoBuild) {
        Write-Host "cargo build --release"
        cargo build --release
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    }

    $runtime = Join-Path $Root "runtime/$Target"
    if (-not (Test-Path $runtime)) {
        throw "Runtime not found: $runtime. Run scripts/fetch-runtime.ps1 -Target $Target first."
    }

    $stage = Join-Path $Root "dist/ocr-tool-$Version-$Target"
    if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
    New-Item -ItemType Directory -Force -Path $stage | Out-Null

    $exeSuffix = ""
    if ($Target -like "windows-*") { $exeSuffix = ".exe" }
    foreach ($name in @("ocr-tool$exeSuffix", "ocr-tool-gui$exeSuffix")) {
        $source = Join-Path $Root "target/release/$name"
        if (-not (Test-Path $source)) { throw "Missing build artifact: $source" }
        Copy-Item $source $stage
    }
    Copy-Item (Join-Path $Root "README.md") $stage
    Copy-Item (Join-Path $Root "README.ru.md") $stage
    Copy-Item (Join-Path $Root "NOTICE") $stage
    Copy-Item -Recurse $runtime (Join-Path $stage "runtime/$Target")

    $zip = Join-Path $Root "dist/ocr-tool-$Version-$Target.zip"
    if (Test-Path $zip) { Remove-Item -Force $zip }
    Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $zip -CompressionLevel Optimal
    Write-Host "Package: $zip"
    Write-Host "Contents: executables + runtime/$Target + docs; model not included."
    Write-Output $zip
}
finally {
    Pop-Location
}
