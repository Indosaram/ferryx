# Ferryx website measurement and search

## Scope

Measure discovery of `https://ferryx.dev` and clicks leading to download destinations.
A download click is not a completed download, installation, or active desktop user.
Website analytics must not be used to claim desktop retention.

## Production configuration

The site reads public build-time settings, not runtime Worker secrets:

| Setting | Purpose |
| --- | --- |
| `PUBLIC_GA_MEASUREMENT_ID` | GA4 web stream measurement ID |
| `PUBLIC_GSC_VERIFICATION` | Google Search Console HTML verification token |
| `PUBLIC_BING_VERIFICATION` | Bing Webmaster Tools HTML verification token |

These identifiers are public in generated HTML; never put OAuth tokens, passwords,
service-account keys, or Analytics API secrets in `PUBLIC_*` variables.
Set the values before the Astro build. A settings change requires rebuilding and
deploying the static assets; changing a Worker runtime variable is insufficient.

### Configured account

The authorized Google identity is `freedomzero91@gmail.com`.
The existing Analytics account is `indosaram` (`162818114`), with a dedicated
Ferryx property (`555083020`) and web stream (`15807325534`). The measurement ID
is `G-D6CY5B9DF5`. Google signals is off. Event-scoped custom dimensions are
`platform`, `link_location`, and `asset_id`; `download_click` is a key event.

This workstation stores build values in gitignored `site/.env.local`:

```dotenv
PUBLIC_GA_MEASUREMENT_ID=G-D6CY5B9DF5
PUBLIC_GSC_VERIFICATION=RvDMNp1gu3QoJl-nGsntxCcY5i34jQYl6der51aAQJw
```

Recreate that file or export the values before deployment from another machine.
Leave them unset for untracked preview builds. Search Console verification and
sitemap submission require the verification tag to be present on the live site.

### Verified deployment (2026-09-19)

- Cloudflare apex version: `3a81b704-c23d-40eb-9a26-c77214d3c65c`.
- Live homepage and facts page return 200 with the measurement ID and Google
  verification tag. Robots and sitemap index return 200 on the canonical domain.
- Google Search Console URL-prefix property is accessible under the authorized
  identity; `/sitemap-index.xml` shows **Success**.
- Bing imported only `https://ferryx.dev/` from Search Console. Its sitemap list
  shows one imported sitemap, **Processing**, with zero errors and warnings.
- GA Realtime displays `page_view` and `download_click`; the latter also appears
  under key events. QA events are test traffic, not evidence of customer adoption.
- Site test suite: 82 passing tests. Production build: 17 generated pages.
- Desktop/mobile Chrome checks: no Google requests before consent or after an
  initial decline; page and download events after consent; no test query-secret
  leakage and no JavaScript errors.

Search indexing and Bing sitemap processing are engine-side asynchronous work;
successful submission does not mean pages already rank or appear in AI answers.

## Acquisition links

Use lowercase, stable UTM values on links shared outside the website:

```text
https://ferryx.dev/?utm_source=youtube&utm_medium=video&utm_campaign=parallel_agents&utm_content=workflow_demo
https://ferryx.dev/use-cases/parallel-ai-agents/?utm_source=reddit&utm_medium=community&utm_campaign=parallel_agents
```

- `utm_source`: the actual referring platform or publication.
- `utm_medium`: `video`, `community`, `social`, or `referral`.
- `utm_campaign`: a stable campaign or use-case name.
- `utm_content`: optional creative identifier, not a person's email or name.

Do not add UTMs to internal links: they obscure the original acquisition source.
Do not put personal data, credentials, terminal output, or project paths in UTMs.

## Reports

In GA4, use Traffic acquisition for session source/medium and campaign. Compare
visits with `download_click`, broken down by `platform` and `link_location`.
Register those parameters as event-scoped custom dimensions and mark
`download_click` as a key event. Interpret conversion rates as download interest
among measured visitors, not all visitors: people who decline analytics or block
tracking are absent.

Use Realtime or DebugView to validate an intentional test visit. Ordinary reports
and custom dimensions can take time to populate. A successful browser collection
request alone does not prove that the correct property received the event.

## Search registration

1. Verify `https://ferryx.dev/` in Google Search Console using the configured HTML
   tag, or verify the domain via DNS if available.
2. Submit `https://ferryx.dev/sitemap-index.xml`.
3. Inspect the homepage and the main use-case pages. Request indexing when useful;
   a successful request does not guarantee inclusion or ranking.
4. Add the same site in Bing Webmaster Tools, using Search Console import or its
   own verification tag, and submit the sitemap there too.
5. Check indexing errors and search queries after engines have processed the site.

The canonical origin is `https://ferryx.dev`. `www.ferryx.dev` should return a 301
preserving paths and queries. Robots and sitemap must be available without login.
Server-rendered product facts and documentation support search and AI retrieval;
no special file or schema guarantees inclusion in AI answers.
