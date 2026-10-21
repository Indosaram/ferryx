import { createPairedDaemonProjectAdapter, type PairedHostContext } from "../../../lib/pairedDaemonProject";
import { DEFAULT_MACHINE_LABEL } from "../../../lib/pairedHostInventory";
import type { SshHost } from "../../../lib/sshHosts";
import { remoteHostKey, selectHostList, type HostEndpoint, type RemoteHostState } from "../../../state/remoteHostStore";
import { AccountSessionError, type AccountMachineView } from "../../../remote/accountSession";

/** Codes a capability check may report; unchanged from the pre-redesign RemoteSection. */
export const CHECK_RESULT_CODES = new Set([
  "NATIVE_CONTEXT_REQUIRED", "PAIR_FAILED", "MACHINE_GRANT_REQUIRED", "UNSUPPORTED_CAPABILITY",
  "STALE_HOST_GENERATION", "PAIRED_HOST_UNAVAILABLE", "OFFLINE", "UNCHECKED", "READY",
]);

export async function defaultNegotiate(context: PairedHostContext) {
  const result = await createPairedDaemonProjectAdapter(context).capabilities();
  if (!["directoryBrowseV1", "machineWorkspaceV1"].every(capability => result.capabilities.includes(capability))) {
    throw new Error("UNSUPPORTED_CAPABILITY");
  }
}

export function sameConnection(left: SshHost, right: SshHost): boolean {
  return (
    left.id === right.id &&
    left.hostname === right.hostname &&
    left.username === right.username &&
    left.port === right.port &&
    left.identityFile === right.identityFile &&
    left.jumpHost === right.jumpHost &&
    left.authMethod === right.authMethod &&
    left.disabled === right.disabled
  );
}

/** Message for a typed or plain error; null for values that carry none. */
export function accountErrorMessage(err: unknown): string | null {
  if (err instanceof AccountSessionError) return err.message;
  if (err && typeof err === "object" && "code" in err && typeof (err as { code: unknown }).code === "string") {
    const typed = err as { code: string; message?: string };
    return typed.message || typed.code;
  }
  return err instanceof Error ? err.message : null;
}

export function describeAccountError(err: unknown): string {
  return accountErrorMessage(err) ?? String(err);
}

/** Account-enrolled machines merged with the local paired inventory, account entries first. */
export function mergePairedHosts(accountMachines: AccountMachineView[], state: RemoteHostState, accountOrigin: string): HostEndpoint[] {
  const accountHostEndpoints: HostEndpoint[] = accountMachines.map((m) => {
    const hostId = m.machineId
      ? remoteHostKey(m.relayOrigin || accountOrigin, m.machineId)
      : m.machineRecordId;
    const local =
      state.hosts[hostId] ??
      Object.values(state.hosts).find((h) => h.machineId === m.machineId);
    const effectiveScope = local?.grantScope;
    const isPaired = effectiveScope === "machine" && (local ? local.authStatus === "paired" : true);
    return {
      hostId: local?.hostId ?? hostId,
      name: m.displayName || local?.name || DEFAULT_MACHINE_LABEL,
      address: m.relayOrigin || accountOrigin,
      transport: "relay",
      authStatus: isPaired ? "paired" : (local?.authStatus ?? "needsMachineGrant"),
      online: m.online,
      machineId: m.machineId,
      displayName: m.displayName,
      relayOrigin: m.relayOrigin || accountOrigin,
      generation: local?.generation ?? String(m.enrollmentEpoch ?? "1"),
      grantScope: effectiveScope,
      lastSeenAt: m.lastSeenAt,
    };
  });
  return [
    ...accountHostEndpoints,
    ...selectHostList(state).filter(
      (ph) => !accountHostEndpoints.some((ah) => ah.machineId === ph.machineId || ah.hostId === ph.hostId),
    ),
  ];
}

export type MixedItem =
  | { kind: "paired"; id: string; name: string; host: HostEndpoint }
  | { kind: "ssh"; id: string; name: string; host: SshHost };

export function matchesQuery(item: MixedItem, query: string): boolean {
  if (!query.trim()) return true;
  const q = query.toLowerCase();
  if (item.kind === "paired") {
    return (
      item.host.name.toLowerCase().includes(q) ||
      Boolean(item.host.machineId && item.host.machineId.toLowerCase().includes(q)) ||
      item.host.hostId.toLowerCase().includes(q)
    );
  }
  return (
    item.host.label.toLowerCase().includes(q) ||
    item.host.hostname.toLowerCase().includes(q) ||
    Boolean(item.host.username && item.host.username.toLowerCase().includes(q))
  );
}
