// capture_bypass CLI
//
// Usage: cli.exe <pid> <dll_path>
//
// Exits 0 on success, 1 on any failure.
// Prints "OK pid=<pid>" on success, "ERROR <reason>" on failure.
//
// Uses inject_dll_stealth so the DLL gets copied to a random temp name
// before loading -- the module list won't show "payload_dll.dll".

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
