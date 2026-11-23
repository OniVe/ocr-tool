#requires -Version 5.1
<#
.SYNOPSIS
  Sign Windows release executables with Authenticode (SHA-256, RFC 3161 timestamp).

.DESCRIPTION
  Requires a code-signing certificate exported as PFX and Windows SDK signtool.exe.
  The script signs every requested file, verifies the signature with signtool and
  checks that Get-AuthenticodeSignature reports Valid. Any failure throws.

  The PFX password is never written to the console or to logs.

.EXAMPLE
  ./scripts/sign-windows.ps1 -PfxPath ./cert.pfx -PfxPassword $env:WINDOWS_CERT_PASSWORD
#>
[CmdletBinding()]
param(
    [string[]]$Path = @(
        "target/release/ocr-tool.exe",
        "target/release/ocr-tool-gui.exe"
    ),
    [Parameter(Mandatory = $true)][string]$PfxPath,
    [Parameter(Mandatory = $true)][string]$PfxPassword,
    [string]$TimestampUrl = "http://timestamp.digicert.com"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $PSScriptRoot

function Find-SignTool {
    $kitsBin = Join-Path ${env:ProgramFiles(x86)} "Windows Kits/10/bin"
    if (-not (Test-Path -LiteralPath $kitsBin)) {
        throw "Windows SDK bin directory not found: $kitsBin"
    }

    $signTool = Get-ChildItem -LiteralPath $kitsBin -Directory |
        Where-Object { $_.Name -match '^\d+(\.\d+)+$' } |
        Sort-Object { [version]$_.Name } -Descending |
        ForEach-Object { Join-Path $_.FullName "x64/signtool.exe" } |
        Where-Object { Test-Path -LiteralPath $_ } |
        Select-Object -First 1

    if (-not $signTool) {
        throw "signtool.exe not found under $kitsBin\*\x64. Install the Windows 10/11 SDK."
    }
    return $signTool
}

if ([string]::IsNullOrWhiteSpace($PfxPassword)) {
    throw "PfxPassword is empty; set WINDOWS_CERT_PASSWORD before signing."
}

$pfx = (Resolve-Path -LiteralPath $PfxPath).Path
$signTool = Find-SignTool
Write-Host "signtool: $signTool"

foreach ($item in $Path) {
    $file = $item
    if (-not [System.IO.Path]::IsPathRooted($file)) {
        $file = Join-Path $Root $item
    }
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) {
        throw "File to sign not found: $file"
    }
    $file = (Resolve-Path -LiteralPath $file).Path

    Write-Host "Signing $file"
    & $signTool sign /fd SHA256 /f $pfx /p $PfxPassword /tr $TimestampUrl /td SHA256 $file
    if ($LASTEXITCODE -ne 0) {
        throw "signtool sign failed for $file (exit $LASTEXITCODE)"
    }

    & $signTool verify /pa /v $file
    if ($LASTEXITCODE -ne 0) {
        throw "signtool verify failed for $file (exit $LASTEXITCODE)"
    }

    $signature = Get-AuthenticodeSignature -LiteralPath $file
    if ($signature.Status -ne "Valid") {
        throw "Authenticode status for $file is $($signature.Status), expected Valid"
    }
    Write-Host "Signed and verified: $file"
}

Write-Host "Authenticode signing complete."
