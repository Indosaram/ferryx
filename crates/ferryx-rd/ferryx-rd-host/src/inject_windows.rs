//! Windows input injection through `SendInput`.
//!
//! Protocol v3 keeps the existing macOS `NSEvent.keyCode` values on the wire.
//! This module mirrors that physical-key table to Windows virtual-key values,
//! maps the v3 modifier bits (Option -> Alt, Command -> Windows), and supports
//! normalized absolute pointer motion, relative motion, buttons, and scrolling.
//!
//! CI validates the bindings and pure mapping tests. Runtime QA must confirm
//! behavior across keyboard layouts, mixed-DPI virtual desktops, UAC elevation,
//! and games that reject synthetic input.

use std::{collections::HashSet, io, mem::size_of};

use ferryx_rd_proto::{InputEvent, InputEventType, Modifiers};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL};
use windows::Win32::Security::{
    Authorization::ConvertSidToStringSidW, GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_USERS, RRF_RT_REG_DWORD};
use windows::Win32::System::RemoteDesktop::{WTSGetActiveConsoleSessionId, WTSQueryUserToken};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::{
    Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSEEVENTF_ABSOLUTE,
        MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
        MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
        MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS, VIRTUAL_KEY,
        VK_CAPITAL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_RCONTROL, VK_RMENU, VK_RSHIFT,
        VK_RWIN,
    },
    WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    },
};

pub use crate::windows_logic::{
    macos_keycode_to_vk, normalize_absolute_pointer, vk_to_macos_keycode, wheel_data_from_scroll,
    TargetDisplay, VirtualDesktop,
};
use crate::windows_logic::{macos_keycode_to_windows_key, vk_is_extended};

pub struct WindowsInputInjector {
    gamepads: crate::gamepad_windows::Gamepads,
    target: TargetDisplay,
    desktop: VirtualDesktop,
    modifiers: Modifiers,
    // Ordinary keys and mouse buttons the peer is currently holding, so a
    // dropped session can release them instead of leaving them stuck.
    active_keys: HashSet<(u16, bool)>,
    active_buttons: HashSet<u8>,
    // `HKEY_USERS` path of the interactive user's mouse settings; the worker
    // runs as SYSTEM, so its own HKCU does not hold the user's preferences.
    mouse_settings_key: Option<Vec<u16>>,
}

