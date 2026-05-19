// build.rs -- embeds the UAC manifest and app icon, and handles version injection.
//
// Set APP_VERSION before cargo build to override the version string without
// touching Cargo.toml. The CI workflow does this from the git tag:
//
//   set APP_VERSION=3.4.12 && cargo build --release -p gui
//
// If APP_VERSION isn't set, Cargo.toml's version is used as normal.

fn main() {
    // Pass APP_VERSION through to env!("CARGO_PKG_VERSION") in main.rs
    if let Ok(v) = std::env::var("APP_VERSION") {
        if !v.trim().is_empty() {
            println!("cargo:rustc-env=CARGO_PKG_VERSION={}", v.trim());
        }
    }
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
