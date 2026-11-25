import { invoke, isTauri } from "@tauri-apps/api/core";

export interface CloudMachineSummary {
  name: string;
  provider: string;
  state: string;
  sandboxId: string;
  transport?: "ssh" | "relay";
  slot?: string | null;
}

export interface CloudProviderStatus {
  id: string;
  label: string;
  transport: "ssh" | "relay";
  freeQuota: string;
  configured: boolean;
  secretKeys: string[];
  missingKeys: string[];
  note: string;
}

export const CLOUD_HOST_LIST_COMMAND = "cmd_cloud_host_list";
export const CLOUD_HOST_PROVIDERS_COMMAND = "cmd_cloud_host_providers";
export const CLOUD_HOST_SET_SECRET_COMMAND = "cmd_cloud_host_set_secret";
export const CLOUD_HOST_DELETE_COMMAND = "cmd_cloud_host_delete";

export function isCloudMachine(value: unknown): value is CloudMachineSummary {
  if (!value || typeof value !== "object") return false;
  const row = value as Record<string, unknown>;
  return (
    typeof row.name === "string" &&
    row.name.length > 0 &&
    typeof row.provider === "string" &&
    row.provider.length > 0 &&
    typeof row.state === "string" &&
    typeof row.sandboxId === "string"
  );
}

export async function listCloudMachines(): Promise<CloudMachineSummary[]> {
  if (!isTauri()) return [];
  try {
    const rows = await invoke<CloudMachineSummary[]>(CLOUD_HOST_LIST_COMMAND);
    return Array.isArray(rows) ? rows.filter(isCloudMachine) : [];
  } catch {
    return [];
  }
}

export async function listCloudProviders(): Promise<CloudProviderStatus[]> {
  if (!isTauri()) return [];
  try {
    const rows = await invoke<CloudProviderStatus[]>(CLOUD_HOST_PROVIDERS_COMMAND);
    return Array.isArray(rows) ? rows : [];
  } catch {
    return [];
  }
}

export async function setCloudProviderSecret(
  key: string,
  value: string | null,
): Promise<CloudProviderStatus[]> {
  if (!isTauri()) return [];
  const rows = await invoke<CloudProviderStatus[]>(CLOUD_HOST_SET_SECRET_COMMAND, {
    key,
    value,
  });
  return Array.isArray(rows) ? rows : [];
}

export async function deleteCloudMachine(name: string): Promise<void> {
  if (!isTauri()) return;
  await invoke(CLOUD_HOST_DELETE_COMMAND, { name });
}
