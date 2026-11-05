import { remoteAgentDefect } from './contract.mjs';
import { StepFailed } from './steps.mjs';

/** Resolve through the pinned helper before starting the agent; attest before any operation. */
export async function connectRemoteAgent({ helper, host, run, lane, connect }) {
  let agent;
  try {
    const resolved = await helper.call('resolve-agent', { path: host.remoteAgent,
      checkouts: [lane.receipt.harness?.checkout, lane.receipt.candidate?.checkout].filter(Boolean) });
    const expected = run.harnessRevision ?? run.revision;
    const location = { ...resolved, headRevision: expected, clean: true };
    const locationDefect = remoteAgentDefect({ ...lane.receipt, remoteAgent: location }, expected);
    if (locationDefect) throw new Error(locationDefect);
    agent = connect(resolved.resolvedPath);
    const reported = await agent.call('identity', {});
    lane.receipt.remoteAgent = { ...reported, resolvedPath: resolved.resolvedPath };
    lane.write('remote-agent', 'remote-agent.json', lane.receipt.remoteAgent);
    if (reported.resolvedPath !== resolved.resolvedPath) throw new Error('agent path differs from resolved executable');
    const defect = remoteAgentDefect(lane.receipt, expected);
    if (defect) throw new Error(defect);
    return agent;
  } catch (error) {
    agent?.close();
    throw new StepFailed('REMOTE_AGENT_PROVENANCE', String(error.message ?? error));
  }
}
