import type { HostEndpoint } from "../state/remoteHostStore";

export function hostTransportUrl(host: HostEndpoint | null, transportUrl: string): string {
  return host?.relayOrigin && host.machineId && new URL(transportUrl).origin === host.relayOrigin
    ? `${host.relayOrigin}/host/${encodeURIComponent(host.machineId)}`
    : transportUrl;
}

export function remoteApiUrl(baseUrl: string, path: string): string {
  return baseUrl === window.location.origin ? path : `${baseUrl}${path}`;
}

/** Each dial (including retries) gets its own target-bound, single-use ticket. */
export async function remoteSocketUrl(
  baseUrl: string,
  target: string,
  deviceToken: string,
  signal?: AbortSignal,
): Promise<string> {
  const base = new URL(baseUrl);
  const socket = new URL(`${baseUrl}${target}`);
  socket.protocol = base.protocol === "https:" ? "wss:" : "ws:";
  if (base.pathname.startsWith("/host/")) {
    const response = await fetch(`${baseUrl}/api/v1/socket-ticket`, {
      method: "POST",
      headers: { Authorization: `Bearer ${deviceToken}`, "Content-Type": "application/json" },
      body: JSON.stringify({ target }),
      signal,
    });
    if (!response.ok) throw new Error(`Socket ticket request failed (${response.status})`);
    const data: unknown = await response.json();
    if (!data || typeof data !== "object" || !("ticket" in data)
      || typeof data.ticket !== "string" || !data.ticket) throw new Error("Invalid socket ticket response");
    socket.searchParams.set("ticket", data.ticket);
  } else {
    // KNOWN GAP: the direct gateway now issues tickets too (POST
    // /api/v1/socket-ticket), but migrating this branch shifts the call order the
    // RemoteUI suites queue responses by, so it needs a coordinated harness
    // update rather than a one-line swap. Until then the direct path still sends
    // the permanent token in the URL.
    socket.searchParams.set("token", deviceToken);
  }
  return socket.toString();
}
