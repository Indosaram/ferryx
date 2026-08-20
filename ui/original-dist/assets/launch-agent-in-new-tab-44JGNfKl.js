import { a as translate } from "./jsx-runtime-Cv_nyRjc.js";
import { $d as buildAgentDraftLaunchPlan, Fd as isNativeChatTranscriptLocalReadable, Fu as isWebTerminalSurfaceTabId, Kc as makePaneKey, Ld as resolveInitialNativeChatSessionOptions, Ou as getRuntimeEnvironmentIdForWorktree, Ut as getConnectionIdFromState, Wd as initialAgentTabViewModeProps, _h as resolveTuiAgentLaunchEnv, bo as reconcileTabOrder, cc as resolveLocalWindowsAgentStartupShell, da as getLocalProjectExecutionRuntimeContext, gh as resolveTuiAgentLaunchArgs, t as useAppStore, tf as buildAgentStartupPlan } from "./store-CgXrfmaH.js";
import { dt as TUI_AGENT_CONFIG } from "./agent-status-3vUKbY6l.js";
import { n as toast } from "./dist-DgqligFk.js";
import { n as tuiAgentToAgentKind } from "./agent-kind-Dfx6MnkP.js";
import { a as track } from "./telemetry-ZyUPyKMD.js";
import { P as toAgentLaunchPreferences, T as seedNativeChatLaunchDraftForAgentTab, c as createWebRuntimeAgentSessionTerminalWithLaunchDraft, d as isWebRuntimeSessionActive, s as createWebRuntimeAgentSessionTerminal, u as createWebRuntimeSessionTerminal, w as deliverLaunchPromptToAgentTab } from "./web-runtime-session-CN2syA39.js";
import { m as CLIENT_PLATFORM, n as seedNativeChatAppliedSessionOptions } from "./native-chat-session-option-cache-DGE3h47U.js";
import { n as repoIsRemote, r as getAgentLaunchPlatformForRepo, t as resolveLiveAgentStatusConnectionRouting } from "./agent-status-connection-ownership-D5nXPHBo.js";
function planLaunchAgentStartupPrompt(args) {
	const { base, prompt, promptDelivery, isFollowupPath } = args;
	const hasPrompt = prompt.length > 0;
	const launchEmpty = () => buildAgentStartupPlan({
		...base,
		prompt: "",
		allowEmptyPromptLaunch: true
	});
	const pasteAfterReady = (submit) => ({
		startupPlan: launchEmpty(),
		pasteDraftAfterLaunch: prompt,
		submitPastedPrompt: submit
	});
	if (hasPrompt && promptDelivery === "submit-after-ready") return pasteAfterReady(true);
	if (hasPrompt && promptDelivery === "draft") {
		const draftLaunchPlan = buildAgentDraftLaunchPlan({
			...base,
			draft: prompt
		});
		if (!draftLaunchPlan) return pasteAfterReady(false);
		return {
			startupPlan: {
				agent: draftLaunchPlan.agent,
				launchCommand: draftLaunchPlan.launchCommand,
				expectedProcess: draftLaunchPlan.expectedProcess,
				followupPrompt: null,
				launchConfig: draftLaunchPlan.launchConfig,
				...draftLaunchPlan.sessionOptions ? { sessionOptions: draftLaunchPlan.sessionOptions } : {},
				...draftLaunchPlan.startupCommandDelivery ? { startupCommandDelivery: draftLaunchPlan.startupCommandDelivery } : {},
				...draftLaunchPlan.env ? { env: draftLaunchPlan.env } : {}
			},
			pasteDraftAfterLaunch: null,
			submitPastedPrompt: false
		};
	}
	if (hasPrompt && isFollowupPath) return pasteAfterReady(false);
	return {
		startupPlan: buildAgentStartupPlan({
			...base,
			prompt: hasPrompt ? prompt : "",
			allowEmptyPromptLaunch: !hasPrompt
		}),
		pasteDraftAfterLaunch: null,
		submitPastedPrompt: false
	};
}
function createPasteReadinessTimeoutNotice(args) {
	let notified = false;
	return {
		wasNotified: () => notified,
		onTimeout: () => {
			const state = useAppStore.getState();
			const currentTab = (state.tabsByWorktree[args.worktreeId] ?? []).find((tab) => tab.id === args.tabId);
			if (currentTab?.ptyId === null) return;
			if (!currentTab || state.activeWorktreeId !== args.worktreeId) {
				notified = true;
				return;
			}
			toast.message(translate("auto.lib.launch.agent.in.new.tab.a5a1f7033f", "Your {{value0}} wasn't sent — paste it once the agent is ready.", { value0: args.submitted ? "prompt" : "notes" }));
			notified = true;
			track("agent_error", {
				error_class: "paste_readiness_timeout",
				agent_kind: tuiAgentToAgentKind(args.agent)
			});
		}
	};
}
function removeStaleLocalAgentTabsForWebHostLaunch(worktreeId) {
	const state = useAppStore.getState();
	for (const tab of state.tabsByWorktree[worktreeId] ?? []) if (tab.launchAgent && !isWebTerminalSurfaceTabId(tab.id)) state.closeTab(tab.id, { reason: "cleanup" });
}
function launchAgentInWebHostTab(args) {
	const { agent, worktreeId, environmentId, groupId, cwd, startupPlan, prompt, promptDelivery, pastePromptAfterReady, submitPastedPrompt, agentArgs, viewMode, onPromptDelivered } = args;
	const hasPrompt = prompt.length > 0;
	const launchPreferences = toAgentLaunchPreferences(startupPlan.sessionOptions);
	const structuredPromptDelivery = promptDelivery === "draft" ? "draft" : "auto-submit";
	removeStaleLocalAgentTabsForWebHostLaunch(worktreeId);
	const launch = {
		worktreeId,
		environmentId,
		targetGroupId: groupId,
		activate: true,
		...cwd?.trim() ? { cwd } : {},
		...viewMode ? { viewMode } : {},
		agentSessionKind: "fresh",
		...hasPrompt ? {
			launchAgent: agent,
			command: startupPlan.launchCommand,
			...startupPlan.env ? { env: startupPlan.env } : {},
			launchConfig: startupPlan.launchConfig,
			...startupPlan.startupCommandDelivery ? { startupCommandDelivery: startupPlan.startupCommandDelivery } : {}
		} : { agent },
		...hasPrompt && pastePromptAfterReady === null ? { prompt } : {},
		...hasPrompt && pastePromptAfterReady === null ? { promptDelivery: structuredPromptDelivery } : {},
		...agentArgs !== void 0 ? { agentArgs } : {},
		...launchPreferences ? { launchPreferences } : {}
	};
	const handleCreation = ({ outcome, promptDelivered }) => {
		removeStaleLocalAgentTabsForWebHostLaunch(worktreeId);
		if (outcome.status === "failed") {
			toast.error(outcome.message || translate("auto.lib.launch.agent.in.new.tab.11cce5cc77", "Could not launch {{value0}} in a new terminal.", { value0: agent }));
			return {
				delivered: false,
				failureNotified: true
			};
		}
		useAppStore.getState().setActiveTabType("terminal");
		if (hasPrompt && promptDelivered) onPromptDelivered?.();
		return {
			delivered: promptDelivered,
			failureNotified: false
		};
	};
	if (pastePromptAfterReady !== null) return createWebRuntimeAgentSessionTerminal({
		...launch,
		agent,
		promptAfterReady: pastePromptAfterReady,
		submitPrompt: submitPastedPrompt,
		forcePromptPaste: promptDelivery === "submit-after-ready"
	}).then(handleCreation);
	if (hasPrompt && promptDelivery === "draft") return createWebRuntimeAgentSessionTerminalWithLaunchDraft({
		...launch,
		agent,
		launchDraft: prompt
	}).then((outcome) => handleCreation({
		outcome,
		promptDelivered: outcome.status === "created"
	}));
	return createWebRuntimeSessionTerminal(launch).then((outcome) => handleCreation({
		outcome,
		promptDelivered: outcome.status === "created" && hasPrompt
	}));
}
function seedCommandCodeSubmittedPromptStatus(worktreeId, tabId, prompt) {
	const state = useAppStore.getState();
	const leafId = state.terminalLayoutsByTabId[tabId]?.activeLeafId;
	if (!leafId || !(state.tabsByWorktree[worktreeId] ?? []).some((tab) => tab.id === tabId)) return;
	const paneKey = makePaneKey(tabId, leafId);
	const ptyId = state.terminalLayoutsByTabId[tabId]?.ptyIdsByLeafId?.[leafId];
	if (!ptyId) return;
	const routing = resolveLiveAgentStatusConnectionRouting({
		state,
		paneKey,
		ptyId,
		expectedConnectionId: getConnectionIdFromState(state, worktreeId)
	});
	if (!routing) return;
	try {
		state.setAgentStatus(paneKey, {
			state: "working",
			prompt,
			agentType: "command-code"
		}, void 0, void 0, routing);
	} catch {}
}
function launchAgentInNewTab(args) {
	const { agent, worktreeId, groupId, prompt, agentArgs, initialCwd, promptDelivery = "auto-submit", launchSource, quickCommandLabel, launchPlatform, onPromptDelivered } = args;
	const store = useAppStore.getState();
	const worktree = store.allWorktrees?.().find((entry) => entry.id === worktreeId);
	const repo = worktree ? store.repos?.find((entry) => entry.id === worktree.repoId) : null;
	const resolvedLaunchPlatform = launchPlatform ?? (repo ? getAgentLaunchPlatformForRepo(repo, repo.connectionId ? void 0 : getLocalProjectExecutionRuntimeContext(store, worktreeId)) : CLIENT_PLATFORM);
	const isRemote = repo ? repoIsRemote(repo) : false;
	const queuedShell = resolveLocalWindowsAgentStartupShell({
		platform: resolvedLaunchPlatform,
		isRemote,
		terminalWindowsShell: store.settings?.terminalWindowsShell
	});
	const cmdOverrides = store.settings?.agentCmdOverrides ?? {};
	const effectiveAgentArgs = agentArgs !== void 0 ? agentArgs : resolveTuiAgentLaunchArgs(agent, store.settings?.agentDefaultArgs);
	const agentEnv = resolveTuiAgentLaunchEnv(agent, store.settings?.agentDefaultEnv);
	const trimmedPrompt = prompt?.trim() ?? "";
	const hasPrompt = trimmedPrompt.length > 0;
	const isFollowupPath = TUI_AGENT_CONFIG[agent].promptInjectionMode === "stdin-after-start";
	const initialViewModeOptions = {
		agent,
		promptDelivery: hasPrompt && isFollowupPath && promptDelivery === "auto-submit" ? "draft" : promptDelivery,
		launchDraftText: trimmedPrompt,
		nativeChatTranscriptIsLocalReadable: isNativeChatTranscriptLocalReadable(getConnectionIdFromState(store, worktreeId))
	};
	const initialViewModeProps = initialAgentTabViewModeProps(store.settings, initialViewModeOptions);
	const { startupPlan, pasteDraftAfterLaunch, submitPastedPrompt } = planLaunchAgentStartupPrompt({
		base: {
			agent,
			cmdOverrides,
			platform: resolvedLaunchPlatform,
			shell: queuedShell,
			isRemote,
			agentArgs: effectiveAgentArgs,
			agentEnv,
			sessionOptions: resolveInitialNativeChatSessionOptions(store.settings, initialViewModeOptions)
		},
		prompt: trimmedPrompt,
		promptDelivery,
		isFollowupPath
	});
	let promptDeliveryResult;
	if (!startupPlan) return null;
	const runtimeEnvironmentId = getRuntimeEnvironmentIdForWorktree(store, worktreeId);
	if (isWebRuntimeSessionActive(runtimeEnvironmentId)) {
		const webHostDelivery = launchAgentInWebHostTab({
			agent,
			worktreeId,
			environmentId: runtimeEnvironmentId,
			groupId,
			cwd: initialCwd,
			startupPlan,
			prompt: trimmedPrompt,
			promptDelivery,
			pastePromptAfterReady: pasteDraftAfterLaunch,
			submitPastedPrompt,
			agentArgs,
			viewMode: initialViewModeProps.viewMode ?? "terminal",
			onPromptDelivered
		});
		return {
			tabId: null,
			startupPlan,
			pasteDraftAfterLaunch: pasteDraftAfterLaunch !== null,
			...pasteDraftAfterLaunch !== null && promptDelivery === "submit-after-ready" ? { promptDeliveryResult: webHostDelivery } : {}
		};
	}
	const tab = store.createTab(worktreeId, groupId, void 0, {
		launchAgent: agent,
		quickCommandLabel,
		...initialViewModeProps
	});
	seedNativeChatAppliedSessionOptions(tab.id, agent, startupPlan.sessionOptions);
	if (initialCwd?.trim()) store.queueTabInitialCwd(tab.id, initialCwd);
	store.queueTabStartupCommand(tab.id, {
		command: startupPlan.launchCommand,
		...startupPlan.env ? { env: startupPlan.env } : {},
		launchConfig: startupPlan.launchConfig,
		launchAgent: agent,
		...agentArgs !== void 0 ? { agentArgsOverride: agentArgs } : {},
		...startupPlan.sessionOptions ? { sessionOptions: startupPlan.sessionOptions } : {},
		...startupPlan.startupCommandDelivery ? { startupCommandDelivery: startupPlan.startupCommandDelivery } : {},
		...agent === "command-code" && hasPrompt && promptDelivery === "auto-submit" ? { initialAgentStatus: {
			agent,
			prompt: trimmedPrompt
		} } : {},
		telemetry: {
			agent_kind: tuiAgentToAgentKind(agent),
			launch_source: launchSource ?? "tab_bar_quick_launch",
			request_kind: "new"
		}
	});
	if (hasPrompt && promptDelivery === "draft" && pasteDraftAfterLaunch === null) seedNativeChatLaunchDraftForAgentTab({
		tabId: tab.id,
		agent,
		text: trimmedPrompt
	});
	if (pasteDraftAfterLaunch !== null) {
		const timeoutNotice = createPasteReadinessTimeoutNotice({
			worktreeId,
			tabId: tab.id,
			agent,
			submitted: submitPastedPrompt
		});
		const deliveryPromise = deliverLaunchPromptToAgentTab({
			tabId: tab.id,
			content: pasteDraftAfterLaunch,
			agent,
			submit: submitPastedPrompt,
			forcePaste: promptDelivery === "submit-after-ready",
			onTimeout: timeoutNotice.onTimeout
		}).then((delivered) => {
			if (delivered) {
				if (agent === "command-code" && submitPastedPrompt) seedCommandCodeSubmittedPromptStatus(worktreeId, tab.id, trimmedPrompt);
				onPromptDelivered?.();
			}
			return {
				delivered,
				failureNotified: !delivered && timeoutNotice.wasNotified()
			};
		});
		if (promptDelivery === "submit-after-ready") promptDeliveryResult = deliveryPromise;
		else deliveryPromise.catch((error) => console.error("Prompt delivery failed after launch", error));
	} else if (hasPrompt) onPromptDelivered?.();
	store.setActiveTabType("terminal");
	const fresh = useAppStore.getState();
	const termIds = (fresh.tabsByWorktree[worktreeId] ?? []).map((t) => t.id);
	const editorIds = fresh.openFiles.filter((f) => f.worktreeId === worktreeId).map((f) => f.id);
	const browserIds = (fresh.browserTabsByWorktree?.[worktreeId] ?? []).map((t) => t.id);
	const order = reconcileTabOrder(fresh.tabBarOrderByWorktree[worktreeId], termIds, editorIds, browserIds).filter((id) => id !== tab.id);
	order.push(tab.id);
	fresh.setTabBarOrder(worktreeId, order);
	return {
		tabId: tab.id,
		startupPlan,
		pasteDraftAfterLaunch: pasteDraftAfterLaunch !== null,
		...promptDeliveryResult ? { promptDeliveryResult } : {}
	};
}
export { launchAgentInNewTab as t };
