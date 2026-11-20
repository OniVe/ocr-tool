#requires -Version 5.1
<#
.SYNOPSIS
  Check the PE subsystem of the OCR binaries.

.DESCRIPTION
  The release GUI must be a Windows-subsystem executable (2) so a double-click
  opens only the GUI window, without a console. The CLI must remain a console
  executable (3) so `ocr-tool --help` and piped output work in a terminal.

  Reads the DOS header (e_lfanew at 0x3C), the PE signature, the optional
  header magic (PE32 0x10B / PE32+ 0x20B) and the Subsystem field at optional
  header offset 68 (0x44).

  Exit code 0 when both binaries match, 1 otherwise.
#>
[CmdletBinding()]
param(
    [string]$Gui,
    [string]$Cli
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
if (-not $Gui) { $Gui = Join-Path $Root "target/release/ocr-tool-gui.exe" }
if (-not $Cli) { $Cli = Join-Path $Root "target/release/ocr-tool.exe" }

function Get-PeSubsystem {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "file not found: $Path"
    }
    $bytes = [IO.File]::ReadAllBytes($Path)
    if ($bytes.Length -lt 0x40 -or $bytes[0] -ne 0x4D -or $bytes[1] -ne 0x5A) {
        throw "not a PE file (missing MZ header): $Path"
    }
    $peOffset = [BitConverter]::ToInt32($bytes, 0x3C)
    if ($peOffset -lt 0 -or ($peOffset + 24 + 70) -gt $bytes.Length) {
        throw "invalid e_lfanew 0x$('{0:X}' -f $peOffset): $Path"
    }
    if ($bytes[$peOffset] -ne 0x50 -or $bytes[$peOffset + 1] -ne 0x45 -or
        $bytes[$peOffset + 2] -ne 0x00 -or $bytes[$peOffset + 3] -ne 0x00) {
        throw "missing PE signature: $Path"
    }
    $optional = $peOffset + 24
    $magic = [BitConverter]::ToUInt16($bytes, $optional)
    if ($magic -ne 0x10B -and $magic -ne 0x20B) {
        throw ("unknown optional header magic 0x{0:X}: {1}" -f $magic, $Path)
    }
    return [BitConverter]::ToUInt16($bytes, $optional + 68)
}

$checks = @(
    [pscustomobject]@{ Name = "ocr-tool-gui.exe"; Path = $Gui; Expected = 2; Label = "Windows GUI" },
    [pscustomobject]@{ Name = "ocr-tool.exe"; Path = $Cli; Expected = 3; Label = "Windows console" }
)

$failures = 0
foreach ($check in $checks) {
    try {
        $actual = Get-PeSubsystem -Path $check.Path
        if ($actual -eq $check.Expected) {
            Write-Output ("{0}: subsystem {1} ({2}) - PASS [{3}]" -f $check.Name, $actual, $check.Label, $check.Path)
        } else {
            $failures++
            Write-Output ("{0}: subsystem {1}, expected {2} ({3}) - FAIL [{4}]" -f $check.Name, $actual, $check.Expected, $check.Label, $check.Path)
        }
    } catch {
        $failures++
        Write-Output ("{0}: {1} - FAIL" -f $check.Name, $_.Exception.Message)
    }
}

if ($failures -ne 0) {
    Write-Host "SUBSYSTEM CHECK FAILED: $failures of $($checks.Count) checks failed"
    exit 1
}
Write-Host "SUBSYSTEM CHECK PASSED: GUI=2 (Windows GUI), CLI=3 (console)"
exit 0
