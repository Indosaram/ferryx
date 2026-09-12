# Moving the Ferryx site to a custom domain

The site is currently served from `https://indosaram.github.io/ferryx/`. That project
subpath is the single largest cap on organic search traffic, for two reasons that no amount
of on-page SEO can fix:

- **`robots.txt` is origin-scoped.** Crawlers read `https://indosaram.github.io/robots.txt`,
  which belongs to a different repository and currently 404s. The `robots.txt` Ferryx
  generates at `/ferryx/robots.txt` is well-formed but is never consulted by Google.
- **Domain authority is shared.** Every signal earned on `indosaram.github.io` is pooled
  across every project hosted there, and none of it accrues to a Ferryx brand domain.

## The code is already ready

No source change is required. `site/astro.config.mjs` reads `SITE_URL` and `BASE_URL` from
the environment, and `.github/workflows/deploy-pages.yml` feeds them from
`actions/configure-pages@v5` outputs (`origin` and `base_path`). When a custom domain is
configured in repository settings, those two outputs change on their own to the custom
origin and `/`.

The root-origin build is covered by a regression test. `site/src/seo.test.ts` contains
`a root-origin build still emits absolute, non-doubled URLs`, which rebuilds the whole site
with `BASE_URL` unset and `SITE_URL=https://ferryx.dev` and asserts the canonical, `og:url`,
`og:image` and `robots.txt` `Sitemap:` values. That test passes today, so the migration is
already proven at the build level.

## Steps

1. Register the domain. `ferryx.dev` is the name the regression test already assumes.
2. Add DNS records at the registrar. For an apex domain, four `A` records pointing at
   `185.199.108.153`, `185.199.109.153`, `185.199.110.153` and `185.199.111.153`; for a
   `www` subdomain, a `CNAME` to `indosaram.github.io`. Confirm the current addresses in
   GitHub's Pages documentation before relying on them.
3. **Commit `site/public/CNAME` containing only the bare domain**, for example `ferryx.dev`.
   This step is easy to get wrong: setting the custom domain in the repository UI creates a
   `CNAME` file at the repository root, but this site deploys a build artifact from
   `site/dist`, so a root-level `CNAME` is not part of what gets published and the domain
   setting is silently dropped on the next deploy. Putting it in `site/public/` makes Astro
   copy it into `dist/` on every build.
4. Set the custom domain in repository Settings, Pages, then enable Enforce HTTPS once the
   certificate is issued.
5. Redeploy and verify: `curl -sI https://ferryx.dev/` returns 200,
   `curl -s https://ferryx.dev/robots.txt` returns the generated file with an absolute
   `Sitemap:` line, and the sitemap lists `https://ferryx.dev/...` URLs with no `/ferryx`
   segment.
6. Submit the property in Google Search Console and submit `https://ferryx.dev/sitemap-index.xml`.
   Keep the old `indosaram.github.io/ferryx/` property registered as well so you can watch
   the migration.

## What GitHub Pages cannot do for you

GitHub Pages serves no server-side redirects, so the old `/ferryx/*` URLs cannot issue a 301
to the new domain. The practical options are to leave the old path in place and let the
canonical tags on the new domain do the consolidating, or to publish a small client-side
redirect page at the old path. Prefer the canonical route: a JavaScript redirect passes far
less signal than a 301 and can be mistaken for cloaking.

## Analytics and Search Console

Both are wired but inert until configured, in `site/src/components/SiteAnalytics.astro`.
They render nothing unless the corresponding build-time environment variables are set, so
forks and local builds ship no third-party requests:

- `PUBLIC_GSC_VERIFICATION` emits the `google-site-verification` meta tag. Only needed if
  you verify by HTML tag rather than by DNS; DNS verification is preferable because it
  survives a domain move.
- `PUBLIC_ANALYTICS_SRC` and `PUBLIC_ANALYTICS_DOMAIN` together emit a deferred analytics
  script. The pair is shaped for the script-plus-data-domain convention used by
  privacy-friendly hosts such as Plausible and Umami. Set both or neither.

Add them under `env:` in the Build static site step of `.github/workflows/deploy-pages.yml`,
sourced from repository variables or secrets.
