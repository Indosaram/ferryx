import { describe, expect, test, beforeAll } from "bun:test";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import path from "node:path";

// Assertions run against the production build output, the only artifact a crawler
// sees. The suite builds the site itself so CI's `bun test --cwd site` can never
// pass against a stale dist/.

const SITE_ROOT = path.resolve(import.meta.dir, "..");
const DIST = path.join(SITE_ROOT, "dist");
const PUBLIC_DIR = path.join(SITE_ROOT, "public");
const BASE_PATH = "/ferryx/";
const ORIGIN = "https://indosaram.github.io";
const SITE_URL = `${ORIGIN}${BASE_PATH}`;

const BUILD_TIMEOUT_MS = 300_000;

async function buildSite(env: Record<string, string>, extraArgs: string[] = []) {
  const proc = Bun.spawn(["bun", "run", "build", ...extraArgs], {
    cwd: SITE_ROOT,
    env: { ...process.env, ...env },
    stdout: "pipe",
    stderr: "pipe",
  });
  const [code, stdout, stderr] = await Promise.all([
    proc.exited,
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
  ]);
  return { code, output: `${stdout}\n${stderr}` };
}

beforeAll(async () => {
  const { code, output } = await buildSite({
    BASE_URL: "/ferryx",
    SITE_URL: ORIGIN,
  });
  if (code !== 0) throw new Error(`astro build failed (exit ${code}):\n${output}`);
}, BUILD_TIMEOUT_MS);

function builtPages(): Record<string, string> {
  const pages: Record<string, string> = {};
  const walk = (dir: string) => {
    for (const entry of readdirSync(dir)) {
      const full = path.join(dir, entry);
      if (statSync(full).isDirectory()) {
        if (entry === "_astro" || entry === "pagefind") continue;
        walk(full);
        continue;
      }
      if (!entry.endsWith(".html")) continue;
      pages[full.slice(DIST.length) || "/"] = readFileSync(full, "utf8");
    }
  };
  walk(DIST);
  return pages;
}

const DOC_PAGES = [
  "/docs/introduction/index.html",
  "/docs/shortcuts/index.html",
  "/docs/architecture/index.html",
  "/privacy/index.html",
];

function distFileFor(href: string): string | null {
  let pathname = href;
  if (/^https?:\/\//.test(href)) {
    const url = new URL(href);
    if (url.origin !== ORIGIN) return null;
    pathname = url.pathname;
  }
  if (!pathname.startsWith(BASE_PATH)) return null;
  const rel = pathname.slice(BASE_PATH.length);
  const candidate = path.join(DIST, rel);
  if (existsSync(candidate) && statSync(candidate).isFile()) return candidate;
  const indexed = path.join(candidate, "index.html");
  return existsSync(indexed) ? indexed : null;
}

function tags(html: string, tagName: string): string[] {
  return [...html.matchAll(new RegExp(`<${tagName}\\b[^>]*>`, "gi"))].map((m) => m[0]);
}

function attr(tag: string, name: string): string | null {
  const match = tag.match(new RegExp(`\\b${name}=("([^"]*)"|'([^']*)')`, "i"));
  if (!match) return null;
  return match[2] ?? match[3] ?? "";
}

function metaContent(html: string, selector: { name?: string; property?: string }): string | null {
  for (const tag of tags(html, "meta")) {
    if (selector.name && attr(tag, "name")?.toLowerCase() === selector.name) return attr(tag, "content");
    if (selector.property && attr(tag, "property")?.toLowerCase() === selector.property) return attr(tag, "content");
  }
  return null;
}

function linksWithRel(html: string, rel: string): string[] {
  return tags(html, "link").filter((tag) => {
    const value = attr(tag, "rel")?.toLowerCase() ?? "";
    return value.split(/\s+/).includes(rel);
  });
}

function jsonLdBlocks(html: string): unknown[] {
  const blocks = [...html.matchAll(/<script[^>]*type="application\/ld\+json"[^>]*>([\s\S]*?)<\/script>/gi)];
  return blocks.map(([, body]) => JSON.parse(body!));
}

function jsonLdNodes(html: string): Record<string, unknown>[] {
  const out: Record<string, unknown>[] = [];
  const push = (value: unknown) => {
    if (Array.isArray(value)) {
      value.forEach(push);
      return;
    }
    if (value && typeof value === "object") {
      const node = value as Record<string, unknown>;
      out.push(node);
      if (node["@graph"]) push(node["@graph"]);
    }
  };
  jsonLdBlocks(html).forEach(push);
  return out;
}

