import path from "node:path";
import { fileURLToPath } from "node:url";

import react from "@vitejs/plugin-react";
import autoprefixer from "autoprefixer";
import tailwindcss from "tailwindcss";
import { defineConfig } from "vite";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const UI = path.resolve(HERE, "../../../../../ui");

// `./node_modules` here is a symlink to `ui/node_modules` created by drive.mjs and
// removed again on exit, so this harness adds no second dependency tree under docs/.
const uiTailwindConfig = (await import(path.join(UI, "tailwind.config.js"))).default;

export default defineConfig({
  root: HERE,
  plugins: [react()],
  resolve: {
    alias: {
      "@ui": path.join(UI, "src"),
      "@": path.join(UI, "src"),
    },
  },
  css: {
    postcss: {
      plugins: [
        tailwindcss({
          ...uiTailwindConfig,
          content: [path.join(HERE, "*.html"), path.join(HERE, "*.tsx"), path.join(UI, "src/**/*.{ts,tsx}")],
        }),
        autoprefixer(),
      ],
    },
  },
  server: { host: "127.0.0.1", port: 0 },
});
