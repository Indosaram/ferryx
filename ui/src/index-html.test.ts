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
});
