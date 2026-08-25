#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! `loom-setup.exe` — the whole app in one file you can double-click.
//!
//! It carries the release binary and the two PowerShell scripts inside itself,
//! unpacks them to a temp folder and runs one of them. The scripts already do
//! the real work (copy, shortcut, Add/Remove Programs entry, uninstall), and
//! they are the part that has been tested, so this wrapper deliberately adds
//! no install logic of its own.
//!
//! Build (in this order — the payload has to exist before it can be embedded):
//!     cargo build --release -p cli
//!     cargo build --release --manifest-path installer/setup/Cargo.toml

use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// CREATE_NO_WINDOW. A windows-subsystem process that spawns a console app
/// gets a console allocated for the child, which flashed up a black box behind
/// the installer window.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The app itself. `include_bytes!` is relative to this source file.
const APP_EXE: &[u8] = include_bytes!("../../../target/release/ide-ai.exe");
/// Kept as bytes, not `include_str!`: these files are UTF-8 *with a BOM*, and
/// PowerShell 5.1 needs that BOM to read them as UTF-8 rather than CP1252.
const INSTALL_PS1: &[u8] = include_bytes!("../../install.ps1");
const UNINSTALL_PS1: &[u8] = include_bytes!("../../uninstall.ps1");
/// Shown and accepted before anything is written to the machine.
const AGREEMENT: &[u8] = include_bytes!("../../AGREEMENT.txt");
/// The windowed front end, which drives the two scripts above.
const GUI_PS1: &[u8] = include_bytes!("../../gui.ps1");
/// Registered as the uninstall command: a GUI host, so Settings > Apps opens
/// the window without a console appearing first.
const UNINSTALL_VBS: &[u8] = include_bytes!("../../uninstall-launcher.vbs");

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let uninstall = args.iter().any(|a| {
        let a = a.trim_start_matches(['-', '/']).to_ascii_lowercase();
        a == "uninstall"
    });
    let no_pause = args.iter().any(|a| a == "--no-pause");
    // GUI unless asked otherwise: --console keeps the text flow, which is what
    // scripted installs and CI want.
    let console = args.iter().any(|a| a == "--console");

    // Everything else is passed through to the script: -Desktop, -RemoveData,
    // -Force, -InstallDir <path>.
    let forwarded: Vec<&String> = args
        .iter()
        .filter(|a| {
            let f = a.trim_start_matches(['-', '/']).to_ascii_lowercase();
            f != "uninstall" && *a != "--no-pause" && *a != "--console"
        })
        .collect();

    let code = match run(uninstall, console, &forwarded) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("setup: {e}");
            1
        }
    };

    // Only the text flow has a console to keep open; the window speaks for
    // itself. Double-clicked from Explorer the console would otherwise close
    // the instant we exit, taking the result with it.
    if console && !no_pause {
        println!();
        print!("Press Enter to close...");
        let _ = std::io::stdout().flush();
        let mut sink = String::new();
        let _ = std::io::stdin().read_line(&mut sink);
    }
    std::process::exit(code);
}

fn run(uninstall: bool, console: bool, forwarded: &[&String]) -> std::io::Result<i32> {
    let dir = unpack_dir()?;
    std::fs::create_dir_all(&dir)?;

    // Both, always: install.ps1 copies uninstall.ps1 in beside the app so
    // Add/Remove Programs still works once this temp folder is gone.
    std::fs::write(dir.join("install.ps1"), INSTALL_PS1)?;
    std::fs::write(dir.join("uninstall.ps1"), UNINSTALL_PS1)?;
    std::fs::write(dir.join("AGREEMENT.txt"), AGREEMENT)?;
    std::fs::write(dir.join("gui.ps1"), GUI_PS1)?;
    std::fs::write(dir.join("uninstall-launcher.vbs"), UNINSTALL_VBS)?;

    // The window is the default; --console keeps the text flow for scripted
    // installs, which cannot answer a dialog.
    let script = if console {
        dir.join(if uninstall { "uninstall.ps1" } else { "install.ps1" })
    } else {
        dir.join("gui.ps1")
    };

    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-File"])
        .arg(&script);
    // The text flow needs the console it was started with; the window must not
    // conjure one.
    if !console {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    if !console && uninstall {
        cmd.arg("-Uninstall");
    }

    if uninstall {
        if console {
            println!("Loom — uninstall\n");
        }
    } else {
        // The payload only needs unpacking when we are installing; the
        // uninstaller works off what is already on disk.
        let exe = dir.join("ide-ai.exe");
        std::fs::write(&exe, APP_EXE)?;
        if console {
            println!("Loom — setup\n");
        }
        cmd.arg("-Source").arg(&exe);
    }
    for arg in forwarded {
        cmd.arg(arg);
    }

    let status = cmd.status()?;
    // Best-effort: the exe we just unpacked has been copied elsewhere by now,
    // and a leftover temp folder is not worth failing the install over.
    let _ = std::fs::remove_dir_all(&dir);
    Ok(status.code().unwrap_or(1))
}

/// A per-process temp folder, so two copies running at once cannot fight over
/// the same unpacked payload.
fn unpack_dir() -> std::io::Result<PathBuf> {
    let base = std::env::var_os("TEMP")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join(format!("loom-setup-{}", std::process::id()));
    if Path::new(&dir).exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(dir)
}