function nodesOfType(html: string, type: string): Record<string, unknown>[] {
  return jsonLdNodes(html).filter((node) => {
    const t = node["@type"];
    return Array.isArray(t) ? t.includes(type) : t === type;
  });
}

function pngSize(file: string): { width: number; height: number } {
  const buf = readFileSync(file);
  if (buf.subarray(1, 4).toString("ascii") !== "PNG") throw new Error(`${file} is not a PNG`);
  // PNG IHDR chunk: 8-byte signature + 4-byte length + 4-byte type, then width/height as BE u32.
  return { width: buf.readUInt32BE(16), height: buf.readUInt32BE(20) };
}

describe("SEO — crawl and indexation", () => {
  test("robots.txt ships with an absolute Sitemap directive", () => {
    const robots = path.join(DIST, "robots.txt");
    expect(existsSync(robots)).toBe(true);
    const body = readFileSync(robots, "utf8");
    expect(body).toContain("User-agent: *");
    expect(body).toContain(`Sitemap: ${SITE_URL}sitemap-index.xml`);
  });

  test("every page advertises the sitemap and the sitemap resolves", () => {
    for (const [route, html] of Object.entries(builtPages())) {
      const links = linksWithRel(html, "sitemap");
      expect(links.length, `${route} has no <link rel="sitemap">`).toBeGreaterThan(0);
      const href = attr(links[0]!, "href");
      expect(href, `${route} sitemap link has no href`).toBeTruthy();
      expect(distFileFor(href!), `${route} sitemap href ${href} does not resolve in dist`).not.toBeNull();
    }
  });

  test("sitemap entries carry lastmod so crawlers can schedule recrawls", () => {
    const sitemap = readFileSync(path.join(DIST, "sitemap-0.xml"), "utf8");
    const locs = [...sitemap.matchAll(/<loc>/g)].length;
    const lastmods = [...sitemap.matchAll(/<lastmod>/g)].length;
    expect(locs).toBeGreaterThan(0);
    expect(lastmods).toBe(locs);
  });

  test("the 404 page is noindex and still describes itself", () => {
    const html = readFileSync(path.join(DIST, "404.html"), "utf8");
    expect(metaContent(html, { name: "robots" })).toContain("noindex");
    expect((metaContent(html, { name: "description" }) ?? "").length).toBeGreaterThan(20);
  });
});

