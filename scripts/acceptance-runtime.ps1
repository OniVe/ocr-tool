#requires -Version 5.1
<#
.SYNOPSIS
  Real end-to-end test of `ocr-tool runtime download` (network + ~69 MB).

.DESCRIPTION
  Downloads the pinned llama.cpp b12535 + PDFium 8165 builds into a temporary
  runtime folder, then checks:
    * `runtime download` exit 0 and `runtime verify` exit 0;
    * `llama-server --version` reports build b12535;
    * a second `runtime download` reports the components as already present;
    * `doctor --llama-dir` sees a GPU build, a CPU build and PDFium;
    * CPU OCR of out/acceptance/cpu_crop.jpg exits 0 and returns Russian text.

  Inputs are read from the maintainer-local assets (.tmp/models, the accepted
  CPU crop). Nothing in the repository is modified. Run from the repo root:
    pwsh -File scripts/acceptance-runtime.ps1
  The downloaded runtime is kept on disk unless -CleanRuntime is passed.
#>
[CmdletBinding()]
param(
    [string]$Dir = "",
    [string]$ModelDir = "",
    [string]$CpuImage = "",
    [switch]$SkipBuild,
    [switch]$ForceRedownload,
    [switch]$CleanRuntime
)

$ErrorActionPreference = "Stop"
try {
    [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
    $OutputEncoding = [System.Text.UTF8Encoding]::new()
} catch { }

$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root
$Exe = Join-Path $Root "target/release/ocr-tool.exe"
if (-not $Dir) { $Dir = Join-Path $env:TEMP ("ocr-tool-runtime-e2e-" + $PID) }
if (-not $ModelDir) { $ModelDir = Join-Path $Root ".tmp/models" }
if (-not $CpuImage) { $CpuImage = Join-Path $Root "out/acceptance/cpu_crop.jpg" }

$script:Failures = 0
$script:Checks = 0
function Check {
    param([string]$Name, [bool]$Ok, [string]$Detail = "")
    $script:Checks++
    if ($Ok) {
        Write-Host "[PASS] $Name" -ForegroundColor Green
    } else {
        $script:Failures++
        Write-Host "[FAIL] $Name $Detail" -ForegroundColor Red
    }
}

if (-not $SkipBuild) {
    Write-Host "== cargo build --release =="
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed" }
}
if (-not (Test-Path $Exe)) { throw "Missing $Exe (run without -SkipBuild)" }
foreach ($asset in @($ModelDir, $CpuImage)) {
    if (-not (Test-Path $asset)) { throw "Acceptance asset missing: $asset" }
}

$LogRoot = Join-Path $env:TEMP ("ocr-tool-runtime-logs-" + $PID)
New-Item -ItemType Directory -Force -Path $LogRoot | Out-Null
$DownloadLog = Join-Path $LogRoot "runtime-download.log"
$ReuseLog = Join-Path $LogRoot "runtime-reuse.log"

Write-Host "`n== runtime download -> $Dir =="
$sw = [Diagnostics.Stopwatch]::StartNew()
& $Exe runtime download --dir $Dir *> $DownloadLog
$code = $LASTEXITCODE
$sw.Stop()
Check "runtime download exits 0" ($code -eq 0) "exit $code; see $DownloadLog"
Write-Host ("Runtime download wall time: {0:N1} s" -f $sw.Elapsed.TotalSeconds)
Get-Content $DownloadLog -Tail 10 | ForEach-Object { Write-Host "  $_" }

& $Exe runtime verify --dir $Dir *> (Join-Path $LogRoot "runtime-verify.log")
$code = $LASTEXITCODE
Check "runtime verify exits 0" ($code -eq 0) "exit $code"

$ServerGpu = Join-Path $Dir "llama-vulkan/llama-server.exe"
$ServerCpu = Join-Path $Dir "llama-cpu/llama-server.exe"
$Pdfium = Join-Path $Dir "pdfium/bin/pdfium.dll"
Check "llama-vulkan/llama-server.exe exists" (Test-Path $ServerGpu)
Check "llama-cpu/llama-server.exe exists" (Test-Path $ServerCpu)
Check "pdfium/bin/pdfium.dll exists" (Test-Path $Pdfium)

$version = & $ServerGpu --version 2>&1 | Out-String
# llama.cpp prints "version: 1.0.0-dev (build 12535, commit ...)".
Check "llama-server --version reports build 12535" ($version -match "build 12535") $version.Trim()

Write-Host "`n== reuse: second runtime download =="
& $Exe runtime download --dir $Dir *> $ReuseLog
$code = $LASTEXITCODE
$reuseText = Get-Content $ReuseLog -Raw
Check "second runtime download exits 0" ($code -eq 0) "exit $code; see $ReuseLog"
Check "second runtime download skips installed components" `
    ($reuseText -match "already present") $reuseText

if ($ForceRedownload) {
    Write-Host "`n== --force re-downloads =="
    $forceLog = Join-Path $LogRoot "runtime-force.log"
    $sw = [Diagnostics.Stopwatch]::StartNew()
    & $Exe runtime download --dir $Dir --force *> $forceLog
    $code = $LASTEXITCODE
    $sw.Stop()
    $forceText = Get-Content $forceLog -Raw
    Check "--force exits 0" ($code -eq 0) "exit $code; see $forceLog"
    Check "--force installs (not skips) components" `
        (($forceText -match "installed") -and -not ($forceText -match "already present")) $forceText
    Write-Host ("--force wall time: {0:N1} s" -f $sw.Elapsed.TotalSeconds)
}

Write-Host "`n== doctor with the downloaded runtime =="
$doctorJson = & $Exe doctor --llama-dir $Dir --model-dir $ModelDir --json
$doctor = $doctorJson | ConvertFrom-Json
$gpu = @($doctor.llama_candidates | Where-Object { $_.device -eq "gpu" }).Count
$cpu = @($doctor.llama_candidates | Where-Object { $_.device -eq "cpu" }).Count
Check "doctor sees GPU and CPU llama-server candidates" (($gpu -ge 1) -and ($cpu -ge 1)) `
    "gpu=$gpu cpu=$cpu"
Check "doctor sees PDFium in the downloaded runtime" ([bool]$doctor.pdfium.ok) $doctor.pdfium.detail

Write-Host "`n== CPU OCR with the downloaded runtime =="
$OcrOut = Join-Path $LogRoot "ocr-cpu"
$OcrLog = Join-Path $LogRoot "ocr-cpu.log"
$sw = [Diagnostics.Stopwatch]::StartNew()
& $Exe ocr $CpuImage -o $OcrOut --format md --device cpu `
    --llama-dir (Join-Path $Dir "llama-cpu") --pdfium $Pdfium --model-dir $ModelDir `
    --max-tokens 96 --timeout 1800 *> $OcrLog
$code = $LASTEXITCODE
$sw.Stop()
$md = Join-Path $OcrOut ([IO.Path]::GetFileNameWithoutExtension($CpuImage) + ".md")
$text = if (Test-Path $md) { [IO.File]::ReadAllText($md, [Text.Encoding]::UTF8) } else { "" }
Check "CPU OCR exits 0" ($code -eq 0) "exit $code; see $OcrLog"
Check "CPU OCR produced Russian text" ($text -match "[А-Яа-яЁё]{3,}") "length $($text.Length)"
Write-Host ("CPU OCR wall time: {0:N1} s" -f $sw.Elapsed.TotalSeconds)

if ($CleanRuntime) {
    Remove-Item -Recurse -Force $Dir -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force $LogRoot -ErrorAction SilentlyContinue
} else {
    Write-Host "`nRuntime kept at $Dir (logs: $LogRoot)"
}

Write-Host "`n============================================"
if ($script:Failures -eq 0) {
    Write-Host "RUNTIME E2E PASSED ($script:Checks checks)" -ForegroundColor Green
    exit 0
} else {
    Write-Host "RUNTIME E2E FAILED: $($script:Failures) of $($script:Checks) checks failed" -ForegroundColor Red
    exit 1
}
