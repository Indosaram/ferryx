# Public release preflight
Captured: 2026-08-26T14:36:13.301406
This is the last remote read-only check before a user-approved updater-only commit and tag push.
## Tag absent at origin
```text
$ git ls-remote --tags origin refs/tags/v2026.08.26.1
(no output)
exit=0
```
## Release absent
```text
$ gh release view v2026.08.26.1
release not found
exit=1
```
## Current public latest.json endpoint
```text
$ curl -sS -L -o /tmp/ferryx-preflight-latest -w 'http=%{http_code} url=%{url_effective} bytes=%{size_download}' -m 15 https://github.com/Indosaram/ferryx/releases/latest/download/latest.json; echo; head -c 120 /tmp/ferryx-preflight-latest 2>/dev/null; rm -f /tmp/ferryx-preflight-latest
http=404 url=https://github.com/Indosaram/ferryx/releases/download/v0.1.0/latest.json bytes=9
Not Found
exit=0
```
## GitHub signing secret names
```text
$ gh secret list | rg '^TAURI_SIGNING_PRIVATE_KEY'
TAURI_SIGNING_PRIVATE_KEY	2026-08-26T04:36:03Z
TAURI_SIGNING_PRIVATE_KEY_PASSWORD	2026-08-26T04:36:04Z
exit=0
```
## Verdict
`v2026.08.26.1` has no origin tag or GitHub Release. The public latest endpoint resolves to the existing `v0.1.0` release and returns 404 because that release predates updater publishing. Both signing-secret names exist. Creating the updater-only commit and pushing the date tag remain the explicit user-approval boundary.
