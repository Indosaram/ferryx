# P27 bounded remote wheel normalization

2026-09-13; child st_01a0995a. Local macOS arm64 mounted-component verification only.
Windows browser runtime remains pending. Lead owns combined verification/build.

## Scope and mechanism

Read root/UI AGENTS, P27 gap-packet-addendum and root loop C002 registration
at 2026-09-13T06:00:03.143Z. Both owned source files were clean before writes;
foreign working-tree edits were preserved. Only these two files and this receipt
were changed. No preferences, touch/pinch, transport protocol, native P02 files,
shared abstraction, commits, refs/worktrees or external runtime launches.

RemoteApp mounts RemoteTerminal using the effective session/retry key and host
transport. The measured socket request establishes viewport geometry; full/diff
frames set the displayed grid. The mounted wheel callback sends JSON scroll rows
to the socket; the Rust remote mirror preserves negative=older, positive=newer.
The original callback ignored deltaMode and forced every nonzero sub-row pixel
input into one row.

The wheel-only remainder now accumulates normalized fractional rows: 20 pixels
per row, three rows per line, displayed frame rows per page, then existing
[-10,10] clamp. Whole rows discarded by the clamp do not become future backlog.
Zero/horizontal-only input does not consume remainder. Socket open/close/cleanup
and document visibility changes discard remainder; hidden/non-open input cannot
send or accumulate. Socket cleanup covers session and host/socket replacement.
Page input before the first frame uses measured handshake geometry (one row only
if neither source is available). Visibility means document visibility here;
RemoteApp already unmounts/rekeys switched terminal sessions.

## Regression and readiness

The existing sign/clamp test now uses two -10 pixel half-row events and asserts
no extra send after the first. Added mounted real-handler coverage includes
positive/negative line and page input (7-row frame deliberately differs from
20-row measured geometry), both clamps, fractional 9.5+10+0.5 pixel accumulation,
zero/horizontal no-op, dropped-clamp backlog, session/transport/visibility reset,
and CONNECTING/CLOSING/CLOSED socket states.

Only the existing WebSocket seam is controlled. Production onopen/onmessage
subscriptions are checked before synchronous emission inside React act; lifecycle
callback and rendered line count prove readiness before wheel dispatch. All new
wheel tests are synchronous and have no async wait, polling, fixed sleeps or
readiness timers. Existing unrelated tests were not altered. Cleanup unmounts
components and restores spies/globals; no network socket or external process was
created by these tests.

Exact command for both runs:
`bun run --cwd ui test src/remote/RemoteTerminal.contract.test.tsx`

RED ran before any product edit: exit 1, 17 intended wheel failures, 41 passes.
GREEN ran once after minimal product change: exit 0, all 58 passed.
Test SHA256 before product edit and after edit (assertions identical):
`827c1d7c8f7ef991ada26c3c34b0a0258a5ea79d688727bf1564e2909b2fa169`.

Diagnostics (all severities, before GREEN):
- RemoteTerminal.tsx: `hint[typescript] (6385) at 744:18: 'keyCode' is deprecated.`
  Existing untouched keyboard compatibility line; no errors/warnings.
- RemoteTerminal.contract.test.tsx: `No diagnostics found`.
- Scoped `git diff --check`: exit 0, no output.

## Full RED output (ANSI styling removed)

