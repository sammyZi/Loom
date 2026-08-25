# Installer

Per-user install for Windows. No admin prompt, nothing written outside your
profile, and Windows can uninstall it from Settings like any other app.

## Install

```powershell
cargo build --release -p cli
powershell -ExecutionPolicy Bypass -File installer\install.ps1
```

Add `-Desktop` for a desktop shortcut. `-Source <path>` installs a binary from
somewhere else, `-InstallDir <path>` puts it somewhere else.

It shows the licence and privacy summary ([AGREEMENT.txt](AGREEMENT.txt)) and
will not write anything to the machine until you accept. `-Accept` agrees
without the prompt, for scripted installs.

What it does:

| | |
| --- | --- |
| Binary | `%LOCALAPPDATA%\Programs\Loom\ide-ai.exe` |
| Shortcut | Start Menu (plus desktop with `-Desktop`) |
| Registration | `HKCU\...\Uninstall\Loom` — shows in Settings → Apps |
| Terms | A copy of `AGREEMENT.txt`, plus `AcceptedTerms` / `AcceptedOn` in the registry |

It refuses a debug build. Debug binaries read the UI from `frontend\out` on
disk rather than from inside the exe, so an installed one opens a blank window.

## Uninstall

From Settings → Apps → Loom → Uninstall, or:

```powershell
powershell -ExecutionPolicy Bypass -File "$env:LOCALAPPDATA\Programs\Loom\uninstall.ps1"
```

**Your sessions and API keys are kept.** They live in `%APPDATA%\ide-ai`
(`sessions.db`, `config.json`, and the DPAPI-sealed `keys.dat`), and
uninstalling an app is no reason to destroy the work it holds. Pass
`-RemoveData` to delete them too.

A copy of `uninstall.ps1` is installed next to the binary, so uninstalling
still works after this repository is gone.

## Notes

- The app needs the **WebView2 runtime**. Windows 11 ships it; the installer
  warns with a download link if it is missing.
- These scripts are UTF-8 **with a BOM** on purpose. PowerShell 5.1 reads a
  BOM-less file as CP1252, where the third byte of an em dash decodes to a
  curly quote — which PowerShell treats as a string delimiter, so the script
  fails to parse. Keep the BOM if you edit them.
- Distributing a single `setup.exe` instead would mean [Inno
  Setup](https://jrsoftware.org/isinfo.php) and a build step; these scripts
  need nothing beyond what Windows already has.
