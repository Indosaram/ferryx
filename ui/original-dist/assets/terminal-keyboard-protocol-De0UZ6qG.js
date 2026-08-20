import { Eu as getExecutionHostIdForWorktree, Io as getRemoteRuntimePtyEnvironmentId, Ou as getRuntimeEnvironmentIdForWorktree, Ut as getConnectionIdFromState, Vp as isWslUncPath } from "./store-CgXrfmaH.js";
import { dt as TUI_AGENT_CONFIG, o as resolveCommittedTitleAgentType, st as parseExecutionHostId } from "./agent-status-3vUKbY6l.js";
import { r as isLocalNativeWindowsConpty } from "./windows-pty-compatibility-XujC9UTf.js";
import { t as isWslShellOverride } from "./terminal-paste-runtime-pAfrhxQJ.js";
function resolveWindowsShiftEnterEncoding(signals) {
	if (signals.foreground?.shellForeground) return "alt-enter";
	const agent = signals.foreground?.routingTrusted === true ? signals.foreground.agent : null;
	return agent ? TUI_AGENT_CONFIG[agent].windowsShiftEnterEncoding ?? "alt-enter" : "alt-enter";
}
function resolveWindowsShiftEnterEncodingForPane(state, paneKey, terminalTitle) {
	const foreground = state.paneForegroundAgentByPaneKey[paneKey];
	const encoding = resolveWindowsShiftEnterEncoding({
		foreground: state.paneForegroundAgentByPaneKey[paneKey],
		launchAgentType: state.agentLaunchConfigByPaneKey[paneKey]?.identity.agentType
	});
	if (encoding === "csi-u" || !terminalTitle || foreground?.routingTrusted === true || foreground?.routingRevoked === true || foreground?.shellForeground === true) return encoding;
	const titleAgent = resolveCommittedTitleAgentType(terminalTitle);
	return titleAgent ? TUI_AGENT_CONFIG[titleAgent].windowsShiftEnterEncoding ?? "alt-enter" : "alt-enter";
}
function agentAcceptsCtrlEnterCsiU(agent) {
	return agent !== null && TUI_AGENT_CONFIG[agent].ctrlEnterEncoding === "csi-u";
}
function hasCtrlEnterCsiUAuthorityForPane(state, paneKey, terminalTitle) {
	const foreground = state.paneForegroundAgentByPaneKey[paneKey];
	if (foreground?.shellForeground === true || foreground?.routingRevoked === true) return false;
	if (foreground?.routingTrusted === true) return agentAcceptsCtrlEnterCsiU(foreground.agent);
	const titleAgent = terminalTitle ? resolveCommittedTitleAgentType(terminalTitle) : null;
	if (foreground?.agent != null && foreground.agent !== titleAgent) return false;
	return agentAcceptsCtrlEnterCsiU(titleAgent);
}
function resolveTerminalInputHostPlatform(args) {
	const authoritativePlatform = args.transport?.getRemotePlatform?.();
	if (authoritativePlatform) return authoritativePlatform;
	const transportConnectionId = args.transport?.getConnectionId?.();
	const connectionId = transportConnectionId === void 0 ? getConnectionIdFromState(args.state, args.worktreeId) : transportConnectionId;
	if (connectionId) return args.state.sshConnectionStates.get(connectionId)?.remotePlatform ?? "linux";
	const ptyId = args.transport?.getPtyId?.() ?? null;
	const runtimeEnvironmentId = args.transport?.getRuntimeEnvironmentId?.() ?? (ptyId ? getRemoteRuntimePtyEnvironmentId(ptyId) : null);
	const transportExecutionHost = parseExecutionHostId(args.transport?.getExecutionHostId?.());
	if (runtimeEnvironmentId && transportExecutionHost?.kind === "ssh") return args.state.sshStateByEnvironment.get(runtimeEnvironmentId)?.connectionStates.get(transportExecutionHost.targetId)?.remotePlatform ?? "linux";
	if (runtimeEnvironmentId) return args.state.runtimeStatusByEnvironmentId.get(runtimeEnvironmentId)?.status?.hostPlatform ?? args.clientPlatform;
	const localSessionMetadata = args.transport?.getLocalSessionMetadata?.();
	if (ptyId !== null && localSessionMetadata != null) {
		const isWslSession = isWslUncPath(localSessionMetadata.cwd ?? "") || isWslShellOverride(localSessionMetadata.shellOverride);
		return args.clientPlatform === "win32" && isWslSession ? "linux" : args.clientPlatform;
	}
	const host = parseExecutionHostId(getExecutionHostIdForWorktree(args.state, args.worktreeId));
	if (host?.kind === "ssh") {
		const ownerEnvironmentId = getRuntimeEnvironmentIdForWorktree(args.state, args.worktreeId);
		return ownerEnvironmentId ? args.state.sshStateByEnvironment.get(ownerEnvironmentId)?.connectionStates.get(host.targetId)?.remotePlatform ?? "linux" : args.state.sshConnectionStates.get(host.targetId)?.remotePlatform ?? "linux";
	}
	if (host?.kind === "runtime") return args.state.runtimeStatusByEnvironmentId.get(host.environmentId)?.status?.hostPlatform ?? args.clientPlatform;
	return args.clientPlatform;
}
function prefersKittyKeyboardDespiteWindowsConpty(agent) {
	return agent === "grok";
}
function shouldDisableKittyKeyboardForTerminal(context) {
	if (prefersKittyKeyboardDespiteWindowsConpty(context.tuiAgent)) return false;
	return isLocalNativeWindowsConpty(context);
}
function buildTerminalKeyboardProtocolOptions(context) {
	if (!shouldDisableKittyKeyboardForTerminal(context)) return {};
	return { vtExtensions: { kittyKeyboard: false } };
}
export { resolveWindowsShiftEnterEncodingForPane as a, hasCtrlEnterCsiUAuthorityForPane as i, shouldDisableKittyKeyboardForTerminal as n, resolveTerminalInputHostPlatform as r, buildTerminalKeyboardProtocolOptions as t };
