//! The engine executable on macOS (and on Windows without the sandbox feature).
#![cfg_attr(
    all(not(debug_assertions), not(feature = "sandbox"), target_os = "windows"),
    windows_subsystem = "windows"
)]

pub mod engine;

#[cfg(not(all(feature = "sandbox", target_os = "windows")))]
fn main() -> Result<(), &'static str> {
    let _library = engine::load_cef();
    let args = cef::args::Args::new();
    let Some(cmd_line) = args.as_cmd_line() else {
        return Err("Failed to parse command line arguments");
    };
    engine::run(args.as_main_args(), &cmd_line, std::ptr::null_mut());
    Ok(())
}

#[cfg(all(feature = "sandbox", target_os = "windows"))]
fn main() -> Result<(), &'static str> {
    Err("With the sandbox, Windows runs the engine through CEF's bootstrap.exe.")
}
