# Microsoft Store resubmission: verification and recovery

Use this alongside [the submission guide](../MS_STORE_SUBMISSION.md) when updating the existing Ferryx Store product. This is an update procedure, not a new-product registration procedure.

## Identity and history: check before changing anything

- Ferryx Store product: `9NLHQL5JNLM4`.
- Package identity: `ProjectMaho.Ferryx`.
- Read the current app, pending submission, package versions, listing metadata, pricing, and certification history before deciding what to change.
- Distinguish **previous submission**, **certification acceptance**, and **public Store publication**. None implies the others.
- A `firstPublishedDate` of `1601-01-01` does not prove that the app has never been submitted. A CLI exception describing a "first submission" is not authoritative historical evidence either.
- Replace the package in the existing editable submission where possible. Do not delete a draft to work around a CLI error without first backing up its metadata and establishing why deletion is necessary. A recreated draft may not inherit descriptions, images, pricing, or other declarations.
- A canceled submission's certification report is evidence of prior processing, not proof of approval or public availability.

Before uploading, inspect the actual package version and identity, not just the stable filename `Ferryx_x64.msix`. Use the release selected for this request; do not reuse the incident's version as a permanent default. Follow the release runbook and the user's build-host restrictions. Submission recovery does not require a local rebuild.

## Browser automation: dispatched is not applied

The 2026-09-28 incident demonstrated that Maho can return `dispatched: true` for an offscreen click that does not apply. Background-tab input and focus changes also require verification.

1. Pin every operation to the intended Partner Center tab. Do not operate on unrelated tabs.
2. Bring the control into view by scrolling. Inspect a screenshot before retrying an apparently ineffective action. A scroll receipt alone does not prove that the target is visible.
3. Refresh the accessibility snapshot and resolve the control again. Accessibility references are scoped to the session; do not move a reference from one CLI process into another.
4. Click the visible control, then verify the actual change: expanded menu, chooser, saved navigation, server field, or status transition.
5. If text entry reports failure, inspect the field before retrying. In this incident the description was entered despite a dispatch error, with line breaks removed. Check the saved content rather than trusting either the tool error or the form's appearance.
6. After two equivalent failures, change the diagnostic approach. Do not repeatedly click the same invisible control or declare an unsupported feature from failed dispatches.

### Screenshot upload

Maho supports `input.file_upload_select`. The required sequence is:

1. Open the English listing and scroll the **Desktop screenshots** add card into view.
2. Click that card using a fresh reference in the same session as its snapshot.
3. Once a chooser is pending, invoke `input.file_upload_select` with the pinned `tab_id` and the absolute path to the intended PNG.
4. Verify the Desktop screenshot count increases and the uploaded image is the intended asset.
5. Scroll the **Save** button into view and click it.
6. Verify the submission API contains an `Uploaded` screenshot and its validation errors no longer include `NoScreenshotsOfAnyType`.

`file upload could not be dispatched` does not establish that file upload is unsupported. First establish that the preceding click actually opened a chooser. Neither `file_selected: true` nor an upload preview proves that the listing was saved.

## Description and pricing recovery

Restore the intended description from a reviewed backup or current release copy. Save it, then read it back from the server. Populated browser fields are unsaved state until persistence is verified.

For `No PriceSchedule created for purchasable product` or `InvalidPricingAvailabilitySettings`:

1. Inspect the current pricing controls and API fields. Do not immediately label this a Microsoft outage.
2. Select a valid base market/currency and move focus into **Retail price** so its price tiers load. Confirm the list contains real options, including the zero-price tier for a free app.
3. Choose the zero-price tier, leave the field to trigger its change/blur handling, and save the draft.
4. Read back pricing and validation errors. For this free release, the required result was `pricing.priceId = Free`, not merely a textbox showing `0`.
5. If an incomplete future price-change row was added while diagnosing, remove it before saving. Do not leave accidental schedules or change intended market availability.

Observed recovery, not a universal recipe: adding then removing an empty price-change row initialized the missing controls. The EUR/Andorra tier list was empty; a USD market loaded real tiers. Typing `0` before tiers loaded saved an invalid `Base` price and triggered a tax/payout warning. Selecting zero after tiers loaded saved `Free` and removed the validation error. Do not change tax or payout details to address that warning until the intended free price is verified.

## API versus Partner Center

The [official update endpoint](https://learn.microsoft.com/en-us/windows/uwp/monetize/update-an-app-submission) documents updates to Partner Center-created submissions. Nevertheless, this incident's draft returned HTTP 409:

```text
Cannot update the submission because it is in the state 'None'.
If you need to change the submission, delete the submission and create a new one.
```

The same draft was repaired and submitted through Partner Center without deletion. Therefore, do not turn that API response into a claim that the UI draft cannot be recovered. Inspect the actual UI and preserve the current package and metadata. Do not print tokens, client secrets, or upload SAS URLs in evidence or documentation.

## Completion gate: accepted submission, not uploaded package

Before clicking **Submit for certification**, verify all of these against the current submission:

| Requirement | Required evidence |
| --- | --- |
| Correct app and release | Product ID, submission ID, actual package version and identity |
| Package accepted | Package `Uploaded` and UI package validation complete |
| Description persisted | Nonempty intended description read back from the submission |
| Screenshot persisted | Intended screenshot `Uploaded` and listing complete |
| Intended pricing | For this free app, `Free`, with intended market availability |
| Validation clear | API `statusDetails.errors` empty and required UI sections Complete |

Run `msstore submission status 9NLHQL5JNLM4 --output-stream Stdout` or inspect the live submission API, and inspect Partner Center. Click the visible **Submit for certification** button once. Subscribe to any processing wait instead of repeatedly polling manually.

After submitting, verify the same submission and package version remain present, errors are empty, and the UI shows **In certification**. API `PreProcessing` means accepted into preprocessing; `Certification` means the certification stage has begun. `PendingCommit`, `CommitStarted` alone, or `CommitFailed` is insufficient proof of certification acceptance. Report approval and publication only when those later states are actually observed.

Capture a screenshot and a redacted API/status result, and record IDs, version, state, and publication mode. Once the requested acceptance condition holds, stop. Repeated continuation messages for the completed goal are not authorization to cancel or submit it again.

## Incident receipt: 2026-09-28

- Previous canceled submission: `1152921505701820247`, package `2026.922.1.0`, description and screenshot present, certification report dated 2026-09-27.
- Recreated submission: `1152921505701985808`, package `2026.927.3.0`. Description, screenshot, and pricing were initially incomplete.
- Recovery retained that submission and package; listing and free price were saved through Partner Center.
- Final live audit: HTTP 200, `status = Certification`, `errors = []`, `priceId = Free`, package `2026.927.3.0` Uploaded, description length 739, screenshot `Ferryx.png` Uploaded.
- UI: **In certification**, automatic publication after approval. This was not a claim of completed approval or publication.
- Session evidence: `.omo/ulw-store-resubmission-20260928.md` and `/tmp/maho-screenshot-1790596127.png`. These are local evidence paths, not durable public artifacts; the verified facts are preserved here.

The original failure was an incorrect first-submission assumption compounded by unverified automation actions. Preserve history, verify persistence, and base completion claims on the live submission rather than tool dispatch receipts.
