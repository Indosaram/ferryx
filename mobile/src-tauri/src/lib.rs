/// Mobile is a remote companion. It never starts the desktop PTY daemon,
/// binds a server, or exposes desktop filesystem/terminal commands to web content.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("failed to run Ferryx mobile");
}
