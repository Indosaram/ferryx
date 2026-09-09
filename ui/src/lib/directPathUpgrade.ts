/**
 * Direct path upgrade: probe candidate endpoints (LAN / Tailscale) and pick the best
 * reachable one so the client can stop paying the relay hop.
 *
 * Browsers reject direct LAN probes for several unrelated reasons that all look the
 * same from JS: Private Network Access (PNA) preflight denial, Mixed Content blocking
 * of an http:// target from an https:// page, missing CORS headers, DNS/connect
 * failures. Every one of those surfaces as a rejected `fetch()` (usually `TypeError`),
 * and an unhandled one would become an uncaught rejection that crashes the upgrade.
 * `probeCandidate` therefore never rejects — it resolves to a reachability verdict.
 */

export type CandidateEndpointType = "lan" | "tailscale" | "relay";

export interface CandidateEndpoint {
  type: CandidateEndpointType;
  url: string;
  /** Higher wins. LAN is expected to outrank Tailscale, which outranks relay. */
  priority: number;
}

export type ProbeFailureReason =
  | "timeout"
  | "blocked"
  | "network"
  | "http-error";

export interface ProbeResult {
  candidate: CandidateEndpoint;
  ok: boolean;
  /** Wall-clock duration of the probe, in milliseconds. */
  durationMs: number;
  /** Present only when `ok` is false. */
  reason?: ProbeFailureReason;
  /** Human-readable detail for diagnostics; never thrown. */
  detail?: string;
}

export const DEFAULT_PROBE_TIMEOUT_MS = 1500;

const HEALTH_PATH = "/health";

function healthUrl(url: string): string {
  return `${url.replace(/\/+$/, "")}${HEALTH_PATH}`;
}

function now(): number {
  return typeof performance !== "undefined" && typeof performance.now === "function"
    ? performance.now()
    : Date.now();
}

function classifyFailure(error: unknown): ProbeFailureReason {
  const name = (error as { name?: unknown })?.name;
  if (name === "AbortError" || name === "TimeoutError") return "timeout";
  const message = String((error as { message?: unknown })?.message ?? error ?? "");
  // PNA preflight denial, Mixed Content block, and CORS rejection are all reported as
  // opaque TypeErrors whose message (when present) names the policy that fired.
  if (
    /private network|mixed content|cors|cross-origin|access-control|blocked/i.test(
      message,
    )
  ) {
    return "blocked";
  }
  return "network";
}

/**
 * Sends a health request to `candidate` and resolves with a verdict. Never rejects,
 * and always clears its timer and aborts its in-flight request before resolving.
 */
export async function probeCandidate(
  candidate: CandidateEndpoint,
  timeoutMs: number = DEFAULT_PROBE_TIMEOUT_MS,
): Promise<ProbeResult> {
  const startedAt = now();
  const controller = new AbortController();
  let timedOut = false;
  const timer = setTimeout(() => {
    timedOut = true;
    controller.abort();
  }, timeoutMs);

  try {
    const response = await fetch(healthUrl(candidate.url), {
      method: "GET",
      signal: controller.signal,
      cache: "no-store",
      credentials: "omit",
      mode: "cors",
    });
    if (!response?.ok) {
      return {
        candidate,
        ok: false,
        durationMs: now() - startedAt,
        reason: "http-error",
        detail: `HTTP ${response?.status ?? "unknown"}`,
      };
    }
    return { candidate, ok: true, durationMs: now() - startedAt };
  } catch (error) {
    const reason = timedOut ? "timeout" : classifyFailure(error);
    return {
      candidate,
      ok: false,
      durationMs: now() - startedAt,
      reason,
      detail: String((error as { message?: unknown })?.message ?? error ?? reason),
    };
  } finally {
    clearTimeout(timer);
    // Releases the socket when we bailed out on an HTTP error rather than a timeout.
    if (!controller.signal.aborted) controller.abort();
  }
}

/** Probes every non-relay candidate concurrently and reports each verdict. */
export async function probeCandidates(
  candidates: readonly CandidateEndpoint[],
  timeoutMs: number = DEFAULT_PROBE_TIMEOUT_MS,
): Promise<ProbeResult[]> {
  const direct = candidates.filter((candidate) => candidate.type !== "relay");
  return Promise.all(direct.map((candidate) => probeCandidate(candidate, timeoutMs)));
}

/**
 * Probes all direct candidates concurrently and returns the highest-priority reachable
 * one, or `null` when none is reachable — in which case the caller stays on the relay.
 * Ties on priority are broken by the faster probe.
 */
export async function selectBestDirectCandidate(
  candidates: readonly CandidateEndpoint[],
  timeoutMs: number = DEFAULT_PROBE_TIMEOUT_MS,
): Promise<CandidateEndpoint | null> {
  const results = await probeCandidates(candidates, timeoutMs);
  const reachable = results.filter((result) => result.ok);
  if (reachable.length === 0) return null;
  reachable.sort((a, b) => {
    if (b.candidate.priority !== a.candidate.priority) {
      return b.candidate.priority - a.candidate.priority;
    }
    return a.durationMs - b.durationMs;
  });
  return reachable[0].candidate;
}
