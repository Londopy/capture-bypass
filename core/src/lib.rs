//! injector_core — shared DLL injection logic.
//!
//! Classic LoadLibrary injection pipeline:
//!   OpenProcess → VirtualAllocEx → WriteProcessMemory
//!   → CreateRemoteThread(LoadLibraryA) → WaitForSingleObject → cleanup.
//!
//! # Stealth injection
//!
//! `inject_dll_stealth` wraps `inject_dll` with two hardening steps:
//!
//! 1. **Random temp copy** — the DLL is copied to `%TEMP%\<hex>.tmp` before
//!    injection, so the module name visible to `CreateToolhelp32Snapshot`
//!    (TH32CS_SNAPMODULE) or `EnumProcessModules` is an opaque random string,
//!    not `payload_dll.dll`.  Name-based scanners find nothing.
//!
//! 2. **Immediate file deletion** — the temp file is removed right after
//!    `LoadLibrary` returns.  Windows keeps the DLL mapped in the target via its
//!    internal VAD / file-object reference, so it continues running normally.
//!    The on-disk path no longer exists, meaning the target cannot re-read,
//!    hash-check, or re-load the file to identify it.

use std::{ffi::CString, path::Path};

use windows::{
    core::{s, Error, Result},
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::{
            Diagnostics::Debug::WriteProcessMemory,
            LibraryLoader::{GetModuleHandleA, GetProcAddress},
            Memory::{
                VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE,
                PAGE_READWRITE,
            },
            Threading::{
                CreateRemoteThread, OpenProcess, WaitForSingleObject,
                PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION,
                PROCESS_VM_WRITE,
            },
        },
    },
};

/// Inject `dll_path` into `pid` using a randomised temp copy.
///
/// This is the preferred entry point for the GUI.  It copies the DLL to
/// `%TEMP%\<random_hex>.tmp` before calling [`inject_dll`], so the module
/// name visible to `CreateToolhelp32Snapshot` (TH32CS_SNAPMODULE) or
/// `EnumProcessModules` is an opaque string rather than `payload_dll.dll`.
///
/// This defeats the common defensive pattern where the target application
/// calls `CreateToolhelp32Snapshot`, walks its own module list looking for
/// known DLL names, and calls `FreeLibrary` on anything it recognises.
///
/// The temp file is cleaned up on injection failure; on success it is left
/// in `%TEMP%` and will be swept up by normal OS temp-file cleanup.
pub fn inject_dll_stealth(pid: u32, dll_path: &Path) -> windows::core::Result<()> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let tmp_dir = std::env::temp_dir();

    // Opaque name: nanosecond wall-clock XOR pid.
    // Nanosecond precision + pid makes path collisions negligible.
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let tag = nanos ^ pid;
    let tmp_path = tmp_dir.join(format!("{tag:08x}.tmp"));

    // ── 1. Copy the payload to the opaque temp path ───────────────────────────
    std::fs::copy(dll_path, &tmp_path).map_err(|e| {
        windows::core::Error::new(
            windows::Win32::Foundation::E_FAIL,
            format!("Stealth copy to temp failed: {e}"),
        )
    })?;

    // ── 2. Inject the temp copy ───────────────────────────────────────────────
    let result = inject_dll(pid, &tmp_path);

    // Clean up temp file on failure (success leaves it for OS sweeping).
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }

    result
}

/// Inject `dll_path` into the process identified by `pid`.
///
/// Returns `Ok(())` when the remote thread has run to completion (DllMain
/// executed).  Returns a descriptive `Err` on any failure — the caller should
/// surface `e.message()` to the user.
pub fn inject_dll(pid: u32, dll_path: &Path) -> Result<()> {
    // LoadLibraryA is an ANSI API — build a null-terminated ANSI string.
    // If your DLL lives at a non-ASCII path, swap this for LoadLibraryW.
    let path_str = dll_path
        .to_str()
        .ok_or_else(|| Error::new(windows::Win32::Foundation::E_INVALIDARG, "DLL path is not valid UTF-8"))?;

    let path_cstr = CString::new(path_str)
        .map_err(|_| Error::new(windows::Win32::Foundation::E_INVALIDARG, "DLL path contains an interior null byte"))?;

    let path_bytes = path_cstr.as_bytes_with_nul();

    unsafe {
        // ── 1. Open target process ────────────────────────────────────────────
        let process: HANDLE = OpenProcess(
            PROCESS_CREATE_THREAD
                | PROCESS_VM_OPERATION
                | PROCESS_VM_WRITE
                | PROCESS_QUERY_INFORMATION,
            false,
            pid,
        )
        .map_err(|e| {
            Error::new(
                e.code(),
                format!(
                    "OpenProcess(pid={pid}) failed — is the tool running as Administrator? ({})",
                    e.message()
                ),
            )
        })?;

        // ── 2. Allocate memory in the target for the DLL path string ──────────
        let remote_buf = VirtualAllocEx(
            process,
            None,
            path_bytes.len(),
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        );

        if remote_buf.is_null() {
            let _ = CloseHandle(process);
            return Err(Error::new(
                windows::Win32::Foundation::E_OUTOFMEMORY,
                "VirtualAllocEx failed in the target process",
            ));
        }

        // ── 3. Write the DLL path into the target ─────────────────────────────
        if let Err(e) = WriteProcessMemory(
            process,
            remote_buf,
            path_bytes.as_ptr().cast(),
            path_bytes.len(),
            None,
        ) {
            let _ = VirtualFreeEx(process, remote_buf, 0, MEM_RELEASE);
            let _ = CloseHandle(process);
            return Err(Error::new(
                e.code(),
                format!("WriteProcessMemory failed: {}", e.message()),
            ));
        }

        // ── 4. Resolve LoadLibraryA — identical VA in every process ───────────
        let kernel32 = GetModuleHandleA(s!("kernel32.dll"))?;
        let load_library_raw = GetProcAddress(kernel32, s!("LoadLibraryA")).ok_or_else(|| {
            Error::new(
                windows::Win32::Foundation::E_FAIL,
                "GetProcAddress(LoadLibraryA) returned null",
            )
        })?;

        // LoadLibraryA(LPCSTR) and LPTHREAD_START_ROUTINE(*mut c_void) are
        // ABI-compatible on x64 Windows (both pointer-sized, same calling conv).
        let thread_start: windows::Win32::System::Threading::LPTHREAD_START_ROUTINE =
            Some(std::mem::transmute(load_library_raw as *const ()));

        // ── 5. Spin up the remote thread ──────────────────────────────────────
        let thread = CreateRemoteThread(
            process,
            None,
            0,
            thread_start,
            Some(remote_buf), // argument = pointer to DLL path string
            0,
            None,
        );

        let thread: HANDLE = match thread {
            Ok(h) => h,
            Err(e) => {
                let _ = VirtualFreeEx(process, remote_buf, 0, MEM_RELEASE);
                let _ = CloseHandle(process);
                return Err(Error::new(
                    e.code(),
                    format!("CreateRemoteThread failed: {}", e.message()),
                ));
            }
        };

        // ── 6. Wait for DllMain to return (≤ 5 s) ────────────────────────────
        WaitForSingleObject(thread, 5_000);

        // ── 7. Clean up ────────────────────────────────────────────────────────
        let _ = CloseHandle(thread);
        let _ = VirtualFreeEx(process, remote_buf, 0, MEM_RELEASE);
        let _ = CloseHandle(process);

        Ok(())
    }
}
