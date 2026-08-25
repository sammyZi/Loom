' Loom uninstaller launcher.
' Add/Remove Programs runs this instead of powershell.exe directly: wscript is
' a windowed host, so nothing flashes up before the uninstall window appears.
Dim shell, here
Set shell = CreateObject("WScript.Shell")
here = Left(WScript.ScriptFullName, InStrRev(WScript.ScriptFullName, "\"))
shell.Run "powershell.exe -NoProfile -ExecutionPolicy Bypass -File """ & here & "gui.ps1"" -Uninstall", 0, False
