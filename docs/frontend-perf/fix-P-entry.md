# Fix: P-entry (F-remote-01, F-bundle-01)

Lead implemented after DAG `fix-entry` never started and standalone `st_01a029fb` stayed queued.

## Files
- `ui/src/main.tsx` — conditional `import()` of `./App` (Tauri) or `./remote/RemoteApp` (web)
- `ui/src/index-html.test.ts` — pins Ferryx PNG favicon; asserts main.tsx does not statically import both roots

## RED
```
cd /Users/indo/code/project/orca-lite/ui && bun run test src/index-html.test.ts
```
Failed: `does not statically import both App and RemoteApp in main.tsx` expected true to be false.

## GREEN
Same command: 2/2 passed. Also `devRuntimeContract.test.ts` 2/2 and `manifest.test.ts` 3/3 passed.

`appearanceThemeContract.test.ts` has a pre-existing/tabbar-owned failure (`SortableTab` Globe className), not caused by this packet.