impl WindowsInputInjector {
    pub fn new(target: Option<TargetDisplay>) -> io::Result<Self> {
        let desktop = current_virtual_desktop()?;
        let target = target.unwrap_or_else(|| TargetDisplay::from_virtual_desktop(desktop));
        if target.width == 0 || target.height == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "target display dimensions must be non-zero",
            ));
        }
        Ok(Self {
            gamepads: crate::gamepad_windows::Gamepads::default(),
            target,
            desktop,
            modifiers: Modifiers::empty(),
            active_keys: HashSet::new(),
            active_buttons: HashSet::new(),
            mouse_settings_key: interactive_user_sid()
                .map(|sid| wide_nul(&format!("{sid}\\Control Panel\\Mouse"))),
        })
    }

    pub fn update_target(&mut self, target: TargetDisplay) {
        self.target = target;
    }

    pub fn refresh_virtual_desktop(&mut self) -> io::Result<()> {
        self.desktop = current_virtual_desktop()?;
        Ok(())
    }

    pub fn move_relative(&mut self, dx: i32, dy: i32) -> io::Result<()> {
        send_inputs(&[mouse_input(dx, dy, 0, MOUSEEVENTF_MOVE)])
    }

    pub fn inject(&mut self, event: &InputEvent) -> io::Result<()> {
        if !event.x.is_finite()
            || !event.y.is_finite()
            || !event.scroll_dx.is_finite()
            || !event.scroll_dy.is_finite()
        {
            return Ok(());
        }
        match event.event_type {
            InputEventType::MouseMove
            | InputEventType::LeftMouseDragged
            | InputEventType::RightMouseDragged
            | InputEventType::LeftMouseDown
            | InputEventType::LeftMouseUp
            | InputEventType::RightMouseDown
            | InputEventType::RightMouseUp
            | InputEventType::MiddleMouseDown
            | InputEventType::MiddleMouseUp
            | InputEventType::PenMove
            | InputEventType::PenDown
            | InputEventType::PenUp => {
                let (x, y) =
                    normalize_absolute_pointer(event.x, event.y, self.target, self.desktop);
                let mut flags = MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK;
                flags |= match event.event_type {
                    InputEventType::LeftMouseDown | InputEventType::PenDown => MOUSEEVENTF_LEFTDOWN,
                    InputEventType::LeftMouseUp | InputEventType::PenUp => MOUSEEVENTF_LEFTUP,
                    InputEventType::RightMouseDown => MOUSEEVENTF_RIGHTDOWN,
                    InputEventType::RightMouseUp => MOUSEEVENTF_RIGHTUP,
                    InputEventType::MiddleMouseDown => MOUSEEVENTF_MIDDLEDOWN,
                    InputEventType::MiddleMouseUp => MOUSEEVENTF_MIDDLEUP,
                    _ => MOUSE_EVENT_FLAGS(0),
                };
                match event.event_type {
                    InputEventType::LeftMouseDown | InputEventType::PenDown => {
                        self.active_buttons.insert(0);
                    }
                    InputEventType::LeftMouseUp | InputEventType::PenUp => {
                        self.active_buttons.remove(&0);
                    }
                    InputEventType::RightMouseDown => {
                        self.active_buttons.insert(1);
                    }
                    InputEventType::RightMouseUp => {
                        self.active_buttons.remove(&1);
                    }
                    InputEventType::MiddleMouseDown => {
                        self.active_buttons.insert(2);
                    }
                    InputEventType::MiddleMouseUp => {
                        self.active_buttons.remove(&2);
                    }
                    _ => {}
                }
                send_inputs(&[mouse_input(x, y, 0, flags)])
            }
            InputEventType::GamepadAxis
            | InputEventType::GamepadButtonDown
            | InputEventType::GamepadButtonUp => self.gamepads.inject(event),
            InputEventType::RelativeMove => {
                let dx = event.scroll_dx.round() as i32;
                let dy = event.scroll_dy.round() as i32;
                self.move_relative(dx, dy)
            }
            InputEventType::Reset => {
                self.gamepads.reset();
                let mut inputs = Vec::new();
                inputs.push(mouse_input(
                    0,
                    0,
                    0,
                    MOUSEEVENTF_LEFTUP | MOUSEEVENTF_RIGHTUP | MOUSEEVENTF_MIDDLEUP,
                ));
                let mut released: HashSet<(u16, bool)> = std::mem::take(&mut self.active_keys);
                released.extend(
                    [VK_LSHIFT, VK_LCONTROL, VK_LMENU, VK_LWIN]
                        .map(|vk| (vk.0, vk_is_extended(vk.0))),
                );
                for (vk, extended) in released {
                    inputs.push(key_input_with_extended(vk, extended, true));
                }
                self.active_buttons.clear();
                self.modifiers = Modifiers::empty();
                send_inputs(&inputs)
            }
            InputEventType::ScrollWheel => {
                let mut inputs = Vec::with_capacity(2);
                // Read per event: the user can toggle the setting mid-session.
                let (vertical, horizontal) = wheel_data_from_scroll(
                    event.scroll_dx,
                    event.scroll_dy,
                    self.host_wheel_reversed(),
                );
                if vertical != 0 {
                    inputs.push(mouse_input(0, 0, vertical as u32, MOUSEEVENTF_WHEEL));
                }
                if horizontal != 0 {
                    inputs.push(mouse_input(0, 0, horizontal as u32, MOUSEEVENTF_HWHEEL));
                }
                send_inputs(&inputs)
            }
            InputEventType::KeyDown | InputEventType::KeyUp => {
                let is_up = event.event_type == InputEventType::KeyUp;
                let mut inputs = Vec::with_capacity(6);
                if let Some((vk, extended)) = macos_keycode_to_windows_key(event.key_code) {
                    // Right-side modifiers and CapsLock are modifier keys too:
                    // treating them as ordinary keys re-synthesizes the modifier
                    // state around them, duplicating presses and breaking AltGr.
                    let is_mod_key = matches!(
                        VIRTUAL_KEY(vk),
                        VK_LSHIFT
                            | VK_LCONTROL
                            | VK_LMENU
                            | VK_LWIN
                            | VK_RSHIFT
                            | VK_RCONTROL
                            | VK_RMENU
                            | VK_RWIN
                            | VK_CAPITAL
                    );
                    if !is_mod_key {
                        inputs.extend(modifier_inputs(self.modifiers, event.modifiers));
                        self.modifiers = event.modifiers;
                    }
                    if is_up {
                        self.active_keys.remove(&(vk, extended));
                    } else {
                        self.active_keys.insert((vk, extended));
                    }
                    inputs.push(key_input_with_extended(vk, extended, is_up));
                }
                send_inputs(&inputs)
            }
            InputEventType::FlagsChanged => {
                let inputs = modifier_inputs(self.modifiers, event.modifiers);
                self.modifiers = event.modifiers;
                send_inputs(&inputs)
            }
            InputEventType::UnicodeChar => {
                // Unicode text input: emit key down and key up with KEYEVENTF_UNICODE
                let inputs = vec![
                    unicode_input(event.key_code, false),
                    unicode_input(event.key_code, true),
                ];
                send_inputs(&inputs)
            }
        }
    }
}

