<#
.SYNOPSIS
    Removes Loom for the current user.

.DESCRIPTION
    Deletes the installed binary, the shortcuts and the Add/Remove Programs
    entry.

    Your sessions and API keys in %APPDATA%\ide-ai are KEPT by default —
    uninstalling an app is not a reason to destroy the work it holds, and a
    sealed key file is not something to delete on a guess. Pass -RemoveData to
    take them with it.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File uninstall.ps1
    powershell -ExecutionPolicy Bypass -File uninstall.ps1 -RemoveData
#>
[CmdletBinding()]
param(
    # Also delete sessions.db, config.json and the sealed keys.dat.
    [switch] $RemoveData,
    # Skip the confirmation prompt (used by Add/Remove Programs).
    [switch] $Force
)

$ErrorActionPreference = 'Stop'
$AppName = 'Loom'
$RegKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Loom'
$DataDir = Join-Path $env:APPDATA 'ide-ai'

# Where the app was installed, per the registry; fall back to the default.
$InstallDir = Join-Path $env:LOCALAPPDATA 'Programs\Loom'
if (Test-Path $RegKey) {
    $recorded = (Get-ItemProperty -Path $RegKey -Name InstallLocation -ErrorAction SilentlyContinue).InstallLocation
    if ($recorded) { $InstallDir = $recorded }
}

if (-not $Force) {
    $answer = Read-Host "Remove $AppName from $InstallDir? [y/N]"
    if ($answer -notmatch '^(y|yes)$') {
        Write-Host "uninstall: cancelled"
        exit 0
    }
}

# --- stop it before deleting it -------------------------------------------
Get-Process -Name 'ide-ai' -ErrorAction SilentlyContinue | ForEach-Object {
    Write-Host "uninstall: closing the running app (pid $($_.Id))"
    try { $_.Kill(); $_.WaitForExit(5000) } catch {}
}

# --- shortcuts -------------------------------------------------------------
$links = @(
    (Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\$AppName.lnk"),
    (Join-Path ([Environment]::GetFolderPath('Desktop')) "$AppName.lnk")
)
foreach ($link in $links) {
    if (Test-Path $link) {
        Remove-Item $link -Force
        Write-Host "uninstall: removed $link"
    }
}

# --- registry --------------------------------------------------------------
if (Test-Path $RegKey) {
    Remove-Item $RegKey -Recurse -Force
    Write-Host "uninstall: removed the Add/Remove Programs entry"
}

# --- files -----------------------------------------------------------------
# This script usually lives inside the directory it is deleting, so the folder
# goes on a short delay after PowerShell has let go of it.
if (Test-Path $InstallDir) {
    $self = $MyInvocation.MyCommand.Path
    $inside = $self -and $self.StartsWith($InstallDir, [StringComparison]::OrdinalIgnoreCase)

    Get-ChildItem -Path $InstallDir -File -ErrorAction SilentlyContinue |
        Where-Object { -not ($inside -and $_.FullName -eq $self) } |
        ForEach-Object { Remove-Item $_.FullName -Force -ErrorAction SilentlyContinue }

    if ($inside) {
        Start-Process -WindowStyle Hidden -FilePath 'cmd.exe' `
            -ArgumentList '/c', 'timeout', '/t', '2', '/nobreak', '>nul', '&', 'rmdir', '/s', '/q', "`"$InstallDir`""
        Write-Host "uninstall: removed $InstallDir"
    } else {
        Remove-Item $InstallDir -Recurse -Force -ErrorAction SilentlyContinue
        Write-Host "uninstall: removed $InstallDir"
    }
}

# --- user data -------------------------------------------------------------
if ($RemoveData) {
    if (Test-Path $DataDir) {
        Remove-Item $DataDir -Recurse -Force
        Write-Host "uninstall: removed your sessions and keys ($DataDir)" -ForegroundColor Yellow
    }
} elseif (Test-Path $DataDir) {
    Write-Host ""
    Write-Host "Kept your sessions and API keys:" -ForegroundColor Cyan
    Write-Host "  $DataDir"
    Write-Host "  Delete that folder yourself, or re-run with -RemoveData, to be rid of them."
}

Write-Host ""
Write-Host "$AppName uninstalled" -ForegroundColor Green
