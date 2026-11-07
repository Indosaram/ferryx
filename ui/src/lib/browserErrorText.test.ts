import { describe, expect, it } from "vitest";

import { browserErrorText, describeBrowserError, isVaultUnavailable, VAULT_UNAVAILABLE_CODE } from "./browserErrorText";

describe("browserErrorText", () => {
  it("never shows a known code verbatim", () => {
    for (const code of ["BROWSER_FIND_FAILED", "BROWSER_NOT_FOUND", VAULT_UNAVAILABLE_CODE, "UNKNOWN_ERROR"]) {
      expect(browserErrorText(code)).not.toContain(code);
    }
  });

  it("passes an unmapped code through so it stays identifiable", () => {
    expect(browserErrorText("SOME_NEW_CODE")).toBe("SOME_NEW_CODE");
    expect(describeBrowserError({ code: "SOME_NEW_CODE", message: "x" })).toBe("SOME_NEW_CODE");
  });

  it("recognizes the vault-unavailable rejection", () => {
    expect(isVaultUnavailable({ code: VAULT_UNAVAILABLE_CODE, message: "no keyring" })).toBe(true);
    expect(isVaultUnavailable({ code: "BROWSER_VAULT_NOT_FOUND", message: "gone" })).toBe(false);
  });
});
