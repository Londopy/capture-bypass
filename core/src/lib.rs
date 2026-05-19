// injector_core -- shared DLL injection logic used by both the GUI and CLI.
//
// Basic flow:
//   OpenProcess → VirtualAllocEx → WriteProcessMemory
//   → CreateRemoteThread(LoadLibraryA) → WaitForSingleObject → cleanup
//
// Stealth mode (inject_dll_stealth / inject_checked):
//   Before injecting, the DLL gets copied to %TEMP%\<random_hex>.tmp so the
//   module name visible in CreateToolhelp32Snapshot or EnumProcessModules is
//   just a random string instead of "payload_dll.dll". After LoadLibrary
//   returns, the temp file is deleted -- the DLL stays mapped in the target
//   via Windows' internal file-object reference, so it keeps running fine.

use std::{ffi::CString, path::Path};

// Typed error so callers can match on what actually went wrong
#[derive(Debug)]
pub enum InjectError {
    // DLL file doesn't exist at the given path
    DllNotFound(std::path::PathBuf),
    // Path isn't valid UTF-8
    BadPath,
    // Path has a null byte in it
    NullByte,
    // OpenProcess failed -- usually means we need admin or the PID is wrong
    OpenProcess(windows::core::Error),
    // VirtualAllocEx returned null
    Alloc,
    // WriteProcessMemory failed
    WriteMemory(windows::core::Error),
    // CreateRemoteThread failed
    RemoteThread(windows::core::Error),
    // Copying to temp path failed
    StealthCopy(std::io::Error),
    // The target process has mitigation policies that block DLL injection.
    // Nothing user-mode can do about this one.
    MitigationPolicy(String),
    // Anything else Windows threw at us
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
            InjectError::MitigationPolicy(reason) =>
                write!(f, "Process has Windows mitigation policies that block DLL injection \
                           (no user-mode workaround): {reason}"),
            InjectError::Other(e) =>
                write!(f, "{}", e.message()),
        }
    }
}

impl std::error::Error for InjectError {}

pub type InjectResult = std::result::Result<(), InjectError>;

// inject_checked is what the GUI uses -- returns a typed InjectError so
// we can match on MitigationPolicy separately from other failures.
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

// Copy the DLL to %TEMP%\<random_hex>.tmp before injecting
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
                CreateRemoteThread, GetProcessMitigationPolicy, OpenProcess,
                WaitForSingleObject,
                ProcessDynamicCodePolicy, ProcessExtensionPointDisablePolicy,
                ProcessSignaturePolicy,
                PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION,
                PROCESS_VM_WRITE,
                PROCESS_MITIGATION_BINARY_SIGNATURE_POLICY,
                PROCESS_MITIGATION_DYNAMIC_CODE_POLICY,
                PROCESS_MITIGATION_EXTENSION_POINT_DISABLE_POLICY,
            },
        },
    },
};

// inject_dll_stealth is what the CLI uses -- wraps inject_dll with the temp-copy
// stealth trick. Returns a windows::core::Error on failure.
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

// inject_dll is the basic version -- no stealth copy, no typed errors.
// Prefer inject_checked when you want to match on the failure reason.
pub fn inject_dll(pid: u32, dll_path: &Path) -> Result<()> {
    inject_dll_inner(pid, dll_path).map_err(|e| {
        Error::new(windows::Win32::Foundation::E_FAIL, e.to_string())
    })
}

