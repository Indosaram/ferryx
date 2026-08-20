import { Da as resolveTerminalWorktreeRoute, Gc as isTerminalLeafId, Iu as toHostSessionTabId, Kc as makePaneKey, Ro as parseRemoteRuntimePtyId, So as resolveUnifiedTabLabel, t as useAppStore } from "./store-CgXrfmaH.js";
import { j as create } from "./plugin-manifest-Bs-50M_g.js";
import { d as isWebRuntimeSessionActive, i as closeWebRuntimeSessionTab } from "./web-runtime-session-CN2syA39.js";
import { s as inspectRuntimeTerminalProcess } from "./agent-process-recognition-BB0O3DaN.js";
import { d as resolveHostSessionTabIdForWebSessionTab, u as getLatestWebSessionTabsPublicationEpoch } from "./web-session-tabs-sync-CYKZbAxS.js";
function resolvePinnedTabLabel(state, worktreeId, visibleId) {
	return resolveUnifiedTabLabel((state.unifiedTabsByWorktree?.[worktreeId] ?? []).find((candidate) => candidate.id === visibleId || candidate.entityId === visibleId), state.settings?.tabAutoGenerateTitle === true);
}
function isUnifiedTabPinned(state, worktreeId, tabId) {
	return (state.unifiedTabsByWorktree?.[worktreeId] ?? []).some((tab) => (tab.id === tabId || tab.entityId === tabId) && tab.isPinned === true);
}
function shouldConfirmPinnedTabClose(state) {
	return state.settings?.confirmClosePinnedTab ?? true;
}
function guardPinnedTabClose(params) {
	const { isPinned, tabLabel, onClose, onCancel } = params;
	if (!isPinned) {
		onClose();
		return;
	}
	const state = useAppStore.getState();
	if (!shouldConfirmPinnedTabClose(state)) {
		onClose();
		return;
	}
	state.requestPinnedTabCloseConfirm({
		tabLabel,
		onConfirm: onClose,
		...onCancel ? { onCancel } : {}
	});
}
function mergeRequests(pending, duplicate) {
	return {
		...pending,
		onConfirm: () => {
			pending.onConfirm();
			duplicate.onConfirm();
		},
		onCancel: () => {
			pending.onCancel?.();
			duplicate.onCancel?.();
		}
	};
}
const useRunningTerminalCloseConfirmStore = create()((set, get) => {
	const queuedRequests = [];
	const INTER_REQUEST_ACTION_GUARD_MS = 350;
	let nextRequestActionAllowedAt = 0;
	const advanceRequest = () => {
		const next = queuedRequests.shift() ?? null;
		set({ runningTerminalCloseConfirm: next });
		return next !== null;
	};
	const guardNextAction = (revealedNextRequest) => {
		if (revealedNextRequest) nextRequestActionAllowedAt = Date.now() + INTER_REQUEST_ACTION_GUARD_MS;
	};
	return {
		runningTerminalCloseConfirm: null,
		requestRunningTerminalCloseConfirm: (request) => {
			const visible = get().runningTerminalCloseConfirm;
			if (visible?.terminalTabId === request.terminalTabId) {
				set({ runningTerminalCloseConfirm: mergeRequests(visible, request) });
				return;
			}
			const queuedIndex = queuedRequests.findIndex((queued) => queued.terminalTabId === request.terminalTabId);
			if (queuedIndex !== -1) {
				queuedRequests[queuedIndex] = mergeRequests(queuedRequests[queuedIndex], request);
				return;
			}
			if (visible) {
				queuedRequests.push(request);
				return;
			}
			set({ runningTerminalCloseConfirm: request });
		},
		confirmRunningTerminalClose: () => {
			const request = get().runningTerminalCloseConfirm;
			if (!request || Date.now() < nextRequestActionAllowedAt) return;
			guardNextAction(advanceRequest());
			request.onConfirm();
		},
		confirmAllRunningTerminalCloses: () => {
			if (Date.now() < nextRequestActionAllowedAt) return;
			const pending = [get().runningTerminalCloseConfirm, ...queuedRequests.splice(0)];
			set({ runningTerminalCloseConfirm: null });
			for (const request of pending) request?.onConfirm();
		},
		dismissRunningTerminalClose: () => {
			const request = get().runningTerminalCloseConfirm;
			if (!request || Date.now() < nextRequestActionAllowedAt) return;
			guardNextAction(advanceRequest());
			request.onCancel?.();
		}
	};
});
function resolveLeafCloseCopyKind(tabId, leafId) {
	if (!leafId || !isTerminalLeafId(leafId) || !tabId || tabId.includes(":")) return "command";
	const agentType = (useAppStore.getState().agentStatusByPaneKey ?? {})[makePaneKey(tabId, leafId)]?.agentType;
	return agentType && agentType !== "unknown" ? "agent" : "command";
}
function resolveBusyPtyCloseCopyKind(tabId, busyPtyIds) {
	const ptyIdsByLeafId = useAppStore.getState().terminalLayoutsByTabId?.[tabId]?.ptyIdsByLeafId ?? {};
	for (const [leafId, ptyId] of Object.entries(ptyIdsByLeafId)) if (busyPtyIds.includes(ptyId) && resolveLeafCloseCopyKind(tabId, leafId) === "agent") return "agent";
	return "command";
}
const RUNNING_CLOSE_PROBE_TIMEOUT_MS = 4e3;
function shouldConfirmRunningTerminalClose(options) {
	if (options?.force === true || options?.rejectPinned === true) return false;
	if (options?.skipRunningProcessConfirm === true || options?.lifecyclePtyId !== void 0) return false;
	const isUserReason = (reason) => reason === void 0 || reason === "user";
	return isUserReason(options?.reason) && isUserReason(options?.hostCloseReason);
}
function collectTabPtyIds(state, terminalTabId) {
	const ptyIds = /* @__PURE__ */ new Set();
	for (const ptyId of state.ptyIdsByTabId?.[terminalTabId] ?? []) if (ptyId) ptyIds.add(ptyId);
	const ptyIdsByLeafId = state.terminalLayoutsByTabId?.[terminalTabId]?.ptyIdsByLeafId ?? {};
	for (const ptyId of Object.values(ptyIdsByLeafId)) if (typeof ptyId === "string" && ptyId) ptyIds.add(ptyId);
	return [...ptyIds];
}
function guardRunningTerminalClose(params) {
	const { terminalTabId, tabLabel, onClose, onCancel } = params;
	const state = useAppStore.getState();
	const settings = state.settings;
	const ptyIds = collectTabPtyIds(state, terminalTabId);
	if (ptyIds.length === 0 || settings?.skipCloseTerminalWithRunningProcessConfirm === true) {
		onClose();
		return;
	}
	let decided = false;
	const closeNow = () => {
		if (decided) return;
		decided = true;
		onClose();
	};
	const confirmClose = (busyPtyIds) => {
		if (decided) return;
		const copyKind = resolveBusyPtyCloseCopyKind(terminalTabId, busyPtyIds);
		useRunningTerminalCloseConfirmStore.getState().requestRunningTerminalCloseConfirm({
			terminalTabId,
			tabLabel,
			copyKind,
			onConfirm: onClose,
			...onCancel ? { onCancel } : {}
		});
		decided = true;
	};
	const probeTimeout = setTimeout(() => {
		try {
			confirmClose(ptyIds);
		} catch {
			closeNow();
		}
	}, RUNNING_CLOSE_PROBE_TIMEOUT_MS);
	Promise.allSettled(ptyIds.map((ptyId) => inspectRuntimeTerminalProcess(settings, ptyId))).then((results) => {
		clearTimeout(probeTimeout);
		if (decided) return;
		const busyPtyIds = ptyIds.filter((_, index) => {
			const result = results[index];
			return result?.status === "fulfilled" && result.value.hasChildProcesses && result.value.unavailable !== true;
		});
		if (busyPtyIds.length === 0) {
			closeNow();
			return;
		}
		confirmClose(busyPtyIds);
	}).catch(() => {
		clearTimeout(probeTimeout);
		closeNow();
	});
}
function closeLocalTerminalTabState(terminalTabId, options) {
	const state = useAppStore.getState();
	if (options?.precomputedRetirementPlan?.tabId === terminalTabId || Object.values(state.tabsByWorktree).some((tabs) => tabs.some((tab) => tab.id === terminalTabId))) {
		if (options?.reason || options?.captureRecentlyClosed !== void 0 || options?.remoteCloseOwnedByHost || options?.localPtyTeardownOwnedExternally || options?.precomputedRetirementPlan) state.closeTab(terminalTabId, options);
		else state.closeTab(terminalTabId);
		return;
	}
	for (const tabs of Object.values(state.unifiedTabsByWorktree ?? {})) {
		const unified = tabs.find((tab) => tab.contentType === "terminal" && (tab.entityId === terminalTabId || tab.id === terminalTabId));
		if (unified) {
			state.closeTab(unified.entityId, options);
			return;
		}
	}
}
function getTerminalIncarnationHandle(ptyId, environmentId) {
	const terminal = parseRemoteRuntimePtyId(ptyId);
	if (terminal?.handle && terminal.environmentId === environmentId) return terminal.handle;
	return null;
}
function validatePrecomputedTerminalCloseState(tabId, retirementPlan, closeState) {
	return retirementPlan?.tabId === tabId && retirementPlan.worktreeId === closeState?.owningWorktreeId ? closeState : void 0;
}
function resolveTerminalCloseTarget(state, tabId, precomputed) {
	if (precomputed) return {
		worktreeId: precomputed.owningWorktreeId,
		terminalTabId: tabId
	};
	for (const [worktreeId, worktreeTabs] of Object.entries(state.tabsByWorktree)) if (worktreeTabs.some((tab) => tab.id === tabId)) return {
		worktreeId,
		terminalTabId: tabId
	};
	for (const [worktreeId, unifiedTabs] of Object.entries(state.unifiedTabsByWorktree ?? {})) {
		const unified = unifiedTabs.find((tab) => tab.contentType === "terminal" && (tab.entityId === tabId || tab.id === tabId));
		if (unified) return {
			worktreeId,
			terminalTabId: unified.entityId
		};
	}
	return null;
}
function getWorktreeTerminalTabIds(state, worktreeId) {
	const ids = /* @__PURE__ */ new Set();
	for (const tab of state.tabsByWorktree[worktreeId] ?? []) ids.add(tab.id);
	for (const tab of state.unifiedTabsByWorktree?.[worktreeId] ?? []) if (tab.contentType === "terminal") ids.add(tab.entityId);
	return [...ids];
}
function closeTerminalTab(tabId, options) {
	const state = useAppStore.getState();
	const precomputedCloseState = validatePrecomputedTerminalCloseState(tabId, options?.precomputedRetirementPlan, options?.precomputedCloseState);
	const target = resolveTerminalCloseTarget(state, tabId, precomputedCloseState);
	if (!target) {
		const closeReason = options?.reason ?? options?.hostCloseReason ?? "user";
		if (closeReason !== "pty-exit") state.closeTab(tabId, {
			reason: closeReason,
			...options?.localPtyTeardownOwnedExternally ? { localPtyTeardownOwnedExternally: true } : {},
			...options?.precomputedRetirementPlan ? { precomputedRetirementPlan: options.precomputedRetirementPlan } : {}
		});
		options?.onClosed?.();
		return;
	}
	const { worktreeId: owningWorktreeId, terminalTabId } = target;
	const worktreeRoute = resolveTerminalWorktreeRoute(state, owningWorktreeId);
	if (!worktreeRoute) {
		options?.onCancel?.();
		return;
	}
	if (options?.reason !== "pty-exit" && !options?.force && isUnifiedTabPinned(state, owningWorktreeId, terminalTabId)) {
		if (options?.rejectPinned) {
			options.onCancel?.();
			return;
		}
		if (shouldConfirmPinnedTabClose(state)) {
			guardPinnedTabClose({
				isPinned: true,
				tabLabel: resolvePinnedTabLabel(state, owningWorktreeId, terminalTabId),
				onClose: () => closeTerminalTab(tabId, {
					...options,
					force: true
				}),
				...options?.onCancel ? { onCancel: options.onCancel } : {}
			});
			return;
		}
	}
	if (shouldConfirmRunningTerminalClose(options)) {
		guardRunningTerminalClose({
			terminalTabId,
			tabLabel: resolvePinnedTabLabel(state, owningWorktreeId, terminalTabId),
			onClose: () => closeTerminalTab(tabId, {
				...options,
				skipRunningProcessConfirm: true
			}),
			...options?.onCancel ? { onCancel: options.onCancel } : {}
		});
		return;
	}
	const runtimeEnvironmentId = worktreeRoute.runtimeEnvironmentId;
	if (runtimeEnvironmentId && isWebRuntimeSessionActive(runtimeEnvironmentId)) {
		if (options?.reason === "pty-exit") return;
		const hostBackedTabId = resolveHostSessionTabIdForWebSessionTab(state, {
			environmentId: runtimeEnvironmentId,
			worktreeId: owningWorktreeId,
			tabId: terminalTabId
		}) ?? toHostSessionTabId(terminalTabId);
		const wireReason = options?.reason ?? options?.hostCloseReason ?? "user";
		const lifecycleTerminalHandle = wireReason === "user" ? null : getTerminalIncarnationHandle(options?.lifecyclePtyId ?? "", runtimeEnvironmentId);
		const publicationEpoch = wireReason === "user" ? null : getLatestWebSessionTabsPublicationEpoch(runtimeEnvironmentId, owningWorktreeId);
		closeLocalTerminalTabState(terminalTabId, {
			reason: options?.reason,
			...options?.captureRecentlyClosed !== void 0 ? { captureRecentlyClosed: options.captureRecentlyClosed } : {},
			remoteCloseOwnedByHost: true,
			...options?.localPtyTeardownOwnedExternally ? { localPtyTeardownOwnedExternally: true } : {},
			...options?.precomputedRetirementPlan ? { precomputedRetirementPlan: options.precomputedRetirementPlan } : {}
		});
		closeWebRuntimeSessionTab({
			worktreeId: owningWorktreeId,
			tabId: hostBackedTabId,
			environmentId: runtimeEnvironmentId,
			reason: wireReason,
			...wireReason !== "user" ? {
				publicationEpoch,
				terminalHandle: lifecycleTerminalHandle
			} : {}
		});
		options?.onClosed?.();
		return;
	}
	const currentTerminalTabIds = precomputedCloseState ? null : getWorktreeTerminalTabIds(state, owningWorktreeId);
	if ((precomputedCloseState?.terminalCountBeforeClose ?? currentTerminalTabIds.length) <= 1) {
		closeLocalTerminalTabState(terminalTabId, {
			reason: options?.reason,
			...options?.captureRecentlyClosed !== void 0 ? { captureRecentlyClosed: options.captureRecentlyClosed } : {},
			...options?.localPtyTeardownOwnedExternally ? { localPtyTeardownOwnedExternally: true } : {},
			...options?.precomputedRetirementPlan ? { precomputedRetirementPlan: options.precomputedRetirementPlan } : {}
		});
		if (state.activeWorktreeId === owningWorktreeId) {
			const worktreeFile = state.openFiles.find((f) => f.worktreeId === owningWorktreeId);
			if (worktreeFile) {
				state.setActiveFile(worktreeFile.id);
				state.setActiveTabType("editor");
			} else {
				const browserTab = (state.browserTabsByWorktree?.[owningWorktreeId] ?? [])[0];
				if (browserTab) {
					state.setActiveBrowserTab(browserTab.id);
					state.setActiveTabType("browser");
				} else state.setActiveWorktree(null);
			}
		}
		options?.onClosed?.();
		return;
	}
	if (state.activeWorktreeId === owningWorktreeId && terminalTabId === state.activeTabId) {
		const currentIndex = currentTerminalTabIds?.indexOf(terminalTabId) ?? -1;
		const nextTabId = precomputedCloseState ? precomputedCloseState.nextTerminalTabId : currentTerminalTabIds[currentIndex + 1] ?? currentTerminalTabIds[currentIndex - 1];
		if (nextTabId) state.setActiveTab(nextTabId);
	}
	closeLocalTerminalTabState(terminalTabId, {
		reason: options?.reason,
		...options?.captureRecentlyClosed !== void 0 ? { captureRecentlyClosed: options.captureRecentlyClosed } : {},
		...options?.localPtyTeardownOwnedExternally ? { localPtyTeardownOwnedExternally: true } : {},
		...options?.precomputedRetirementPlan ? { precomputedRetirementPlan: options.precomputedRetirementPlan } : {}
	});
	options?.onClosed?.();
}
export { guardPinnedTabClose as a, useRunningTerminalCloseConfirmStore as i, RUNNING_CLOSE_PROBE_TIMEOUT_MS as n, resolvePinnedTabLabel as o, resolveLeafCloseCopyKind as r, closeTerminalTab as t };
