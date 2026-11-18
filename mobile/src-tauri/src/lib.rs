/// Mobile is a remote companion. It never starts the desktop PTY daemon,
/// binds a server, or exposes desktop filesystem/terminal commands to web content.
/// The native capabilities are opening https/mailto links in the system handler (privacy policy,
/// support mail, scoped in capabilities/mobile.json), `share_text_file`, the iOS share sheet
/// the supervisor transcript export uses (there is no browser download handler in this shell),
/// and APNs registration, because an iOS WKWebView has no `PushManager`.
mod apns;
mod share;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            share::share_text_file,
            apns::request_push_permission
        ])
        .setup(|_app| {
            // Register with APNs at launch so the token exists before the user opens Settings:
            // the delegate callbacks are what mint it, and a token that only appears after a
            // gesture leaves the first alerts after enabling silently undeliverable. iOS shows
            // the permission prompt on its own schedule; registration itself needs no gesture.
            #[cfg(target_os = "ios")]
            apns::register_at_launch(_app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run Ferryx mobile");
}
