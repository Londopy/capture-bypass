// Embeds the stress tester icon into stress_tester.exe.
// Uses its own icon.ico (red locked padlock) to distinguish it
// from the main GUI (blue unlocked padlock).

fn main() {
    if std::env::var("CARGO_CFG_WINDOWS").is_ok() {
        let mut res = winres::WindowsResource::new();
        res.set_icon("icon.ico");
        res.compile().expect("winres icon compilation failed");
    }
}
