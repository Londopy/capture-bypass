// payload_dll_persistent — dual-layer protection removal
//
// Layer 1 — IAT hook (instantaneous):
//   On load, the DLL walks the Import Address Table of every module already
//   loaded in the host process and replaces the SetWindowDisplayAffinity
//   function pointer with our detour.  Every subsequent call that goes through
//   a static import is silently redirected to WDA_NONE with zero latency.
//
// Layer 2 — Polling fallback (every 5 s):
//   A background thread still calls SetWindowDisplayAffinity(WDA_NONE) on all
//   owned windows.  This catches:
//     • windows that were already protected before injection
//     • callers that resolve the function dynamically via GetProcAddress
//       (bypassing the IAT entirely)
//   Five seconds is long enough to stay invisible CPU-wise; the IAT hook
//   handles the fast path for everything else.

#![allow(clippy::missing_safety_doc)]

use std::{
    ffi::c_void,
    sync::atomic::{AtomicPtr, Ordering},
};

use windows::Win32::{
    Foundation::{BOOL, CloseHandle, HMODULE, HWND, LPARAM, TRUE},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Module32FirstW, Module32NextW,
            MODULEENTRY32W, TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32,
        },
        LibraryLoader::{DisableThreadLibraryCalls, GetModuleHandleA, GetProcAddress},
        Memory::{VirtualProtect, PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS},
        SystemServices::DLL_PROCESS_ATTACH,
        Threading::{CreateThread, GetCurrentProcessId, Sleep, THREAD_CREATION_FLAGS},
    },
    UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, SetWindowDisplayAffinity, WDA_NONE,
    },
};

// ── Minimal PE structure definitions ──────────────────────────────────────
//
// Defined manually so we don't need to pull in extra windows-rs feature flags
// and sidestep union-accessor complexity for import table parsing.

/// Represents IMAGE_IMPORT_DESCRIPTOR (only the fields we actually read).
#[repr(C)]
struct ImportDescriptor {
    /// RVA to the Hint/Name table (OriginalFirstThunk).
    original_first_thunk: u32,
    _time_date_stamp:      u32,
    _forwarder_chain:      u32,
    /// RVA to the null-terminated ASCII DLL name.
    name:                  u32,
    /// RVA to the IAT — this is what the loader patches at load time and what
    /// we patch to redirect calls.
    first_thunk:           u32,
}

/// Represents IMAGE_IMPORT_BY_NAME (variable-length name follows the hint).
#[repr(C)]
struct ImportByName {
    _hint: u16,
    /// First byte of the null-terminated ASCII function name.
    name:  [u8; 1],
}

// ── Original function pointer ──────────────────────────────────────────────

/// Holds the real SetWindowDisplayAffinity address so our detour can call
/// through to the actual Win32 implementation.
static ORIGINAL_SWDA: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());

// ── Detour ─────────────────────────────────────────────────────────────────

/// Replacement for SetWindowDisplayAffinity.
/// Forwards every call to the real function but always passes WDA_NONE,
/// regardless of what the caller requested.
unsafe extern "system" fn hooked_swda(hwnd: HWND, _affinity: u32) -> BOOL {
    let orig = ORIGINAL_SWDA.load(Ordering::Relaxed);
    if orig.is_null() {
        return TRUE; // Shouldn't happen, but fail open
    }
    let real: unsafe extern "system" fn(HWND, u32) -> BOOL = core::mem::transmute(orig);
    real(hwnd, 0 /* WDA_NONE */)
}

// ── DLL entry point ────────────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "system" fn DllMain(
    module:      HMODULE,
    call_reason: u32,
    _reserved:   *mut c_void,
) -> BOOL {
    if call_reason == DLL_PROCESS_ATTACH {
        let _ = DisableThreadLibraryCalls(module);
        // Install the IAT hook synchronously here in DllMain.
        // Safe: we only call GetModuleHandle/GetProcAddress on already-loaded
        // DLLs and VirtualProtect — no LoadLibrary, no locks we don't own.
        install_iat_hook();
        let _ = CreateThread(
            None,
            0,
            Some(worker_thread),
            None,
            THREAD_CREATION_FLAGS(0),
            None,
        );
    }
    TRUE
}

