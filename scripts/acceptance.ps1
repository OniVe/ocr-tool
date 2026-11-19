#requires -Version 5.1
<#
.SYNOPSIS
  Local acceptance test for ocr-tool on Windows.

.DESCRIPTION
  Expects the maintainer-local assets:
    .tmp/models/{chandra-q4.gguf,mmproj.gguf}
    .tmp/llama/llama-server.exe          (Vulkan)
    .tmp/llama-cpu/llama-server.exe      (CPU)
    .tmp/pdfium/bin/pdfium.dll
    samples/invoice_scan_ru.jpg
    samples/contract_ru.pdf

  Checks GPU recognition (JPG + 2 PDF pages), CPU fallback on a crop, doctor,
  and GUI startup. Run from the repository root:
    pwsh -File scripts/acceptance.ps1
#>
[CmdletBinding()]
param(
    [switch]$SkipCpu,
    [switch]$SkipGui,
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
try {
    [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
    $OutputEncoding = [System.Text.UTF8Encoding]::new()
} catch { }
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

$Exe = Join-Path $Root "target/release/ocr-tool.exe"
$Gui = Join-Path $Root "target/release/ocr-tool-gui.exe"
$ModelDir = Join-Path $Root ".tmp/models"
$LlamaGpu = Join-Path $Root ".tmp/llama"
$LlamaCpu = Join-Path $Root ".tmp/llama-cpu"
$Pdfium = Join-Path $Root ".tmp/pdfium/bin/pdfium.dll"
$Jpg = Join-Path $Root "samples/invoice_scan_ru.jpg"
$Pdf = Join-Path $Root "samples/contract_ru.pdf"
$OutRoot = Join-Path $Root "out/acceptance"

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

function Assert-FileContains {
    param([string]$Path, [string]$Needle, [string]$Name)
    if (-not (Test-Path $Path)) {
        Check $Name $false "missing file $Path"
        return
    }
    $text = [IO.File]::ReadAllText($Path, [Text.Encoding]::UTF8)
    Check $Name $text.Contains($Needle) "missing '$Needle' in $Path"
}

function Invoke-Ocr {
    param([string[]]$Arguments, [string]$LogPath)
    & $Exe @Arguments *> $LogPath
    return $LASTEXITCODE
}

if (-not (Test-Path $Exe)) { throw "Build first: cargo build --release ($Exe not found)" }
foreach ($asset in @($ModelDir, $LlamaGpu, $LlamaCpu, $Pdfium, $Jpg, $Pdf)) {
    if (-not (Test-Path $asset)) { throw "Acceptance asset missing: $asset" }
}

if (-not $SkipBuild) {
    Write-Host "== cargo build --release =="
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed" }
}

if (Test-Path $OutRoot) { Remove-Item -Recurse -Force $OutRoot }
New-Item -ItemType Directory -Force -Path $OutRoot | Out-Null
$env:OCR_PDFIUM = $Pdfium
Remove-Item Env:\OCR_LLAMA_SERVER -ErrorAction SilentlyContinue
Remove-Item Env:\OCR_MODEL_DIR -ErrorAction SilentlyContinue

# ---------------------------------------------------------------- doctor
Write-Host "`n== doctor =="
$doctorText = & $Exe doctor --llama-dir $LlamaGpu --model-dir $ModelDir 2>&1 | Out-String
Check "doctor prints model components" ($doctorText -match "model dir:" -and $doctorText -match "mmproj")
Check "doctor prints pdfium" ($doctorText -match "pdfium" -and $doctorText -match "OK")
Check "doctor prints llama-server" ($doctorText -match "llama-server candidates" -and $doctorText -match "gpu")
Write-Host $doctorText

# ---------------------------------------------------------------- GPU JPG
Write-Host "`n== GPU: invoice_scan_ru.jpg =="
$jpgOut = Join-Path $OutRoot "jpg"
$jpgLog = Join-Path $OutRoot "jpg.log"
$sw = [Diagnostics.Stopwatch]::StartNew()
$code = Invoke-Ocr @(
    "ocr", $Jpg, "-o", $jpgOut, "--format", "both", "--device", "gpu",
    "--llama-dir", $LlamaGpu, "--model-dir", $ModelDir, "--json"
) $jpgLog
$sw.Stop()
Check "JPG exit code 0" ($code -eq 0) "exit $code; see $jpgLog"
$md = Join-Path $jpgOut "invoice_scan_ru.md"
$html = Join-Path $jpgOut "invoice_scan_ru.html"
Check "JPG .md and .html exist" ((Test-Path $md) -and (Test-Path $html))
foreach ($needle in @("Внимание", "Счет № 99", "Дедморозим", "43,200.00")) {
    Assert-FileContains $md $needle "MD contains '$needle'"
}
Assert-FileContains $html 'lang="ru"' "HTML has lang=ru"
Assert-FileContains $html 'charset="utf-8"' "HTML declares UTF-8"
Assert-FileContains $html "Дедморозим" "HTML contains Russian text"
Check "JPG _meta.json exists" (Test-Path (Join-Path $jpgOut "_meta.json"))
Write-Host ("GPU JPG wall time: {0:N1} s" -f $sw.Elapsed.TotalSeconds)

# ---------------------------------------------------------------- GPU PDF
Write-Host "`n== GPU: contract_ru.pdf pages 1-2 =="
$pdfOut = Join-Path $OutRoot "pdf"
$pdfLog = Join-Path $OutRoot "pdf.log"
$sw = [Diagnostics.Stopwatch]::StartNew()
$code = Invoke-Ocr @(
    "ocr", $Pdf, "-o", $pdfOut, "--format", "both", "--device", "gpu",
    "--llama-dir", $LlamaGpu, "--model-dir", $ModelDir,
    "--pages", "1-2", "--json"
) $pdfLog
$sw.Stop()
Check "PDF exit code 0" ($code -eq 0) "exit $code; see $pdfLog"
$pdfMd = Join-Path $pdfOut "contract_ru.md"
$pdfHtml = Join-Path $pdfOut "contract_ru.html"
Check "PDF .md and .html exist" ((Test-Path $pdfMd) -and (Test-Path $pdfHtml))
if (Test-Path $pdfMd) {
    $pdfText = [IO.File]::ReadAllText($pdfMd, [Text.Encoding]::UTF8)
    Check "PDF has page 1 marker" $pdfText.Contains("<!-- page 1 -->")
    Check "PDF has page 2 marker" $pdfText.Contains("<!-- page 2 -->")
    Check "PDF contains Russian text" ($pdfText -match "[А-Яа-яЁё]{4,}")
    Check "PDF Markdown is non-trivial" ($pdfText.Length -gt 500) "length $($pdfText.Length)"
}
Write-Host ("GPU PDF wall time: {0:N1} s" -f $sw.Elapsed.TotalSeconds)

# ---------------------------------------------------------------- CPU crop
if (-not $SkipCpu) {
    Write-Host "`n== CPU: small crop with --max-tokens 96 =="
    Add-Type -AssemblyName System.Drawing
    $cropPath = Join-Path $OutRoot "cpu_crop.jpg"
    $img = [System.Drawing.Image]::FromFile($Jpg)
    try {
        $src = New-Object System.Drawing.Rectangle(
            [int]($img.Width * 0.08), [int]($img.Height * 0.18),
            [int]($img.Width * 0.50), [int]($img.Height * 0.10))
        $bmp = New-Object System.Drawing.Bitmap($src.Width, $src.Height)
        try {
            $g = [System.Drawing.Graphics]::FromImage($bmp)
            try {
                $g.DrawImage($img,
                    (New-Object System.Drawing.Rectangle(0, 0, $src.Width, $src.Height)),
                    $src, [System.Drawing.GraphicsUnit]::Pixel)
            } finally { $g.Dispose() }
            $bmp.Save($cropPath, [System.Drawing.Imaging.ImageFormat]::Jpeg)
        } finally { $bmp.Dispose() }
    } finally { $img.Dispose() }

    $cpuOut = Join-Path $OutRoot "cpu"
    $cpuLog = Join-Path $OutRoot "cpu.log"
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $code = Invoke-Ocr @(
        "ocr", $cropPath, "-o", $cpuOut, "--format", "md", "--device", "cpu",
        "--llama-dir", $LlamaCpu, "--model-dir", $ModelDir,
        "--max-tokens", "96", "--timeout", "1800"
    ) $cpuLog
    $sw.Stop()
    $cpuMd = Join-Path $cpuOut "cpu_crop.md"
    $cpuText = if (Test-Path $cpuMd) { [IO.File]::ReadAllText($cpuMd, [Text.Encoding]::UTF8) } else { "" }
    Check "CPU exit code 0" ($code -eq 0) "exit $code; see $cpuLog"
    Check "CPU produced non-empty Russian text" ($cpuText -match "[А-Яа-яЁё]{3,}") "length $($cpuText.Length)"
    Write-Host ("CPU crop wall time: {0:N1} s" -f $sw.Elapsed.TotalSeconds)
    Write-Host $cpuText
}

# ---------------------------------------------------------------- GUI smoke
if (-not $SkipGui) {
    Write-Host "`n== GUI smoke test (>= 5 s) =="
    if (-not (Test-Path $Gui)) {
        Check "GUI binary exists" $false $Gui
    } else {
        $process = Start-Process -FilePath $Gui -PassThru
        Start-Sleep -Seconds 5
        $alive = -not $process.HasExited
        if ($alive) {
            Stop-Process -Id $process.Id -Force
            $process.WaitForExit()
        }
        Check "GUI stayed alive for 5 s" $alive $(if ($alive) { "" } else { "exit code $($process.ExitCode)" })
    }
}

Write-Host "`n============================================"
if ($script:Failures -eq 0) {
    Write-Host "ACCEPTANCE PASSED ($script:Checks checks)" -ForegroundColor Green
    exit 0
} else {
    Write-Host "ACCEPTANCE FAILED: $script:Failures of $script:Checks checks failed" -ForegroundColor Red
    exit 1
}
