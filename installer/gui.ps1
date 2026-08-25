<#
.SYNOPSIS
    Windowed front end for the Loom installer.

.DESCRIPTION
    A WinForms wizard over install.ps1 / uninstall.ps1. The scripts still do
    all the work: this only collects the choices and shows what happened, so
    there is one implementation of installing and it is the tested one.

    WinForms because it ships with Windows - a GUI installer should not need a
    toolkit downloaded to build it.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File gui.ps1
    powershell -ExecutionPolicy Bypass -File gui.ps1 -Uninstall
#>
[CmdletBinding()]
param(
    [switch] $Uninstall,
    [string] $Source,
    [string] $InstallDir,
    # Builds the window and closes it again. Proves the layout has no runtime
    # errors without needing someone to watch it.
    [switch] $SelfTest
)

$ErrorActionPreference = 'Stop'

# If something launched this with a console attached, hide it: the window is
# the interface, and a black box behind it is just noise. Harmless when there
# is no console to hide.
try {
    Add-Type -Namespace Loom -Name Con -MemberDefinition @'
[DllImport("kernel32.dll")] public static extern IntPtr GetConsoleWindow();
[DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
'@ -ErrorAction Stop
    $console = [Loom.Con]::GetConsoleWindow()
    if ($console -ne [IntPtr]::Zero) { [void][Loom.Con]::ShowWindow($console, 0) }  # SW_HIDE
} catch {}

# Windows stretches a DPI-unaware window like a bitmap, which is what made the
# text look soft on a scaled display. Awareness has to be declared before the
# first window exists, and powershell.exe's own manifest does not declare it,
# so it is done here by hand. Newest API first, older ones as fallbacks.
try {
    Add-Type -Namespace Loom -Name Dpi -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
[DllImport("shcore.dll")] public static extern int SetProcessDpiAwareness(int value);
[DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
'@ -ErrorAction Stop
    $done = $false
    try { $done = [Loom.Dpi]::SetProcessDpiAwarenessContext((New-Object IntPtr(-4))) } catch {}   # per-monitor v2
    if (-not $done) { try { [void][Loom.Dpi]::SetProcessDpiAwareness(2) } catch {} }              # per-monitor
    if (-not $done) { try { [void][Loom.Dpi]::SetProcessDPIAware() } catch {} }                   # system
} catch {}

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
[System.Windows.Forms.Application]::EnableVisualStyles()

$AppName = 'Loom'
$Version = '0.1.0'
$Ink = [System.Drawing.Color]::FromArgb(20, 20, 19)
$Paper = [System.Drawing.Color]::FromArgb(250, 249, 247)
$Accent = [System.Drawing.Color]::FromArgb(15, 118, 110)
$Muted = [System.Drawing.Color]::FromArgb(110, 106, 99)

function New-Label($text, $x, $y, $w, $h, $size, $color, $bold) {
    $l = New-Object System.Windows.Forms.Label
    $l.Text = $text
    $l.SetBounds($x, $y, $w, $h)
    $style = if ($bold) { [System.Drawing.FontStyle]::Bold } else { [System.Drawing.FontStyle]::Regular }
    $l.Font = New-Object System.Drawing.Font('Segoe UI', $size, $style)
    $l.ForeColor = $color
    $l.BackColor = [System.Drawing.Color]::Transparent
    return $l
}

function New-Button($text, $x, $y, $w, $h, $primary) {
    $b = New-Object System.Windows.Forms.Button
    $b.Text = $text
    $b.SetBounds($x, $y, $w, $h)
    $b.Font = New-Object System.Drawing.Font('Segoe UI', 9.5)
    $b.FlatStyle = [System.Windows.Forms.FlatStyle]::Flat
    $b.FlatAppearance.BorderSize = 1
    if ($primary) {
        $b.BackColor = $Accent
        $b.ForeColor = [System.Drawing.Color]::White
        $b.FlatAppearance.BorderColor = $Accent
    } else {
        $b.BackColor = $Paper
        $b.ForeColor = $Ink
        $b.FlatAppearance.BorderColor = [System.Drawing.Color]::FromArgb(206, 202, 200)
    }
    return $b
}

# Runs a script in its own process: install.ps1 calls exit on failure, which
# would take this window down with it if it ran in-process.
function Invoke-Step($script, $argList) {
    # ProcessStartInfo rather than Start-Process: -WindowStyle Hidden is
    # ignored the moment output is redirected, which is what made a console
    # window blink up during the install. CreateNoWindow is not.
    $quoted = @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "`"$script`"")
    foreach ($a in $argList) {
        if ($a -match '\s') { $quoted += "`"$a`"" } else { $quoted += $a }
    }

    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = 'powershell.exe'
    $psi.Arguments = ($quoted -join ' ')
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true

    $p = [System.Diagnostics.Process]::Start($psi)
    # Output here is a few lines; reading to end before waiting is safe and
    # keeps the pipe from filling.
    $text = $p.StandardOutput.ReadToEnd()
    $text += $p.StandardError.ReadToEnd()
    $p.WaitForExit()
    return @{ Code = $p.ExitCode; Text = $text }
}

# WebView2 renders the whole UI. Windows 11 ships it and Edge keeps it updated,
# but a clean Windows 10 may not have it, and without it the app opens a blank
# window - which looks like a broken build rather than a missing prerequisite.
function Test-WebView2 {
    $keys = @(
        'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
        'HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
        'HKCU:\Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
    )
    foreach ($k in $keys) {
        if (Test-Path $k) {
            $v = (Get-ItemProperty $k -Name pv -ErrorAction SilentlyContinue).pv
            if ($v -and $v -ne '0.0.0.0') { return $v }
        }
    }
    return $null
}

$form = New-Object System.Windows.Forms.Form
$form.Text = if ($Uninstall) { "$AppName Uninstall" } else { "$AppName Setup" }
$form.Size = New-Object System.Drawing.Size(600, 560)
$form.StartPosition = 'CenterScreen'
$form.FormBorderStyle = 'FixedDialog'
$form.MaximizeBox = $false
# The bounds below are written at 96 DPI; this scales them for anything else.
$form.AutoScaleDimensions = New-Object System.Drawing.SizeF(96, 96)
$form.AutoScaleMode = [System.Windows.Forms.AutoScaleMode]::Dpi
$form.BackColor = $Paper
try {
    $exe = Join-Path $PSScriptRoot 'ide-ai.exe'
    if ($Source -and (Test-Path $Source)) { $exe = $Source }
    if (Test-Path $exe) { $form.Icon = [System.Drawing.Icon]::ExtractAssociatedIcon($exe) }
} catch {}

$form.Controls.Add((New-Label $AppName 28 24 300 34 17 $Ink $true))
$sub = if ($Uninstall) { "Remove version $Version from this computer" } else { "Version $Version" }
$form.Controls.Add((New-Label $sub 30 60 400 22 9.5 $Muted $false))

$rule = New-Object System.Windows.Forms.Panel
$rule.SetBounds(28, 92, 536, 1)
$rule.BackColor = [System.Drawing.Color]::FromArgb(224, 220, 216)
$form.Controls.Add($rule)

$status = New-Label '' 28 430 536 40 9 $Muted $false
$form.Controls.Add($status)

if ($Uninstall) {
    # ---- uninstall ----------------------------------------------------------
    $form.Controls.Add((New-Label "This removes the app, its Start Menu shortcut and its entry in Settings > Apps." 30 120 520 40 10 $Ink $false))

    $keep = New-Object System.Windows.Forms.Label
    $keep.Text = "Your chat history and API keys are kept by default. They live in`r`n$env:APPDATA\ide-ai and are not needed by any other program."
    $keep.SetBounds(30, 176, 520, 56)
    $keep.Font = New-Object System.Drawing.Font('Segoe UI', 9.5)
    $keep.ForeColor = $Muted
    $form.Controls.Add($keep)

    $wipe = New-Object System.Windows.Forms.CheckBox
    $wipe.Text = 'Also delete my sessions and API keys'
    $wipe.SetBounds(30, 248, 400, 24)
    $wipe.Font = New-Object System.Drawing.Font('Segoe UI', 9.5)
    $wipe.ForeColor = $Ink
    $form.Controls.Add($wipe)

    $go = New-Button 'Uninstall' 380 480 90 34 $true
    $cancel = New-Button 'Cancel' 478 480 86 34 $false
    $form.Controls.AddRange(@($go, $cancel))

    $go.Add_Click({
        $go.Enabled = $false
        $cancel.Enabled = $false
        $status.ForeColor = $Muted
        $status.Text = 'Removing...'
        $form.Refresh()
        $a = @('-Force')
        if ($wipe.Checked) { $a += '-RemoveData' }
        $r = Invoke-Step (Join-Path $PSScriptRoot 'uninstall.ps1') $a
        if ($r.Code -eq 0) {
            $status.ForeColor = $Accent
            $status.Text = "$AppName has been removed."
            $go.Visible = $false
            $cancel.Text = 'Close'
            $cancel.Enabled = $true
        } else {
            $status.ForeColor = [System.Drawing.Color]::FromArgb(190, 60, 50)
            $status.Text = "Uninstall failed. $($r.Text)"
            $go.Enabled = $true
            $cancel.Enabled = $true
        }
    })
} else {
    # ---- install ------------------------------------------------------------
    $form.Controls.Add((New-Label 'Licence and privacy terms' 30 116 400 22 10 $Ink $true))

    $terms = New-Object System.Windows.Forms.TextBox
    $terms.Multiline = $true
    $terms.ReadOnly = $true
    $terms.ScrollBars = 'Vertical'
    $terms.SetBounds(30, 144, 534, 210)
    $terms.Font = New-Object System.Drawing.Font('Consolas', 8.5)
    $terms.BackColor = [System.Drawing.Color]::White
    $terms.ForeColor = $Ink
    $agreement = Join-Path $PSScriptRoot 'AGREEMENT.txt'
    if (Test-Path $agreement) {
        $terms.Text = (Get-Content $agreement -Raw)
    } else {
        # Fail closed: no terms shown means no install offered.
        $terms.Text = 'AGREEMENT.txt is missing, so the terms cannot be shown.'
    }
    $terms.Select(0, 0)
    $form.Controls.Add($terms)

    $accept = New-Object System.Windows.Forms.CheckBox
    $accept.Text = 'I accept the licence and privacy terms'
    $accept.SetBounds(30, 364, 400, 24)
    $accept.Font = New-Object System.Drawing.Font('Segoe UI', 9.5)
    $accept.ForeColor = $Ink
    $form.Controls.Add($accept)

    $desktop = New-Object System.Windows.Forms.CheckBox
    $desktop.Text = 'Create a desktop shortcut'
    $desktop.SetBounds(30, 392, 400, 24)
    $desktop.Font = New-Object System.Drawing.Font('Segoe UI', 9.5)
    $desktop.ForeColor = $Ink
    $form.Controls.Add($desktop)

    $wv = Test-WebView2
    if (-not $wv) {
        $warn = New-Label "WebView2 runtime not found. Loom needs it to draw its window - install it, then run this setup again." 30 418 350 40 8.5 ([System.Drawing.Color]::FromArgb(170, 90, 20)) $false
        $form.Controls.Add($warn)
        $get = New-Button 'Get WebView2' 380 418 110 26 $false
        $get.Add_Click({
            Start-Process 'https://developer.microsoft.com/microsoft-edge/webview2/'
        })
        $form.Controls.Add($get)
    }

    $go = New-Button 'Install' 380 480 90 34 $true
    $cancel = New-Button 'Cancel' 478 480 86 34 $false
    $go.Enabled = $false
    $form.Controls.AddRange(@($go, $cancel))

    # The button stays dead until the box is ticked: the same rule the console
    # installer enforces, made visible.
    $accept.Add_CheckedChanged({
        $go.Enabled = $accept.Checked -and (Test-Path (Join-Path $PSScriptRoot 'AGREEMENT.txt'))
    })

    $go.Add_Click({
        $go.Enabled = $false
        $cancel.Enabled = $false
        $accept.Enabled = $false
        $desktop.Enabled = $false
        $status.ForeColor = $Muted
        $status.Text = 'Installing...'
        $form.Refresh()

        $a = @('-Accept')
        if ($Source) { $a += @('-Source', $Source) }
        elseif (Test-Path (Join-Path $PSScriptRoot 'ide-ai.exe')) {
            $a += @('-Source', (Join-Path $PSScriptRoot 'ide-ai.exe'))
        }
        if ($InstallDir) { $a += @('-InstallDir', $InstallDir) }
        if ($desktop.Checked) { $a += '-Desktop' }

        $r = Invoke-Step (Join-Path $PSScriptRoot 'install.ps1') $a
        if ($r.Code -eq 0) {
            $status.ForeColor = $Accent
            $status.Text = "$AppName is installed. Find it in the Start Menu."
            $go.Visible = $false
            $cancel.Text = 'Close'
            $cancel.Enabled = $true
        } else {
            $status.ForeColor = [System.Drawing.Color]::FromArgb(190, 60, 50)
            $status.Text = "Install failed. $($r.Text)"
            $go.Enabled = $true
            $cancel.Enabled = $true
            $accept.Enabled = $true
            $desktop.Enabled = $true
        }
    })
}

$cancel.Add_Click({ $form.Close() })
$form.CancelButton = $cancel

if ($SelfTest) {
    # Force the handles to be created so a bad control is an error here rather
    # than in front of whoever runs it.
    $form.CreateControl()
    foreach ($c in $form.Controls) { $null = $c.Handle }
    Write-Host "gui: built '$($form.Text)' with $($form.Controls.Count) controls"
    $form.Dispose()
    exit 0
}

[void]$form.ShowDialog()
