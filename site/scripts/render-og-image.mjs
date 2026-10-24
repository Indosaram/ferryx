// Renders site/scripts/og-image.html to site/public/og-image.png (1200x630).
// Usage (from worktree root): bun site/scripts/render-og-image.mjs
import { fileURLToPath, pathToFileURL } from "node:url";
import { dirname, resolve } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const htmlPath = resolve(here, "og-image.html");
const outPath = resolve(here, "../public/og-image.png");

await using view = new Bun.WebView({ width: 1200, height: 630 });
await view.navigate(pathToFileURL(htmlPath).href);

const state = await view.evaluate(`(async () => {
  await document.fonts.ready;
  await Promise.all([...document.images].map((img) =>
    img.complete ? null : new Promise((r) => { img.onload = img.onerror = r; })));
  return {
    inter: document.fonts.check('600 76px "Inter"'),
    mono: document.fonts.check('18px "JetBrains Mono"'),
    icon: [...document.images].every((img) => img.naturalWidth > 0),
    overflowX: document.documentElement.scrollWidth > 1200,
    overflowY: document.documentElement.scrollHeight > 630,
  };
})()`);

if (!state.inter || !state.mono || !state.icon) {
  throw new Error(`OG render assets not ready: ${JSON.stringify(state)}`);
}
if (state.overflowX || state.overflowY) {
  throw new Error(`OG layout overflows the 1200x630 frame: ${JSON.stringify(state)}`);
}

// WebKit captures at the device pixel ratio (2x on Retina); normalize to 1200x630.
const shot = await view.screenshot();
const png = await new Bun.Image(await shot.arrayBuffer())
  .resize(1200, 630, { fit: "fill" })
  .png({ palette: true, compressionLevel: 9 })
  .bytes();
await Bun.write(outPath, png);
console.log(`wrote ${outPath} (${png.byteLength} bytes)`, state);
