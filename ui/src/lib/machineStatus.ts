import { isTailscaleProvisioned } from "./tailscaleDiscovery";
import type { PairedHostReachabilityView } from "./pairedHostReachability";
import type { HostEndpoint } from "../state/remoteHostStore";
import type { SshHostSource, SshTargetSummary } from "./sshHosts";

export type MachineState = "connected" | "offline" | "checking" | "needsAccess" | "settingUp" | "error" | "disabled";
export type PrimaryAction = "openProject" | "connect" | "setUp" | "retry" | "signIn" | "none";

export interface MachineStatus {
  state: MachineState;
  /** Machine-status data-code, kept identical to the pre-redesign codes plus CHECKING. */
  code: string;
  label: string;
  reason: string | null;
  primary: PrimaryAction;
  primaryEnabled: boolean;
}

export const MACHINE_STATE_LABELS: Record<MachineState, string> = {
  connected: "Connected",
  offline: "Offline",
  checking: "Checking\u2026",
  needsAccess: "Needs access",
  settingUp: "Setting up",
  error: "Error",
  disabled: "Disabled",
};

/** Same texts as RemoteSection's `explanations`, so tooltips keep their meaning. */
export const MACHINE_STATUS_EXPLANATIONS: Record<string, string> = {
  NATIVE_CONTEXT_REQUIRED: "Native host inventory is unavailable. Use the desktop app with a compatible local daemon; browser mirror access cannot manage machines.",
  PAIR_FAILED: "Could not pair. Check connectivity and daemon compatibility, issue a fresh enrollment code from a signed-in desktop, and retry.",
  MACHINE_GRANT_REQUIRED: "Needs machine access. This credential carries no machine grant. Pair again from a signed-in account; revoked credentials cannot reconnect.",
  UNSUPPORTED_CAPABILITY: "This relay, remote daemon, or local daemon does not advertise the required machine capabilities. Upgrade compatible components.",
  STALE_HOST_GENERATION: "Credentials changed during this request. Refresh the inventory and check capabilities again.",
  PAIRED_HOST_UNAVAILABLE: "The native host operation failed. Check the relay and daemon versions, connectivity and machine grant, then retry. Saved projects have not been removed.",
  OFFLINE: "Machine is offline. Saved projects remain available in the workspace; reconnect to the owning daemon.",
  UNCHECKED: "Check the remote machine capabilities before adding a project.",
  CHECKING: "Checking whether the machine is reachable.",
  READY: "Machine project capabilities verified.",
};

const REPAIR_REASON = "Re-pair with an enrollment code";
const NO_OPEN_REASON = "Opening projects is not available here";

function status(state: MachineState, code: string, primary: PrimaryAction, primaryEnabled: boolean, reason: string | null, label = MACHINE_STATE_LABELS[state]): MachineStatus {
  // Invariant: a primary the user cannot press always says why.
  return { state, code, label, primary, primaryEnabled, reason: primaryEnabled ? reason : reason ?? MACHINE_STATUS_EXPLANATIONS[code] ?? "Unavailable" };
}

export interface PairedMachineStatusInput {
  host: Pick<HostEndpoint, "authStatus" | "grantScope" | "generation" | "online">;
  /** The local native inventory is ready (remoteHostStore nativeStatus === "ready"). */
  nativeReady: boolean;
  /** The host is an enrolled machine of the signed-in account. */
  accountEnrolled: boolean;
  reachability: PairedHostReachabilityView;
  /** Last capability check result for this host, if any. */
  check?: { generation: string; code: string } | null;
  canOpenProject?: boolean;
}

