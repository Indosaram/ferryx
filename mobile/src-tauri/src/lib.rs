/// Mobile is a remote companion. It never starts the desktop PTY daemon,
/// binds a server, or exposes desktop filesystem/terminal commands to web content.
/// The native capabilities are opening https/mailto links in the system handler (privacy policy,
/// support mail, scoped in capabilities/mobile.json) and `share_text_file`, the iOS share sheet
/// the supervisor transcript export uses (there is no browser download handler in this shell).
mod share;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![share::share_text_file])
        .run(tauri::generate_context!())
        .expect("failed to run Ferryx mobile");
}
