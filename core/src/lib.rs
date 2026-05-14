//! injector_core — shared DLL injection logic.
//!
//! Classic LoadLibrary injection pipeline:
//!   OpenProcess → VirtualAllocEx → WriteProcessMemory
//!   → CreateRemoteThread(LoadLibraryA) → WaitForSingleObject → cleanup.

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
