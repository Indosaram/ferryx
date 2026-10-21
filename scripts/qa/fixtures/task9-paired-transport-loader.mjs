/**
 * Task 9 transport fixture loader: provisions a verifier-owned isolated paired client/host
 * daemon pair from the candidate binary and returns the ferryx-task9-transport-provider/1
 * fixture. No production profile, desktop or remote host is touched.
 */
import { provisionPairedFixture } from './transport-provider.mjs';
import { provisionIsolatedPairedSession } from './paired-isolated-provider.mjs';

export async function loadTransportFixture(ctx) {
  return provisionPairedFixture(ctx, {
    binary: ctx.binary,
    timeoutMs: ctx.timeoutMs,
    ledger: ctx.ledger,
    provisionPairedSession: provisionIsolatedPairedSession,
  });
}

export default loadTransportFixture;
