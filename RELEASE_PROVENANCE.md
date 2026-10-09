# Release Provenance: Ferryx 2026.930.1

- **Base Frozen SHA**: `016936d4`
- **Release Version**: `2026.930.1`
- **Builder Host**: `maho-mac`
- **Signing Policy**: Local signing only (no hosted/CI signing)
- **Included Target Commits**:
  - `ce0bcce8`
  - `74d560e2`
- **Source Diffs from Base SHA**: Exactly 4 source modifications
  1. `src-tauri/Cargo.toml`: Version bumped from `2026.928.7` to `2026.930.1`
  2. `src-tauri/tauri.conf.json`: Version bumped from `2026.928.7` to `2026.930.1`
  3. `ui/src/lib/tauri.ts`: Corrected `watchDagProject` function boundary and duplicate return syntax
  4. `src-tauri/src/remote/server.rs`: Added missing `RemoteServerHandle::prepare_relay` and `RemoteServerHandle::replace_relay` definitions required by committed daemon callers
- **Auxiliary Artifacts in Snapshot**:
  - `release-build.sh`: Build orchestration script for `maho-mac` (`REMOTE_RELEASE_BUILD_OK`)
- **Shared Repository Safety**: Zero modifications to shared repositories or working trees
