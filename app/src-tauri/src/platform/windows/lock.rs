//! Session lock and sleep (`wipeOnLock`).
//!
//! A hidden top-level window on its own thread receives `WM_WTSSESSION_CHANGE` (registered with
//! `WTSRegisterSessionNotification`) and `WM_POWERBROADCAST` (also registered with
//! `RegisterSuspendResumeNotification`). It is never shown and has no content.
//! (Named `lock.rs`; `session.rs` is the session secrets.)

use std::sync::OnceLock;

use windows::core::w;
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::RegisterSuspendResumeNotification;
use windows::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW,
    TranslateMessage, DEVICE_NOTIFY_WINDOW_HANDLE, MSG, PBT_APMSUSPEND, WINDOW_EX_STYLE,
    WM_POWERBROADCAST, WM_WTSSESSION_CHANGE, WNDCLASSW, WS_OVERLAPPED, WTS_SESSION_LOCK,
};

use crate::wipe::WipeReason;

type Callback = Box<dyn Fn(WipeReason) + Send + Sync>;

static CALLBACK: OnceLock<Callback> = OnceLock::new();

/// Starts the watcher. `on_event` runs on the watcher thread; call once per process.
pub fn spawn(on_event: impl Fn(WipeReason) + Send + Sync + 'static) {
    if CALLBACK.set(Box::new(on_event)).is_err() {
        return;
    }
    std::thread::spawn(|| {
        // SAFETY: standard window class registration and message loop on this thread. The
        // window and class live until the process ends; pointers passed are static strings.
        unsafe {
            let Ok(module) = GetModuleHandleW(None) else {
                return;
            };
            let class = w!("RedactorLockWatcher");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: module.into(),
                lpszClassName: class,
                ..Default::default()
            };
            if RegisterClassW(&wc) == 0 {
                return;
            }
            let Ok(hwnd) = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                w!(""),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(module.into()),
                None,
            ) else {
                return;
            };
            let _ = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);
            let _ = RegisterSuspendResumeNotification(HANDLE(hwnd.0), DEVICE_NOTIFY_WINDOW_HANDLE);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    });
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let reason = match msg {
        WM_WTSSESSION_CHANGE if wparam.0 as u32 == WTS_SESSION_LOCK => Some(WipeReason::Lock),
        WM_POWERBROADCAST if wparam.0 as u32 == PBT_APMSUSPEND => Some(WipeReason::Sleep),
        _ => None,
    };
    if let (Some(reason), Some(cb)) = (reason, CALLBACK.get()) {
        cb(reason);
    }
    // SAFETY: forwarding the unmodified message to the default handler.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}
