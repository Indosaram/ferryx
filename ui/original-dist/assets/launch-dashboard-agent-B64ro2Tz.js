import { Eu as getExecutionHostIdForWorktree, eg as isTuiAgentEnabled, t as useAppStore } from "./store-CgXrfmaH.js";
import { t as launchAgentInNewTab } from "./launch-agent-in-new-tab-44JGNfKl.js";
function launchDashboardAgent({ worktreeId, agent }) {
	const state = useAppStore.getState();
	const executionHostId = getExecutionHostIdForWorktree(state, worktreeId);
	if (!state.getKnownWorktreeById(worktreeId, executionHostId) || !isTuiAgentEnabled(agent, state.settings?.disabledTuiAgents)) return false;
	state.setActiveWorktree(worktreeId, executionHostId);
	return launchAgentInNewTab({
		agent,
		worktreeId,
		launchSource: "unknown"
	}) !== null;
}
export { launchDashboardAgent as t };