// ── IAT hook installation ──────────────────────────────────────────────────

/// Enumerate every module loaded in the current process and patch the
/// SetWindowDisplayAffinity IAT slot in each one.
unsafe fn install_iat_hook() {
    // Resolve the real function and stash its address for the detour.
    let user32 = match GetModuleHandleA(windows::core::s!("user32.dll")) {
        Ok(h) if !h.is_invalid() => h,
        _ => return,
    };
    let orig = match GetProcAddress(user32, windows::core::s!("SetWindowDisplayAffinity")) {
        Some(f) => f as *mut c_void,
        None    => return,
    };
    ORIGINAL_SWDA.store(orig, Ordering::Relaxed);

    let pid  = GetCurrentProcessId();
    // TH32CS_SNAPMODULE32 includes 32-bit modules when running under WOW64.
    let snap = match CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid) {
        Ok(h)  => h,
        Err(_) => {
            // Fallback: hook at least the host EXE.
            if let Ok(host) = GetModuleHandleA(None) {
                hook_module_iat(host.0 as *mut u8, orig);
            }
            return;
        }
    };

    let mut entry = MODULEENTRY32W {
        dwSize: core::mem::size_of::<MODULEENTRY32W>() as u32,
        ..Default::default()
    };
    if Module32FirstW(snap, &mut entry).is_ok() {
        loop {
            hook_module_iat(entry.modBaseAddr as *mut u8, orig);
            if Module32NextW(snap, &mut entry).is_err() {
                break;
            }
        }
    }
    let _ = CloseHandle(snap);
}

/// Walk `base`'s PE import table and replace the user32.dll
/// SetWindowDisplayAffinity IAT entry with our detour.
///
/// Returns `true` if the entry was found and patched.
unsafe fn hook_module_iat(base: *mut u8, _target_fn: *mut c_void) -> bool {
    if base.is_null() { return false; }

    // ── Validate MZ signature ──────────────────────────────────────────────
    if *(base as *const u16) != 0x5A4D /* "MZ" */ { return false; }

    // ── Locate NT headers ──────────────────────────────────────────────────
    let nt_offset = *(base.add(60) as *const i32) as usize;
    let nt        =  base.add(nt_offset);
    if *(nt as *const u32) != 0x0000_4550 /* "PE\0\0" */ { return false; }

    // ── Determine the import directory RVA ────────────────────────────────
    // Optional header starts at nt + 24 (4-byte sig + 20-byte file header).
    // The import DataDirectory entry (index 1) is located at:
    //   PE32  (magic 0x010B): OptHdr offset 96  + 8 bytes (skip export dir)
    //   PE32+ (magic 0x020B): OptHdr offset 112 + 8 bytes
    let opt_magic = *(nt.add(24) as *const u16);
    let import_rva_off = match opt_magic {
        0x010B => 24 + 96  + 8, // PE32
        0x020B => 24 + 112 + 8, // PE32+
        _      => return false,
    };
    let import_rva = *(nt.add(import_rva_off) as *const u32) as usize;
    if import_rva == 0 { return false; }

    // ── Walk import descriptors ────────────────────────────────────────────
    let mut desc = base.add(import_rva) as *mut ImportDescriptor;
    let mut patched = false;

    'desc: loop {
        if (*desc).name == 0 { break; } // null-terminator entry

        // Check whether this descriptor is for user32.dll (case-insensitive).
        if ascii_icase_eq(base.add((*desc).name as usize), b"user32.dll") {
            let oft_rva = (*desc).original_first_thunk as usize;
            let iat_rva = (*desc).first_thunk         as usize;
            if oft_rva == 0 { break; }

            // Walk hint/name thunks (OFT) and IAT thunks in lockstep.
            // Each thunk entry is pointer-sized (4 bytes on PE32, 8 on PE32+).
            let mut oft = base.add(oft_rva) as *const usize;
            let mut iat = base.add(iat_rva) as *mut   usize;

            loop {
                let thunk = *oft;
                if thunk == 0 { break; }

                // High bit set → imported by ordinal; we only care about named.
                #[cfg(target_pointer_width = "64")]
                let by_ordinal = thunk & (1usize << 63) != 0;
                #[cfg(target_pointer_width = "32")]
                let by_ordinal = thunk & (1usize << 31) != 0;

                if !by_ordinal {
                    // `thunk` is an RVA to IMAGE_IMPORT_BY_NAME.
                    let ibn = base.add(thunk) as *const ImportByName;
                    if ascii_icase_eq(
                        &(*ibn).name as *const u8,
                        b"SetWindowDisplayAffinity",
                    ) {
                        // Make the IAT page writable, swap the pointer, restore.
                        let mut old_prot = PAGE_PROTECTION_FLAGS(0);
                        if VirtualProtect(
                            iat as *const c_void,
                            core::mem::size_of::<usize>(),
                            PAGE_EXECUTE_READWRITE,
                            &mut old_prot,
                        ).is_ok() {
                            *iat = hooked_swda as *const () as usize;
                            let mut _tmp = PAGE_PROTECTION_FLAGS(0);
                            let _ = VirtualProtect(
                                iat as *const c_void,
                                core::mem::size_of::<usize>(),
                                old_prot,
                                &mut _tmp,
                            );
                            patched = true;
                            break 'desc;
                        }
                    }
                }

                oft = oft.add(1);
                iat = iat.add(1);
            }
            break; // Only one user32.dll descriptor per module
        }

        desc = desc.add(1);
    }

    patched
}

