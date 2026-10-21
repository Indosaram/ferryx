import { mergeConfig } from "vite";
import base from "./vite.config";

// Bundle the existing remote UI locally, not a mutable remote webpage.
// The explicit build flag keeps Tauri's native shell from selecting Desktop App.
export default mergeConfig(base, {
  define: { "import.meta.env.VITE_FERRYX_MOBILE": JSON.stringify("1") },
  build: { outDir: "dist-mobile" },
  server: {
    host: "0.0.0.0",
    hmr: { host: process.env.TAURI_DEV_HOST ?? "localhost", clientPort: 5173 },
  },
});
