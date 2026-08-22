import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import react from '@astrojs/react';
import tailwind from '@astrojs/tailwind';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  server: { port: 14173 },
  integrations: [
    starlight({
      title: 'Ferryx Docs',
      logo: { src: './src/assets/ferryx-icon.png' },
      social: { github: 'https://github.com/ferryx/ferryx' },
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
