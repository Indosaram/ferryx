use serde::{Deserialize, Serialize};
use tauri::menu::{
    ContextMenu, IconMenuItemBuilder, Menu, MenuItemBuilder, MenuItemKind, NativeIcon,
    PredefinedMenuItem, SubmenuBuilder,
};
use tauri::{AppHandle, Emitter, LogicalPosition, Manager, Position, Runtime};

use crate::ipc::IpcError;

pub const MENU_ACTION_EVENT: &str = "ferryx://menu-action";

#[derive(Debug, Clone, Serialize)]
pub struct NativeMenuActionPayload {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeMenuPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeMenuItemSpec {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub shortcut: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum NativeMenuEntry {
    Item(NativeMenuItemSpec),
    Separator,
    Submenu {
        label: String,
        items: Vec<NativeMenuEntry>,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativePopupMenuRequest {
    pub items: Vec<NativeMenuEntry>,
    pub position: NativeMenuPoint,
}

fn resolve_native_icon(icon: &str) -> Option<NativeIcon> {
    match icon {
        "add" => Some(NativeIcon::Add),
        "folder" => Some(NativeIcon::Folder),
        "reveal" => Some(NativeIcon::RevealFreestanding),
        "refresh" => Some(NativeIcon::Refresh),
        "remove" => Some(NativeIcon::Remove),
        "share" => Some(NativeIcon::Share),
        "trash" => Some(NativeIcon::TrashFull),
        "user" => Some(NativeIcon::User),
        _ => None,
    }
}

fn resolve_agent_icon(agent: &str) -> Option<&'static [u8]> {
    match agent.trim().to_ascii_lowercase().as_str() {
        "claude" => Some(include_bytes!(
            "../../resources/agent-menu-icons/claude.png"
        )),
        "codex" => Some(include_bytes!("../../resources/agent-menu-icons/codex.png")),
        "antigravity" | "agy" | "antigravity-cli" => Some(include_bytes!(
            "../../resources/agent-menu-icons/antigravity.png"
        )),
        "opencode" => Some(include_bytes!(
            "../../resources/agent-menu-icons/opencode.png"
        )),
        "omo" => Some(include_bytes!("../../resources/agent-menu-icons/omo.png")),
        "cursor" | "cursor-agent" => Some(include_bytes!(
            "../../resources/agent-menu-icons/cursor.png"
        )),
        "aider" => Some(include_bytes!("../../resources/agent-menu-icons/aider.png")),
        "crush" => Some(include_bytes!("../../resources/agent-menu-icons/crush.png")),
        "droid" => Some(include_bytes!("../../resources/agent-menu-icons/droid.png")),
        "copilot" => Some(include_bytes!(
            "../../resources/agent-menu-icons/copilot.png"
        )),
        "grok" => Some(include_bytes!("../../resources/agent-menu-icons/grok.png")),
        "kimi" => Some(include_bytes!("../../resources/agent-menu-icons/kimi.png")),
        "cline" => Some(include_bytes!("../../resources/agent-menu-icons/cline.png")),
        "pi" => Some(include_bytes!("../../resources/agent-menu-icons/pi.png")),
        "gjc" => Some(include_bytes!("../../resources/agent-menu-icons/gjc.png")),
        _ => None,
    }
}

fn resolve_custom_icon(icon: &str) -> tauri::Result<Option<tauri::image::Image<'static>>> {
    let agent = icon.strip_prefix("agent:").unwrap_or(icon);
    resolve_agent_icon(agent)
        .map(|bytes| tauri::image::Image::from_bytes(bytes).map(|image| image.to_owned()))
        .transpose()
}

fn build_item_kind<R: Runtime, M: Manager<R>>(
    manager: &M,
    spec: &NativeMenuItemSpec,
) -> tauri::Result<MenuItemKind<R>> {
    let enabled = spec.enabled.unwrap_or(true);
    let icon_name = spec.icon.as_deref().unwrap_or("");
    if let Some(icon) = resolve_native_icon(icon_name) {
        let mut item = IconMenuItemBuilder::with_id(spec.id.clone(), &spec.label)
            .enabled(enabled)
            .native_icon(icon);
        if let Some(shortcut) = &spec.shortcut {
            item = item.accelerator(shortcut.as_str());
        }
        Ok(MenuItemKind::Icon(item.build(manager)?))
    } else if let Some(icon) = resolve_custom_icon(icon_name)? {
        let mut item = IconMenuItemBuilder::with_id(spec.id.clone(), &spec.label)
            .enabled(enabled)
            .icon(icon);
        if let Some(shortcut) = &spec.shortcut {
            item = item.accelerator(shortcut.as_str());
        }
        Ok(MenuItemKind::Icon(item.build(manager)?))
    } else {
        let mut item = MenuItemBuilder::with_id(spec.id.clone(), &spec.label).enabled(enabled);
        if let Some(shortcut) = &spec.shortcut {
            item = item.accelerator(shortcut.as_str());
        }
        Ok(MenuItemKind::MenuItem(item.build(manager)?))
    }
}

fn add_entries<R: Runtime, M: Manager<R>>(
    manager: &M,
    entries: &[NativeMenuEntry],
) -> tauri::Result<Menu<R>> {
    let mut menu_items: Vec<MenuItemKind<R>> = Vec::new();
    for entry in entries {
        match entry {
            NativeMenuEntry::Item(spec) => {
                menu_items.push(build_item_kind(manager, spec)?);
            }
            NativeMenuEntry::Separator => {
                menu_items.push(MenuItemKind::Predefined(PredefinedMenuItem::separator(
                    manager,
                )?));
            }
            NativeMenuEntry::Submenu { label, items } => {
                let mut sub = SubmenuBuilder::new(manager, label);
                for child in items {
                    sub = match child {
                        NativeMenuEntry::Item(spec) => {
                            let enabled = spec.enabled.unwrap_or(true);
                            let item = MenuItemBuilder::with_id(spec.id.clone(), &spec.label)
                                .enabled(enabled)
                                .build(manager)?;
                            sub.item(&item)
                        }
                        NativeMenuEntry::Separator => {
                            sub.item(&PredefinedMenuItem::separator(manager)?)
                        }
                        NativeMenuEntry::Submenu { .. } => {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidInput,
                                "nested submenus are not supported",
                            )
                            .into());
                        }
                    };
                }
                menu_items.push(MenuItemKind::Submenu(sub.build()?));
            }
        }
    }
    let menu = Menu::new(manager)?;
    let item_refs: Vec<&dyn tauri::menu::IsMenuItem<R>> = menu_items
        .iter()
        .map(|item| item as &dyn tauri::menu::IsMenuItem<R>)
        .collect();
    menu.append_items(&item_refs)?;
    Ok(menu)
}

