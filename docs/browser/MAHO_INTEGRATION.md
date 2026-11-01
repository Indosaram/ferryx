# Maho integration

## Owner grant

The Maho browser source is owned by the Ferryx owner. The owner granted reuse of that code in Ferryx as is.

| Item | Value |
| --- | --- |
| Grant | "maho 코드: 내가 maho 소유자, 그대로 써" |
| Translation | "Maho code: I am the owner of Maho, use it as is" |
| Date | 2026-11-01 |
| Source tree | `maho/crates/maho-core`, `maho/crates/maho-import`, `maho-chromium` |
| Source repo and commit | `Project-Maho/maho-browser@c2830251` |

### What follows from the grant

- Code from these paths may be copied into Ferryx without a separate license review.
- Each ported file gets a row in `docs/browser/maho-source-manifest.json` with `reuseMode` set to `"owner-granted-copy"` and the source commit `Project-Maho/maho-browser@c2830251`.
- A ported file keeps a header comment that names its Maho source path.

### What the grant doesn't cover

- Third-party code vendored inside Maho keeps its own license. Check it before copying anything from a vendored directory.
- Chromium and CEF binaries keep their own licenses.
