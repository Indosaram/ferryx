import { beforeEach, describe, expect, it } from "vitest";
import {
  clearRemoteAuthToken,
  getRemoteAuthToken,
  setRemoteAuthToken,
} from "./remoteClient";

describe("remoteClient storage helpers", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("reads canonical token when set", () => {
    localStorage.setItem("ferryx_remote_token", "test-canonical-token");
    expect(getRemoteAuthToken()).toBe("test-canonical-token");
  });

  it("reads legacy token as fallback and migrates on setRemoteAuthToken", () => {
    localStorage.setItem("rorca_remote_token", "test-legacy-token");
    expect(getRemoteAuthToken()).toBe("test-legacy-token");

    setRemoteAuthToken("test-new-token");
    expect(localStorage.getItem("ferryx_remote_token")).toBe("test-new-token");
    expect(localStorage.getItem("rorca_remote_token")).toBeNull();
    expect(getRemoteAuthToken()).toBe("test-new-token");
  });

  it("clears both canonical and legacy tokens on clearRemoteAuthToken", () => {
    localStorage.setItem("ferryx_remote_token", "canonical");
    localStorage.setItem("rorca_remote_token", "legacy");

    clearRemoteAuthToken();
    expect(localStorage.getItem("ferryx_remote_token")).toBeNull();
    expect(localStorage.getItem("rorca_remote_token")).toBeNull();
    expect(getRemoteAuthToken()).toBeNull();
  });
});
