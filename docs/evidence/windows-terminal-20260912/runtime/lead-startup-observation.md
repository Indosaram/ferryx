# Initial Windows debug launch: direct lead observation

The lead retrieved the existing QA log with:

```sh
scp 'maho-win:C:/Users/sook/ferryx-qa-rt-st01a0958a/logs/dev-stdout.log' /tmp/ferryx-st01a0958a-startup.log
```

The launch log records `bun tauri dev`, isolated `FERRYX_RUNTIME_DIR` and
`FERRYX_SESSION_DIR`, and WebView debugging port 9223. The first launch did not
record a separate WebView profile.

The lead read the actual build completion and startup error:

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 6m 10s
Running `target\debug\ferryx.exe`
2026-09-12T12:25:46.503715Z ERROR tauri_runtime_wry:
failed to create webview: WebView2 error: WindowsError(Error {
  code: HRESULT(0x8007139F),
  message: "그룹 또는 리소스가 요청된 작업을 실행할 올바른 상태에 있지 않습니다."
})
```

The `Running` line is Cargo's child launch inside the required `bun tauri dev`
entry point, not an alternate direct-binary invocation.

This failure precedes terminal attachment. It is **not** RED evidence for the
reported native-bounds regression. A shared WebView profile is a hypothesis,
not a demonstrated cause. A QA-only profile is required for the next attempt;
the user's installed app and daemon must remain untouched.

The QA `ferryx.exe` process was observed in interactive Session 1 with a zero
main-window handle. This confirms neither a visible window nor rendering.

## Recorder correction

The lead fixed `qa/cdp-console-capture.mjs` to use the real `node:fs` `watch`
export on the existing evidence directory, filtering `cdp-stop.txt`. It closes
the watcher and clears the timeout when finishing. Actual Node execution on
the Mac reached the endpoint fetch and failed with `ECONNREFUSED 127.0.0.1:9223`,
as no local Windows CDP endpoint was forwarded. The former invalid named-import
failure is gone; live Windows CDP collection remains unverified.

The raw first-launch log must be preserved before a subsequent launcher
overwrites `dev-stdout.log`. The runtime worker owns that archival step.
