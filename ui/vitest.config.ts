import react from "@vitejs/plugin-react";
import path from "node:path";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    clearMocks: true,
    restoreMocks: true,
    fileParallelism: false,
    // The input-latency soak harness is an opt-in Bun measurement, run with
    // `bun test ./ui/src/remote/input-latency-soak/soak.test.mjs`. It imports
    // `bun:test`, which Vite externalizes for the jsdom/browser environment,
    // so sweeping it into this suite fails the file on every run.
    exclude: [
      "**/node_modules/**",
      "**/dist/**",
      "src/remote/input-latency-soak/**",
    ],
    coverage: {
      provider: "v8",
      reporter: ["text", "json-summary"],
      reportsDirectory: "coverage",
      include: ["src/**/*.{ts,tsx}"],
      exclude: [
        "src/**/*.test.{ts,tsx}",
        "src/**/*.bench.ts",
        "src/**/*.check.ts",
        "src/test/**",
      ],
    },
  },
});