```text
$ vitest run --maxWorkers=1 src/remote/RemoteTerminal.contract.test.tsx

 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui

 ❯ src/remote/RemoteTerminal.contract.test.tsx (58 tests | 17 failed) 261ms
   ✓ remote terminal grid contract > includes viewport geometry in the initial grid socket request 44ms
   ✓ remote terminal grid contract > reports whether a socket opened or closed before opening 9ms
   ✓ remote terminal grid contract > does not report an intentional component teardown as a socket failure 3ms
   ✓ remote terminal grid contract > automatically re-dials with exponential backoff on abnormal close (1s, 2s, 4s) 6ms
   ✓ remote terminal grid contract > successful reconnect reports open lifecycle again and resets backoff to 1s 7ms
   ✓ remote terminal grid contract > unmount does not schedule a re-dial or fire lifecycle events 3ms
   ✓ remote terminal grid contract > changing session cancels any pending reconnect timer and connects only to the new session 4ms
   ✓ remote terminal grid contract > guards against double-dialing so only one reconnect timer runs at a time 3ms
   ✓ remote terminal grid contract > caps backoff delay at 10s and continues retrying indefinitely 4ms
   ✓ remote terminal grid contract > reattaches to the newly focused session and ignores callbacks from the old socket 7ms
   ✓ remote terminal grid contract > renders a full grid frame and patches only named diff lines 8ms
   ✓ remote terminal grid contract > preserves keyboard, control-signal, destructive-editing, Enter, and paste encodings 6ms
   ✓ remote terminal grid contract > wraps multiline paste in bracketed paste mode markers to prevent prompt splitting 3ms
   ✓ remote terminal grid contract > does not shatter IME jamo keydowns into individual PTY writes 4ms
   ✓ remote terminal grid contract > commits IME composition through the input sink as one write 6ms
   ✓ remote terminal grid contract > renders and clears a local preedit overlay while composing 5ms
   ✓ remote terminal grid contract > sends non-composing sink input as text without keydown duplication 3ms
   ✓ remote terminal grid contract > snaps Hangul runs and the cursor overlay onto exact cell boundaries 6ms
   ✓ remote terminal grid contract > preserves control modifiers for physical navigation keys 3ms
   ✓ remote terminal grid contract > keeps MobileKeyDock modified navigation wiring 10ms
   ✓ remote terminal grid contract > resizes after the initial handshake only when viewport geometry changes 5ms
   ✓ remote terminal grid contract > renders surface with overflow-hidden to prevent layout scrollbars 3ms
   × remote terminal grid contract > sends clamped scroll message on wheel events over an open socket 6ms
     → expected "spy" to be called 1 times, but got 2 times
   × remote terminal grid contract > normalizes wheel mode 1 delta 1 using frame rows 7 to 3 wire rows 5ms
     → expected [ [ '{"type":"scroll","rows":1}' ] ] to deeply equal [ [ '{"type":"scroll","rows":3}' ] ]
   × remote terminal grid contract > normalizes wheel mode 1 delta -1 using frame rows 7 to -3 wire rows 4ms
     → expected [ [ '{"type":"scroll","rows":-1}' ] ] to deeply equal [ [ '{"type":"scroll","rows":-3}' ] ]
   × remote terminal grid contract > normalizes wheel mode 1 delta 100 using frame rows 7 to 10 wire rows 4ms
     → expected [ [ '{"type":"scroll","rows":5}' ] ] to deeply equal [ [ '{"type":"scroll","rows":10}' ] ]
   × remote terminal grid contract > normalizes wheel mode 1 delta -100 using frame rows 7 to -10 wire rows 3ms
     → expected [ [ '{"type":"scroll","rows":-5}' ] ] to deeply equal [ [ '{"type":"scroll","rows":-10}' ] ]
   × remote terminal grid contract > normalizes wheel mode 2 delta 1 using frame rows 7 to 7 wire rows 4ms
     → expected [ [ '{"type":"scroll","rows":1}' ] ] to deeply equal [ [ '{"type":"scroll","rows":7}' ] ]
   × remote terminal grid contract > normalizes wheel mode 2 delta -1 using frame rows 7 to -7 wire rows 3ms
     → expected [ [ '{"type":"scroll","rows":-1}' ] ] to deeply equal [ [ '{"type":"scroll","rows":-7}' ] ]
   × remote terminal grid contract > normalizes wheel mode 2 delta 1 using frame rows 24 to 10 wire rows 3ms
     → expected [ [ '{"type":"scroll","rows":1}' ] ] to deeply equal [ [ '{"type":"scroll","rows":10}' ] ]
   × remote terminal grid contract > normalizes wheel mode 2 delta -1 using frame rows 24 to -10 wire rows 3ms
     → expected [ [ '{"type":"scroll","rows":-1}' ] ] to deeply equal [ [ '{"type":"scroll","rows":-10}' ] ]
   × remote terminal grid contract > accumulates fractional pixels in direction 1 without zero or horizontal input consuming them 3ms
     → expected "spy" to not be called at all, but actually been called 2 times

Received: 

  1st spy call:

    Array [
      "{"type":"scroll","rows":1}",
    ]

  2nd spy call:

    Array [
      "{"type":"scroll","rows":1}",
    ]


Number of calls: 2

   × remote terminal grid contract > accumulates fractional pixels in direction -1 without zero or horizontal input consuming them 3ms
     → expected "spy" to not be called at all, but actually been called 2 times

Received: 

  1st spy call:

    Array [
      "{"type":"scroll","rows":-1}",
    ]

  2nd spy call:

    Array [
      "{"type":"scroll","rows":-1}",
    ]


Number of calls: 2

   × remote terminal grid contract > discards fractional wheel remainder across a session transition 5ms
     → expected "spy" to not be called at all, but actually been called 1 times

Received: 

  1st spy call:

    Array [
      "{"type":"scroll","rows":1}",
    ]


Number of calls: 1

   × remote terminal grid contract > discards fractional wheel remainder across a socket transition 4ms
     → expected "spy" to not be called at all, but actually been called 1 times

Received: 

  1st spy call:

    Array [
      "{"type":"scroll","rows":1}",
    ]


Number of calls: 1

   × remote terminal grid contract > discards fractional wheel remainder across a visibility transition 3ms
     → expected "spy" to not be called at all, but actually been called 1 times

Received: 

  1st spy call:

    Array [
      "{"type":"scroll","rows":1}",
    ]


Number of calls: 1

   × remote terminal grid contract > does not accumulate or send wheel input with socket readyState 0 4ms
     → expected "spy" to not be called at all, but actually been called 1 times

Received: 

  1st spy call:

    Array [
      "{"type":"scroll","rows":1}",
    ]


Number of calls: 1

   × remote terminal grid contract > does not accumulate or send wheel input with socket readyState 2 3ms
     → expected "spy" to not be called at all, but actually been called 1 times

Received: 

  1st spy call:

    Array [
      "{"type":"scroll","rows":1}",
    ]


Number of calls: 1

   × remote terminal grid contract > does not accumulate or send wheel input with socket readyState 3 3ms
     → expected "spy" to not be called at all, but actually been called 1 times

Received: 

  1st spy call:

    Array [
      "{"type":"scroll","rows":1}",
    ]


Number of calls: 1

   ✓ remote terminal grid contract > auto-focuses its input sink on mount and when activeTabId changes 4ms
   ✓ remote terminal grid contract > re-focuses its input sink when re-rendered with a new sessionId 3ms
   ✓ remote terminal grid contract > does not send scroll message on wheel event when socket is not open 2ms
   ✓ remote terminal grid contract > pastes literal text that collides with key-command names verbatim, not as key commands 2ms
   ✓ remote terminal grid contract > pastes multiline text whose lines collide with key names as verbatim bracketed paste 3ms
   ✓ remote terminal grid contract > pastes Object.prototype key names verbatim without leaking inherited members 2ms
   ✓ remote terminal grid contract > pastes non-ASCII unicode literals verbatim without control-byte mangling 2ms
   ✓ remote terminal grid contract > maps Korean hardware Ctrl+C (key jamo, code KeyC) to a structured interrupt 3ms
   ✓ remote terminal grid contract > maps Korean hardware Ctrl+D / Ctrl+L to physical ASCII control codes 3ms
   ✓ remote terminal grid contract > lets hardware Ctrl+V fall through to the browser paste event instead of sending a control byte 2ms
   ✓ remote terminal grid contract > preserves Ctrl+Shift+C for browser copy instead of sending an interrupt 2ms
   ✓ remote terminal grid contract > does not send CR for Enter while an IME composition is active 2ms
   ✓ remote terminal grid contract > does not send CR for Enter reported with IME keyCode 229 2ms
   ✓ remote terminal grid contract > encodes hardware Alt+letter as an ESC-prefixed meta chord instead of dropping it 2ms
   ✓ remote terminal grid contract > encodes Korean hardware Alt+letter via the physical code, not the remapped jamo 2ms
   ✓ remote terminal grid contract > preserves Shift case for Alt+letter meta chords (Alt+Shift+X -> ESC X) 3ms
   ✓ remote terminal grid contract > hands AltGraph printable keys to the input sink so the glyph is emitted once 2ms
   ✓ remote terminal grid contract > encodes Shift+Tab as a CSI back-tab sequence rather than a plain Tab 2ms
   ✓ remote terminal grid contract > never wraps an unsupported Ctrl+digit chord into a junk control byte 2ms

⎯⎯⎯⎯⎯⎯ Failed Tests 17 ⎯⎯⎯⎯⎯⎯⎯

 FAIL  src/remote/RemoteTerminal.contract.test.tsx > remote terminal grid contract > sends clamped scroll message on wheel events over an open socket
AssertionError: expected "spy" to be called 1 times, but got 2 times
 ❯ src/remote/RemoteTerminal.contract.test.tsx:699:27
    697|     // Two negative half-row pixel events produce one older-content ro…
    698|     fireEvent.wheel(surface(), { deltaY: -10 });
    699|     expect(socket().send).toHaveBeenCalledTimes(1);
       |                           ^
    700|     fireEvent.wheel(surface(), { deltaY: -10 });
    701|     expect(socket().send).toHaveBeenLastCalledWith(JSON.stringify({ ty…

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[1/17]⎯

 FAIL  src/remote/RemoteTerminal.contract.test.tsx > remote terminal grid contract > normalizes wheel mode 1 delta 1 using frame rows 7 to 3 wire rows
AssertionError: expected [ [ '{"type":"scroll","rows":1}' ] ] to deeply equal [ [ '{"type":"scroll","rows":3}' ] ]

- Expected
+ Received

  [
    [
-     "{\"type\":\"scroll\",\"rows\":3}",
+     "{\"type\":\"scroll\",\"rows\":1}",
    ],
  ]

 ❯ src/remote/RemoteTerminal.contract.test.tsx:743:37
    741|     fireEvent.wheel(surface(), { deltaMode, deltaY });
    742| 
    743|     expect(current.send.mock.calls).toEqual([[JSON.stringify({ type: "…
       |                                     ^
    744|   });
    745| 

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[2/17]⎯

 FAIL  src/remote/RemoteTerminal.contract.test.tsx > remote terminal grid contract > normalizes wheel mode 1 delta -1 using frame rows 7 to -3 wire rows
AssertionError: expected [ [ '{"type":"scroll","rows":-1}' ] ] to deeply equal [ [ '{"type":"scroll","rows":-3}' ] ]

- Expected
+ Received

  [
    [
-     "{\"type\":\"scroll\",\"rows\":-3}",
+     "{\"type\":\"scroll\",\"rows\":-1}",
    ],
  ]

 ❯ src/remote/RemoteTerminal.contract.test.tsx:743:37
    741|     fireEvent.wheel(surface(), { deltaMode, deltaY });
    742| 
    743|     expect(current.send.mock.calls).toEqual([[JSON.stringify({ type: "…
       |                                     ^
    744|   });
    745| 

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[3/17]⎯

 FAIL  src/remote/RemoteTerminal.contract.test.tsx > remote terminal grid contract > normalizes wheel mode 1 delta 100 using frame rows 7 to 10 wire rows
AssertionError: expected [ [ '{"type":"scroll","rows":5}' ] ] to deeply equal [ [ '{"type":"scroll","rows":10}' ] ]

- Expected
+ Received

  [
    [
-     "{\"type\":\"scroll\",\"rows\":10}",
+     "{\"type\":\"scroll\",\"rows\":5}",
    ],
  ]

 ❯ src/remote/RemoteTerminal.contract.test.tsx:743:37
    741|     fireEvent.wheel(surface(), { deltaMode, deltaY });
    742| 
    743|     expect(current.send.mock.calls).toEqual([[JSON.stringify({ type: "…
       |                                     ^
    744|   });
    745| 

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[4/17]⎯

 FAIL  src/remote/RemoteTerminal.contract.test.tsx > remote terminal grid contract > normalizes wheel mode 1 delta -100 using frame rows 7 to -10 wire rows
AssertionError: expected [ [ '{"type":"scroll","rows":-5}' ] ] to deeply equal [ [ '{"type":"scroll","rows":-10}' ] ]

- Expected
+ Received

  [
    [
-     "{\"type\":\"scroll\",\"rows\":-10}",
+     "{\"type\":\"scroll\",\"rows\":-5}",
    ],
  ]

 ❯ src/remote/RemoteTerminal.contract.test.tsx:743:37
    741|     fireEvent.wheel(surface(), { deltaMode, deltaY });
    742| 
    743|     expect(current.send.mock.calls).toEqual([[JSON.stringify({ type: "…
       |                                     ^
    744|   });
    745| 

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[5/17]⎯

 FAIL  src/remote/RemoteTerminal.contract.test.tsx > remote terminal grid contract > normalizes wheel mode 2 delta 1 using frame rows 7 to 7 wire rows
AssertionError: expected [ [ '{"type":"scroll","rows":1}' ] ] to deeply equal [ [ '{"type":"scroll","rows":7}' ] ]

- Expected
+ Received

  [
    [
-     "{\"type\":\"scroll\",\"rows\":7}",
+     "{\"type\":\"scroll\",\"rows\":1}",
    ],
  ]

 ❯ src/remote/RemoteTerminal.contract.test.tsx:743:37
    741|     fireEvent.wheel(surface(), { deltaMode, deltaY });
    742| 
    743|     expect(current.send.mock.calls).toEqual([[JSON.stringify({ type: "…
       |                                     ^
    744|   });
    745| 

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[6/17]⎯

 FAIL  src/remote/RemoteTerminal.contract.test.tsx > remote terminal grid contract > normalizes wheel mode 2 delta -1 using frame rows 7 to -7 wire rows
AssertionError: expected [ [ '{"type":"scroll","rows":-1}' ] ] to deeply equal [ [ '{"type":"scroll","rows":-7}' ] ]

- Expected
+ Received

  [
    [
-     "{\"type\":\"scroll\",\"rows\":-7}",
+     "{\"type\":\"scroll\",\"rows\":-1}",
    ],
  ]

 ❯ src/remote/RemoteTerminal.contract.test.tsx:743:37
    741|     fireEvent.wheel(surface(), { deltaMode, deltaY });
    742| 
    743|     expect(current.send.mock.calls).toEqual([[JSON.stringify({ type: "…
       |                                     ^
    744|   });
    745| 

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[7/17]⎯

 FAIL  src/remote/RemoteTerminal.contract.test.tsx > remote terminal grid contract > normalizes wheel mode 2 delta 1 using frame rows 24 to 10 wire rows
AssertionError: expected [ [ '{"type":"scroll","rows":1}' ] ] to deeply equal [ [ '{"type":"scroll","rows":10}' ] ]

- Expected
+ Received

  [
    [
-     "{\"type\":\"scroll\",\"rows\":10}",
+     "{\"type\":\"scroll\",\"rows\":1}",
    ],
  ]

 ❯ src/remote/RemoteTerminal.contract.test.tsx:743:37
    741|     fireEvent.wheel(surface(), { deltaMode, deltaY });
    742| 
    743|     expect(current.send.mock.calls).toEqual([[JSON.stringify({ type: "…
       |                                     ^
    744|   });
    745| 

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[8/17]⎯

 FAIL  src/remote/RemoteTerminal.contract.test.tsx > remote terminal grid contract > normalizes wheel mode 2 delta -1 using frame rows 24 to -10 wire rows
AssertionError: expected [ [ '{"type":"scroll","rows":-1}' ] ] to deeply equal [ [ '{"type":"scroll","rows":-10}' ] ]

- Expected
+ Received

  [
    [
-     "{\"type\":\"scroll\",\"rows\":-10}",
+     "{\"type\":\"scroll\",\"rows\":-1}",
    ],
  ]

 ❯ src/remote/RemoteTerminal.contract.test.tsx:743:37
    741|     fireEvent.wheel(surface(), { deltaMode, deltaY });
    742| 
    743|     expect(current.send.mock.calls).toEqual([[JSON.stringify({ type: "…
       |                                     ^
    744|   });
    745| 

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[9/17]⎯

 FAIL  src/remote/RemoteTerminal.contract.test.tsx > remote terminal grid contract > accumulates fractional pixels in direction 1 without zero or horizontal input consuming them
AssertionError: expected "spy" to not be called at all, but actually been called 2 times

Received: 

  1st spy call:

    Array [
      "{"type":"scroll","rows":1}",
    ]

  2nd spy call:

    Array [
      "{"type":"scroll","rows":1}",
    ]


Number of calls: 2

 ❯ src/remote/RemoteTerminal.contract.test.tsx:755:31
    753|     fireEvent.wheel(surface(), { deltaX: 100, deltaY: 0, deltaMode: 1 …
    754|     fireEvent.wheel(surface(), { deltaY: sign * 10 });
    755|     expect(socket().send).not.toHaveBeenCalled();
       |                               ^
    756|     fireEvent.wheel(surface(), { deltaY: sign * 0.5 });
    757|     expect(socket().send.mock.calls).toEqual([[JSON.stringify({ type: …

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[10/17]⎯

 FAIL  src/remote/RemoteTerminal.contract.test.tsx > remote terminal grid contract > accumulates fractional pixels in direction -1 without zero or horizontal input consuming them
AssertionError: expected "spy" to not be called at all, but actually been called 2 times

Received: 

  1st spy call:

    Array [
      "{"type":"scroll","rows":-1}",
    ]

  2nd spy call:

    Array [
      "{"type":"scroll","rows":-1}",
    ]


Number of calls: 2

 ❯ src/remote/RemoteTerminal.contract.test.tsx:755:31
    753|     fireEvent.wheel(surface(), { deltaX: 100, deltaY: 0, deltaMode: 1 …
    754|     fireEvent.wheel(surface(), { deltaY: sign * 10 });
    755|     expect(socket().send).not.toHaveBeenCalled();
       |                               ^
    756|     fireEvent.wheel(surface(), { deltaY: sign * 0.5 });
    757|     expect(socket().send.mock.calls).toEqual([[JSON.stringify({ type: …

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[11/17]⎯

 FAIL  src/remote/RemoteTerminal.contract.test.tsx > remote terminal grid contract > discards fractional wheel remainder across a session transition
 FAIL  src/remote/RemoteTerminal.contract.test.tsx > remote terminal grid contract > discards fractional wheel remainder across a socket transition
AssertionError: expected "spy" to not be called at all, but actually been called 1 times

Received: 

  1st spy call:

    Array [
      "{"type":"scroll","rows":1}",
    ]


Number of calls: 1

 ❯ src/remote/RemoteTerminal.contract.test.tsx:789:31
    787| 
    788|     fireEvent.wheel(surface(), { deltaY: 10 });
    789|     expect(socket().send).not.toHaveBeenCalled();
       |                               ^
    790|     fireEvent.wheel(surface(), { deltaY: 10 });
    791|     expect(socket().send.mock.calls).toEqual([[JSON.stringify({ type: …

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[12/17]⎯

 FAIL  src/remote/RemoteTerminal.contract.test.tsx > remote terminal grid contract > discards fractional wheel remainder across a visibility transition
AssertionError: expected "spy" to not be called at all, but actually been called 1 times

Received: 

  1st spy call:

    Array [
      "{"type":"scroll","rows":1}",
    ]


Number of calls: 1

 ❯ src/remote/RemoteTerminal.contract.test.tsx:783:33
    781|       fireEvent(document, new Event("visibilitychange"));
    782|       fireEvent.wheel(surface(), { deltaY: 10 });
    783|       expect(socket().send).not.toHaveBeenCalled();
       |                                 ^
    784|       visibility.mockReturnValue("visible");
    785|       fireEvent(document, new Event("visibilitychange"));

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[13/17]⎯

 FAIL  src/remote/RemoteTerminal.contract.test.tsx > remote terminal grid contract > does not accumulate or send wheel input with socket readyState 0
 FAIL  src/remote/RemoteTerminal.contract.test.tsx > remote terminal grid contract > does not accumulate or send wheel input with socket readyState 2
 FAIL  src/remote/RemoteTerminal.contract.test.tsx > remote terminal grid contract > does not accumulate or send wheel input with socket readyState 3
AssertionError: expected "spy" to not be called at all, but actually been called 1 times

Received: 

  1st spy call:

    Array [
      "{"type":"scroll","rows":1}",
    ]


Number of calls: 1

 ❯ src/remote/RemoteTerminal.contract.test.tsx:807:31
    805|     act(() => socket().onopen?.());
    806|     fireEvent.wheel(surface(), { deltaY: 10 });
    807|     expect(socket().send).not.toHaveBeenCalled();
       |                               ^
    808|     fireEvent.wheel(surface(), { deltaY: 10 });
    809|     expect(socket().send.mock.calls).toEqual([[JSON.stringify({ type: …

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[14/17]⎯


 Test Files  1 failed (1)
      Tests  17 failed | 41 passed (58)
   Start at  15:03:40
   Duration  1.19s (transform 91ms, setup 111ms, collect 461ms, tests 261ms, environment 245ms, prepare 29ms)

error: script "test" exited with code 1
```

