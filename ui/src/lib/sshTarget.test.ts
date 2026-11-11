import { describe, expect, it } from "vitest";
import { looksLikeSshTarget, parseSshTarget } from "./sshTarget";

describe("parseSshTarget", () => {
  it.each([
    ["ssh -J indo box@100.110.229.115", { hostname: "100.110.229.115", username: "box", jumpHost: "indo" }],
    ["box@100.110.229.115", { hostname: "100.110.229.115", username: "box" }],
    ["ssh -p 2222 deploy@host.example.com", { hostname: "host.example.com", username: "deploy", port: 2222 }],
    ["ssh deploy@host.example.com -p2222", { hostname: "host.example.com", username: "deploy", port: 2222 }],
    ["ssh deploy@host -t tmux -p 9", { hostname: "host", username: "deploy" }],
    ["user@host:2200", { hostname: "host", username: "user", port: 2200 }],
    ["ssh://user@host:2200/", { hostname: "host", username: "user", port: 2200 }],
    ["ssh -l alice -i ~/.ssh/id_ed25519 host", { hostname: "host", username: "alice", identityFile: "~/.ssh/id_ed25519" }],
    ["ssh -o User=bob -o Port=2022 -o ProxyJump=j1,j2 host", { hostname: "host", username: "bob", port: 2022, jumpHost: "j1,j2" }],
    ["ssh -At -J jump@bastion:2222 root@[fe80::1]:22", { hostname: "fe80::1", username: "root", port: 22, jumpHost: "jump@bastion:2222" }],
    ["ssh root@10.0.0.1 tmux attach", { hostname: "10.0.0.1", username: "root" }],
    ["/usr/bin/ssh -i '/keys/my key' me@h", { hostname: "h", username: "me", identityFile: "/keys/my key" }],
    ["ssh -J indo \\\n  box@100.110.229.115", { hostname: "100.110.229.115", username: "box", jumpHost: "indo" }],
    ["fe80::1", { hostname: "fe80::1" }],
  ])("%s", (input, expected) => {
    expect(parseSshTarget(input)).toEqual(expected);
  });

  it("applies flags after the destination over the destination's own fields", () => {
    expect(parseSshTarget("ssh -p 2222 user@host:22")).toEqual({ hostname: "host", username: "user", port: 2222 });
  });

  it.each(["", "ssh", "ssh -J indo", "ssh -p notaport host", "user@host:99999", "'unterminated", "user@", "a b@c/d"])(
    "rejects %j",
    (input) => {
      expect(parseSshTarget(input)).toBeNull();
    },
  );
});

describe("looksLikeSshTarget", () => {
  it.each(["ssh host", "user@host", "host:2222", "[::1]:22", "ssh://host"])("treats %s as a target", (text) => {
    expect(looksLikeSshTarget(text)).toBe(true);
  });
  it.each(["host.example.com", "10.0.0.1", "fe80::1"])("leaves a plain hostname %s alone", (text) => {
    expect(looksLikeSshTarget(text)).toBe(false);
  });
});
