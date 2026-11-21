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

# ---------------------------------------------------------------- PE subsystems
# The GUI must never open a console window: GUI=2 (Windows GUI), CLI=3
# (console). The same helper is available standalone for release checks.
Write-Host "`n== executable subsystems =="
$subsystemLog = Join-Path $OutRoot "subsystems.log"
& (Join-Path $PSScriptRoot "check-gui-subsystem.ps1") -Gui $Gui -Cli $Exe *> $subsystemLog
$subsystemCode = $LASTEXITCODE
$subsystemText = if (Test-Path $subsystemLog) { Get-Content $subsystemLog -Raw } else { "" }
Check "check-gui-subsystem.ps1 passes" ($subsystemCode -eq 0) $subsystemText
Check "PE subsystem is GUI=2 and CLI=3" (
    $subsystemText -match "ocr-tool-gui\.exe: subsystem 2" -and
    $subsystemText -match "ocr-tool\.exe: subsystem 3"
) $subsystemText
Write-Host $subsystemText

# ---------------------------------------------------------------- doctor
Write-Host "`n== doctor =="
$doctorText = & $Exe doctor --llama-dir $LlamaGpu --model-dir $ModelDir 2>&1 | Out-String
Check "doctor prints model components" ($doctorText -match "model dir:" -and $doctorText -match "mmproj")
Check "doctor prints pdfium" ($doctorText -match "pdfium" -and $doctorText -match "OK")
Check "doctor prints llama-server" ($doctorText -match "llama-server candidates" -and $doctorText -match "gpu")
Check "doctor prints llama version" ($doctorText -match "version:")
Check "doctor prints pdfium version" ($doctorText -match "\[version ")
Write-Host $doctorText

$doctorJson = & $Exe doctor --llama-dir $LlamaGpu --model-dir $ModelDir --json 2>&1 | Out-String
$doctorReport = $doctorJson | ConvertFrom-Json
Check "doctor JSON has version fields" (($null -ne $doctorReport.pdfium.version) -and ($null -ne $doctorReport.llama_candidates[0].version))

# ---------------------------------------------------------------- exit codes
Write-Host "`n== error exit codes =="
$badOut = Join-Path $OutRoot "errors"
$code = Invoke-Ocr @("ocr", $Jpg, "-o", $badOut, "--pages", "0") (Join-Path $OutRoot "pages0.log")
Check "malformed --pages exits 2" ($code -eq 2) "exit $code"

$isoDir = Join-Path $OutRoot "iso"
New-Item -ItemType Directory -Force -Path $isoDir | Out-Null
Copy-Item $Exe (Join-Path $isoDir "ocr-tool.exe") -Force
$brokenDir = Join-Path $isoDir "broken-llama"
New-Item -ItemType Directory -Force -Path $brokenDir | Out-Null
Copy-Item (Join-Path $LlamaGpu "llama-server.exe") (Join-Path $brokenDir "llama-server.exe") -Force
Push-Location $isoDir
try {
    $code = Invoke-Ocr @(
        "ocr", $Jpg, "-o", (Join-Path $isoDir "out"), "--device", "gpu",
        "--llama-dir", $brokenDir, "--model-dir", $ModelDir
    ) (Join-Path $isoDir "broken.log")
} finally {
    Pop-Location
}
Check "broken engine exits 3" ($code -eq 3) "exit $code"

# ------------------------------------------------- first-run configuration (no network)
Write-Host "`n== first-run configuration without model/pdfium =="
$TempRoot = Join-Path $env:TEMP ("ocr-tool-acceptance-" + $PID)
$emptyModel = Join-Path $TempRoot "empty-models"
$emptyOut = Join-Path $TempRoot "empty-out"
$emptyRuntime = Join-Path $TempRoot "empty-runtime"
New-Item -ItemType Directory -Force -Path $emptyModel, $emptyOut, $emptyRuntime | Out-Null

# Runtime download/verify CLI surface (offline checks only; the real ~69 MB E2E
# lives in scripts/acceptance-runtime.ps1).
Write-Host "`n== runtime CLI (offline) =="
$runtimeHelp = & $Exe runtime --help 2>&1 | Out-String
Check "runtime --help lists download and verify" `
    ($runtimeHelp -match "download" -and $runtimeHelp -match "verify")
$runtimeDownloadHelp = & $Exe runtime download --help 2>&1 | Out-String
Check "runtime download --help lists --dir and --force" `
    ($runtimeDownloadHelp -match "--dir" -and $runtimeDownloadHelp -match "--force")
