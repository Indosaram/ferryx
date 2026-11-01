import { extractBrowserErrorCode } from "./browserTauri";

export const VAULT_UNAVAILABLE_CODE = "BROWSER_VAULT_UNAVAILABLE";

export const VAULT_UNAVAILABLE_TEXT =
  "Saved passwords need the system keyring (Keychain, Credential Manager, or Secret Service/KWallet). It isn't available in this session.";

// Codes the browser UI can name in plain words. Anything else is shown as the code itself, so an
// unexpected failure stays identifiable in a bug report.
const READABLE: Record<string, string> = {
  BROWSER_FIND_FAILED: "This page can't be searched.",
  BROWSER_NOT_FOUND: "This tab is no longer open.",
  BROWSER_DOWNLOAD_FAILED: "The download failed.",
  BROWSER_DOWNLOAD_CANCELLED: "The download was cancelled.",
  [VAULT_UNAVAILABLE_CODE]: VAULT_UNAVAILABLE_TEXT,
  BROWSER_VAULT_CORRUPTED: "The saved passwords file is damaged and can't be read.",
  BROWSER_VAULT_NOT_FOUND: "That saved login no longer exists.",
  BROWSER_VAULT_PROMPT_EXPIRED: "This offer expired. Sign in again to save the password.",
  UNKNOWN_ERROR: "Something went wrong.",
};

/** Plain-words text for a browser error code, or the code itself when it has none. */
export function browserErrorText(code: string): string {
  return READABLE[code] ?? code;
}

/** Plain-words text for a rejected browser IPC call. */
export function describeBrowserError(error: unknown): string {
  return browserErrorText(extractBrowserErrorCode(error));
}

export function isVaultUnavailable(error: unknown): boolean {
  return extractBrowserErrorCode(error) === VAULT_UNAVAILABLE_CODE;
}