## Full GREEN output

```text
$ vitest run --maxWorkers=1 src/remote/RemoteTerminal.contract.test.tsx

 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui

 ✓ src/remote/RemoteTerminal.contract.test.tsx (58 tests) 263ms

 Test Files  1 passed (1)
      Tests  58 passed (58)
   Start at  15:04:38
   Duration  1.32s (transform 82ms, setup 102ms, collect 433ms, tests 263ms, environment 233ms, prepare 27ms)

```

## Acceptance boundary

Lead independently read the entire diff, socket/geometry caller and full
RED/GREEN outputs. Preregistered independent command:
`bun run --cwd ui test src/remote/RemoteTerminal.contract.test.tsx --config vitest.remote-wheel-mutation.config.ts`.
mon_184VCM1AGGQ3SQAE / bash_40: exit 1, 17 intended failures / 41 passes,
1.43s, start 15:08:38; full 22,965-character output read. Exact-once
REMOTE_WHEEL_MUTATION_APPLIED:original-handler marker confirmed.
Production handler alone was replaced in memory with the original
open-socket/zero gate, trunc(deltaY/20)||sign and clamp10.
The temporary config initially had a mergeConfig type mismatch; replaced
with a typed base spread before execution and error diagnostics were clean.
Config removed after exit and absence checked. Production SHA256 unchanged:
5489567b0a2998d3c6e88982be206678956b2d2891ca45603cb78fec5b6ba269.
No test/config setup failure was accepted as behavioral RED.

The mounted DOM wheel surface and exact sent JSON are verified locally, not a
Windows browser or physical wheel. No browser/desktop/daemon/SSH launch occurred.
Windows browser runtime remains pending, as do lead combined tests/build. Changes
remain uncommitted in the shared tree and can be affected by concurrent writers.
