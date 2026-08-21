import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";

const DEV_HOST = "127.0.0.1";
const DEV_PORT = 5173;

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  clearScreen: false,
  server: {
    host: DEV_HOST,
    port: DEV_PORT,
    strictPort: true,
    hmr: {
      protocol: "ws",
      host: DEV_HOST,
      clientPort: DEV_PORT,
    },
    watch: {
      usePolling: true,
      interval: 100,
      ignored: ["**/src-tauri/**", "**/target/**", "**/.git/**"],
    },
  },
});
