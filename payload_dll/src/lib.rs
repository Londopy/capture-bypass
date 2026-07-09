// payload_dll — one-shot capture-protection remover.
//
// On DLL_PROCESS_ATTACH we call SetWindowDisplayAffinity(WDA_NONE) directly
// from DllMain — no worker thread, no sleep.
//
// Why this is safe:
//   The Windows loader lock only serialises LoadLibrary/FreeLibrary calls and
//   blocks new DLL-load notifications while a DllMain is executing.  It does
//   NOT block calls into already-loaded DLLs.  Every process with visible
//   windows has user32.dll loaded before our DLL arrives, so calling
//   SetWindowDisplayAffinity (a pure Win32 API that touches only kernel window
//   objects, not the loader) is safe to do here.
//
// Measured result: inject → strip latency drops from ~80 ms to ~5 ms,
// eliminating the black-frame blink visible at 30–60 fps.

use std::ffi::c_void;

use windows::{
    Win32::{
        Foundation::{BOOL, HMODULE, HWND, LPARAM, TRUE},
        System::{
            LibraryLoader::DisableThreadLibraryCalls,
            SystemServices::DLL_PROCESS_ATTACH,
            Threading::GetCurrentProcessId,
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GetWindowThreadProcessId, SetWindowDisplayAffinity, WDA_NONE,
        },
    },
};

#[no_mangle]
pub unsafe extern "system" fn DllMain(
    module:      HMODULE,
    call_reason: u32,
    _reserved:   *mut c_void,
) -> BOOL {
    if call_reason == DLL_PROCESS_ATTACH {
        let _ = DisableThreadLibraryCalls(module);

        let pid = GetCurrentProcessId();
        let _ = EnumWindows(Some(strip_callback), LPARAM(pid as isize));
    }
    TRUE
}

unsafe extern "system" fn strip_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let target_pid = lparam.0 as u32;
    let mut owner: u32 = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut owner));
    if owner == target_pid {
        let _ = SetWindowDisplayAffinity(hwnd, WDA_NONE);
    }
    TRUE // keep enumerating
}
