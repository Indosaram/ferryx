import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { Agent as HttpsAgent } from "node:https";
import path from "node:path";

const DEV_HOST = "127.0.0.1";
const DEV_PORT = 5173;

/**
 * Dev-only: the Vite SPA fallback answers every unknown path with 200 text/html, so the
 * account-origin probe would otherwise pick the dev origin and post login to Vite. Proxy only
 * the relay-owned account + attach routes to the real relay; the page origin stays the token
 * issuer, and every other /api path is untouched.
 */
const DEV_RELAY_TARGET = "https://relay.ferryx.dev";
/**
 * Relay-only outbound agent pinned to IPv4: from this dev host the relay's AAAA addresses fail
 * with EHOSTUNREACH while IPv4 answers. Scoped to the relay proxies (http + ws); no global DNS
 * or resolver change, no retries.
 */
const devRelayAgent = new HttpsAgent({ family: 4, keepAlive: true });

type ProxyErr = Error & { code?: string; address?: string; port?: number; errors?: ProxyErr[] };
type DevProxyRes = {
  statusCode?: number;
  headers?: Record<string, string | string[] | undefined>;
  on(event: string, cb: (...args: any[]) => void): void;
};

/** Log each failed connect attempt (family/address/code) next to Vite's own proxy error line. */
function logRelayProxyErrors(proxy: {
  on(event: "error", cb: (err: ProxyErr) => void): void;
  on(event: "proxyReq", cb: (proxyReq: { setHeader(name: string, value: string): void }, req: { method?: string; url?: string }) => void): void;
  on(event: "proxyRes", cb: (res: DevProxyRes, req: { method?: string; url?: string }) => void): void;
}) {
  proxy.on("error", (err) => {
    const attempts = (err.errors ?? [err]).map((e) => `${e.code ?? e.name} ${e.address ?? "?"}:${e.port ?? "?"}`);
    console.error(`[dev-relay-proxy] ${err.code ?? err.name} via ${DEV_RELAY_TARGET}: ${attempts.join(", ")}`);
  });
  // Ask the relay for an uncompressed machine list so the summary below can read a copy.
  proxy.on("proxyReq", (proxyReq, req) => {
    if (req.method === "GET" && (req.url ?? "").split("?")[0] === "/api/account/v1/machines") {
      proxyReq.setHeader("accept-encoding", "identity");
    }
  });
  // Dev-only diagnostics: method, path (query stripped), status. The machine list additionally
  // logs an 8-char machineId prefix + online flag from a copy of the body; the response itself
  // is untouched and no tokens, headers, or other bodies are logged.
  proxy.on("proxyRes", (res, req) => {
    const pathOnly = (req.url ?? "").split("?")[0];
    console.log(`[dev-relay-proxy] ${req.method ?? "?"} ${pathOnly} -> ${res.statusCode ?? "?"}`);
    if (req.method !== "GET" || pathOnly !== "/api/account/v1/machines") return;
    const chunks: Buffer[] = [];
    res.on("data", (chunk: Buffer) => chunks.push(chunk));
    res.on("end", () => {
      try {
        const decoded = Buffer.concat(chunks);
        const raw = JSON.parse(decoded.toString("utf8"));
        const list: unknown[] = Array.isArray(raw) ? raw : Array.isArray(raw?.machines) ? raw.machines : [];
        const summary = list.map((m) => {
          const row = m as { machineId?: unknown; online?: unknown };
          return `${String(row.machineId ?? "?").slice(0, 8)}:${row.online === false ? "offline" : "online"}`;
        });
        console.log(`[dev-relay-proxy] machines [${summary.join(", ")}]`);
      } catch (err) {
        const type = String(res.headers?.["content-type"] ?? "?");
        const enc = String(res.headers?.["content-encoding"] ?? "none");
        const bytes = chunks.reduce((n, c) => n + c.length, 0);
        console.log(`[dev-relay-proxy] machines <unparsed body: ${(err as Error).name} type=${type} enc=${enc} bytes=${bytes}>`);
      }
    });
  });
}

const devRelayProxy = {
  target: DEV_RELAY_TARGET,
  changeOrigin: true,
  secure: true,
  agent: devRelayAgent,
  configure: logRelayProxyErrors,
};

/**
 * Build identity stamped into the bundle: version, source revision (dirty-marked), build clock.
 * A phone that shows an older stamp than the host serves is running a stale client.
 */
function buildStamp(): string {
  const version = JSON.parse(readFileSync(path.resolve(__dirname, "package.json"), "utf8")).version as string;
  let revision = "unknown";
  try {
    const sha = execFileSync("git", ["rev-parse", "--short", "HEAD"], { cwd: __dirname }).toString().trim();
    const dirty = execFileSync("git", ["status", "--porcelain"], { cwd: __dirname }).toString().trim().length > 0;
    revision = `${sha}${dirty ? "+dirty" : ""}`;
  } catch {
    revision = "unknown";
  }
  const builtAt = new Date().toISOString().slice(0, 16).replace("T", " ");
  return `${version} ${revision} ${builtAt}`;
}

export default defineConfig({
  plugins: [react()],
  define: {
    __FERRYX_BUILD__: JSON.stringify(buildStamp()),
  },
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
    // No hard-coded host/clientPort: the HMR client follows the port the server actually bound.
    hmr: {
      protocol: "ws",
    },
    proxy: {
      "/api/account/v1": devRelayProxy,
      "/api/v1/attach/session": devRelayProxy,
      "/tunnel/opaque": { ...devRelayProxy, ws: true },
    },
    watch: {
      usePolling: true,
      interval: 100,
      ignored: ["**/src-tauri/**", "**/target/**", "**/.git/**"],
    },
  },
});
