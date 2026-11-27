import { beforeEach, expect, it, vi } from "vitest";
import { bootTrace } from "./tauri";
import { adoptAccountSession, adoptAccountSessionWithReport } from "./accountPairing";
import { nativePairedHostCommands } from "./pairedHostInventory";

vi.mock("./tauri", () => ({ bootTrace: vi.fn().mockResolvedValue(undefined) }));

const origin = "https://relay.example";
const empty = { paired: [], alreadyUsable: [], skipped: [], blocked: null as string | null };

beforeEach(() => {
  vi.restoreAllMocks();
  vi.mocked(bootTrace).mockClear();
});

it("hands the account session and its origin to the daemon from a signed-in session", async () => {
  const adopt = vi.spyOn(nativePairedHostCommands, "adoptAccountSession").mockResolvedValue({ ...empty, paired: ["host-a"] });
  await adoptAccountSession(origin, "session-token");
  expect(adopt).toHaveBeenCalledWith({ accountOrigin: origin, sessionToken: "session-token" });
});

it("hands null on sign-out so the daemon stops using the session", async () => {
  const adopt = vi.spyOn(nativePairedHostCommands, "adoptAccountSession").mockResolvedValue(empty);
  await adoptAccountSession(origin, null);
  expect(adopt).toHaveBeenCalledWith({ accountOrigin: origin, sessionToken: null });
});

it("resolves with no value for a caller that only triggers the pass", async () => {
  vi.spyOn(nativePairedHostCommands, "adoptAccountSession").mockResolvedValue({ ...empty, paired: ["host-a"] });
  await expect(adoptAccountSession(origin, "session-token")).resolves.toBeUndefined();
});

it("returns the report so a caller can show what the pass did", async () => {
  const answer = {
    paired: ["host-a"],
    alreadyUsable: ["machine-b"],
    skipped: [{ machineId: "machine-c", code: "MACHINE_GRANT_REQUIRED" }],
    blocked: null,
  };
  vi.spyOn(nativePairedHostCommands, "adoptAccountSession").mockResolvedValue(answer);
  await expect(adoptAccountSessionWithReport(origin, "session-token")).resolves.toEqual(answer);
});

it("records a blocked report as a sanitized diagnostic and never throws", async () => {
  vi.spyOn(nativePairedHostCommands, "adoptAccountSession").mockResolvedValue({ ...empty, blocked: "SESSION_EXPIRED" });
  const result = await adoptAccountSessionWithReport(origin, "session-token");
  expect(result?.blocked).toBe("SESSION_EXPIRED");
  expect(bootTrace).toHaveBeenCalledWith("paired.account.blocked", { code: "SESSION_EXPIRED" });
});

it("records each skipped machine with the code the daemon reported", async () => {
  vi.spyOn(nativePairedHostCommands, "adoptAccountSession").mockResolvedValue({
    ...empty,
    skipped: [{ machineId: "machine-a", code: "MACHINE_GRANT_REQUIRED" }],
  });
  await adoptAccountSessionWithReport(origin, "session-token");
  expect(bootTrace).toHaveBeenCalledWith("paired.account.skipped", { code: "MACHINE_GRANT_REQUIRED", machineId: "machine-a" });
});

it("swallows a rejected command so the next observed event retries it", async () => {
  vi.spyOn(nativePairedHostCommands, "adoptAccountSession").mockRejectedValue(new Error("PAIRED_HOST_UNAVAILABLE"));
  await expect(adoptAccountSessionWithReport(origin, "session-token")).resolves.toBeNull();
  await expect(adoptAccountSession(origin, "session-token")).resolves.toBeUndefined();
  expect(bootTrace).not.toHaveBeenCalled();
});

it("is a no-op when the installed build has no adopt command", async () => {
  const original = nativePairedHostCommands.adoptAccountSession;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  delete (nativePairedHostCommands as any).adoptAccountSession;
  try {
    await expect(adoptAccountSessionWithReport(origin, "session-token")).resolves.toBeNull();
    await expect(adoptAccountSession(origin, "session-token")).resolves.toBeUndefined();
    expect(bootTrace).not.toHaveBeenCalled();
  } finally {
    nativePairedHostCommands.adoptAccountSession = original;
  }
});
