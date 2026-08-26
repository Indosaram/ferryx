import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import react from '@astrojs/react';
import tailwind from '@astrojs/tailwind';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const baseUrl = process.env.BASE_URL
  ? `${process.env.BASE_URL.replace(/\/$/, '')}/`
  : undefined;
// Absolute origin, required so canonical/og:url resolve instead of emitting empty attributes.
const siteOrigin = process.env.SITE_URL ?? 'https://indosaram.github.io';
const socialImage = `${siteOrigin.replace(/\/$/, '')}${baseUrl ?? '/'}og-image.png`;

export default defineConfig({
  ...(baseUrl ? { base: baseUrl } : {}),
  site: siteOrigin,
  server: { port: 14173 },
  integrations: [
    starlight({
      title: 'Ferryx Docs',
      logo: { src: './src/assets/ferryx-icon.png' },
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
      ],
    }),
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
