import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";
import tailwindcss from "tailwindcss";
import autoprefixer from "autoprefixer";
import tailwindConfig from "./tailwind.config.js";

export default defineConfig({
  root: path.resolve(import.meta.dirname, "."),
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(import.meta.dirname, "src"),
    },
  },
  css: {
    postcss: {
      plugins: [
        tailwindcss({
          ...tailwindConfig,
          content: [
            path.resolve(import.meta.dirname, "src/**/*.{js,ts,jsx,tsx}"),
            path.resolve(import.meta.dirname, "qa-cont-sidebar*.{js,ts,jsx,tsx,html}"),
          ],
        }),
        autoprefixer(),
      ],
    },
  },
  server: {
    host: "127.0.0.1",
    port: 5211,
    strictPort: true,
    hmr: false,
  },
  cacheDir: ".vite-qa-cont-sidebar",
});
