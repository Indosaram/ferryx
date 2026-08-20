import { $f as toRuntimeWorktreeSelector, ku as getSettingsForWorktreeRuntimeOwner, lp as RuntimeRpcCallError, op as getActiveRuntimeTarget, rp as callRuntimeRpc, t as useAppStore } from "./store-CgXrfmaH.js";
import { n as POST_PASTE_SUBMIT_DELAY_MS, o as sanitizeBracketedPasteContent, t as BRACKETED_PASTE_BEGIN } from "./agent-paste-draft-C2PA7vXu.js";
import { n as BRACKETED_PASTE_END } from "./terminal-pty-input-transaction-2UskR-Bm.js";
var ACTIVE_AGENT_TERMINAL_LIST_LIMIT = 200;
function getActiveTerminalNoteTarget(state, worktreeId) {
	if (state.activeWorktreeId !== worktreeId) return null;
	const tabId = state.activeTabType === "terminal" ? state.activeTabId ?? state.activeTabIdByWorktree[worktreeId] : state.activeTabIdByWorktree[worktreeId];
	if (!tabId || !(state.tabsByWorktree[worktreeId] ?? []).some((tab) => tab.id === tabId)) return null;
	const leafId = state.terminalLayoutsByTabId[tabId]?.activeLeafId;
	return leafId ? {
		tabId,
		leafId
	} : null;
}
async function findActiveRuntimeTerminal(runtimeTarget, worktreeId, noteTarget, timeoutMs) {
	const { terminals } = await callRuntimeRpc(runtimeTarget, "terminal.list", {
		worktree: toRuntimeWorktreeSelector(worktreeId),
		limit: ACTIVE_AGENT_TERMINAL_LIST_LIMIT,
		includeVisualLayouts: false
	}, { timeoutMs });
	return terminals.find((terminal) => terminal.tabId === noteTarget.tabId && terminal.leafId === noteTarget.leafId) ?? null;
}
const ACTIVE_AGENT_SEND_RPC_TIMEOUT_MS = 15e3;
async function getTerminalAgentSendReadiness(runtimeTarget, terminalHandle, options) {
	try {
		const { agentStatus } = await callRuntimeRpc(runtimeTarget, "terminal.agentStatus", { terminal: terminalHandle }, { timeoutMs: ACTIVE_AGENT_SEND_RPC_TIMEOUT_MS });
		if (!agentStatus.isRunningAgent) return {
			status: "no-agent",
			supportsGuardedSend: true
		};
		if (agentStatus.status === "permission") return {
			status: "permission",
			supportsGuardedSend: true
		};
		return {
			status: "sendable",
			supportsGuardedSend: true
		};
	} catch (error) {
		if (error instanceof RuntimeRpcCallError && error.code === "method_not_found") {
			if (!options.allowLegacyFallback) return {
				status: "status-unavailable",
				supportsGuardedSend: false
			};
			return {
				status: await getLegacyTerminalAgentSendStatus(runtimeTarget, terminalHandle),
				supportsGuardedSend: false
			};
		}
		if (isRuntimeTerminalUnavailable(error)) return {
			status: "no-active-terminal",
			supportsGuardedSend: false
		};
		throw error;
	}
}
async function getLegacyTerminalAgentSendStatus(runtimeTarget, terminalHandle) {
	try {
		const { isRunningAgent } = await callRuntimeRpc(runtimeTarget, "terminal.isRunningAgent", { terminal: terminalHandle }, { timeoutMs: ACTIVE_AGENT_SEND_RPC_TIMEOUT_MS });
		return isRunningAgent ? "sendable" : "no-agent";
	} catch (error) {
		if (isRuntimeTerminalUnavailable(error)) return "no-active-terminal";
		throw error;
	}
}
function isRuntimeTimeout(error) {
	return (error instanceof Error ? error.message : String(error)).includes("timeout");
}
function isRuntimeTerminalUnavailable(error) {
	const message = error instanceof Error ? error.message : String(error);
	return message.includes("terminal_handle_stale") || message.includes("terminal_exited") || message.includes("terminal_gone") || message.includes("no_active_terminal");
}
function isRuntimeTerminalNotWritable(error) {
	return (error instanceof Error ? error.message : String(error)).includes("terminal_not_writable");
}
function activeAgentNotesSendFailureMessage(status, options = {}) {
	const target = options.explicitTarget ? "selected" : "active";
	switch (status) {
		case "empty": return "No notes to send.";
		case "no-active-terminal": return options.explicitTarget ? "The selected terminal is no longer available." : "Open the agent terminal in this worktree, then send the notes again.";
		case "no-agent": return `The ${target} terminal is not a recognized agent session.`;
		case "permission": return options.explicitTarget ? "The selected agent needs permission." : "The active agent needs permission.";
		case "status-unavailable": return `The ${target} agent status could not be verified.`;
		case "not-ready": return `The ${target} agent was not ready for input yet.`;
		case "not-writable": return `The ${target} terminal did not accept the notes.`;
		case "partial-submit-failed": return options.explicitTarget ? "The notes may already be pasted in the selected terminal, but Orca could not submit them." : "The notes may already be pasted in the active terminal, but Orca could not submit them.";
		case "sent": return "";
	}
}
var ACTIVE_AGENT_SEND_TIMEOUT_MS = 8e3;
var ORCA_DESKTOP_TERMINAL_CLIENT = {
	id: "orca-desktop",
	type: "desktop"
};
async function sendNotesToActiveAgentSession({ worktreeId, prompt, noteTarget: explicitNoteTarget, timeoutMs }) {
	const trimmedPrompt = prompt.trim();
	if (!trimmedPrompt) return { status: "empty" };
	const state = useAppStore.getState();
	const noteTarget = explicitNoteTarget ?? getActiveTerminalNoteTarget(state, worktreeId);
	if (!noteTarget) return { status: "no-active-terminal" };
	const runtimeTarget = getActiveRuntimeTarget(getSettingsForWorktreeRuntimeOwner(state, worktreeId));
	const terminal = await findActiveRuntimeTerminal(runtimeTarget, worktreeId, noteTarget, ACTIVE_AGENT_SEND_RPC_TIMEOUT_MS);
	if (!terminal) return { status: "no-active-terminal" };
	if (explicitNoteTarget) return await sendPromptToExplicitAgentTarget(runtimeTarget, terminal.handle, trimmedPrompt);
	const effectiveTimeoutMs = timeoutMs ?? ACTIVE_AGENT_SEND_TIMEOUT_MS;
	const initialAgentStatus = await getTerminalAgentSendReadiness(runtimeTarget, terminal.handle, { allowLegacyFallback: true });
	if (initialAgentStatus.status !== "sendable") return { status: initialAgentStatus.status };
	try {
		const { wait } = await callRuntimeRpc(runtimeTarget, "terminal.wait", {
			terminal: terminal.handle,
			for: "tui-idle",
			timeoutMs: effectiveTimeoutMs
		}, { timeoutMs: effectiveTimeoutMs + 5e3 });
		if (wait.status !== "running") return { status: "no-active-terminal" };
		if (wait.blockedReason) return { status: "permission" };
		if (!wait.satisfied) return { status: "not-ready" };
	} catch (error) {
		if (isRuntimeTerminalUnavailable(error)) return { status: "no-active-terminal" };
		if (isRuntimeTimeout(error)) return { status: "not-ready" };
		throw error;
	}
	const finalAgentStatus = await getTerminalAgentSendReadiness(runtimeTarget, terminal.handle, { allowLegacyFallback: true });
	if (finalAgentStatus.status !== "sendable") return { status: finalAgentStatus.status };
	if (finalAgentStatus.supportsGuardedSend) return await sendPromptWithGuardedPasteAndEnter(runtimeTarget, terminal.handle, trimmedPrompt, { allowLegacyFallback: false });
	return await sendPromptWithLegacyCombinedSend(runtimeTarget, terminal.handle, trimmedPrompt);
}
async function sendPromptWithLegacyCombinedSend(runtimeTarget, terminalHandle, prompt) {
	try {
		const { send } = await callRuntimeRpc(runtimeTarget, "terminal.send", {
			terminal: terminalHandle,
			text: prompt,
			enter: true,
			client: ORCA_DESKTOP_TERMINAL_CLIENT
		}, { timeoutMs: ACTIVE_AGENT_SEND_RPC_TIMEOUT_MS });
		return send.accepted ? { status: "sent" } : { status: "not-writable" };
	} catch (error) {
		if (isRuntimeTerminalUnavailable(error)) return { status: "no-active-terminal" };
		if (isRuntimeTerminalNotWritable(error)) return { status: "not-writable" };
		throw error;
	}
}
async function sendPromptWithGuardedPasteAndEnter(runtimeTarget, terminalHandle, prompt, options) {
	const initialAgentStatus = await getTerminalAgentSendReadiness(runtimeTarget, terminalHandle, { allowLegacyFallback: options.allowLegacyFallback });
	if (initialAgentStatus.status !== "sendable" && !(initialAgentStatus.status === "no-agent" && initialAgentStatus.supportsGuardedSend)) return { status: initialAgentStatus.status };
	const pastePayload = `${BRACKETED_PASTE_BEGIN}${sanitizeBracketedPasteContent(prompt)}${BRACKETED_PASTE_END}`;
	try {
		const { send } = await callRuntimeRpc(runtimeTarget, "terminal.send", {
			terminal: terminalHandle,
			text: pastePayload,
			requireAgentStatus: "sendable",
			client: ORCA_DESKTOP_TERMINAL_CLIENT
		}, { timeoutMs: ACTIVE_AGENT_SEND_RPC_TIMEOUT_MS });
		if (!send.accepted) {
			if (send.refusedReason === "permission") return { status: "permission" };
			if (send.refusedReason === "no-agent") return { status: "no-agent" };
			return { status: "not-writable" };
		}
	} catch (error) {
		if (isRuntimeTerminalUnavailable(error)) return { status: "no-active-terminal" };
		if (isRuntimeTerminalNotWritable(error)) return { status: "not-writable" };
		throw error;
	}
	await new Promise((resolve) => setTimeout(resolve, 50));
	try {
		const submitAgentStatus = await getTerminalAgentSendReadiness(runtimeTarget, terminalHandle, { allowLegacyFallback: options.allowLegacyFallback });
		if (submitAgentStatus.status !== "sendable" && !(submitAgentStatus.status === "no-agent" && submitAgentStatus.supportsGuardedSend)) return { status: "partial-submit-failed" };
	} catch (error) {
		if (isRuntimeTerminalUnavailable(error)) return { status: "partial-submit-failed" };
		throw error;
	}
	try {
		const { send } = await callRuntimeRpc(runtimeTarget, "terminal.send", {
			terminal: terminalHandle,
			enter: true,
			requireAgentStatus: "sendable",
			client: ORCA_DESKTOP_TERMINAL_CLIENT
		}, { timeoutMs: ACTIVE_AGENT_SEND_RPC_TIMEOUT_MS });
		return send.accepted ? { status: "sent" } : { status: "partial-submit-failed" };
	} catch (error) {
		if (isRuntimeTerminalUnavailable(error) || isRuntimeTerminalNotWritable(error)) return { status: "partial-submit-failed" };
		throw error;
	}
}
async function sendPromptToExplicitAgentTarget(runtimeTarget, terminalHandle, prompt) {
	return await sendPromptWithGuardedPasteAndEnter(runtimeTarget, terminalHandle, prompt, { allowLegacyFallback: false });
}
export { activeAgentNotesSendFailureMessage as n, getActiveTerminalNoteTarget as r, sendNotesToActiveAgentSession as t };
