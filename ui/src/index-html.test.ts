import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("ferryx browser document identity", () => {
  it("uses the ferryx title and ferryx favicon", () => {
    const html = readFileSync(resolve(process.cwd(), "index.html"), "utf8");

    expect(html).toContain("<title>Ferryx</title>");
    expect(html).toContain('href="/src/assets/ferryx-icon.png"');
    expect(html).not.toContain("ferryx-icon.svg");
    expect(html).not.toContain("crab-Orca");
  });

  it("uses semantic body theme tokens", () => {
    const html = readFileSync(resolve(process.cwd(), "index.html"), "utf8");

    expect(html).toContain('<body class="bg-background text-foreground antialiased">');
    expect(html).not.toContain("bg-[#09090b]");
    expect(html).not.toContain("text-[#fafafa]");
  });

  it("does not statically import both App and RemoteApp in main.tsx", () => {
    const source = readFileSync(resolve(process.cwd(), "src/main.tsx"), "utf8");
    const hasStaticApp = /^\s*import\s+App\s+from\s+["']\.\/App["'];?/m.test(source);
    const hasStaticRemote = /^\s*import\s+\{\s*RemoteApp\s*\}\s+from\s+["']\.\/remote\/RemoteApp["'];?/m.test(source);
    expect(hasStaticApp && hasStaticRemote).toBe(false);
    expect(source).toContain('import("./App")');
    expect(source).toContain('import("./remote/RemoteApp")');
  });

  // The served CSP is `script-src 'self'`, which blocks inline scripts. The randomUUID
  // polyfill (needed on plain-http LAN origins) must load from a same-origin file first.
  it("has no inline script and loads the uuid polyfill before the app entry", () => {
    const html = readFileSync(resolve(process.cwd(), "index.html"), "utf8");
    const scripts = [...html.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/g)];
    expect(scripts.length).toBeGreaterThan(0);
    for (const [, attributes, body] of scripts) {
      expect(attributes).toMatch(/\bsrc="/);
      expect(body.trim()).toBe("");
    }
    const polyfill = html.indexOf('<script src="/polyfill-uuid.js"></script>');
    const entry = html.indexOf('<script type="module" src="/src/main.tsx"></script>');
    expect(polyfill).toBeGreaterThan(-1);
    expect(entry).toBeGreaterThan(polyfill);
  });

  it("installs crypto.randomUUID where the browser omits it", () => {
    const source = readFileSync(resolve(process.cwd(), "public/polyfill-uuid.js"), "utf8");
    const fakeGlobal: { crypto: { getRandomValues: (array: Uint8Array) => Uint8Array; randomUUID?: () => string } } = {
      crypto: { getRandomValues: (array: Uint8Array) => globalThis.crypto.getRandomValues(array) },
    };
    new Function("globalThis", source)(fakeGlobal);
    const uuid = fakeGlobal.crypto.randomUUID?.();
    expect(uuid).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
    expect(fakeGlobal.crypto.randomUUID?.()).not.toBe(uuid);
  });
});
