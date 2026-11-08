# Supervisor voice call (plan)

Status: implemented in this change set; nothing here has been built or tested on the authoring
Mac (see Verification). Extends the Discord-only speech path with an in-app surface.

## Goal

The Supervisor sidebar gets a call button that opens a full-screen voice surface: the owner
speaks a task or a control word, the supervisor acts through the existing hub request service,
and the answer is spoken back. It is a hands-free **command and status channel**, not a chat
partner: every spoken reply is the hub's own receipt for a request just made, or a sentence
built from the live snapshot.

## Why this shape

The hub's conversation view replays admitted owner messages only
(`crates/ferryx-hub/src/supervisor/request.rs`, `conversation`): engine replies are never
persisted there. A voice surface therefore cannot answer open questions from history without
inventing text. Three honest outcomes exist today, and the surface speaks exactly those.

| Owner says | Hub answer | Spoken |
|---|---|---|
| a task | `admitted` / `awaiting_owner` with a generated `acknowledgment` | the acknowledgment verbatim |
| a question | `conversational { reason }` | that nothing was launched, plus the reason |
| a scope that needs a pick | `needs_choice { missing, detail }` | the missing choice and its detail |
| "status" | no hub request | `goalWaitLine` for each open goal |

## Architecture

```
microphone -> VoiceCallSurface -> encodeWav (24 kHz mono 16-bit)
                     |                        |
                     |                        v
                     |            cmd_voice_call_transcribe --> hub voice::transcribe
                     |                        |
                     v                        v
              answerVoiceTurn --> desktopUnifiedClient.request (SupervisorRequestV1)
                     |                        |
                     |                        v
                     |              parseSubmitReceipt --> spoken receipt text
                     v                        |
            cmd_voice_call_speak <------------+
                     |
                     v
        hub voice::synthesize_pcm -> PCM 24 kHz -> pcm_to_wav -> base64 WAV -> <audio>
```

No new hub route and no new daemon protocol variant. The Tauri command layer calls
`ferryx_hub::voice::*` in process, the same functions the Discord huddle uses, and so obeys the
same settings (`OMON_STT_*`, `OMON_TTS_*`, `OPENAI_API_KEY`, `OMON_VOICE_ENABLED`).

## Contracts

### Tauri commands (`src-tauri/src/ipc/voice_call.rs`, `desktop` gated)

| Command | Args | Returns |
|---|---|---|
| `cmd_voice_call_status` | - | `VoiceCallStatus` |
| `cmd_voice_call_transcribe` | `wavBase64` | `{ text }` |
| `cmd_voice_call_speak` | `text` | `SpokenAudio` |

`VoiceCallStatus`: `{ available, reason, sttModel, ttsModel, ttsVoice, sampleRate, channels,
maxUtteranceMs, minSpeechMs, silenceMs }`. `reason` is null only when available, and its
wording is written for this surface rather than reusing the Discord-worded
`VoiceSettings::unavailable_reason`.

`SpokenAudio`: `{ audioBase64, mimeType, characters, durationMs }`. Empty `audioBase64` means
nothing speakable survived `voice::spoken_text`; the surface then stays silent instead of
playing a zero-length file.

Refusal codes (structured; the UI never matches message text):

- `VOICE_UNAVAILABLE` - no key, voice disabled, or a build without `hub-voice`.
- `VOICE_INPUT_INVALID` - the upload is not RIFF/WAVE, or exceeds the size limit.
- `VOICE_PROVIDER_FAILED` - the speech provider refused, or the call failed.

### UI (`ui/src/lib/voiceCall.ts`)

Pure, testable pieces:

- `encodeWav(samples, sampleRate)` - canonical 44-byte header, mono 16-bit, mirroring
  `voice::pcm_to_wav`.
- `resample(input, from, to)` - linear interpolation; the device rate (44.1/48 kHz) is not the
  provider's 24 kHz.
- `UtteranceGate` - frame-driven VAD returning `collect | close | discard` under the same
  silence rules as `voice_pcm::UtteranceAssembler`, measured per frame so the first take closes
  without a timer.
- `parseVoiceIntent(transcript, goals)` - deterministic, no model: `status`, a control word on
  exactly one open goal, `choose-goal` when several are open, `submit` otherwise.
- `spokenReceipt(receipt)` and `statusSpeech(view)`.
- `answerVoiceTurn(transcript, client, view)` - performs the request and returns the text to
  speak, so the whole decision runs in tests without a microphone or a hub.

### Turn loop

`listening -> transcribing -> thinking -> speaking -> listening`, with `error`, `unavailable`
and `idle` (muted) states. A tap on the orb while speaking stops playback and opens a new take;
a take whose gate never saw speech is discarded silently.

## Settings and failure surfaces

Not-ready is a normal state, not a crash: the surface opens, states the reason, and offers the
typed composer. The three reasons are: the `hub-voice` feature is off, `OMON_VOICE_ENABLED`
switched voice off, or no speech key is configured.

## Platform matrix (measured against the pinned wry 0.55.1)

