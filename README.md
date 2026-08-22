# Ferryx

Ferryx is a native workspace for terminal-based development and AI-assisted workflows.

## Web and documentation

The public web and documentation site is built with [Astro](https://astro.build/), [Starlight](https://starlight.astro.build/), and [React](https://react.dev/) under [`site/`](./site/).

When running locally:

- Landing page: <http://localhost:14173/>
- Documentation: <http://localhost:14173/docs/introduction/>
- Shortcut reference: <http://localhost:14173/docs/shortcuts/>

## Development

```bash
git clone https://github.com/ferryx/ferryx.git
cd ferryx
bun install
bun run --cwd site dev
```

Build the site with:

```bash
bun run --cwd site build
```

## Deployment

GitHub Pages deployment is managed by [`.github/workflows/deploy-pages.yml`](./.github/workflows/deploy-pages.yml). It runs automatically for changes to the site or workflow on `main`. Before the first deployment, set **Repository Settings > Pages > Build and deployment > Source** to **GitHub Actions**.

## Repository

- Repository: <https://github.com/ferryx/ferryx>
- Site source: [`site/`](./site/)
- Documentation content: [`site/src/content/docs/docs/`](./site/src/content/docs/docs/)

## License

Distributed under the MIT and Apache 2.0 dual licenses. See [`LICENSE`](./LICENSE).
