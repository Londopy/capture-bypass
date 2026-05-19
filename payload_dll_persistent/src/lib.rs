// payload_dll_persistent -- same as payload_dll but keeps looping.
//
// Instead of stripping once and exiting, the worker re-applies WDA_NONE
// every 500 ms for as long as the host process is alive. Handles apps
// that call SetWindowDisplayAffinity on a timer to fight back.

use std::ffi::c_void;

use windows::{
    Win32::{
        Foundation::{BOOL, HMODULE, HWND, LPARAM, TRUE},
        System::{
            LibraryLoader::DisableThreadLibraryCalls,
            SystemServices::DLL_PROCESS_ATTACH,
            Threading::{CreateThread, GetCurrentProcessId, Sleep, THREAD_CREATION_FLAGS},
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GetWindowThreadProcessId, SetWindowDisplayAffinity, WDA_NONE,
        },
    },
};

#[no_mangle]
pub unsafe extern "system" fn DllMain(
    module: HMODULE,
    call_reason: u32,
    _reserved: *mut c_void,
) -> BOOL {
    if call_reason == DLL_PROCESS_ATTACH {
        let _ = DisableThreadLibraryCalls(module);
        let _ = CreateThread(None, 0, Some(worker_thread), None, THREAD_CREATION_FLAGS(0), None);
    }
    TRUE
}

// Loops forever, clearing protection every 500 ms
unsafe extern "system" fn worker_thread(_: *mut c_void) -> u32 {
    // Let the loader finish before we start touching user32
    Sleep(100);

    loop {
        let pid = GetCurrentProcessId();
        let _ = EnumWindows(Some(strip_callback), LPARAM(pid as isize));
        Sleep(500);
    }
}

unsafe extern "system" fn strip_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let target_pid = lparam.0 as u32;
    let mut owner_pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut owner_pid));
    if owner_pid == target_pid {
        let _ = SetWindowDisplayAffinity(hwnd, WDA_NONE);
    }
    TRUE
}
