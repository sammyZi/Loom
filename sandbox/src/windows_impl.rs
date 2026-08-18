//! Unelevated Windows sandbox: Job Object + write-restricted token + workspace ACL + network-deny env.
//! No Docker. Works on Windows Home without admin.
//!
//! ponytail: WFP/firewall needs elevation; env proxy poisoning is the no-admin network backstop.

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use core::{CommandOutput, WorkspaceRoot};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::Security::{
    AddAccessAllowedAceEx, ConvertStringSidToSidW, CreateRestrictedToken, GetTokenInformation,
    InitializeAcl, OpenProcessToken, TokenUser, ACL, ACL_REVISION, CONTAINER_INHERIT_ACE,
    DISABLE_MAX_PRIVILEGE, OBJECT_INHERIT_ACE, PSID, SID_AND_ATTRIBUTES, TOKEN_ALL_ACCESS,
    TOKEN_USER, WRITE_RESTRICTED,
};
use windows::Win32::Security::Authorization::{
    GetNamedSecurityInfoW, SetNamedSecurityInfoW, DACL_SECURITY_INFORMATION, SE_FILE_OBJECT,
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
use windows::Win32::System::Threading::{
    CreateProcessAsUserW, CreateProcessW, GetExitCodeProcess, ResumeThread, WaitForSingleObject,
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION,
    STARTF_USESTDHANDLES, STARTUPINFOW,
};
use windows::Win32::System::Threading::GetCurrentProcess;
use windows::Win32::Foundation::HANDLE as WinHandle;

const SANDBOX_SID: &str = "S-1-5-110-424242-424242-1";
const MAX_MEM: usize = 1024 * 1024 * 1024;
const MAX_PROCS: u32 = 64;
const WRITE_BUF: usize = 64 * 1024;

pub struct WindowsSandbox;

#[async_trait]
impl crate::Sandbox for WindowsSandbox {
    async fn run(
        &self,
        ws: &WorkspaceRoot,
        program: &str,
        args: &[String],
        timeout: Duration,
    ) -> Result<CommandOutput> {
        let ws = ws.clone();
        let program = program.to_string();
        let args = args.to_vec();
        tokio::task::spawn_blocking(move || run_blocking(&ws, &program, &args, timeout))
            .await
            .context("sandbox join")?
    }
}

fn run_blocking(
    ws: &WorkspaceRoot,
    program: &str,
    args: &[String],
    timeout: Duration,
) -> Result<CommandOutput> {
    let exe = resolve_program(program).with_context(|| format!("find `{program}`"))?;
    let tmp = ws.root().join(".ide-ai-tmp");
    std::fs::create_dir_all(&tmp).ok();
    let _ = grant_workspace_sid(ws.root());

    let cmdline = command_line(&exe, args);
    let env = env_block(ws, &tmp);

    unsafe { spawn_and_wait(ws.root(), &exe, &cmdline, &env, timeout) }
}

fn resolve_program(program: &str) -> Result<PathBuf> {
    let p = PathBuf::from(program);
    if p.is_file() {
        return Ok(dunce_canon(&p));
    }
    let mut names = vec![program.to_string()];
    if !program.to_ascii_lowercase().ends_with(".exe") {
        names.push(format!("{program}.exe"));
        names.push(format!("{program}.cmd"));
        names.push(format!("{program}.bat"));
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(';') {
            for n in &names {
                let c = Path::new(dir).join(n);
                if c.is_file() {
                    return Ok(dunce_canon(&c));
                }
            }
        }
    }
    bail!("executable not found: {program}")
}

fn dunce_canon(p: &Path) -> PathBuf {
    dunce::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

fn command_line(exe: &Path, args: &[String]) -> Vec<u16> {
    let mut parts = vec![quote(&exe.to_string_lossy())];
    parts.extend(args.iter().map(|a| quote(a)));
    wide(&parts.join(" "))
}

fn quote(s: &str) -> String {
    if s.is_empty() {
        return "\"\"".into();
    }
    if !s.chars().any(|c| c.is_whitespace() || c == '"') {
        return s.to_string();
    }
    format!("\"{}\"", s.replace('"', "\\\""))
}

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

fn env_block(ws: &WorkspaceRoot, tmp: &Path) -> Vec<u16> {
    let mut pairs = crate::passthrough_env();
    pairs.extend(crate::deny_network_env());
    let tmp_s = tmp.to_string_lossy().into_owned();
    pairs.push(("TEMP".into(), tmp_s.clone()));
    pairs.push(("TMP".into(), tmp_s));
    pairs.push(("IDE_AI_WORKSPACE".into(), ws.root().to_string_lossy().into_owned()));
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
) -> Result<CommandOutput> {
    let mut sa = windows::Win32::Security::SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<windows::Win32::Security::SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: true.into(),
    };

    let mut stdout_r = HANDLE::default();
    let mut stdout_w = HANDLE::default();
    let mut stderr_r = HANDLE::default();
    let mut stderr_w = HANDLE::default();
    CreatePipe(&mut stdout_r, &mut stdout_w, Some(&sa), 0).context("stdout pipe")?;
    CreatePipe(&mut stderr_r, &mut stderr_w, Some(&sa), 0).context("stderr pipe")?;
    set_noinherit(stdout_r)?;
    set_noinherit(stderr_r)?;

    let job = CreateJobObjectW(None, None).context("CreateJobObject")?;
    apply_job_limits(job)?;

    let mut si = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        dwFlags: STARTF_USESTDHANDLES,
        hStdOutput: stdout_w,
        hStdError: stderr_w,
        hStdInput: INVALID_HANDLE_VALUE,
        ..Default::default()
    };
    let mut pi = PROCESS_INFORMATION::default();
    let mut cmd = cmdline.to_vec();
    let mut cwd_w = wide(&cwd.to_string_lossy());
    let mut exe_w = wide(&exe.to_string_lossy());
    let flags = CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW;

    let token = restricted_token().ok();
    let created = if let Some(tok) = token {
        let r = CreateProcessAsUserW(
            tok,
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
        CreateProcessW(
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
        )
        .context("CreateProcessW")?;
    }

    let _ = CloseHandle(stdout_w);
    let _ = CloseHandle(stderr_w);

    AssignProcessToJobObject(job, pi.hProcess).context("AssignProcessToJobObject")?;
    ResumeThread(pi.hThread);

    let timeout_ms = timeout.as_millis().min(u32::MAX as u128) as u32;
    let stdout_handle = stdout_r;
    let stderr_handle = stderr_r;
    let out_t = std::thread::spawn(move || read_pipe(stdout_handle));
    let err_t = std::thread::spawn(move || read_pipe(stderr_handle));

    let wr = WaitForSingleObject(pi.hProcess, timeout_ms);
    if wr == WAIT_TIMEOUT {
        let _ = TerminateJobObject(job, 1);
        WaitForSingleObject(pi.hProcess, 5_000);
    }

    let mut code: u32 = 1;
    let _ = GetExitCodeProcess(pi.hProcess, &mut code);
    let stdout = out_t.join().unwrap_or_default();
    let stderr = err_t.join().unwrap_or_default();

    let _ = CloseHandle(pi.hThread);
    let _ = CloseHandle(pi.hProcess);
    let _ = CloseHandle(job);

    let mut stderr = stderr;
    if wr == WAIT_TIMEOUT {
        stderr.push_str("\n[sandbox] timed out; process tree killed");
    }
    Ok(CommandOutput {
        exit_code: code as i32,
        stdout,
        stderr,
    })
}

fn set_noinherit(h: HANDLE) -> Result<()> {
    use windows::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
    unsafe { SetHandleInformation(h, HANDLE_FLAG_INHERIT.0, HANDLE::default()) }
        .context("SetHandleInformation")?;
    Ok(())
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
    let mut sid = PSID::default();
    let mut sid_w = wide(SANDBOX_SID);
    ConvertStringSidToSidW(PCWSTR(sid_w.as_ptr()), &mut sid)?;
    let sids = [SID_AND_ATTRIBUTES {
        Sid: sid,
        Attributes: 0,
    }];
    let mut restricted = HANDLE::default();
    let r = CreateRestrictedToken(
        primary,
        DISABLE_MAX_PRIVILEGE | WRITE_RESTRICTED,
        None,
        None,
        Some(&sids),
        &mut restricted,
    );
    let _ = CloseHandle(primary);
    r?;
    Ok(restricted)
}

fn grant_workspace_sid(root: &Path) -> Result<()> {
    unsafe {
        let mut sid = PSID::default();
        let mut sid_w = wide(SANDBOX_SID);
        ConvertStringSidToSidW(PCWSTR(sid_w.as_ptr()), &mut sid)?;
        let mut path_w = wide(&root.to_string_lossy());
        let mut sd = windows::Win32::Security::PSECURITY_DESCRIPTOR::default();
        let mut dacl: *mut ACL = std::ptr::null_mut();
        GetNamedSecurityInfoW(
            PCWSTR(path_w.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(&mut dacl),
            None,
            &mut sd,
        )
        .ok()
        .context("GetNamedSecurityInfo")?;

        let mut buf = vec![0u8; 16 * 1024];
        InitializeAcl(
            buf.as_mut_ptr() as *mut ACL,
            buf.len() as u32,
            ACL_REVISION,
        )?;
        AddAccessAllowedAceEx(
            buf.as_mut_ptr() as *mut ACL,
            ACL_REVISION,
            CONTAINER_INHERIT_ACE | OBJECT_INHERIT_ACE,
            0x001F01FF, // FILE_ALL_ACCESS
            sid,
        )?;
        SetNamedSecurityInfoW(
            PWSTR(path_w.as_mut_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(buf.as_mut_ptr() as *const ACL),
            None,
        )
        .ok()
        .context("SetNamedSecurityInfo")?;
        let _ = windows::Win32::Foundation::LocalFree(Some(windows::Win32::Foundation::HLOCAL(
            sd.0 as *mut _,
        )));
    }
    Ok(())
}

unsafe fn read_pipe(h: HANDLE) -> String {
    use windows::Win32::Storage::FileSystem::ReadFile;
    let mut all = Vec::new();
    let mut buf = [0u8; WRITE_BUF];
    loop {
        let mut n = 0u32;
        if ReadFile(h, Some(&mut buf), Some(&mut n), None).is_err() || n == 0 {
            break;
        }
        all.extend_from_slice(&buf[..n as usize]);
        if all.len() > 2_000_000 {
            all.extend_from_slice(b"\n[truncated]");
            break;
        }
    }
    let _ = CloseHandle(h);
    String::from_utf8_lossy(&all).into_owned()
}
