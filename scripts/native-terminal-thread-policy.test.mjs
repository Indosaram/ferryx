import { afterAll, beforeAll, expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { scanThreadPolicy } from "./native-terminal-thread-policy.mjs";

let fixtureRoot;
const acquireOutsideClosure = `
fn render(window: &Window) {
    let frame = surface.get_current_texture().unwrap();
    let _ = window.run_on_main_thread(move || {
        host.present(frame);
    });
}
`;
const acquireInsideClosure = `
fn render(window: &Window) {
    let _ = window.run_on_main_thread(move || {
        let frame = surface.get_current_texture().unwrap();
        host.present(frame);
    });
}
`;
const deviceRequestInsideClosure = `
fn boot(window: &Window) {
    let _ = window.run_on_main_thread(move || {
        let adapter = pollster::block_on(instance.request_adapter(&options)).unwrap();
    });
}
`;

beforeAll(async () => {
  fixtureRoot = mkdtempSync(join(tmpdir(), "ferryx-thread-policy-"));
  await Bun.write(join(fixtureRoot, "outside/a.rs"), acquireOutsideClosure);
  await Bun.write(join(fixtureRoot, "inside/b.rs"), acquireInsideClosure);
  await Bun.write(join(fixtureRoot, "device/c.rs"), deviceRequestInsideClosure);
});

afterAll(() => {
  if (fixtureRoot) rmSync(fixtureRoot, { recursive: true, force: true });
});

test("GPU acquisition outside a run_on_main_thread closure is allowed", async () => {
  expect(await scanThreadPolicy([join(fixtureRoot, "outside")])).toEqual([]);
});

test("GPU acquisition inside a run_on_main_thread closure is rejected", async () => {
  const violations = await scanThreadPolicy([join(fixtureRoot, "inside")]);
  expect(violations).toHaveLength(1);
  expect(violations[0].line).toBe(4);
  expect(violations[0].text).toContain("get_current_texture");
});

test("adapter requests inside a run_on_main_thread closure are rejected", async () => {
  const violations = await scanThreadPolicy([join(fixtureRoot, "device")]);
  expect(violations).toHaveLength(1);
  expect(violations[0].text).toContain("request_adapter");
});

test("the shipped backend tree holds the policy", async () => {
  expect(await scanThreadPolicy()).toEqual([]);
});
