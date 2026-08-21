import { describe, expect, it } from "vitest";

import type { StructuredIpcError } from "./types";
import { worktreeErrorMessage } from "./ipcErrors";

function error(code: string, message: string): StructuredIpcError {
  return { code, message, details: {} };
}

describe("worktreeErrorMessage", () => {
  it("branches on the structured error code", () => {
    expect(worktreeErrorMessage(error("DIRTY_WORKTREE", "backend wording may change"))).toContain("uncommitted changes");
    expect(worktreeErrorMessage(error("WRITER_ALREADY_ACTIVE", "irrelevant"))).toContain("another writer session");
    expect(worktreeErrorMessage(error("GIT_ERROR", "irrelevant"))).toContain("base ref");
  });

  it("never infers an error code by parsing the message", () => {
    expect(worktreeErrorMessage(error("UNKNOWN", "DIRTY_WORKTREE: text only"))).toBe("DIRTY_WORKTREE: text only");
  });
});