export function pairedMachineStatus(input: PairedMachineStatusInput): MachineStatus {
  const { host, nativeReady, accountEnrolled, reachability, check } = input;
  if (!nativeReady && !accountEnrolled) {
    return status("error", "NATIVE_CONTEXT_REQUIRED", "none", false, MACHINE_STATUS_EXPLANATIONS.NATIVE_CONTEXT_REQUIRED);
  }
  if (host.authStatus === "revoked") {
    return accountEnrolled
      ? status("needsAccess", "MACHINE_GRANT_REQUIRED", "connect", true, "Access to this machine was revoked")
      : status("needsAccess", "MACHINE_GRANT_REQUIRED", "none", false, `Access to this machine was revoked. ${REPAIR_REASON}`);
  }
  if (host.authStatus !== "paired" || host.grantScope !== "machine") {
    return accountEnrolled
      ? status("needsAccess", "MACHINE_GRANT_REQUIRED", "connect", true, MACHINE_STATUS_EXPLANATIONS.MACHINE_GRANT_REQUIRED)
      : status("needsAccess", "MACHINE_GRANT_REQUIRED", "none", false, REPAIR_REASON);
  }
  if (reachability.supported) {
    // No probe for the current credential generation yet counts as checking (Wire contract).
    const entry = reachability.entry?.generation === host.generation ? reachability.entry : null;
    if (entry?.state === "offline") {
      return status("offline", "OFFLINE", "openProject", false, entry.lastError ? `Machine is offline: ${entry.lastError}` : MACHINE_STATUS_EXPLANATIONS.OFFLINE);
    }
    if (entry?.state !== "online") {
      return status("checking", "CHECKING", "openProject", false, MACHINE_STATUS_EXPLANATIONS.CHECKING);
    }
  } else if (!host.online) {
    return status("offline", "OFFLINE", "openProject", false, MACHINE_STATUS_EXPLANATIONS.OFFLINE);
  }
  if (check && check.generation === host.generation && check.code !== "READY") {
    if (check.code === "UNCHECKED") {
      // RemoteSection writes UNCHECKED for the duration of an in-flight capability check.
      return status("checking", "UNCHECKED", "openProject", false, "Checking machine capabilities");
    }
    if (check.code === "OFFLINE") return status("offline", "OFFLINE", "openProject", false, MACHINE_STATUS_EXPLANATIONS.OFFLINE);
    if (check.code === "MACHINE_GRANT_REQUIRED") {
      return accountEnrolled
        ? status("needsAccess", "MACHINE_GRANT_REQUIRED", "connect", true, MACHINE_STATUS_EXPLANATIONS.MACHINE_GRANT_REQUIRED)
        : status("needsAccess", "MACHINE_GRANT_REQUIRED", "none", false, REPAIR_REASON);
    }
    const reason = MACHINE_STATUS_EXPLANATIONS[check.code] ?? MACHINE_STATUS_EXPLANATIONS.PAIRED_HOST_UNAVAILABLE;
    return nativeReady
      ? status("error", check.code, "retry", true, reason)
      : status("error", check.code, "retry", false, MACHINE_STATUS_EXPLANATIONS.NATIVE_CONTEXT_REQUIRED);
  }
  return input.canOpenProject === false
    ? status("connected", "READY", "openProject", false, NO_OPEN_REASON)
    : status("connected", "READY", "openProject", true, null);
}

export interface SshMachineStatusInput {
  host: { source: SshHostSource; disabled?: boolean | null };
  tailscaleTarget?: SshTargetSummary;
  /** Tailscale discovery is refreshing (useTailscaleDiscovery().refreshing). */
  discoveryRefreshing: boolean;
  test?: { testing?: boolean; reachable?: boolean } | null;
  /** Agent integration was prepared on the host. */
  prepared: boolean;
  canOpenProject?: boolean;
}

/** The SSH code ladder of RemoteSection, code for code, mapped onto the unified states. */
export function sshMachineStatus(input: SshMachineStatusInput): MachineStatus {
  const { host, tailscaleTarget: target, discoveryRefreshing: refreshing, test, prepared } = input;
  const provisioned = isTailscaleProvisioned(target);
  const needsTailscale = (host.source === "tailscale" || Boolean(target)) && !provisioned;
  const retryTailscale = (reason: string) => refreshing
    ? { enabled: false, reason: "Tailscale discovery is in progress" }
    : { enabled: true, reason };

  if (host.disabled) return status("disabled", "DISABLED", "none", false, "Machine is disabled");
  if (refreshing && !target && host.source === "tailscale") {
    return status("checking", "CONNECTING", "openProject", false, "Discovering Tailscale machines\u2026");
  }
  if (target?.diagnostic) {
    const retry = retryTailscale(target.diagnostic.message);
    return status("error", target.diagnostic.code, "retry", retry.enabled, retry.reason);
  }
  if (target && !target.reachable) {
    const retry = retryTailscale(target.lastError || "Machine is offline");
    return status("offline", "OFFLINE", "retry", retry.enabled, retry.reason);
  }
  if (provisioned) return openable("READY", input);
  // Reachable Tailscale peer whose helper is not installed: the row belongs under Discovered.
  // The code keeps the test ladder value below so existing data-code assertions hold.
  const code = test?.testing
    ? "TESTING"
    : test?.reachable === true
    ? prepared ? "READY" : "REACHABLE"
    : test?.reachable === false
    ? "ERROR"
    : "UNCHECKED";
  if (target?.reachable) {
    const setUp = retryTailscale("Tailscale machine needs setup");
    return status("settingUp", code, "setUp", setUp.enabled, setUp.reason);
  }
  switch (code) {
    case "TESTING":
      return status("checking", "TESTING", "retry", false, "Testing connection\u2026");
    case "READY":
    case "REACHABLE":
      return needsTailscale ? status("settingUp", code, "setUp", !refreshing, refreshing ? "Tailscale discovery is in progress" : "Tailscale machine needs setup") : openable(code, input);
    case "ERROR":
      return status("error", "ERROR", "retry", true, "Connection test failed");
    default:
      return status("settingUp", "UNCHECKED", "retry", true, null, "Not checked");
  }
}

function openable(code: "READY" | "REACHABLE", input: SshMachineStatusInput): MachineStatus {
  return input.canOpenProject === false
    ? status("connected", code, "openProject", false, NO_OPEN_REASON)
    : status("connected", code, "openProject", true, null);
}