/// Byte-by-byte ASCII case-insensitive equality check.
///
/// `actual`   — pointer to a null-terminated string in the target module.
/// `expected` — lowercase byte slice **without** a null terminator.
///
/// Returns `true` iff the strings are equal (ignoring ASCII case).
#[inline]
unsafe fn ascii_icase_eq(actual: *const u8, expected: &[u8]) -> bool {
    for (i, &e) in expected.iter().enumerate() {
        let a = *actual.add(i);
        if a == 0 { return false; }                   // actual is shorter
        if a.to_ascii_lowercase() != e { return false; }
    }
    *actual.add(expected.len()) == 0                  // actual must end here
}

// ── Polling fallback thread ────────────────────────────────────────────────

/// Background thread — strips protection on all owned windows every 5 seconds.
///
/// Acts as a safety net for:
///   • windows that were already protected when the DLL was injected
///   • callers that resolve SetWindowDisplayAffinity via GetProcAddress at
///     runtime (dynamic loading bypasses the IAT patch entirely)
///
/// Five seconds is long enough that the thread is effectively invisible from
/// a CPU perspective; the IAT hook covers the hot path with zero latency.
unsafe extern "system" fn worker_thread(_: *mut c_void) -> u32 {
    // Small delay to let the loader finish before we start calling user32.
    Sleep(100);

    // Immediately strip any windows that were already protected at inject time.
    let pid = GetCurrentProcessId();
    let _ = EnumWindows(Some(strip_callback), LPARAM(pid as isize));

    loop {
        // 500 ms is fast enough to catch dynamic GetProcAddress callers with
        // no perceptible CPU overhead.  The IAT hook handles the zero-latency
        // path for static callers; this loop is just the safety net.
        Sleep(500);
        let pid = GetCurrentProcessId();
        let _ = EnumWindows(Some(strip_callback), LPARAM(pid as isize));
    }
}

unsafe extern "system" fn strip_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let target_pid = lparam.0 as u32;
    let mut owner: u32 = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut owner));
    if owner == target_pid {
        let _ = SetWindowDisplayAffinity(hwnd, WDA_NONE);
    }
    TRUE
}
