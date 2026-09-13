# V05 resumed pixel review

Date: 2026-09-12. Reviewer task: `st_01a097fe`.

## Whole-set verdict: FAIL (evidence blocked, not a product failure)

All eight requested PNGs were individually submitted to the Read image tool in this session. None was delivered as visible image input. Every call returned exactly:

```text
Read image file [image/png]
[Current model does not support images. The image will be omitted from this request.]
(tool image omitted: model does not support images)
```

Consequently, zero of eight captures received actual pixel inspection. No visual PASS, product defect, contrast assessment, or absence-of-clipping claim is justified. The binary verdict below concerns acceptance of the required pixel review: FAIL means unavailable visual evidence, not proof that the rendered product is wrong. No dimensions, OCR, DOM measurements, or prior code-review conclusions were substituted for viewing pixels.

## Per-capture binary verdicts

Paths below are relative to `/Users/indo/code/project/orca-lite-wt/herdr-wave0-clean/docs/evidence/paired-daemon/v05-settings-qa/`. Dimensions are requested/documented viewports, not newly measured results.

| Capture | Viewport | Verdict | Exact evidence limitation and unchecked visual requirements |
| --- | --- | --- | --- |
| `ssh-desktop-light.png` | 1280x900 | FAIL | Read omitted image with the error above. Heading/body readability, overlap/clipping, light-theme contrast, and the small green check beside Added in the expanded system-config row remain uninspected. |
| `ssh-desktop-dark.png` | 1280x900 | FAIL | Read omitted image with the error above. Heading/body readability, overlap/clipping, dark-theme contrast, and the small green Added check remain uninspected. |
| `remote-desktop-light.png` | 1280x900 | FAIL | Read omitted image with the error above. Heading/body readability, overlap/clipping, light-theme contrast, and visible reconnection/re-pair paragraph content and wrapping remain uninspected. |
| `remote-desktop-dark.png` | 1280x900 | FAIL | Read omitted image with the error above. Heading/body readability, overlap/clipping, dark-theme contrast, and visible reconnection/re-pair paragraph content and wrapping remain uninspected. |
| `ssh-mobile-light.png` | 390x844 | FAIL | Read omitted image with the error above. Narrow-layout heading/body readability, overlap/clipping, light-theme contrast, and the small green Added check remain uninspected. |
| `ssh-mobile-dark.png` | 390x844 | FAIL | Read omitted image with the error above. Narrow-layout heading/body readability, overlap/clipping, dark-theme contrast, and the small green Added check remain uninspected. |
| `remote-mobile-light.png` | 390x844 | FAIL | Read omitted image with the error above. Narrow-layout heading/body readability, overlap/clipping, light-theme contrast, and reconnection/re-pair paragraph wrapping remain uninspected. |
| `remote-mobile-dark.png` | 390x844 | FAIL | Read omitted image with the error above. Narrow-layout heading/body readability, overlap/clipping, dark-theme contrast, and reconnection/re-pair paragraph wrapping remain uninspected. |

## Findings and evidence boundaries

- **[evidence] Blocking: image input unsupported for every capture.** This is a reviewer-input limitation, not an identified defective PNG or source defect. Actual inspection requires an image-capable review environment; recapture or product edits are not justified solely by this error. Exact pixel defect locations cannot be supplied without seeing the pixels.
- Read in full: `V05-visual-review-boundary.md`, `V05-settings-visual.md`, `V05-independent-code-review.md`, and `V05-parent-source-manifest.json` from the read-only source worktree. The prior boundary documents the same unsupported-image limitation; this session independently reproduced it for all eight paths rather than assuming the earlier result.
- The prior settings report records persisted PNGs, requested dimensions, DOM/layout checks, theme-token colors, deterministic IPC fixtures, and cleanup. Those are prior reported facts, not new pixel evidence. The manifest records candidate source hashes; this session did not independently hash current source or certify capture freshness against a current build.
- The recorded fixture imports real settings components and styles, but uses a padded, maximum-960px container instead of the full application shell. SSH captures show the expanded system-config state; Remote captures cover disabled Remote Access with no paired devices. Other states, interactions, motion, shell integration, and design-system integrity are not newly approved here. There was no independent dual-oracle PASS in this session.

## Residual native-desktop and test gates

Settings-only browser fixture captures cannot certify native terminal compositor pixels, native desktop runtime behavior, real transport, or daemon E2E operation, even if subsequently approved visually. The existing native manual gate remains open, including:

1. Final-output retention after shell exit and disappearing session metadata, with post-exit input blocked.
2. Switching disposable panes and back while preserving focus, cursor, typing, selection, split geometry, and unrelated sessions; native dialog ownership and teardown remain outside these captures.
3. Same-backend-ID epoch/generation replacement or reconnect: replacement binding attaches/presents, and stale binding input cannot recover or write through the replacement.

Any eventual native check remains subject to the documented isolated `bun tauri dev` launch path, isolated application data, no replacement/restart of a running user daemon, and no unauthorized desktop input automation. None was executed by this review.

The separate full-UI gate also remains failed according to `V05-independent-code-review.md`: 4,396 passed and three pre-existing push-stub failures in `ui/src/features/ferryx/push/client.test.ts`. The recorded failures are exact-task-link parsing (expected URL, received null), denied permission (expected denied, received enabled), and server-unsubscribe failure preservation (expected rejection, received disabled). This session did not rerun tests, resolve these failures, or treat them as V05 pixel defects. Backend V04 and whole-plan acceptance remain separate.

## Execution receipt / disposition

Only this report was written, using `apply_patch`. No source, prior evidence, images, or other worktree files were modified. No browser/app was launched, no capture was regenerated, no package was installed, and no daemon, PTY, or OS input was touched.

The requested fallback stop condition is met: all eight image reads have explicit tooling limitations recorded and the report is persisted. The visual completion gate is **not satisfied**; no user acceptance of that gap is asserted.
