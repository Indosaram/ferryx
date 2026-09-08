import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";
import { createRequire } from "node:module";

const projectRoot = "/Users/indo/code/project/orca-lite";
const uiRoot = path.join(projectRoot, "ui");
const here = path.join(projectRoot, "docs/session-continuation-20260908/integration");

const require = createRequire(path.join(uiRoot, "package.json"));
const tailwindcss = require("tailwindcss");
const autoprefixer = require("autoprefixer");

export default defineConfig({
  root: here,
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.join(uiRoot, "src"),
      "react": path.join(uiRoot, "node_modules/react"),
      "react-dom": path.join(uiRoot, "node_modules/react-dom"),
    },
    dedupe: ["react", "react-dom"],
  },
  css: {
    postcss: {
      plugins: [
        tailwindcss({
          configFile: path.join(uiRoot, "tailwind.config.js"),
          content: [
            path.join(uiRoot, "src/**/*.{js,ts,jsx,tsx}"),
            path.join(here, "*.{js,ts,jsx,tsx,html}"),
          ],
        }),
        autoprefixer(),
      ],
    },
  },
  server: {
    host: "127.0.0.1",
    port: 5214,
    strictPort: true,
    hmr: false,
    fs: {
      allow: [projectRoot],
    },
  },
  cacheDir: path.join(here, ".vite-qa-integration"),
});
