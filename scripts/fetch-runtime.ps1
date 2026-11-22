#requires -Version 5.1
<#
.SYNOPSIS
  Download the pinned llama.cpp b12535 builds and PDFium 8165 into runtime/<target>/.

.DESCRIPTION
  Layout produced:
    runtime/<target>/llama-vulkan/   (GPU; llama-metal on macOS)
    runtime/<target>/llama-cpu/
    runtime/<target>/pdfium/{bin,lib}/
  The model weights are NOT downloaded here; use `ocr-tool models download`.
#>
[CmdletBinding()]
param(
    [ValidateSet("auto", "windows-x64", "linux-x64", "macos-arm64", "macos-x64")]
    [string]$Target = "auto",
    [string]$OutDir = "",
    [switch]$SkipLlama,
    [switch]$SkipPdfium
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$LlamaVersion = "b12535"
$PdfiumTag = "chromium/8165"
$Root = Split-Path -Parent $PSScriptRoot
if (-not $OutDir) { $OutDir = Join-Path $Root "runtime" }

function Resolve-Target {
    param([string]$Requested)
    if ($Requested -ne "auto") { return $Requested }
    $isWindows = $true
    if (Get-Variable -Name IsWindows -ErrorAction SilentlyContinue) { $isWindows = $IsWindows }
    $isMac = $false
    if (Get-Variable -Name IsMacOS -ErrorAction SilentlyContinue) { $isMac = $IsMacOS }
    $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
    if ($isWindows) { return "windows-x64" }
    if ($isMac) {
        if ($arch -match "arm") { return "macos-arm64" }
        return "macos-x64"
    }
    return "linux-x64"
}

function Get-RuntimeSpec {
    param([string]$Name)
    $llamaBase = "https://github.com/ggml-org/llama.cpp/releases/download/$LlamaVersion"
    $pdfiumBase = "https://github.com/bblanchon/pdfium-binaries/releases/download/$PdfiumTag"
    switch ($Name) {
        "windows-x64" {
            return @{
                LlamaGpu = "$llamaBase/llama-$LlamaVersion-bin-win-vulkan-x64.zip"
                LlamaCpu = "$llamaBase/llama-$LlamaVersion-bin-win-cpu-x64.zip"
                Pdfium = "$pdfiumBase/pdfium-win-x64.tgz"
                Exe = "llama-server.exe"
            }
        }
        "linux-x64" {
            return @{
                LlamaGpu = "$llamaBase/llama-$LlamaVersion-bin-ubuntu-vulkan-x64.tar.gz"
                LlamaCpu = "$llamaBase/llama-$LlamaVersion-bin-ubuntu-x64.tar.gz"
                Pdfium = "$pdfiumBase/pdfium-linux-x64.tgz"
                Exe = "llama-server"
            }
        }
        "macos-arm64" {
            return @{
                LlamaGpu = "$llamaBase/llama-$LlamaVersion-bin-macos-arm64.tar.gz"
                LlamaCpu = "$llamaBase/llama-$LlamaVersion-bin-macos-arm64.tar.gz"
                Pdfium = "$pdfiumBase/pdfium-mac-univ.tgz"
                Exe = "llama-server"
            }
        }
        "macos-x64" {
            return @{
                LlamaGpu = "$llamaBase/llama-$LlamaVersion-bin-macos-x64.tar.gz"
                LlamaCpu = "$llamaBase/llama-$LlamaVersion-bin-macos-x64.tar.gz"
                Pdfium = "$pdfiumBase/pdfium-mac-univ.tgz"
                Exe = "llama-server"
            }
        }
        default { throw "Unsupported target: $Name" }
    }
}

function Invoke-Download {
    param([string]$Url, [string]$Destination)
    Write-Host "  downloading $Url"
    Invoke-WebRequest -Uri $Url -OutFile $Destination -UseBasicParsing
}

function Expand-Package {
    param([string]$Archive, [string]$Destination)
    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
    if ($Archive.EndsWith(".zip")) {
        Expand-Archive -LiteralPath $Archive -DestinationPath $Destination -Force
    } else {
        # Prefer the Windows system bsdtar: a MSYS/Git-Bash tar earlier in PATH
        # misreads drive-letter paths ("E:\...") as remote hosts.
        $systemTar = $null
        if ($env:SystemRoot) { $systemTar = Join-Path $env:SystemRoot "System32\tar.exe" }
        if ($systemTar -and (Test-Path $systemTar)) {
            & $systemTar -xzf $Archive -C $Destination
        } else {
            & tar -xzf $Archive -C $Destination
        }
        if ($LASTEXITCODE -ne 0) { throw "tar failed for $Archive" }
    }
}

$target = Resolve-Target $Target
$spec = Get-RuntimeSpec $target
$targetDir = Join-Path $OutDir $target
$cache = Join-Path $Root ".tmp/runtime-download"
New-Item -ItemType Directory -Force -Path $targetDir, $cache | Out-Null

Write-Host "ocr-tool runtime: $target -> $targetDir"

if (-not $SkipLlama) {
    $gpuZip = Join-Path $cache ([IO.Path]::GetFileName($spec.LlamaGpu))
    $cpuZip = Join-Path $cache ([IO.Path]::GetFileName($spec.LlamaCpu))
    if (-not (Test-Path $gpuZip)) { Invoke-Download $spec.LlamaGpu $gpuZip }
    if (-not (Test-Path $cpuZip)) { Invoke-Download $spec.LlamaCpu $cpuZip }

    $gpuDir = Join-Path $targetDir "llama-vulkan"
    $cpuDir = Join-Path $targetDir "llama-cpu"
    Write-Host "  extracting Vulkan/Metal build"
    Expand-Package $gpuZip $gpuDir
    Write-Host "  extracting CPU build"
    if ($spec.LlamaGpu -eq $spec.LlamaCpu) {
        New-Item -ItemType Directory -Force -Path $cpuDir | Out-Null
        Copy-Item -Recurse -Force (Join-Path $gpuDir "*") $cpuDir
    } else {
        Expand-Package $cpuZip $cpuDir
    }
    foreach ($dir in @($gpuDir, $cpuDir)) {
        $exe = Join-Path $dir $spec.Exe
        if (-not (Test-Path $exe)) { throw "llama-server not found after extraction: $exe" }
        if (-not $IsWindows -and (Get-Variable -Name IsWindows -ErrorAction SilentlyContinue)) {
            & chmod +x $exe
        }
    }
}

if (-not $SkipPdfium) {
    $pdfiumArchive = Join-Path $cache ([IO.Path]::GetFileName($spec.Pdfium))
    if (-not (Test-Path $pdfiumArchive)) { Invoke-Download $spec.Pdfium $pdfiumArchive }
    $pdfiumDir = Join-Path $targetDir "pdfium"
    Write-Host "  extracting PDFium"
    Expand-Package $pdfiumArchive $pdfiumDir
    $lib = @(
        (Join-Path $pdfiumDir "bin/pdfium.dll"),
        (Join-Path $pdfiumDir "lib/libpdfium.so"),
        (Join-Path $pdfiumDir "lib/libpdfium.dylib")
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $lib) { throw "PDFium library not found under $pdfiumDir" }
    Write-Host "  PDFium: $lib"
}

Write-Host "Runtime ready: $targetDir"
