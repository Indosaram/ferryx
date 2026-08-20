import { a as translate } from "./jsx-runtime-Cv_nyRjc.js";
import { qg as isGitRepoKind } from "./store-CgXrfmaH.js";
import { nt as isRuntimeOwnedSshTargetId } from "./agent-status-3vUKbY6l.js";
import { n as isConnectingSshStatus } from "./ssh-connection-recoverability-CNHp0WBp.js";
function isSshConnectInProgress(status) {
	return isConnectingSshStatus(status);
}
function getSelectedRepoSshGate(input) {
	const selectedRepoConnectionId = isRuntimeOwnedSshTargetId(input.connectionId) ? null : input.connectionId ?? null;
	const selectedRepoSshStatus = selectedRepoConnectionId ? input.status ?? null : null;
	return {
		selectedRepoConnectionId,
		selectedRepoSshStatus,
		selectedRepoRequiresConnection: selectedRepoConnectionId !== null && selectedRepoSshStatus !== "connected",
		selectedRepoConnectInProgress: isSshConnectInProgress(selectedRepoSshStatus)
	};
}
function canUseRepoBackedComposerSources(input) {
	return !input.connectionId || isRuntimeOwnedSshTargetId(input.connectionId) || input.status === "connected";
}
function getRepoHeaderCreateState(input) {
	if (!isGitRepoKind(input.repo)) return {
		disabled: false,
		tooltip: translate("auto.components.sidebar.repo.header.create.state.62e71f2d5d", "Create workspace for {{value0}}", { value0: input.label }),
		ariaLabel: translate("auto.components.sidebar.repo.header.create.state.62e71f2d5d", "Create workspace for {{value0}}", { value0: input.label }),
		requiresSshReconnect: false
	};
	if (getSelectedRepoSshGate({
		connectionId: input.repo.connectionId,
		status: input.repo.connectionId ? input.sshStatus : null
	}).selectedRepoRequiresConnection) return {
		disabled: true,
		tooltip: translate("auto.components.sidebar.repo.header.create.state.6d022563a8", "Reconnect SSH target before creating workspaces"),
		ariaLabel: translate("auto.components.sidebar.repo.header.create.state.3a70acd808", "Reconnect SSH target before creating workspaces for {{value0}}", { value0: input.label }),
		requiresSshReconnect: true
	};
	return {
		disabled: false,
		tooltip: translate("auto.components.sidebar.repo.header.create.state.992cfbc44b", "Create new worktree for {{value0}}", { value0: input.label }),
		ariaLabel: translate("auto.components.sidebar.repo.header.create.state.992cfbc44b", "Create new worktree for {{value0}}", { value0: input.label }),
		requiresSshReconnect: false
	};
}
export { isSshConnectInProgress as i, canUseRepoBackedComposerSources as n, getSelectedRepoSshGate as r, getRepoHeaderCreateState as t };
