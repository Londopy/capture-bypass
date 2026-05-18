//! capture_bypass CLI
//!
//! Usage:
//!   cli.exe <pid> <dll_path>
//!
//! Exit codes:
//!   0  — injection succeeded
//!   1  — bad arguments or injection failed
//!
//! On success, prints one line to stdout:  OK pid=<pid>
//! On failure, prints one line to stderr:  ERROR <message>
//!
//! Injection uses `inject_dll_stealth`: the DLL is first copied to a randomly
//! named temp file so the module name visible to CreateToolhelp32Snapshot is
//! an opaque string rather than the original DLL filename.

use std::{path::PathBuf, process};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() != 3 {
        eprintln!("ERROR usage: cli.exe <pid> <dll_path>");
        process::exit(1);
    }

    let pid: u32 = match args[1].parse() {
        Ok(p) => p,
        Err(_) => {
            eprintln!("ERROR invalid PID '{}' — must be a positive integer", args[1]);
            process::exit(1);
        }
    };

    let dll_path = PathBuf::from(&args[2]);

    if !dll_path.exists() {
        eprintln!("ERROR DLL not found: {}", dll_path.display());
        process::exit(1);
    }

    match injector_core::inject_dll_stealth(pid, &dll_path) {
        Ok(()) => {
            println!("OK pid={pid}");
            process::exit(0);
        }
        Err(e) => {
            eprintln!("ERROR {}", e.message());
            process::exit(1);
        }
    }
}
