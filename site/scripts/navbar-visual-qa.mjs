// Navbar responsive QA: drives the locally installed Google Chrome against a built site and
// reports navbar geometry, overlap, and download-menu reachability.
//
// Usage: bun run site/scripts/navbar-visual-qa.mjs <distDir> <evidenceDir> [label]
//
// Browser: Playwright's `chrome` channel (the Google Chrome already on this machine).
// No browser download happens here.
import path from "node:path";
import { measureNavbar } from "./navbar-probe.mjs";

const SITE_ROOT = path.resolve(import.meta.dir ?? path.dirname(new URL(import.meta.url).pathname), "..");
const dist = path.resolve(process.argv[2] ?? path.join(SITE_ROOT, "dist"));
const evidenceDir = path.resolve(process.argv[3] ?? path.join(SITE_ROOT, "navbar-evidence"));
const label = process.argv[4] ?? "run";

const report = await measureNavbar({ dist, evidenceDir, label });
console.log(JSON.stringify(report, null, 2));
