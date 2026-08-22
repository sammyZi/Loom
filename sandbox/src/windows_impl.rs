//! Unelevated Windows sandbox: Job Object + restricted token + network-deny env.
//! No Docker. Works on Windows Home without admin.
//!
//! ponytail: skip rewriting NTFS DACLs (easy to lock the user out of their own folder).
//! WRITE_RESTRICTED needs those ACLs, so we drop max privileges + job limits instead.

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use ide_core::{CommandOutput, WorkspaceRoot};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, HANDLE_FLAGS,
    WAIT_TIMEOUT,
};
use windows::Win32::Security::{
    CreateRestrictedToken, DISABLE_MAX_PRIVILEGE, TOKEN_ALL_ACCESS,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject, TerminateJobObject,
    JobObjectBasicUIRestrictions, JobObjectExtendedLimitInformation,
    JOBOBJECT_BASIC_UI_RESTRICTIONS, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOB_OBJECT_LIMIT_PROCESS_MEMORY, JOB_OBJECT_UILIMIT_DESKTOP, JOB_OBJECT_UILIMIT_EXITWINDOWS,
    JOB_OBJECT_UILIMIT_HANDLES, JOB_OBJECT_UILIMIT_READCLIPBOARD, JOB_OBJECT_UILIMIT_WRITECLIPBOARD,
};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows::Win32::System::Threading::{
    CreateProcessAsUserW, CreateProcessW, GetCurrentProcess, GetExitCodeProcess, OpenProcessToken,
    ResumeThread, WaitForSingleObject, CREATE_NO_WINDOW, CREATE_SUSPENDED,
    CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOW,
};
use windows::Win32::Security::SECURITY_ATTRIBUTES;

const MAX_MEM: usize = 1024 * 1024 * 1024;
const MAX_PROCS: u32 = 64;
const WRITE_BUF: usize = 64 * 1024;

pub struct WindowsSandbox;

#[async_trait]
impl crate::Sandbox for WindowsSandbox {
    async fn run_streaming(
        &self,
        ws: &WorkspaceRoot,
        program: &str,
        args: &[String],
        timeout: Duration,
        cancel: &CancellationToken,
        on_output: Option<crate::OutputSink>,
        stdin: Option<crate::InputSource>,
    ) -> Result<CommandOutput> {
        let ws = ws.clone();
        let program = program.to_string();
        let args = args.to_vec();
        let cancel = cancel.clone();
        tokio::task::spawn_blocking(move || {
            run_blocking(&ws, &program, &args, timeout, &cancel, on_output, stdin)
        })
        .await
        .context("sandbox join")?
    }
}

/// Scratch space for a run, kept inside the project so commands write next to
/// the code they build. It self-ignores: a `*` gitignore inside the directory
/// hides the whole thing from `git status` without editing the user's own
/// .gitignore — otherwise every project the sandbox touched grew a permanent
/// untracked folder, which also inflated the commit bar's changed-file count.
fn scratch_dir(ws: &WorkspaceRoot) -> PathBuf {
    let tmp = ws.root().join(".ide-ai-tmp");
    if std::fs::create_dir_all(&tmp).is_err() {
        return tmp;
    }
    let marker = tmp.join(".gitignore");
    if !marker.exists() {
        let _ = std::fs::write(&marker, "*\n");
    }
    tmp
}

fn run_blocking(
    ws: &WorkspaceRoot,
    program: &str,
    args: &[String],
    timeout: Duration,
    cancel: &CancellationToken,
    on_output: Option<crate::OutputSink>,
    stdin: Option<crate::InputSource>,
) -> Result<CommandOutput> {
    let resolved = resolve_program(program).with_context(|| format!("find `{program}`"))?;
    let tmp = scratch_dir(ws);
    let (exe, cmdline): (PathBuf, Vec<u16>) = if is_batch_script(&resolved) {
        // The wrapped line is spliced together by hand: cmd's `/s /c` parsing
        // expects raw inner quotes, and the CRT-style \" escaping used by
        // `command_line` mangles them into a UNC-ish path ("The network path
        // was not found").
        let line = [
            quote(&comspec().to_string_lossy()),
            "/d".into(),
            "/s".into(),
            "/c".into(),
            script_wrapped(&resolved, args),
        ]
        .join(" ");
        (comspec(), wide(&line))
    } else {
        (resolved.clone(), command_line(&resolved, args))
    };
    let env = env_block(ws, &tmp);
    unsafe { spawn_and_wait(ws.root(), &exe, &cmdline, &env, timeout, cancel, on_output, stdin) }
}

