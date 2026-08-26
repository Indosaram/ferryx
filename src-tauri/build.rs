#[path = "native_terminal/build_ghostty.rs"]
mod build_ghostty;

fn main() {
    if std::env::var_os("CARGO_FEATURE_NATIVE_TERMINAL").is_some() {
        if let Err(err) = build_ghostty::build_ghostty_vt() {
            eprintln!("\n[ghostty build error] {err}\n");
            std::process::exit(1);
        }
    }
    tauri_build::build();
}
