//! Keeps the shelf on the desktop, like desktop icons, so "Show desktop"
//! (Win+D) reveals it instead of hiding it with every other window.

use std::sync::atomic::{AtomicIsize, Ordering};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetWindow, SetWindowLongPtrW, SetWindowPos, EVENT_SYSTEM_FOREGROUND,
    GWLP_HWNDPARENT, GW_OWNER, HWND_BOTTOM, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    WINEVENT_OUTOFCONTEXT,
};

/// The shelf window, for the event hook (which can't capture state).
static SHELF: AtomicIsize = AtomicIsize::new(0);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// Points the shelf at the current desktop window, which Explorer recreates
/// whenever it restarts. Returns false when there is no desktop window.
fn own_by_desktop(hwnd: HWND) -> bool {
    unsafe {
        let progman = FindWindowW(wide("Progman").as_ptr(), std::ptr::null());
        if progman.is_null() {
            return false;
        }
        if GetWindow(hwnd, GW_OWNER) != progman {
            SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, progman as isize);
        }
        true
    }
}

fn send_to_back(hwnd: HWND) {
    unsafe {
        SetWindowPos(
            hwnd,
            HWND_BOTTOM,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

/// Runs whenever another window comes to the front. Leaving Show desktop
/// raises the shelf above the windows coming back, so push it behind again.
/// Also re-attaches the shelf if Explorer restarted since the last check.
unsafe extern "system" fn on_foreground_change(
    _hook: HWINEVENTHOOK,
    _event: u32,
    foreground: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    let shelf = SHELF.load(Ordering::Relaxed) as HWND;
    if !shelf.is_null() && foreground != shelf {
        own_by_desktop(shelf);
        send_to_back(shelf);
    }
}

/// Makes the desktop window (`Progman`) the shelf's owner. Windows the desktop
/// owns are treated as part of it and stay visible on Show desktop.
pub fn attach(hwnd: HWND) {
    unsafe {
        if !own_by_desktop(hwnd) {
            return;
        }
        // Changing the owner brings the window forward; put it back behind
        // every other window (just above the desktop it now belongs to).
        send_to_back(hwnd);

        SHELF.store(hwnd as isize, Ordering::Relaxed);
        // Out-of-context hooks are delivered through this (the main) thread's
        // message loop, which Tauri keeps running.
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            std::ptr::null_mut(),
            Some(on_foreground_change),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
    }
}
