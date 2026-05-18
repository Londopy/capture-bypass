//! Embed a Windows application manifest that:
//!   • Requests administrator elevation (UAC) — required to OpenProcess on
//!     other users' processes.
//!   • Declares Vista+ common controls (for proper DPI / theming).
//!
//! Release version injection
//! ─────────────────────────
//! Set the APP_VERSION environment variable before `cargo build` to override
//! the CARGO_PKG_VERSION baked into the exe.  This lets the Inno Setup build
//! step pass the release tag without touching Cargo.toml each time:
//!
//!   set APP_VERSION=3.4.12 && cargo build --release -p gui
//!   iscc /DMyAppVersion=3.4.12 installer\capture-bypass.iss
//!
//! If APP_VERSION is not set, the value in [package].version (Cargo.toml)
//! is used as normal.

fn main() {
    // Propagate APP_VERSION → CARGO_PKG_VERSION so that env!("CARGO_PKG_VERSION")
    // in main.rs returns the injected version rather than the Cargo.toml default.
    if let Ok(v) = std::env::var("APP_VERSION") {
        if !v.trim().is_empty() {
            println!("cargo:rustc-env=CARGO_PKG_VERSION={}", v.trim());
        }
    }
    // Re-run build.rs if APP_VERSION changes between runs.
    println!("cargo:rerun-if-env-changed=APP_VERSION");

    if std::env::var("CARGO_CFG_WINDOWS").is_ok() {
        let mut res = winres::WindowsResource::new();
        res.set_icon("icon.ico");
        res.set_manifest(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity
      version="1.0.0.0"
      processorArchitecture="amd64"
      name="capture_bypass"
      type="win32"/>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="requireAdministrator" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
          type="win32"
          name="Microsoft.Windows.Common-Controls"
          version="6.0.0.0"
          processorArchitecture="*"
          publicKeyToken="6595b64144ccf1df"
          language="*"/>
    </dependentAssembly>
  </dependency>
</assembly>"#,
        );
        res.compile().expect("winres manifest compilation failed");
    }
}
