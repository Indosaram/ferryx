import { describe, expect, it } from "vitest";

import type { RemoteGatewayStatus } from "../../lib/tauri";
import { remoteAccessUrl } from "./PhoneStep";

const relay = (relayUrl: string): RemoteGatewayStatus => ({
  enabled: true,
  mode: "relay",
  port: 43821,
  boundAddress: null,
  localIp: null,
  relayUrl,
});

describe("remoteAccessUrl", () => {
  it("turns a wss:// relay into the https pairing page a phone can open", () => {
    expect(remoteAccessUrl(relay("wss://relay.example.com/"))).toBe("https://relay.example.com/pair?relay=1");
  });

  it("turns a ws:// relay into an http pairing page", () => {
    expect(remoteAccessUrl(relay("ws://127.0.0.1:9000"))).toBe("http://127.0.0.1:9000/pair?relay=1");
  });

  it("keeps an https relay as is", () => {
    expect(remoteAccessUrl(relay("https://relay.example.com"))).toBe("https://relay.example.com/pair?relay=1");
  });
});
