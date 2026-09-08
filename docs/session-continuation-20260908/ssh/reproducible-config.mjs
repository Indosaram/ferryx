import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  cacheDir: path.resolve(__dirname, "node_modules/.vite-qa-ssh-autocomplete"),
  server: { host: "127.0.0.1", port: 5213, strictPort: true, hmr: false },
  optimizeDeps: { entries: ["qa-ssh-autocomplete.html"] },
});
