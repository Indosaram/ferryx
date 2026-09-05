# Typing Input Audit and Fix - 2026-09-05

## Result

Fixed input ownership and encoding defects in the desktop native terminal, the
remote terminal, and the global shortcut router. Related regression suites passed
313 tests across 8 files. The frontend typecheck and build passed with exit code 0.

## Confirmed defects and changes

1. Remote paste interpreted text as commands. Pasting `ctrl-c` sent an interrupt
   signal; `up`, `tab`, and `delete` became terminal escape/control sequences.
   `onPaste` now sends literal UTF-8 through `sendText`, not the mobile-key command
   decoder. Multiline newline normalization and bracketed-paste markers remain.
   Tests also cover `toString` and `constructor`, which collided with inherited
   properties in the old decoder.
2. Remote hardware Ctrl chords used layout-dependent Unicode key values. Korean
   Ctrl+C could become byte 0x0A rather than an interrupt. Physical KeyA-KeyZ
   mapping now preserves Ctrl+C interrupt, Ctrl+D 0x04, and Ctrl+L 0x0C. Explicit
   control-byte mapping prevents unsupported digits from wrapping to junk bytes.
3. Remote Ctrl+V was prevented and sent as byte 0x16, blocking browser paste.
   Ctrl+V, Ctrl+Shift+V, and Ctrl+Shift+C now leave browser clipboard handling intact.
