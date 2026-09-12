import { remoteHostKey, remoteHostStore } from "../state/remoteHostStore";

export type PairRemoteMachineParams = {
  codeOrUrl: string;
  relayUrl?: string;
  deviceName?: string;
};

export async function pairRemoteMachine({
  codeOrUrl,
  relayUrl = "https://relay.checka.cc",
  deviceName = "Ferryx Desktop",
}: PairRemoteMachineParams): Promise<{ hostId: string; machineId: string; displayName: string }> {
  let code = codeOrUrl.trim();
  let targetRelay = relayUrl.trim().replace(/\/+$/, "");

  // If the user pasted a full URL like https://relay.checka.cc/#pair=TOKEN or #pair=TOKEN
  if (code.includes("#pair=")) {
    const [urlPart, fragment] = code.split("#pair=");
    if (urlPart.startsWith("http://") || urlPart.startsWith("https://")) {
      targetRelay = new URL(urlPart).origin;
    }
    // Extract token/pin from fragment (may contain &hints=... etc)
    code = fragment.split("&")[0];
  }

  const endpoint = `${targetRelay}/api/v1/pair/exchange`;
  const res = await fetch(endpoint, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ code, deviceName }),
  });

  if (!res.ok) {
    let msg = `Pairing failed (${res.status})`;
    try {
      const errJson = await res.json();
      if (errJson.message) msg = errJson.message;
    } catch {}
    throw new Error(msg);
  }

  const data = await res.json();
  const { token, machineId, displayName } = data;
  if (!token || !machineId) {
    throw new Error("Invalid response from pairing server");
  }

  const hostKey = remoteHostKey(targetRelay, machineId);
  const name = displayName || `Machine ${machineId.slice(0, 8)}`;

  remoteHostStore.upsertHost({
    hostId: hostKey,
    machineId,
    name,
    address: targetRelay,
    relayOrigin: targetRelay,
    deviceToken: token,
    transport: "relay",
    authStatus: "paired",
    online: true,
    lastSeenAt: Date.now(),
    directHints: [],
  });

  remoteHostStore.setActiveHost(hostKey);

  return { hostId: hostKey, machineId, displayName: name };
}
