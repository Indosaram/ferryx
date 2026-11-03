# ferryx-rd-host Knowledge Base

<!-- Score: 26 | Domain: Rust cross-platform host streaming daemon (DXGI/Hyprland/SCK capture, VAAPI/NVENC/MF encode) -->

## OVERVIEW
Cross-platform host daemon orchestrating native screen capture, hardware video encoding, input injection, audio capture, and network streaming across macOS, Windows, and Linux.

## STRUCTURE
```
clients/rust/ferryx-rd-host/src/
├── main.rs               # Binary entry point & CLI argument parser
├── lib.rs                # Library interface and public re-exports
├── session.rs            # Host session state machine, pairing store, consent prompt
├── capture_macos.rs      # ScreenCaptureKit capture implementation (macOS)
├── capture_windows.rs    # DXGI Desktop Duplication capture (Windows)
├── capture_linux.rs      # Hyprland / wlroots / X11 / PipeWire capture (Linux)
├── encode_vt.rs          # VideoToolbox hardware HEVC encoder (macOS)
├── encode_windows.rs     # MediaFoundation H.264/HEVC encoder (Windows)
├── encode_linux.rs       # NVENC / VAAPI hardware, OpenH264 software encoder (Linux)
├── inject_macos.rs       # Quartz CGEvent input injection (macOS)
├── inject_windows.rs     # SendInput API injection (Windows)
└── inject_linux.rs       # uinput / libei synthetic input (Linux)
```

## WHERE TO LOOK
| Task | File | Key Symbol |
|------|------|------------|
| Host Server Lifecycle | `session.rs` | `HostServer`, `HostConfig`, `SessionState` |
| Pairing & Consent Auth | `session.rs` | `PairingStore`, `ConsentPrompt`, `random_pin` |
| macOS Capture & Encode | `capture_macos.rs`, `encode_vt.rs` | `ScreenCapture`, `VideoToolboxEncoder` |
| Windows Capture & Encode | `capture_windows.rs`, `encode_windows.rs` | `WindowsCapture`, `MediaFoundationEncoder` |
| Linux Capture & Encode | `capture_linux.rs`, `encode_linux.rs` | `focused_output_name` |
| OS Input Injections | `inject_macos.rs`, `inject_windows.rs`, `inject_linux.rs` | `InputInjector`, `WindowsInputInjector` |

## CONVENTIONS
- Platform implementations are strictly partitioned by `#[cfg(target_os = "...")]`.
- Long-running encoding or synchronous frame captures must execute within `tokio::task::spawn_blocking` or dedicated background threads.
- Host configuration defaults to secure pairing authorization via PIN generation and persistent pairing store.

## ANTI-PATTERNS
- Do not run blocking platform APIs directly on the async Tokio event loop.
- Never inject un-normalized mouse/keyboard coordinates into OS input injection targets.
- Avoid holding frame buffers across capture cycles to minimize streaming latency.