describe("SEO — structured data", () => {
  test("every JSON-LD block on every page is valid JSON", () => {
    for (const [route, html] of Object.entries(builtPages())) {
      expect(() => jsonLdBlocks(html), `${route} has malformed JSON-LD`).not.toThrow();
    }
  });

  test("landing page declares SoftwareApplication with the fields Google requires", () => {
    const html = readFileSync(path.join(DIST, "index.html"), "utf8");
    const apps = nodesOfType(html, "SoftwareApplication");
    expect(apps.length).toBe(1);
    const app = apps[0]!;
    expect(app.name).toBe("Ferryx");
    expect(app.applicationCategory).toBeTruthy();
    expect(Array.isArray(app.operatingSystem) ? app.operatingSystem : [app.operatingSystem]).toContain("macOS");
    expect(app.url).toBe(SITE_URL);
    const offers = app.offers as Record<string, unknown> | undefined;
    expect(offers?.["@type"]).toBe("Offer");
    expect(String(offers?.price)).toBe("0");
    expect(offers?.priceCurrency).toBe("USD");
  });

  test("landing page declares WebSite and Organization identity", () => {
    const html = readFileSync(path.join(DIST, "index.html"), "utf8");
    const sites = nodesOfType(html, "WebSite");
    expect(sites.length).toBe(1);
    expect(sites[0]!.url).toBe(SITE_URL);
    expect(sites[0]!.name).toBe("Ferryx");

    const orgs = nodesOfType(html, "Organization");
    expect(orgs.length).toBe(1);
    expect(orgs[0]!.name).toBeTruthy();
    expect(String(orgs[0]!.url ?? "")).toMatch(/^https:\/\//);
  });

  test("docs and policy pages expose an ordered BreadcrumbList", () => {
    for (const route of DOC_PAGES) {
      const html = readFileSync(path.join(DIST, route), "utf8");
      const crumbs = nodesOfType(html, "BreadcrumbList");
      expect(crumbs.length, `${route} has no BreadcrumbList`).toBe(1);
      const items = crumbs[0]!.itemListElement as Record<string, unknown>[];
      expect(Array.isArray(items)).toBe(true);
      expect(items.length, `${route} breadcrumb is too shallow`).toBeGreaterThanOrEqual(2);
      items.forEach((item, index) => {
        expect(item.position, `${route} breadcrumb position ${index}`).toBe(index + 1);
        expect(item.name, `${route} breadcrumb ${index} has no name`).toBeTruthy();
        expect(String(item.item ?? ""), `${route} breadcrumb ${index} has no absolute item URL`).toMatch(
          new RegExp(`^${ORIGIN}`),
        );
      });
    }
  });
});

describe("SEO — head metadata and icons", () => {
  test("landing description fits inside the SERP snippet budget", () => {
    const html = readFileSync(path.join(DIST, "index.html"), "utf8");
    const description = metaContent(html, { name: "description" }) ?? "";
    expect(description.length).toBeGreaterThan(70);
    expect(description.length).toBeLessThanOrEqual(160);
  });

  test("every page links an apple-touch-icon that exists at 180x180", () => {
    for (const [route, html] of Object.entries(builtPages())) {
      const links = linksWithRel(html, "apple-touch-icon");
      expect(links.length, `${route} has no apple-touch-icon`).toBeGreaterThan(0);
      const file = distFileFor(attr(links[0]!, "href") ?? "");
      expect(file, `${route} apple-touch-icon does not resolve in dist`).not.toBeNull();
      expect(pngSize(file!)).toEqual({ width: 180, height: 180 });
    }
  });

  test("every page links a valid web app manifest", () => {
    for (const [route, html] of Object.entries(builtPages())) {
      const links = linksWithRel(html, "manifest");
      expect(links.length, `${route} has no manifest link`).toBeGreaterThan(0);
      const file = distFileFor(attr(links[0]!, "href") ?? "");
      expect(file, `${route} manifest does not resolve in dist`).not.toBeNull();
      const manifest = JSON.parse(readFileSync(file!, "utf8"));
      expect(manifest.name).toBeTruthy();
      expect(manifest.short_name).toBeTruthy();
      expect(Array.isArray(manifest.icons) && manifest.icons.length).toBeTruthy();
      for (const icon of manifest.icons) {
        expect(distFileFor(icon.src), `manifest icon ${icon.src} missing from dist`).not.toBeNull();
      }
    }
  });

  test("no page makes the browser download an oversized favicon", () => {
    for (const [route, html] of Object.entries(builtPages())) {
      for (const tag of tags(html, "link")) {
        const rel = (attr(tag, "rel") ?? "").toLowerCase();
        if (!/(^|\s)(icon|shortcut icon|alternate icon)(\s|$)/.test(rel)) continue;
        if (rel.includes("apple-touch-icon")) continue;
        const file = distFileFor(attr(tag, "href") ?? "");
        expect(file, `${route} icon ${attr(tag, "href")} does not resolve`).not.toBeNull();
        const bytes = statSync(file!).size;
        expect(bytes, `${route} ships a ${bytes} byte favicon (${attr(tag, "href")})`).toBeLessThan(100_000);
      }
    }
  });

  test("docs pages carry explicit Twitter card text and a theme colour", () => {
    for (const route of DOC_PAGES) {
      const html = readFileSync(path.join(DIST, route), "utf8");
      expect((metaContent(html, { name: "twitter:title" }) ?? "").length, `${route} twitter:title`).toBeGreaterThan(0);
      expect(
        (metaContent(html, { name: "twitter:description" }) ?? "").length,
        `${route} twitter:description`,
      ).toBeGreaterThan(0);
      expect((metaContent(html, { name: "theme-color" }) ?? "").length, `${route} theme-color`).toBeGreaterThan(0);
    }
  });
});

describe("SEO — Core Web Vitals signals", () => {
  test("landing images reserve their layout box", () => {
    const html = readFileSync(path.join(DIST, "index.html"), "utf8");
    const imgs = tags(html, "img");
    expect(imgs.length).toBeGreaterThan(0);
    for (const img of imgs) {
      expect(attr(img, "width"), `img ${attr(img, "src")} has no width`).toBeTruthy();
      expect(attr(img, "height"), `img ${attr(img, "src")} has no height`).toBeTruthy();
    }
  });

  test("no third-party stylesheet blocks the landing page's first paint", () => {
    const html = readFileSync(path.join(DIST, "index.html"), "utf8");
    for (const tag of linksWithRel(html, "stylesheet")) {
      const href = attr(tag, "href") ?? "";
      if (!/^https?:\/\//.test(href)) continue;
      expect(
        attr(tag, "media"),
        `${href} is a render-blocking third-party stylesheet`,
      ).toBe("print");
    }
  });

  test("below-the-fold islands defer hydration", () => {
    const html = readFileSync(path.join(DIST, "index.html"), "utf8");
    for (const island of tags(html, "astro-island")) {
      const componentUrl = attr(island, "component-url") ?? "";
      const isBelowFold = /\/(Features|Benchmarks|Footer)\./.test(componentUrl);
      if (!isBelowFold) continue;
      expect(attr(island, "client"), `${componentUrl} still hydrates eagerly`).not.toBe("load");
    }
  });

  test("the landing page ships no oversized public image", () => {
    for (const entry of readdirSync(PUBLIC_DIR)) {
      if (!/\.(png|jpe?g)$/i.test(entry)) continue;
      const bytes = statSync(path.join(PUBLIC_DIR, entry)).size;
      expect(bytes, `public/${entry} is ${bytes} bytes`).toBeLessThan(400_000);
    }
  });
});

describe("SEO — crawlable content and internal linking", () => {
  test("the preview section server-renders a heading and descriptive copy", () => {
    const html = readFileSync(path.join(DIST, "index.html"), "utf8");
    const section = html.match(/<section id="preview"[\s\S]*?<\/section>/);
    expect(section, "no #preview section in the built HTML").not.toBeNull();
    const markup = section![0];
    expect(markup, "#preview has no server-rendered <h2>").toMatch(/<h2[\s>]/);
    const paragraphs = [...markup.matchAll(/<p\b[^>]*>([\s\S]*?)<\/p>/g)].map(([, body]) =>
      body!.replace(/<[^>]+>/g, "").trim(),
    );
    const longest = paragraphs.reduce((max, text) => Math.max(max, text.length), 0);
    expect(longest, "#preview has no server-rendered descriptive paragraph").toBeGreaterThanOrEqual(80);
  });

  test("the architecture page is not orphaned", () => {
    const landing = readFileSync(path.join(DIST, "index.html"), "utf8");
    const landingLinks = tags(landing, "a").map((tag) => attr(tag, "href") ?? "");
    expect(
      landingLinks.some((href) => href.includes("docs/architecture")),
      "landing page does not link to /docs/architecture/",
    ).toBe(true);

    for (const route of ["/docs/introduction/index.html", "/docs/shortcuts/index.html"]) {
      const html = readFileSync(path.join(DIST, route), "utf8");
      expect(html.includes("docs/architecture"), `${route} sidebar omits the architecture page`).toBe(true);
    }
  });

  test("every internal link in the landing page resolves to a built page", () => {
    const html = readFileSync(path.join(DIST, "index.html"), "utf8");
    for (const tag of tags(html, "a")) {
      const href = attr(tag, "href") ?? "";
      if (!href.startsWith(BASE_PATH)) continue;
      const [pathname] = href.split("#");
      expect(distFileFor(pathname!), `landing link ${href} is broken`).not.toBeNull();
    }
  });
});

describe("SEO — deployment portability", () => {
  test(
    "a root-origin build still emits absolute, non-doubled URLs",
    async () => {
      const outDir = path.join(SITE_ROOT, "node_modules/.cache/seo-root-dist");
      const { code, output } = await buildSite(
        { BASE_URL: "", SITE_URL: "https://ferryx.dev" },
        ["--outDir", outDir],
      );
      expect(code, `root-origin build failed:\n${output}`).toBe(0);

      const html = readFileSync(path.join(outDir, "index.html"), "utf8");
      expect(linksWithRel(html, "canonical").map((tag) => attr(tag, "href"))).toEqual(["https://ferryx.dev/"]);
      expect(metaContent(html, { property: "og:url" })).toBe("https://ferryx.dev/");
      expect(metaContent(html, { property: "og:image" })).toBe("https://ferryx.dev/og-image.png");

      const robots = readFileSync(path.join(outDir, "robots.txt"), "utf8");
      expect(robots).toContain("Sitemap: https://ferryx.dev/sitemap-index.xml");
    },
    BUILD_TIMEOUT_MS,
  );
});
