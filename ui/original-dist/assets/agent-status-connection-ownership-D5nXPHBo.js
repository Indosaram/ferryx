import { Ip as isWindowsAbsolutePathLike, Jc as parsePaneKey, Ro as parseRemoteRuntimePtyId, dd as parseAppSshPtyId } from "./store-CgXrfmaH.js";
import { m as CLIENT_PLATFORM } from "./native-chat-session-option-cache-DGE3h47U.js";
function getAgentLaunchPlatformForRepo(repo, projectRuntime) {
	if (!repo.connectionId) {
		if (projectRuntime?.status === "repair-required") return projectRuntime.repair.preferredRuntime.kind === "wsl" ? "linux" : CLIENT_PLATFORM;
		if (projectRuntime?.status === "resolved" && projectRuntime.runtime.kind === "wsl") return "linux";
		return CLIENT_PLATFORM;
	}
	return isWindowsAbsolutePathLike(repo.path) ? "win32" : "linux";
}
function repoIsRemote(repo) {
	return Boolean(repo.connectionId);
}
function resolveAgentStatusConnectionRouting(args) {
	const ptyId = args.ptyId?.trim();
	if (!ptyId) return;
	const expectedConnectionId = args.expectedConnectionId?.trim() || args.expectedConnectionId;
	const sshPty = parseAppSshPtyId(ptyId);
	if (sshPty) {
		if (typeof args.runtimeEnvironmentId === "string" || expectedConnectionId === null || typeof expectedConnectionId === "string" && expectedConnectionId !== sshPty.connectionId) return;
		return { connectionId: sshPty.connectionId };
	}
	if (ptyId.startsWith("ssh:")) return;
	const runtimePty = parseRemoteRuntimePtyId(ptyId);
	if (runtimePty?.handle) {
		if (typeof expectedConnectionId === "string" || args.runtimeEnvironmentId === null || typeof args.runtimeEnvironmentId === "string" && runtimePty.environmentId !== null && runtimePty.environmentId !== args.runtimeEnvironmentId) return;
		return { connectionId: null };
	}
	if (ptyId.startsWith("remote:")) return;
	if (typeof expectedConnectionId === "string") return;
	return { connectionId: null };
}
function resolveLiveAgentStatusConnectionRouting(args) {
	const pane = parsePaneKey(args.paneKey);
	if (!pane || !args.state.ptyIdsByTabId?.[pane.tabId]?.includes(args.ptyId) || args.state.terminalLayoutsByTabId?.[pane.tabId]?.ptyIdsByLeafId?.[pane.leafId] !== args.ptyId) return;
	const routing = resolveAgentStatusConnectionRouting(args);
	if (!routing) return;
	if (routing.connectionId !== null && (args.state.sshConnectionStates.get(routing.connectionId)?.status !== "connected" || routing.connectionId in args.state.transientClearedAgentStatusConnectionIds)) return;
	return routing;
}
export { repoIsRemote as n, getAgentLaunchPlatformForRepo as r, resolveLiveAgentStatusConnectionRouting as t };
