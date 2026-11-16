import { beforeEach, expect, it, vi } from "vitest";
import { bootTrace } from "./tauri";
import { adoptAccountSession } from "./accountPairing";
import { nativePairedHostCommands } from "./pairedHostInventory";

vi.mock("./tauri", () => ({ bootTrace: vi.fn().mockResolvedValue(undefined) }));

const origin = "https://relay.example";
const report = { paired: [], alreadyUsable: [], skipped: [], blocked: null as string | null };

beforeEach(() => {
  vi.restoreAllMocks();
  vi.mocked(bootTrace).mockClear();
});

it("hands the account session and its origin to the daemon from a signed-in session", async () => {
  const adopt = vi.spyOn(nativePairedHostCommands, "adoptAccountSession").mockResolvedValue({ ...report, paired: ["host-a"] });
  await adoptAccountSession(origin, "session-token");
  expect(adopt).toHaveBeenCalledWith({ accountOrigin: origin, sessionToken: "session-token" });
});

it("hands null on sign-out so the daemon stops using the session", async () => {
  const adopt = vi.spyOn(nativePairedHostCommands, "adoptAccountSession").mockResolvedValue(report);
  await adoptAccountSession(origin, null);
  expect(adopt).toHaveBeenCalledWith({ accountOrigin: origin, sessionToken: null });
});

it("records a blocked report as a sanitized diagnostic and never throws", async () => {
  vi.spyOn(nativePairedHostCommands, "adoptAccountSession").mockResolvedValue({ ...report, blocked: "SESSION_EXPIRED" });
  await expect(adoptAccountSession(origin, "session-token")).resolves.toBeUndefined();
  expect(bootTrace).toHaveBeenCalledWith("paired.account.blocked", { code: "SESSION_EXPIRED" });
});

it("records each skipped machine with the code the daemon reported", async () => {
  vi.spyOn(nativePairedHostCommands, "adoptAccountSession").mockResolvedValue({
    ...report,
    skipped: [{ machineId: "machine-a", code: "MACHINE_GRANT_REQUIRED" }],
  });
  await adoptAccountSession(origin, "session-token");
  expect(bootTrace).toHaveBeenCalledWith("paired.account.skipped", { code: "MACHINE_GRANT_REQUIRED", machineId: "machine-a" });
});

it("swallows a rejected command so the next observed event retries it", async () => {
  vi.spyOn(nativePairedHostCommands, "adoptAccountSession").mockRejectedValue(new Error("PAIRED_HOST_UNAVAILABLE"));
  await expect(adoptAccountSession(origin, "session-token")).resolves.toBeUndefined();
  expect(bootTrace).not.toHaveBeenCalled();
});

it("is a no-op when the installed build has no adopt command", async () => {
  const original = nativePairedHostCommands.adoptAccountSession;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  delete (nativePairedHostCommands as any).adoptAccountSession;
  try {
    await expect(adoptAccountSession(origin, "session-token")).resolves.toBeUndefined();
    expect(bootTrace).not.toHaveBeenCalled();
  } finally {
    nativePairedHostCommands.adoptAccountSession = original;
  }
});
