// Shipped docs, generated before dev/build; no network or CLI binary required.
import { readdir, readFile, mkdir, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, join, relative } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
export function headings(markdown, path) {
  const entries = [];
  let current;
  let fenced = false;
  for (const line of markdown.split(/\r?\n/)) {
    if (/^\s*(```|~~~)/.test(line)) fenced = !fenced;
    const heading = !fenced && /^(#{1,6})\s+(.+?)\s*#*\s*$/.exec(line);
    if (heading) {
      current = { path, title: heading[2], text: "" };
      entries.push(current);
    } else if (current) current.text += `${line}\n`;
  }
  return entries;
}

async function collect(dir) {
  const entries = [];
  for (const item of (await readdir(dir, { withFileTypes: true })).sort((a, b) => a.name.localeCompare(b.name, "en"))) {
    const path = join(dir, item.name);
    if (item.isDirectory()) entries.push(...await collect(path));
    else if (item.name.endsWith(".md")) entries.push(...headings(await readFile(path, "utf8"), relative(root, path)));
  }
  return entries;
}

if (import.meta.main) {
  const publicDir = join(root, "ui/public");
  await mkdir(publicDir, { recursive: true });
  await writeFile(join(publicDir, "search.json"), `${JSON.stringify(await collect(join(root, "docs")), null, 2)}\n`);
  await writeFile(join(publicDir, "llms.txt"), await readFile(join(root, "docs/llms.txt")));
}
