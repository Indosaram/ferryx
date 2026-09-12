import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import sitemap from '@astrojs/sitemap';
import react from '@astrojs/react';
import tailwind from '@astrojs/tailwind';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const baseUrl = process.env.BASE_URL
  ? `${process.env.BASE_URL.replace(/\/$/, '')}/`
  : undefined;
// Absolute origin, required so canonical/og:url resolve instead of emitting empty attributes.
const siteOrigin = process.env.SITE_URL ?? 'https://indosaram.github.io';
const socialImage = `${siteOrigin.replace(/\/$/, '')}${baseUrl ?? '/'}og-image.png`;

// Astro does not rewrite root-relative links written inside markdown, so `/compare/warp/`
// would 404 on a base-path deployment. Prefixing here keeps the prose portable: the same
// source builds correctly under /ferryx and at a domain root.
function rehypeBasePath() {
  const base = (baseUrl ?? '/').replace(/\/$/, '');
  return (tree) => {
    const walk = (node) => {
      if (node.tagName === 'a') {
        const href = node.properties?.href;
        if (typeof href === 'string' && href.startsWith('/') && !href.startsWith('//')) {
          if (base && !href.startsWith(`${base}/`)) node.properties.href = `${base}${href}`;
        }
      }
      for (const child of node.children ?? []) walk(child);
    };
    walk(tree);
  };
}

export default defineConfig({
  ...(baseUrl ? { base: baseUrl } : {}),
  markdown: { rehypePlugins: [rehypeBasePath] },
  site: siteOrigin,
  server: { port: 14173 },
  integrations: [
    starlight({
      title: 'Ferryx Docs',
      logo: { src: './src/assets/ferryx-icon.png' },
      favicon: '/favicon.ico',
      disable404Route: true,
      components: { Head: './src/components/StarlightHead.astro' },
      head: [
        { tag: 'meta', attrs: { property: 'og:image', content: socialImage } },
        { tag: 'meta', attrs: { property: 'og:image:width', content: '1200' } },
        { tag: 'meta', attrs: { property: 'og:image:height', content: '630' } },
        { tag: 'meta', attrs: { property: 'og:image:type', content: 'image/png' } },
        {
          tag: 'meta',
          attrs: { property: 'og:image:alt', content: 'Ferryx — parallel agentic development workspace' },
        },
        { tag: 'meta', attrs: { name: 'twitter:image', content: socialImage } },
      ],
      social: { github: 'https://github.com/Indosaram/ferryx' },
      sidebar: [
        {
          label: 'Getting Started',
          items: [{ label: 'Introduction', slug: 'docs/introduction' }],
        },
        {
          label: 'Reference',
          items: [{ label: 'Keyboard Shortcuts', slug: 'docs/shortcuts' }],
        },
        {
          label: 'Architecture',
          items: [{ label: 'Technical Architecture', slug: 'docs/architecture' }],
        },
        {
          label: 'Use cases',
          items: [
            { label: 'Running agents in parallel', slug: 'use-cases/parallel-ai-agents' },
            { label: 'Git worktree workflow', slug: 'use-cases/git-worktree-workflow' },
            { label: 'Remote terminal access', slug: 'use-cases/remote-terminal-access' },
          ],
        },
        {
          label: 'Compare',
          items: [
            { label: 'All comparisons', slug: 'compare' },
            { label: 'Ferryx vs Warp', slug: 'compare/warp' },
            { label: 'Ferryx vs Wave Terminal', slug: 'compare/wave-terminal' },
            { label: 'Ferryx vs Conductor', slug: 'compare/conductor' },
            { label: 'Ferryx vs Crystal', slug: 'compare/crystal' },
            { label: 'Ferryx vs tmux + worktree', slug: 'compare/tmux-git-worktree' },
            { label: 'Ferryx and Ghostty', slug: 'compare/ghostty' },
          ],
        },
      ],
    }),
    sitemap({ lastmod: new Date(), changefreq: 'weekly', priority: 0.7 }),
    react(),
    tailwind({ applyBaseStyles: false }),
  ],
  vite: {
    resolve: {
      alias: {
        '@tauri-apps/api/core': path.resolve(__dirname, './src/mock/core.ts'),
        '@tauri-apps/api/event': path.resolve(__dirname, './src/mock/events.ts'),
        '@tauri-apps/plugin-dialog': path.resolve(__dirname, './src/mock/dialog.ts'),
        '@': path.resolve(__dirname, './src'),
        '@ui': path.resolve(__dirname, '../ui/src'),
      },
    },
    server: {
      // Native fsevents watcher silently misses edits in this repo; poll instead.
      watch: { usePolling: true, interval: 300 },
    },
  },
});
