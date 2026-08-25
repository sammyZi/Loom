<#
.SYNOPSIS
    Installs Loom for the current user.

.DESCRIPTION
    Copies the binary into %LOCALAPPDATA%\Programs\Loom, adds a Start Menu
    shortcut, and registers an entry in Add/Remove Programs so Windows can
    uninstall it the usual way.

    Per-user on purpose: no admin prompt, and nothing outside your profile is
    touched. Run with -Desktop for a desktop shortcut as well.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File installer\install.ps1
    powershell -ExecutionPolicy Bypass -File installer\install.ps1 -Desktop
#>
[CmdletBinding()]
param(
    # The binary to install. Must be a release build: debug builds read the
    # frontend from ..\frontend\out on disk, which will not exist once copied.
    [string] $Source = (Join-Path $PSScriptRoot "..\target\release\ide-ai.exe"),
    [string] $InstallDir = (Join-Path $env:LOCALAPPDATA "Programs\Loom"),
    [switch] $Desktop,
    # Accept the licence and privacy terms without being asked. For scripted
    # installs; it means the same thing as answering yes at the prompt.
    [switch] $Accept
)

$ErrorActionPreference = 'Stop'
$AppName = 'Loom'
$ExeName = 'ide-ai.exe'
$RegKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Loom'

function Fail($msg) {
    Write-Host "install: $msg" -ForegroundColor Red
    exit 1
}

# --- terms -----------------------------------------------------------------
# Shown before anything is copied or registered, so declining leaves the
# machine exactly as it was found.
$agreement = Join-Path $PSScriptRoot 'AGREEMENT.txt'
if (-not (Test-Path $agreement)) {
    Fail "AGREEMENT.txt is missing from $PSScriptRoot - refusing to install without showing the terms"
}

if ($Accept) {
    Write-Host "install: terms accepted with -Accept ($agreement)"
} else {
    Write-Host ""
    Get-Content $agreement | ForEach-Object { Write-Host $_ }
    Write-Host ""
    $answer = Read-Host "Accept these terms and install? [y/N]"
    if ($answer -notmatch '^(y|yes)$') {
        Write-Host "install: declined - nothing was installed" -ForegroundColor Yellow
        exit 1
    }
}
$acceptedOn = (Get-Date).ToString('yyyy-MM-dd HH:mm:ss')

# --- the binary ------------------------------------------------------------
if (-not (Test-Path $Source)) {
    Fail @"
no binary at $Source

Build one first:
    cargo build --release -p cli

Or point at an existing exe:
    install.ps1 -Source C:\path\to\ide-ai.exe
"@
}
$Source = (Resolve-Path $Source).Path

if ($Source -match '\\target\\debug\\') {
    Write-Host "install: that is a debug build — it loads the UI from frontend\out on disk" -ForegroundColor Yellow
    Write-Host "         and will show a blank window once installed. Build with --release." -ForegroundColor Yellow
    Fail "refusing to install a debug build"
}

# --- close a running copy, or the copy below fails on a locked file --------
Get-Process -Name 'ide-ai' -ErrorAction SilentlyContinue | ForEach-Object {
    Write-Host "install: closing the running app (pid $($_.Id))"
    try { $_.Kill(); $_.WaitForExit(5000) } catch {}
}

# --- files -----------------------------------------------------------------
if (-not (Test-Path $InstallDir)) { New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null }
$Target = Join-Path $InstallDir $ExeName
Copy-Item -Path $Source -Destination $Target -Force

# The uninstaller ships alongside the app so it still works after the source
# tree is deleted — Add/Remove Programs points straight at this copy.
Copy-Item -Path (Join-Path $PSScriptRoot 'uninstall.ps1') -Destination $InstallDir -Force
# The terms travel with the app: agreeing to something you can no longer read
# is not much of an agreement.
Copy-Item -Path $agreement -Destination $InstallDir -Force
# The uninstall window and the launcher that opens it without a console
# flash. Both optional: a console uninstall still works without them.
foreach ($extra in @('gui.ps1', 'uninstall-launcher.vbs')) {
    $from = Join-Path $PSScriptRoot $extra
    if (Test-Path $from) { Copy-Item -Path $from -Destination $InstallDir -Force }
}

$version = '0.1.0'
try {
    $fv = (Get-Item $Target).VersionInfo.FileVersion
    if ($fv) { $version = $fv.Trim() }
} catch {}

# --- shortcuts -------------------------------------------------------------
$shell = New-Object -ComObject WScript.Shell
$startMenu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
$links = @((Join-Path $startMenu "$AppName.lnk"))
if ($Desktop) { $links += (Join-Path ([Environment]::GetFolderPath('Desktop')) "$AppName.lnk") }

foreach ($link in $links) {
    $sc = $shell.CreateShortcut($link)
    $sc.TargetPath = $Target
    $sc.WorkingDirectory = $InstallDir
    $sc.IconLocation = $Target          # the icon is compiled into the exe
    $sc.Description = 'A coding agent with an IDE around it'
    $sc.Save()
}

# --- Add/Remove Programs ---------------------------------------------------
# Prefer the window: wscript is a GUI host, so Settings > Apps opens the
# uninstall dialog with nothing flashing first. powershell.exe launched
# straight from there would show a console before it could hide itself.
$launcher = Join-Path $InstallDir 'uninstall-launcher.vbs'
if ((Test-Path $launcher) -and (Test-Path (Join-Path $InstallDir 'gui.ps1'))) {
    $uninstallCommand = "wscript.exe `"$launcher`""
} else {
    $uninstallCommand = "powershell.exe -ExecutionPolicy Bypass -File `"$InstallDir\uninstall.ps1`""
}

$size = [math]::Round((Get-Item $Target).Length / 1KB)
New-Item -Path $RegKey -Force | Out-Null
$entries = @{
    DisplayName     = $AppName
    DisplayVersion  = $version
    Publisher       = 'Sam'
    InstallLocation = $InstallDir
    DisplayIcon     = $Target
    UninstallString = $uninstallCommand
    EstimatedSize   = $size
    NoModify        = 1
    NoRepair        = 1
    # Recorded so there is an answer to "which terms did I agree to, and when".
    AcceptedTerms   = "PolyForm Noncommercial 1.0.0 + privacy summary $version"
    AcceptedOn      = $acceptedOn
}
foreach ($name in $entries.Keys) {
    $value = $entries[$name]
    if ($value -is [int]) {
        New-ItemProperty -Path $RegKey -Name $name -Value $value -PropertyType DWord -Force | Out-Null
    } else {
        New-ItemProperty -Path $RegKey -Name $name -Value $value -PropertyType String -Force | Out-Null
    }
}

# --- WebView2: the window is blank without it ------------------------------
$wv = @(
    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
    'HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
    'HKCU:\Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
) | Where-Object { Test-Path $_ }

Write-Host ""
Write-Host "$AppName $version installed" -ForegroundColor Green
Write-Host "  $Target"
Write-Host "  Start Menu shortcut added$(if ($Desktop) { ' (and desktop)' })"
Write-Host "  Listed in Settings > Apps as '$AppName'"
if (-not $wv) {
    Write-Host ""
    Write-Host "  WebView2 runtime not found — the window will be blank without it:" -ForegroundColor Yellow
    Write-Host "  https://developer.microsoft.com/microsoft-edge/webview2/" -ForegroundColor Yellow
}
Write-Host ""
Write-Host "Your sessions and API keys live in $env:APPDATA\ide-ai and are untouched by"
Write-Host "install or uninstall unless you ask for them to be removed."
