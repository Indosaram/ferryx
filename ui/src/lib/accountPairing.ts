import { nativePairedHostCommands, recordInventoryDiagnostic } from "./pairedHostInventory";

/**
 * Signing in IS the connection. The desktop hands its account session to the daemon, which
 * enrolls this computer when it has no enrollment record and pairs every machine of the
 * account, then opens agent control. Sign-out hands `null` so the daemon stops using the
 * session. There is no per-machine registration step anywhere in the UI.
 *
 * Failure is deliberately silent here: the next observed event (a focus, a sign-in, an
 * inventory refresh) retries, and the affected rows stay in their transient pairing state.
 * A thrown error would surface a dialog for something the user cannot act on.
 */
export async function adoptAccountSession(
  accountOrigin: string,
  sessionToken: string | null,
): Promise<void> {
  const adopt = nativePairedHostCommands.adoptAccountSession;
  if (!adopt) return;
  try {
    const report = await adopt({ accountOrigin, sessionToken });
    // The report is the only place a whole-account failure is visible; each code is already a
    // sanitized token from the daemon, so it can be logged as-is.
    if (report?.blocked) recordInventoryDiagnostic("paired.account.blocked", { code: report.blocked });
    for (const skip of report?.skipped ?? []) {
      recordInventoryDiagnostic("paired.account.skipped", { code: skip.code, machineId: skip.machineId });
    }
  } catch {
    // Retried by the next trigger; see above.
  }
}
