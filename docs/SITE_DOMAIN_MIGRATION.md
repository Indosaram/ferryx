# Ferryx site hosting on ferryx.dev

The site is served from `https://ferryx.dev` by a Cloudflare Worker with static assets,
deployed from `site/`. It previously lived at `https://indosaram.github.io/ferryx/`, which
capped organic search traffic for two reasons no on-page work could fix:

- **`robots.txt` is origin-scoped.** Crawlers read `https://indosaram.github.io/robots.txt`,
  which belongs to a different repository. The well-formed file generated at
  `/ferryx/robots.txt` was never consulted.
- **Domain authority was shared.** Every signal earned on `indosaram.github.io` pooled
  across every project on that host and none of it accrued to a Ferryx brand domain.

## Why a Worker and not Pages

The site is static, so Pages was the obvious first choice and the project was built and
deployed there. Attaching the custom domain is where it stopped: the Pages API only creates
the zone's DNS records when the calling credential carries DNS edit permission. A
`wrangler login` OAuth session does not have it. Its zone permissions are `#zone:read`,
`#worker:edit` and `#ssl:edit`, with no `#dns_records:edit`, so both domains sat at
`pending` against an empty zone and nothing resolved.

Workers custom domains provision their DNS server-side under `#worker:edit`, which the same
session does have. Workers Static Assets serves the identical `dist/` output, so the switch
cost nothing functionally and removed the permission deadlock. It is also where Cloudflare
is steering new static projects.

Reverting to Pages is possible at any time, but it needs an API token with Zone, DNS, Edit
on `ferryx.dev` to create the two records.

## How the build knows where it lives

`site/astro.config.mjs` reads two environment variables, and nothing else decides the URLs:

- `SITE_URL` is the absolute origin, used for `canonical`, `og:url`, `og:image` and the
  `Sitemap:` line in `robots.txt`. It is `https://ferryx.dev`.
- `BASE_URL` is the path prefix. It is **deliberately unset**, because the site is served
  from the domain root. Setting it would prefix every generated path and break the site.

Both are covered by regression tests in `site/src/seo.test.ts`: one rebuilds the whole site
with `BASE_URL` unset and asserts the canonical, `og:url`, `og:image` and `robots.txt`
values, and another walks every built page and fails if an internal link is missing its
expected base, which is what catches a half-applied prefix change.

## Deployment

`site/wrangler.jsonc` declares the Worker, the asset directory and both custom domains, so
`wrangler deploy` reproduces the whole routing setup rather than depending on dashboard
state. `.github/workflows/deploy-cloudflare.yml` runs the same build and deploy in CI.

Two repository secrets are required for CI:

- `CLOUDFLARE_API_TOKEN`, a custom token with Account, Workers Scripts, Edit, plus Zone,
  Workers Routes, Edit on `ferryx.dev`.
- `CLOUDFLARE_ACCOUNT_ID`.

An OAuth session from `wrangler login` deploys fine from a laptop but cannot be used in CI.

## The www redirect

`site/worker.js` answers any `www.` hostname with a 301 to the bare domain, preserving the
path. Two details make this the only workable shape:

- Workers Static Assets rejects absolute URLs in a `_redirects` file, so the Pages-style
  `https://www.ferryx.dev/* https://ferryx.dev/:splat 301` cannot be used. The redirect has
  to be code.
- Static assets are served *before* the Worker script by default, so the redirect would
  never run. `assets.run_worker_first` is set to `true` so the Worker sees every request
  and hands non-redirect traffic to `env.ASSETS`.

## No duplicate origins

`workers_dev` and `preview_urls` are both `false`. Left enabled, `ferryx-site.<subdomain>.workers.dev`
would publicly serve the same pages as `ferryx.dev`, splitting ranking signals between two
origins. The Pages project was deleted for the same reason once the Worker took over.

GitHub Pages is disabled. It serves no server-side redirects, so the old `/ferryx/*` URLs
could never have issued a 301 to the new domain; taking the site down at least stops it
competing with the new one.

## Verifying a deploy

```sh
curl -sI https://ferryx.dev/ | head -3
curl -s https://ferryx.dev/robots.txt
curl -s https://ferryx.dev/sitemap-0.xml | grep -c '<loc>'
curl -sI https://www.ferryx.dev/compare/warp/ | grep -i 'http/\|location'
```

The apex returns 200, `robots.txt` carries an absolute `Sitemap:` line on `ferryx.dev`, the
sitemap lists 15 URLs with no `/ferryx` segment, and `www` returns 301 to the apex with the
path preserved.

## Analytics and Search Console

Both are wired but inert until configured, in `site/src/components/SiteAnalytics.astro`.
They render nothing unless the corresponding build-time environment variables are set, so
forks and local builds ship no third-party requests:

- `PUBLIC_GSC_VERIFICATION` emits the `google-site-verification` meta tag. Only needed if
  you verify by HTML tag rather than by DNS. DNS verification is preferable now that the
  domain is on Cloudflare, because it is a single TXT record and survives a host move.
- `PUBLIC_ANALYTICS_SRC` and `PUBLIC_ANALYTICS_DOMAIN` together emit a deferred analytics
  script, shaped for the script-plus-data-domain convention used by privacy-friendly hosts
  such as Plausible and Umami. Set both or neither.

Both are already passed through in the Build static site step of
`.github/workflows/deploy-cloudflare.yml`, sourced from repository secrets. Setting the
secrets is all that is needed to activate them.

Submit `https://ferryx.dev/sitemap-index.xml` in Google Search Console.