4. Remote Alt+letter was dropped; Shift+Tab sent ordinary Tab. Alt chords now
   produce an ESC prefix, preserve Shift letter case, and Shift+Tab sends ESC [ Z.
5. IME-owned keydowns could reach the PTY before composition commit. Both native
   keydown paths and the remote handler now honor active composition and legacy
   keyCode 229. Native fallback focuses only its owning sink when composition
   starts without sink focus. Composition commit remains the text-send path.
6. Native AltGr characters became Ctrl+Alt physical-key chords. Both native input
   paths and remote input now preserve the browser input event when AltGraph is
   active. The actual modifier state is authoritative; Ctrl+Alt alone is not
   treated as AltGr. Genuine Ctrl+Alt chords still work.
7. Global shortcuts intercepted IME and AltGr input before the terminal handler.
   `matchesBinding` now rejects isComposing, keyCode 229, Process, Dead, and
   AltGraph-owned events, preventing mid-composition palette/tab actions and
   AltGr-triggered unsplit actions.

## Changed files

- `ui/src/components/NativeTerminalPane.tsx`: event normalization and shared
  ownership guards, applied to sink and document fallback.
- `ui/src/components/NativeTerminalPane.test.tsx`: IME/AltGr regressions and
  normal-chord controls.
- `ui/src/lib/shortcuts.ts`: ownership guard in `matchesBinding` (line 428).
- `ui/src/lib/shortcuts.test.tsx`: independent IME-signal cases and AltGr control.
- `ui/src/remote/RemoteTerminal.tsx`: chord encoding, input ownership, and paste
  (keyboard handler near line 652; paste handler near line 725).
- `ui/src/remote/RemoteTerminal.contract.test.tsx`: literal paste, keyboard-layout,
  clipboard, composition, AltGr, Alt, and back-tab regressions.

The pre-existing device-density matchMedia changes in NativeTerminalPane.tsx were
preserved. Other sessions had many uncommitted backend/UI/documentation changes;
those were not reverted. No repository commit was created.

## Regression evidence

- Native initial RED: 5 new ownership tests failed before the fix because keys
  were prevented or sent as chords. Final file run: 137 passed.
- Remote initial RED: 12 failed and 28 passed before the encoder fix. Final
  contract run after review corrections: 42 passed.
- Shortcut initial RED: 3 failed and 52 passed. Tests were then split into
  independent ownership signals. AltGr RED: 1 failed and 62 passed. Final: 63 passed.
- Final combined command, exit code 0:

```sh
bun run --cwd ui test \
  src/components/NativeTerminalPane.test.tsx \
  src/components/NativeTerminalPane.lifecycle.test.tsx \
  src/lib/shortcuts.test.tsx \
  src/lib/nativeTerminalLifecycle.test.ts \
  src/remote/RemoteTerminal.contract.test.tsx \
  src/remote/RemoteTerminalGestures.test.tsx \
  src/remote/RemoteUI.test.tsx \
  src/remote/RemoteAttention.test.tsx
```

```text
Test Files  8 passed (8)
Tests       313 passed (313)
```

- `bun run --cwd ui build`: exit code 0; tsc passed, 1860 Vite modules transformed,
  Vite build completed in 1.98 seconds. This was a frontend build, not a desktop
  release bundle.
- `git diff --check`: exit code 0.
- LSP diagnostics were attempted, but the LSP daemon was unreachable. The complete
  frontend tsc run in the build supplies typechecking evidence instead.

## Real browser runtime verification

Used an isolated Bun.WebView on macOS WebKit with the actual React components.
The remote component connected to a real loopback WebSocket on an ephemeral port.
The native component used the public Tauri mockIPC boundary to record calls. No
user desktop app, clipboard, existing terminal session, or daemon was manipulated.

Before the fix, the browser demonstrated:

- Paste `ctrl-c` -> text WebSocket frame
  `{"type":"signal","signal":"interrupt"}`.
- Native Enter with keyCode 229 -> defaultPrevented true and
  `cmd_native_terminal_send_input` containing keyEvent Enter.
- Native AltGr @ on KeyQ -> defaultPrevented true and keyEvent q with Ctrl+Alt.

After the fix:

- Nine remote wire checks passed: literal ctrl-c/up/toString paste, multiline
  bracketed paste, Korean Ctrl+C and Ctrl+D, Alt+x, Alt+Shift+X, and Shift+Tab.
- Remote IME Enter produced no write; composition commit produced exactly one
  UTF-8 syllable. AltGr produced no keydown write, then one @ on input. Ctrl+V
  remained unprevented and the following paste emitted literal tab text.
- Native sink and body fallback both left IME229 and active-composition Enter
  unprevented with no input IPC. Composition commit sent one syllable. AltGr
  sent no key chord and the input event sent @ once; the correct sink held focus.
- Ordinary native x still sent x once. IME-owned Ctrl+K produced neither a
  global shortcut action nor an input IPC.
- A clean final remote browser run reported no browser errors. One earlier
  harness evaluation had an unescaped CR/LF syntax error; fixing the evaluation
  string and repeating in a fresh page cleared that harness-only error.

These are real WebKit DOM and wire-boundary checks using constructed keyboard/IME
events, not proof of physical OS IME behavior or native PTY end-to-end rendering.
No visual layout change was made.

## Manual desktop verification still required

Desktop automation is prohibited by the user. Use the debug app only, launched
with exactly `bun tauri dev`, and check:

1. Click between split panes and immediately type a Korean sentence with spaces.
   The first syllable must compose normally, without isolated jamo or duplicate
   spaces, and all text must remain in the selected pane.
2. While choosing an IME candidate, press Enter. It must confirm the candidate
   without prematurely submitting the terminal command. Then press Enter after
   composition ends to submit normally.
3. With a Korean keyboard layout, verify the intended Ctrl+C/Ctrl+D/Ctrl+L and
   clipboard shortcuts in a disposable terminal session.
4. On the remote client, paste the literal strings up, tab, delete, and ctrl-c.
   They must remain text, not navigation/deletion/interrupt commands.
5. On a Windows/Linux AltGr layout, type @ or braces and verify no shortcut runs.
   Check Alt+b/Alt+f and Shift+Tab in a program that uses those bindings.

The full frontend suite and backend Rust tests were not run; validation targeted
the changed input domain and its lifecycle/remote callers, plus the full frontend
typecheck/build. No backend source was changed by this task.
