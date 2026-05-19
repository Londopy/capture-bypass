// payload_dll -- gets injected into a target process to clear WDA capture protection.
//
// When LoadLibraryA fires DllMain with DLL_PROCESS_ATTACH, we immediately spin
// up a worker thread so we're not messing with anything while the loader lock
// is still held.
//
// The worker waits a tiny bit, then calls SetWindowDisplayAffinity(WDA_NONE)
// on every window that belongs to this process.

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

#[no_mangle]
pub unsafe extern "system" fn DllMain(
    module: HMODULE,
    call_reason: u32,
    _reserved: *mut c_void,
) -> BOOL {
    if call_reason == DLL_PROCESS_ATTACH {
        // Don't care about thread attach/detach events
        let _ = DisableThreadLibraryCalls(module);

        // Spin up the worker off the loader lock
        let _ = CreateThread(None, 0, Some(worker_thread), None, THREAD_CREATION_FLAGS(0), None);
    }
    TRUE
}

unsafe extern "system" fn worker_thread(_param: *mut c_void) -> u32 {
    // Short pause so the loader finishes up before we call into user32
    Sleep(50);

    let pid = GetCurrentProcessId();
    let _ = EnumWindows(Some(strip_callback), LPARAM(pid as isize));

    0
}

unsafe extern "system" fn strip_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let target_pid = lparam.0 as u32;

    let mut owner_pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut owner_pid));

    if owner_pid == target_pid {
        // WDA_NONE clears the capture-protection flag
        let _ = SetWindowDisplayAffinity(hwnd, WDA_NONE);
    }

    TRUE // keep enumerating
}
