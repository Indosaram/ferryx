import { Mu as resolveWorktreeOperationRoute } from "./store-CgXrfmaH.js";
import { lt as toSshExecutionHostId, st as parseExecutionHostId } from "./agent-status-3vUKbY6l.js";
var SSH_OWNER_CHANGED_MESSAGE = "Couldn't verify the SSH connection. Reconnect the host and try again.";
function captureDirectSshMutationExpectation(state, connectionId, runtimeEnvironmentId) {
	const generation = runtimeEnvironmentId ? state.sshStateByEnvironment?.get(runtimeEnvironmentId)?.connectionStates.get(connectionId)?.connectionGeneration : state.sshConnectionStates.get(connectionId)?.connectionGeneration;
	if (generation === void 0) throw new Error(SSH_OWNER_CHANGED_MESSAGE);
	return {
		expectedExecutionHostId: toSshExecutionHostId(connectionId),
		expectedSshTargetId: connectionId,
		expectedSshConnectionGeneration: generation
	};
}
function captureWorktreeSshMutationExpectation(state, worktreeId) {
	const route = resolveWorktreeOperationRoute(state, worktreeId);
	const host = parseExecutionHostId(route?.executionHostId);
	if (host?.kind === "local" || host?.kind === "runtime") return { expectedExecutionHostId: "local" };
	if (host?.kind !== "ssh") throw new Error(SSH_OWNER_CHANGED_MESSAGE);
	const generation = route?.runtimeEnvironmentId ? state.sshStateByEnvironment.get(route.runtimeEnvironmentId)?.connectionStates.get(host.targetId)?.connectionGeneration : state.sshConnectionStates.get(host.targetId)?.connectionGeneration;
	if (generation === void 0) throw new Error(SSH_OWNER_CHANGED_MESSAGE);
	return {
		expectedExecutionHostId: host.id,
		expectedSshTargetId: host.targetId,
		expectedSshConnectionGeneration: generation
	};
}
export { captureWorktreeSshMutationExpectation as n, captureDirectSshMutationExpectation as t };