| Target | `getUserMedia({audio})` | Notes |
|---|---|---|
| macOS | granted automatically: `wry_web_view_ui_delegate.rs` implements `requestMediaCapturePermissionForOrigin` and answers `WKPermissionDecision::Grant` unconditionally | still needs `NSMicrophoneUsageDescription`, and `com.apple.security.device.audio-input` under hardened runtime |
| Windows | depends on the WebView2 prompt and the OS privacy setting; wry auto-grants only clipboard reads | a prompt is expected; denial is a supported path back to the composer |
| Linux | WebKitGTK exposes no media-stream setting in wry; capture depends on the distro build | least reliable; the typed composer carries it |

Fallback ladder: hosted STT/TTS (`hub-voice`) -> browser `SpeechRecognition` +
`speechSynthesis` (the adapter already exists in `ui/src/remote/chat/referenceDictation.ts`) ->
typed composer. This change ships the first and third tiers; the browser tier stays a named
follow-up so an unmeasured path is never presented as working.

## Files

- `src-tauri/src/ipc/voice_call.rs` (new) - the three commands and their tests.
- `src-tauri/src/ipc/mod.rs` - one additive module line.
- `src-tauri/src/lib.rs` - three additive handler registrations.
- `src-tauri/Info.plist` (new) - `NSMicrophoneUsageDescription`.
- `src-tauri/Entitlements.plist` (new) - `com.apple.security.device.audio-input`, wired through
  `bundle.macOS.entitlements`; `scripts/macos-daemon-helper.mjs` already records the outer app's
  entitlements and carries them to the daemon helper.
- `src-tauri/tauri.conf.json` - `media-src` gains `blob: data:` in `csp` and `devCsp`; the
  entitlements path joins `bundle.macOS`.
- `ui/src/lib/voiceCall.ts` (new), `ui/src/lib/voiceCall.test.ts`.
- `ui/src/components/supervisor/VoiceCallSurface.tsx` (new), `.test.tsx`.
- `ui/src/components/supervisor/SupervisorPanel.tsx` - the header call button.

## Verification

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::voice_call
bun run --cwd ui test src/lib/voiceCall.test.ts src/components/supervisor/VoiceCallSurface.test.tsx
bunx tsc --noEmit -p ui/tsconfig.json
```

Measured 2026-11-08 on maho-mac (detached worktree `voicecall-20261108` at `084f91e97`, shared
`CARGO_TARGET_DIR`), with `--no-default-features --features desktop,hub-slack,hub-discord,hub-voice`:

- `cargo check`: exit 0.
- `cargo test --lib voice_call`: 13 passed, 0 failed.
- `vitest run` on the three voice files: 78 passed, 3 files.
- `bunx tsc --noEmit -p tsconfig.json`: exit 0.
- `cargo test --lib voice_call::live -- --ignored` (live provider round trip): 1 passed,
  0 failed, finished in 4.01s.

The run above used `--no-default-features` to skip `remote-desktop` while iterating, which needs
no FFmpeg SDK. That set fails at HEAD without `hub-slack` on an ungated `meta::omo_client`
reference in `meta/backend.rs`, which is pre-existing and unrelated to this change.

Default features (including `remote-desktop` and its FFmpeg 9 SDK) were built for real on
maho-mac: `cargo tauri build --bundles app` finished the release profile in 15m 24s and produced
`Ferryx.app` (457 MB); only the updater step failed, for a missing `TAURI_SIGNING_PRIVATE_KEY`.
Signing that bundle with the host's Developer ID identity and `Entitlements.plist` gave
`codesign --verify --strict` exit 0 and a readable `com.apple.security.device.audio-input`, and
`NSMicrophoneUsageDescription` is in the built `Info.plist`. Launched with an isolated
`FERRYX_DATA_DIR`/`FERRYX_RUNTIME_DIR` it started and created its daemon sockets; all three
`cmd_voice_call_*` commands are present in the shipped binary and in the bundled UI assets.

Measured with real provider credentials through the shipped command path:

- `cmd_voice_call_speak("The supervisor voice call is working correctly.")` returned a clip
  (base64 > 1000 chars, duration > 500 ms), and feeding that exact clip back through
  `cmd_voice_call_transcribe` returned a transcript containing both "supervisor" and "working".
  The text is synthesized as PCM, wrapped to WAV, and sent to `/audio/transcriptions`, so the
  surface's own speech path is what ran.

Still required before shipping:

1. A real human call in the GUI. Microphone capture from an SSH-launched process is digital
   silence on macOS (TCC denies the grant: both 3 s and 6 s captures measured rms 0.0, peak 0,
   input level 90%, and the silent file still got HTTP 200 from the STT endpoint), so the
   acoustic leg has to be measured from an interactive session that holds the microphone
   permission. Everything downstream of the microphone is measured.
2. Windows and Linux capture stay unmeasured until run on those hosts.
3. A free-form spoken question is answered with the hub's receipt, not model prose: the desktop
   submit path yields `SubmitOutcome::Conversational` and no prose source is wired here.
