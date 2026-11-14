import { describe, expect, test } from "bun:test";

import { identifyKind } from "./lib/release-platforms.mjs";

// `ferryx-cli-darwin-universal` and `ferryx-rd-client-darwin-universal` both end
// with `-darwin-universal`, which every darwin executable kind lists as an
// allowed extension. Identifying by suffix alone let the first declared kind win,
// so the remote-desktop client was recorded as a second CLI and the darwin run
// failed with `produced [cli-darwin-universal, cli-darwin-universal, macos-updater]`.
describe("identifyKind", () => {
  test("separates the darwin CLI from the darwin remote-desktop client", () => {
    expect(identifyKind("ferryx-cli-darwin-universal", "macbook")).toBe("cli-darwin-universal");
    expect(identifyKind("ferryx-rd-client-darwin-universal", "macbook")).toBe("rd-client-darwin-universal");
  });

  test("separates the linux CLI from the linux remote-desktop client", () => {
    expect(identifyKind("ferryx-cli-linux-amd64", "omaki")).toBe("cli-linux-amd64");
    expect(identifyKind("ferryx-rd-client-linux-amd64", "omaki")).toBe("rd-client-linux-amd64");
  });

  test("keeps the darwin app archive and updater signature apart", () => {
    expect(identifyKind("Ferryx.app.tar.gz", "macbook")).toBe("macos-updater");
    expect(identifyKind("Ferryx.app.tar.gz.sig", "macbook")).toBeNull();
    expect(identifyKind("Ferryx_universal.dmg", "macbook")).toBe("dmg");
  });

  test("ignores a kind that belongs to another host", () => {
    expect(identifyKind("Ferryx_x64.msix", "macbook")).toBeNull();
    expect(identifyKind("Ferryx_x64.msix", "maho-win")).toBe("msix");
  });
});