// Check if the target process has mitigation policies that will block injection.
// Returns Some(reason) if anything would stop us, None if we're probably fine.
// We check three policies: signature requirements, extension point disable, and
// dynamic code prohibition. Any one of them blocks CreateRemoteThread+LoadLibrary.
fn check_mitigation_policies(process: HANDLE) -> Option<String> {
    let mut reasons: Vec<&'static str> = Vec::new();

    unsafe {
        // Only MS-signed DLLs allowed -- our unsigned DLL gets rejected by the loader
        let mut sig =
            std::mem::zeroed::<PROCESS_MITIGATION_BINARY_SIGNATURE_POLICY>();
        if GetProcessMitigationPolicy(
            process,
            ProcessSignaturePolicy,
            std::ptr::addr_of_mut!(sig).cast(),
            std::mem::size_of::<PROCESS_MITIGATION_BINARY_SIGNATURE_POLICY>(),
        ).is_ok() && (sig._bitfield & 0x1) != 0 {
            reasons.push("requires Microsoft-signed DLLs (ProcessSignaturePolicy)");
        }

        // Blocks the LoadLibrary injection path at the AppInit/shim layer
        let mut ext =
            std::mem::zeroed::<PROCESS_MITIGATION_EXTENSION_POINT_DISABLE_POLICY>();
        if GetProcessMitigationPolicy(
            process,
            ProcessExtensionPointDisablePolicy,
            std::ptr::addr_of_mut!(ext).cast(),
            std::mem::size_of::<PROCESS_MITIGATION_EXTENSION_POINT_DISABLE_POLICY>(),
        ).is_ok() && (ext._bitfield & 0x1) != 0 {
            reasons.push("blocks extension-point DLL loading \
                          (ProcessExtensionPointDisablePolicy)");
        }

        // No new executable memory allowed -- CreateRemoteThread won't work
        let mut dyn_code =
            std::mem::zeroed::<PROCESS_MITIGATION_DYNAMIC_CODE_POLICY>();
        if GetProcessMitigationPolicy(
            process,
            ProcessDynamicCodePolicy,
            std::ptr::addr_of_mut!(dyn_code).cast(),
            std::mem::size_of::<PROCESS_MITIGATION_DYNAMIC_CODE_POLICY>(),
        ).is_ok() && (dyn_code._bitfield & 0x1) != 0 {
            reasons.push("prohibits dynamic code execution \
                          (ProcessDynamicCodePolicy)");
        }
    }

    if reasons.is_empty() {
        None
    } else {
        Some(reasons.join("; "))
    }
}

// The actual injection -- opens the process, writes the DLL path into it,
// and starts a remote thread pointing at LoadLibraryA.
fn inject_dll_inner(pid: u32, dll_path: &Path) -> InjectResult {
    // LoadLibraryA takes an ANSI string, so we need a null-terminated one
    let path_str = dll_path
        .to_str()
        .ok_or(InjectError::BadPath)?;

    let path_cstr = CString::new(path_str)
        .map_err(|_| InjectError::NullByte)?;

    let path_bytes = path_cstr.as_bytes_with_nul();

    unsafe {
        // 1. Open the target process
        let process: HANDLE = OpenProcess(
            PROCESS_CREATE_THREAD
                | PROCESS_VM_OPERATION
                | PROCESS_VM_WRITE
                | PROCESS_QUERY_INFORMATION,
            false,
            pid,
        )
        .map_err(InjectError::OpenProcess)?;

        // 1a. Check mitigation policies before doing anything else --
        // if the OS will reject our DLL anyway, bail out with a clear message
        if let Some(reason) = check_mitigation_policies(process) {
            let _ = CloseHandle(process);
            return Err(InjectError::MitigationPolicy(reason));
        }

        // 2. Allocate memory in the target for the DLL path string
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

        // 3. Write the DLL path into the target
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

        // 4. Resolve LoadLibraryA -- same virtual address in every process on the same OS
        let kernel32 = GetModuleHandleA(s!("kernel32.dll"))
            .map_err(InjectError::Other)?;
        let load_library_raw = GetProcAddress(kernel32, s!("LoadLibraryA"))
            .ok_or_else(|| InjectError::Other(Error::new(
                windows::Win32::Foundation::E_FAIL,
                "GetProcAddress(LoadLibraryA) returned null",
            )))?;

        // LoadLibraryA and LPTHREAD_START_ROUTINE are ABI-compatible on x64 Windows
        let thread_start: windows::Win32::System::Threading::LPTHREAD_START_ROUTINE =
            Some(std::mem::transmute(load_library_raw as *const ()));

        // 5. Start the remote thread -- argument is the pointer to our DLL path string
        let thread = CreateRemoteThread(
            process,
            None,
            0,
            thread_start,
            Some(remote_buf),
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

        // 6. Wait for DllMain to return (up to 5 seconds)
        WaitForSingleObject(thread, 5_000);

        // 7. Clean up
        let _ = CloseHandle(thread);
        let _ = VirtualFreeEx(process, remote_buf, 0, MEM_RELEASE);
        let _ = CloseHandle(process);

        Ok(())
    }
}
