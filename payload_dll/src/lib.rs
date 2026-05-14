//! payload_dll — injected into a target process to strip WDA capture protection.
//!
//! When loaded via LoadLibraryA (e.g. via CreateRemoteThread injection), DllMain
//! fires on DLL_PROCESS_ATTACH.  We immediately spawn a worker thread so we are
//! not holding the loader lock while calling back into user32.
//!
//! The worker:
//!   1. Sleeps a short moment to let the loader lock drop.
//!   2. Enumerates every top-level window.
//!   3. For windows owned by *this* process it calls
//!      SetWindowDisplayAffinity(hwnd, WDA_NONE), clearing the capture-protection
//!      flag that normally makes the window appear black in screenshots / OBS.

use std::ffi::c_void;

use windows::{
    Win32::{
        Foundation::{BOOL, HMODULE, HWND, LPARAM, TRUE},
        System::{
            LibraryLoader::DisableThreadLibraryCalls,
            SystemServices::DLL_PROCESS_ATTACH,
            Threading::{CreateThread, GetCurrentProcessId, Sleep, THREAD_CREATION_FLAGS},
        },
        UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, SetWindowDisplayAffinity, WDA_NONE},
    },
};

// ── DllMain ──────────────────────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "system" fn DllMain(
    module: HMODULE,
    call_reason: u32,
    _reserved: *mut c_void,
) -> BOOL {
    if call_reason == DLL_PROCESS_ATTACH {
        // Suppress per-thread DLL notifications — we don't need them.
        let _ = DisableThreadLibraryCalls(module);

        // Spawn worker thread so we never touch user32 while holding the
        // loader lock (avoids potential deadlock on some app configurations).
        let _ = CreateThread(None, 0, Some(worker_thread), None, THREAD_CREATION_FLAGS(0), None);
    }
    TRUE
}

// ── Worker thread ─────────────────────────────────────────────────────────────

unsafe extern "system" fn worker_thread(_param: *mut c_void) -> u32 {
    // Brief pause — gives the loader time to fully release its lock before
    // we start enumerating windows.
    Sleep(50);

    let pid = GetCurrentProcessId();

    // EnumWindows iterates every top-level window; the callback filters to
    // those belonging to our injected process.
    let _ = EnumWindows(Some(strip_callback), LPARAM(pid as isize));

    0
}

// ── Enumeration callback ──────────────────────────────────────────────────────

unsafe extern "system" fn strip_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let target_pid = lparam.0 as u32;

    let mut owner_pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut owner_pid));

    if owner_pid == target_pid {
        // WDA_NONE (0) clears the display-affinity flag, making the window
        // visible to all capture sources again.
        let _ = SetWindowDisplayAffinity(hwnd, WDA_NONE);
    }

    TRUE // return TRUE to continue enumeration
}
