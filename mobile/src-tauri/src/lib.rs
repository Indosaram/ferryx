/// Mobile is a remote companion. It never starts the desktop PTY daemon,
/// binds a server, or exposes desktop filesystem/terminal commands to web content.
/// The only native capability is opening https/mailto links in the system handler
/// (privacy policy, support mail), scoped in capabilities/mobile.json.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .run(tauri::generate_context!())
        .expect("failed to run Ferryx mobile");
}