& $Exe runtime verify --dir $emptyRuntime *> (Join-Path $TempRoot "runtime-verify-empty.log")
$code = $LASTEXITCODE
Check "runtime verify on an empty folder exits 2" ($code -eq 2) "exit $code"

$noModelLog = Join-Path $TempRoot "no-model.log"
$code = Invoke-Ocr @("ocr", $Jpg, "-o", $emptyOut, "--model-dir", $emptyModel) $noModelLog
$noModelText = if (Test-Path $noModelLog) { Get-Content $noModelLog -Raw } else { "" }
Check "CLI without model exits 2" ($code -eq 2) "exit $code"
Check "CLI without model says 'Model files not found'" `
    ($noModelText.Contains("Model files not found")) $noModelText

$missingPdfium = Join-Path $TempRoot "no-pdfium/pdfium.dll"
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $missingPdfium) | Out-Null
$savedPdfiumEnv = $env:OCR_PDFIUM
$env:OCR_PDFIUM = $missingPdfium
Push-Location $TempRoot
try {
    $noPdfiumLog = Join-Path $TempRoot "no-pdfium.log"
    $code = Invoke-Ocr @(
        "ocr", $Pdf, "-o", (Join-Path $TempRoot "pdf-out"), "--model-dir", $ModelDir
    ) $noPdfiumLog
} finally {
    Pop-Location
    $env:OCR_PDFIUM = $savedPdfiumEnv
}
$noPdfiumText = if (Test-Path $noPdfiumLog) { Get-Content $noPdfiumLog -Raw } else { "" }
Check "CLI PDF without pdfium exits 2" ($code -eq 2) "exit $code"
Check "CLI PDF without pdfium says 'PDFium not found'" `
    ($noPdfiumText.Contains("PDFium not found")) $noPdfiumText

# ---------------------------------------------------------------- dist layout
Write-Host "`n== dist-like layout (exe-relative runtime discovery) =="
$dist = Join-Path $OutRoot "dist"
$distRuntime = Join-Path $dist "runtime/windows-x64"
New-Item -ItemType Directory -Force -Path $distRuntime | Out-Null
Copy-Item $Exe (Join-Path $dist "ocr-tool.exe") -Force
Copy-Item -Recurse $LlamaGpu (Join-Path $distRuntime "llama-vulkan")
Copy-Item -Recurse $LlamaCpu (Join-Path $distRuntime "llama-cpu")
New-Item -ItemType Directory -Force -Path (Join-Path $distRuntime "pdfium/bin") | Out-Null
Copy-Item $Pdfium (Join-Path $distRuntime "pdfium/bin/pdfium.dll") -Force
$savedPdfium = $env:OCR_PDFIUM
Remove-Item Env:\OCR_PDFIUM -ErrorAction SilentlyContinue
try {
    $distDoctor = & (Join-Path $dist "ocr-tool.exe") doctor --model-dir $ModelDir 2>&1 | Out-String
} finally {
    $env:OCR_PDFIUM = $savedPdfium
}
Check "dist layout finds bundled llama and pdfium" (
    $distDoctor -match "llama-server candidates" -and $distDoctor -match "\[OK\s+\] pdfium"
) $distDoctor
& $Exe runtime verify --dir $distRuntime *> (Join-Path $OutRoot "runtime-verify-dist.log")
$code = $LASTEXITCODE
Check "runtime verify accepts the packaged layout" ($code -eq 0) "exit $code"

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

