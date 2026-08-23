# Fix: P-backlog (F-terminal-01)

## Summary
- **Packet ID**: P-backlog
- **Finding ID**: F-terminal-01
- **Severity**: High
- **Description**: Replaced monolithic 512KB string concatenation and slicing (`${this.backlog.get(sessionId) ?? ""}${text}`) in `TerminalEventBus.publishOutput` with a chunk ring (`SessionBacklog = { chunks: string[], totalChars: number }`). Oldest chunks are trimmed incrementally when total characters exceed `MAX_BACKLOG_CHARS` (512KiB). String joining occurs lazily only when replaying output on initial subscriber attachment.

## Files Changed
- `ui/src/lib/terminalEvents.ts` (production implementation)
- `ui/src/lib/terminalEvents.bus.test.ts` (RED/GREEN test suite)

## Production Change Location
- File: `ui/src/lib/terminalEvents.ts`
- Key changes:
  - Line 14: Defined `SessionBacklog` type (`{ chunks: string[]; totalChars: number }`).
  - Line 76: Updated `backlog` map to `Map<string, SessionBacklog>()`.
  - Lines 92-97: In `subscribeOutput`, joined `existing.chunks.join("")` lazily upon replay.
  - Lines 137-160: In `publishOutput`, pushed incoming chunk to `entry.chunks`, updated `totalChars`, and trimmed oldest chunks while `totalChars > MAX_BACKLOG_CHARS`.
  - Lines 111-133: Implemented test helper `getBacklogMetricsForTest`.

## RED Test Run

Command:
```bash
cd /Users/indo/code/project/orca-lite/ui && bun run test src/lib/terminalEvents.test.ts src/lib/terminalEvents.bus.test.ts
```

Output:
```
 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui

 ✓ src/lib/terminalEvents.test.ts (4 tests) 3ms
 ❯ src/lib/terminalEvents.bus.test.ts (5 tests | 2 failed) 27ms
   ✓ terminalEventBus title tracking > publishes OSC title changes even when no xterm output subscriber is mounted 2ms
   ✓ terminalEventBus title tracking > deduplicates repeated identical OSC titles 1ms
   × terminalEventBus backlog buffer > stores incoming output in chunk array without quadratic concatenation 6ms
     → expected 1 to be greater than 1
   × terminalEventBus backlog buffer > bounds total backlog size to MAX_BACKLOG_CHARS and replays latest tail to new subscriber 16ms
     → expected 1 to be greater than 1
   ✓ terminalEventBus backlog buffer > clears session backlog and listeners on clearSession 0ms

⎯⎯⎯⎯⎯⎯⎯ Failed Tests 2 ⎯⎯⎯⎯⎯⎯⎯

 FAIL  src/lib/terminalEvents.bus.test.ts > terminalEventBus backlog buffer > stores incoming output in chunk array without quadratic concatenation
AssertionError: expected 1 to be greater than 1
 ❯ src/lib/terminalEvents.bus.test.ts:88:28
     86|     expect(metrics.chars).toBe(chunkCount * 100);
     87|     // On chunk-ring implementation, chunks must be preserved (>1) rat…
     88|     expect(metrics.chunks).toBeGreaterThan(1);
       |                            ^
     89|     expect(metrics.chunks).toBe(chunkCount);
     90| 

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[1/2]⎯

 FAIL  src/lib/terminalEvents.bus.test.ts > terminalEventBus backlog buffer > bounds total backlog size to MAX_BACKLOG_CHARS and replays latest tail to new subscriber
AssertionError: expected 1 to be greater than 1
 ❯ src/lib/terminalEvents.bus.test.ts:123:28
    121|     const metrics = getBacklogMetricsForTest(sessionId);
    122|     expect(metrics.chars).toBe(maxBacklog);
    123|     expect(metrics.chunks).toBeGreaterThan(1);
       |                            ^
    124| 
    125|     unsubscribe();

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[2/2]⎯


 Test Files  1 failed | 1 passed (2)
      Tests  2 failed | 7 passed (9)
   Start at  23:40:19
   Duration  1.77s (transform 90ms, setup 308ms, collect 89ms, tests 29ms, environment 725ms, prepare 118ms)
```

## GREEN Test Run

Command:
```bash
cd /Users/indo/code/project/orca-lite/ui && bun run test src/lib/terminalEvents.test.ts src/lib/terminalEvents.bus.test.ts
```

Output:
```
 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui

 ✓ src/lib/terminalEvents.bus.test.ts (5 tests) 11ms
 ✓ src/lib/terminalEvents.test.ts (4 tests) 2ms

 Test Files  2 passed (2)
      Tests  9 passed (9)
   Start at  23:40:36
   Duration  860ms (transform 46ms, setup 144ms, collect 66ms, tests 13ms, environment 334ms, prepare 55ms)
```

## Leftover Risk
- None identified. Backlog buffer size remains strictly capped at 512KiB (`MAX_BACKLOG_CHARS = 512 * 1024`). Replay on subscription preserves exact tail output and ordering. Memory is cleaned up during `clearSession(sessionId)`.
