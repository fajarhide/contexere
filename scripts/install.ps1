<#
.SYNOPSIS
OMNI - Universal Windows Install Script

.DESCRIPTION
Installs the latest Contexere binary to $HOME\.local\bin\contexere.exe, with omni.exe beside it.
Supports: Windows (x86_64)

.EXAMPLE
irm https://raw.githubusercontent.com/fajarhide/omni/main/scripts/install.ps1 | iex
#>

$ErrorActionPreference = "Stop"

$Repo = "fajarhide/omni"
$InstallDir = if ($env:OMNI_INSTALL_DIR) { $env:OMNI_INSTALL_DIR } else { "$env:USERPROFILE\.local\bin" }
$Version = if ($env:OMNI_VERSION) { $env:OMNI_VERSION } else { "latest" }

Write-Host ""
Write-Host "  ┌─────────────────────────────────────┐" -ForegroundColor Cyan
Write-Host "  │  OMNI Installer                     │" -ForegroundColor Cyan
Write-Host "  │  Less noise. More signal.           │" -ForegroundColor Cyan
Write-Host "  └─────────────────────────────────────┘" -ForegroundColor Cyan
Write-Host ""

# --- Version Resolution ---
if ($Version -eq "latest") {
    Write-Host "[omni] Fetching latest version from GitHub..." -ForegroundColor Cyan
    try {
        $ReleaseInfo = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
        $Version = $ReleaseInfo.tag_name
    } catch {
        Write-Error "[omni] Failed to fetch latest version from GitHub."
        exit 1
    }
}

$Platform = "x86_64-pc-windows-msvc"
$TargetUrl = "https://github.com/$Repo/releases/download/$Version/omni-$Version-$Platform.zip"
$TmpDir = Join-Path $env:TEMP "omni_install_$([guid]::NewGuid().ToString().Substring(0,8))"
$ZipFile = Join-Path $TmpDir "omni.zip"

Write-Host "[omni] Platform: $Platform" -ForegroundColor Cyan
Write-Host "[omni] Version:  $Version" -ForegroundColor Cyan
Write-Host "[omni] Target:   $InstallDir" -ForegroundColor Cyan
Write-Host ""

# --- Download & Install ---
if (-not (Test-Path $TmpDir)) {
    New-Item -ItemType Directory -Force -Path $TmpDir | Out-Null
}

Write-Host "[omni] Downloading omni $Version for $Platform..." -ForegroundColor Cyan
try {
    Invoke-WebRequest -Uri $TargetUrl -OutFile $ZipFile
} catch {
    Write-Error "[omni] Download failed. Check if version $Version exists at: $TargetUrl"
    exit 1
}

Write-Host "[omni] Extracting archive..." -ForegroundColor Cyan
Expand-Archive -Path $ZipFile -DestinationPath $TmpDir -Force

if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
}

$Contexere = Join-Path $TmpDir "contexere.exe"
$Legacy = Join-Path $TmpDir "omni.exe"
if (Test-Path $Contexere) {
    $Bin = "contexere.exe"
    Copy-Item -Path $Contexere -Destination "$InstallDir\contexere.exe" -Force
    # A copy, not a link: Windows symlinks need admin. Same program under the old name.
    Copy-Item -Path $Contexere -Destination "$InstallDir\omni.exe" -Force
} elseif (Test-Path $Legacy) {
    # Archives from before 0.8.0 hold only omni.exe.
    $Bin = "omni.exe"
    Copy-Item -Path $Legacy -Destination "$InstallDir\omni.exe" -Force
} else {
    Write-Error "[omni] Extract failed: no contexere.exe or omni.exe in downloaded archive."
    exit 1
}

# --- Cleanup ---
Remove-Item -Path $TmpDir -Recurse -Force | Out-Null

Write-Host ""
Write-Host "[omni] ✓ Contexere installed to $InstallDir\$Bin" -ForegroundColor Green

# --- Verify ---
try {
    $ExeVersion = & "$InstallDir\$Bin" version
    Write-Host "[omni] Verified: $ExeVersion" -ForegroundColor Green
} catch {
    Write-Host "[omni] Unable to verify executable easily." -ForegroundColor Yellow
}

# --- PATH Check ---
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notmatch [regex]::Escape($InstallDir)) {
    Write-Host ""
    Write-Host "[omni] $InstallDir is not in your PATH." -ForegroundColor Yellow
    Write-Host "[omni] Adding $InstallDir to your User PATH..." -ForegroundColor Cyan
    
    $NewPath = "$UserPath;$InstallDir"
    [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
    
    Write-Host "[omni] ✓ PATH updated successfully." -ForegroundColor Green
    Write-Host "[omni] IMPORTANT: You must restart your terminal or open a new PowerShell window to use the 'contexere' command." -ForegroundColor Yellow
}

Write-Host ""
Write-Host "  Next steps:"
Write-Host "    contexere init              # Interactive setup for your preferred AI Agent"
Write-Host "    contexere doctor            # Verify installation"
Write-Host "    contexere stats             # View savings after first session"
Write-Host ""
