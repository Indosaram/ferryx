import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

import baseTailwindConfig from "../../ui/tailwind.config.js";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const uiRoot = path.resolve(process.env.FERRYX_QA_UI_ROOT || path.resolve(__dirname, "../../ui"));
const extraAllow = process.env.FERRYX_QA_NODE_MODULES ? [path.resolve(process.env.FERRYX_QA_NODE_MODULES)] : [];

const req = createRequire(path.join(uiRoot, "package.json"));
const tailwindcss = req("tailwindcss");
const autoprefixer = req("autoprefixer");

const tauriMockPath = path.resolve(__dirname, "ssh-reboot-recovery-tauri-mock.ts");
const nativeMockPath = path.resolve(__dirname, "ssh-reboot-recovery-native-mock.tsx");
const dagMockPath = path.resolve(__dirname, "ssh-reboot-recovery-dag-mock.tsx");

const qaMocks = {
  name: "ssh-reboot-recovery-boundary-mocks",
  enforce: "pre",
  resolveId(source) {
    if (source === "./NativeTerminalPane" || source.endsWith("/NativeTerminalPane") || source.endsWith("/NativeTerminalPane.tsx")) {
      return nativeMockPath;
    }
    if (source === "./dag/DagPaneBadge" || source.endsWith("/dag/DagPaneBadge") || source.endsWith("/DagPaneBadge") || source.endsWith("/DagPaneBadge.tsx")) {
      return dagMockPath;
    }
    if (source === "./tauri" || source === "../lib/tauri" || source === "@/lib/tauri" || source.endsWith("/lib/tauri") || source.endsWith("/lib/tauri.ts")) {
      return tauriMockPath;
    }
    return null;
  },
};

export default {
  root: __dirname,
  cacheDir: path.resolve(uiRoot, "node_modules/.vite-qa-ssh-reboot-recovery"),
  plugins: [qaMocks],
  css: {
    postcss: {
      plugins: [
        tailwindcss({
          ...baseTailwindConfig,
          content: [
            path.resolve(uiRoot, "index.html"),
            path.resolve(uiRoot, "src/**/*.{js,ts,jsx,tsx}"),
            path.resolve(__dirname, "ssh-reboot-recovery-harness.html"),
            path.resolve(__dirname, "ssh-reboot-recovery-harness.tsx"),
            path.resolve(__dirname, "ssh-reboot-recovery-native-mock.tsx"),
          ],
        }),
        autoprefixer(),
      ],
    },
  },
  optimizeDeps: {
    entries: ["ssh-reboot-recovery-harness.html", "ssh-reboot-recovery-harness.tsx"],
    include: ["react", "react-dom", "react-dom/client", "react/jsx-runtime", "react/jsx-dev-runtime", "lucide-react"],
  },
  esbuild: {
    jsx: "automatic",
  },
  define: {
    __FERRYX_BUILD__: JSON.stringify("qa-ssh-reboot-recovery-harness"),
  },
  resolve: {
    alias: {
      "@": path.resolve(uiRoot, "src"),
    },
    dedupe: ["react", "react-dom"],
  },
  build: {
    outDir: path.resolve(__dirname, "dist"),
    emptyOutDir: true,
    rollupOptions: {
      input: {
        harness: path.resolve(__dirname, "ssh-reboot-recovery-harness.html"),
      },
    },
  },
  server: {
    port: 5188,
    strictPort: true,
    hmr: false,
    fs: {
      allow: [__dirname, uiRoot, ...extraAllow],
    },
  },
};