/// Windows PATHEXT semantics decide what an extensionless name can become:
/// `.exe` beats script extensions, and the bare name itself is accepted only
/// when it is a real executable image. Node ships an extensionless sh script
/// named `npm` right next to `npm.cmd`; the old first-match-wins scan picked
/// that script and every npm call died with "CreateProcessW".
fn resolve_program(program: &str) -> Result<PathBuf> {
    let path = std::env::var("PATH").unwrap_or_default();
    resolve_in_search_path(program, &path)
}

/// Split out so tests never mutate process-global `PATH` — cargo runs this
/// crate's tests in parallel, and a swapped env var made `powershell` vanish
/// mid-run for every other test.
fn resolve_in_search_path(program: &str, search_path: &str) -> Result<PathBuf> {
    let direct = PathBuf::from(program);
    if program.contains('\\') || program.contains('/') {
        if direct.is_file() {
            return Ok(dunce::canonicalize(&direct).unwrap_or(direct));
        }
        bail!("executable not found: {program}");
    }
    let has_ext = direct.extension().is_some();
    let names: Vec<String> = if has_ext {
        vec![program.to_string()]
    } else {
        let mut v: Vec<String> =
            pathext().into_iter().map(|e| format!("{program}{e}")).collect();
        // Last resort: the bare name, but only as a PE image (checked below),
        // never as an arbitrary file that happens to sit on PATH.
        v.push(program.to_string());
        v
    };
    for dir in search_path.split(';') {
        if dir.is_empty() {
            continue;
        }
        for n in &names {
            let c = Path::new(dir).join(n);
            if !c.is_file() {
                continue;
            }
            if n == program && !is_executable_image(&c) {
                continue;
            }
            return Ok(dunce::canonicalize(&c).unwrap_or(c));
        }
    }
    if direct.is_file() && (has_ext || is_executable_image(&direct)) {
        return Ok(dunce::canonicalize(&direct).unwrap_or(direct));
    }
    bail!("executable not found: {program}")
}

/// `%PATHEXT%` lowercased, defaulting to the stock Windows order.
fn pathext() -> Vec<String> {
    let raw = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    let mut out: Vec<String> = Vec::new();
    for e in raw.split(';') {
        let e = e.trim().to_ascii_lowercase();
        if e.len() > 1 && e.starts_with('.') && !out.contains(&e) {
            out.push(e);
        }
    }
    if out.is_empty() {
        out = [".com", ".exe", ".bat", ".cmd"].iter().map(|s| s.to_string()).collect();
    }
    out
}

/// A real PE image starts with "MZ". Anything else — sh scripts, text — cannot
/// be launched by CreateProcessW and must lose to script extensions.
fn is_executable_image(p: &Path) -> bool {
    use std::io::Read;
    let mut magic = [0u8; 2];
    std::fs::File::open(p)
        .and_then(|mut f| f.read_exact(&mut magic))
        .is_ok()
        && &magic == b"MZ"
}

fn is_batch_script(p: &Path) -> bool {
    matches!(
        p.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("cmd") | Some("bat")
    )
}

fn comspec() -> PathBuf {
    std::env::var_os("COMSPEC")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\Windows\\System32\\cmd.exe"))
}

/// `cmd /d /s /c "<script> <args…>"`: `/s` makes cmd take the whole quoted
/// remainder as one command line, so quoted paths inside survive intact. The
/// inner quotes stay raw — cmd toggles on them itself.
fn script_wrapped(script: &Path, args: &[String]) -> String {
    let mut inner = quote(&script.to_string_lossy());
    for a in args {
        inner.push(' ');
        inner.push_str(&quote(a));
    }
    format!("\"{inner}\"")
}

fn command_line(exe: &Path, args: &[String]) -> Vec<u16> {
    let mut parts = vec![quote(&exe.to_string_lossy())];
    parts.extend(args.iter().map(|a| quote(a)));
    wide(&parts.join(" "))
}

