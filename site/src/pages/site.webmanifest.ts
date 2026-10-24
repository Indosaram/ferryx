/// <reference types="astro/client" />
import type { APIRoute } from 'astro';

export const GET: APIRoute = () => {
  const base = import.meta.env.BASE_URL.endsWith('/') ? import.meta.env.BASE_URL : `${import.meta.env.BASE_URL}/`;
  const manifest = {
    name: 'Ferryx',
    short_name: 'Ferryx',
    description: 'Run Claude Code, Codex and any CLI agent side by side in one repo, each in its own worktree.',
    start_url: base,
    scope: base,
    display: 'standalone',
    background_color: '#0b0b0d',
    theme_color: '#0b0b0d',
    icons: [
      { src: `${base}favicon-192.png`, sizes: '192x192', type: 'image/png' },
      { src: `${base}favicon-512.png`, sizes: '512x512', type: 'image/png' },
      { src: `${base}apple-touch-icon.png`, sizes: '180x180', type: 'image/png' },
    ],
  };
  return new Response(JSON.stringify(manifest, null, 2), {
    headers: { 'Content-Type': 'application/manifest+json; charset=utf-8' },
  });
};
