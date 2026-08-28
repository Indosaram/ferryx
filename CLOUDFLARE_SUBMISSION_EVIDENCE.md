# Cloudflare Domain Evidence - Submission Ready

Verified on 2026-08-28. This record identifies the two unmodified, original
Cloudflare PDFs retained locally for the pending Microsoft verification
submission. No Microsoft, Partner Center, or other submission portal was
opened during collection or verification.

## Original source documents

| Purpose | Original file | SHA-256 | Verified contents |
| --- | --- | --- | --- |
| Paid domain-registration invoice | `/Users/indo/Downloads/d2fa46be-71b3-575a-8941-518f8148fbfb.pdf` | `622cedc9e4013e1280cf6c80d1e1fa4d4b9a57830cbd141a000e643bdc9dc344` | Cloudflare invoice `IN-70662991`; issued 2026-07-06; `Registrar Registration Fee - mahobrowser.com (1 yr)`; total `$10.46 USD`; billed to the registrant. |
| Domain ownership certificate | `/Users/indo/Downloads/mahobrowser.com_ownership_letter.pdf` | `2d65e1fcd6449b6292bcdd436b9a5abcc8b3eb8ddaed850965e9a2ddd22b46e5` | Cloudflare's ICANN-accredited registrar certificate for `MAHOBROWSER.COM`, identifying the registrant and confirming creation at `2026-07-06T15:01:25Z` and expiry at `2027-07-06T15:01:25Z`. |

## Verification record

- Both files exist, begin with the `%PDF-1.4` signature, and extract cleanly
  with `pdftotext`.
- Chromium Download History records both files as downloaded from
  `https://dash.cloudflare.com`.
- The invoice is the official Cloudflare download selected from the paid
  `IN-70662991` billing row; its internal issuer is `Cloudflare, Inc.`.
- The certificate identifies Cloudflare as the registrar and the domain's
  registration data. Its expiry is well beyond the requested two-month
  threshold.

## Remaining action

The verified originals remain in `~/Downloads` without modification. The only
remaining step is to attach these two files to the Microsoft verification
submission; no upload or submission has been attempted.
