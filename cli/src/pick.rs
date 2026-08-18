use std::path::PathBuf;

pub fn pick_folder() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        windows_pick()
    }
    #[cfg(not(windows))]
    {
        rfd::FileDialog::new().set_title("Open Folder").pick_folder()
    }
}

#[cfg(windows)]
fn windows_pick() -> Option<PathBuf> {
    use raw_window_handle::{
        DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
        RawWindowHandle, Win32WindowHandle, WindowHandle, WindowsDisplayHandle,
    };
    use std::num::NonZeroIsize;
    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::HiDpi::{
        SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_SYSTEM_AWARE,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, DestroyWindow,
        GetForegroundWindow, GetWindowThreadProcessId, RegisterClassW, SetForegroundWindow,
        SetWindowPos, ShowWindow, ASFW_ANY, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
        SW_SHOW, WINDOW_EX_STYLE, WNDCLASSW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
    };

    struct Owner(NonZeroIsize);
    impl HasWindowHandle for Owner {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            let handle = Win32WindowHandle::new(self.0);
            Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(handle)) })
        }
    }
    impl HasDisplayHandle for Owner {
        fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
            Ok(unsafe {
                DisplayHandle::borrow_raw(RawDisplayHandle::Windows(WindowsDisplayHandle::new()))
            })
        }
    }

    unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        unsafe { DefWindowProcW(hwnd, msg, w, l) }
    }

    unsafe {
        let prev = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_SYSTEM_AWARE);
        let class = w!("IdeAiPickerHost");
        let hinstance = GetModuleHandleW(None).ok().map(Into::into);
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance.unwrap_or_default(),
            lpszClassName: class,
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(WS_EX_TOPMOST.0 | WS_EX_TOOLWINDOW.0),
            class,
            w!("Open Folder"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            hinstance,
            None,
        )
        .ok();

        let path = match hwnd {
            Some(hwnd) => {
                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = AllowSetForegroundWindow(ASFW_ANY);
                let fg = GetForegroundWindow();
                let fg_tid = GetWindowThreadProcessId(fg, None);
                let our_tid = GetCurrentThreadId();
                if fg_tid != 0 && fg_tid != our_tid {
                    let _ = AttachThreadInput(our_tid, fg_tid, true);
                }
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
                );
                let _ = SetForegroundWindow(hwnd);
                if fg_tid != 0 && fg_tid != our_tid {
                    let _ = AttachThreadInput(our_tid, fg_tid, false);
                }
                let picked = match NonZeroIsize::new(hwnd.0 as isize).map(Owner) {
                    Some(owner) => rfd::FileDialog::new()
                        .set_title("Open Folder")
                        .set_parent(&owner)
                        .pick_folder(),
                    None => rfd::FileDialog::new().set_title("Open Folder").pick_folder(),
                };
                let _ = DestroyWindow(hwnd);
                picked
            }
            None => rfd::FileDialog::new().set_title("Open Folder").pick_folder(),
        };

        let _ = SetThreadDpiAwarenessContext(prev);
        path
    }
}
