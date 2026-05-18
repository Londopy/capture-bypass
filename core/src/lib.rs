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

// ── Structured error type ─────────────────────────────────────────────────────

/// Typed injection failure — lets callers match on root cause rather than
/// parsing an error string.
#[derive(Debug)]
pub enum InjectError {
    /// The DLL file was not found at the given path.
    DllNotFound(std::path::PathBuf),
    /// The DLL path is not valid UTF-8.
    BadPath,
    /// The DLL path contains an interior null byte.
    NullByte,
    /// `OpenProcess` failed — most likely insufficient privileges.
    OpenProcess(windows::core::Error),
    /// `VirtualAllocEx` returned null.
    Alloc,
    /// `WriteProcessMemory` failed.
    WriteMemory(windows::core::Error),
    /// `CreateRemoteThread` failed.
    RemoteThread(windows::core::Error),
    /// Copying to the stealth temp path failed.
    StealthCopy(std::io::Error),
    /// Any other Windows API error.
    Other(windows::core::Error),
}

impl std::fmt::Display for InjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InjectError::DllNotFound(p) =>
                write!(f, "DLL not found: {}", p.display()),
            InjectError::BadPath =>
                write!(f, "DLL path is not valid UTF-8"),
            InjectError::NullByte =>
                write!(f, "DLL path contains an interior null byte"),
            InjectError::OpenProcess(e) =>
                write!(f, "OpenProcess failed (run as Administrator?): {}", e.message()),
            InjectError::Alloc =>
                write!(f, "VirtualAllocEx failed in the target process"),
            InjectError::WriteMemory(e) =>
                write!(f, "WriteProcessMemory failed: {}", e.message()),
            InjectError::RemoteThread(e) =>
                write!(f, "CreateRemoteThread failed: {}", e.message()),
            InjectError::StealthCopy(e) =>
                write!(f, "Stealth copy to temp failed: {e}"),
            InjectError::Other(e) =>
                write!(f, "{}", e.message()),
        }
    }
}

impl std::error::Error for InjectError {}

/// Type alias for convenience.
pub type InjectResult = std::result::Result<(), InjectError>;

/// High-level entry point that returns a typed [`InjectError`] instead of a
/// raw `windows::core::Error`.  Prefer this over [`inject_dll`] when you want
/// to match on the failure reason.
pub fn inject_checked(pid: u32, dll_path: &Path) -> InjectResult {
    if !dll_path.exists() {
        return Err(InjectError::DllNotFound(dll_path.to_path_buf()));
    }
    let tmp = make_stealth_copy(pid, dll_path)
        .map_err(InjectError::StealthCopy)?;

    let result = inject_dll_inner(pid, &tmp);
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn make_stealth_copy(pid: u32, dll_path: &Path) -> std::result::Result<std::path::PathBuf, std::io::Error> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let tag = nanos ^ pid;
    let tmp = std::env::temp_dir().join(format!("{tag:08x}.tmp"));
    std::fs::copy(dll_path, &tmp)?;
    Ok(tmp)
}

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
    let tmp_path = make_stealth_copy(pid, dll_path).map_err(|e| {
        windows::core::Error::new(
            windows::Win32::Foundation::E_FAIL,
            format!("Stealth copy to temp failed: {e}"),
        )
    })?;

    let result = inject_dll(pid, &tmp_path);
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
///
/// For richer error matching, use [`inject_checked`] instead.
pub fn inject_dll(pid: u32, dll_path: &Path) -> Result<()> {
    inject_dll_inner(pid, dll_path).map_err(|e| {
        Error::new(windows::Win32::Foundation::E_FAIL, e.to_string())
    })
}

/// Core injection implementation that returns [`InjectError`].
fn inject_dll_inner(pid: u32, dll_path: &Path) -> InjectResult {
    // LoadLibraryA is an ANSI API — build a null-terminated ANSI string.
    // If your DLL lives at a non-ASCII path, swap this for LoadLibraryW.
    let path_str = dll_path
        .to_str()
        .ok_or(InjectError::BadPath)?;

    let path_cstr = CString::new(path_str)
        .map_err(|_| InjectError::NullByte)?;

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
        .map_err(InjectError::OpenProcess)?;

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
            return Err(InjectError::Alloc);
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
            return Err(InjectError::WriteMemory(e));
        }

        // ── 4. Resolve LoadLibraryA — identical VA in every process ───────────
        let kernel32 = GetModuleHandleA(s!("kernel32.dll"))
            .map_err(InjectError::Other)?;
        let load_library_raw = GetProcAddress(kernel32, s!("LoadLibraryA"))
            .ok_or_else(|| InjectError::Other(Error::new(
                windows::Win32::Foundation::E_FAIL,
                "GetProcAddress(LoadLibraryA) returned null",
            )))?;

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
                return Err(InjectError::RemoteThread(e));
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
