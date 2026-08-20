import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as Settings } from "./settings-BX3azETW.js";
import { $h as filterEnabledTuiAgents, Du as getExplicitRuntimeEnvironmentIdForWorktree, Eu as getExecutionHostIdForWorktree, Gc as isTerminalLeafId, Kc as makePaneKey, Mr as resolveRuntimePaneTitleLeafResolution, Oa as focusTerminalTabSurface, Pm as FLOATING_TERMINAL_WORKTREE_ID, Pr as detectAgentSendTitleStatus, Ut as getConnectionIdFromState, Zh as DEFAULT_DISABLED_TUI_AGENTS, kr as deriveRunningAgentSendTargets, t as useAppStore, vd as parseWorkspaceKey } from "./store-CgXrfmaH.js";
import { O as resolveTerminalTitleAgentType, n as agentTypeToIconAgent, r as formatAgentTypeLabel, st as parseExecutionHostId, t as agentKindForAgentType } from "./agent-status-3vUKbY6l.js";
import { n as toast } from "./dist-DgqligFk.js";
import { a as track } from "./telemetry-ZyUPyKMD.js";
import { a as DropdownMenuLabel, i as DropdownMenuItem, l as DropdownMenuSeparator, u as DropdownMenuShortcut } from "./dropdown-menu-Dth6LPK-.js";
import { t as useShallow } from "./shallow-BpOhx1Gc.js";
import { t as getResolvedExecutionHostIdForWorktree } from "./resolved-worktree-execution-host-BcjAq7e6.js";
import { i as useOptionalShortcutLabel } from "./useShortcutLabel-C-KRYtlB.js";
import { a as lastEnteredDoneAt } from "./worktree-agent-rows-C1pW_DbE.js";
import { t as selectLivePtyIdsForWorktree } from "./worktree-card-status-inputs-DozvjAa5.js";
import { n as agentStateLabel, t as AgentStateDot } from "./AgentStateDot-DFt63YGw.js";
import { n as getAgentCatalog, t as AgentIcon } from "./agent-catalog-CBF2CV5Q.js";
import { n as useNow, t as useWorktreeAgentRows } from "./useWorktreeAgentRows-DfUM0dP9.js";
import { t as useDetectedAgents } from "./useDetectedAgents-KkNokXI_.js";
import { t as launchAgentInNewTab } from "./launch-agent-in-new-tab-44JGNfKl.js";
import { n as activeAgentNotesSendFailureMessage, t as sendNotesToActiveAgentSession } from "./active-agent-note-send-CsxZ0dL2.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
const AGENT_DETECTION_LOCAL_TARGET_KEY = "local";
function getLocalAgentDetectionTargetKey(worktreeId) {
	return worktreeId === "global-floating-terminal" ? `${AGENT_DETECTION_LOCAL_TARGET_KEY}:${encodeURIComponent(worktreeId)}:host` : AGENT_DETECTION_LOCAL_TARGET_KEY;
}
function getAgentDetectionTargetKeyForWorktree(state, worktreeId) {
	if (worktreeId === null) return AGENT_DETECTION_LOCAL_TARGET_KEY;
	if (parseWorkspaceKey(worktreeId)?.type === "folder") {
		const explicitRuntimeEnvironmentId = getExplicitRuntimeEnvironmentIdForWorktree(state, worktreeId);
		if (explicitRuntimeEnvironmentId) return `runtime:${explicitRuntimeEnvironmentId}`;
		if (getConnectionIdFromState(state, worktreeId) === void 0) return;
	} else if (getResolvedExecutionHostIdForWorktree(state, worktreeId) === null) return;
	const executionHost = parseExecutionHostId(getExecutionHostIdForWorktree(state, worktreeId));
	if (executionHost?.kind === "ssh") return `ssh:${executionHost.targetId}`;
	if (executionHost?.kind === "runtime") return `runtime:${executionHost.environmentId}`;
	return getLocalAgentDetectionTargetKey(worktreeId);
}
function parseAgentDetectionTargetKey(key) {
	if (key === void 0) return;
	if (key === "local") return { kind: "local" };
	if (key.startsWith(`local:`)) {
		const [encodedWorktreeId, encodedContextKey] = key.slice(`${AGENT_DETECTION_LOCAL_TARGET_KEY}:`.length).split(":");
		if (!encodedWorktreeId || !encodedContextKey) return { kind: "local" };
		try {
			return {
				kind: "local",
				worktreeId: decodeURIComponent(encodedWorktreeId),
				contextKey: decodeURIComponent(encodedContextKey)
			};
		} catch {
			return { kind: "local" };
		}
	}
	if (key.startsWith("ssh:")) return {
		kind: "ssh",
		connectionId: key.slice(4)
	};
	if (key.startsWith("runtime:")) return {
		kind: "runtime",
		environmentId: key.slice(8)
	};
	return { kind: "local" };
}
function useAgentDetectionTargetForWorktree(worktreeId) {
	const key = useAppStore((s) => getAgentDetectionTargetKeyForWorktree(s, worktreeId));
	return (0, import_react.useMemo)(() => parseAgentDetectionTargetKey(key), [key]);
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function getCatalogEntry(agent) {
	return getAgentCatalog().find((a) => a.id === agent) ?? null;
}
function orderAgents(defaultAgent, detected) {
	const inCatalogOrder = getAgentCatalog().filter((entry) => detected.includes(entry.id)).map((entry) => entry.id);
	if (!defaultAgent || defaultAgent === "blank" || !inCatalogOrder.includes(defaultAgent)) return inCatalogOrder;
	return [defaultAgent, ...inCatalogOrder.filter((id) => id !== defaultAgent)];
}
function shouldShowLaunchWatchdogTimeout({ hasPty }) {
	return !hasPty;
}
function getLaunchWatchdogTimeoutMessage(label) {
	return `Couldn't launch ${label} — the terminal did not start.`;
}
function getTerminalLaunchState(tabId) {
	const state = useAppStore.getState();
	const hasPtyBinding = (state.ptyIdsByTabId[tabId]?.length ?? 0) > 0;
	let stillOpen = false;
	let tabPtyId = null;
	for (const tabs of Object.values(state.tabsByWorktree)) {
		const tab = tabs.find((t) => t.id === tabId);
		if (tab) {
			stillOpen = true;
			tabPtyId = tab.ptyId;
			break;
		}
	}
	return {
		stillOpen,
		hasPty: hasPtyBinding || tabPtyId !== null
	};
}
async function waitForTerminalPty(tabId, timeoutMs) {
	const deadline = Date.now() + timeoutMs;
	while (Date.now() < deadline) {
		if (getTerminalLaunchState(tabId).hasPty) return true;
		await new Promise((resolve) => window.setTimeout(resolve, 100));
	}
	return getTerminalLaunchState(tabId).hasPty;
}
function QuickLaunchAgentMenuItemsInner({ worktreeId, groupId, onFocusTerminal, prompt, promptDelivery, launchSource, onPromptDelivered }) {
	const { detectedIds } = useDetectedAgents(useAgentDetectionTargetForWorktree(worktreeId));
	const defaultAgent = useAppStore((s) => s.settings?.defaultTuiAgent);
	const disabledAgents = useAppStore((s) => s.settings?.disabledTuiAgents ?? DEFAULT_DISABLED_TUI_AGENTS);
	const openSettingsPage = useAppStore((s) => s.openSettingsPage);
	const openSettingsTarget = useAppStore((s) => s.openSettingsTarget);
	const newAgentShortcut = useOptionalShortcutLabel("tab.newAgent");
	const openAgentSettings = (0, import_react.useCallback)(() => {
		openSettingsTarget({
			pane: "agents",
			repoId: null
		});
		openSettingsPage();
	}, [openSettingsPage, openSettingsTarget]);
	const runLaunch = (0, import_react.useCallback)((agent) => {
		const label = getCatalogEntry(agent)?.label ?? agent;
		const result = launchAgentInNewTab({
			agent,
			worktreeId,
			groupId,
			...prompt !== void 0 ? { prompt } : {},
			...promptDelivery !== void 0 ? { promptDelivery } : {},
			...launchSource !== void 0 ? { launchSource } : {},
			...onPromptDelivered !== void 0 ? { onPromptDelivered } : {}
		});
		if (!result) {
			toast.error(translate("auto.components.tab.bar.QuickLaunchButton.465e432ef1", "Could not build launch command for {{value0}}.", { value0: label }));
			return;
		}
		if (!result.tabId) return;
		onFocusTerminal(result.tabId);
		const launchedTabId = result.tabId;
		waitForTerminalPty(launchedTabId, 5e3).then((hasPty) => {
			if (hasPty) return;
			const launchState = getTerminalLaunchState(launchedTabId);
			if (!launchState.stillOpen) return;
			if (useAppStore.getState().activeWorktreeId !== worktreeId) return;
			if (!shouldShowLaunchWatchdogTimeout({ hasPty: launchState.hasPty })) return;
			toast.message(getLaunchWatchdogTimeoutMessage(label));
		});
	}, [
		worktreeId,
		groupId,
		onFocusTerminal,
		prompt,
		promptDelivery,
		launchSource,
		onPromptDelivered
	]);
	const enabledDetectedIds = detectedIds ? filterEnabledTuiAgents(detectedIds, disabledAgents) : [];
	const agents = detectedIds ? orderAgents(defaultAgent, enabledDetectedIds) : [];
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
		agents.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuItem, {
			disabled: true,
			className: "gap-2 rounded-[7px] px-2 py-1.5 text-[12px] leading-5 text-muted-foreground",
			children: detectedIds && detectedIds.length > 0 ? translate("auto.components.tab.bar.QuickLaunchButton.8dea9b5cdf", "No enabled agents") : translate("auto.components.tab.bar.QuickLaunchButton.e518f544b1", "No agents detected")
		}) : null,
		agents.map((agent) => {
			const label = getCatalogEntry(agent)?.label ?? agent;
			const showsDefaultAgentShortcut = newAgentShortcut !== null && defaultAgent !== "blank" && agent === defaultAgent;
			return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
				onSelect: () => runLaunch(agent),
				className: "gap-2 rounded-[7px] px-2 py-1.5 text-[12px] leading-5 font-medium",
				title: translate("auto.components.tab.bar.QuickLaunchButton.ec2adf093e", "Launch {{value0}} in a new terminal", { value0: label }),
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentIcon, {
						agent,
						size: 14
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "flex-1",
						children: label
					}),
					showsDefaultAgentShortcut ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuShortcut, { children: newAgentShortcut }) : null
				]
			}, agent);
		}),
		/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
			onSelect: openAgentSettings,
			className: "gap-2 rounded-[7px] px-2 py-1.5 text-[12px] leading-5 font-medium text-muted-foreground",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Settings, { className: "size-4" }), translate("auto.components.tab.bar.QuickLaunchButton.348a04c1ad", "Agent settings…")]
		})
	] });
}
const QuickLaunchAgentMenuItems = import_react.memo(QuickLaunchAgentMenuItemsInner);
function detectTitleHintPaneEvidence(paneTitleResolution, tabTitle) {
	if (paneTitleResolution.title !== null) {
		const status$1 = detectAgentSendTitleStatus(paneTitleResolution.title);
		return status$1 ? {
			status: status$1,
			title: paneTitleResolution.title
		} : null;
	}
	if (paneTitleResolution.hasAnyPaneTitle) return null;
	const status = detectAgentSendTitleStatus(tabTitle);
	return status ? {
		status,
		title: tabTitle
	} : null;
}
function deriveNotesSendAgentTargets(state, worktreeId, now = Date.now()) {
	const targets = deriveRunningAgentSendTargets(state, worktreeId, now).map((target) => ({
		paneKey: target.paneKey,
		tabId: target.tabId,
		leafId: target.leafId,
		agentType: resolveNotesTargetAgentType(target.entry.agentType, target.tab.launchAgent),
		tabTitle: target.tab.title,
		status: target.status,
		...target.disabledReason ? { disabledReason: target.disabledReason } : {}
	}));
	for (const tab of state.tabsByWorktree[worktreeId] ?? []) {
		const titleHintTarget = deriveTitleHintAgentTarget(state, tab);
		if (!titleHintTarget) continue;
		if (tab.launchAgent) mergeLaunchAgentTitleTarget(targets, titleHintTarget);
		else mergeManualAgentTitleTarget(targets, titleHintTarget);
	}
	return targets;
}
function resolveNotesTargetAgentType(entryAgentType, launchAgent) {
	if (entryAgentType && entryAgentType !== "unknown") return entryAgentType;
	return launchAgent ?? entryAgentType;
}
function deriveTitleHintAgentTarget(state, tab) {
	const layout = state.terminalLayoutsByTabId[tab.id];
	const leafId = layout?.activeLeafId;
	if (!leafId || !isTerminalLeafId(leafId)) return null;
	const ptyId = layout.ptyIdsByLeafId?.[leafId] ?? null;
	if (!ptyId || !state.ptyIdsByTabId[tab.id]?.includes(ptyId)) return null;
	const paneTitles = state.runtimePaneTitlesByTabId[tab.id];
	const titleEvidence = detectTitleHintPaneEvidence(resolveRuntimePaneTitleLeafResolution(layout, paneTitles, leafId), tab.title);
	if (!titleEvidence) return null;
	const disabledReason = titleEvidence.status === "permission" ? "Agent needs permission" : void 0;
	return {
		paneKey: makePaneKey(tab.id, leafId),
		tabId: tab.id,
		leafId,
		agentType: tab.launchAgent ?? resolveTerminalTitleAgentType(titleEvidence.title),
		tabTitle: tab.title,
		status: disabledReason ? "disabled" : "eligible",
		...disabledReason ? { disabledReason } : {}
	};
}
function mergeManualAgentTitleTarget(targets, target) {
	if (targets.some((existing) => existing.tabId === target.tabId)) return;
	targets.push(target);
}
function mergeLaunchAgentTitleTarget(targets, target) {
	const samePaneIndex = targets.findIndex((existing) => existing.paneKey === target.paneKey);
	if (samePaneIndex !== -1) {
		const existing = targets[samePaneIndex];
		if (existing.status === "eligible" || existing.disabledReason === "Agent needs permission") return;
		targets[samePaneIndex] = {
			...target,
			agentType: existing.agentType && existing.agentType !== "unknown" ? existing.agentType : target.agentType,
			tabTitle: existing.tabTitle || target.tabTitle
		};
		return;
	}
	if (targets.some((existing) => existing.tabId === target.tabId && (existing.status === "eligible" || existing.disabledReason === "Agent needs permission"))) return;
	targets.push(target);
}
function ReviewNotesSendMenuContent({ worktreeId, groupId, prompt, promptDelivery = "submit-after-ready", launchSource = "notes_send", onPromptDelivered }) {
	const hasPrompt = prompt.trim().length > 0;
	const agentStatusByPaneKey = useAppStore((s) => s.agentStatusByPaneKey);
	const tabsByWorktree = useAppStore((s) => s.tabsByWorktree);
	const terminalLayoutsByTabId = useAppStore((s) => s.terminalLayoutsByTabId);
	const ptyIdsByTabId = useAppStore(useShallow((s) => selectLivePtyIdsForWorktree(s, worktreeId)));
	const runtimePaneTitlesByTabId = useAppStore((s) => s.runtimePaneTitlesByTabId);
	const agentStatusEpoch = useAppStore((s) => s.agentStatusEpoch);
	const agentRows = useWorktreeAgentRows(worktreeId);
	const now = useNow(3e4);
	const sendTargets = (0, import_react.useMemo)(() => {
		return deriveNotesSendAgentTargets({
			agentStatusByPaneKey,
			tabsByWorktree,
			terminalLayoutsByTabId,
			ptyIdsByTabId,
			runtimePaneTitlesByTabId
		}, worktreeId);
	}, [
		agentStatusEpoch,
		agentStatusByPaneKey,
		tabsByWorktree,
		terminalLayoutsByTabId,
		runtimePaneTitlesByTabId,
		ptyIdsByTabId,
		worktreeId
	]);
	const orderedSendTargets = (0, import_react.useMemo)(() => orderSendTargetsByWorktreeAgentRows(sendTargets, agentRows), [agentRows, sendTargets]);
	const runNotesSend = (0, import_react.useCallback)((send, onSent, options = {}) => {
		const pending = toast.loading(translate("auto.components.editor.ReviewNotesSendMenuContent.50f7e753ea", "Sending notes..."));
		send().then((result) => {
			if (result.status === "sent") {
				onSent();
				toast.success(translate("auto.components.editor.ReviewNotesSendMenuContent.bb9c69a0c9", "Notes sent."));
				return;
			}
			toast.message(activeAgentNotesSendFailureMessage(result.status, { explicitTarget: options.explicitTarget }));
		}).catch((error) => {
			console.error("Failed to send notes:", error);
			toast.error(translate("auto.components.editor.ReviewNotesSendMenuContent.f5096c6e4e", "Could not send notes."));
		}).finally(() => {
			toast.dismiss(pending);
		});
	}, []);
	const sendToAgentTarget = (0, import_react.useCallback)((target) => {
		if (!hasPrompt || target.status !== "eligible") return;
		const currentEligibility = resolveCurrentSendTargetEligibility(target, worktreeId);
		if (currentEligibility.status !== "eligible") {
			toast.message(currentEligibility.disabledReason);
			return;
		}
		runNotesSend(() => sendNotesToActiveAgentSession({
			worktreeId,
			prompt,
			noteTarget: {
				tabId: target.tabId,
				leafId: target.leafId
			}
		}), () => {
			onPromptDelivered?.();
			track("agent_prompt_sent", {
				agent_kind: agentKindForAgentType(target.agentType),
				launch_source: launchSource,
				request_kind: "followup"
			});
		}, { explicitTarget: true });
	}, [
		hasPrompt,
		runNotesSend,
		worktreeId,
		prompt,
		onPromptDelivered,
		launchSource
	]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
		/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuLabel, { children: translate("auto.components.editor.ReviewNotesSendMenuContent.03378aea75", "Send notes to") }),
		orderedSendTargets.map(({ target, agent }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentTargetMenuItem, {
			target,
			agent,
			now,
			disabled: !hasPrompt || target.status !== "eligible",
			onSend: sendToAgentTarget
		}, target.paneKey)),
		/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {}),
		/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuLabel, { children: translate("auto.components.editor.ReviewNotesSendMenuContent.a49800405b", "New agent") }),
		/* @__PURE__ */ (0, import_jsx_runtime.jsx)(QuickLaunchAgentMenuItems, {
			worktreeId,
			groupId,
			onFocusTerminal: focusTerminalTabSurface,
			prompt,
			promptDelivery,
			launchSource,
			onPromptDelivered
		})
	] });
}
function resolveCurrentSendTargetEligibility(target, worktreeId) {
	const currentTarget = deriveNotesSendAgentTargets(useAppStore.getState(), worktreeId).find((candidate) => candidate.paneKey === target.paneKey);
	if (currentTarget) return currentTarget.status === "eligible" ? { status: "eligible" } : {
		status: "disabled",
		disabledReason: currentTarget.disabledReason ?? "Terminal is no longer available"
	};
	return {
		status: "disabled",
		disabledReason: "Terminal is no longer available"
	};
}
function AgentTargetMenuItem({ target, agent, now, disabled, onSend }) {
	const tabTitle = target.tabTitle.trim();
	const state = asDotState(agent?.state ?? "idle");
	const timeAgo = agent ? formatAgentRelativeTime(agent, now) : null;
	const secondaryParts = [
		agentStateLabel(state),
		...timeAgo ? [timeAgo] : [],
		...tabTitle ? [tabTitle] : []
	];
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
		disabled,
		onSelect: () => onSend(target),
		title: target.status === "disabled" ? target.disabledReason : void 0,
		className: "min-w-[240px] gap-2 rounded-[7px] px-2 py-1.5 text-[12px] leading-5 font-medium",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentStateDot, {
				state,
				size: "sm",
				className: "shrink-0"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentIcon, {
				agent: agentTypeToIconAgent(target.agentType ?? agent?.agentType),
				size: 14
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
				className: "grid min-w-0 flex-1 text-left",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "truncate",
					children: formatAgentTypeLabel(target.agentType ?? agent?.agentType)
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "truncate text-[11px] font-normal text-muted-foreground",
					children: secondaryParts.join(" · ")
				})]
			})
		]
	});
}
function orderSendTargetsByWorktreeAgentRows(sendTargets, agentRows) {
	const targetsByPaneKey = new Map(sendTargets.map((target) => [target.paneKey, target]));
	const usedPaneKeys = /* @__PURE__ */ new Set();
	const ordered = [];
	for (const agent of agentRows) {
		const target = targetsByPaneKey.get(agent.paneKey);
		if (!target) continue;
		ordered.push({
			target: {
				...target,
				agentType: agent.agentType
			},
			agent
		});
		usedPaneKeys.add(target.paneKey);
	}
	for (const target of sendTargets) if (!usedPaneKeys.has(target.paneKey)) ordered.push({
		target,
		agent: null
	});
	return ordered;
}
function asDotState(state) {
	switch (state) {
		case "working":
		case "blocked":
		case "waiting":
		case "done":
		case "idle": return state;
	}
	return "idle";
}
function formatAgentRelativeTime(agent, now) {
	const doneAt = lastEnteredDoneAt(agent);
	if (doneAt !== null) return `${formatTimeAgo(doneAt, now)}`;
	const startedAt = agent.startedAt > 0 ? agent.startedAt : agent.entry.stateStartedAt;
	return startedAt > 0 ? `${formatTimeAgo(startedAt, now)}` : null;
}
function formatTimeAgo(ts, now) {
	const delta = now - ts;
	if (delta < 6e4) return "just now";
	const minutes = Math.floor(delta / 6e4);
	if (minutes < 60) return `${minutes}m ago`;
	const hours = Math.floor(minutes / 60);
	if (hours < 24) return `${hours}h ago`;
	return `${Math.floor(hours / 24)}d ago`;
}
export { QuickLaunchAgentMenuItems as n, useAgentDetectionTargetForWorktree as r, ReviewNotesSendMenuContent as t };
