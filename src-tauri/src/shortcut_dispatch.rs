use tauri::Emitter;

/// Returns whether the native shortcut was delivered and may be consumed. The shortcut goes to
/// the focused app window only, so a second window never acts on a chord typed in another.
pub(crate) fn dispatch<R: tauri::Runtime, S: serde::Serialize + Clone>(
    app: &tauri::AppHandle<R>,
    event: &str,
    payload: S,
) -> bool {
    let Some(window) = crate::window_registry::focused_window(app) else {
        tracing::warn!(event, "Shortcut target window is unavailable");
        return false;
    };
    match app.emit_to(window.label(), event, payload) {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(event, ?error, "Shortcut dispatch failed");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::{Listener, Manager};

    #[test]
    fn shortcuts_reach_listeners_with_browser_children() {
        // Given a real Tauri manager containing the app and browser webviews.
        let app = tauri::test::mock_app();
        let main = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
            .build()
            .expect("main webview");
        let window = main.as_ref().window();
        let child = window
            .add_child(
                tauri::WebviewBuilder::new("browser-test", tauri::WebviewUrl::default()),
                tauri::LogicalPosition::new(0.0, 0.0),
                tauri::LogicalSize::new(400.0, 300.0),
            )
            .expect("browser child");
        assert!(app.get_webview_window("main").is_none());

        for hidden in [false, true] {
            if hidden {
                child.hide().expect("hide browser");
            }
            for event in [
                "menu_select_worktree",
                "menu_select_tab",
                "menu_next_tab",
                "menu_prev_tab",
                "menu_new_terminal_tab",
                "menu_close_tab",
                "menu_split_right",
                "menu_split_down",
                "menu_command_palette",
                "menu_toggle_sidebar",
                "menu_open_settings",
            ] {
                let (sender, receiver) = std::sync::mpsc::channel();
                // Shortcuts are emitted to the target window, so listen where the UI listens.
                let listener = main.listen(event, move |received| {
                    sender
                        .send(received.payload().to_owned())
                        .expect("receive shortcut");
                });
                // When dispatching through the production native shortcut path.
                let consumed = dispatch(app.handle(), event, 3_u8);
                // Then delivery succeeds even if the browser is retained but hidden.
                assert!(consumed, "{event}, hidden={hidden}");
                assert_eq!(
                    receiver
                        .recv_timeout(std::time::Duration::from_secs(1))
                        .expect("event"),
                    "3"
                );
                assert!(receiver.try_recv().is_err(), "duplicate event");
                main.unlisten(listener);
            }
        }
    }

    #[test]
    fn missing_window_does_not_consume_shortcut() {
        let app = tauri::test::mock_app();
        assert!(!dispatch(app.handle(), "menu_next_tab", ()));
    }

    #[test]
    fn shortcuts_reach_only_the_focused_window() {
        let app = tauri::test::mock_app();
        app.manage(crate::window_registry::WindowRegistry::default());
        let main = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
            .build()
            .expect("main window");
        let secondary =
            tauri::WebviewWindowBuilder::new(&app, "ws-test", tauri::WebviewUrl::default())
                .build()
                .expect("secondary window");
        let record = |window: &tauri::WebviewWindow<tauri::test::MockRuntime>| {
            let (sender, receiver) = std::sync::mpsc::channel();
            window.listen("menu_close_tab", move |event| {
                let _ = sender.send(event.payload().to_owned());
            });
            receiver
        };
        let main_events = record(&main);
        let secondary_events = record(&secondary);

        app.state::<crate::window_registry::WindowRegistry>()
            .note_focused("ws-test");
        assert!(dispatch(app.handle(), "menu_close_tab", ()));
        secondary_events
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("focused window receives the shortcut");
        assert!(main_events.try_recv().is_err(), "unfocused window must not act");
    }
}
