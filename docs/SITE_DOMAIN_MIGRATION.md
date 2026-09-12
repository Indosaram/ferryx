# Ferryx site hosting and the ferryx.dev domain

The site is published to Cloudflare Pages, project `ferryx`, and served from `ferryx.dev`.
It previously lived at `https://indosaram.github.io/ferryx/`. That project subpath was the
single largest cap on organic search traffic, for two reasons no amount of on-page SEO can
fix:

- **`robots.txt` is origin-scoped.** Crawlers read `https://indosaram.github.io/robots.txt`,
  which belongs to a different repository. The well-formed `robots.txt` generated at
  `/ferryx/robots.txt` was never consulted.
- **Domain authority was shared.** Every signal earned on `indosaram.github.io` is pooled
  across every project hosted there, and none of it accrued to a Ferryx brand domain.

## How the build knows where it lives

`site/astro.config.mjs` reads two environment variables and nothing else decides the URLs:

- `SITE_URL` is the absolute origin, used for `canonical`, `og:url`, `og:image` and the
  `Sitemap:` line in `robots.txt`. Cloudflare builds set it to `https://ferryx.dev`.
- `BASE_URL` is the path prefix. It is **deliberately unset** for Cloudflare, because the
  site is served from the domain root. Setting it would prefix every generated path and
  break the whole site.

Root-origin output is covered by a regression test. `site/src/seo.test.ts` contains
`a root-origin build still emits absolute, non-doubled URLs`, which rebuilds the entire site
with `BASE_URL` unset and `SITE_URL=https://ferryx.dev`, then asserts the canonical,
`og:url`, `og:image` and `robots.txt` values. A second test walks every built page and fails
if any internal link is missing the expected base, which is what catches a half-applied
prefix change.

## Deployment

`.github/workflows/deploy-cloudflare-pages.yml` builds the site and uploads `site/dist` with
`wrangler pages deploy`. This is Direct Upload, not Cloudflare's Git integration: the build
runs in GitHub Actions where the rest of the repository's checks already run, and the build
configuration stays in version control instead of in a dashboard form.

Two repository secrets are required:

- `CLOUDFLARE_API_TOKEN`, a custom token with Account, Cloudflare Pages, Edit.
- `CLOUDFLARE_ACCOUNT_ID`.

A `wrangler login` OAuth session is enough to deploy from a laptop but cannot be used in CI,
and its scopes do not include DNS. Creating or changing DNS records needs a token with
Zone, DNS, Edit on `ferryx.dev`.

## DNS

Both records are proxied `CNAME`s to the Pages project. Cloudflare flattens the apex
`CNAME` automatically, so no `A` records are needed and no registrar IP list has to be kept
up to date:

- `ferryx.dev` CNAME to `ferryx.pages.dev`, proxied.
- `www.ferryx.dev` CNAME to `ferryx.pages.dev`, proxied.

Attaching a custom domain through the Cloudflare dashboard creates these records for you.
Attaching it through the API only creates them when the calling token carries DNS edit
permission; otherwise the domain sits at `pending` with an empty zone and nothing resolves.

`www` redirects to the apex with a 301 from `site/public/_redirects`. Cloudflare Pages reads
that file from the deployed output, which is why the redirect lives in the repository rather
than in a dashboard rule. GitHub Pages had no equivalent, and that gap is why the old host
could not redirect anything.

Note that a `CNAME` file is a GitHub Pages mechanism and has no meaning on Cloudflare. If
one is ever reintroduced for a GitHub Pages deploy, it has to live at `site/public/CNAME`,
because this site publishes the `site/dist` artifact and a repository-root `CNAME` is not
part of it.

## Verifying a deploy

```sh
curl -sI https://ferryx.dev/ | head -3
curl -s https://ferryx.dev/robots.txt
curl -s https://ferryx.dev/sitemap-0.xml | grep -c '<loc>'
curl -sI https://www.ferryx.dev/ | head -3
```

The apex returns 200, `robots.txt` carries an absolute `Sitemap:` line on `ferryx.dev`, the
sitemap lists 15 URLs with no `/ferryx` segment, and `www` returns 301 to the apex.

## The old GitHub Pages site

It is disabled. Because GitHub Pages serves no server-side redirects, the old `/ferryx/*`
URLs could never have issued a 301 to the new domain; taking the site down avoids serving
the same content from two origins, which would have split ranking signals between them.

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
`.github/workflows/deploy-cloudflare-pages.yml`, sourced from repository secrets. Setting
the secrets is all that is needed to activate them.

Submit `https://ferryx.dev/sitemap-index.xml` in Google Search Console once DNS resolves.
