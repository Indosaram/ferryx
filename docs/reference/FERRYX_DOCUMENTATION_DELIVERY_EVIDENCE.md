# Ferryx Documentation Delivery Evidence

Date: 2026-08-23

## Delivered surfaces

- Root `README.md` is the sole root Markdown entry point.
- Product and icon specifications moved to [`PRODUCT.md`](./PRODUCT.md) and [`BRANDING_AND_ICON_SPECIFICATION.md`](./BRANDING_AND_ICON_SPECIFICATION.md).
- The repository homepage and README both link to the live Pages site: <https://indosaram.github.io/ferryx/>.

## Captured checks

| Check | Result |
| --- | --- |
| README Pages URL extractor | Printed `https://indosaram.github.io/ferryx/` |
| Root Markdown inventory | Only `./README.md` |
| GitHub repository homepage | `https://indosaram.github.io/ferryx/` |
| GitHub Pages configuration | Public workflow deployment at `https://indosaram.github.io/ferryx/` |
| Pages workflow | Run `32615521563` succeeded for published `main` |
| Live Pages request | `curl -sSIL https://indosaram.github.io/ferryx/` returned HTTP 200 |
| Live HTML smoke check | Fetched HTML contained `Ferryx` |
| UI tests | `bun run ui:test` passed: 76 files, 536 tests |
| Documentation site build | `bun run --cwd site build` passed |
| Rust check | `cargo check --manifest-path src-tauri/Cargo.toml` passed |
| Clean CI-equivalent Pages build | Installed `site` and `ui` dependencies from a fresh archive, then `BASE_URL=/ferryx bun run --cwd site build` passed |

## Cleanup receipt

The temporary full-repository CI extraction used for the build check was removed after verification. No server, browser, container, terminal multiplexer, or bound port remains from this delivery.