/// Windows command-line quoting per the argv rules every CRT parses:
/// backslash runs are literal except immediately before a quote, where they
/// double up and the quote itself is escaped. The old naive `"` -> `\"` swap
/// broke args like `C:\dir\"` and trailing-backslash paths.
fn quote(s: &str) -> String {
    if s.is_empty() {
        return "\"\"".into();
    }
    if !s.chars().any(|c| c.is_whitespace() || c == '"') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 8);
    out.push('"');
    let mut backslashes = 0usize;
    for c in s.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                // A run of n backslashes directly before a quote doubles to 2n,
                // plus one more to escape the quote itself.
                for _ in 0..(backslashes * 2 + 1) {
                    out.push('\\');
                }
                out.push('"');
                backslashes = 0;
            }
            c => {
                for _ in 0..backslashes {
                    out.push('\\');
                }
                out.push(c);
                backslashes = 0;
            }
        }
    }
    // Trailing backslashes double up so they cannot escape the closing quote.
    for _ in 0..(backslashes * 2) {
        out.push('\\');
    }
    out.push('"');
    out
}

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod quote_tests {
    #[test]
    fn plain_args_stay_unquoted() {
        assert_eq!(super::quote("ping"), "ping");
        assert_eq!(super::quote("127.0.0.1"), "127.0.0.1");
    }

    #[test]
    fn spaces_quote_without_noise() {
        assert_eq!(super::quote("hello world"), "\"hello world\"");
    }

    /// Regression: an early draft emitted one stray backslash before the
    /// closing quote for every arg, so `echo hi` printed `hi\`.
    #[test]
    fn no_stray_backslash_when_none_are_present() {
        assert_eq!(super::quote("a b"), "\"a b\"");
        // no whitespace or quotes: stays unquoted entirely
        assert_eq!(super::quote("tick-$_"), "tick-$_");
    }

    #[test]
    fn quotes_and_backslash_runs_escape_per_argv_rules() {
        // n backslashes before a quote -> 2n+1 backslashes then an escaped quote
        assert_eq!(super::quote("say \"hi\""), r#""say \"hi\"""#);
        assert_eq!(super::quote("a\\\".b"), r#""a\\\".b""#);
        // trailing backslashes double up so they cannot escape the closing
        // quote; without spaces the arg never needs quoting in the first place
        assert_eq!(super::quote("C:\\dir\\"), "C:\\dir\\");
        assert_eq!(super::quote("C:\\my dir\\"), "\"C:\\my dir\\\\\"");
    }
}

#[cfg(test)]
mod resolve_tests {
    use super::*;

    #[test]
    fn pathext_defaults_to_stock_windows_order() {
        let exts = super::pathext();
        let joined = exts.join(",");
        assert!(joined.starts_with(".com,"), "{joined}");
        assert!(joined.contains(".exe") && joined.contains(".cmd"), "{joined}");
    }

    #[test]
    fn batch_scripts_are_detected_by_extension_only() {
        assert!(super::is_batch_script(Path::new("C:\\x\\npm.CMD")));
        assert!(super::is_batch_script(Path::new("C:\\x\\run.bat")));
        assert!(!super::is_batch_script(Path::new("C:\\x\\node.exe")));
        assert!(!super::is_batch_script(Path::new("C:\\x\\npm")));
    }

    #[test]
    fn text_files_are_not_executable_images() {
        let dir = std::env::temp_dir().join("ide-ai-sandbox-resolve-test");
        std::fs::create_dir_all(&dir).unwrap();
        let sh = dir.join("npm");
        std::fs::write(&sh, "#!/bin/sh\necho hi\n").unwrap();
        assert!(!super::is_executable_image(&sh));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn script_command_line_wraps_in_cmd_s_semantics() {
        let wrapped = super::script_wrapped(
            Path::new("C:\\Program Files\\nodejs\\npm.cmd"),
            &["run".into(), "build".into()],
        );
        // Raw inner quotes, one outer pair — never CRT \" escapes, which cmd
        // misreads and turns into a UNC path.
        assert_eq!(wrapped, "\"\"C:\\Program Files\\nodejs\\npm.cmd\" run build\"");
    }

    #[test]
    fn comspec_falls_back_to_system_cmd() {
        let p = super::comspec();
        assert!(p.as_os_str().to_string_lossy().to_ascii_lowercase().contains("cmd.exe"));
    }

    /// A bare name resolves to its PATHEXT candidate, never to an
    /// extensionless lookalike sitting in the same directory. This is the
    /// exact shape of the npm failure: `npm` (sh script) vs `npm.cmd`.
    #[test]
    fn bare_name_never_resolves_to_a_non_image_file() {
        let dir = std::env::temp_dir().join("ide-ai-sandbox-resolve-path-test");
        std::fs::create_dir_all(&dir).unwrap();
        let sh = dir.join("fake-tool");
        std::fs::write(&sh, "#!/bin/sh\necho hi\n").unwrap();

        // Only the extensionless non-image exists: nothing may resolve.
        let only_script = dir.to_str().unwrap().to_string();
        assert!(super::resolve_in_search_path("fake-tool", &only_script).is_err());

        // With a real image present the same lookup succeeds via .exe.
        std::fs::copy(
            super::comspec(),
            dir.join("fake-tool.exe"),
        )
        .ok();
        assert!(super::resolve_in_search_path("fake-tool", &only_script).is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(windows)]
    #[test]
    fn cmd_resolves_through_pathext_candidates() {
        let found = super::resolve_program("cmd").expect("cmd should resolve");
        assert_eq!(
            found.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()),
            Some("exe".into())
        );
    }

    /// The reported bug, end to end: Node ships an extensionless sh script
    /// named `npm` beside `npm.cmd`, the old resolver picked the script and
    /// CreateProcessW failed. Resolution must land on the batch file, and the
    /// spawn must wrap it in cmd so `--version` actually runs.
    #[cfg(windows)]
    #[tokio::test]
    async fn npm_resolves_to_the_batch_file_and_executes() {
        let path = std::env::var("PATH").unwrap_or_default();
        let Ok(resolved) = super::resolve_in_search_path("npm", &path) else {
            return; // no Node on this machine; nothing to prove
        };
        assert_eq!(
            resolved.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()),
            Some("cmd".into()),
            "bare `npm` must resolve to npm.cmd, got {resolved:?}"
        );

        let dir = std::env::temp_dir().join("ide-ai-sandbox-npm-test");
        std::fs::create_dir_all(&dir).unwrap();
        let ws = ide_core::WorkspaceRoot::open(&dir).unwrap();
        let out = crate::native()
            .run(
                &ws,
                "npm",
                &["--version".into()],
                Duration::from_secs(120),
                &tokio_util::sync::CancellationToken::new(),
            )
            .await
            .expect("npm --version should spawn");
        assert_eq!(out.exit_code, 0, "stderr: {:?}", out.stderr);
        let v = out.stdout.trim();
        assert!(
            v.chars().next().is_some_and(|c| c.is_ascii_digit()),
            "expected a semver on stdout, got {v:?} (stderr {:?})",
            out.stderr
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}

fn env_block(ws: &WorkspaceRoot, tmp: &Path) -> Vec<u16> {
    let mut pairs = crate::passthrough_env();
    pairs.extend(crate::deny_network_env());
    let tmp_s = tmp.to_string_lossy().into_owned();
    pairs.push(("TEMP".into(), tmp_s.clone()));
    pairs.push(("TMP".into(), tmp_s));
    pairs.push((
        "IDE_AI_WORKSPACE".into(),
        ws.root().to_string_lossy().into_owned(),
    ));
    pairs.sort_by(|a, b| a.0.to_ascii_uppercase().cmp(&b.0.to_ascii_uppercase()));
    let mut block: Vec<u16> = Vec::new();
    for (k, v) in pairs {
        block.extend(OsStr::new(&format!("{k}={v}")).encode_wide());
        block.push(0);
    }
    block.push(0);
    block
}

unsafe fn spawn_and_wait(
    cwd: &Path,
    exe: &Path,
    cmdline: &[u16],
    env: &[u16],
    timeout: Duration,
    cancel: &CancellationToken,
    on_output: Option<crate::OutputSink>,
    stdin: Option<crate::InputSource>,
) -> Result<CommandOutput> {
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: true.into(),
    };

    let mut stdout_r = HANDLE::default();
    let mut stdout_w = HANDLE::default();
    let mut stderr_r = HANDLE::default();
    let mut stderr_w = HANDLE::default();
    let mut stdin_r = HANDLE::default();
    let mut stdin_w = HANDLE::default();
    CreatePipe(&mut stdout_r, &mut stdout_w, Some(&sa), 0).context("stdout pipe")?;
    CreatePipe(&mut stderr_r, &mut stderr_w, Some(&sa), 0).context("stderr pipe")?;
    CreatePipe(&mut stdin_r, &mut stdin_w, Some(&sa), 0).context("stdin pipe")?;
    SetHandleInformation(stdout_r, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0)).ok();
    SetHandleInformation(stderr_r, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0)).ok();
    // Only the child inherits the read end; our write end must stay private or
    // the pipe never reports EOF once we drop it.
    SetHandleInformation(stdin_w, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0)).ok();

    let job = CreateJobObjectW(None, None).context("CreateJobObject")?;
    apply_job_limits(job)?;

    let si = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        dwFlags: STARTF_USESTDHANDLES,
        hStdOutput: stdout_w,
        hStdError: stderr_w,
        hStdInput: stdin_r,
        ..Default::default()
    };
    let mut pi = PROCESS_INFORMATION::default();
    let mut cmd = cmdline.to_vec();
    let cwd_w = wide(&cwd.to_string_lossy());
    let exe_w = wide(&exe.to_string_lossy());
    let flags = CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW;

    let token = restricted_token().ok();
    let created = if let Some(tok) = token {
        let r = CreateProcessAsUserW(
            Some(tok),
            PCWSTR(exe_w.as_ptr()),
            Some(PWSTR(cmd.as_mut_ptr())),
            None,
            None,
            true,
            flags,
            Some(env.as_ptr() as *const _),
            PCWSTR(cwd_w.as_ptr()),
            &si,
            &mut pi,
        );
        let _ = CloseHandle(tok);
        r
    } else {
        Err(windows::core::Error::from_win32())
    };

    if created.is_err() {
        match CreateProcessW(
            PCWSTR(exe_w.as_ptr()),
            Some(PWSTR(cmd.as_mut_ptr())),
            None,
            None,
            true,
            flags,
            Some(env.as_ptr() as *const _),
            PCWSTR(cwd_w.as_ptr()),
            &si,
            &mut pi,
        ) {
            Ok(()) => {}
            Err(e) => {
                // Nothing was spawned, so the job and pipe handles we created
                // above would leak — close them before bailing.
                let _ = CloseHandle(job);
                let _ = CloseHandle(stdout_r);
                let _ = CloseHandle(stderr_r);
                return Err(e).context("CreateProcessW");
            }
        }
    }

    let _ = CloseHandle(stdout_w);
    let _ = CloseHandle(stderr_w);
    // The child holds its own copy of the read end now.
    let _ = CloseHandle(stdin_r);

    // Feed keystrokes in on a thread. With no source we close the write end at
    // once so a prompting command reads EOF and gives up, rather than blocking
    // until the timeout with nobody able to answer it.
    match stdin {
        Some(mut rx) => {
            let raw = stdin_w.0 as usize;
            std::thread::spawn(move || {
                let h = HANDLE(raw as *mut _);
                while let Some(text) = rx.blocking_recv() {
                    let bytes = text.into_bytes();
                    let mut written = 0u32;
                    if unsafe { WriteFile(h, Some(&bytes), Some(&mut written), None) }.is_err() {
                        break;
                    }
                }
                unsafe {
                    let _ = CloseHandle(h);
                }
            });
        }
        None => {
            let _ = CloseHandle(stdin_w);
        }
    }

    AssignProcessToJobObject(job, pi.hProcess).context("AssignProcessToJobObject")?;
    ResumeThread(pi.hThread);

    let stdout_raw = stdout_r.0 as usize;
    let stderr_raw = stderr_r.0 as usize;
    let out_sink = on_output.clone();
    let err_sink = on_output.clone();
    let out_t = std::thread::spawn(move || read_pipe(HANDLE(stdout_raw as *mut _), out_sink));
    let err_t = std::thread::spawn(move || read_pipe(HANDLE(stderr_raw as *mut _), err_sink));

    // Wake regularly rather than blocking for the whole timeout, so a cancel
    // (Ctrl+C, or closing the terminal tab) kills the tree promptly instead of
    // leaving something like `ping -t` running until the deadline.
    let deadline = Instant::now() + timeout;
    let mut stopped: Option<&str> = None;
    loop {
        if cancel.is_cancelled() {
            stopped = Some("cancelled");
            break;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            stopped = Some("timed out");
            break;
        }
        let slice = left.min(Duration::from_millis(100)).as_millis() as u32;
        if WaitForSingleObject(pi.hProcess, slice) != WAIT_TIMEOUT {
            break; // exited on its own
        }
    }
    if stopped.is_some() {
        // Kills every descendant too, which is what closes the pipes and lets
        // the reader threads below finish.
        let _ = TerminateJobObject(job, 1);
        WaitForSingleObject(pi.hProcess, 5_000);
    }

    let mut code: u32 = 1;
    let _ = GetExitCodeProcess(pi.hProcess, &mut code);
    let stdout = out_t.join().unwrap_or_default();
    let mut stderr = err_t.join().unwrap_or_default();

    let _ = CloseHandle(pi.hThread);
    let _ = CloseHandle(pi.hProcess);
    let _ = CloseHandle(job);

    // A cancel is the user's own doing and the terminal already shows ^C, so it
    // stays silent like a real shell. A timeout is not asked for, so it says so.
    // Written after both pipes closed, hence pushed to any follower explicitly.
    if stopped == Some("timed out") {
        let note = "\n[sandbox] timed out; process tree killed";
        if let Some(tx) = &on_output {
            let _ = tx.send(note.to_string());
        }
        stderr.push_str(note);
    }
    Ok(CommandOutput {
        exit_code: code as i32,
        stdout,
        stderr,
    })
}