# ---------------------------------------------------------------- collision + meta merge
Write-Host "`n== collision and _meta.json merge in one output directory =="
$copySrc = Join-Path $OutRoot "copy-src"
New-Item -ItemType Directory -Force -Path $copySrc | Out-Null
Copy-Item $Jpg (Join-Path $copySrc "invoice_scan_ru.jpg") -Force
$code = Invoke-Ocr @(
    "ocr", (Join-Path $copySrc "invoice_scan_ru.jpg"), "-o", $jpgOut, "--format", "md",
    "--device", "gpu", "--llama-dir", $LlamaGpu, "--model-dir", $ModelDir
) (Join-Path $OutRoot "collision.log")
Check "second job exits 0" ($code -eq 0) "exit $code; see $OutRoot/collision.log"
Check "collision writes invoice_scan_ru-2.md" (Test-Path (Join-Path $jpgOut "invoice_scan_ru-2.md"))
$jobMeta = Get-Content (Join-Path $jpgOut "_meta.json") -Raw | ConvertFrom-Json
Check "_meta.json keeps documents of both jobs" ($jobMeta.documents.Count -ge 2) "documents=$($jobMeta.documents.Count)"

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
    $guiLog = Join-Path $OutRoot "gui.log"
    $savedGuiLogEnv = $env:OCR_GUI_LOG
    $savedNoPreload = $env:OCR_GUI_NO_PRELOAD
    $env:OCR_GUI_LOG = $guiLog
    # The smoke test only checks that the window stays alive; do not load a
    # multi-gigabyte model into VRAM during it.
    $env:OCR_GUI_NO_PRELOAD = "1"
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

        # First run against an empty model folder: the setup panel (Download /
        # Replace / PDFium status) must render without crashing.
        $savedModelDir = $env:OCR_MODEL_DIR
        $env:OCR_MODEL_DIR = $emptyModel
        try {
            $firstRun = Start-Process -FilePath $Gui -PassThru
            Start-Sleep -Seconds 5
            $firstRunAlive = -not $firstRun.HasExited
            if ($firstRunAlive) {
                Stop-Process -Id $firstRun.Id -Force
                $firstRun.WaitForExit()
            }
        } finally {
            if ($null -ne $savedModelDir) {
                $env:OCR_MODEL_DIR = $savedModelDir
            } else {
                Remove-Item Env:\OCR_MODEL_DIR -ErrorAction SilentlyContinue
            }
        }
        Check "GUI first run with empty model dir stayed alive for 5 s" $firstRunAlive `
            $(if ($firstRunAlive) { "" } else { "exit code $($firstRun.ExitCode)" })

        # Runtime-ready path: the real dist runtime from the layout section is
        # pointed at through the documented environment variables.
        $savedLlamaEnv = $env:OCR_LLAMA_SERVER
        $savedPdfiumEnv = $env:OCR_PDFIUM
        $env:OCR_LLAMA_SERVER = Join-Path $distRuntime "llama-cpu/llama-server.exe"
        $env:OCR_PDFIUM = Join-Path $distRuntime "pdfium/bin/pdfium.dll"
        try {
            $readyGui = Start-Process -FilePath $Gui -PassThru
            Start-Sleep -Seconds 5
            $readyAlive = -not $readyGui.HasExited
            if ($readyAlive) {
                Stop-Process -Id $readyGui.Id -Force
                $readyGui.WaitForExit()
            }
        } finally {
            if ($null -ne $savedLlamaEnv) {
                $env:OCR_LLAMA_SERVER = $savedLlamaEnv
            } else {
                Remove-Item Env:\OCR_LLAMA_SERVER -ErrorAction SilentlyContinue
            }
            if ($null -ne $savedPdfiumEnv) {
                $env:OCR_PDFIUM = $savedPdfiumEnv
            } else {
                Remove-Item Env:\OCR_PDFIUM -ErrorAction SilentlyContinue
            }
        }
        Check "GUI with a ready runtime stayed alive for 5 s" $readyAlive `
            $(if ($readyAlive) { "" } else { "exit code $($readyGui.ExitCode)" })

        # A Windows-subsystem GUI has no stdout; the file log is the only
        # diagnostic after a double-click.
        $guiLogText = if (Test-Path $guiLog) { Get-Content $guiLog -Raw } else { "" }
        Check "GUI wrote its file log" ($guiLogText -match "ocr-tool-gui [0-9.]+ starting") $guiLogText
    }
    if ($null -ne $savedGuiLogEnv) {
        $env:OCR_GUI_LOG = $savedGuiLogEnv
    } else {
        Remove-Item Env:\OCR_GUI_LOG -ErrorAction SilentlyContinue
    }
    if ($null -ne $savedNoPreload) {
        $env:OCR_GUI_NO_PRELOAD = $savedNoPreload
    } else {
        Remove-Item Env:\OCR_GUI_NO_PRELOAD -ErrorAction SilentlyContinue
    }
}

Remove-Item -Recurse -Force $TempRoot -ErrorAction SilentlyContinue

Write-Host "`n============================================"
$leftover = @(Get-Process llama-server -ErrorAction SilentlyContinue | Where-Object {
    try { $_.Path -and $_.Path.StartsWith($Root, [StringComparison]::OrdinalIgnoreCase) } catch { $false }
})
Check "no llama-server sidecar left after the run" ($leftover.Count -eq 0) "pids: $($leftover.Id -join ',')"
if ($script:Failures -eq 0) {
    Write-Host "ACCEPTANCE PASSED ($script:Checks checks)" -ForegroundColor Green
    exit 0
} else {
    Write-Host "ACCEPTANCE FAILED: $script:Failures of $script:Checks checks failed" -ForegroundColor Red
    exit 1
}