impl Drop for WindowsInputInjector {
    fn drop(&mut self) {
        // A session can end without a peer Reset, so release everything the
        // remote user was holding: modifiers, ordinary keys, and buttons.
        let mut inputs = modifier_inputs(self.modifiers, Modifiers::empty());
        for (vk, extended) in self.active_keys.drain() {
            inputs.push(key_input_with_extended(vk, extended, true));
        }
        let mut button_flags = MOUSE_EVENT_FLAGS(0);
        for button in self.active_buttons.drain() {
            button_flags |= match button {
                0 => MOUSEEVENTF_LEFTUP,
                1 => MOUSEEVENTF_RIGHTUP,
                _ => MOUSEEVENTF_MIDDLEUP,
            };
        }
        if button_flags != MOUSE_EVENT_FLAGS(0) {
            inputs.push(mouse_input(0, 0, 0, button_flags));
        }
        let _ = send_inputs(&inputs);
    }
}

impl WindowsInputInjector {
    /// Whether Windows will reverse injected wheel input because the host user
    /// enabled "Reverse scrolling direction" (Settings > Mouse).
    fn host_wheel_reversed(&self) -> bool {
        let Some(key) = self.mouse_settings_key.as_ref() else {
            return false;
        };
        let value = wide_nul("ReverseMouseWheelDirection");
        let mut data = 0u32;
        let mut size = size_of::<u32>() as u32;
        // SAFETY: both strings are NUL-terminated and `data`/`size` describe a valid DWORD buffer.
        let status = unsafe {
            RegGetValueW(
                HKEY_USERS,
                PCWSTR(key.as_ptr()),
                PCWSTR(value.as_ptr()),
                RRF_RT_REG_DWORD,
                None,
                Some((&mut data as *mut u32).cast()),
                Some(&mut size),
            )
        };
        status.is_ok() && data != 0
    }
}

fn wide_nul(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// SID string of the user logged on to the console, falling back to the
/// current process user when not running as SYSTEM (for example a user task).
fn interactive_user_sid() -> Option<String> {
    let mut token = HANDLE::default();
    // SAFETY: `token` is valid output storage and is closed below on every path.
    unsafe {
        if WTSQueryUserToken(WTSGetActiveConsoleSessionId(), &mut token).is_err() {
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).ok()?;
        }
        let sid = token_user_sid(token);
        let _ = CloseHandle(token);
        sid
    }
}

/// # Safety
/// `token` must be a valid token handle opened with `TOKEN_QUERY` access.
unsafe fn token_user_sid(token: HANDLE) -> Option<String> {
    let mut needed = 0u32;
    // SAFETY: a size query with no buffer only writes `needed`.
    let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut needed) };
    if needed == 0 {
        return None;
    }
    // u64 storage keeps the TOKEN_USER header suitably aligned.
    let mut buffer = vec![0u64; (needed as usize).div_ceil(size_of::<u64>())];
    // SAFETY: `buffer` holds at least `needed` bytes and outlives every use of the SID.
    unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            needed,
            &mut needed,
        )
        .ok()?;
        let user = &*(buffer.as_ptr() as *const TOKEN_USER);
        let mut text = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut text).ok()?;
        let sid = text.to_string().ok();
        let _ = LocalFree(Some(HLOCAL(text.0.cast())));
        sid
    }
}

fn current_virtual_desktop() -> io::Result<VirtualDesktop> {
    let x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    let width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) };
    let height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) };
    if width <= 0 || height <= 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(VirtualDesktop {
        x,
        y,
        width: width as u32,
        height: height as u32,
    })
}

fn modifier_inputs(previous: Modifiers, current: Modifiers) -> Vec<INPUT> {
    [
        (Modifiers::SHIFT, VK_LSHIFT.0),
        (Modifiers::CONTROL, VK_LCONTROL.0),
        (Modifiers::OPTION, VK_LMENU.0),
        (Modifiers::COMMAND, VK_LWIN.0),
    ]
    .into_iter()
    .filter_map(|(modifier, vk)| {
        let was_down = previous.contains(modifier);
        let is_down = current.contains(modifier);
        (was_down != is_down).then(|| key_input(vk, !is_down))
    })
    .collect()
}

fn key_input(vk: u16, key_up: bool) -> INPUT {
    key_input_with_extended(vk, vk_is_extended(vk), key_up)
}

fn key_input_with_extended(vk: u16, extended: bool, key_up: bool) -> INPUT {
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if key_up {
        flags |= KEYEVENTF_KEYUP;
    }
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn mouse_input(dx: i32, dy: i32, data: u32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn unicode_input(code_unit: u16, key_up: bool) -> INPUT {
    let mut flags = KEYEVENTF_UNICODE;
    if key_up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: code_unit,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send_inputs(inputs: &[INPUT]) -> io::Result<()> {
    if inputs.is_empty() {
        return Ok(());
    }
    let sent = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
    if sent == inputs.len() as u32 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