unsafe fn apply_job_limits(job: HANDLE) -> Result<()> {
    let mut ext = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    ext.BasicLimitInformation.LimitFlags =
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
            | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
    ext.BasicLimitInformation.ActiveProcessLimit = MAX_PROCS;
    ext.ProcessMemoryLimit = MAX_MEM;
    SetInformationJobObject(
        job,
        JobObjectExtendedLimitInformation,
        &ext as *const _ as *const _,
        std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
    )
    .context("job limits")?;

    let ui = JOBOBJECT_BASIC_UI_RESTRICTIONS {
        UIRestrictionsClass: JOB_OBJECT_UILIMIT_DESKTOP
            | JOB_OBJECT_UILIMIT_EXITWINDOWS
            | JOB_OBJECT_UILIMIT_HANDLES
            | JOB_OBJECT_UILIMIT_READCLIPBOARD
            | JOB_OBJECT_UILIMIT_WRITECLIPBOARD,
    };
    let _ = SetInformationJobObject(
        job,
        JobObjectBasicUIRestrictions,
        &ui as *const _ as *const _,
        std::mem::size_of::<JOBOBJECT_BASIC_UI_RESTRICTIONS>() as u32,
    );
    Ok(())
}

unsafe fn restricted_token() -> Result<HANDLE> {
    let mut primary = HANDLE::default();
    OpenProcessToken(GetCurrentProcess(), TOKEN_ALL_ACCESS, &mut primary)?;
    let mut restricted = HANDLE::default();
    let r = CreateRestrictedToken(
        primary,
        DISABLE_MAX_PRIVILEGE,
        None,
        None,
        None,
        &mut restricted,
    );
    let _ = CloseHandle(primary);
    r?;
    Ok(restricted)
}

