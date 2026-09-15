# ==============================================================================
# Raqim Sovereign Runtime & Tooling Installer (Windows PowerShell)
# Installs: raqim-core.exe, raqim-cli.exe, raqim-mcp.exe
# ==============================================================================
$ErrorActionPreference = "Stop"

$Repo = "raqim-ai/raqim"
$InstallDir = if ($env:RAQIM_INSTALL_DIR) { $env:RAQIM_INSTALL_DIR } else { "$env:USERPROFILE\.raqim\bin" }

Write-Host @"
  ____             _             
 |  _ \ __ _  __ _(_)_ __ ___    
 | |_) / _` |/ _` | | '_ ` _ \   
 |  _ < (_| | (_| | | | | | | |  
 |_| \_\__,_|\__, |_|_| |_| |_|  
                |_|              
"@ -ForegroundColor Cyan

Write-Host "Raqim Sovereign Cryptographic Runtime & Tooling Installer" -ForegroundColor Green
Write-Host "=================================================================="

# 1. Determine Tag
$Tag = $env:RAQIM_VERSION
if (-not $Tag) {
    Write-Host "Discovering latest release from GitHub..." -ForegroundColor Gray
    try {
        $Release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -Headers @{"User-Agent"="RaqimInstaller"}
        $Tag = $Release.tag_name
    } catch {
        $Tag = "v0.1.0"
    }
}

$Target = "raqim-windows-x86_64.zip"
$DownloadUrl = "https://github.com/$Repo/releases/download/$Tag/$Target"

Write-Host "Target: $Tag ($Target)" -ForegroundColor Cyan
Write-Host "Downloading from: $DownloadUrl"

$TmpDir = [System.IO.Path]::Combine([System.IO.Path]::GetTempPath(), [System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $TmpDir -Force | Out-Null
$ZipPath = Join-Path $TmpDir $Target

try {
    Invoke-WebRequest -Uri $DownloadUrl -OutFile $ZipPath -UseBasicParsing
    Expand-Archive -Path $ZipPath -DestinationPath $TmpDir -Force

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null

    $Binaries = @("raqim-core.exe", "raqim-cli.exe", "raqim-mcp.exe")
    foreach ($bin in $Binaries) {
        $Found = Get-ChildItem -Path $TmpDir -Filter $bin -Recurse -File | Select-Object -First 1
        if ($Found) {
            Move-Item -Path $Found.FullName -Destination (Join-Path $InstallDir $bin) -Force
            Write-Host "   ✔ Installed $bin" -ForegroundColor Green
        }
    }

    # Add to User PATH if not present
    $UserPath = [Environment]::GetEnvironmentVariable("Path", [EnvironmentVariableTarget]::User)
    if ($UserPath -split ";" -notcontains $InstallDir) {
        $NewPath = "$UserPath;$InstallDir"
        [Environment]::SetEnvironmentVariable("Path", $NewPath, [EnvironmentVariableTarget]::User)
        $env:Path = "$env:Path;$InstallDir"
        Write-Host "Added $InstallDir to User PATH environment variable." -ForegroundColor Cyan
    }

    Write-Host ""
    Write-Host "=================================================================="
    Write-Host "Alhamdulillah! Raqim installation completed successfully." -ForegroundColor Green
    Write-Host "=================================================================="
    Write-Host "Binaries installed in $InstallDir:"
    Write-Host "  * raqim-core.exe : Sovereign microkernel daemon"
    Write-Host "  * raqim-cli.exe  : Administrative PKI forge and WAL tooling"
    Write-Host "  * raqim-mcp.exe  : Model Context Protocol bridge"
    Write-Host ""
    Write-Host "Restart your terminal or run:"
    Write-Host "  `$env:Path += ';$InstallDir'" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "Get started:"
    Write-Host "  raqim-core.exe --help"
    Write-Host "  pip install raqim"
    Write-Host "=================================================================="
}
finally {
    Remove-Item -Path $TmpDir -Recurse -Force -ErrorAction SilentlyContinue
}
