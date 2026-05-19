// Embeds the app icon into stress_tester.exe.
// Reuses gui/icon.ico so both binaries share the same file.

fn main() {
    if std::env::var("CARGO_CFG_WINDOWS").is_ok() {
        let mut res = winres::WindowsResource::new();
        res.set_icon("../gui/icon.ico");
        res.compile().expect("winres icon compilation failed");
    }
}