/// Drain one pipe, forwarding each chunk to `sink` as it arrives so callers can
/// follow a long command, while still accumulating the whole text to return.
/// Once the accumulation cap hits, reading continues but the text is discarded:
/// stopping the reads used to let a full pipe block a chatty child forever.
unsafe fn read_pipe(h: HANDLE, sink: Option<crate::OutputSink>) -> String {
    const CAP: usize = 2_000_000;
    let mut all: Vec<u8> = Vec::new();
    let mut truncated = false;
    let mut buf = [0u8; WRITE_BUF];
    loop {
        let mut n = 0u32;
        if ReadFile(h, Some(&mut buf), Some(&mut n), None).is_err() || n == 0 {
            break;
        }
        let chunk = &buf[..n as usize];
        if truncated {
            continue; // keep draining; content is already capped
        }
        all.extend_from_slice(chunk);
        if let Some(tx) = &sink {
            // A closed receiver just means nobody is following any more; the
            // command keeps running and the full output is still returned.
            let _ = tx.send(String::from_utf8_lossy(chunk).into_owned());
        }
        if all.len() > CAP {
            all.extend_from_slice(b"\n[truncated]");
            truncated = true;
        }
    }
    let _ = CloseHandle(h);
    String::from_utf8_lossy(&all).into_owned()
}
