// Vite build config for the F5 copy audit harness. copy-audit.mjs sets F5_UI_ROOT (the ui tree
// under test), F5_OUT_DIR (a throwaway dist) and F5_HTML (the harness page it placed in that tree)
// and runs vite with cwd = F5_UI_ROOT so the tree's own postcss/tailwind config applies.
import path from "node:path";

const uiRoot = path.resolve(process.env.F5_UI_ROOT ?? ".");

export default {
  root: uiRoot,
  esbuild: { jsx: "automatic" },
  define: { __FERRYX_BUILD__: JSON.stringify("qa-supervisor-f5") },
  resolve: {
    alias: { "@": path.resolve(uiRoot, "src") },
    dedupe: ["react", "react-dom"],
  },
  logLevel: "warn",
  build: {
    outDir: path.resolve(process.env.F5_OUT_DIR),
    emptyOutDir: true,
    target: "esnext",
    minify: false,
    sourcemap: false,
    rollupOptions: { input: { harness: path.resolve(uiRoot, process.env.F5_HTML) } },
  },
};