async fn popup_native_menu<R: Runtime>(
    app: &AppHandle<R>,
    request: NativePopupMenuRequest,
) -> Result<(), IpcError> {
    let window: tauri::Window<R> = app
        .get_window("main")
        .ok_or_else(|| IpcError::internal("Main Ferryx window is unavailable"))?;
    let window_clone = window.clone();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    window
        .run_on_main_thread(move || {
            let result = add_entries(&window_clone, &request.items)
                .and_then(|menu| {
                    let logical = LogicalPosition::new(request.position.x, request.position.y);
                    menu.popup_at(window_clone, Position::Logical(logical))
                })
                .map_err(|e| e.to_string());
            let _ = sender.send(result);
        })
        .map_err(|e| IpcError::internal(format!("Could not dispatch popup menu: {e}")))?;
    receiver
        .await
        .map_err(|_| IpcError::internal("Popup menu task was cancelled"))?
        .map_err(IpcError::internal)
}

#[tauri::command]
pub async fn cmd_native_terminal_context_menu<R: Runtime>(
    app: AppHandle<R>,
    items: Vec<NativeMenuEntry>,
    position: NativeMenuPoint,
) -> Result<(), IpcError> {
    popup_native_menu(&app, NativePopupMenuRequest { items, position }).await
}

#[tauri::command]
pub async fn cmd_native_tab_context_menu<R: Runtime>(
    app: AppHandle<R>,
    items: Vec<NativeMenuEntry>,
    position: NativeMenuPoint,
) -> Result<(), IpcError> {
    popup_native_menu(&app, NativePopupMenuRequest { items, position }).await
}

#[tauri::command]
pub async fn cmd_native_new_tab_menu<R: Runtime>(
    app: AppHandle<R>,
    items: Vec<NativeMenuEntry>,
    position: NativeMenuPoint,
) -> Result<(), IpcError> {
    popup_native_menu(&app, NativePopupMenuRequest { items, position }).await
}

#[tauri::command]
pub async fn cmd_native_sidebar_context_menu<R: Runtime>(
    app: AppHandle<R>,
    items: Vec<NativeMenuEntry>,
    position: NativeMenuPoint,
) -> Result<(), IpcError> {
    popup_native_menu(&app, NativePopupMenuRequest { items, position }).await
}

pub fn register_menu_event_forwarder<R: Runtime>(app: &AppHandle<R>) {
    app.on_menu_event(move |app_handle, event| {
        let _ = app_handle.emit(
            MENU_ACTION_EVENT,
            NativeMenuActionPayload {
                id: event.id().0.clone(),
            },
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_native_icon_maps_only_supported_names() {
        assert_eq!(resolve_native_icon("add"), Some(NativeIcon::Add));
        assert_eq!(resolve_native_icon("trash"), Some(NativeIcon::TrashFull));
        assert_eq!(resolve_native_icon("unknown"), None);
        assert_eq!(resolve_native_icon(""), None);
    }

    #[test]
    fn resolve_agent_icons_and_decode_custom_icon() {
        for agent in [
            "claude",
            "codex",
            "antigravity",
            "agy",
            "antigravity-cli",
            "opencode",
            "omo",
            "cursor",
            "cursor-agent",
            "aider",
            "crush",
            "droid",
            "copilot",
            "grok",
            "kimi",
            "cline",
            "pi",
            "gjc",
        ] {
            assert!(
                resolve_agent_icon(agent).is_some(),
                "missing icon for {agent}"
            );
        }
        assert!(resolve_agent_icon("unknown").is_none());

        let image = resolve_custom_icon("agent:claude")
            .expect("icon decodes")
            .expect("Claude icon resolves");
        assert_eq!(image.width(), 36);
        assert_eq!(image.height(), 36);
    }

    #[test]
    fn item_spec_deserializes_with_defaults() {
        let spec: NativeMenuItemSpec =
            serde_json::from_str(r#"{"id":"copy","label":"Copy","shortcut":"CmdOrCtrl+C"}"#)
                .expect("spec parses");
        assert_eq!(spec.id, "copy");
        assert!(spec.enabled.is_none());
        assert!(spec.icon.is_none());
        assert_eq!(spec.shortcut.as_deref(), Some("CmdOrCtrl+C"));
    }

    #[test]
    fn entries_deserialize_separator_and_submenu() {
        let entries: Vec<NativeMenuEntry> = serde_json::from_str(
            r#"[
                {"kind":"item","id":"a","label":"A"},
                {"kind":"separator"},
                {"kind":"submenu","label":"Sub","items":[{"kind":"item","id":"b","label":"B"}]}
            ]"#,
        )
        .expect("entries parse");
        assert_eq!(entries.len(), 3);
    }
}
