import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export const BROWSER_VAULT_SAVE_PROMPT_EVENT = "browser_vault_save_prompt";

/** Password-free listing entry; secrets only leave the backend through `revealVaultPassword`. */
export type VaultItemSummary = {
  id: string;
  origin: string;
  username: string;
  /** Milliseconds since the Unix epoch. */
  createdAt: number;
  updatedAt: number;
};

export type VaultStatus = {
  available: boolean;
  backend: "keychain" | "dpapi" | "secretService" | "unsupported";
  error?: string | null;
};

export type VaultSavePrompt = {
  browserId: string;
  promptId: string;
  origin: string;
  username: string;
  update: boolean;
};

export type PasswordGeneratorOptions = {
  mode?: "password" | "passphrase";
  length?: number;
  includeLowercase?: boolean;
  includeUppercase?: boolean;
  includeDigits?: boolean;
  includeSymbols?: boolean;
  avoidAmbiguous?: boolean;
  wordCount?: number;
  separator?: string;
  capitalize?: boolean;
  includeNumber?: boolean;
};

export type GeneratedPassword = {
  password: string;
  /** Strength bucket, 0 (weak) to 4 (strong). */
  score: number;
  entropyBits: number;
};

export function getVaultStatus(): Promise<VaultStatus> {
  return invoke<VaultStatus>("cmd_browser_vault_status");
}

export function listVaultItems(query?: string): Promise<VaultItemSummary[]> {
  return invoke<VaultItemSummary[]>("cmd_browser_vault_list", { query: query ?? null });
}

export function revealVaultPassword(id: string): Promise<string> {
  return invoke<string>("cmd_browser_vault_reveal", { id });
}

/** A `null` password keeps the stored one. */
export function updateVaultItem(id: string, username: string, password: string | null): Promise<VaultItemSummary> {
  return invoke<VaultItemSummary>("cmd_browser_vault_update", { id, username, password });
}

export function deleteVaultItem(id: string): Promise<void> {
  return invoke<void>("cmd_browser_vault_delete", { id });
}

export function generateVaultPassword(options: PasswordGeneratorOptions = {}): Promise<GeneratedPassword> {
  return invoke<GeneratedPassword>("cmd_browser_vault_generate", { options });
}

export function vaultMatchesForBrowser(browserId: string): Promise<VaultItemSummary[]> {
  return invoke<VaultItemSummary[]>("cmd_browser_vault_matches", { browserId });
}

export function fillVaultItem(browserId: string, id: string): Promise<void> {
  return invoke<void>("cmd_browser_vault_fill", { browserId, id });
}

export function saveVaultPrompt(promptId: string): Promise<VaultItemSummary> {
  return invoke<VaultItemSummary>("cmd_browser_vault_save_pending", { promptId });
}

export function dismissVaultPrompt(promptId: string): Promise<void> {
  return invoke<void>("cmd_browser_vault_dismiss_pending", { promptId });
}

export function onVaultSavePrompt(handler: (prompt: VaultSavePrompt) => void): Promise<UnlistenFn> {
  return listen<VaultSavePrompt>(BROWSER_VAULT_SAVE_PROMPT_EVENT, (event) => handler(event.payload));
}
