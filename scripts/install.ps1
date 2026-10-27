<#
.SYNOPSIS
VENG - Universal Windows Install Script

.DESCRIPTION
Installs the latest VENG binary to $HOME\.local\bin\veng.exe.
Supports: Windows (x86_64)

.EXAMPLE
irm https://raw.githubusercontent.com/fajarhide/veng/main/scripts/install.ps1 | iex
#>

$ErrorActionPreference = "Stop"

$Repo = "fajarhide/veng"
$InstallDir = if ($env:VENG_INSTALL_DIR) { $env:VENG_INSTALL_DIR } else { "$env:USERPROFILE\.local\bin" }
$Version = if ($env:VENG_VERSION) { $env:VENG_VERSION } else { "latest" }

Write-Host ""
Write-Host "  ┌─────────────────────────────────────┐" -ForegroundColor Cyan
Write-Host "  │  VENG Installer                     │" -ForegroundColor Cyan
Write-Host "  │  Less noise. More signal.           │" -ForegroundColor Cyan
Write-Host "  └─────────────────────────────────────┘" -ForegroundColor Cyan
Write-Host ""

# --- Version Resolution ---
if ($Version -eq "latest") {
    Write-Host "[veng] Fetching latest version from GitHub..." -ForegroundColor Cyan
    try {
        $ReleaseInfo = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
        $Version = $ReleaseInfo.tag_name
    } catch {
        Write-Error "[veng] Failed to fetch latest version from GitHub."
        exit 1
    }
}

$Platform = "x86_64-pc-windows-msvc"
$TargetUrl = "https://github.com/$Repo/releases/download/$Version/veng-$Version-$Platform.zip"
$TmpDir = Join-Path $env:TEMP "veng_install_$([guid]::NewGuid().ToString().Substring(0,8))"
$ZipFile = Join-Path $TmpDir "veng.zip"

Write-Host "[veng] Platform: $Platform" -ForegroundColor Cyan
Write-Host "[veng] Version:  $Version" -ForegroundColor Cyan
Write-Host "[veng] Target:   $InstallDir\veng.exe" -ForegroundColor Cyan
Write-Host ""

# --- Download & Install ---
if (-not (Test-Path $TmpDir)) {
    New-Item -ItemType Directory -Force -Path $TmpDir | Out-Null
}

Write-Host "[veng] Downloading veng $Version for $Platform..." -ForegroundColor Cyan
try {
    Invoke-WebRequest -Uri $TargetUrl -OutFile $ZipFile
} catch {
    Write-Error "[veng] Download failed. Check if version $Version exists at: $TargetUrl"
    exit 1
}

Write-Host "[veng] Extracting archive..." -ForegroundColor Cyan
Expand-Archive -Path $ZipFile -DestinationPath $TmpDir -Force

if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
}

$ExtractedExe = Join-Path $TmpDir "veng.exe"
if (-not (Test-Path $ExtractedExe)) {
    Write-Error "[veng] Extract failed: veng.exe not found in downloaded archive."
    exit 1
}

Copy-Item -Path $ExtractedExe -Destination "$InstallDir\veng.exe" -Force

# --- Cleanup ---
Remove-Item -Path $TmpDir -Recurse -Force | Out-Null

Write-Host ""
Write-Host "[veng] ✓ VENG installed to $InstallDir\veng.exe" -ForegroundColor Green

# --- Verify ---
try {
    $ExeVersion = & "$InstallDir\veng.exe" version
    Write-Host "[veng] Verified: $ExeVersion" -ForegroundColor Green
} catch {
    Write-Host "[veng] Unable to verify executable easily." -ForegroundColor Yellow
}

# --- PATH Check ---
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notmatch [regex]::Escape($InstallDir)) {
    Write-Host ""
    Write-Host "[veng] $InstallDir is not in your PATH." -ForegroundColor Yellow
    Write-Host "[veng] Adding $InstallDir to your User PATH..." -ForegroundColor Cyan
    
    $NewPath = "$UserPath;$InstallDir"
    [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
    
    Write-Host "[veng] ✓ PATH updated successfully." -ForegroundColor Green
    Write-Host "[veng] IMPORTANT: You must restart your terminal or open a new PowerShell window to use the 'veng' command." -ForegroundColor Yellow
}

Write-Host ""
Write-Host "  Next steps:"
Write-Host "    veng init              # Interactive setup for your preferred AI Agent"
Write-Host "    veng doctor            # Verify installation"
Write-Host "    veng stats             # View savings after first session"
Write-Host ""
