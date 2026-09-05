import { describe, expect, it } from "vitest";
import { resolveRunTarget, validateHost, type HostConfig } from "./model";
export const host: HostConfig = { id: "qa", name: "QA", hostname: "localhost", user: "indo", port: 22222, identityFile: "/key", proxyJump: "jump@host:2200", knownHostsFile: "/trust" };
describe("SSH target configuration", () => {
  it("rejects invalid ports and option injection", () => {
    expect(validateHost({ ...host, port: 1.5 })).not.toBeNull();
    expect(validateHost({ ...host, hostname: "-oProxyCommand=evil" })).not.toBeNull();
    expect(validateHost(host)).toBeNull();
  });
  it("never falls back to local when a referenced host is removed", () => {
    expect(() => resolveRunTarget({ kind: "ssh", hostId: "qa" }, [])).toThrow();
    expect(resolveRunTarget({ kind: "ssh", hostId: "qa" }, [host])).toEqual({ kind: "ssh", hostId: "qa" });
    expect(resolveRunTarget(undefined, [])).toEqual({ kind: "local" });
  });
});
