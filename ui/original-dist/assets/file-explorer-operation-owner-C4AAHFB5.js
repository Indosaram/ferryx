import { a as translate } from "./jsx-runtime-Cv_nyRjc.js";
import { Cc as captureWorktreeOperationGenerationGuard, Du as getExplicitRuntimeEnvironmentIdForWorktree, Mu as resolveWorktreeOperationRoute, Pm as FLOATING_TERMINAL_WORKTREE_ID, Ut as getConnectionIdFromState, ku as getSettingsForWorktreeRuntimeOwner, t as useAppStore, vd as parseWorkspaceKey } from "./store-CgXrfmaH.js";
import { st as parseExecutionHostId } from "./agent-status-3vUKbY6l.js";
function getFileExplorerOperationOwnerFromState(state, worktreeId) {
	if (worktreeId === "global-floating-terminal") return { kind: "local" };
	const parsedWorkspace = worktreeId ? parseWorkspaceKey(worktreeId) : null;
	if (worktreeId && parsedWorkspace?.type !== "folder") {
		const route = resolveWorktreeOperationRoute(state, worktreeId);
		if (!route) return { kind: "unresolved" };
		if (route.runtimeEnvironmentId) return {
			kind: "runtime",
			environmentId: route.runtimeEnvironmentId,
			executionHostId: route.executionHostId ?? `runtime:${encodeURIComponent(route.runtimeEnvironmentId)}`
		};
		if (route.executionHostId) return operationOwnerFromHostId(route.executionHostId);
	}
	const connectionId = getConnectionIdFromState(state, worktreeId ?? null);
	const explicitRuntimeEnvironmentId = getExplicitRuntimeEnvironmentIdForWorktree(state, worktreeId);
	if (connectionId === void 0 && explicitRuntimeEnvironmentId === null) return { kind: "unresolved" };
	const settings = getSettingsForWorktreeRuntimeOwner(state, worktreeId);
	const runtimeEnvironmentId = connectionId && explicitRuntimeEnvironmentId === null ? null : settings.activeRuntimeEnvironmentId?.trim();
	if (runtimeEnvironmentId) return {
		kind: "runtime",
		environmentId: runtimeEnvironmentId,
		executionHostId: `runtime:${encodeURIComponent(runtimeEnvironmentId)}`
	};
	if (connectionId === void 0) return { kind: "unresolved" };
	return connectionId ? {
		kind: "ssh",
		connectionId
	} : { kind: "local" };
}
function getFileExplorerOperationOwner(worktreeId) {
	return getFileExplorerOperationOwnerFromState(useAppStore.getState(), worktreeId);
}
function getFileExplorerOperationRoute(owner) {
	switch (owner.kind) {
		case "local": return {
			settings: { activeRuntimeEnvironmentId: null },
			expectedExecutionHostId: "local"
		};
		case "ssh": return {
			settings: { activeRuntimeEnvironmentId: null },
			connectionId: owner.connectionId,
			expectedExecutionHostId: `ssh:${encodeURIComponent(owner.connectionId)}`
		};
		case "runtime": {
			const host = parseExecutionHostId(owner.executionHostId);
			return {
				settings: { activeRuntimeEnvironmentId: owner.environmentId },
				...host?.kind === "ssh" ? { expectedExecutionHostId: host.id } : { expectedExecutionHostId: "local" }
			};
		}
		case "unresolved": return null;
	}
}
function requireMatchingFileExplorerOperationRoute(worktreeId, expectedOwner) {
	if (!expectedOwner || expectedOwner.kind === "unresolved") throw new Error(getFileExplorerOwnerUnresolvedMessage());
	const currentOwner = getFileExplorerOperationOwner(worktreeId);
	if (JSON.stringify(currentOwner) !== JSON.stringify(expectedOwner)) throw new Error(getFileExplorerOwnerUnresolvedMessage());
	const route = getFileExplorerOperationRoute(expectedOwner);
	if (!route) throw new Error(getFileExplorerOwnerUnresolvedMessage());
	return route;
}
function captureFileExplorerOperationGuard(worktreeId, expectedOwner) {
	if (!worktreeId) throw new Error(getFileExplorerOwnerUnresolvedMessage());
	const route = requireMatchingFileExplorerOperationRoute(worktreeId, expectedOwner);
	const operationRoute = getFileExplorerGenerationRoute(expectedOwner);
	if (!operationRoute) throw new Error(getFileExplorerOwnerUnresolvedMessage());
	const generationGuard = captureWorktreeOperationGenerationGuard(useAppStore.getState, worktreeId, operationRoute, () => new Error(getFileExplorerOwnerUnresolvedMessage()), () => getFileExplorerGenerationRoute(getFileExplorerOperationOwner(worktreeId)));
	const expectedSshConnectionGeneration = getExpectedSshConnectionGeneration(useAppStore.getState(), operationRoute);
	const operationHost = parseExecutionHostId(operationRoute.executionHostId);
	if (!operationHost) throw new Error(getFileExplorerOwnerUnresolvedMessage());
	if (operationHost?.kind === "ssh" && expectedSshConnectionGeneration === void 0) throw new Error(getFileExplorerOwnerUnresolvedMessage());
	const guardedRoute = {
		...route,
		expectedExecutionHostId: operationHost.kind === "ssh" ? operationHost.id : "local",
		...operationHost?.kind === "ssh" ? { expectedSshTargetId: operationHost.targetId } : {},
		...expectedSshConnectionGeneration === void 0 ? {} : { expectedSshConnectionGeneration }
	};
	return {
		route: guardedRoute,
		assertCurrent: () => {
			generationGuard.assertCurrent();
			if (getExpectedSshConnectionGeneration(useAppStore.getState(), operationRoute) !== expectedSshConnectionGeneration) throw new Error(getFileExplorerOwnerUnresolvedMessage());
			return guardedRoute;
		}
	};
}
function getExpectedSshConnectionGeneration(state, route) {
	const host = parseExecutionHostId(route.executionHostId);
	if (host?.kind !== "ssh") return;
	return route.runtimeEnvironmentId ? state.sshStateByEnvironment.get(route.runtimeEnvironmentId)?.connectionStates.get(host.targetId)?.connectionGeneration : state.sshConnectionStates.get(host.targetId)?.connectionGeneration;
}
function getFileExplorerGenerationRoute(owner) {
	switch (owner?.kind) {
		case "local": return {
			executionHostId: "local",
			runtimeEnvironmentId: null
		};
		case "ssh": return {
			executionHostId: `ssh:${encodeURIComponent(owner.connectionId)}`,
			runtimeEnvironmentId: null
		};
		case "runtime": return {
			executionHostId: owner.executionHostId,
			runtimeEnvironmentId: owner.environmentId
		};
		case "unresolved":
		case void 0: return null;
	}
}
function getFileExplorerOwnerUnresolvedMessage() {
	return translate("auto.components.right.sidebar.fileExplorerOperationOwner.unresolved", "Couldn't determine which host owns this workspace. Check the connection and try again.");
}
function operationOwnerFromHostId(hostId) {
	const parsed = parseExecutionHostId(hostId);
	switch (parsed?.kind) {
		case "local": return { kind: "local" };
		case "ssh": return {
			kind: "ssh",
			connectionId: parsed.targetId
		};
		case "runtime": return {
			kind: "runtime",
			environmentId: parsed.environmentId,
			executionHostId: hostId
		};
		case void 0: return { kind: "unresolved" };
	}
}
export { getFileExplorerOwnerUnresolvedMessage as a, getFileExplorerOperationRoute as i, getFileExplorerOperationOwner as n, requireMatchingFileExplorerOperationRoute as o, getFileExplorerOperationOwnerFromState as r, captureFileExplorerOperationGuard as t };
