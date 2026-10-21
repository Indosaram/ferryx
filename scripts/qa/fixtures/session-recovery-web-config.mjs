/** Source-only configuration builder. No artifact reads, servers or process proof. */
export function buildWebSelection(candidate, { target, rebindTarget, exitFrame, gapFrame }) {
  const fail = code => { throw Object.assign(new Error(code), { code }); };
  if (!candidate?.stamp || !candidate.sourceHash ||
      candidate.binary?.sourceHash !== candidate.sourceHash ||
      candidate.ui?.sourceHash !== candidate.sourceHash ||
      !candidate.binary?.path || !candidate.binary.sha256 ||
      !candidate.ui?.path || !candidate.ui.sha256 || !candidate.ui.files?.length)
    fail('CANDIDATE_RECEIPT_REQUIRED');
  for (const binding of [target, rebindTarget]) {
    if (!binding || !['sessionId', 'daemonEpoch', 'incarnation'].every(key =>
        typeof binding[key] === 'string' && binding[key].length > 0) ||
        !Number.isSafeInteger(binding.pid) || binding.pid <= 0)
      fail('WEB_FIXTURE_TARGET_REQUIRED');
  }
  if (target.sessionId === rebindTarget.sessionId) fail('WEB_FIXTURE_DISTINCT_TARGETS_REQUIRED');
  // The inspected legacy grid consumer defines neither exit nor output-gap controls.
  // Only the owning consumed-contract lane may supply these callable codecs.
  if (typeof exitFrame !== 'function' || typeof gapFrame !== 'function')
    fail('WEB_CONSUMED_EXIT_GAP_CODECS_REQUIRED');
  return {
    target: { ...target }, rebindTarget: { ...rebindTarget }, exitFrame, gapFrame,
    evidenceScope: 'controlled-ui',
    identityEvidence: 'fixture-identity-proof-not-machine-pid',
    createRoute: {
      method: 'POST', path: '/api/v1/workspace/select',
      response(previousTarget, body) {
        if (!previousTarget || previousTarget.sessionId !== target.sessionId ||
            !body || typeof body.workspaceId !== 'string' || !body.workspaceId ||
            body.createTerminal !== true || body.sessionId !== undefined || body.tabId !== undefined ||
            (body.worktreeSlug !== undefined && typeof body.worktreeSlug !== 'string'))
          fail('WEB_EXPLICIT_CREATE_BODY_REQUIRED');
        // Local-token RemoteApp consumes response.ok, not a machine session body.
        return { ok: true };
      },
    },
  };
}
