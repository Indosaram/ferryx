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
        host.finish_presentation(window);
    });
}
`;
const acquireInsideClosure = `
fn render(window: &Window) {
    let _ = window.run_on_main_thread(move || {
        let frame = surface.get_current_texture().unwrap();
        host.finish_presentation(window);
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

// The shape this repository actually uses. The render site never names run_on_main_thread; it
// hands a closure to a wrapper that does. An earlier version of the rule matched only the direct
// spelling, so it passed this file clean while every render pass acquired on the main thread.
const acquireInsideDispatchWrapper = `
fn dispatch_render_on_main_thread(window: &Window, task: impl FnOnce() + Send + 'static) {
    window.run_on_main_thread(task);
}

fn render(window: &Window) {
    dispatch_render_on_main_thread(window, move || {
        let frame = surface.get_current_texture();
    });
}
`;

beforeAll(async () => {
  fixtureRoot = mkdtempSync(join(tmpdir(), "ferryx-thread-policy-"));
  await Bun.write(join(fixtureRoot, "outside/a.rs"), acquireOutsideClosure);
  await Bun.write(join(fixtureRoot, "inside/b.rs"), acquireInsideClosure);
  await Bun.write(join(fixtureRoot, "device/c.rs"), deviceRequestInsideClosure);
  await Bun.write(join(fixtureRoot, "wrapper/d.rs"), acquireInsideDispatchWrapper);
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

test("GPU acquisition inside the project's own main-thread dispatch wrapper is rejected", async () => {
  const violations = await scanThreadPolicy([join(fixtureRoot, "wrapper")]);
  expect(violations).toHaveLength(1);
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

for (const dispatch of ["window.run_on_main_thread", "dispatch_render_on_main_thread"]) {
  for (const call of ["queue.submit(commands)", "frame.present()", "renderer.render_to_surface_viewport(snapshot)"]) {
    test(`${dispatch} rejects ${call}`, async () => {
      const directory = join(fixtureRoot, `${dispatch}-${call.split("(")[0]}`);
      await Bun.write(join(directory, "render.rs"), `
fn render() {
    ${dispatch}(move || {
        ${call};
    });
}
`);
      const violations = await scanThreadPolicy([directory]);
      expect(violations).toHaveLength(1);
      expect(violations[0].text).toContain(call);
    });
  }
}
