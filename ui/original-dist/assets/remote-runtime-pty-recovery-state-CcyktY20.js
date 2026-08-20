import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as init_defineProperty, t as _defineProperty } from "./defineProperty-BAtR-r70.js";
import { t as Button } from "./button-DszXJEV6.js";
import { t as ExternalLink } from "./external-link-BrcDtGAn.js";
import { t as Settings } from "./settings-BX3azETW.js";
import { As as resolveTerminalLayoutActiveLeafId, Cp as getRepoIdFromWorktreeId, Ds as collectLeafIdsInOrder, E as assertClientCreationActionAvailable, H as createUntitledMarkdownFileWithTemplateSelection, Jc as parsePaneKey, Oa as focusTerminalTabSurface, Or as buildAgentNotificationId, Os as normalizeTerminalLayoutSnapshot, Pm as FLOATING_TERMINAL_WORKTREE_ID, Rt as detectLanguage, Ut as getConnectionIdFromState, au as clampUtf8TextTail, bh as resolveTuiAgentPermissionMode, bs as TERMINAL_SCROLLBACK_SESSION_BUFFER_BYTE_LIMIT, en as fileUriToFilesystemPath, eo as isClipboardTextTooLargeError, t as useAppStore, ta as isPiCompatibleAgentType, uu as measureUtf8ByteLength, xa as getRendererAppPlatform, yo as getGroupVisibleTabOrder } from "./store-CgXrfmaH.js";
import { E as matchKeybindingDigitIndex, w as keybindingMatchesAction } from "./plugin-manifest-Bs-50M_g.js";
import { B as isGeminiTerminalTitle, G as DROID_AGENT_NAME_RE, J as titleHasAnyLegacyAgentName, K as HERMES_AGENT_NAME_RE, V as isPiTerminalTitle, W as AGY_AGENT_NAME_RE, a as isExplicitAgentStatusFresh, f as isFreshNonDoneAgentStatus, i as classifyTitleActivity, o as resolveCommittedTitleAgentType, st as parseExecutionHostId, u as AGENT_STATUS_STALE_AFTER_MS, w as detectAgentStatusFromTitle } from "./agent-status-3vUKbY6l.js";
import { n as toast } from "./dist-DgqligFk.js";
import { t as Checkbox } from "./checkbox-PAbetBh2.js";
import { t as Label } from "./label-D-n9s_wS.js";
import { i as getWorktreeMapFromState, r as getRepoMapFromState } from "./selectors-XOBeaOSb.js";
import { i as recognizeAgentProcess, r as isRecognizedAgentType } from "./agent-process-recognition-BB0O3DaN.js";
import { i as resolveCompatibleAgentTypeForOwner, o as getSyntheticAgentTerminalTitle, s as getSyntheticAgentTitleProfile } from "./pane-agent-owner-BPfoVAtS.js";
import { t as getConnectionId } from "./connection-context-BUPsamzR.js";
import { a as isForeignMachineCodexPtyId, o as TOGGLE_FLOATING_TERMINAL_EVENT, r as markRestoredStaleCodexSessionsForRestart } from "./codex-session-restart-DLDE2Yzk.js";
import { t as Badge } from "./badge-BBptl5GG.js";
import { t as ShortcutKeyCombo } from "./ShortcutKeyCombo-Ch456Md0.js";
import { a as DialogFooter, i as DialogDescription, o as DialogHeader, r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
import { c as createTextControlRejectedResult, o as TEXT_CONTROL_PASTE_DIRECT_MAX_BYTES, r as pasteTextIntoTextControl, s as TEXT_CONTROL_PASTE_MAX_BYTES, t as measureTextControlPasteByteLength } from "./text-control-paste-PhBVbE2p.js";
import { t as PRIMARY_SELECTION_MAX_LENGTH } from "./primary-selection-BsidtYsF.js";
var parkedTabIdsByWorktreeId = /* @__PURE__ */ new Map();
function recordTerminalTabParkedOnUnresolvedHost(worktreeId, tabId) {
	const parked = parkedTabIdsByWorktreeId.get(worktreeId) ?? /* @__PURE__ */ new Set();
	parked.add(tabId);
	parkedTabIdsByWorktreeId.set(worktreeId, parked);
}
function getTabIdsAwaitingHostHydrationRemount(state) {
	const remountable = [];
	for (const [worktreeId, parkedTabIds] of parkedTabIdsByWorktreeId) {
		if (getConnectionIdFromState(state, worktreeId) === void 0) continue;
		const tabs = state.tabsByWorktree?.[worktreeId] ?? [];
		for (const tabId of parkedTabIds) {
			const tab = tabs.find((candidate) => candidate.id === tabId);
			const hasLivePty = (state.ptyIdsByTabId?.[tabId]?.length ?? 0) > 0;
			if (tab && !tab.ptyId && !hasLivePty) remountable.push(tabId);
		}
		parkedTabIdsByWorktreeId.delete(worktreeId);
	}
	return remountable;
}
const ZOOM_LEVEL_CHANGED_EVENT = "orca:zoom-level-changed";
function dispatchZoomLevelChanged(type, percent) {
	window.dispatchEvent(new CustomEvent(ZOOM_LEVEL_CHANGED_EVENT, { detail: {
		type,
		percent
	} }));
}
function getActiveEntityIdForTabType(activeTabType, activeTabId, activeFileId, activeBrowserTabId) {
	if (activeTabType === "editor") return activeFileId;
	if (activeTabType === "browser") return activeBrowserTabId;
	if (activeTabType === "simulator") return activeTabId;
	return activeTabId;
}
function getNextTabAcrossAllTypes({ tabs, activeTabType, activeTabId, activeFileId, activeBrowserTabId, activeGroupTabId, direction }) {
	if (tabs.length <= 1) return null;
	const groupTabIdInNav = activeGroupTabId && tabs.some((entry) => entry.tabId === activeGroupTabId) ? activeGroupTabId : null;
	const currentId = getActiveEntityIdForTabType(activeTabType, activeTabId, activeFileId, activeBrowserTabId);
	const currentIndex = groupTabIdInNav ? tabs.findIndex((tab) => tab.tabId === groupTabIdInNav) : tabs.findIndex((tab) => tab.type === activeTabType && tab.id === currentId);
	if (currentIndex === -1) return direction < 0 ? tabs.at(-1) : tabs.at(0);
	return tabs[(currentIndex + direction + tabs.length) % tabs.length];
}
function getNextTabWithinActiveType({ tabs, activeTabType, activeTabId, activeFileId, activeBrowserTabId, activeGroupTabId, direction }) {
	const tabsOfActiveType = tabs.filter((tab) => tab.type === activeTabType);
	if (tabsOfActiveType.length <= 1) return null;
	const groupTabIdInNav = activeGroupTabId && tabsOfActiveType.some((entry) => entry.tabId === activeGroupTabId) ? activeGroupTabId : null;
	const currentId = getActiveEntityIdForTabType(activeTabType, activeTabId, activeFileId, activeBrowserTabId);
	const currentIndex = groupTabIdInNav ? tabsOfActiveType.findIndex((tab) => tab.tabId === groupTabIdInNav) : tabsOfActiveType.findIndex((tab) => tab.id === currentId);
	if (currentIndex === -1) return direction < 0 ? tabsOfActiveType.at(-1) : tabsOfActiveType.at(0);
	return tabsOfActiveType[(currentIndex + direction + tabsOfActiveType.length) % tabsOfActiveType.length];
}
var driverByPtyId = /* @__PURE__ */ new Map();
var changeListeners = /* @__PURE__ */ new Set();
function onDriverChange(listener) {
	changeListeners.add(listener);
	return () => changeListeners.delete(listener);
}
function notifyChange(event) {
	for (const listener of changeListeners) listener(event);
}
function setDriverForPty(ptyId, driver) {
	if (driver.kind === "idle") driverByPtyId.delete(ptyId);
	else driverByPtyId.set(ptyId, driver);
	notifyChange({
		ptyId,
		driver
	});
}
function replaceDriverPtyId(replacedPtyId, ptyId) {
	const replaced = driverByPtyId.get(replacedPtyId);
	if (!replaced) return;
	if (!driverByPtyId.has(ptyId)) setDriverForPty(ptyId, replaced);
	setDriverForPty(replacedPtyId, { kind: "idle" });
}
function getDriverForPty(ptyId) {
	return driverByPtyId.get(ptyId) ?? { kind: "idle" };
}
function getAllDrivers() {
	return new Map(driverByPtyId);
}
function isPtyLocked(ptyId) {
	return driverByPtyId.get(ptyId)?.kind === "mobile";
}
function hydrateDrivers(drivers) {
	const affectedPtyIds = new Set(driverByPtyId.keys());
	driverByPtyId.clear();
	for (const { ptyId, driver } of drivers) {
		affectedPtyIds.add(ptyId);
		if (driver.kind !== "idle") driverByPtyId.set(ptyId, driver);
	}
	for (const ptyId of affectedPtyIds) notifyChange({
		ptyId,
		driver: getDriverForPty(ptyId)
	});
}
var persistedAuthorityFlagCache;
function readPersistedSideEffectAuthorityFlagSync() {
	if (persistedAuthorityFlagCache === void 0) try {
		const getSync = globalThis.window?.api?.settings?.getSync;
		persistedAuthorityFlagCache = typeof getSync === "function" ? getSync()?.terminalMainSideEffectAuthority ?? null : null;
	} catch {
		persistedAuthorityFlagCache = null;
	}
	return persistedAuthorityFlagCache;
}
function isMainTerminalSideEffectAuthorityForPty(args) {
	if (args.runtimeEnvironmentId !== null) return false;
	if (args.settings !== null) return args.settings.terminalMainSideEffectAuthority !== false;
	return readPersistedSideEffectAuthorityFlagSync() !== false;
}
var consumersByPtyId = /* @__PURE__ */ new Map();
var channelUnsubscribe = null;
function applyLiveFact(entry, fact, seq) {
	switch (fact.kind) {
		case "agent-status":
			entry.callbacks.onAgentStatus?.(fact.payload);
			return;
		case "title":
			entry.lastLiveTitleSeq = seq;
			entry.callbacks.onTitleChange?.(fact.normalizedTitle, fact.rawTitle, fact.staleWorkingTitleClear ? { staleWorkingTitleClear: true } : void 0);
			return;
		case "bell":
			entry.callbacks.onBell?.();
			return;
		case "agent-working":
			entry.callbacks.onAgentBecameWorking?.();
			return;
		case "agent-idle":
			entry.callbacks.onAgentBecameIdle?.(fact.title, fact.staleWorkingTitleClear ? { staleWorkingTitleClear: true } : void 0);
			return;
		case "agent-exited":
			entry.callbacks.onAgentExited?.();
			return;
		case "command-finished":
			entry.callbacks.onCommandFinished?.(fact.exitCode);
			return;
		case "pr-link":
			entry.callbacks.onPrLink?.(fact.link);
			return;
		case "command-code-working":
			entry.callbacks.onCommandCodeWorking?.(fact.prompt);
			return;
		case "command-code-done":
			entry.callbacks.onCommandCodeDone?.(fact.prompt);
			return;
		case "2031-subscribe":
			entry.callbacks.onMode2031Subscribe?.();
			return;
		case "2031-unsubscribe": entry.callbacks.onMode2031Unsubscribe?.();
	}
}
function applyBatchToConsumer(entry, batch) {
	if (batch.replay) {
		if (entry.lastLiveTitleSeq !== null && batch.seq <= entry.lastLiveTitleSeq) return;
		for (const fact of batch.facts) if (fact.kind === "title") entry.callbacks.onTitleChange?.(fact.normalizedTitle, fact.rawTitle);
		return;
	}
	for (const fact of batch.facts) applyLiveFact(entry, fact, batch.seq);
}
var HANDOFF_FACT_BUFFER_TTL_MS = 15e3;
var MAX_HANDOFF_FACT_BATCHES = 64;
var MAX_HANDOFF_FACT_PTYS = 32;
var handoffFactBuffersByPtyId = /* @__PURE__ */ new Map();
function deleteHandoffFactBuffer(ptyId) {
	const buffer = handoffFactBuffersByPtyId.get(ptyId);
	if (buffer) {
		clearTimeout(buffer.expiryTimer);
		handoffFactBuffersByPtyId.delete(ptyId);
	}
}
function openHandoffFactBuffer(ptyId) {
	const nowMs = Date.now();
	for (const [bufferedPtyId, buffer$1] of handoffFactBuffersByPtyId) if (buffer$1.expiresAtMs <= nowMs) deleteHandoffFactBuffer(bufferedPtyId);
	deleteHandoffFactBuffer(ptyId);
	if (handoffFactBuffersByPtyId.size >= MAX_HANDOFF_FACT_PTYS) {
		const oldestPtyId = handoffFactBuffersByPtyId.keys().next().value;
		if (typeof oldestPtyId === "string") deleteHandoffFactBuffer(oldestPtyId);
	}
	const buffer = {
		batches: [],
		expiresAtMs: nowMs + HANDOFF_FACT_BUFFER_TTL_MS,
		expiryTimer: setTimeout(() => {
			if (handoffFactBuffersByPtyId.get(ptyId) === buffer) handoffFactBuffersByPtyId.delete(ptyId);
		}, HANDOFF_FACT_BUFFER_TTL_MS)
	};
	buffer.expiryTimer.unref?.();
	handoffFactBuffersByPtyId.set(ptyId, buffer);
}
function bufferHandoffFactBatch(batch) {
	const buffer = handoffFactBuffersByPtyId.get(batch.ptyId);
	if (!buffer) return;
	if (buffer.expiresAtMs <= Date.now()) {
		deleteHandoffFactBuffer(batch.ptyId);
		return;
	}
	if (batch.replay) return;
	if (buffer.batches.length >= MAX_HANDOFF_FACT_BATCHES) buffer.batches.shift();
	buffer.batches.push(batch);
}
function drainHandoffFactBuffer(ptyId, entry) {
	const buffer = handoffFactBuffersByPtyId.get(ptyId);
	if (!buffer) return;
	deleteHandoffFactBuffer(ptyId);
	if (buffer.expiresAtMs <= Date.now()) return;
	for (const batch of buffer.batches) applyBatchToConsumer(entry, batch);
}
function dispatchTerminalSideEffectBatch(batch) {
	const entry = consumersByPtyId.get(batch.ptyId);
	if (!entry) {
		bufferHandoffFactBatch(batch);
		return;
	}
	applyBatchToConsumer(entry, batch);
}
function ensureSideEffectChannelSubscription() {
	if (channelUnsubscribe !== null) return;
	const onSideEffect = globalThis.window?.api?.pty?.onSideEffect;
	if (typeof onSideEffect !== "function") return;
	channelUnsubscribe = onSideEffect(dispatchTerminalSideEffectBatch);
}
function registerTerminalSideEffectFactConsumer(options) {
	ensureSideEffectChannelSubscription();
	const entry = {
		callbacks: options.callbacks,
		lastLiveTitleSeq: null
	};
	consumersByPtyId.set(options.ptyId, entry);
	drainHandoffFactBuffer(options.ptyId, entry);
	if (options.restoreTitleOnRegister) {
		const getSnapshot = globalThis.window?.api?.pty?.getSideEffectSnapshot;
		if (typeof getSnapshot === "function") getSnapshot(options.ptyId).then((batch) => {
			if (batch && consumersByPtyId.get(options.ptyId) === entry) applyBatchToConsumer(entry, {
				...batch,
				replay: true
			});
		}).catch(() => {});
	}
	return () => {
		if (consumersByPtyId.get(options.ptyId) === entry) {
			consumersByPtyId.delete(options.ptyId);
			openHandoffFactBuffer(options.ptyId);
		}
	};
}
function repoNeedsRendererCapturedScrollback(repo) {
	if (repo.connectionId) return true;
	const parsedHost = parseExecutionHostId(repo.executionHostId);
	return parsedHost !== null && parsedHost.kind !== "local";
}
function shouldPreserveTerminalScrollbackBuffersForRepoMap(worktreeId, repoById) {
	if (worktreeId === void 0 || worktreeId === "global-floating-terminal") return false;
	const repoId = getRepoIdFromWorktreeId(worktreeId);
	const repo = repoById.get(repoId);
	if (repo && repoNeedsRendererCapturedScrollback(repo)) return true;
	if (!repoById.has(repoId)) return true;
	return false;
}
function shouldPreserveTerminalScrollbackBuffers(worktreeId, repos) {
	return shouldPreserveTerminalScrollbackBuffersForRepoMap(worktreeId, new Map(repos.map((repo) => [repo.id, repo])));
}
function capTerminalScrollbackSessionBuffer(buffer) {
	if (buffer.length <= 524288 && !measureUtf8ByteLength(buffer, { stopAfterBytes: 524288 }).exceededLimit) return buffer;
	return clampUtf8TextTail(buffer, TERMINAL_SCROLLBACK_SESSION_BUFFER_BYTE_LIMIT).text;
}
function capTerminalScrollbackLeafBuffers(buffers) {
	if (!buffers) return {
		buffers: void 0,
		changed: false
	};
	let changed = false;
	const capped = {};
	for (const [leafId, buffer] of Object.entries(buffers)) {
		const next = capTerminalScrollbackSessionBuffer(buffer);
		capped[leafId] = next;
		changed || (changed = next !== buffer);
	}
	return {
		buffers: Object.keys(capped).length > 0 ? capped : void 0,
		changed
	};
}
function pruneLocalTerminalScrollbackBuffers(session, repos) {
	const repoById = new Map(repos.map((repo) => [repo.id, repo]));
	const worktreeIdByTabId = /* @__PURE__ */ new Map();
	const tabsByWorktree = session.tabsByWorktree ?? {};
	const terminalLayoutsByTabIdForRead = session.terminalLayoutsByTabId ?? {};
	for (const [worktreeId, tabs] of Object.entries(tabsByWorktree)) for (const tab of tabs) worktreeIdByTabId.set(tab.id, worktreeId);
	let terminalLayoutsByTabId = null;
	for (const [tabId, layout] of Object.entries(terminalLayoutsByTabIdForRead)) {
		if (!layout.buffersByLeafId && !layout.scrollbackRefsByLeafId) continue;
		if (shouldPreserveTerminalScrollbackBuffersForRepoMap(worktreeIdByTabId.get(tabId), repoById)) {
			const capped = capTerminalScrollbackLeafBuffers(layout.buffersByLeafId);
			if (capped.changed) {
				terminalLayoutsByTabId ?? (terminalLayoutsByTabId = { ...terminalLayoutsByTabIdForRead });
				terminalLayoutsByTabId[tabId] = {
					...layout,
					buffersByLeafId: capped.buffers
				};
			}
			continue;
		}
		terminalLayoutsByTabId ?? (terminalLayoutsByTabId = { ...terminalLayoutsByTabIdForRead });
		const layoutWithoutBuffers = { ...layout };
		delete layoutWithoutBuffers.buffersByLeafId;
		delete layoutWithoutBuffers.scrollbackRefsByLeafId;
		terminalLayoutsByTabId[tabId] = layoutWithoutBuffers;
	}
	if (!terminalLayoutsByTabId) return session;
	return {
		...session,
		terminalLayoutsByTabId
	};
}
function removeLeafFromTree(node, leafId) {
	if (node.type === "leaf") return node.leafId === leafId ? {
		node: null,
		removed: true
	} : {
		node,
		removed: false
	};
	const first = removeLeafFromTree(node.first, leafId);
	const second = removeLeafFromTree(node.second, leafId);
	if (!first.removed && !second.removed) return {
		node,
		removed: false
	};
	if (!first.node) return {
		node: second.node,
		removed: true
	};
	if (!second.node) return {
		node: first.node,
		removed: true
	};
	return {
		node: {
			...node,
			first: first.node,
			second: second.node
		},
		removed: true
	};
}
function omitLeafRecord(source, leafId) {
	if (!source || !Object.hasOwn(source, leafId)) return source;
	const next = { ...source };
	delete next[leafId];
	return Object.keys(next).length > 0 ? next : void 0;
}
function singleLeafRecord(source, leafId) {
	const value = source?.[leafId];
	return value ? { [leafId]: value } : void 0;
}
function detachTerminalLayoutLeaf(snapshot, leafId) {
	const layout = normalizeTerminalLayoutSnapshot(snapshot).snapshot;
	if (!layout.root) return null;
	const originalLeafIds = collectLeafIdsInOrder(layout.root);
	if (!originalLeafIds.includes(leafId) || originalLeafIds.length <= 1) return null;
	const removal = removeLeafFromTree(layout.root, leafId);
	if (!removal.removed || !removal.node) return null;
	const ptyIdsByLeafId = omitLeafRecord(layout.ptyIdsByLeafId, leafId);
	const buffersByLeafId = omitLeafRecord(layout.buffersByLeafId, leafId);
	const scrollbackRefsByLeafId = omitLeafRecord(layout.scrollbackRefsByLeafId, leafId);
	const titlesByLeafId = omitLeafRecord(layout.titlesByLeafId, leafId);
	const sourceLayout = {
		root: removal.node,
		activeLeafId: resolveTerminalLayoutActiveLeafId({
			root: removal.node,
			activeLeafId: layout.activeLeafId === leafId ? null : layout.activeLeafId,
			ptyIdsByLeafId
		}),
		expandedLeafId: layout.expandedLeafId === leafId ? null : layout.expandedLeafId,
		...ptyIdsByLeafId ? { ptyIdsByLeafId } : {},
		...buffersByLeafId ? { buffersByLeafId } : {},
		...scrollbackRefsByLeafId ? { scrollbackRefsByLeafId } : {},
		...titlesByLeafId ? { titlesByLeafId } : {}
	};
	const detachedPtyIdsByLeafId = singleLeafRecord(layout.ptyIdsByLeafId, leafId);
	const detachedBuffersByLeafId = singleLeafRecord(layout.buffersByLeafId, leafId);
	const detachedScrollbackRefsByLeafId = singleLeafRecord(layout.scrollbackRefsByLeafId, leafId);
	const detachedTitlesByLeafId = singleLeafRecord(layout.titlesByLeafId, leafId);
	return {
		sourceLayout,
		detachedLayout: {
			root: {
				type: "leaf",
				leafId
			},
			activeLeafId: leafId,
			expandedLeafId: null,
			...detachedPtyIdsByLeafId ? { ptyIdsByLeafId: detachedPtyIdsByLeafId } : {},
			...detachedBuffersByLeafId ? { buffersByLeafId: detachedBuffersByLeafId } : {},
			...detachedScrollbackRefsByLeafId ? { scrollbackRefsByLeafId: detachedScrollbackRefsByLeafId } : {},
			...detachedTitlesByLeafId ? { titlesByLeafId: detachedTitlesByLeafId } : {}
		},
		ptyId: detachedPtyIdsByLeafId?.[leafId] ?? null
	};
}
async function createFloatingWorkspaceTerminalTab(store, shellOverride) {
	const targetGroupId = store.activeGroupIdByWorktree[FLOATING_TERMINAL_WORKTREE_ID];
	const tab = store.createTab(FLOATING_TERMINAL_WORKTREE_ID, targetGroupId, shellOverride, { activate: false });
	store.activateTab(tab.id);
	focusTerminalTabSurface(tab.id);
	return tab;
}
async function createFloatingWorkspaceBrowserTab(store) {
	assertClientCreationActionAvailable(store, FLOATING_TERMINAL_WORKTREE_ID, "managed-browser");
	const targetGroupId = store.activeGroupIdByWorktree[FLOATING_TERMINAL_WORKTREE_ID];
	const url = store.browserDefaultUrl ?? "about:blank";
	return store.createBrowserTab(FLOATING_TERMINAL_WORKTREE_ID, url, {
		title: translate("auto.lib.floating.workspace.tab.creation.f3785eddc2", "New Browser Tab"),
		focusAddressBar: true,
		targetGroupId,
		browserRuntimeEnvironmentId: null
	});
}
async function createFloatingWorkspaceMarkdownTab(store, markdownDirectory) {
	const targetGroupId = store.activeGroupIdByWorktree[FLOATING_TERMINAL_WORKTREE_ID];
	const floatingMarkdownDirectory = markdownDirectory ?? await window.api.app.getFloatingMarkdownDirectory();
	if (!floatingMarkdownDirectory) return;
	const fileInfo = await createUntitledMarkdownFileWithTemplateSelection(floatingMarkdownDirectory, FLOATING_TERMINAL_WORKTREE_ID, getConnectionId("global-floating-terminal") ?? void 0, { activeRuntimeEnvironmentId: null });
	if (!fileInfo) return;
	store.openFile({
		...fileInfo,
		language: detectLanguage(fileInfo.relativePath)
	}, {
		preview: false,
		targetGroupId,
		suppressActiveRuntimeFallback: true
	});
}
var FLOATING_WORKSPACE_SHORTCUT_SURFACE_SELECTOR = "[data-floating-terminal-shortcut-surface]";
var FLOATING_WORKSPACE_PANEL_SHORTCUT_ACTIONS = [
	"tab.newTerminal",
	"tab.newBrowser",
	"tab.newMarkdown",
	"tab.openMarkdown",
	"tab.close"
];
function isFloatingWorkspacePanelShortcutTarget(target, panelRoot = null) {
	if (!(target instanceof HTMLElement)) return false;
	return target === panelRoot || target.getAttribute("data-floating-terminal-panel") !== null || target.closest(FLOATING_WORKSPACE_SHORTCUT_SURFACE_SELECTOR) !== null;
}
function matchFloatingWorkspacePanelOwnedAction(event, platform, keybindings, options) {
	return FLOATING_WORKSPACE_PANEL_SHORTCUT_ACTIONS.find((actionId) => keybindingMatchesAction(actionId, event, platform, keybindings, options)) ?? null;
}
function matchFloatingWorkspacePanelShortcut(event, platform, keybindings, options, chromeOptions = options) {
	if (keybindingMatchesAction("tab.rename", event, platform, keybindings, chromeOptions)) return {
		kind: "action",
		action: "tab.rename"
	};
	const index = matchKeybindingDigitIndex("workspace.selectByIndex", event, platform, keybindings, options) ?? matchKeybindingDigitIndex("tab.selectByIndex", event, platform, keybindings, options);
	if (index !== null) return {
		kind: "index",
		index
	};
	if (keybindingMatchesAction("floatingWorkspace.maximize", event, platform, keybindings, chromeOptions)) return {
		kind: "action",
		action: "floatingWorkspace.maximize"
	};
	if (keybindingMatchesAction("floatingWorkspace.minimize", event, platform, keybindings, chromeOptions)) return {
		kind: "action",
		action: "floatingWorkspace.minimize"
	};
	return null;
}
function matchFloatingWorkspacePanelChord(event, platform, panelRoot, keybindings, options, chromeOptions = options) {
	const ownedAction = isFloatingWorkspacePanelShortcutTarget(event.target, panelRoot) ? matchFloatingWorkspacePanelOwnedAction(event, platform, keybindings, options) : null;
	if (ownedAction) return {
		kind: "action",
		action: ownedAction
	};
	return matchFloatingWorkspacePanelShortcut(event, platform, keybindings, options, chromeOptions);
}
var FLOATING_WORKSPACE_PANEL_SELECTOR = "[data-floating-terminal-panel]";
var EMPTY_FLOATING_WORKSPACE_PANEL_SELECTOR = "[data-floating-terminal-panel][aria-hidden=\"false\"] [data-floating-terminal-empty-state]";
function getActiveFloatingWorkspaceGroup(store) {
	const groups = store.groupsByWorktree["global-floating-terminal"] ?? [];
	const activeGroupId = store.activeGroupIdByWorktree[FLOATING_TERMINAL_WORKTREE_ID];
	if (activeGroupId) {
		const activeGroup = groups.find((group) => group.id === activeGroupId);
		if (activeGroup) return activeGroup;
	}
	return groups.find((group) => group.activeTabId != null) ?? groups[0] ?? null;
}
function getFloatingWorkspaceVisibleTabs(store, group) {
	return getGroupVisibleTabOrder(group, (store.unifiedTabsByWorktree["global-floating-terminal"] ?? []).filter((tab) => tab.groupId === group.id), new Set((store.tabsByWorktree["global-floating-terminal"] ?? []).map((tab) => tab.id)), new Set(store.openFiles.filter((file) => file.worktreeId === FLOATING_TERMINAL_WORKTREE_ID).map((file) => file.id)), new Set((store.browserTabsByWorktree["global-floating-terminal"] ?? []).map((tab) => tab.id)));
}
function countVisibleFloatingWorkspaceItems(store) {
	const group = getActiveFloatingWorkspaceGroup(store);
	return group ? getFloatingWorkspaceVisibleTabs(store, group).length : 0;
}
function getFloatingWorkspaceActiveEntry(visibleTabs, group) {
	if (group.activeTabId) {
		const active = visibleTabs.find((tab) => tab.tabId === group.activeTabId);
		if (active) return active;
	}
	return visibleTabs[0] ?? null;
}
function getActiveIdsForFloatingEntry(entry) {
	return {
		activeBrowserTabId: entry.type === "browser" ? entry.id : null,
		activeFileId: entry.type === "editor" ? entry.id : null,
		activeTabId: entry.type === "terminal" ? entry.id : null,
		activeTabType: entry.type
	};
}
function getFloatingWorkspaceBrowserTab(store, browserTabId) {
	return (store.browserTabsByWorktree["global-floating-terminal"] ?? []).find((tab) => tab.id === browserTabId) ?? null;
}
function resolveFloatingWorkspaceBrowserWorkspaceId(store, sourceId) {
	const workspaces = store.browserTabsByWorktree["global-floating-terminal"] ?? [];
	const pagesByWorkspace = store.browserPagesByWorkspace ?? {};
	for (const workspace of workspaces) {
		if (workspace.id === sourceId) return workspace.id;
		if ((pagesByWorkspace[workspace.id] ?? []).some((page) => page.id === sourceId)) return workspace.id;
	}
	return null;
}
function activateFloatingWorkspaceCyclableTab(store, next) {
	if (next.tabId) store.activateTab(next.tabId);
	if (next.type === "terminal") {
		store.setActiveTab(next.id);
		focusTerminalTabSurface(next.id);
		return;
	}
	if (next.type === "browser") {
		const workspace = getFloatingWorkspaceBrowserTab(store, next.id);
		if (workspace?.activePageId && typeof window !== "undefined" && window.api?.browser) window.api.browser.notifyActiveTabChanged({ browserPageId: workspace.activePageId });
	}
}
function getNextFloatingWorkspaceTerminalTab(visibleTabs, active, direction) {
	const terminalTabs = visibleTabs.filter((tab) => tab.type === "terminal");
	if (terminalTabs.length === 0) return null;
	const currentIndex = terminalTabs.findIndex((tab) => tab.id === active.id);
	if (terminalTabs.length === 1 && currentIndex === 0 && active.type === "terminal") return null;
	return terminalTabs[((currentIndex === -1 && direction > 0 ? -1 : currentIndex === -1 ? 0 : currentIndex) + direction + terminalTabs.length) % terminalTabs.length];
}
function isFloatingWorkspacePanelVisible(doc = document) {
	return Boolean(doc.querySelector("[data-floating-terminal-panel][aria-hidden=\"false\"]"));
}
function isEmptyFloatingWorkspacePanelVisible(doc = typeof document === "undefined" ? null : document) {
	return Boolean(doc?.querySelector(EMPTY_FLOATING_WORKSPACE_PANEL_SELECTOR));
}
function isFloatingWorkspacePanelFocused(doc = typeof document === "undefined" ? null : document) {
	const active = doc?.activeElement;
	return active instanceof HTMLElement && active.closest(FLOATING_WORKSPACE_PANEL_SELECTOR) !== null;
}
function isEventTargetInsideFloatingWorkspacePanel(target) {
	return target instanceof HTMLElement && target.closest(FLOATING_WORKSPACE_PANEL_SELECTOR) !== null;
}
function isFloatingWorkspaceTerminalInputTarget(target) {
	if (!(target instanceof HTMLElement)) return false;
	if (target.closest(FLOATING_WORKSPACE_PANEL_SELECTOR) === null) return false;
	return target.classList?.contains("xterm-helper-textarea") === true || target.closest(".xterm") !== null;
}
function shouldMinimizeFloatingWorkspacePanelOnCloseShortcut({ floatingTerminalOpen, floatingVisibleTabCount }) {
	return floatingTerminalOpen && floatingVisibleTabCount === 0;
}
function handleEmptyFloatingWorkspacePanelCloseShortcut(event, platform, keybindings) {
	if (event.repeat || !isEmptyFloatingWorkspacePanelVisible() || !keybindingMatchesAction("tab.close", event, platform, keybindings, { context: "app" })) return false;
	event.preventDefault();
	event.stopPropagation();
	event.stopImmediatePropagation();
	window.dispatchEvent(new Event(TOGGLE_FLOATING_TERMINAL_EVENT));
	return true;
}
function switchFloatingWorkspaceTab(store, direction, mode) {
	const group = getActiveFloatingWorkspaceGroup(store);
	if (!group) return false;
	const visibleTabs = getFloatingWorkspaceVisibleTabs(store, group);
	if (visibleTabs.length <= 1) return false;
	const active = getFloatingWorkspaceActiveEntry(visibleTabs, group);
	if (!active) return false;
	const groupTabIdInNav = group.activeTabId && visibleTabs.some((entry) => entry.tabId === group.activeTabId) ? group.activeTabId : null;
	const next = mode === "terminal" ? getNextFloatingWorkspaceTerminalTab(visibleTabs, active, direction) : mode === "all-types" ? getNextTabAcrossAllTypes({
		tabs: visibleTabs,
		...getActiveIdsForFloatingEntry(active),
		activeGroupTabId: groupTabIdInNav,
		direction
	}) : getNextTabWithinActiveType({
		tabs: visibleTabs,
		...getActiveIdsForFloatingEntry(active),
		activeGroupTabId: groupTabIdInNav,
		direction
	});
	if (!next) return false;
	activateFloatingWorkspaceCyclableTab(store, next);
	return true;
}
var MAX_CONCURRENT_INSPECTIONS = 4;
var MAX_INSPECTION_STARTS_PER_SECOND = 8;
var activeInspections = 0;
var inspectionPumpTimer = null;
var inspectionStarts = [];
var inspectionQueue = [];
function canStartInspection(now) {
	if (inspectionStarts.length > 0 && now < inspectionStarts[0]) inspectionStarts.length = 0;
	while (inspectionStarts.length > 0 && now - inspectionStarts[0] >= 1e3) inspectionStarts.shift();
	return activeInspections < MAX_CONCURRENT_INSPECTIONS && inspectionStarts.length < MAX_INSPECTION_STARTS_PER_SECOND;
}
function scheduleInspectionPump(delayMs = 0) {
	if (inspectionPumpTimer !== null) return;
	inspectionPumpTimer = setTimeout(() => {
		inspectionPumpTimer = null;
		pumpInspectionQueue();
	}, delayMs);
}
function pumpInspectionQueue() {
	const now = Date.now();
	if (!canStartInspection(now)) {
		scheduleInspectionPump(100);
		return;
	}
	const priorityIndex = inspectionQueue.findIndex((task) => task.priority === "pending-title");
	const next = priorityIndex !== -1 ? inspectionQueue.splice(priorityIndex, 1)[0] : inspectionQueue.shift();
	if (!next) return;
	activeInspections += 1;
	inspectionStarts.push(now);
	next.run().finally(() => {
		activeInspections = Math.max(0, activeInspections - 1);
		if (inspectionQueue.length > 0) scheduleInspectionPump();
	});
	if (inspectionQueue.length > 0) scheduleInspectionPump();
}
function enqueueAgentProcessInspection(task) {
	inspectionQueue.push(task);
	pumpInspectionQueue();
}
var EXTRA_TITLE_AGENT_TOKEN_RE = /(?<![\w./\\-])(?:cursor-agent|pi)(?:\.(?:exe|cmd|bat|ps1))?(?![\w./\\-])/i;
function titleHasExplicitAgentIdentity(title) {
	if (!title) return false;
	if (title.startsWith(". ") || title.startsWith("* ") || title.startsWith("✳") || isGeminiTerminalTitle(title) || isPiTerminalTitle(title)) return true;
	return titleHasAnyLegacyAgentName(title) || AGY_AGENT_NAME_RE.test(title) || DROID_AGENT_NAME_RE.test(title) || HERMES_AGENT_NAME_RE.test(title) || EXTRA_TITLE_AGENT_TOKEN_RE.test(title);
}
function titleIsInconclusiveNativeDroidTitle(title) {
	return /\bDroid\b/i.test(title) && detectAgentStatusFromTitle(title) === null;
}
var lastCompletionIdentityByPaneKey = /* @__PURE__ */ new Map();
var IDLE_POLL_INTERVAL_MS = 2e3;
var ACTIVE_POLL_INTERVAL_MS = 750;
var HIDDEN_POLL_INTERVAL_MS = 3e3;
var NO_EVIDENCE_POLL_INTERVAL_MS = 15e3;
var NO_EVIDENCE_ACTIVITY_HOT_WINDOW_MS = 1e4;
var PENDING_TITLE_TTL_MS = Math.max(2e3, 15500);
var PENDING_TITLE_MAX_TTL_MS = Math.max(3e4, PENDING_TITLE_TTL_MS);
var COMPLETION_REPLAY_GUARD_MS = 1e3;
var HOOK_DONE_QUIET_MS = 1500;
var CODEX_ATTENTION_QUIET_MS = 1500;
var POLL_TIER_INTERVAL_MS = {
	active: ACTIVE_POLL_INTERVAL_MS,
	idle: IDLE_POLL_INTERVAL_MS,
	hidden: HIDDEN_POLL_INTERVAL_MS,
	"no-evidence": NO_EVIDENCE_POLL_INTERVAL_MS
};
function isCompletionHookState(state) {
	return state === "done";
}
function isAttentionHookState(state) {
	return state === "waiting" || state === "blocked";
}
function createAgentCompletionCoordinator(options) {
	let disposed = false;
	let agentIdentityEstablished = false;
	let hasAgentRunEvidence = false;
	let workingStatusObserved = false;
	let lastTitleStatus = null;
	let currentTurn = 0;
	let processSession = 0;
	let lastCompletionToken = null;
	let lastCompletionAt = 0;
	let lastCompletedTurn = null;
	let lastCompletionSource = null;
	let lastCompletionIdentity = null;
	let lastAttentionToken = null;
	let lastForegroundAgent = null;
	let requiresFreshWorking = false;
	let pollTimer = null;
	let pendingTitleTimer = null;
	let pendingHookDoneTimer = null;
	let pendingHookDoneTitle = null;
	let pendingHookDonePayload = null;
	let pendingCodexAttentionTimer = null;
	let pendingProcessExitAgent = null;
	let pendingTitleSequence = 0;
	let pendingTitle = null;
	let inspectionInFlight = false;
	let inspectionGeneration = 0;
	let consecutiveInspectionErrors = 0;
	let pollTrackingStarted = false;
	let pollTimerTier = null;
	let lastPaneActivityAt = 0;
	function clearPollTimer() {
		if (pollTimer === null) return;
		clearTimeout(pollTimer);
		pollTimer = null;
		pollTimerTier = null;
	}
	function clearPendingTitleTimer() {
		if (pendingTitleTimer === null) return;
		clearTimeout(pendingTitleTimer);
		pendingTitleTimer = null;
	}
	function clearPendingHookDone() {
		if (pendingHookDoneTimer !== null) {
			clearTimeout(pendingHookDoneTimer);
			pendingHookDoneTimer = null;
		}
		pendingHookDoneTitle = null;
		pendingHookDonePayload = null;
	}
	function clearPendingCodexAttention() {
		if (pendingCodexAttentionTimer !== null) {
			clearTimeout(pendingCodexAttentionTimer);
			pendingCodexAttentionTimer = null;
		}
	}
	function establishAgentEvidence() {
		agentIdentityEstablished = true;
		hasAgentRunEvidence = true;
		scheduleNextPoll();
	}
	function clearAgentRunEvidence() {
		agentIdentityEstablished = false;
		hasAgentRunEvidence = false;
		workingStatusObserved = false;
		pendingProcessExitAgent = null;
		dropPendingTitle();
	}
	function completionToken(source) {
		if (workingStatusObserved) return `turn:${currentTurn}`;
		if (lastForegroundAgent) return `process:${processSession}`;
		return `${source}:${currentTurn}:${processSession}`;
	}
	function hookCompletionIdentity(payload) {
		if (typeof payload.stateStartedAt !== "number" || !Number.isFinite(payload.stateStartedAt)) return null;
		return [
			payload.state,
			payload.agentType ?? "",
			String(Math.trunc(payload.stateStartedAt))
		].join(":");
	}
	function hookCompletionAgentIdentity(payload) {
		return payload.agentType?.trim().toLowerCase() || null;
	}
	function doneShouldUseQuietWindow(payload) {
		return workingStatusObserved || isPiCompatibleAgentType(hookCompletionAgentIdentity(payload));
	}
	function hookAttentionToken(payload) {
		const identity = hookCompletionIdentity(payload);
		if (identity) return `identity:${identity}`;
		return [
			"turn",
			String(currentTurn),
			payload.state,
			payload.agentType ?? "",
			payload.toolName ?? "",
			payload.toolInput ?? "",
			payload.prompt
		].join(":");
	}
	function titleCompletionIdentity(title) {
		return title;
	}
	function titleCompletionAgentIdentity(title) {
		const normalized = title.toLowerCase();
		if (/\bcodex\b/.test(normalized)) return "codex";
		if (/\bclaude\b/.test(normalized)) return "claude";
		if (/\bgemini\b/.test(normalized)) return "gemini";
		if (/\bcursor(?: agent)?\b/.test(normalized)) return "cursor";
		if (/\bopencode\b/.test(normalized)) return "opencode";
		if (/\bdroid\b/.test(normalized)) return "droid";
		if (/\bhermes\b/.test(normalized)) return "hermes";
		if (/\baider\b/.test(normalized)) return "aider";
		if (/\bpi\b/.test(normalized) || normalized.includes("π")) return "pi";
		return null;
	}
	function completionIdentityAlreadyNotified(completionIdentity) {
		if (!completionIdentity) return false;
		const previous = lastCompletionIdentityByPaneKey.get(options.paneKey);
		if (!previous) return false;
		if (previous.source === completionIdentity.source) return previous.identity === completionIdentity.identity;
		return previous.agentIdentity !== null && completionIdentity.agentIdentity !== null && previous.agentIdentity === completionIdentity.agentIdentity;
	}
	function dispatchCompletion(source, title, optionsOverride = {}) {
		if (source !== "hook" && pendingHookDoneTimer !== null) return;
		if (requiresFreshWorking || lastCompletedTurn === currentTurn) return;
		if (!options.isLive() || !hasAgentRunEvidence) return;
		const now = Date.now();
		const token = completionToken(source);
		if (token === lastCompletionToken && now - lastCompletionAt < COMPLETION_REPLAY_GUARD_MS) return;
		if (completionIdentityAlreadyNotified(optionsOverride.completionIdentity)) return;
		lastCompletionToken = token;
		lastCompletionAt = now;
		lastCompletedTurn = currentTurn;
		lastCompletionSource = source;
		workingStatusObserved = false;
		clearPendingCodexAttention();
		if (optionsOverride.completionIdentity) lastCompletionIdentityByPaneKey.set(options.paneKey, optionsOverride.completionIdentity);
		if (source === "hook" && optionsOverride.agentStatus) options.dispatchHookLifecycle?.(optionsOverride.agentStatus);
		if (optionsOverride.quietedHookDone === true || source === "process-exit") options.dispatchCompletion(title, {
			source,
			quietedHookDone: optionsOverride.quietedHookDone === true,
			...optionsOverride.terminalIdleConfirmed === true ? { terminalIdleConfirmed: true } : {},
			...optionsOverride.agentStatus ? { agentStatus: optionsOverride.agentStatus } : {}
		});
		else options.dispatchCompletion(title);
	}
	function dispatchAttentionNotification(payload) {
		options.dispatchAttention?.(payload.agentType ?? options.paneKey, {
			source: "hook",
			agentStatus: payload
		});
	}
	function dispatchAttention(payload) {
		if (!options.dispatchAttention || !options.isLive() || !hasAgentRunEvidence) return;
		const token = hookAttentionToken(payload);
		if (token === lastAttentionToken) return;
		lastAttentionToken = token;
		options.dispatchHookLifecycle?.(payload);
		if (payload.agentType === "codex") {
			clearPendingCodexAttention();
			pendingCodexAttentionTimer = setTimeout(() => {
				pendingCodexAttentionTimer = null;
				if (!options.isLive() || !hasAgentRunEvidence) return;
				dispatchAttentionNotification(payload);
			}, CODEX_ATTENTION_QUIET_MS);
			return;
		}
		dispatchAttentionNotification(payload);
	}
	function scheduleHookDoneCompletion(title, payload) {
		pendingHookDoneTitle = title;
		pendingHookDonePayload = payload;
		if (pendingHookDoneTimer !== null) return;
		pendingHookDoneTimer = setTimeout(() => {
			pendingHookDoneTimer = null;
			const pendingTitle$1 = pendingHookDoneTitle;
			const pendingPayload = pendingHookDonePayload;
			pendingHookDoneTitle = null;
			pendingHookDonePayload = null;
			if (pendingTitle$1) {
				const hookIdentity = pendingPayload ? hookCompletionIdentity(pendingPayload) : null;
				dispatchCompletion("hook", pendingTitle$1, {
					quietedHookDone: true,
					...pendingPayload ? { agentStatus: pendingPayload } : {},
					...hookIdentity ? { completionIdentity: {
						source: "hook",
						identity: hookIdentity,
						agentIdentity: pendingPayload ? hookCompletionAgentIdentity(pendingPayload) : null
					} } : {}
				});
			}
		}, HOOK_DONE_QUIET_MS);
	}
	function dropPendingTitle() {
		clearPendingTitleTimer();
		pendingTitle = null;
	}
	function dispatchPendingTitleIfEligible() {
		if (!pendingTitle || !pendingTitle.validatedByFreshInspection || !agentIdentityEstablished || !hasAgentRunEvidence) return;
		const title = pendingTitle.title;
		dropPendingTitle();
		markTitleCompletionNotified(title);
		dispatchCompletion("title", title, { completionIdentity: {
			source: "title",
			identity: titleCompletionIdentity(title),
			agentIdentity: titleCompletionAgentIdentity(title)
		} });
	}
	function schedulePendingTitleExpiry() {
		clearPendingTitleTimer();
		const pending = pendingTitle;
		if (!pending) return;
		const remaining = pending.expiresAt - Date.now();
		if (remaining <= 0) {
			pendingTitle = null;
			scheduleNextPoll();
			return;
		}
		pendingTitleTimer = setTimeout(() => {
			pendingTitleTimer = null;
			if (!pendingTitle) return;
			if (!pendingTitle.firstInspectionFinished && Date.now() < pendingTitle.maxExpiresAt) {
				pendingTitle.expiresAt = Math.min(Date.now() + 500, pendingTitle.maxExpiresAt);
				schedulePendingTitleExpiry();
				return;
			}
			pendingTitle = null;
			scheduleNextPoll();
		}, remaining);
	}
	function holdTitleCompletionPending(title) {
		const now = Date.now();
		pendingTitle = {
			id: ++pendingTitleSequence,
			title,
			expiresAt: Math.min(now + PENDING_TITLE_TTL_MS, now + PENDING_TITLE_MAX_TTL_MS),
			maxExpiresAt: now + PENDING_TITLE_MAX_TTL_MS,
			firstInspectionFinished: false,
			validatedByFreshInspection: false
		};
		schedulePendingTitleExpiry();
		requestInspection("pending-title");
	}
	function handleRecognizedProcess(process) {
		pendingProcessExitAgent = null;
		if (lastForegroundAgent?.agent !== process.agent) {
			if (lastForegroundAgent && hasAgentRunEvidence) {
				if (options.shouldSuppressProcessReplacementCompletion?.(lastForegroundAgent, process) !== true) dispatchCompletion("process-exit", lastForegroundAgent.processName, { completionIdentity: {
					source: "process-exit",
					identity: `${lastForegroundAgent.agent}:${lastForegroundAgent.processName}`,
					agentIdentity: lastForegroundAgent.agent
				} });
			}
			processSession += 1;
		}
		lastForegroundAgent = process;
		establishAgentEvidence();
	}
	function handleProcessInspectionResult(result) {
		if (result.unavailable === true) {
			pendingProcessExitAgent = null;
			consecutiveInspectionErrors += 1;
			scheduleNextPoll();
			return false;
		}
		consecutiveInspectionErrors = 0;
		const recognized = recognizeAgentProcess(result.foregroundProcess);
		if (recognized) {
			handleRecognizedProcess(recognized);
			return true;
		}
		if (pendingHookDoneTimer !== null || pendingCodexAttentionTimer !== null) {
			scheduleNextPoll();
			return false;
		}
		if (lastForegroundAgent && hasAgentRunEvidence) {
			if (result.hasChildProcesses) {
				pendingProcessExitAgent = null;
				scheduleNextPoll();
				return false;
			}
			if (!pendingProcessExitAgent || pendingProcessExitAgent.agent !== lastForegroundAgent.agent || pendingProcessExitAgent.processName !== lastForegroundAgent.processName) {
				pendingProcessExitAgent = lastForegroundAgent;
				scheduleNextPoll();
				return false;
			}
			const exited = lastForegroundAgent;
			pendingProcessExitAgent = null;
			if (options.shouldSuppressConfirmedProcessExitCompletion?.(exited) !== true) dispatchCompletion("process-exit", exited.processName, {
				terminalIdleConfirmed: true,
				completionIdentity: {
					source: "process-exit",
					identity: `${exited.agent}:${exited.processName}`,
					agentIdentity: exited.agent
				}
			});
			lastForegroundAgent = null;
			clearAgentRunEvidence();
		} else {
			lastForegroundAgent = null;
			clearAgentRunEvidence();
		}
		return false;
	}
	function requestInspection(priority) {
		if (disposed || inspectionInFlight || !options.isLive()) return;
		if (priority === "cadence" && !shouldRunCadenceInspection()) return;
		const ptyId = options.getPtyId();
		if (!ptyId) return;
		inspectionInFlight = true;
		const generationAtRequest = inspectionGeneration;
		const pendingTitleIdAtRequest = priority === "pending-title" ? pendingTitle?.id : null;
		enqueueAgentProcessInspection({
			priority,
			run: async () => {
				let inspectedRecognizedAgent = false;
				let inspectionSucceeded = false;
				try {
					const result = await options.inspectProcess(options.getSettings(), ptyId);
					if (!disposed && generationAtRequest === inspectionGeneration) {
						if (!pendingTitle || priority === "pending-title" && pendingTitle.id === pendingTitleIdAtRequest) inspectedRecognizedAgent = handleProcessInspectionResult(result);
						inspectionSucceeded = true;
					}
				} catch {
					pendingProcessExitAgent = null;
					consecutiveInspectionErrors += 1;
				} finally {
					inspectionInFlight = false;
					if (generationAtRequest !== inspectionGeneration) if (pendingTitle) requestInspection("pending-title");
					else scheduleNextPoll();
					else {
						if (pendingTitle) if (priority === "pending-title" && pendingTitle.id === pendingTitleIdAtRequest) {
							pendingTitle.firstInspectionFinished = true;
							if (inspectionSucceeded && inspectedRecognizedAgent) {
								pendingTitle.validatedByFreshInspection = true;
								dispatchPendingTitleIfEligible();
							} else if (!inspectionSucceeded) dropPendingTitle();
							schedulePendingTitleExpiry();
						} else requestInspection("pending-title");
						scheduleNextPoll();
					}
				}
			}
		});
	}
	function shouldRunCadenceInspection() {
		return hasAgentRunEvidence || lastForegroundAgent !== null || options.shouldPollProcessCadence?.() !== false;
	}
	function isHiddenBackstop() {
		return options.shouldPollProcessCadence?.() === false;
	}
	function paneActivityWithinHotWindow() {
		return lastPaneActivityAt > 0 && Date.now() - lastPaneActivityAt < NO_EVIDENCE_ACTIVITY_HOT_WINDOW_MS;
	}
	function currentPollTier() {
		if (isHiddenBackstop()) return "hidden";
		if (lastForegroundAgent) return "active";
		if (hasAgentRunEvidence) return "idle";
		if (options.isProcessInspectionCostly?.() === true && !paneActivityWithinHotWindow()) return "no-evidence";
		return "idle";
	}
	function nextPollInterval(tier) {
		const base = POLL_TIER_INTERVAL_MS[tier];
		const backoff = consecutiveInspectionErrors > 0 ? Math.min(Math.max(1e4, base), base * 2 ** consecutiveInspectionErrors) : base;
		const jitter = 1 + (Math.random() * .2 - .1);
		return Math.round(backoff * jitter);
	}
	function scheduleNextPoll() {
		if (disposed || !pollTrackingStarted || !options.isLive() || pendingTitle) return;
		const tier = currentPollTier();
		if (pollTimer !== null) if (pollTimerTier !== null && POLL_TIER_INTERVAL_MS[tier] < POLL_TIER_INTERVAL_MS[pollTimerTier]) clearPollTimer();
		else return;
		if (!shouldRunCadenceInspection()) return;
		if (!options.getPtyId()) return;
		pollTimerTier = tier;
		pollTimer = setTimeout(() => {
			pollTimer = null;
			pollTimerTier = null;
			requestInspection("cadence");
		}, nextPollInterval(tier));
	}
	function recordPaneActivity() {
		lastPaneActivityAt = Date.now();
		if (pollTimer === null || pollTimerTier === "no-evidence") scheduleNextPoll();
	}
	function observeOutputActivity() {
		recordPaneActivity();
	}
	function recordTitleWorking() {
		clearPendingHookDone();
		if (lastCompletionSource === "hook" && Date.now() - lastCompletionAt < COMPLETION_REPLAY_GUARD_MS) return false;
		clearPendingCodexAttention();
		workingStatusObserved = true;
		requiresFreshWorking = false;
		lastCompletionIdentityByPaneKey.delete(options.paneKey);
		currentTurn += 1;
		dropPendingTitle();
		return true;
	}
	function observeTitleWorking() {
		recordTitleWorking();
	}
	function observeTitle(title) {
		recordPaneActivity();
		const status = detectAgentStatusFromTitle(title);
		const isInconclusiveNativeDroidTitle = titleIsInconclusiveNativeDroidTitle(title);
		const hasExplicitAgentIdentity = titleHasExplicitAgentIdentity(title) && !isInconclusiveNativeDroidTitle;
		const hadPendingTitle = pendingTitle !== null;
		if (hasExplicitAgentIdentity) establishAgentEvidence();
		if (status === "working") {
			if (!recordTitleWorking()) return;
		} else if (lastTitleStatus === "working") {
			if (isInconclusiveNativeDroidTitle) {
				lastTitleStatus = status;
				return;
			}
			if (status === null && !titleHasExplicitAgentIdentity(title)) {
				holdTitleCompletionPending(title);
				lastTitleStatus = status;
				return;
			}
			if (agentIdentityEstablished && hasAgentRunEvidence) {
				markTitleCompletionNotified(title);
				dispatchCompletion("title", title, { completionIdentity: {
					source: "title",
					identity: titleCompletionIdentity(title),
					agentIdentity: titleCompletionAgentIdentity(title)
				} });
			} else holdTitleCompletionPending(title);
		} else if (hadPendingTitle && status !== null && hasExplicitAgentIdentity) {
			dropPendingTitle();
			markTitleCompletionNotified(title);
			dispatchCompletion("title", title, { completionIdentity: {
				source: "title",
				identity: titleCompletionIdentity(title),
				agentIdentity: titleCompletionAgentIdentity(title)
			} });
		}
		lastTitleStatus = status;
	}
	function observeClassifiedTitleCompletion(title) {
		if (titleHasExplicitAgentIdentity(title)) establishAgentEvidence();
		if (agentIdentityEstablished && hasAgentRunEvidence) {
			markTitleCompletionNotified(title);
			dispatchCompletion("title", title, { completionIdentity: {
				source: "title",
				identity: titleCompletionIdentity(title),
				agentIdentity: titleCompletionAgentIdentity(title)
			} });
		} else holdTitleCompletionPending(title);
	}
	function observeHookStatus(payload) {
		recordPaneActivity();
		if (options.shouldSuppressHookCompletion?.(payload)) {
			if (isAttentionHookState(payload.state)) {
				clearPendingHookDone();
				clearPendingCodexAttention();
			}
			return;
		}
		if (isRecognizedAgentType(payload.agentType)) establishAgentEvidence();
		if (payload.state === "working") {
			clearPendingHookDone();
			clearPendingCodexAttention();
			workingStatusObserved = true;
			requiresFreshWorking = false;
			lastCompletionIdentity = null;
			lastAttentionToken = null;
			currentTurn += 1;
			dropPendingTitle();
			options.dispatchHookLifecycle?.(payload);
			return;
		}
		if (isAttentionHookState(payload.state)) {
			clearPendingHookDone();
			dispatchAttention(payload);
			return;
		}
		if (payload.state === "done" && payload.sessionBoundary === true) return;
		if (isCompletionHookState(payload.state)) {
			clearPendingCodexAttention();
			if (isRecognizedAgentType(payload.agentType)) establishAgentEvidence();
			const hookIdentity = hookCompletionIdentity(payload);
			if (hookIdentity && lastCompletionIdentity?.source === "hook" && hookIdentity === lastCompletionIdentity.identity) {
				if (payload.state === "done" && pendingHookDoneTimer !== null) scheduleHookDoneCompletion(payload.agentType ?? options.paneKey, payload);
				return;
			}
			if (!workingStatusObserved && lastCompletionSource === "hook" && lastCompletedTurn === currentTurn && Date.now() - lastCompletionAt >= COMPLETION_REPLAY_GUARD_MS) currentTurn += 1;
			if (payload.state === "done" && doneShouldUseQuietWindow(payload)) {
				lastCompletionIdentity = hookIdentity ? {
					source: "hook",
					identity: hookIdentity,
					agentIdentity: hookCompletionAgentIdentity(payload)
				} : null;
				scheduleHookDoneCompletion(payload.agentType ?? options.paneKey, payload);
				return;
			}
			lastCompletionIdentity = hookIdentity ? {
				source: "hook",
				identity: hookIdentity,
				agentIdentity: hookCompletionAgentIdentity(payload)
			} : null;
			dispatchCompletion("hook", payload.agentType ?? options.paneKey, {
				agentStatus: payload,
				...lastCompletionIdentity ? { completionIdentity: lastCompletionIdentity } : {}
			});
		}
	}
	function markTitleCompletionNotified(title) {
		lastCompletionIdentity = {
			source: "title",
			identity: titleCompletionIdentity(title),
			agentIdentity: titleCompletionAgentIdentity(title)
		};
	}
	function startProcessTracking() {
		pollTrackingStarted = true;
		scheduleNextPoll();
	}
	function hasPendingHookDoneCompletion() {
		return pendingHookDoneTimer !== null;
	}
	function resetCompletionState(options$1 = {}) {
		clearPendingHookDone();
		clearPendingCodexAttention();
		dropPendingTitle();
		agentIdentityEstablished = false;
		hasAgentRunEvidence = false;
		workingStatusObserved = false;
		lastTitleStatus = null;
		lastCompletionToken = null;
		lastCompletionAt = 0;
		lastCompletedTurn = null;
		lastCompletionSource = null;
		lastCompletionIdentity = null;
		lastAttentionToken = null;
		lastForegroundAgent = null;
		requiresFreshWorking = options$1.requireFreshWorking ?? false;
		inspectionGeneration += 1;
	}
	function dispose() {
		disposed = true;
		clearPollTimer();
		clearPendingHookDone();
		clearPendingCodexAttention();
		dropPendingTitle();
		if (!options.isLive()) lastCompletionIdentityByPaneKey.delete(options.paneKey);
	}
	return {
		observeTitle,
		observeClassifiedTitleCompletion,
		observeTitleWorking,
		observeOutputActivity,
		observeHookStatus,
		startProcessTracking,
		hasPendingHookDoneCompletion,
		resetCompletionState,
		dispose
	};
}
async function playDesktopNotificationSound(customSoundId, customSoundVolume) {
	if (!customSoundId || customSoundId === "system") return false;
	try {
		const result = await window.api.notifications.playSound({ volume: customSoundVolume ?? void 0 });
		if (!result.played && result.reason !== "deduped") console.warn("Failed to play custom notification sound:", result.reason);
		return result.played;
	} catch (err) {
		console.warn("Failed to play custom notification sound:", err);
		return false;
	}
}
var shownThisSession = false;
function showBlockedNotificationFallbackToast() {
	if (shownThisSession) return;
	shownThisSession = true;
	toast.warning(translate("auto.lib.blocked.notification.fallback.de50bef680", "macOS is blocking Orca notifications"), {
		description: translate("auto.components.onboarding.mac.notification.permission.card.721d2bedb6", "Turn on Allow notifications for Orca in System Settings."),
		action: {
			label: translate("auto.components.onboarding.NotificationStep.4f6a1da718", "Open System Settings"),
			onClick: () => {
				window.api.notifications.openSystemSettings();
			}
		}
	});
}
function isSupersededAgentCompletionSnapshot(storedAgentStatus, snapshot) {
	if (!storedAgentStatus || !snapshot) return false;
	if (typeof snapshot.stateStartedAt !== "number") return storedAgentStatus.state !== snapshot.state;
	if (storedAgentStatus.stateStartedAt > snapshot.stateStartedAt) return true;
	return storedAgentStatus.stateStartedAt === snapshot.stateStartedAt && storedAgentStatus.state !== snapshot.state;
}
function getPaneKeyTabId(paneKey) {
	const parsed = parsePaneKey(paneKey);
	if (parsed) return parsed.tabId;
	const sepIdx = paneKey.indexOf(":");
	if (sepIdx <= 0 || sepIdx !== paneKey.lastIndexOf(":") || sepIdx === paneKey.length - 1) return null;
	return paneKey.slice(0, sepIdx);
}
function isSuppressedPtyHint(state, ptyId) {
	return Boolean(ptyId && state.suppressedPtyExitIds?.[ptyId]);
}
function hasLivePtyForWorktree(state, candidateWorktreeId) {
	return (state.tabsByWorktree[candidateWorktreeId] ?? []).some((tab) => (state.ptyIdsByTabId[tab.id] ?? []).some((ptyId) => !isSuppressedPtyHint(state, ptyId)));
}
function hasLivePtyForPaneKey(state, paneKey) {
	if (!paneKey) return false;
	const tabId = getPaneKeyTabId(paneKey);
	return tabId !== null && (state.ptyIdsByTabId[tabId] ?? []).some((ptyId) => !isSuppressedPtyHint(state, ptyId));
}
function hasLivePtyForNotification(state, worktreeId, paneKey) {
	return hasLivePtyForWorktree(state, worktreeId) || hasLivePtyForPaneKey(state, paneKey);
}
function layoutContainsLeaf(node, leafId) {
	if (!node) return false;
	if (node.type === "leaf") return node.leafId === leafId;
	return layoutContainsLeaf(node.first, leafId) || layoutContainsLeaf(node.second, leafId);
}
function isCurrentLivePaneKey(state, worktreeId, paneKey) {
	const parsed = parsePaneKey(paneKey);
	if (!parsed) return false;
	if (Object.entries(state.tabsByWorktree).some(([candidateWorktreeId, tabs]) => candidateWorktreeId !== worktreeId && tabs.some((tab) => tab.id === parsed.tabId))) return false;
	const livePtyIds = (state.ptyIdsByTabId[parsed.tabId] ?? []).filter((ptyId) => !isSuppressedPtyHint(state, ptyId));
	if (livePtyIds.length === 0) return false;
	const layout = state.terminalLayoutsByTabId?.[parsed.tabId];
	if (!layout) return true;
	if (!layoutContainsLeaf(layout.root, parsed.leafId)) return false;
	const leafPtyId = layout.ptyIdsByLeafId?.[parsed.leafId];
	return leafPtyId === void 0 || livePtyIds.includes(leafPtyId);
}
function isCurrentKnownPaneKey(state, worktreeId, paneKey) {
	const parsed = parsePaneKey(paneKey);
	if (!parsed) return false;
	let targetTabPtyId;
	for (const [candidateWorktreeId, tabs] of Object.entries(state.tabsByWorktree)) {
		const tab = tabs.find((candidate) => candidate.id === parsed.tabId);
		if (!tab) continue;
		if (candidateWorktreeId !== worktreeId) return false;
		targetTabPtyId = tab.ptyId;
	}
	if (targetTabPtyId === void 0) return false;
	const layout = state.terminalLayoutsByTabId?.[parsed.tabId];
	if (layout?.root && !layoutContainsLeaf(layout.root, parsed.leafId)) return false;
	const leafPtyId = layout?.ptyIdsByLeafId?.[parsed.leafId];
	const ptyHints = [targetTabPtyId, leafPtyId].filter((ptyId) => Boolean(ptyId));
	return ptyHints.length === 0 || ptyHints.some((ptyId) => !isSuppressedPtyHint(state, ptyId));
}
function hasActiveWorktreeState(state, worktreeId) {
	if (hasLivePtyForWorktree(state, worktreeId)) return true;
	if ((state.browserTabsByWorktree?.[worktreeId] ?? []).length > 0) return true;
	if (getWorktreeMapFromState(state).get(worktreeId)?.workspaceStatus === "in-progress") return true;
	if (Object.values(state.retainedAgentsByPaneKey ?? {}).some((agent) => agent.worktreeId === worktreeId)) return true;
	const tabs = state.tabsByWorktree[worktreeId] ?? [];
	const tabIds = new Set(tabs.map((tab) => tab.id));
	if (tabIds.size === 0) return false;
	const now = Date.now();
	return Object.values(state.agentStatusByPaneKey ?? {}).some((entry) => {
		const tabId = getPaneKeyTabId(entry.paneKey);
		return tabId !== null && tabIds.has(tabId) && isExplicitAgentStatusFresh(entry, now, 18e5);
	});
}
function countReposWithWorktrees(state) {
	let count = 0;
	for (const worktrees of Object.values(state.worktreesByRepo)) if (worktrees.length > 0) count += 1;
	return count;
}
function countReposNeedingNotificationDisambiguation(state) {
	const activeRepoIds = /* @__PURE__ */ new Set();
	const worktreeMap = getWorktreeMapFromState(state);
	for (const worktreeId of Object.keys(state.tabsByWorktree)) {
		if (!hasActiveWorktreeState(state, worktreeId)) continue;
		const repoId = worktreeMap.get(worktreeId)?.repoId;
		if (repoId) activeRepoIds.add(repoId);
	}
	for (const [repoId, worktrees] of Object.entries(state.worktreesByRepo)) {
		if (activeRepoIds.has(repoId)) continue;
		if (worktrees.some((worktree) => hasActiveWorktreeState(state, worktree.id))) activeRepoIds.add(repoId);
	}
	return Math.max(activeRepoIds.size, countReposWithWorktrees(state));
}
function isOrcaWindowForegroundFocused() {
	if (typeof document === "undefined") return true;
	return document.visibilityState === "visible" && document.hasFocus();
}
function isVisibleForegroundPaneKey(state, worktreeId, paneKey) {
	if (!isOrcaWindowForegroundFocused() || state.activeWorktreeId !== worktreeId) return false;
	const parsed = parsePaneKey(paneKey);
	if (!parsed || state.activeTabId !== parsed.tabId) return false;
	return state.terminalLayoutsByTabId?.[parsed.tabId]?.activeLeafId === parsed.leafId;
}
var import_react = /* @__PURE__ */ __toESM(require_react());
var AGENT_NOTIFICATION_SNAPSHOT_MAX_AGE_MS = 1e4;
function agentSnapshotMatchesExplicitTitle(snapshot, explicitTitleAgentType) {
	return !snapshot || !explicitTitleAgentType || snapshot.agentType === explicitTitleAgentType;
}
function hasFreshActiveHookStatus(snapshot, explicitTitleAgentType) {
	const activeHookAgentForTitle = resolveCompatibleAgentTypeForOwner(snapshot?.agentType, explicitTitleAgentType);
	const titleNamesDifferentKnownAgent = explicitTitleAgentType && snapshot?.agentType && snapshot.agentType !== "unknown" && activeHookAgentForTitle !== explicitTitleAgentType;
	return Boolean(isFreshNonDoneAgentStatus(snapshot) && !titleNamesDifferentKnownAgent);
}
function dispatchTerminalNotification(worktreeId, event) {
	const state = useAppStore.getState();
	const explicitTitleAgentType = event.source === "agent-task-complete" && event.terminalTitle ? resolveCommittedTitleAgentType(event.terminalTitle) : null;
	const storedAgentStatus = event.source === "agent-task-complete" && event.paneKey ? state.agentStatusByPaneKey[event.paneKey] : void 0;
	const eventAgentStatusSnapshot = event.source === "agent-task-complete" && agentSnapshotMatchesExplicitTitle(event.agentStatusSnapshot, explicitTitleAgentType) ? event.agentStatusSnapshot : void 0;
	const freshStoredAgentStatus = storedAgentStatus && Date.now() - storedAgentStatus.updatedAt <= AGENT_NOTIFICATION_SNAPSHOT_MAX_AGE_MS && agentSnapshotMatchesExplicitTitle(storedAgentStatus, explicitTitleAgentType) ? storedAgentStatus : void 0;
	if (event.source === "agent-task-complete" && event.agentCompletionSource !== "process-exit" && !eventAgentStatusSnapshot && hasFreshActiveHookStatus(storedAgentStatus, explicitTitleAgentType)) return;
	const agentStatus = event.source === "agent-task-complete" ? eventAgentStatusSnapshot ?? (event.agentCompletionSource === "process-exit" && freshStoredAgentStatus?.state !== "done" ? void 0 : freshStoredAgentStatus) : void 0;
	if (event.source === "agent-task-complete" && isSupersededAgentCompletionSnapshot(storedAgentStatus, eventAgentStatusSnapshot)) return;
	const agentNotificationStateStartedAt = freshStoredAgentStatus?.stateStartedAt ?? eventAgentStatusSnapshot?.stateStartedAt;
	const hasFreshAgentStatus = Boolean(agentStatus);
	const hasLivePty = hasLivePtyForNotification(state, worktreeId, event.paneKey);
	if (!hasLivePty && !hasFreshAgentStatus) return;
	if (event.source === "agent-task-complete") {
		const terminalAttentionEnabled = state.settings?.experimentalTerminalAttention === true;
		let tabId = null;
		if (event.paneKey) {
			tabId = getPaneKeyTabId(event.paneKey);
			const isCurrentPane = hasLivePty ? isCurrentLivePaneKey(state, worktreeId, event.paneKey) : isCurrentKnownPaneKey(state, worktreeId, event.paneKey);
			if (!tabId || !isCurrentPane) return;
		}
		if (event.paneKey ? !isVisibleForegroundPaneKey(state, worktreeId, event.paneKey) : state.activeWorktreeId !== worktreeId || !isOrcaWindowForegroundFocused()) {
			state.markWorktreeUnread(worktreeId);
			if (event.paneKey) state.markAgentCompletionPaneUnread(event.paneKey);
			if (terminalAttentionEnabled && tabId && event.paneKey) {
				state.markTerminalTabUnread(tabId);
				state.markTerminalPaneUnread(event.paneKey);
			}
		}
	}
	if (event.suppressOsNotification) return;
	const worktree = getWorktreeMapFromState(state).get(worktreeId);
	const repo = worktree ? getRepoMapFromState(state).get(worktree.repoId) : null;
	const customSoundId = state.settings?.notifications?.customSoundId ?? "system";
	const customSoundVolume = state.settings?.notifications?.customSoundVolume ?? null;
	const agentSnapshot = agentStatus ? {
		agentType: agentStatus.agentType,
		agentState: agentStatus.state,
		agentPrompt: agentStatus.prompt,
		agentToolName: agentStatus.toolName,
		agentToolInput: agentStatus.toolInput,
		agentLastAssistantMessage: agentStatus.lastAssistantMessage,
		agentInterrupted: agentStatus.interrupted
	} : {};
	const notificationId = event.source === "agent-task-complete" ? buildAgentNotificationId({
		worktreeId,
		paneKey: event.paneKey,
		stateStartedAt: agentNotificationStateStartedAt
	}) : null;
	window.api.notifications.dispatch({
		source: event.source,
		...notificationId ? { notificationId } : {},
		worktreeId,
		paneKey: event.paneKey,
		repoLabel: repo?.displayName,
		worktreeLabel: worktree?.displayName || worktree?.branch || worktreeId,
		hasMultipleActiveRepos: countReposNeedingNotificationDisambiguation(state) > 1,
		terminalTitle: event.terminalTitle,
		isActiveWorktree: state.activeWorktreeId === worktreeId,
		...agentSnapshot
	}).then((result) => {
		if (result.delivered) {
			playDesktopNotificationSound(customSoundId, customSoundVolume);
			return;
		}
		if (result.reason === "blocked-by-system") showBlockedNotificationFallbackToast();
	}).catch((err) => {
		console.warn("Failed to dispatch notification:", err);
	});
}
function useNotificationDispatch(worktreeId) {
	return (0, import_react.useCallback)((event) => dispatchTerminalNotification(worktreeId, event), [worktreeId]);
}
function isAskUserQuestionTool(toolName) {
	const normalized = toolName?.replaceAll(/[^a-z0-9]/gi, "").toLowerCase();
	return normalized === "askuserquestion" || normalized === "requestuserinput";
}
var QUESTION_ANSWER_ENTER_INPUTS = new Set([
	"\r",
	"\n",
	"\r\n",
	"\x1B[13u",
	"\x1B[13;1u"
]);
var QUESTION_ANSWER_DIGIT_INPUTS = /* @__PURE__ */ new Set("123456789");
function isPotentialQuestionAnsweredSubmitInput(data) {
	return QUESTION_ANSWER_ENTER_INPUTS.has(data) || QUESTION_ANSWER_DIGIT_INPUTS.has(data);
}
function readSingleSelectOptionCount(interactivePrompt) {
	if (!interactivePrompt) return null;
	try {
		const parsed = JSON.parse(interactivePrompt);
		if (!Array.isArray(parsed.questions) || parsed.questions.length !== 1) return -1;
		const [question] = parsed.questions;
		if (!question || question.multiSelect === true || !Array.isArray(question.options)) return -1;
		return question.options.length;
	} catch {
		return -1;
	}
}
function isQuestionAnsweredSubmitInput(data, interactivePrompt) {
	if (!isPotentialQuestionAnsweredSubmitInput(data)) return false;
	const optionCount = readSingleSelectOptionCount(interactivePrompt);
	if (optionCount === -1) return false;
	if (QUESTION_ANSWER_ENTER_INPUTS.has(data)) return true;
	if (optionCount === null) return false;
	return Number(data) <= optionCount;
}
var CODEX_AUTO_APPROVED_PERMISSION_STATES = ["waiting", "blocked"];
function isCodexAutoApprovedPermissionState(state) {
	return CODEX_AUTO_APPROVED_PERMISSION_STATES.some((permissionState) => permissionState === state);
}
function shouldSuppressCodexAutoApprovalStatus(payload, context) {
	if (payload.agentType !== "codex" || !isCodexAutoApprovedPermissionState(payload.state)) return false;
	if (isAskUserQuestionTool(payload.toolName)) return false;
	const state = useAppStore.getState();
	if (typeof state.getAgentLaunchConfigForStatusMetadata !== "function") return false;
	const launchConfig = state.getAgentLaunchConfigForStatusMetadata({
		paneKey: context.paneKey,
		agentType: "codex",
		tabId: context.tabId,
		terminalHandle: context.terminalHandle,
		launchToken: context.launchToken,
		providerSession: context.providerSession,
		existingProviderSession: context.existingProviderSession
	});
	if (!launchConfig) return false;
	return resolveTuiAgentPermissionMode({
		agent: "codex",
		agentArgs: launchConfig.agentArgs,
		agentEnv: launchConfig.agentEnv
	}) === "yolo";
}
function shouldSuppressCodexAutoApprovalSyntheticTitle(title, context) {
	if (title !== getSyntheticAgentTitleProfile("codex")?.permissionLabel) return false;
	return shouldSuppressCodexAutoApprovalStatus({
		state: "waiting",
		prompt: "",
		agentType: "codex"
	}, context);
}
function createCodexAutoApprovalHookCompletionSuppressor(paneKey, getContext) {
	return (payload) => shouldSuppressCodexAutoApprovalStatus(payload, {
		paneKey,
		...getContext?.()
	});
}
var handlersByPaneKey = /* @__PURE__ */ new Map();
function registerAgentHookTerminalLifecycleHandler(paneKey, handler) {
	handlersByPaneKey.set(paneKey, handler);
	return () => {
		if (handlersByPaneKey.get(paneKey) === handler) handlersByPaneKey.delete(paneKey);
	};
}
function dispatchAgentHookTerminalLifecycle(paneKey, payload) {
	handlersByPaneKey.get(paneKey)?.(payload);
}
function resolveAgentStatusTerminalTitle(payload, currentTitle) {
	const syntheticTitle = getSyntheticAgentTerminalTitle(payload.agentType, payload.state);
	if (!syntheticTitle) return currentTitle;
	if (shouldReplaceCurrentTitle(payload, currentTitle)) return syntheticTitle;
	return currentTitle;
}
function shouldReplaceCurrentTitle(payload, currentTitle) {
	if (!currentTitle?.trim()) return true;
	const currentStatus = classifyTitleActivity(currentTitle);
	if (currentStatus === "working") return true;
	if (payload.state === "done" && currentStatus === "permission") return true;
	const profile = getSyntheticAgentTitleProfile(payload.agentType);
	if (!profile) return false;
	if (currentTitle.trim().toLowerCase() === profile.workingLabel.toLowerCase()) return true;
	return payload.state === "blocked" || payload.state === "waiting";
}
var FOOTER_MARKER = "---";
var ORCA_LINE_PREFIX = "Orca:";
var CLIENT_ENVIRONMENT_FOOTER_BLOCK = /(^|\r?\n)---\r?\nOrca:[^\r\n]*\r?\nOS:[^\r\n]*(?:\r?\nShell:[^\r\n]*)?/;
function normalizeEnvironmentValue(value) {
	return value.trim().replace(/[\r\n]+/g, " ");
}
function formatClientEnvironmentInfo(info) {
	const version = normalizeEnvironmentValue(info.appVersion) || "unknown";
	const platform = normalizeEnvironmentValue(info.platform) || "unknown";
	const osRelease = normalizeEnvironmentValue(info.osRelease);
	const arch = normalizeEnvironmentValue(info.arch);
	const osParts = [
		platform,
		osRelease,
		arch ? `(${arch})` : ""
	].filter(Boolean);
	const lines = [`${ORCA_LINE_PREFIX} ${version}`, `OS: ${osParts.join(" ")}`];
	const shell = info.shell ? normalizeEnvironmentValue(info.shell) : "";
	if (shell) lines.push(`Shell: ${shell}`);
	return lines.join("\n");
}
function formatClientEnvironmentFooter(info) {
	return `${FOOTER_MARKER}\n${formatClientEnvironmentInfo(info)}`;
}
function hasClientEnvironmentFooter(text) {
	return CLIENT_ENVIRONMENT_FOOTER_BLOCK.test(text);
}
function stripClientEnvironmentFooter(text) {
	return text.replace(CLIENT_ENVIRONMENT_FOOTER_BLOCK, "$1");
}
function appendClientEnvironmentFooter(params) {
	if (hasClientEnvironmentFooter(params.message)) return params.message;
	const footer = formatClientEnvironmentFooter(params.info);
	const base = params.message.trimEnd();
	return base.length > 0 ? `${base}\n\n${footer}` : footer;
}
async function resolveClientEnvironmentInfo() {
	const platformInfo = resolvePlatformInfo();
	return {
		appVersion: await resolveAppVersion(),
		platform: platformInfo?.platform ?? resolveFallbackPlatform(),
		osRelease: platformInfo?.osRelease ?? "",
		arch: platformInfo?.arch ?? "",
		...platformInfo?.shell ? { shell: platformInfo.shell } : {}
	};
}
function resolvePlatformInfo() {
	try {
		return window.api?.platform?.get?.() ?? null;
	} catch {
		return null;
	}
}
function resolveFallbackPlatform() {
	try {
		return getRendererAppPlatform();
	} catch {
		return "unknown";
	}
}
async function resolveClientEnvironmentFooter() {
	return formatClientEnvironmentFooter(await resolveClientEnvironmentInfo());
}
async function resolveAppVersion() {
	try {
		const version = await window.api?.updater?.getVersion?.();
		if (typeof version === "string" && version.trim()) return version.trim();
	} catch {}
	return "unknown";
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var LinkRoutingPreferenceDialogContext = (0, import_react.createContext)(null);
function displayHostForUrl(url) {
	if (!url) return null;
	try {
		return new URL(url).host;
	} catch {
		return null;
	}
}
function LinkRoutingPreferenceDialogProvider({ children }) {
	const nextIdRef = (0, import_react.useRef)(0);
	const [queue$1, setQueue] = (0, import_react.useState)([]);
	const activeRequest = queue$1[0] ?? null;
	const activeRequestRef = (0, import_react.useRef)(activeRequest);
	const setContextualToursBlockingSurfaceVisible = useAppStore((s) => s.setContextualToursBlockingSurfaceVisible);
	const lastDisplayedRequestRef = (0, import_react.useRef)(activeRequest);
	activeRequestRef.current = activeRequest;
	if (activeRequest) lastDisplayedRequestRef.current = activeRequest;
	const displayedRequest = activeRequest ?? lastDisplayedRequestRef.current;
	const displayHost = displayHostForUrl(displayedRequest?.options.url);
	const openLinksInAppDefault = displayedRequest?.options.openLinksInAppDefault === true;
	const systemBrowserShortcutKeys = navigator.userAgent.includes("Mac") ? ["⇧", "⌘"] : ["Shift", "Ctrl"];
	(0, import_react.useEffect)(() => {
		setContextualToursBlockingSurfaceVisible(activeRequest !== null);
		return () => setContextualToursBlockingSurfaceVisible(false);
	}, [activeRequest, setContextualToursBlockingSurfaceVisible]);
	const requestPreference = (0, import_react.useCallback)((options = {}) => {
		return new Promise((resolve) => {
			const request = {
				id: nextIdRef.current,
				options,
				resolve
			};
			nextIdRef.current += 1;
			setQueue((currentQueue) => [...currentQueue, request]);
		});
	}, []);
	(0, import_react.useEffect)(() => {}, [requestPreference]);
	const settleActiveRequest = (0, import_react.useCallback)((openInOrca) => {
		const request = activeRequestRef.current;
		if (!request) return;
		request.resolve(openInOrca);
		setQueue((currentQueue) => {
			if (currentQueue[0]?.id === request.id) return currentQueue.slice(1);
			return currentQueue.filter((queuedRequest) => queuedRequest.id !== request.id);
		});
	}, []);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(LinkRoutingPreferenceDialogContext.Provider, {
		value: requestPreference,
		children: [children, /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
			open: activeRequest !== null,
			onOpenChange: (open) => !open && settleActiveRequest(false),
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogContent, {
				showCloseButton: false,
				overlayClassName: "!z-[140]",
				className: "!z-[150] gap-4 p-0 sm:max-w-[520px]",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "rounded-t-lg border-b border-border bg-muted/30 px-6 pt-5 pb-4",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, {
							className: "gap-3",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "flex items-center justify-between gap-3",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
									variant: "outline",
									className: "bg-background/70 text-muted-foreground",
									children: translate("auto.components.link.routing.preference.dialog.badge", "Terminal link")
								}), displayedRequest?.options.preview ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
									variant: "secondary",
									children: translate("auto.components.link.routing.preference.dialog.preview", "Preview")
								}) : null]
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "space-y-2",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, {
									className: "text-xl leading-tight",
									children: openLinksInAppDefault ? translate("auto.components.link.routing.preference.dialog.keep.title", "Keep terminal links in Orca's browser?") : translate("auto.components.link.routing.preference.dialog.title", "Open terminal links in Orca's browser?")
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, {
									className: "text-sm leading-relaxed",
									children: openLinksInAppDefault ? translate("auto.components.link.routing.preference.dialog.keep.description", "Or use your system browser by default.") : translate("auto.components.link.routing.preference.dialog.description", "Use Orca's browser for terminal links, or keep your system browser.")
								})]
							})]
						})
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "space-y-3 px-6",
						children: [displayHost ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex items-center gap-2 text-xs text-muted-foreground",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.link.routing.preference.dialog.link.label", "Link") }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "rounded-md border border-border bg-muted/30 px-2 py-1 font-mono",
								children: displayHost
							})]
						}) : null, /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex gap-2 rounded-lg border border-border bg-muted/20 p-3 text-xs leading-relaxed text-muted-foreground",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Settings, { className: "mt-0.5 size-3.5 shrink-0" }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "space-y-1",
								children: [
									/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", { children: translate("auto.components.link.routing.preference.dialog.orca.note", "Orca can use imported cookies for logged-in sites.") }),
									/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", { children: translate("auto.components.link.routing.preference.dialog.settings.note", "Change this later in Settings → Browser.") }),
									/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
										className: "flex flex-wrap items-center gap-x-1.5 gap-y-1",
										children: [
											/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.link.routing.preference.dialog.shortcut.note.prefix", "When links open in Orca,") }),
											/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ShortcutKeyCombo, {
												keys: systemBrowserShortcutKeys,
												keyCapClassName: "min-w-0 px-1 py-0 text-[10px] shadow-none",
												separatorClassName: "text-[10px] text-muted-foreground"
											}),
											/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.link.routing.preference.dialog.shortcut.note.suffix", "click opens system browser once.") })
										]
									})
								]
							})]
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogFooter, {
						className: "border-t border-border bg-muted/20 px-6 py-4 sm:justify-between",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
							variant: "outline",
							onClick: () => settleActiveRequest(false),
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExternalLink, { className: "size-4" }), translate("auto.components.link.routing.preference.dialog.system.button", "Use system browser")]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							autoFocus: true,
							onClick: () => settleActiveRequest(true),
							children: openLinksInAppDefault ? translate("auto.components.link.routing.preference.dialog.keep.orca.button", "Keep Orca") : translate("auto.components.link.routing.preference.dialog.orca.button", "Open in Orca")
						})]
					})
				]
			})
		})]
	});
}
function useLinkRoutingPreferenceDialog() {
	const requestPreference = (0, import_react.useContext)(LinkRoutingPreferenceDialogContext);
	if (!requestPreference) throw new Error("useLinkRoutingPreferenceDialog must be used inside LinkRoutingPreferenceDialogProvider");
	return requestPreference;
}
const ORCA_TERMINAL_COMMAND_FINISHED_EVENT = "orca:terminal-command-finished";
function dispatchTerminalCommandFinishedEvent(worktreeId, exitCode) {
	if (typeof window.dispatchEvent !== "function") return;
	window.dispatchEvent(new CustomEvent(ORCA_TERMINAL_COMMAND_FINISHED_EVENT, { detail: {
		worktreeId,
		exitCode
	} }));
}
var TEXT_INPUT_TYPES = new Set([
	"",
	"email",
	"password",
	"search",
	"tel",
	"text",
	"url"
]);
function isTextInputElement(element) {
	return element instanceof HTMLInputElement && TEXT_INPUT_TYPES.has(element.type);
}
function isPrimarySelectionTextControl(element) {
	return isTextInputElement(element) || element instanceof HTMLTextAreaElement;
}
function readTextControlSelection(element) {
	if (element instanceof HTMLInputElement && element.type === "password") return null;
	try {
		const start = element.selectionStart;
		const end = element.selectionEnd;
		if (start === null || end === null || start === end) return null;
		if (Math.abs(end - start) > 65536) return null;
		return element.value.slice(Math.min(start, end), Math.max(start, end));
	} catch {
		return null;
	}
}
function getRangeTextLengthUpTo(range, maxLength) {
	let length = 0;
	const root = range.commonAncestorContainer;
	const ownerDocument = root.ownerDocument ?? document;
	const addTextNode = (node$1) => {
		if (!range.intersectsNode(node$1)) return false;
		let start = 0;
		let end = node$1.data.length;
		if (node$1 === range.startContainer) start = range.startOffset;
		if (node$1 === range.endContainer) end = range.endOffset;
		length += Math.max(0, end - start);
		return length > maxLength;
	};
	if (root.nodeType === Node.TEXT_NODE) {
		addTextNode(root);
		return length;
	}
	const walker = ownerDocument.createTreeWalker(root, NodeFilter.SHOW_TEXT);
	let node = walker.nextNode();
	while (node) {
		if (addTextNode(node)) return length;
		node = walker.nextNode();
	}
	return length;
}
function selectionTextLengthExceeds(selection, maxLength) {
	let length = 0;
	for (let index = 0; index < selection.rangeCount; index += 1) {
		length += getRangeTextLengthUpTo(selection.getRangeAt(index), maxLength - length);
		if (length > maxLength) return true;
	}
	return false;
}
function readDocumentSelection() {
	const selection = window.getSelection();
	if (!selection || selection.isCollapsed) return null;
	if (selectionTextLengthExceeds(selection, 65536)) return null;
	const text = selection.toString();
	return text.length > 0 ? text : null;
}
function readCurrentPrimarySelectionText() {
	const activeElement = document.activeElement;
	if (activeElement instanceof Element) {
		const textControl = activeElement.closest("input, textarea");
		if (textControl && isPrimarySelectionTextControl(textControl)) {
			const text = readTextControlSelection(textControl);
			if (text) return text;
		}
	}
	return readDocumentSelection();
}
function findOwnedTextControlPasteTarget(activeElement = typeof document === "undefined" ? null : document.activeElement) {
	if (!(activeElement instanceof Element)) return null;
	const textControl = activeElement.closest("input, textarea");
	if (!textControl || !isPrimarySelectionTextControl(textControl)) return null;
	if (textControl.disabled || textControl.readOnly) return null;
	return textControl;
}
function findOwnedPasteEventTextControlTarget(eventTarget, activeElement = typeof document === "undefined" ? null : document.activeElement) {
	if (!(eventTarget instanceof Element)) return null;
	if (eventTarget.closest(".xterm-helper-textarea")) return null;
	const textControl = eventTarget.closest("input, textarea");
	if (!textControl || activeElement !== textControl) return null;
	return findOwnedTextControlPasteTarget(textControl);
}
function classifyTextControlPastePayloadOwnership(text, options = {}) {
	if (!text) return {
		action: "allow-native",
		reason: "empty",
		byteLength: 0,
		exceededLimit: false
	};
	const maxBytes = options.maxBytes ?? 16777216;
	const directMaxBytes = options.directMaxBytes ?? 65536;
	const ownershipMeasurement = measureTextControlPasteByteLength(text, { stopAfterBytes: Math.min(directMaxBytes, maxBytes) });
	if (!ownershipMeasurement.exceededLimit) return {
		action: "allow-native",
		reason: "small",
		byteLength: ownershipMeasurement.byteLength,
		exceededLimit: false
	};
	if (text.length > maxBytes || directMaxBytes >= maxBytes) return {
		action: "reject",
		reason: "too-large",
		byteLength: measureRejectedTextControlPasteByteLength(text, maxBytes),
		exceededLimit: true
	};
	return {
		action: "claim-orca",
		byteLength: ownershipMeasurement.byteLength,
		exceededLimit: true
	};
}
function measureRejectedTextControlPasteByteLength(text, maxBytes) {
	if (maxBytes <= 65536) return measureTextControlPasteByteLength(text, { stopAfterBytes: maxBytes }).byteLength;
	return maxBytes + 1;
}
const APP_MENU_PASTE_EVENT = "orca-app-menu-paste";
function dispatchAppMenuPasteEvent(target = window) {
	const event = new CustomEvent(APP_MENU_PASTE_EVENT, {
		bubbles: false,
		cancelable: true
	});
	target.dispatchEvent(event);
	return event.defaultPrevented;
}
function findFocusedAppMenuTextControlPasteTarget(activeElement = typeof document === "undefined" ? null : document.activeElement) {
	return findOwnedTextControlPasteTarget(activeElement);
}
function createAppMenuTextControlRejectedResult({ reason, redactedDiagnostic }) {
	return {
		status: "rejected",
		target: "text-control",
		reason,
		redactedDiagnostic
	};
}
function getNowMs() {
	return globalThis.performance?.now?.() ?? Date.now();
}
async function handleAppMenuPasteRequest({ readClipboardText, performNativePaste, dispatchOwnedPasteEvent = dispatchAppMenuPasteEvent, getActiveElement = () => document.activeElement, nativePasteMode = "paste" }) {
	const startedAtMs = getNowMs();
	if (dispatchOwnedPasteEvent()) return {
		status: "handled",
		target: "terminal"
	};
	const target = findFocusedAppMenuTextControlPasteTarget(getActiveElement());
	if (!target) {
		performNativePaste({ mode: nativePasteMode });
		return {
			status: "native-fallback",
			reason: "no-owned-target"
		};
	}
	let text;
	try {
		text = await readClipboardText({ maxBytes: TEXT_CONTROL_PASTE_MAX_BYTES });
	} catch (error) {
		if (isClipboardTextTooLargeError(error)) return createAppMenuTextControlRejectedResult({
			reason: "too-large",
			redactedDiagnostic: createTextControlRejectedResult("too-large", TEXT_CONTROL_PASTE_MAX_BYTES + 1, "app-menu", getNowMs() - startedAtMs).redactedDiagnostic
		});
		if (target.ownerDocument.activeElement !== target) return createAppMenuTextControlRejectedResult({
			reason: "target-unavailable",
			redactedDiagnostic: createTextControlRejectedResult("target-unavailable", 0, "app-menu", getNowMs() - startedAtMs).redactedDiagnostic
		});
		performNativePaste({ mode: nativePasteMode });
		return {
			status: "native-fallback",
			reason: "clipboard-read-failed"
		};
	}
	const result = await pasteTextIntoTextControl(target, text, {
		source: "app-menu",
		canContinue: (candidate) => candidate.ownerDocument.activeElement === candidate
	});
	if (result.status === "pasted") return {
		status: "handled",
		target: "text-control"
	};
	return createAppMenuTextControlRejectedResult({
		reason: result.reason,
		redactedDiagnostic: result.redactedDiagnostic
	});
}
const APP_MENU_SELECTION_ACTION_EVENT = "orca-app-menu-selection-action";
function dispatchAppMenuSelectionAction(action, target = window) {
	const event = new CustomEvent(APP_MENU_SELECTION_ACTION_EVENT, {
		detail: action,
		cancelable: true
	});
	target.dispatchEvent(event);
	return event.defaultPrevented;
}
function normalizeSegments(pathValue) {
	const segments = pathValue.split(/[\\/]+/);
	const stack = [];
	for (const segment of segments) {
		if (!segment || segment === ".") continue;
		if (segment === "..") {
			if (stack.length > 0) stack.pop();
			continue;
		}
		stack.push(segment);
	}
	return stack;
}
function normalizeAbsolutePath(pathValue) {
	const windowsDriveMatch = /^([A-Za-z]):[\\/]*(.*)$/.exec(pathValue);
	if (windowsDriveMatch) {
		const driveLetter = windowsDriveMatch[1].toUpperCase();
		const suffix = normalizeSegments(windowsDriveMatch[2]).join("/");
		const normalized = suffix ? `${driveLetter}:/${suffix}` : `${driveLetter}:/`;
		return {
			normalized,
			comparisonKey: normalized.toLowerCase(),
			rootKind: "windows"
		};
	}
	const uncMatch = /^(?:\\\\|\/\/)([^\\/]+)[\\/]+([^\\/]+)(?:[\\/]*(.*))?$/.exec(pathValue);
	if (uncMatch) {
		const server = uncMatch[1];
		const share = uncMatch[2];
		const suffix = normalizeSegments(uncMatch[3] ?? "").join("/");
		const normalizedRoot = `//${server}/${share}`;
		const normalized = suffix ? `${normalizedRoot}/${suffix}` : normalizedRoot;
		return {
			normalized,
			comparisonKey: normalized.toLowerCase(),
			rootKind: "unc"
		};
	}
	if (pathValue.startsWith("/")) {
		const normalized = `/${normalizeSegments(pathValue).join("/")}`.replace(/\/+$/, "") || "/";
		return {
			normalized,
			comparisonKey: normalized,
			rootKind: "posix"
		};
	}
	return null;
}
function inferHomePathFromCwd(cwd) {
	const normalizedCwd = normalizeAbsolutePath(cwd);
	if (!normalizedCwd) return null;
	const segments = normalizeSegments(normalizedCwd.normalized);
	if (normalizedCwd.rootKind === "windows") {
		const [drive, usersSegment, userSegment] = segments;
		if (!drive || !usersSegment || !userSegment || usersSegment.toLowerCase() !== "users") return null;
		return `${drive}/${usersSegment}/${userSegment}`;
	}
	if (normalizedCwd.rootKind === "posix") {
		const [rootParent, userSegment] = segments;
		if ((rootParent === "Users" || rootParent === "home") && userSegment) return `/${rootParent}/${userSegment}`;
		if (rootParent === "root") return "/root";
	}
	return null;
}
function normalizeExplicitHomePath(homePath) {
	const trimmedHomePath = homePath?.trim();
	if (!trimmedHomePath) return null;
	return normalizeAbsolutePath(trimmedHomePath)?.normalized ?? null;
}
function resolveTildePath(pathValue, cwd, homePath) {
	if (!/^~[\\/]/.test(pathValue)) return null;
	const resolvedHomePath = normalizeExplicitHomePath(homePath) ?? inferHomePathFromCwd(cwd);
	if (!resolvedHomePath) return null;
	return joinAbsolutePath(resolvedHomePath, pathValue.slice(2));
}
function joinAbsolutePath(basePath, relativePath) {
	const normalizedBase = normalizeAbsolutePath(basePath);
	if (!normalizedBase) return null;
	return normalizeJoinedPath(normalizedBase, relativePath);
}
function normalizeJoinedPath(basePath, relativePath) {
	const normalizedBaseSegments = normalizeSegments(basePath.normalized);
	const relativeSegments = normalizeSegments(relativePath);
	const joinedSegments = [...normalizedBaseSegments, ...relativeSegments];
	if (basePath.rootKind === "unc") {
		const [server, share, ...rest] = joinedSegments;
		return rest.length > 0 ? `//${server}/${share}/${rest.join("/")}` : `//${server}/${share}`;
	}
	if (basePath.rootKind === "windows") {
		const [drive, ...rest] = joinedSegments;
		return rest.length > 0 ? `${drive}/${rest.join("/")}` : drive;
	}
	return `/${joinedSegments.join("/")}`.replace(/\/+$/, "") || "/";
}
function parseFileLinkLocation(value) {
	const match = /^(.*?)(?::(\d+))?(?::(\d+))?$/.exec(value);
	const pathText = match?.[1];
	if (!pathText) return null;
	const line = match[2] ? Number.parseInt(match[2], 10) : null;
	const column = match[3] ? Number.parseInt(match[3], 10) : null;
	if (line !== null && line < 1 || column !== null && column < 1) return null;
	return {
		pathText,
		line,
		column
	};
}
function canKeepTrailingSeparator(pathText) {
	if (/^[\\/]+$/.test(pathText) || /^~[\\/]$/.test(pathText) || /^[A-Za-z]:[\\/]$/.test(pathText)) return false;
	return /^(?:~[\\/]|[\\/]|[A-Za-z]:[\\/])/.test(pathText);
}
function parseExplicitFileLinkTarget(value, options = {}) {
	const parsed = parseFileLinkLocation(value);
	if (!parsed) return null;
	const { pathText, line, column } = parsed;
	const hasLineOrColumn = line !== null || column !== null;
	if (/^[\\/]\s/.test(pathText)) return null;
	if (/[\\/]$/.test(pathText)) {
		const canKeepRelativeDirectory = options.allowRelativeDirectoryPath === true && !hasLineOrColumn;
		if (hasLineOrColumn || !canKeepRelativeDirectory && !canKeepTrailingSeparator(pathText)) return null;
	}
	return {
		pathText,
		line,
		column
	};
}
function resolveExplicitFileLinkTargetPath(pathText, cwd, homePath) {
	if (/^~[\\/]/.test(pathText)) return resolveTildePath(pathText, cwd, homePath);
	return normalizeAbsolutePath(pathText)?.normalized ?? joinAbsolutePath(cwd, pathText);
}
function resolveExplicitFileLinkTarget(parsed, cwd, homePath) {
	const absolutePath = resolveExplicitFileLinkTargetPath(parsed.pathText, cwd, homePath);
	if (!absolutePath) return null;
	return {
		absolutePath,
		line: parsed.line,
		column: parsed.column
	};
}
var LEADING_TRIM_CHARS = new Set([
	"(",
	"[",
	"{",
	"\"",
	"'"
]);
var TRAILING_TRIM_CHARS = new Set([
	")",
	"]",
	"}",
	"\"",
	"'",
	",",
	";",
	"."
]);
function trimBoundaryPunctuation(value, startIndex) {
	let start = 0;
	let end = value.length;
	while (start < end && LEADING_TRIM_CHARS.has(value[start])) start += 1;
	while (end > start && TRAILING_TRIM_CHARS.has(value[end - 1])) end -= 1;
	if (start >= end) return null;
	return {
		text: value.slice(start, end),
		startIndex: startIndex + start,
		endIndex: startIndex + end
	};
}
function* detectTerminalFileLinkRanges(lineText, regex) {
	for (const match of lineText.matchAll(regex)) {
		const rawStart = match.index ?? 0;
		const trimmed = trimBoundaryPunctuation(match[0], rawStart);
		if (trimmed) yield trimmed;
	}
}
function mergeTerminalFileLinkRanges(ranges) {
	if (ranges.length <= 1) return ranges;
	const sorted = ranges.slice().sort((left, right) => left[0] - right[0] || left[1] - right[1]);
	const merged = [];
	for (const range of sorted) {
		const last = merged.at(-1);
		if (!last || range[0] > last[1]) {
			merged.push([range[0], range[1]]);
			continue;
		}
		last[1] = Math.max(last[1], range[1]);
	}
	return merged;
}
function terminalFileLinkRangesOverlap(range, claimedRanges) {
	let low = 0;
	let high = claimedRanges.length;
	while (low < high) {
		const mid = Math.floor((low + high) / 2);
		if (claimedRanges[mid][0] < range.endIndex) low = mid + 1;
		else high = mid;
	}
	const previous = claimedRanges[low - 1];
	return previous !== void 0 && previous[1] > range.startIndex;
}
function insertTerminalFileLinkClaimedRange(claimedRanges, range) {
	const last = claimedRanges.at(-1);
	if (!last || last[0] <= range[0]) {
		claimedRanges.push(range);
		return;
	}
	let low = 0;
	let high = claimedRanges.length;
	while (low < high) {
		const mid = Math.floor((low + high) / 2);
		if (claimedRanges[mid][0] <= range[0]) low = mid + 1;
		else high = mid;
	}
	claimedRanges.splice(low, 0, range);
}
function toParsedTerminalFileLink(range) {
	const parsed = parseExplicitFileLinkTarget(range.text);
	if (!parsed) return null;
	return {
		pathText: parsed.pathText,
		line: parsed.line,
		column: parsed.column,
		startIndex: range.startIndex,
		endIndex: range.endIndex,
		displayText: range.text
	};
}
var WORD_TOKEN_REGEX = /[^\s()[\]{}'",;<>|`]+/g;
var EXTENSIONLESS_FILENAMES = new Set([
	"Makefile",
	"Dockerfile",
	"Rakefile",
	"Gemfile",
	"Procfile",
	"LICENSE",
	"README",
	"CHANGELOG",
	"AUTHORS",
	"NOTICE",
	"CONTRIBUTING"
]);
var BARE_FILENAME_PATTERN = /^[A-Za-z0-9_][A-Za-z0-9._+-]*$/;
var MAX_BARE_FILENAME_TOKEN_LENGTH = 120;
function looksLikeFilename(token) {
	if (token.length < 2 || token.length > 100) return false;
	if (!BARE_FILENAME_PATTERN.test(token)) return false;
	if (/^\d+$/.test(token)) return false;
	if (token.includes(".")) return !/^\.+$/.test(token);
	return EXTENSIONLESS_FILENAMES.has(token);
}
function detectBareFilenameLinks(lineText, claimedRanges) {
	const links = [];
	for (const range of detectTerminalFileLinkRanges(lineText, WORD_TOKEN_REGEX)) {
		if (terminalFileLinkRangesOverlap(range, claimedRanges)) continue;
		if (range.text.length > MAX_BARE_FILENAME_TOKEN_LENGTH) continue;
		const link = toParsedTerminalFileLink(range);
		if (!link || !looksLikeFilename(link.pathText)) continue;
		links.push(link);
	}
	return links;
}
function parseFileUrlLineHash(hash) {
	const trimmed = hash.startsWith("#") ? hash.slice(1) : hash;
	const match = /^L(\d+)(?:C(\d+))?$/i.exec(trimmed);
	if (!match) return null;
	return {
		line: Number(match[1]),
		column: match[2] ? Number(match[2]) : null
	};
}
function parseFilePathTrailingLineTarget(filePath) {
	const match = /^(.*?)(?::(\d+))(?::(\d+))?$/.exec(filePath);
	if (!match || !match[1] || match[1].endsWith("/") || match[1].endsWith("\\")) return null;
	return {
		filePath: match[1],
		line: Number(match[2]),
		column: match[3] ? Number(match[3]) : null
	};
}
function resolveTerminalFileUrlTarget(parsed, options = {}) {
	if (parsed.hostname && parsed.hostname !== "localhost" && !options.allowUncHost) return null;
	const filePath = fileUriToFilesystemPath(parsed);
	if (!filePath) return null;
	const hashTarget = parseFileUrlLineHash(parsed.hash);
	if (hashTarget) return {
		filePath,
		line: hashTarget.line,
		column: hashTarget.column
	};
	return parseFilePathTrailingLineTarget(filePath) ?? {
		filePath,
		line: null,
		column: null
	};
}
var MAX_FILE_URI_LENGTH = 2048;
var FILE_URI_REGEX = /\bfile:\/\/[^\s"`<>|]{1,2049}/gi;
var TRAILING_PROSE_CHARS = new Set([
	".",
	",",
	";",
	":",
	"!",
	"?",
	">",
	"\"",
	"'",
	"`"
]);
function trimTrailingProse(uriText) {
	let parentheses = 0;
	let brackets = 0;
	let braces = 0;
	for (const char of uriText) {
		parentheses += char === ")" ? 1 : char === "(" ? -1 : 0;
		brackets += char === "]" ? 1 : char === "[" ? -1 : 0;
		braces += char === "}" ? 1 : char === "{" ? -1 : 0;
	}
	let end = uriText.length;
	while (end > 0) {
		const char = uriText[end - 1];
		if (TRAILING_PROSE_CHARS.has(char)) {
			end -= 1;
			continue;
		}
		if (char === ")" && parentheses > 0) {
			parentheses -= 1;
			end -= 1;
			continue;
		}
		if (char === "]" && brackets > 0) {
			brackets -= 1;
			end -= 1;
			continue;
		}
		if (char === "}" && braces > 0) {
			braces -= 1;
			end -= 1;
			continue;
		}
		break;
	}
	return uriText.slice(0, end);
}
function toFileUriLink(uriText, startIndex) {
	let url;
	try {
		url = new URL(uriText);
	} catch {
		return null;
	}
	const target = resolveTerminalFileUrlTarget(url);
	if (!target) return null;
	return {
		pathText: target.filePath,
		line: target.line,
		column: target.column,
		startIndex,
		endIndex: startIndex + uriText.length,
		displayText: uriText
	};
}
function detectTerminalFileUriLinks(lineText) {
	const links = [];
	for (const match of lineText.matchAll(FILE_URI_REGEX)) {
		const startIndex = match.index ?? 0;
		if (match[0].length > MAX_FILE_URI_LENGTH) continue;
		const trimmed = trimTrailingProse(match[0]);
		if (!trimmed) continue;
		const link = toFileUriLink(trimmed, startIndex);
		if (link) links.push(link);
	}
	return links;
}
var LOCAL_PATH_REGEX = /(?:~[\\/]|[\\/]|\.{1,2}[\\/]|[A-Za-z]:[\\/]|[A-Za-z0-9._-]+[\\/])[A-Za-z0-9._~\-/%+@\\()[\]]*(?::\d+)?(?::\d+)?/g;
var SPACED_PATH_WITH_SEPARATOR_REGEX = /(?:~[\\/]|[\\/]|\.{1,2}[\\/]|[A-Za-z]:[\\/]|[A-Za-z0-9._-]+[\\/])[^()[\]{}'",;<>|`\r\n]+(?::\d+)?(?::\d+)?/g;
var SPACED_PATH_WITH_EXTENSION_REGEX = /(?:~[\\/]|[\\/]|\.{1,2}[\\/]|[A-Za-z]:[\\/]|[A-Za-z0-9._-]+[\\/])[^()[\]{}'",;<>|`\r\n]+(?::\d+)?(?::\d+)?/g;
var LINE_ENDING_SPACED_PATH_REGEX = /(?:~[\\/]|[\\/]|\.{1,2}[\\/]|[A-Za-z]:[\\/]|[A-Za-z0-9._-]+[\\/])[^()[\]{}'",;<>|`\r\n]+(?::\d+)?(?::\d+)?/g;
var SPACED_LOCAL_PATH_REGEXES = [
	SPACED_PATH_WITH_SEPARATOR_REGEX,
	SPACED_PATH_WITH_EXTENSION_REGEX,
	LINE_ENDING_SPACED_PATH_REGEX
];
var URI_PREFIX_CHAR_PATTERN = /^[A-Za-z0-9+./:-]$/;
function hasPathSeparator(text) {
	return text.includes("/") || text.includes("\\");
}
function hasSeparatorAfterWhitespace(text) {
	let sawWhitespace = false;
	for (const char of text) {
		if (/\s/.test(char)) {
			sawWhitespace = true;
			continue;
		}
		if (sawWhitespace && (char === "/" || char === "\\")) return true;
	}
	return false;
}
function hasInternalWhitespaceBeforeTrimmedEnd(text) {
	const trimmed = text.trimEnd();
	return /\s/.test(trimmed);
}
function isAtTrimmedLineEnd(lineText, endIndex) {
	return lineText.slice(endIndex).trim().length === 0;
}
function hasSpacedPathExtension(text) {
	const trimmedText = trimSpacedPathTrailingProse({
		text,
		startIndex: 0,
		endIndex: text.length
	}).text.trimEnd();
	return /\s/.test(trimmedText) && /\.[A-Za-z0-9_+-]+(?::\d+)?(?::\d+)?$/.test(trimmedText);
}
function getImmediateUriPrefix(lineText, endIndex) {
	let start = endIndex;
	while (start > 0 && URI_PREFIX_CHAR_PATTERN.test(lineText[start - 1])) start -= 1;
	return lineText.slice(start, endIndex);
}
function isInsideUriScheme(lineText, range) {
	const prefix = getImmediateUriPrefix(lineText, range.startIndex);
	return range.text.includes("://") || /[A-Za-z][A-Za-z0-9+.-]*:(?:\/\/)?$/.test(prefix) && (prefix.endsWith("://") || range.text.startsWith("//"));
}
function trimSpacedPathTrailingProse(range) {
	let selected = null;
	const extensionPrefixPattern = /\.[A-Za-z0-9_+-]+(?::\d+)?(?::\d+)?(?=\s+|$)/g;
	let match;
	while ((match = extensionPrefixPattern.exec(range.text)) !== null) {
		const end = match.index + match[0].length;
		const text = range.text.slice(0, end);
		if (countPathStarts(text) > 1) continue;
		if (end < range.text.length || selected === null || /[\\/]/.test(range.text.slice(selected.length, end))) selected = text;
	}
	if (!selected) return range;
	return {
		text: selected,
		startIndex: range.startIndex,
		endIndex: range.startIndex + selected.length
	};
}
function countPathStarts(text) {
	let count = 0;
	for (const match of text.matchAll(/(?:^|\s)(?:~[\\/]|[\\/]|\.{1,2}[\\/]|[A-Za-z]:[\\/])/g)) count += 1;
	return count;
}
function trimTrailingWhitespace(range) {
	const text = range.text.trimEnd();
	return {
		text,
		startIndex: range.startIndex,
		endIndex: range.startIndex + text.length
	};
}
function buildLineEndingSpacedPathPrefixRanges(range) {
	const ranges = [];
	for (const match of range.text.matchAll(/\s+/g)) {
		const endIndex = match.index ?? 0;
		const text = range.text.slice(0, endIndex).trimEnd();
		if (text.includes(" ")) ranges.push({
			text,
			startIndex: range.startIndex,
			endIndex: range.startIndex + text.length
		});
	}
	return ranges.toReversed();
}
function detectLocalPathLinks(lineText, includeLineEndingPrefixCandidates = false) {
	if (!hasPathSeparator(lineText)) return [];
	const links = [];
	const spacedLinks = detectSpacedLocalPathLinks(lineText, includeLineEndingPrefixCandidates);
	const spacedRanges = mergeTerminalFileLinkRanges(spacedLinks.map(({ startIndex, endIndex }) => [startIndex, endIndex]));
	for (const link of spacedLinks) links.push(link);
	for (const range of detectTerminalFileLinkRanges(lineText, LOCAL_PATH_REGEX)) {
		if (terminalFileLinkRangesOverlap(range, spacedRanges)) continue;
		if (isInsideUriScheme(lineText, range)) continue;
		if (!/[\\/]/.test(range.text)) continue;
		const link = toParsedTerminalFileLink(range);
		if (link) links.push(link);
	}
	return links.sort((a, b) => a.startIndex - b.startIndex || b.endIndex - a.endIndex);
}
function detectSpacedLocalPathLinks(lineText, includeLineEndingPrefixCandidates = false) {
	const links = [];
	const claimedRanges = [];
	for (const regex of SPACED_LOCAL_PATH_REGEXES) for (const range of detectTerminalFileLinkRanges(lineText, regex)) {
		if (regex === SPACED_PATH_WITH_SEPARATOR_REGEX && !hasSeparatorAfterWhitespace(range.text)) continue;
		if (regex === SPACED_PATH_WITH_EXTENSION_REGEX && !hasSpacedPathExtension(range.text)) continue;
		if (regex === LINE_ENDING_SPACED_PATH_REGEX && (!hasInternalWhitespaceBeforeTrimmedEnd(range.text) || !isAtTrimmedLineEnd(lineText, range.endIndex))) continue;
		if (terminalFileLinkRangesOverlap(range, claimedRanges) || isInsideUriScheme(lineText, range)) continue;
		const candidateLinks = (includeLineEndingPrefixCandidates && regex === LINE_ENDING_SPACED_PATH_REGEX ? [range, ...buildLineEndingSpacedPathPrefixRanges(range)] : [range]).map((candidateRange) => toParsedTerminalFileLink(trimSpacedPathTrailingProse(trimTrailingWhitespace(candidateRange)))).filter((link$1) => link$1 !== null);
		const link = candidateLinks[0];
		if (link) {
			for (const candidateLink of candidateLinks) links.push(candidateLink);
			insertTerminalFileLinkClaimedRange(claimedRanges, [link.startIndex, link.endIndex]);
		}
	}
	return links;
}
function assembleFileLinks(lineText, includeLineEndingPrefixCandidates) {
	const uriLinks = detectTerminalFileUriLinks(lineText);
	const pathLinks = detectLocalPathLinks(lineText, includeLineEndingPrefixCandidates);
	const explicitLinks = uriLinks.length > 0 ? [...uriLinks, ...pathLinks] : pathLinks;
	const wordLinks = detectBareFilenameLinks(lineText, mergeTerminalFileLinkRanges(explicitLinks.map(({ startIndex, endIndex }) => [startIndex, endIndex])));
	for (const link of wordLinks) explicitLinks.push(link);
	return explicitLinks;
}
function extractTerminalFileLinks(lineText) {
	return assembleFileLinks(lineText, false);
}
function extractTerminalFileLinkCandidates(lineText) {
	return assembleFileLinks(lineText, true);
}
function resolveTerminalFileLink(parsed, cwd, homePath) {
	return resolveExplicitFileLinkTarget(parsed, cwd, homePath);
}
function resolveTerminalFileLinkText(linkText, cwd, homePath) {
	const exactLink = extractTerminalFileLinks(linkText).find((link) => link.startIndex === 0 && link.endIndex === linkText.length);
	return exactLink ? resolveTerminalFileLink(exactLink, cwd, homePath) : null;
}
function isPathInsideWorktree(filePath, worktreePath) {
	const normalizedFile = normalizeAbsolutePath(filePath);
	const normalizedWorktree = normalizeAbsolutePath(worktreePath);
	if (!normalizedFile || !normalizedWorktree || normalizedFile.rootKind !== normalizedWorktree.rootKind) return false;
	if (normalizedFile.comparisonKey === normalizedWorktree.comparisonKey) return true;
	return normalizedFile.comparisonKey.startsWith(`${normalizedWorktree.comparisonKey}/`);
}
function toWorktreeRelativePath(filePath, worktreePath) {
	const normalizedFile = normalizeAbsolutePath(filePath);
	const normalizedWorktree = normalizeAbsolutePath(worktreePath);
	if (!normalizedFile || !normalizedWorktree || normalizedFile.rootKind !== normalizedWorktree.rootKind) return null;
	if (normalizedFile.comparisonKey === normalizedWorktree.comparisonKey) return "";
	if (!normalizedFile.comparisonKey.startsWith(`${normalizedWorktree.comparisonKey}/`)) return null;
	return normalizedFile.normalized.slice(normalizedWorktree.normalized.length + 1);
}
var SWEEP_ATTEMPT_DELAYS_MS = [
	300,
	1500,
	4e3,
	1e4,
	2e4
];
var dueAtByPtyId = /* @__PURE__ */ new Map();
var attemptsByPtyId = /* @__PURE__ */ new Map();
var notifiedPtyIds = /* @__PURE__ */ new Set();
var flushTimer = null;
var flushTimerDueAt = null;
function notifyCodexPaneBoundForStaleSweep(ptyId) {
	if (notifiedPtyIds.has(ptyId)) return;
	if (isForeignMachineCodexPtyId(ptyId)) return;
	queue(ptyId, SWEEP_ATTEMPT_DELAYS_MS[0]);
	armForEarliestDue();
}
function sweepRestoredCodexPanesForStaleAccounts(state) {
	for (const ptyIds of Object.values(state.ptyIdsByTabId)) for (const ptyId of ptyIds) notifyCodexPaneBoundForStaleSweep(ptyId);
}
function queue(ptyId, delayMs) {
	const dueAt = Date.now() + delayMs;
	const existing = dueAtByPtyId.get(ptyId);
	dueAtByPtyId.set(ptyId, existing === void 0 ? dueAt : Math.min(existing, dueAt));
}
function armForEarliestDue() {
	let earliestDueAt = null;
	for (const dueAt of dueAtByPtyId.values()) if (earliestDueAt === null || dueAt < earliestDueAt) earliestDueAt = dueAt;
	if (earliestDueAt === null) return;
	if (flushTimer !== null) {
		if (flushTimerDueAt !== null && flushTimerDueAt <= earliestDueAt) return;
		clearTimeout(flushTimer);
	}
	flushTimerDueAt = earliestDueAt;
	flushTimer = setTimeout(() => {
		flushTimer = null;
		flushTimerDueAt = null;
		flush();
	}, Math.max(0, earliestDueAt - Date.now()));
}
function takeDuePtyIds() {
	const now = Date.now();
	const duePtyIds = [];
	for (const [ptyId, dueAt] of dueAtByPtyId) if (dueAt <= now || !attemptsByPtyId.has(ptyId)) duePtyIds.push(ptyId);
	for (const ptyId of duePtyIds) dueAtByPtyId.delete(ptyId);
	return duePtyIds;
}
function shouldRetry(scan) {
	return !scan.eligible && (scan.inconclusive || scan.launchedCodex);
}
function queueNextRung(ptyId) {
	const attempt = (attemptsByPtyId.get(ptyId) ?? 0) + 1;
	const delayMs = SWEEP_ATTEMPT_DELAYS_MS[attempt];
	if (delayMs === void 0) {
		attemptsByPtyId.delete(ptyId);
		return;
	}
	attemptsByPtyId.set(ptyId, attempt);
	queue(ptyId, delayMs);
}
async function flush() {
	const ptyIds = takeDuePtyIds();
	if (ptyIds.length === 0) {
		armForEarliestDue();
		return;
	}
	let scans;
	try {
		scans = await markRestoredStaleCodexSessionsForRestart({ ptyIds });
	} catch (err) {
		console.warn("Codex stale-pane restart sweep failed:", err);
		for (const ptyId of ptyIds) queueNextRung(ptyId);
		armForEarliestDue();
		return;
	}
	const scanByPtyId = new Map(scans.map((scan) => [scan.ptyId, scan]));
	for (const ptyId of ptyIds) {
		const scan = scanByPtyId.get(ptyId);
		if (scan?.notified === true) {
			notifiedPtyIds.add(ptyId);
			attemptsByPtyId.delete(ptyId);
			continue;
		}
		if (scan !== void 0 && !shouldRetry(scan)) {
			attemptsByPtyId.delete(ptyId);
			continue;
		}
		queueNextRung(ptyId);
	}
	armForEarliestDue();
}
function CloseTerminalDialog({ open, copyKind = "command", tabLabel, subjectKey, onCancel, onConfirm }) {
	const checkboxId = (0, import_react.useId)();
	const [dontAskAgain, setDontAskAgain] = (0, import_react.useState)(false);
	const [previousOpen, setPreviousOpen] = (0, import_react.useState)(open);
	const [previousSubjectKey, setPreviousSubjectKey] = (0, import_react.useState)(subjectKey);
	if (open !== previousOpen) {
		setPreviousOpen(open);
		if (open) setDontAskAgain(false);
	}
	if (subjectKey !== previousSubjectKey) {
		setPreviousSubjectKey(subjectKey);
		if (subjectKey !== void 0) setDontAskAgain(false);
	}
	const isAgent = copyKind === "agent";
	const trimmedTabLabel = tabLabel?.trim();
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
		open,
		onOpenChange: (isOpen) => {
			if (!isOpen) onCancel();
		},
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogContent, {
			className: "max-w-sm",
			showCloseButton: false,
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, {
					className: "text-sm",
					children: isAgent ? translate("auto.components.terminal.pane.CloseTerminalDialog.stop_agent_title", "Stop this agent?") : translate("auto.components.terminal.pane.CloseTerminalDialog.stop_command_title", "Stop running command?")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, {
					className: "text-xs",
					children: isAgent ? translate("auto.components.terminal.pane.CloseTerminalDialog.stop_agent_description", "Closing this terminal will stop the agent's current work.") : translate("auto.components.terminal.pane.CloseTerminalDialog.stop_command_description", "Closing this terminal will stop the command running inside it.")
				})] }),
				trimmedTabLabel ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "truncate text-xs font-medium text-foreground",
					title: trimmedTabLabel,
					children: trimmedTabLabel
				}) : null,
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-center gap-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Checkbox, {
						id: checkboxId,
						checked: dontAskAgain,
						onCheckedChange: (checked) => setDontAskAgain(checked === true)
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
						htmlFor: checkboxId,
						className: "text-xs font-normal text-muted-foreground",
						children: translate("auto.components.terminal.pane.CloseTerminalDialog.dont_ask_again", "Don't ask again for running terminals")
					})]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogFooter, {
					className: "gap-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						type: "button",
						variant: "outline",
						size: "sm",
						onClick: onCancel,
						children: translate("auto.components.terminal.pane.CloseTerminalDialog.1d1a7a9c1f", "Cancel")
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						type: "button",
						variant: "destructive",
						size: "sm",
						autoFocus: true,
						onClick: () => onConfirm(dontAskAgain),
						children: isAgent ? translate("auto.components.terminal.pane.CloseTerminalDialog.stop_agent_confirm", "Stop Agent") : translate("auto.components.terminal.pane.CloseTerminalDialog.stop_command_confirm", "Stop and Close")
					})]
				})
			]
		})
	});
}
const OSC52_CLIPBOARD_SETTING_ID = "terminal-osc52-clipboard";
var authoritativeSnapshotByPtyId = /* @__PURE__ */ new Map();
var unknownCapabilityRetryAtByPtyId = /* @__PURE__ */ new Map();
var unknownCapabilityAttemptsByPtyId = /* @__PURE__ */ new Map();
var UNKNOWN_CAPABILITY_RETRY_MS = 1e3;
var UNKNOWN_CAPABILITY_MAX_RETRY_MS = 3e4;
var UNKNOWN_CAPABILITY_MAX_ATTEMPTS = 8;
var CAPABILITY_RESOLUTION_TIMEOUT_MS = 1e3;
var lastSynchronizedLivePtyIds = null;
var earliestUnknownCapabilityRetryAtMs = Number.POSITIVE_INFINITY;
var synchronizationGeneration = 0;
function collectTerminalProviderSnapshotPtyIds(state) {
	const ids = /* @__PURE__ */ new Set();
	for (const worktreeTabs of Object.values(state.tabsByWorktree)) for (const tab of worktreeTabs) {
		if (tab.ptyId) ids.add(tab.ptyId);
		for (const ptyId of state.ptyIdsByTabId[tab.id] ?? []) ids.add(ptyId);
	}
	for (const ptyId of Object.values(state.pendingReconnectPtyIdByTabId ?? {})) ids.add(ptyId);
	for (const layout of Object.values(state.terminalLayoutsByTabId ?? {})) for (const ptyId of Object.values(layout.ptyIdsByLeafId ?? {})) ids.add(ptyId);
	return [...ids];
}
function refreshEarliestUnknownCapabilityRetry() {
	earliestUnknownCapabilityRetryAtMs = Number.POSITIVE_INFINITY;
	for (const retryAtMs of unknownCapabilityRetryAtByPtyId.values()) earliestUnknownCapabilityRetryAtMs = Math.min(earliestUnknownCapabilityRetryAtMs, retryAtMs);
}
function backOffUnknownCapability(ptyId, nowMs) {
	const attempts = (unknownCapabilityAttemptsByPtyId.get(ptyId) ?? 0) + 1;
	if (attempts >= UNKNOWN_CAPABILITY_MAX_ATTEMPTS) {
		authoritativeSnapshotByPtyId.set(ptyId, false);
		unknownCapabilityAttemptsByPtyId.delete(ptyId);
		unknownCapabilityRetryAtByPtyId.delete(ptyId);
		return;
	}
	unknownCapabilityAttemptsByPtyId.set(ptyId, attempts);
	unknownCapabilityRetryAtByPtyId.set(ptyId, nowMs + Math.min(UNKNOWN_CAPABILITY_RETRY_MS * 2 ** (attempts - 1), UNKNOWN_CAPABILITY_MAX_RETRY_MS));
}
function unknownCapabilityRetryDelayMs(nowMs) {
	return earliestUnknownCapabilityRetryAtMs === Number.POSITIVE_INFINITY ? null : Math.max(0, earliestUnknownCapabilityRetryAtMs - nowMs);
}
async function resolveSnapshotCapabilityBatch(resolve, batch) {
	let timeout;
	try {
		return await Promise.race([resolve(batch), new Promise((resolveTimeout) => {
			timeout = setTimeout(() => resolveTimeout(null), CAPABILITY_RESOLUTION_TIMEOUT_MS);
		})]);
	} finally {
		clearTimeout(timeout);
	}
}
async function synchronizeTerminalProviderSnapshotCapabilities(livePtyIds, resolveCapabilities, observedAtMs) {
	if (livePtyIds === lastSynchronizedLivePtyIds && earliestUnknownCapabilityRetryAtMs === Number.POSITIVE_INFINITY) return null;
	const nowMs = observedAtMs ?? Date.now();
	if (livePtyIds === lastSynchronizedLivePtyIds && nowMs < earliestUnknownCapabilityRetryAtMs) return unknownCapabilityRetryDelayMs(nowMs);
	const generation = ++synchronizationGeneration;
	lastSynchronizedLivePtyIds = livePtyIds;
	const live = new Set(livePtyIds.filter((id) => id.length > 0));
	for (const cachedId of authoritativeSnapshotByPtyId.keys()) if (!live.has(cachedId)) authoritativeSnapshotByPtyId.delete(cachedId);
	for (const pendingId of unknownCapabilityRetryAtByPtyId.keys()) if (!live.has(pendingId)) {
		unknownCapabilityRetryAtByPtyId.delete(pendingId);
		unknownCapabilityAttemptsByPtyId.delete(pendingId);
	}
	const missing = [...live].filter((id) => !authoritativeSnapshotByPtyId.has(id) && (unknownCapabilityRetryAtByPtyId.get(id) ?? 0) <= nowMs);
	const resolve = resolveCapabilities ?? window.api.pty.getAuthoritativeBufferSnapshotCapabilities;
	if (!resolve) {
		for (const id of missing) backOffUnknownCapability(id, nowMs);
		refreshEarliestUnknownCapabilityRetry();
		return unknownCapabilityRetryDelayMs(nowMs);
	}
	for (let offset = 0; offset < missing.length; offset += 512) {
		const batch = missing.slice(offset, offset + 512);
		let resolved;
		try {
			resolved = await resolveSnapshotCapabilityBatch(resolve, batch);
		} catch {
			if (generation !== synchronizationGeneration) return null;
			for (const id of batch) backOffUnknownCapability(id, nowMs);
			continue;
		}
		if (generation !== synchronizationGeneration) return null;
		if (!resolved) {
			for (const id of missing.slice(offset)) backOffUnknownCapability(id, nowMs);
			break;
		}
		const resolvedById = new Map(resolved.map((entry) => [entry.id, entry.authoritative]));
		for (const id of batch) {
			const authoritative = resolvedById.get(id);
			if (typeof authoritative === "boolean") {
				authoritativeSnapshotByPtyId.set(id, authoritative);
				unknownCapabilityRetryAtByPtyId.delete(id);
				unknownCapabilityAttemptsByPtyId.delete(id);
			} else backOffUnknownCapability(id, nowMs);
		}
	}
	refreshEarliestUnknownCapabilityRetry();
	return unknownCapabilityRetryDelayMs(observedAtMs === void 0 ? Date.now() : nowMs);
}
async function refreshTerminalProviderSnapshotCapabilities(livePtyIds, resolveCapabilities) {
	lastSynchronizedLivePtyIds = null;
	for (const id of livePtyIds) {
		authoritativeSnapshotByPtyId.delete(id);
		unknownCapabilityRetryAtByPtyId.delete(id);
		unknownCapabilityAttemptsByPtyId.delete(id);
	}
	refreshEarliestUnknownCapabilityRetry();
	return synchronizeTerminalProviderSnapshotCapabilities(livePtyIds, resolveCapabilities);
}
function startTerminalProviderSnapshotCapabilitySynchronization(livePtyIds) {
	let disposed = false;
	let retryTimer;
	const synchronize = async () => {
		const retryDelayMs = await synchronizeTerminalProviderSnapshotCapabilities(livePtyIds);
		if (!disposed && retryDelayMs !== null) retryTimer = setTimeout(() => void synchronize(), Math.max(1, retryDelayMs));
	};
	synchronize();
	return () => {
		disposed = true;
		clearTimeout(retryTimer);
	};
}
function terminalProviderHasAuthoritativeSnapshot(ptyId) {
	return authoritativeSnapshotByPtyId.get(ptyId) === true;
}
init_defineProperty();
var RECOVERY_DELAYS_MS = [
	250,
	500,
	1e3,
	2e3,
	4e3,
	8e3,
	15e3,
	3e4
];
const REMOTE_RUNTIME_AUTO_RECOVERY_TIMEOUT_MS = 6e4;
var scheduledRecoveries = /* @__PURE__ */ new Set();
function retryAllRemoteRuntimePtyRecoveriesNow() {
	let advanced = 0;
	for (const recovery of Array.from(scheduledRecoveries)) if (recovery.retryNow()) advanced += 1;
	return advanced;
}
var RemoteRuntimePtyRecoveryState = class {
	constructor(onChange) {
		_defineProperty(this, "phase", "idle");
		_defineProperty(this, "epoch", 0);
		_defineProperty(this, "attempt", 0);
		_defineProperty(this, "retryTimer", null);
		_defineProperty(this, "deadlineTimer", null);
		_defineProperty(this, "pendingRetry", null);
		_defineProperty(this, "pendingEpoch", null);
		this.onChange = onChange;
	}
	get isActive() {
		return this.phase === "recovering" || this.phase === "backoff";
	}
	get currentPhase() {
		return this.phase;
	}
	get currentEpoch() {
		return this.epoch;
	}
	get attemptCount() {
		return this.attempt;
	}
	begin() {
		if (this.phase === "disposed") return this.epoch;
		if (!this.isActive) {
			this.epoch += 1;
			this.attempt = 0;
			this.armDeadline(this.epoch);
		}
		this.clearRetryTimer();
		this.phase = "recovering";
		this.onChange?.();
		return this.epoch;
	}
	isCurrent(epoch) {
		return this.isActive && epoch === this.epoch;
	}
	ownsEpoch(epoch) {
		return this.phase !== "disposed" && epoch === this.epoch;
	}
	schedule(epoch, retry) {
		if (!this.isCurrent(epoch)) return false;
		this.clearRetryTimer();
		this.phase = "backoff";
		const delayMs = RECOVERY_DELAYS_MS[Math.min(this.attempt, RECOVERY_DELAYS_MS.length - 1)];
		this.attempt += 1;
		this.pendingRetry = retry;
		this.pendingEpoch = epoch;
		const timer = setTimeout(() => {
			if (this.retryTimer !== timer || !this.isCurrent(epoch)) return;
			this.retryTimer = null;
			this.pendingRetry = null;
			this.pendingEpoch = null;
			scheduledRecoveries.delete(this);
			this.phase = "recovering";
			this.onChange?.();
			retry(epoch);
		}, delayMs);
		timer.unref?.();
		this.retryTimer = timer;
		scheduledRecoveries.add(this);
		this.onChange?.();
		return true;
	}
	parkRetryForExternalTrigger(epoch, retry) {
		if (!this.isCurrent(epoch) || this.pendingRetry !== null) return false;
		this.pendingRetry = retry;
		this.pendingEpoch = epoch;
		scheduledRecoveries.add(this);
		return true;
	}
	discardPendingRetry(retry) {
		if (this.pendingRetry !== retry) return;
		this.clearRetryTimer();
	}
	retryNow() {
		if (this.pendingRetry === null || this.pendingEpoch === null) return false;
		if (this.phase !== "backoff" && this.phase !== "disconnected") return false;
		const retry = this.pendingRetry;
		const latched = this.phase === "disconnected";
		this.clearRetryTimer();
		if (latched) {
			this.epoch += 1;
			this.attempt = 0;
			this.armDeadline(this.epoch);
		}
		const epoch = this.epoch;
		this.phase = "recovering";
		this.onChange?.();
		retry(epoch);
		return true;
	}
	markHealthy() {
		if (this.phase === "disposed") return;
		this.clearTimers();
		this.phase = "idle";
		this.attempt = 0;
		this.onChange?.();
	}
	markDisconnected() {
		if (this.phase === "disposed") return;
		this.clearTimers();
		this.phase = "disconnected";
		this.onChange?.();
	}
	cancel() {
		if (this.phase === "disposed") return;
		this.epoch += 1;
		this.clearTimers();
		this.phase = "idle";
		this.attempt = 0;
		this.onChange?.();
	}
	dispose() {
		this.epoch += 1;
		this.clearTimers();
		this.phase = "disposed";
		this.onChange?.();
	}
	armDeadline(epoch) {
		this.clearDeadlineTimer();
		const timer = setTimeout(() => {
			if (this.deadlineTimer !== timer || !this.isCurrent(epoch)) return;
			this.deadlineTimer = null;
			this.stopRetryTimer();
			this.phase = "disconnected";
			this.onChange?.();
		}, REMOTE_RUNTIME_AUTO_RECOVERY_TIMEOUT_MS);
		timer.unref?.();
		this.deadlineTimer = timer;
	}
	clearTimers() {
		this.clearRetryTimer();
		this.clearDeadlineTimer();
	}
	stopRetryTimer() {
		if (this.retryTimer) {
			clearTimeout(this.retryTimer);
			this.retryTimer = null;
		}
	}
	clearRetryTimer() {
		this.stopRetryTimer();
		this.pendingRetry = null;
		this.pendingEpoch = null;
		scheduledRecoveries.delete(this);
	}
	clearDeadlineTimer() {
		if (this.deadlineTimer) {
			clearTimeout(this.deadlineTimer);
			this.deadlineTimer = null;
		}
	}
};
export { isEmptyFloatingWorkspacePanelVisible as $, ORCA_TERMINAL_COMMAND_FINISHED_EVENT as A, dispatchZoomLevelChanged as At, dispatchAgentHookTerminalLifecycle as B, dispatchAppMenuSelectionAction as C, onDriverChange as Ct, findOwnedPasteEventTextControlTarget as D, getNextTabAcrossAllTypes as Dt, classifyTextControlPastePayloadOwnership as E, getActiveEntityIdForTabType as Et, resolveClientEnvironmentInfo as F, isAskUserQuestionTool as G, createCodexAutoApprovalHookCompletionSuppressor as H, appendClientEnvironmentFooter as I, dispatchTerminalNotification as J, isPotentialQuestionAnsweredSubmitInput as K, hasClientEnvironmentFooter as L, LinkRoutingPreferenceDialogProvider as M, recordTerminalTabParkedOnUnresolvedHost as Mt, useLinkRoutingPreferenceDialog as N, isPrimarySelectionTextControl as O, getNextTabWithinActiveType as Ot, resolveClientEnvironmentFooter as P, handleEmptyFloatingWorkspacePanelCloseShortcut as Q, stripClientEnvironmentFooter as R, APP_MENU_SELECTION_ACTION_EVENT as S, isPtyLocked as St, handleAppMenuPasteRequest as T, setDriverForPty as Tt, shouldSuppressCodexAutoApprovalStatus as U, registerAgentHookTerminalLifecycleHandler as V, shouldSuppressCodexAutoApprovalSyntheticTitle as W, createAgentCompletionCoordinator as X, useNotificationDispatch as Y, countVisibleFloatingWorkspaceItems as Z, toWorktreeRelativePath as _, isMainTerminalSideEffectAuthorityForPty as _t, refreshTerminalProviderSnapshotCapabilities as a, shouldMinimizeFloatingWorkspacePanelOnCloseShortcut as at, resolveExplicitFileLinkTarget as b, getDriverForPty as bt, OSC52_CLIPBOARD_SETTING_ID as c, matchFloatingWorkspacePanelOwnedAction as ct, sweepRestoredCodexPanesForStaleAccounts as d, createFloatingWorkspaceMarkdownTab as dt, isEventTargetInsideFloatingWorkspacePanel as et, extractTerminalFileLinkCandidates as f, createFloatingWorkspaceTerminalTab as ft, resolveTerminalFileLinkText as g, dispatchTerminalSideEffectBatch as gt, resolveTerminalFileLink as h, shouldPreserveTerminalScrollbackBuffers as ht, collectTerminalProviderSnapshotPtyIds as i, resolveFloatingWorkspaceBrowserWorkspaceId as it, dispatchTerminalCommandFinishedEvent as j, getTabIdsAwaitingHostHydrationRemount as jt, readCurrentPrimarySelectionText as k, ZOOM_LEVEL_CHANGED_EVENT as kt, CloseTerminalDialog as l, matchFloatingWorkspacePanelShortcut as lt, isPathInsideWorktree as m, pruneLocalTerminalScrollbackBuffers as mt, RemoteRuntimePtyRecoveryState as n, isFloatingWorkspacePanelVisible as nt, startTerminalProviderSnapshotCapabilitySynchronization as o, switchFloatingWorkspaceTab as ot, extractTerminalFileLinks as p, detachTerminalLayoutLeaf as pt, isQuestionAnsweredSubmitInput as q, retryAllRemoteRuntimePtyRecoveriesNow as r, isFloatingWorkspaceTerminalInputTarget as rt, terminalProviderHasAuthoritativeSnapshot as s, matchFloatingWorkspacePanelChord as st, REMOTE_RUNTIME_AUTO_RECOVERY_TIMEOUT_MS as t, isFloatingWorkspacePanelFocused as tt, notifyCodexPaneBoundForStaleSweep as u, createFloatingWorkspaceBrowserTab as ut, resolveTerminalFileUrlTarget as v, registerTerminalSideEffectFactConsumer as vt, APP_MENU_PASTE_EVENT as w, replaceDriverPtyId as wt, normalizeAbsolutePath as x, hydrateDrivers as xt, parseExplicitFileLinkTarget as y, getAllDrivers as yt, resolveAgentStatusTerminalTitle as z };
