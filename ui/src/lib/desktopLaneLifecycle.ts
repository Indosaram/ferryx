import { invoke } from "@tauri-apps/api/core";
import { httpLaneLifecycleTransport, setLaneLifecycleTransport, type LaneLifecycleTransport, type LaneLifecycleModel, type LaneLifecycleView } from "./sidebarPersistence";

async function request<T>(operation: string, body: unknown, fallback: () => Promise<T>): Promise<T> {
  const reply = await invoke<{ status: number; value: T & { error?: string } } | null>("cmd_lane_lifecycle", { operation, body });
  if (reply === null) return fallback();
  if (reply.status < 200 || reply.status >= 300) {
    throw new Error(reply.value.error ?? `Lane lifecycle ${operation}: ${reply.status}`);
  }
  return reply.value;
}

const http = httpLaneLifecycleTransport();
export const desktopLaneLifecycleTransport: LaneLifecycleTransport = {
  read: () => request<LaneLifecycleModel>("read", {}, () => http.read()),
  reconcileIdentities: (rows) => request<LaneLifecycleModel>("identities", rows, () => http.reconcileIdentities!(rows)),
  write: async (body) => (await request<{ row: LaneLifecycleView }>("apply", body, async () => ({ row: await http.write(body) }))).row,
  writePinTabs: (body) => request<LaneLifecycleView>("pin-tabs", body, () => http.writePinTabs(body)),
  importOrder: async (order) => (await request<{ imported: boolean }>("import", { order }, async () => ({ imported: await http.importOrder(order) }))).imported,
  reorder: async (workspaceId, identifiers) => { await request("order", { workspaceId, identifiers }, () => http.reorder(workspaceId, identifiers)); },
};

/** Must run before App mounts: the webview is not served by the hub's HTTP origin. */
export function installDesktopLaneLifecycleTransport(): void {
  setLaneLifecycleTransport(desktopLaneLifecycleTransport);
}
