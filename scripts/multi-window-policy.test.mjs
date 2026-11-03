import { afterAll, beforeAll, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { scanMultiWindowPolicy } from "./multi-window-policy.mjs";

let root;

beforeAll(() => {
  root = mkdtempSync(join(tmpdir(), "multi-window-policy-"));
  mkdirSync(join(root, "ipc"));
  writeFileSync(join(root, "window_registry.rs"), 'fn primary() { app.get_window("main"); }\n');
  writeFileSync(join(root, "ipc/ok.rs"), 'fn f() { window_for_session(&app, id); }\n#[cfg(test)]\nmod tests { fn t() { app.get_window("main"); } }\n');
  writeFileSync(join(root, "ipc/ok_tests.rs"), 'fn t() { app.get_webview_window("main"); }\n');
  writeFileSync(join(root, "ipc/bad.rs"), 'fn f() {\n    let w = app.get_webview_window( "main" );\n}\n');
  writeFileSync(join(root, "ipc/bad_label.rs"), 'fn f() { windows.iter().find(|w| w.label == "main"); }\n');
});

afterAll(() => rmSync(root, { recursive: true, force: true }));

test("flags a main-label window lookup in production code with file and line", () => {
  expect(scanMultiWindowPolicy(root)).toEqual([
    'ipc/bad.rs:2: let w = app.get_webview_window( "main" );',
    'ipc/bad_label.rs:1: fn f() { windows.iter().find(|w| w.label == "main"); }',
  ]);
});

test("allows window_registry.rs, test files and inline cfg(test) modules", () => {
  rmSync(join(root, "ipc/bad.rs"));
  rmSync(join(root, "ipc/bad_label.rs"));
  expect(scanMultiWindowPolicy(root)).toEqual([]);
});

