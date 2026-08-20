import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { l as createLucideIcon, n as cn, t as Button } from "./button-DszXJEV6.js";
import { u as Timer } from "./workspace-status-wl52y3xd.js";
import { a as getAgentGeneratedTabTitlesDescription, i as getAgentStatusHooksTitle, l as matchesSettingsSearch, o as getAgentGeneratedTabTitlesTitle, r as getAgentStatusHooksDescription, s as getAgentCacheTimerSearchEntries } from "./useWindowsTerminalCapabilityOwnerKey-CsKlekLS.js";
import { t as Check } from "./check-Lb2n4tDb.js";
import { t as ChevronDown } from "./chevron-down-BRkP96Md.js";
import { t as ExternalLink } from "./external-link-BrcDtGAn.js";
import { t as GitFork } from "./git-fork-BvGQdixq.js";
import { t as Info } from "./info-D3uaWrfJ.js";
import { t as LoaderCircle } from "./loader-circle-CRZpWdsi.js";
import { t as RefreshCw } from "./refresh-cw-BU_ChOig.js";
import { t as Terminal } from "./terminal-Cen7Un9b.js";
import { $a as isClipboardTextByteLengthOverLimit, Ca as normalizeGlobalWindowsRuntimeDefault, Jc as parsePaneKey, M as getHostSettingOverride, N as setHostSettingOverride, Ro as parseRemoteRuntimePtyId, Vg as isPairedWebClientWindow, Wm as SSH_TERMINATE_RECONNECT_REQUIRED, _h as resolveTuiAgentLaunchEnv, bf as isResumableTuiAgent, cu as getUtf8ChunkEndIndex, d_ as normalizeLeftSidebarTintColor, ea as isCompletedPiCompatibleAgentWithLiveRecoveryRecord, eg as isTuiAgentEnabled, f_ as normalizeLeftSidebarTintOpacity, fh as getTuiAgentDefaultArgs, gh as resolveTuiAgentLaunchArgs, k as clearHostSettingOverride, n_ as resolveEffectiveTerminalAppearance, no as yieldToEventLoop, p_ as HEX_COLOR_RE, ph as getTuiAgentDefaultEnv, t as useAppStore, tg as normalizeDisabledTuiAgents, vh as applyAgentPermissionMode, yf as getAgentResumeArgv, yh as resolveAgentPermissionModeSummary } from "./store-CgXrfmaH.js";
import { t as TriangleAlert } from "./triangle-alert-HrLt1y9s.js";
import { T as keybindingMatchesInput, a as pluginCommandKeybindingActionId, m as getEffectiveKeybindingsForDefinition } from "./plugin-manifest-Bs-50M_g.js";
import { $ as getRepoExecutionHostId, lt as toSshExecutionHostId, st as parseExecutionHostId } from "./agent-status-3vUKbY6l.js";
import { n as toast } from "./dist-DgqligFk.js";
import { t as Label } from "./label-D-n9s_wS.js";
import { a as SelectTrigger, n as SelectContent, o as SelectValue, r as SelectItem, t as Select } from "./select-B67U0C6J.js";
import { n as SwitchIndicator } from "./switch-NhZdOYtg.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
import { t as useMountedRef } from "./useMountedRef-1omUd-IV.js";
import { O as isPrimarySelectionTextControl, k as readCurrentPrimarySelectionText } from "./remote-runtime-pty-recovery-state-CcyktY20.js";
import { t as getAllWorktreesFromState } from "./selectors-XOBeaOSb.js";
import { a as SettingsSegmentedControl, c as SettingsSwitchRow, i as SettingsRow, o as SettingsSubsectionHeader, r as SettingsBadge, s as SettingsSwitch } from "./SettingsFormControls-C0chb_HE.js";
import { t as getShortcutPlatform } from "./shortcut-platform-BbPBGzth.js";
import { a as DialogFooter, i as DialogDescription, o as DialogHeader, r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
import { t as Input } from "./input-DV5rpysh.js";
import { n as getAgentCatalog, t as AgentIcon } from "./agent-catalog-CBF2CV5Q.js";
import { a as TEXT_CONTROL_PASTE_CHUNK_MAX_BYTES, n as measureTextControlPasteByteLengthWithYield, o as TEXT_CONTROL_PASTE_DIRECT_MAX_BYTES, r as pasteTextIntoTextControl, s as TEXT_CONTROL_PASTE_MAX_BYTES, t as measureTextControlPasteByteLength } from "./text-control-paste-PhBVbE2p.js";
import { t as useDetectedAgents } from "./useDetectedAgents-KkNokXI_.js";
import { n as getAgentAwakeSearchKeywords, r as getAgentAwakeTitle, t as getAgentAwakeDescription } from "./agent-awake-copy-ClRvhNkR.js";
import { n as normalizeComputerAwakeMode, t as computerAwakeSettingsForMode } from "./computer-awake-mode-BI0nmvNI.js";
import { a as isMacUserAgent, i as isLinuxUserAgent } from "./pane-helpers-9eOmrw__.js";
import { a as readPrimarySelectionText, o as setPrimarySelectionEnabled, r as consumePrimarySelectionNativePasteSuppression, s as setPrimarySelectionText } from "./primary-selection-BsidtYsF.js";
var BookOpen = createLucideIcon("book-open", [["path", {
	d: "M12 7v14",
	key: "1akyts"
}], ["path", {
	d: "M3 18a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h5a4 4 0 0 1 4 4 4 4 0 0 1 4-4h5a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-6a3 3 0 0 0-3 3 3 3 0 0 0-3-3z",
	key: "ruj8y"
}]]);
var PanelLeft = createLucideIcon("panel-left", [["rect", {
	width: "18",
	height: "18",
	x: "3",
	y: "3",
	rx: "2",
	key: "afitv7"
}], ["path", {
	d: "M9 3v18",
	key: "fh3hqa"
}]]);
var isMac = navigator.userAgent.includes("Mac");
function applyUIZoom(level) {
	const zoomFactor = 1.2 ** level;
	window.api.ui.setZoomLevel(level);
	document.documentElement.style.setProperty("--ui-zoom-factor", String(zoomFactor));
	if (isMac) window.api.ui.syncTrafficLights(zoomFactor);
}
function syncZoomCSSVar() {
	const zoomFactor = 1.2 ** window.api.ui.getZoomLevel();
	document.documentElement.style.setProperty("--ui-zoom-factor", String(zoomFactor));
	if (isMac) window.api.ui.syncTrafficLights(zoomFactor);
}
function hexToRgba(hex, alpha) {
	let clean = normalizeLeftSidebarTintColor(hex).replace("#", "");
	if (clean.length === 3) clean = clean.split("").map((part) => part + part).join("");
	return `rgba(${Number.parseInt(clean.slice(0, 2), 16)}, ${Number.parseInt(clean.slice(2, 4), 16)}, ${Number.parseInt(clean.slice(4, 6), 16)}, ${alpha})`;
}
function applyAlpha(color, alpha) {
	if (alpha === void 0 || alpha >= 1 || !HEX_COLOR_RE.test(color.trim())) return color;
	return hexToRgba(color, Math.min(1, Math.max(0, alpha)));
}
function buildSurfaceVariables(args) {
	const { background, foreground, overrideTextTokens = false } = args;
	const accent = `color-mix(in srgb, ${foreground} 9%, ${background})`;
	const border = `color-mix(in srgb, ${foreground} 7%, ${background})`;
	const ring = `color-mix(in srgb, ${foreground} 44%, ${background})`;
	const vars = {
		"--worktree-sidebar": background,
		"--worktree-sidebar-foreground": foreground,
		"--worktree-sidebar-accent": accent,
		"--worktree-sidebar-accent-foreground": foreground,
		"--worktree-sidebar-border": border,
		"--worktree-sidebar-ring": ring,
		"--sidebar": background,
		"--sidebar-foreground": foreground,
		"--sidebar-accent": accent,
		"--sidebar-accent-foreground": foreground,
		"--sidebar-border": border,
		"--sidebar-ring": ring
	};
	if (overrideTextTokens) {
		vars["--background"] = background;
		vars["--foreground"] = foreground;
		vars["--card"] = `color-mix(in srgb, ${foreground} 4%, ${background})`;
		vars["--card-foreground"] = foreground;
		vars["--accent"] = `color-mix(in srgb, ${foreground} 9%, ${background})`;
		vars["--accent-foreground"] = foreground;
		vars["--muted"] = `color-mix(in srgb, ${foreground} 7%, ${background})`;
		vars["--muted-foreground"] = `color-mix(in srgb, ${foreground} 62%, ${background})`;
		vars["--border"] = `color-mix(in srgb, ${foreground} 7%, ${background})`;
	}
	return vars;
}
function resolveTerminalSurfaceVariables(settings, systemPrefersDark) {
	const appearance = resolveEffectiveTerminalAppearance(settings, systemPrefersDark);
	return buildSurfaceVariables({
		background: applyAlpha(settings.terminalColorOverrides?.background ?? appearance.theme?.background ?? "#000000", settings.terminalBackgroundOpacity),
		foreground: settings.terminalColorOverrides?.foreground ?? appearance.theme?.foreground ?? "#fafafa",
		overrideTextTokens: true
	});
}
function resolveTintedSurfaceVariables(settings) {
	const tintColor = normalizeLeftSidebarTintColor(settings.leftSidebarTintColor);
	const tintOpacity = normalizeLeftSidebarTintOpacity(settings.leftSidebarTintOpacity);
	return buildSurfaceVariables({
		background: `color-mix(in srgb, ${tintColor} ${Number((tintOpacity * 100).toFixed(2))}%, var(--background))`,
		foreground: "var(--foreground)"
	});
}
function resolveLeftSidebarStyleVariables(settings, systemPrefersDark) {
	if (!settings) return;
	switch (settings.leftSidebarAppearanceMode) {
		case "default": return;
		case "match-terminal": return resolveTerminalSurfaceVariables(settings, systemPrefersDark);
		case "tinted": return resolveTintedSurfaceVariables(settings);
	}
}
function lastInputBlocksHibernation(entry, inputAt) {
	if (inputAt >= entry.stateStartedAt) {
		if (entry.state === "working" || entry.state === "done") return true;
		if (inputAt > entry.stateStartedAt) return false;
	}
	for (let i = entry.stateHistory.length - 1; i >= 0; i--) {
		const past = entry.stateHistory[i];
		if (!past || inputAt < past.startedAt) continue;
		if (past.state === "working") return true;
		if (inputAt > past.startedAt) return false;
	}
	return false;
}
const DEFAULT_AGENT_HIBERNATION_IDLE_MS = 1800 * 1e3;
const MIN_AGENT_HIBERNATION_IDLE_MS = 60 * 1e3;
const MAX_AGENT_HIBERNATION_IDLE_MS = 1440 * 60 * 1e3;
function toRuntimePtyId(ptyId) {
	return parseRemoteRuntimePtyId(ptyId)?.handle ?? ptyId;
}
function getEffectiveAgentHibernationIdleMs(value) {
	return typeof value === "number" && Number.isFinite(value) && value >= 6e4 && value <= 864e5 ? value : DEFAULT_AGENT_HIBERNATION_IDLE_MS;
}
function getLivePtyIdsForTab(tab, ptyIdsByTabId, runtimeLivePtyIdsByWorktreeId, runtimeLivenessRequired) {
	const ids = /* @__PURE__ */ new Set();
	for (const id of runtimeLivePtyIdsByWorktreeId?.[tab.worktreeId] ?? []) if (typeof id === "string" && id.length > 0) ids.add(toRuntimePtyId(id));
	if (!runtimeLivenessRequired) {
		for (const id of ptyIdsByTabId[tab.id] ?? []) if (typeof id === "string" && id.length > 0) ids.add(toRuntimePtyId(id));
	}
	return [...ids];
}
function getPaneLivePtyId(entry, layout) {
	const parsed = parsePaneKey(entry.paneKey);
	if (!parsed || entry.tabId && parsed.tabId !== entry.tabId) return null;
	const ptyId = layout?.ptyIdsByLeafId?.[parsed.leafId];
	return ptyId ? {
		leafId: parsed.leafId,
		ptyId
	} : null;
}
function getEntryTabId(entry) {
	if (entry.tabId) return entry.tabId;
	return parsePaneKey(entry.paneKey)?.tabId ?? null;
}
var hasUnsettledOrUnknownDispatch = ({ orchestration }) => orchestration ? ![
	"completed",
	"failed",
	"circuit_broken"
].includes(orchestration.dispatchStatus ?? "") : false;
function getEligiblePane(args) {
	const { entry, tab, layout, livePtyIds, sleepingAgentSessionsByPaneKey, lastTerminalInputAtByPaneKey, foregroundTerminalLastSeenAtByTabId, mobileLockedPtyIds } = args;
	const sleepingRecord = sleepingAgentSessionsByPaneKey[entry.paneKey];
	const hasOnlyLivePiCompatibleRecoveryIdentity = isCompletedPiCompatibleAgentWithLiveRecoveryRecord(entry, sleepingRecord, tab.worktreeId);
	if (entry.state !== "done" || entry.interrupted === true || Boolean(entry.subagents?.length) || hasUnsettledOrUnknownDispatch(entry) || sleepingRecord && !hasOnlyLivePiCompatibleRecoveryIdentity) return null;
	if (getEntryTabId(entry) !== tab.id || entry.worktreeId && entry.worktreeId !== tab.worktreeId) return null;
	if (!entry.agentType || !isResumableTuiAgent(entry.agentType) || !entry.providerSession) return null;
	if (!getAgentResumeArgv(entry.agentType, entry.providerSession)) return null;
	const foregroundLastSeenAt = foregroundTerminalLastSeenAtByTabId[tab.id];
	const effectiveIdleStart = Math.max(entry.updatedAt, typeof foregroundLastSeenAt === "number" && Number.isFinite(foregroundLastSeenAt) ? foregroundLastSeenAt : 0);
	if (args.now - effectiveIdleStart < args.idleMs) return null;
	const inputAt = lastTerminalInputAtByPaneKey[entry.paneKey];
	if (typeof inputAt === "number" && Number.isFinite(inputAt) && lastInputBlocksHibernation(entry, inputAt)) return null;
	const livePane = getPaneLivePtyId(entry, layout);
	if (!livePane) return null;
	const { leafId, ptyId } = livePane;
	const runtimePtyId = toRuntimePtyId(ptyId);
	if (!livePtyIds.has(runtimePtyId) || mobileLockedPtyIds.has(runtimePtyId)) return null;
	return {
		paneKey: entry.paneKey,
		tabId: tab.id,
		leafId,
		ptyId,
		runtimePtyId,
		providerSessionId: entry.providerSession.id,
		state: entry.state,
		updatedAt: entry.updatedAt,
		effectiveIdleStart,
		inputAt: typeof inputAt === "number" && Number.isFinite(inputAt) ? inputAt : 0
	};
}
function signatureFor(worktreeId, panes) {
	return `${worktreeId}|${panes.slice().sort((a, b) => a.paneKey.localeCompare(b.paneKey)).map((pane) => `${pane.paneKey}:${pane.ptyId}:${pane.runtimePtyId}:${pane.providerSessionId}:${pane.state}:${pane.updatedAt}:${pane.effectiveIdleStart}:${pane.inputAt}`).join("|")}`;
}
function candidateIdFor(worktreeId, paneKey) {
	return `${worktreeId}|${paneKey}`;
}
function getAgentEntriesByTabId(agentStatusByPaneKey) {
	const entriesByTabId = /* @__PURE__ */ new Map();
	for (const entry of Object.values(agentStatusByPaneKey)) {
		if (!entry) continue;
		const tabId = getEntryTabId(entry);
		if (!tabId) continue;
		const entries = entriesByTabId.get(tabId);
		if (entries) entries.push(entry);
		else entriesByTabId.set(tabId, [entry]);
	}
	return entriesByTabId;
}
function planAgentHibernationCandidates(snapshot) {
	if (snapshot.settings?.experimentalAgentHibernation !== true) return [];
	const idleMs = getEffectiveAgentHibernationIdleMs(snapshot.settings.agentHibernationIdleMs);
	const mobileLockedPtyIds = new Set(snapshot.mobileLockedPtyIds.map(toRuntimePtyId));
	const foregroundTerminalTabIds = new Set(snapshot.foregroundTerminalTabIds);
	const runtimeLivenessRequiredWorktreeIds = new Set(snapshot.runtimeLivenessRequiredWorktreeIds ?? []);
	const agentEntriesByTabId = getAgentEntriesByTabId(snapshot.agentStatusByPaneKey);
	const candidates = [];
	for (const [worktreeId, tabs] of Object.entries(snapshot.tabsByWorktree)) {
		if (!worktreeId || worktreeId === snapshot.activeWorktreeId || tabs.length === 0) continue;
		if (runtimeLivenessRequiredWorktreeIds.has(worktreeId) && !Object.hasOwn(snapshot.runtimeLivePtyIdsByWorktreeId ?? {}, worktreeId)) continue;
		for (const tab of tabs) {
			if (foregroundTerminalTabIds.has(tab.id)) continue;
			const tabLivePtyIds = getLivePtyIdsForTab(tab, snapshot.ptyIdsByTabId, snapshot.runtimeLivePtyIdsByWorktreeId, runtimeLivenessRequiredWorktreeIds.has(worktreeId));
			if (tabLivePtyIds.length === 0) continue;
			const layout = snapshot.terminalLayoutsByTabId[tab.id];
			for (const entry of agentEntriesByTabId.get(tab.id) ?? []) {
				const eligible = getEligiblePane({
					entry,
					tab,
					layout,
					livePtyIds: new Set(tabLivePtyIds),
					sleepingAgentSessionsByPaneKey: snapshot.sleepingAgentSessionsByPaneKey,
					lastTerminalInputAtByPaneKey: snapshot.lastTerminalInputAtByPaneKey,
					foregroundTerminalLastSeenAtByTabId: snapshot.foregroundTerminalLastSeenAtByTabId,
					mobileLockedPtyIds,
					now: snapshot.now,
					idleMs
				});
				if (eligible) candidates.push({
					id: candidateIdFor(worktreeId, eligible.paneKey),
					worktreeId,
					paneKey: eligible.paneKey,
					tabId: eligible.tabId,
					leafId: eligible.leafId,
					paneKeys: [eligible.paneKey],
					targetPtyIds: [eligible.ptyId],
					expectedRuntimePtyIds: [eligible.runtimePtyId],
					signature: signatureFor(worktreeId, [eligible])
				});
			}
		}
	}
	return candidates.sort((a, b) => a.worktreeId.localeCompare(b.worktreeId) || a.paneKey.localeCompare(b.paneKey));
}
const SCRIPT_TEXTAREA_ROW_SCAN_CODE_UNITS = 64 * 1024;
function getDetectedSetupScriptTextareaRows(setup) {
	return clampRows(countScriptTextareaLines(setup, 6), 2, 6);
}
function getRepositoryHookScriptTextareaRows(script) {
	return clampRows((script.length === 0 ? 0 : countScriptTextareaLines(script, 13)) + 1, 4, 14);
}
function countScriptTextareaLines(text, maxLines) {
	if (text.length === 0) return 1;
	const scanLength = Math.min(text.length, SCRIPT_TEXTAREA_ROW_SCAN_CODE_UNITS);
	let lines = 1;
	for (let index = 0; index < scanLength; index += 1) {
		if (text.charCodeAt(index) !== 10) continue;
		lines += 1;
		if (lines >= maxLines) return lines;
	}
	return lines;
}
function clampRows(value, minRows, maxRows) {
	return Math.min(Math.max(value, minRows), maxRows);
}
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function RepoForkIndicator({ upstream, className }) {
	if (!upstream) return null;
	const label = `Fork of ${upstream.owner}/${upstream.repo}`;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
		asChild: true,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: cn("inline-flex shrink-0 items-center text-muted-foreground", className),
			"aria-label": label,
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(GitFork, {
				className: "size-3",
				"aria-hidden": "true"
			})
		})
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
		side: "top",
		sideOffset: 4,
		children: label
	})] });
}
function getHostDisplayLabelOverride(settings, hostId) {
	return getHostSettingOverride(settings, hostId, "displayLabel");
}
function applyHostRename(settings, hostId, nextLabel) {
	return setHostSettingOverride(settings, hostId, "displayLabel", nextLabel);
}
function clearHostRename(settings, hostId) {
	return clearHostSettingOverride(settings, hostId, "displayLabel");
}
function resolveHostRemoval(hostId) {
	const parsed = parseExecutionHostId(hostId);
	if (parsed?.kind === "ssh") return {
		kind: "ssh",
		targetId: parsed.targetId
	};
	if (parsed?.kind === "runtime") return {
		kind: "runtime",
		environmentId: parsed.environmentId
	};
	return null;
}
async function removeSshTargetWithBestEffortCleanup(api, id) {
	try {
		await api.terminateSessions({ targetId: id });
	} catch (err) {
		const message = err instanceof Error ? err.message : String(err);
		if (message.includes("SSH_TERMINATE_RECONNECT_REQUIRED")) try {
			await api.connect({ targetId: id });
			await api.terminateSessions({ targetId: id });
		} catch (reconnectErr) {
			console.warn("[ssh] Skipping remote session cleanup during target removal:", reconnectErr instanceof Error ? reconnectErr.message : String(reconnectErr));
		}
		else console.warn("[ssh] Skipping remote session cleanup during target removal:", message);
	}
	await api.removeTarget({ id });
}
function resolveSshHostRemoval(args) {
	const hostRepoIds = [...new Set(args.repos.filter((repo) => repo.connectionId?.trim() === args.targetId).map((repo) => repo.id))];
	const hostRepoIdSet = new Set(hostRepoIds);
	const workspaceWorktreeIds = [...new Set(args.worktrees.filter((worktree) => hostRepoIdSet.has(worktree.repoId) && !worktree.isMainWorktree).map((worktree) => worktree.id))];
	const isConnected = args.sshConnectionStates.get(args.targetId)?.status === "connected";
	return {
		targetId: args.targetId,
		workspaceWorktreeIds,
		hostRepoIds,
		workspaceCount: workspaceWorktreeIds.length + hostRepoIds.length,
		isConnected
	};
}
async function clearSshHostWorkspaces(resolution, mode) {
	const store = useAppStore.getState();
	const forgetLocalOnly = mode === "forget-local";
	const failedIds = [];
	for (const worktreeId of resolution.workspaceWorktreeIds) if (!(await store.removeWorktree(worktreeId, false, forgetLocalOnly ? { mode: "forget-local" } : void 0)).ok) failedIds.push(worktreeId);
	const hostId = toSshExecutionHostId(resolution.targetId);
	for (const repoId of resolution.hostRepoIds) {
		try {
			await store.removeProject(repoId, { hostId });
		} catch {
			failedIds.push(repoId);
		}
		if (useAppStore.getState().repos.some((repo) => repo.id === repoId && getRepoExecutionHostId(repo) === hostId) && !failedIds.includes(repoId)) failedIds.push(repoId);
	}
	return { failedIds };
}
function HostRemoveDialog({ open, onOpenChange, hostId, label, target }) {
	const [busy, setBusy] = (0, import_react.useState)(false);
	const [advancedOpen, setAdvancedOpen] = (0, import_react.useState)(false);
	const [deleteWorkspaces, setDeleteWorkspaces] = (0, import_react.useState)(false);
	const mountedRef = useMountedRef();
	const repos = useAppStore((s) => s.repos);
	const worktreesByRepo = useAppStore((s) => s.worktreesByRepo);
	const sshConnectionStates = useAppStore((s) => s.sshConnectionStates);
	const sshResolution = (0, import_react.useMemo)(() => {
		if (target.kind !== "ssh") return null;
		return resolveSshHostRemoval({
			targetId: target.targetId,
			repos,
			worktrees: getAllWorktreesFromState({ worktreesByRepo }),
			sshConnectionStates
		});
	}, [
		target,
		repos,
		worktreesByRepo,
		sshConnectionStates
	]);
	const workspaceCount = sshResolution?.workspaceCount ?? 0;
	const hasWorkspaces = workspaceCount > 0;
	const isConnected = sshResolution?.isConnected ?? false;
	const dropOverridesForHost = () => {
		const state = useAppStore.getState();
		state.updateSettings({ hostSettingOverrides: clearHostRename(state.settings, hostId) });
	};
	const removeSshTarget = async (targetId) => {
		await removeSshTargetWithBestEffortCleanup(window.api.ssh, targetId);
		useAppStore.getState().clearRemovedSshTargetState(targetId);
		dropOverridesForHost();
	};
	const handleRemoveRuntime = (environmentId) => {
		const state = useAppStore.getState();
		state.openSettingsTarget({
			pane: "servers",
			repoId: null,
			sectionId: environmentId
		});
		state.openSettingsPage();
		onOpenChange(false);
	};
	const runSshRemoval = async () => {
		if (target.kind !== "ssh") return;
		setBusy(true);
		try {
			if (deleteWorkspaces && sshResolution) {
				const { failedIds } = await clearSshHostWorkspaces(sshResolution, isConnected ? "delete-remote" : "forget-local");
				if (failedIds.length > 0) {
					if (mountedRef.current) setBusy(false);
					toast.error(translate("auto.components.sidebar.HostRemoveDialog.workspacesFailed", "Could not remove {{count}} of this host’s workspaces. The host was kept so you can retry.", { count: failedIds.length }));
					return;
				}
			}
			await removeSshTarget(target.targetId);
			if (mountedRef.current) onOpenChange(false);
			toast.success(translate("auto.components.sidebar.HostRemoveDialog.1a2b3c4d5e", "Removed {{value0}}", { value0: label }));
		} catch (err) {
			toast.error(err instanceof Error ? err.message : translate("auto.components.sidebar.HostRemoveDialog.2b3c4d5e6f", "Failed to remove host"));
		} finally {
			if (mountedRef.current) setBusy(false);
		}
	};
	const workspaceCountLabel = workspaceCount === 1 ? translate("auto.components.sidebar.HostRemoveDialog.oneWorkspace", "1 workspace") : translate("auto.components.sidebar.HostRemoveDialog.manyWorkspaces", "{{count}} workspaces", { count: workspaceCount });
	const description = target.kind === "runtime" ? translate("auto.components.sidebar.HostRemoveDialog.4d5e6f7a8b", "This opens the Orca servers settings where you can remove this server.") : hasWorkspaces ? translate("auto.components.sidebar.HostRemoveDialog.hostHasWorkspacesDefault", "Removes {{value0}} and its credentials from this computer. Its {{value1}} stay in Orca — remote files are not touched.", {
		value0: label,
		value1: workspaceCountLabel
	}) : translate("auto.components.sidebar.HostRemoveDialog.5e6f7a8b9c", "This removes the saved SSH host and its credentials from this computer. Remote files are not deleted.");
	const deleteOptionLabel = isConnected ? translate("auto.components.sidebar.HostRemoveDialog.alsoDeleteRemote", "Also delete these {{value0}} on {{value1}}", {
		value0: workspaceCountLabel,
		value1: label
	}) : translate("auto.components.sidebar.HostRemoveDialog.alsoForgetLocal", "Also remove these {{value0}} from Orca", { value0: workspaceCountLabel });
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
		open,
		onOpenChange,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogContent, {
			className: "sm:max-w-md",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, { children: translate("auto.components.sidebar.HostRemoveDialog.3c4d5e6f7a", "Remove {{value0}}?", { value0: label }) }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, { children: description })] }),
				target.kind === "ssh" && hasWorkspaces ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
					type: "button",
					variant: "ghost",
					size: "sm",
					onClick: () => setAdvancedOpen((v) => !v),
					"aria-expanded": advancedOpen,
					className: "-ml-2 text-xs",
					children: [translate("auto.components.sidebar.HostRemoveDialog.advanced", "Advanced"), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronDown, { className: cn("size-4 transition-transform", advancedOpen && "rotate-180") })]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: cn("grid overflow-hidden transition-[grid-template-rows] duration-200 ease-out", advancedOpen ? "grid-rows-[1fr]" : "grid-rows-[0fr]"),
					"aria-hidden": !advancedOpen,
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "min-h-0",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex items-start gap-3 px-1 pt-1",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
								type: "button",
								role: "switch",
								"aria-checked": deleteWorkspaces,
								onClick: () => setDeleteWorkspaces((v) => !v),
								className: "group mt-0.5 flex shrink-0 cursor-pointer items-center rounded-md outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50",
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SwitchIndicator, { checked: deleteWorkspaces })
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
								className: "min-w-0 flex-1 text-xs leading-snug",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "font-medium text-foreground",
									children: deleteOptionLabel
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "mt-0.5 block text-muted-foreground",
									children: isConnected ? translate("auto.components.sidebar.HostRemoveDialog.alsoDeleteRemoteHint", "Permanently deletes the remote Git worktrees and their branches. Cannot be undone.") : translate("auto.components.sidebar.HostRemoveDialog.alsoForgetLocalHint", "Clears them from Orca only. Remote files, worktrees, and branches are left untouched.")
								})]
							})]
						})
					})
				})] }) : null,
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogFooter, {
					className: "gap-2 sm:gap-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						type: "button",
						variant: "outline",
						disabled: busy,
						onClick: () => onOpenChange(false),
						children: translate("auto.components.sidebar.HostRemoveDialog.6f7a8b9c0d", "Cancel")
					}), target.kind === "runtime" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						type: "button",
						variant: "destructive",
						onClick: () => handleRemoveRuntime(target.environmentId),
						children: translate("auto.components.sidebar.HostRemoveDialog.7a8b9c0d1e", "Open settings")
					}) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
						type: "button",
						variant: "destructive",
						disabled: busy,
						onClick: () => void runSshRemoval(),
						children: [busy ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3.5 animate-spin" }) : null, translate("auto.components.sidebar.HostRemoveDialog.8b9c0d1e2f", "Remove host")]
					})]
				})
			]
		})
	});
}
function isMacShortcutPlatform() {
	return getShortcutPlatform() === "darwin";
}
function getUpdateCheckHint(isMac$1 = isMacShortcutPlatform()) {
	const releaseHints = `${isMac$1 ? "⇧+click" : "Shift+click"} checks the latest RC; ${isMac$1 ? "⌘+click" : "Ctrl+click"} checks the latest perf build.`;
	return isMac$1 ? `${releaseHints} ⌥+click chooses a local macOS build.` : releaseHints;
}
function getUpdateCheckClickOptions(event, isMac$1 = isMacShortcutPlatform()) {
	if (isMac$1 && event.altKey) return { localBuild: true };
	return {
		includePrerelease: event.shiftKey,
		includePerfPrerelease: isMac$1 ? event.metaKey : event.ctrlKey
	};
}
var WINDOWS_DRIVE_PATH_PATTERN = /^[A-Za-z]:/;
function isAbsoluteSparseDirectoryPath(value) {
	const entry = value.trim();
	return entry.startsWith("/") || entry.startsWith("\\") || WINDOWS_DRIVE_PATH_PATTERN.test(entry);
}
function forEachSparseDirectoryInputLine(value, visit) {
	let lineStart = 0;
	for (let index = 0; index <= value.length; index += 1) {
		if (index < value.length && value.charCodeAt(index) !== 10) continue;
		const lineEnd = index > lineStart && value.charCodeAt(index - 1) === 13 ? index - 1 : index;
		if (visit(value.slice(lineStart, lineEnd)) === false) return;
		lineStart = index + 1;
	}
}
function normalizeSparseDirectoryLines(value) {
	const seen = /* @__PURE__ */ new Set();
	const directories = [];
	forEachSparseDirectoryInputLine(value, (rawEntry) => {
		const entry = rawEntry.trim().replace(/\\/g, "/").replace(/^\/+|\/+$/g, "");
		if (entry.length === 0 || seen.has(entry)) return;
		seen.add(entry);
		directories.push(entry);
	});
	return directories;
}
function hasSparseDirectoryParentSegment(entry) {
	let segmentStart = 0;
	for (let index = 0; index <= entry.length; index += 1) {
		if (index < entry.length && entry[index] !== "/") continue;
		if (entry.slice(segmentStart, index) === "..") return true;
		segmentStart = index + 1;
	}
	return false;
}
function sparseDirectoriesMatch(left, right) {
	if (left.length !== right.length) return false;
	const set = new Set(left);
	return right.every((entry) => set.has(entry));
}
function parseSparsePresetDirectories(value) {
	let hasAbsoluteEntry = false;
	forEachSparseDirectoryInputLine(value, (rawEntry) => {
		const entry = rawEntry.trim();
		if (entry.length === 0) return;
		if (isAbsoluteSparseDirectoryPath(entry)) {
			hasAbsoluteEntry = true;
			return false;
		}
	});
	if (hasAbsoluteEntry) return {
		directories: [],
		error: translate("auto.lib.sparse.preset.draft.5915a0a1f6", "Use repo-relative directories, not root, absolute paths, or parent segments.")
	};
	const directories = normalizeSparseDirectoryLines(value);
	if (directories.length === 0) return {
		directories,
		error: translate("auto.lib.sparse.preset.draft.efc05d1820", "Add at least one directory.")
	};
	if (directories.some((entry) => entry === "." || hasSparseDirectoryParentSegment(entry))) return {
		directories: [],
		error: translate("auto.lib.sparse.preset.draft.5915a0a1f6", "Use repo-relative directories, not root, absolute paths, or parent segments.")
	};
	return {
		directories,
		error: null
	};
}
function SearchableSetting({ title, description, forceVisible = false, keywords, children, className, id }) {
	const query = useAppStore((state) => state.settingsSearchQuery);
	if (!forceVisible && !matchesSettingsSearch(query, {
		title,
		description,
		keywords
	})) return null;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: cn("scroll-mt-6 w-full max-w-3xl", className),
		id,
		children
	});
}
function AgentAwakeSetting({ settings, updateSettings }) {
	const title = getAgentAwakeTitle();
	const description = getAgentAwakeDescription();
	const mode = normalizeComputerAwakeMode(settings.computerAwakeMode, settings.keepComputerAwakeWhileAgentsRun);
	const setMode = (nextMode) => {
		updateSettings(computerAwakeSettingsForMode(nextMode));
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("section", {
		className: "space-y-3",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SearchableSetting, {
			title,
			description,
			keywords: getAgentAwakeSearchKeywords(),
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-start justify-between gap-4 py-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "min-w-0 flex-1 space-y-0.5",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, { children: title }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "text-xs text-muted-foreground",
						children: description
					})]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsSegmentedControl, {
					value: mode,
					onChange: setMode,
					ariaLabel: title,
					size: "sm",
					options: [
						{
							value: "on",
							label: translate("auto.components.settings.AgentAwakeSetting.on", "On")
						},
						{
							value: "auto",
							label: translate("auto.components.settings.AgentAwakeSetting.auto", "Agent")
						},
						{
							value: "off",
							label: translate("auto.components.settings.AgentAwakeSetting.off", "Off")
						}
					]
				})]
			})
		})
	});
}
function AgentCacheTimerSection({ settings, updateSettings }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", {
		className: "space-y-4",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsSubsectionHeader, {
				title: translate("auto.components.settings.AgentCacheTimerSection.a137f8854d", "Prompt Cache Timer"),
				description: translate("auto.components.settings.AgentCacheTimerSection.fe590653c1", "Claude caches your conversation to reduce costs. When idle too long the cache expires and the next message resends full context at higher cost. This shows a countdown so you know when to resume.")
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(SearchableSetting, {
				title: translate("auto.components.settings.AgentCacheTimerSection.b4e7302944", "Cache Timer"),
				description: translate("auto.components.settings.AgentCacheTimerSection.9c20253679", "Show a countdown after a Claude agent becomes idle."),
				keywords: getAgentCacheTimerSearchEntries().flatMap((entry) => [
					entry.title,
					entry.description ?? "",
					...entry.keywords ?? []
				]),
				className: "flex items-center justify-between gap-4 py-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "min-w-0 flex-1 space-y-0.5",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-center gap-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Timer, { className: "size-4 text-muted-foreground" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, { children: translate("auto.components.settings.AgentCacheTimerSection.b4e7302944", "Cache Timer") })]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "text-xs text-muted-foreground",
						children: translate("auto.components.settings.AgentCacheTimerSection.487b176240", "Show a countdown in the sidebar after a Claude agent becomes idle.")
					})]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsSwitch, {
					ariaLabel: translate("auto.components.settings.AgentCacheTimerSection.b4e7302944", "Cache Timer"),
					checked: settings.promptCacheTimerEnabled,
					onChange: () => {
						const enabling = !settings.promptCacheTimerEnabled;
						updateSettings({ promptCacheTimerEnabled: enabling });
						if (enabling) useAppStore.getState().seedCacheTimersForIdleTabs();
					}
				})]
			}),
			settings.promptCacheTimerEnabled && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(SearchableSetting, {
				title: translate("auto.components.settings.AgentCacheTimerSection.a2a8962138", "Timer Duration"),
				description: translate("auto.components.settings.AgentCacheTimerSection.80c454e8a6", "Match this to your provider's cache TTL."),
				keywords: [
					"cache",
					"timer",
					"duration",
					"ttl"
				],
				className: "flex items-center justify-between gap-4 py-2 pl-7",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "min-w-0 flex-1 space-y-0.5",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, { children: translate("auto.components.settings.AgentCacheTimerSection.a2a8962138", "Timer Duration") }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "text-xs text-muted-foreground",
						children: translate("auto.components.settings.AgentCacheTimerSection.8b9e202e0a", "Match this to your provider's cache TTL. The default is 5 minutes.")
					})]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Select, {
					value: String(settings.promptCacheTtlMs),
					onValueChange: (v) => updateSettings({ promptCacheTtlMs: Number(v) }),
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SelectTrigger, {
						size: "sm",
						className: "h-7 text-xs w-[120px]",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SelectValue, {})
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(SelectContent, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SelectItem, {
						value: "300000",
						children: translate("auto.components.settings.AgentCacheTimerSection.54395ecd7c", "5 minutes")
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SelectItem, {
						value: "3600000",
						children: translate("auto.components.settings.AgentCacheTimerSection.05de84a104", "1 hour")
					})] })]
				})]
			})
		]
	});
}
var EMPTY_WSL_DISTROS = [];
var NO_DISTRO_VALUE = "__select_wsl_distro__";
function getHostRuntimeLabel() {
	return typeof navigator !== "undefined" && navigator.userAgent.includes("Windows") ? "Windows" : "This device";
}
function AgentRuntimeSetting({ settings, updateSettings, refresh, wslSupportedPlatform = false, wslAvailable = false, wslDistros = EMPTY_WSL_DISTROS, wslCapabilitiesLoading = false }) {
	if (!wslSupportedPlatform) return null;
	const runtimeDefault = normalizeGlobalWindowsRuntimeDefault(settings.localWindowsRuntimeDefault);
	const nextWslDistro = getNextWslDistro(runtimeDefault, wslDistros);
	const distroOptions = getVisibleDistroOptions(runtimeDefault, wslDistros);
	const updateAgentRuntime = (updates) => {
		Promise.resolve(updateSettings(updates)).then(() => refresh());
	};
	const handleRuntimeChange = (value) => {
		if (value === "windows-host") {
			updateAgentRuntime({ localWindowsRuntimeDefault: { kind: "windows-host" } });
			return;
		}
		if (nextWslDistro) updateAgentRuntime({ localWindowsRuntimeDefault: {
			kind: "wsl",
			distro: nextWslDistro
		} });
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("section", {
		className: "space-y-3",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsRow, {
			label: translate("auto.components.settings.AgentRuntimeSetting.label", "Agent runtime"),
			alignTop: true,
			description: getDescription(runtimeDefault, wslAvailable, wslCapabilitiesLoading),
			control: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex w-52 flex-col items-stretch gap-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsSegmentedControl, {
					ariaLabel: translate("auto.components.settings.AgentRuntimeSetting.label", "Agent runtime"),
					value: runtimeDefault.kind,
					onChange: handleRuntimeChange,
					equalWidth: true,
					options: [{
						value: "windows-host",
						label: getHostRuntimeLabel()
					}, {
						value: "wsl",
						label: translate("auto.components.settings.AgentRuntimeSetting.wsl", "WSL"),
						disabled: wslCapabilitiesLoading || !wslAvailable || !nextWslDistro
					}]
				}), runtimeDefault.kind === "wsl" ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Select, {
					value: runtimeDefault.distro ?? NO_DISTRO_VALUE,
					onValueChange: (distro) => {
						if (distro !== NO_DISTRO_VALUE) updateAgentRuntime({ localWindowsRuntimeDefault: {
							kind: "wsl",
							distro
						} });
					},
					disabled: wslCapabilitiesLoading || !wslAvailable,
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SelectTrigger, {
						size: "sm",
						className: "w-full min-w-52",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SelectValue, { placeholder: wslCapabilitiesLoading ? translate("auto.components.settings.AgentRuntimeSetting.loadingWsl", "Loading WSL") : translate("auto.components.settings.AgentRuntimeSetting.selectDistro", "Select distro") })
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(SelectContent, { children: [!runtimeDefault.distro ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SelectItem, {
						value: NO_DISTRO_VALUE,
						children: translate("auto.components.settings.AgentRuntimeSetting.selectDistro", "Select distro")
					}) : null, distroOptions.map((distro) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SelectItem, {
						value: distro,
						children: distro
					}, distro))] })]
				}) : null]
			})
		})
	});
}
function getNextWslDistro(runtimeDefault, wslDistros) {
	if (runtimeDefault.kind === "wsl" && runtimeDefault.distro?.trim()) return runtimeDefault.distro.trim();
	return wslDistros.find((distro) => distro.trim().length > 0) ?? null;
}
function getVisibleDistroOptions(runtimeDefault, wslDistros) {
	const options = [...wslDistros];
	if (runtimeDefault.kind === "wsl" && runtimeDefault.distro && !options.includes(runtimeDefault.distro)) return [runtimeDefault.distro, ...options];
	return options;
}
function getDescription(runtimeDefault, wslAvailable, wslCapabilitiesLoading) {
	if (runtimeDefault.kind === "windows-host") return translate("auto.components.settings.AgentRuntimeSetting.windowsDescription", "Detect and launch agents on Windows for projects that do not override their runtime.");
	if (!wslAvailable && !wslCapabilitiesLoading) return translate("auto.components.settings.AgentRuntimeSetting.wslUnavailable", "WSL is not available on this machine.");
	if (!runtimeDefault.distro) return translate("auto.components.settings.AgentRuntimeSetting.distroRequired", "Choose a WSL distro before projects can inherit WSL.");
	return translate("auto.components.settings.AgentRuntimeSetting.wslDescription", "Detect and launch agents in {{value0}} via WSL for projects that do not override their runtime.", { value0: runtimeDefault.distro });
}
function findWslSourceHomeKey(wsl, distro) {
	const normalized = distro.trim().toLowerCase();
	return Object.keys(wsl ?? {}).find((key) => key.trim().toLowerCase() === normalized);
}
function buildCodexSessionSourceHomeControl(settings, updateSettings) {
	const runtimeScope = normalizeGlobalWindowsRuntimeDefault(settings.localWindowsRuntimeDefault);
	const sourceHome = settings.codexSessionSourceHome;
	const wslDistro = runtimeScope.kind === "wsl" ? runtimeScope.distro?.trim() : void 0;
	if (wslDistro) {
		const existingKey = findWslSourceHomeKey(sourceHome?.wsl, wslDistro);
		return {
			runtimeLabel: `${wslDistro}: ~/.codex`,
			value: (existingKey ? sourceHome?.wsl?.[existingKey] : void 0) ?? "",
			onSave: (value) => saveCodexSessionSourceHome(settings, updateSettings, {
				runtime: "wsl",
				distro: existingKey ?? wslDistro,
				value
			})
		};
	}
	return {
		runtimeLabel: "~/.codex",
		value: sourceHome?.host ?? "",
		onSave: (value) => saveCodexSessionSourceHome(settings, updateSettings, {
			runtime: "host",
			value
		})
	};
}
function saveCodexSessionSourceHome(settings, updateSettings, args) {
	const current = settings.codexSessionSourceHome ?? {};
	const trimmed = args.value.trim();
	if (args.runtime === "host") {
		updateSettings({ codexSessionSourceHome: {
			...current,
			host: trimmed || void 0
		} });
		return;
	}
	const nextWsl = { ...current.wsl };
	const targetKey = findWslSourceHomeKey(nextWsl, args.distro) ?? args.distro;
	if (trimmed) nextWsl[targetKey] = trimmed;
	else delete nextWsl[targetKey];
	updateSettings({ codexSessionSourceHome: {
		...current,
		wsl: Object.keys(nextWsl).length > 0 ? nextWsl : void 0
	} });
}
function AgentSessionSourceHomeInput({ runtimeLabel, value, onSave }) {
	const [draft, setDraft] = (0, import_react.useState)(value);
	const commit = () => {
		onSave(draft.trim());
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex flex-col gap-1",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
			className: "flex items-center gap-1.5 text-xs text-muted-foreground",
			children: [translate("auto.components.settings.AgentsPane.codexSessionSource", "Codex home to import from"), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
				asChild: true,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
					type: "button",
					"aria-label": translate("auto.components.settings.AgentsPane.codexSessionSourceInfo", "About importing Codex history"),
					className: "grid size-4 place-items-center rounded text-muted-foreground outline-none transition-colors hover:bg-muted hover:text-foreground focus-visible:ring-[3px] focus-visible:ring-ring/50",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Info, { className: "size-3" })
				})
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
				side: "top",
				sideOffset: 6,
				className: "max-w-xs",
				children: translate("auto.components.settings.AgentsPane.codexSessionSourceTooltip", "Orca runs Codex in an isolated home. Point this at your existing Codex home to import that session history. Empty uses ~/.codex.")
			})] })]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex items-center gap-2",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
				value: draft,
				onChange: (e) => setDraft(e.target.value),
				onBlur: commit,
				onKeyDown: (e) => {
					if (e.key === "Enter") {
						commit();
						e.currentTarget.blur();
					}
					if (e.key === "Escape") {
						setDraft(value);
						e.currentTarget.blur();
					}
				},
				placeholder: runtimeLabel,
				spellCheck: false,
				className: "h-7 flex-1 font-mono text-xs"
			}), value.trim() && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				type: "button",
				variant: "ghost",
				size: "xs",
				onClick: () => {
					onSave("");
					setDraft("");
				},
				className: "h-7 shrink-0 text-xs text-muted-foreground hover:text-foreground",
				children: translate("auto.components.settings.AgentsPane.5200dac9da", "Reset")
			})]
		})]
	});
}
function buildSummaries() {
	return {
		sourceControlAiDefaults: {
			ownership: "client-default",
			label: translate("auto.components.settings.settingOwnership.clientDefault", "Client default"),
			description: translate("auto.components.settings.settingOwnership.sourceControlAiDefaults", "Recipes, prompts, and hosted-review defaults are shared by this client; model choices and discovery stay scoped to the host where the agent runs.")
		},
		repositorySourceControlAi: {
			ownership: "project-host-setup",
			label: translate("auto.components.settings.settingOwnership.projectOnThisHost", "Project on this host"),
			description: translate("auto.components.settings.settingOwnership.repositorySourceControlAi", "These overrides apply to this project setup and inherit the client Source Control AI defaults until customized.")
		},
		agentLaunchDefaults: {
			ownership: "client-default",
			label: translate("auto.components.settings.settingOwnership.clientDefault", "Client default"),
			description: translate("auto.components.settings.settingOwnership.agentLaunchDefaults", "Default agent, command overrides, CLI arguments, and launch environment are client preferences. SSH and remote server launches still validate host availability at run time.")
		},
		terminalQuickCommands: {
			ownership: "host-collection",
			label: translate("auto.components.settings.settingOwnership.hostCollectionProjectScopes", "Host collection + project scopes"),
			description: translate("auto.components.settings.settingOwnership.terminalQuickCommandHostCollections", "Commands are saved on the selected Orca host, then scoped globally or to a project setup. Commands from this device also remain available in remote workspaces.")
		},
		workspaceDirectory: {
			ownership: "host-override",
			label: translate("auto.components.settings.settingOwnership.hostOverride", "Host override"),
			description: translate("auto.components.settings.settingOwnership.workspaceDirectory", "The client default is inherited until a host needs its own worktree directory.")
		},
		providerAccounts: {
			ownership: "provider-host",
			label: translate("auto.components.settings.settingOwnership.providerHost", "Provider host"),
			description: translate("auto.components.settings.settingOwnership.providerAccounts", "Credentials and account checks belong to the local client or selected remote server that owns the provider integration.")
		}
	};
}
function getSettingOwnershipSummary(key) {
	return buildSummaries()[key];
}
function stringifyAgentDefaultEnvDraft(env) {
	return Object.entries(env).map(([name, value]) => `${name}=${value}`).join(" ");
}
function parseAgentDefaultEnvDraft(value) {
	if (isClipboardTextByteLengthOverLimit(value, 8192)) return {
		env: {},
		tooLarge: true
	};
	const env = {};
	for (const pair of getAgentDefaultEnvDraftPairs(value)) {
		const separatorIndex = pair.indexOf("=");
		if (separatorIndex <= 0) continue;
		const name = pair.slice(0, separatorIndex).trim();
		if (!name) continue;
		env[name] = pair.slice(separatorIndex + 1);
	}
	return {
		env,
		tooLarge: false
	};
}
function getAgentDefaultEnvDraftPairs(value) {
	const pairs = [];
	let tokenStart = -1;
	for (let index = 0; index <= value.length; index += 1) {
		if (!(index === value.length) && !isAgentDefaultEnvDraftWhitespace(value.charCodeAt(index))) {
			if (tokenStart === -1) tokenStart = index;
			continue;
		}
		if (tokenStart !== -1) {
			pairs.push(value.slice(tokenStart, index));
			tokenStart = -1;
		}
	}
	return pairs;
}
function isAgentDefaultEnvDraftWhitespace(code) {
	return code === 32 || code >= 9 && code <= 13 || code === 160 || code === 5760 || code >= 8192 && code <= 8202 || code === 8232 || code === 8233 || code === 8239 || code === 8287 || code === 12288 || code === 65279;
}
function buildAgentAvailabilitySettingsUpdate(settings, id, enabled) {
	const latestDisabled = normalizeDisabledTuiAgents(settings.disabledTuiAgents);
	return {
		disabledTuiAgents: enabled ? latestDisabled.filter((agent) => agent !== id) : latestDisabled.includes(id) ? latestDisabled : [...latestDisabled, id],
		...settings.defaultTuiAgent === id && !enabled ? { defaultTuiAgent: null } : {}
	};
}
function createAgentAvailabilityUpdateQueue() {
	let pendingUpdate = Promise.resolve();
	return ({ getSettings, fallbackSettings, updateSettings, agentId, enabled }) => {
		pendingUpdate = pendingUpdate.catch(() => {}).then(() => updateSettings(buildAgentAvailabilitySettingsUpdate(getSettings() ?? fallbackSettings, agentId, enabled)));
		return pendingUpdate.then(() => void 0);
	};
}
var enqueueAgentAvailabilityUpdate = createAgentAvailabilityUpdateQueue();
function AgentAvailabilityControl({ label, isEnabled, onSetEnabled }) {
	const value = isEnabled ? "enabled" : "disabled";
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsSegmentedControl, {
		value,
		onChange: (next) => {
			if (next !== value) onSetEnabled(next === "enabled");
		},
		ariaLabel: translate("auto.components.settings.AgentsPane.1c9a9679ec", "{{value0}} availability", { value0: label }),
		size: "sm",
		options: [{
			value: "enabled",
			label: translate("auto.components.settings.AgentsPane.d4d2a45d63", "Enabled")
		}, {
			value: "disabled",
			label: translate("auto.components.settings.AgentsPane.8dc0192e48", "Disabled")
		}]
	});
}
function AgentPermissionsSetting({ mode, onChange }) {
	const visibleMode = mode === "manual" ? "manual" : "yolo";
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("section", {
		className: "space-y-3",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsSubsectionHeader, {
			title: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
				className: "flex items-center gap-2",
				children: [translate("auto.components.settings.AgentsPane.agentPermissions", "Agent Permissions"), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
					asChild: true,
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
						type: "button",
						"aria-label": translate("auto.components.settings.AgentsPane.agentPermissionsInfo", "Agent permissions info"),
						className: "grid size-5 place-items-center rounded-md text-muted-foreground outline-none transition-colors hover:bg-muted hover:text-foreground focus-visible:ring-[3px] focus-visible:ring-ring/50",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Info, { className: "size-3.5" })
					})
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
					side: "top",
					sideOffset: 6,
					children: translate("auto.components.settings.AgentsPane.agentPermissionsTooltip", "Doesn't apply to agents where you've overridden launch arguments.")
				})] })]
			}),
			description: translate("auto.components.settings.AgentsPane.agentPermissionsDescription", "Choose whether Orca launches agents with fewer permission prompts or with manual checks."),
			action: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsSegmentedControl, {
				value: visibleMode,
				onChange: (nextMode) => {
					if (nextMode !== "mixed") onChange(nextMode);
				},
				ariaLabel: translate("auto.components.settings.AgentsPane.agentPermissions", "Agent Permissions"),
				size: "sm",
				options: [{
					value: "yolo",
					label: translate("auto.components.settings.AgentsPane.agentPermissionsYolo", "Yolo")
				}, {
					value: "manual",
					label: translate("auto.components.settings.AgentsPane.agentPermissionsManual", "Manual")
				}]
			})
		})
	});
}
function AgentCommandOverrideInput({ defaultCmd, cmdOverride, onSaveOverride }) {
	const draftSeed = cmdOverride ?? defaultCmd;
	const [cmdDraft, setCmdDraft] = (0, import_react.useState)(draftSeed);
	const commitCmd = () => {
		const trimmed = cmdDraft.trim();
		if (!trimmed || trimmed === defaultCmd) {
			onSaveOverride("");
			setCmdDraft(defaultCmd);
		} else onSaveOverride(trimmed);
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex flex-col gap-1",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "text-xs text-muted-foreground",
			children: translate("auto.components.settings.AgentsPane.2e45ca29b6", "Command")
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex items-center gap-2",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
				value: cmdDraft,
				onChange: (e) => setCmdDraft(e.target.value),
				onBlur: commitCmd,
				onKeyDown: (e) => {
					if (e.key === "Enter") {
						commitCmd();
						e.currentTarget.blur();
					}
					if (e.key === "Escape") {
						setCmdDraft(draftSeed);
						e.currentTarget.blur();
					}
				},
				placeholder: defaultCmd,
				spellCheck: false,
				className: "h-7 flex-1 font-mono text-xs"
			}), cmdOverride && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				type: "button",
				variant: "ghost",
				size: "xs",
				onClick: () => {
					onSaveOverride("");
					setCmdDraft(defaultCmd);
				},
				className: "h-7 shrink-0 text-xs text-muted-foreground hover:text-foreground",
				children: translate("auto.components.settings.AgentsPane.5200dac9da", "Reset")
			})]
		})]
	});
}
function AgentDefaultArgsInput({ defaultArgs, argsOverride, onSaveArgs }) {
	const draftSeed = argsOverride;
	const [argsDraft, setArgsDraft] = (0, import_react.useState)(draftSeed);
	const commitArgs = () => {
		onSaveArgs(argsDraft.trim());
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex flex-col gap-1",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "text-xs text-muted-foreground",
			children: translate("auto.components.settings.AgentsPane.cfb3f35775", "Arguments")
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex items-center gap-2",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
				value: argsDraft,
				onChange: (e) => setArgsDraft(e.target.value),
				onBlur: commitArgs,
				onKeyDown: (e) => {
					if (e.key === "Enter") {
						commitArgs();
						e.currentTarget.blur();
					}
					if (e.key === "Escape") {
						setArgsDraft(draftSeed);
						e.currentTarget.blur();
					}
				},
				placeholder: defaultArgs || translate("auto.components.settings.AgentsPane.6f99bf5dd0", "No default arguments"),
				spellCheck: false,
				className: "h-7 flex-1 font-mono text-xs"
			}), argsOverride !== defaultArgs && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				type: "button",
				variant: "ghost",
				size: "xs",
				onClick: () => {
					onSaveArgs(defaultArgs);
					setArgsDraft(defaultArgs);
				},
				className: "h-7 shrink-0 text-xs text-muted-foreground hover:text-foreground",
				children: translate("auto.components.settings.AgentsPane.5200dac9da", "Reset")
			})]
		})]
	});
}
function AgentDefaultEnvInput({ defaultEnv, envOverride, onSaveEnv }) {
	const defaultEnvText = stringifyAgentDefaultEnvDraft(defaultEnv);
	const draftSeed = stringifyAgentDefaultEnvDraft(envOverride);
	const [envDraft, setEnvDraft] = (0, import_react.useState)(draftSeed);
	const [envDraftTooLarge, setEnvDraftTooLarge] = (0, import_react.useState)(false);
	const envDraftErrorId = (0, import_react.useId)();
	const commitEnv = () => {
		const parsedDraft = parseAgentDefaultEnvDraft(envDraft);
		setEnvDraftTooLarge(parsedDraft.tooLarge);
		if (parsedDraft.tooLarge) return;
		onSaveEnv(parsedDraft.env);
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex flex-col gap-1",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "text-xs text-muted-foreground",
				children: translate("auto.components.settings.AgentsPane.8fbe1f37c1", "Environment")
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-center gap-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
					value: envDraft,
					onChange: (e) => {
						setEnvDraft(e.target.value);
						if (envDraftTooLarge) setEnvDraftTooLarge(false);
					},
					onBlur: commitEnv,
					onKeyDown: (e) => {
						if (e.key === "Enter") {
							commitEnv();
							e.currentTarget.blur();
						}
						if (e.key === "Escape") {
							setEnvDraft(draftSeed);
							setEnvDraftTooLarge(false);
							e.currentTarget.blur();
						}
					},
					placeholder: defaultEnvText || translate("auto.components.settings.AgentsPane.2d133152fa", "No default environment"),
					spellCheck: false,
					"aria-invalid": envDraftTooLarge || void 0,
					"aria-describedby": envDraftTooLarge ? envDraftErrorId : void 0,
					className: cn("h-7 flex-1 font-mono text-xs", envDraftTooLarge && "border-destructive/50 bg-destructive/5")
				}), draftSeed !== defaultEnvText && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					type: "button",
					variant: "ghost",
					size: "xs",
					onClick: () => {
						onSaveEnv(defaultEnv);
						setEnvDraft(defaultEnvText);
						setEnvDraftTooLarge(false);
					},
					className: "h-7 shrink-0 text-xs text-muted-foreground hover:text-foreground",
					children: translate("auto.components.settings.AgentsPane.5200dac9da", "Reset")
				})]
			}),
			envDraftTooLarge && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				id: envDraftErrorId,
				className: "mt-1 text-[11px] text-destructive",
				children: translate("auto.components.settings.AgentsPane.3f1bdf3cb4", "Environment text is too large to parse safely.")
			})
		]
	});
}
function AgentRow({ agentId, label, homepageUrl, defaultCmd, defaultArgs, defaultEnv, isDetected, isEnabled, isDefault, cmdOverride, argsOverride, envOverride, onSetDefault, onSetEnabled, onSaveOverride, onSaveArgs, onSaveEnv, sessionSourceHome }) {
	const envSummary = stringifyAgentDefaultEnvDraft(envOverride);
	const defaultEnvSummary = stringifyAgentDefaultEnvDraft(defaultEnv);
	const [cmdOpen, setCmdOpen] = (0, import_react.useState)(Boolean(cmdOverride) || argsOverride !== defaultArgs || envSummary !== defaultEnvSummary);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: cn("py-3", !isDetected && "opacity-70"),
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex flex-wrap items-start gap-3",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "flex size-7 shrink-0 items-center justify-center rounded-md border border-border/50 bg-background/50",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentIcon, {
						agent: agentId,
						size: 16
					})
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "min-w-0 flex-1 sm:min-w-[12rem]",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-center gap-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-sm font-medium leading-none",
							children: label
						}), !isEnabled && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsBadge, {
							tone: "muted",
							children: translate("auto.components.settings.AgentsPane.8dc0192e48", "Disabled")
						})]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "mt-1 truncate font-mono text-[11px] text-muted-foreground",
						children: [
							cmdOverride ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "text-muted-foreground/60 line-through",
								children: defaultCmd
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "ml-1.5 text-foreground/80",
								children: cmdOverride
							})] }) : defaultCmd,
							argsOverride && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "ml-1.5 text-foreground/70",
								children: argsOverride
							}),
							envSummary && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "ml-1.5 text-foreground/60",
								children: envSummary
							})
						]
					})]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "ml-auto grid shrink-0 grid-cols-[max-content_6.5rem_1.75rem_1.75rem] items-center gap-1.5",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentAvailabilityControl, {
							label,
							isEnabled,
							onSetEnabled
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "flex justify-start",
							children: isDetected && isEnabled && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
								type: "button",
								variant: isDefault ? "secondary" : "ghost",
								size: "xs",
								onClick: onSetDefault,
								title: isDefault ? translate("auto.components.settings.AgentsPane.d7625cf8b2", "Default agent") : translate("auto.components.settings.AgentsPane.5f986a9b92", "Set as default"),
								className: "h-7 w-full justify-center gap-1 text-xs",
								children: [isDefault && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, { className: "size-3" }), isDefault ? translate("auto.components.settings.AgentsPane.24e032fa34", "Default") : translate("auto.components.settings.AgentsPane.959b67385b", "Set default")]
							})
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("a", {
							href: homepageUrl,
							target: "_blank",
							rel: "noopener noreferrer",
							title: isDetected ? translate("auto.components.settings.AgentsPane.fe4d630c94", "Docs") : translate("auto.components.settings.AgentsPane.f95b5c79b8", "Install"),
							className: "flex size-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted/50 hover:text-foreground",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExternalLink, { className: "size-3.5" })
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "flex size-7 items-center justify-center",
							children: isDetected && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
								type: "button",
								variant: "ghost",
								size: "icon-sm",
								onClick: () => setCmdOpen((prev) => !prev),
								"aria-label": cmdOpen ? translate("auto.components.settings.AgentsPane.cea7d97be1", "Collapse command override") : translate("auto.components.settings.AgentsPane.dc4a2ffdc0", "Expand command override"),
								className: "size-7 text-muted-foreground hover:text-foreground",
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronDown, { className: cn("size-3.5 transition-transform", cmdOpen && "rotate-180") })
							})
						})
					]
				})
			]
		}), isDetected && cmdOpen && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "mt-3 pl-10",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentCommandOverrideInput, {
					defaultCmd,
					cmdOverride,
					onSaveOverride
				}, cmdOverride ?? defaultCmd),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "mt-2",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentDefaultArgsInput, {
						defaultArgs,
						argsOverride,
						onSaveArgs
					}, `${agentId}:${argsOverride}`)
				}),
				(defaultEnvSummary || envSummary) && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "mt-2",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentDefaultEnvInput, {
						defaultEnv,
						envOverride,
						onSaveEnv
					}, `${agentId}:${envSummary}`)
				}),
				sessionSourceHome && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "mt-2",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentSessionSourceHomeInput, {
						runtimeLabel: sessionSourceHome.runtimeLabel,
						value: sessionSourceHome.value,
						onSave: sessionSourceHome.onSave
					}, `${agentId}:${sessionSourceHome.runtimeLabel}:${sessionSourceHome.value}`)
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "mt-2 text-[11px] text-muted-foreground",
					children: translate("auto.components.settings.AgentsPane.f9f127d664", "Override the binary path or name, and edit the default launch arguments or environment for this agent.")
				})
			]
		})]
	});
}
function DefaultAgentPill({ active, onClick, children }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
		type: "button",
		onClick,
		"aria-pressed": active,
		className: cn("inline-flex items-center gap-2 rounded-md border px-3 py-1.5 text-sm outline-none transition-colors focus-visible:ring-[3px] focus-visible:ring-ring/50", active ? "border-muted-foreground/40 bg-accent font-medium text-accent-foreground" : "border-border bg-background/50 text-muted-foreground hover:border-muted-foreground/35 hover:text-foreground"),
		children
	});
}
function AgentsPane({ settings, updateSettings, wslSupportedPlatform, wslAvailable, wslDistros, wslCapabilitiesLoading }) {
	const activeServerEnvironmentId = settings.activeRuntimeEnvironmentId?.trim() || null;
	const { detectedIds: detectedList, detectionFailed, isRefreshing, refresh: refreshTargetAgents } = useDetectedAgents((0, import_react.useMemo)(() => activeServerEnvironmentId ? {
		kind: "runtime",
		environmentId: activeServerEnvironmentId
	} : { kind: "local" }, [activeServerEnvironmentId]));
	const refreshLocalAgents = useAppStore((s) => s.refreshDetectedAgents);
	const activeServerName = useAppStore((s) => activeServerEnvironmentId ? s.runtimeEnvironments.find((environment) => environment.id === activeServerEnvironmentId)?.name ?? null : null);
	const handleRefresh = () => {
		refreshTargetAgents();
	};
	const detectedIds = (0, import_react.useMemo)(() => detectedList ? new Set(detectedList) : null, [detectedList]);
	const defaultAgent = settings.defaultTuiAgent;
	const agentOwnership = getSettingOwnershipSummary("agentLaunchDefaults");
	const cmdOverrides = settings.agentCmdOverrides ?? {};
	const agentDefaultArgs = settings.agentDefaultArgs ?? {};
	const agentDefaultEnv = settings.agentDefaultEnv ?? {};
	const agentPermissionMode = resolveAgentPermissionModeSummary({
		agentDefaultArgs,
		agentDefaultEnv
	});
	const disabledAgents = normalizeDisabledTuiAgents(settings.disabledTuiAgents);
	const setDefault = (id) => {
		updateSettings({ defaultTuiAgent: id });
	};
	const setAgentEnabled = (id, enabled) => {
		enqueueAgentAvailabilityUpdate({
			getSettings: () => useAppStore.getState().settings,
			fallbackSettings: settings,
			updateSettings,
			agentId: id,
			enabled
		});
	};
	const saveOverride = (id, value) => {
		const next = { ...cmdOverrides };
		if (value) next[id] = value;
		else delete next[id];
		updateSettings({ agentCmdOverrides: next });
	};
	const saveAgentArgs = (id, value) => {
		updateSettings({ agentDefaultArgs: {
			...agentDefaultArgs,
			[id]: value
		} });
	};
	const saveAgentEnv = (id, value) => {
		updateSettings({ agentDefaultEnv: {
			...agentDefaultEnv,
			[id]: value
		} });
	};
	const saveAgentPermissionMode = (mode) => {
		updateSettings(applyAgentPermissionMode({
			mode,
			agentDefaultArgs,
			agentDefaultEnv
		}));
	};
	const detectedAgents = detectedIds === null ? [] : getAgentCatalog().filter((agent) => detectedIds.has(agent.id));
	const enabledDetectedAgents = detectedAgents.filter((agent) => isTuiAgentEnabled(agent.id, disabledAgents));
	const undetectedAgents = getAgentCatalog().filter((a) => detectedIds !== null && !detectedIds.has(a.id));
	const isAutoDefault = defaultAgent === null || defaultAgent !== "blank" && (!detectedIds?.has(defaultAgent) || !isTuiAgentEnabled(defaultAgent, disabledAgents));
	const isBlankDefault = defaultAgent === "blank";
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-8",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", {
				className: "space-y-4",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsSubsectionHeader, {
					title: translate("auto.components.settings.AgentsPane.385212c7a1", "Default Agent"),
					description: agentOwnership.description
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex flex-wrap gap-2",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DefaultAgentPill, {
							active: isAutoDefault,
							onClick: () => setDefault(null),
							children: [isAutoDefault && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, { className: "size-3.5" }), translate("auto.components.settings.AgentsPane.92033495ff", "Auto")]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DefaultAgentPill, {
							active: isBlankDefault,
							onClick: () => setDefault("blank"),
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Terminal, { className: "size-3.5" }),
								translate("auto.components.settings.AgentsPane.110b74b022", "No agent (blank terminal)"),
								isBlankDefault && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, { className: "size-3.5" })
							]
						}),
						enabledDetectedAgents.map((agent) => {
							const isActive = defaultAgent === agent.id;
							return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DefaultAgentPill, {
								active: isActive,
								onClick: () => setDefault(agent.id),
								children: [
									/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentIcon, {
										agent: agent.id,
										size: 14
									}),
									agent.label,
									isActive && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, { className: "size-3.5" })
								]
							}, agent.id);
						})
					]
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentRuntimeSetting, {
				settings,
				updateSettings,
				refresh: refreshLocalAgents,
				wslSupportedPlatform,
				wslAvailable,
				wslDistros,
				wslCapabilitiesLoading
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentStatusHooksSetting, {
				settings,
				updateSettings
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentGeneratedTabTitlesSetting, {
				settings,
				updateSettings
			}),
			!isPairedWebClientWindow() ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentAwakeSetting, {
				settings,
				updateSettings
			}) : null,
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentCacheTimerSection, {
				settings,
				updateSettings
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentPermissionsSetting, {
				mode: agentPermissionMode,
				onChange: saveAgentPermissionMode
			}),
			detectedAgents.length > 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", {
				className: "space-y-3",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsSubsectionHeader, {
					title: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
						className: "flex items-center gap-2",
						children: [
							translate("auto.components.settings.AgentsPane.02e0143be5", "Installed"),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(SettingsBadge, {
								tone: "accent",
								children: [
									detectedAgents.length,
									" ",
									translate("auto.components.settings.AgentsPane.ed3e110e61", "detected")
								]
							}),
							activeServerName ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsBadge, {
								tone: "muted",
								children: translate("auto.components.settings.AgentsPane.03e1a5081a", "on {{value0}}", { value0: activeServerName })
							}) : null
						]
					}),
					action: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
						type: "button",
						variant: "ghost",
						size: "xs",
						onClick: handleRefresh,
						disabled: isRefreshing,
						title: activeServerEnvironmentId ? translate("auto.components.settings.AgentsPane.25a41a9aad", "Re-detect agents installed on the active server") : translate("auto.components.settings.AgentsPane.13647f9f80", "Re-read your shell PATH and re-detect installed agents"),
						className: "h-7 gap-1.5 text-xs text-muted-foreground hover:text-foreground",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RefreshCw, { className: cn("size-3", isRefreshing && "animate-spin") }), isRefreshing ? translate("auto.components.settings.AgentsPane.c9b33eb5c0", "Refreshing…") : translate("auto.components.settings.AgentsPane.0d9e293a02", "Refresh")]
					})
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "divide-y divide-border/40",
					children: detectedAgents.map((agent) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentRow, {
						agentId: agent.id,
						label: agent.label,
						homepageUrl: agent.homepageUrl,
						defaultCmd: agent.cmd,
						defaultArgs: getTuiAgentDefaultArgs(agent.id),
						defaultEnv: getTuiAgentDefaultEnv(agent.id),
						isDetected: true,
						isEnabled: isTuiAgentEnabled(agent.id, disabledAgents),
						isDefault: defaultAgent === agent.id,
						cmdOverride: cmdOverrides[agent.id],
						argsOverride: resolveTuiAgentLaunchArgs(agent.id, agentDefaultArgs),
						envOverride: resolveTuiAgentLaunchEnv(agent.id, agentDefaultEnv),
						onSetDefault: () => setDefault(agent.id),
						onSetEnabled: (enabled) => setAgentEnabled(agent.id, enabled),
						onSaveOverride: (v) => saveOverride(agent.id, v),
						onSaveArgs: (v) => saveAgentArgs(agent.id, v),
						onSaveEnv: (v) => saveAgentEnv(agent.id, v),
						sessionSourceHome: agent.id === "codex" ? buildCodexSessionSourceHomeControl(settings, updateSettings) : void 0
					}, agent.id))
				})]
			}),
			undetectedAgents.length > 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", {
				className: "space-y-3",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsSubsectionHeader, { title: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
					className: "flex items-center gap-2 text-muted-foreground",
					children: [translate("auto.components.settings.AgentsPane.e8da2af684", "Available to install"), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(SettingsBadge, {
						tone: "muted",
						children: [
							undetectedAgents.length,
							" ",
							translate("auto.components.settings.AgentsPane.024bd95089", "agents")
						]
					})]
				}) }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "divide-y divide-border/40",
					children: undetectedAgents.map((agent) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentRow, {
						agentId: agent.id,
						label: agent.label,
						homepageUrl: agent.homepageUrl,
						defaultCmd: agent.cmd,
						defaultArgs: getTuiAgentDefaultArgs(agent.id),
						defaultEnv: getTuiAgentDefaultEnv(agent.id),
						isDetected: false,
						isEnabled: isTuiAgentEnabled(agent.id, disabledAgents),
						isDefault: false,
						cmdOverride: void 0,
						argsOverride: resolveTuiAgentLaunchArgs(agent.id, agentDefaultArgs),
						envOverride: resolveTuiAgentLaunchEnv(agent.id, agentDefaultEnv),
						onSetDefault: () => {},
						onSetEnabled: (enabled) => setAgentEnabled(agent.id, enabled),
						onSaveOverride: () => {},
						onSaveArgs: (v) => saveAgentArgs(agent.id, v),
						onSaveEnv: (v) => saveAgentEnv(agent.id, v)
					}, agent.id))
				})]
			}),
			detectedIds === null && !detectionFailed && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "flex items-center justify-center rounded-md border border-dashed border-border/50 py-6 text-sm text-muted-foreground",
				children: translate("auto.components.settings.AgentsPane.d83834f5e6", "Detecting installed agents…")
			}),
			detectionFailed && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-start justify-between gap-3 rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-xs text-destructive",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
					className: "flex min-w-0 items-start gap-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "mt-0.5 size-3.5 shrink-0" }), translate("auto.components.settings.AgentsPane.remoteDetectionFailed", "Couldn’t detect installed agents. Check the host connection and try again.")]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
					type: "button",
					variant: "ghost",
					size: "xs",
					onClick: handleRefresh,
					className: "h-6 shrink-0 gap-1.5 px-2 text-destructive hover:text-destructive",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RefreshCw, { className: "size-3" }), translate("auto.components.settings.AgentsPane.retryDetection", "Retry")]
				})]
			})
		]
	});
}
function AgentStatusHooksSetting({ settings, updateSettings }) {
	const enabled = settings.agentStatusHooksEnabled !== false;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("section", {
		className: "space-y-3",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsSwitchRow, {
			label: getAgentStatusHooksTitle(),
			description: getAgentStatusHooksDescription(),
			checked: enabled,
			onChange: () => updateSettings({ agentStatusHooksEnabled: !enabled }),
			ariaLabel: getAgentStatusHooksTitle()
		})
	});
}
function AgentGeneratedTabTitlesSetting({ settings, updateSettings }) {
	const enabled = settings.tabAutoGenerateTitle === true;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("section", {
		className: "space-y-3",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsSwitchRow, {
			label: getAgentGeneratedTabTitlesTitle(),
			description: getAgentGeneratedTabTitlesDescription(),
			checked: enabled,
			onChange: () => updateSettings({ tabAutoGenerateTitle: !enabled }),
			ariaLabel: getAgentGeneratedTabTitlesTitle()
		})
	});
}
function dispatchInputEvent(target, text) {
	const event = typeof InputEvent === "function" ? new InputEvent("input", {
		bubbles: true,
		cancelable: false,
		data: text,
		inputType: "insertFromPaste"
	}) : new Event("input", {
		bubbles: true,
		cancelable: false
	});
	target.dispatchEvent(event);
}
function setContentEditableCaretFromPoint(target, point) {
	const ownerDocument = target.ownerDocument;
	const selection = ownerDocument.getSelection();
	if (!selection) return;
	const caretPosition = ownerDocument.caretPositionFromPoint?.(point.clientX, point.clientY);
	const range = caretPosition ? ownerDocument.createRange() : ownerDocument.caretRangeFromPoint?.(point.clientX, point.clientY);
	if (caretPosition && range) {
		range.setStart(caretPosition.offsetNode, caretPosition.offset);
		range.collapse(true);
	}
	if (!range || !target.contains(range.startContainer)) return;
	selection.removeAllRanges();
	selection.addRange(range);
}
function insertTextIntoContentEditable(target, text) {
	const ownerDocument = target.ownerDocument;
	if (ownerDocument.queryCommandSupported?.("insertText") && ownerDocument.execCommand("insertText", false, text)) return true;
	const selection = ownerDocument.getSelection();
	if (!selection || selection.rangeCount === 0) return false;
	const range = selection.getRangeAt(0);
	range.deleteContents();
	const textNode = ownerDocument.createTextNode(text);
	range.insertNode(textNode);
	range.setStartAfter(textNode);
	range.collapse(true);
	selection.removeAllRanges();
	selection.addRange(range);
	dispatchInputEvent(target, text);
	return true;
}
function isContentEditablePasteTargetAvailable(target, canContinue) {
	return target.isConnected && target.isContentEditable && (canContinue?.(target) ?? true);
}
function getContentEditableInsertionRange(target) {
	const selection = target.ownerDocument.getSelection();
	if (!selection || selection.rangeCount === 0) return null;
	const range = selection.getRangeAt(0);
	if (!target.contains(range.startContainer) || !target.contains(range.endContainer)) return null;
	return range;
}
function insertContentEditableChunk(target, range, text) {
	range.deleteContents();
	const textNode = target.ownerDocument.createTextNode(text);
	range.insertNode(textNode);
	range.setStartAfter(textNode);
	range.collapse(true);
	const selection = target.ownerDocument.getSelection();
	selection?.removeAllRanges();
	selection?.addRange(range);
	return range;
}
async function pasteLargeTextIntoContentEditable(target, text, options) {
	const chunkMaxBytes = options.chunkMaxBytes ?? 16384;
	let range = getContentEditableInsertionRange(target);
	if (!range) return false;
	range.deleteContents();
	let textIndex = 0;
	while (textIndex < text.length) {
		if (!isContentEditablePasteTargetAvailable(target, options.canContinue)) {
			if (textIndex > 0) dispatchInputEvent(target, null);
			return false;
		}
		const nextIndex = getUtf8ChunkEndIndex(text, textIndex, chunkMaxBytes);
		range = insertContentEditableChunk(target, range, text.slice(textIndex, nextIndex));
		textIndex = nextIndex;
		if (textIndex < text.length) await (options.yieldToEventLoop ?? yieldToEventLoop)();
	}
	dispatchInputEvent(target, null);
	return true;
}
async function pasteIntoContentEditable(target, text, point, options) {
	const maxBytes = options.maxBytes ?? 16777216;
	const directMaxBytes = options.directMaxBytes ?? 65536;
	const directByteLengthMeasurement = measureTextControlPasteByteLength(text, { stopAfterBytes: Math.min(directMaxBytes, maxBytes) });
	const { byteLength } = directByteLengthMeasurement;
	if (byteLength === 0) return false;
	if (maxBytes <= directMaxBytes && directByteLengthMeasurement.exceededLimit) return false;
	const largeByteLengthMeasurement = directByteLengthMeasurement.exceededLimit ? await measureTextControlPasteByteLengthWithYield(text, {
		stopAfterBytes: maxBytes,
		yieldAfterCodeUnits: options.measureYieldAfterCodeUnits,
		yieldToEventLoop: options.yieldToEventLoop
	}) : directByteLengthMeasurement;
	if (largeByteLengthMeasurement.exceededLimit) return false;
	target.focus();
	setContentEditableCaretFromPoint(target, point);
	if (!isContentEditablePasteTargetAvailable(target, options.canContinue)) return false;
	if (largeByteLengthMeasurement.byteLength <= directMaxBytes) return insertTextIntoContentEditable(target, text);
	return pasteLargeTextIntoContentEditable(target, text, options);
}
function findEditablePrimarySelectionPasteTarget(target) {
	if (!(target instanceof Element)) return null;
	if (target.closest(".xterm-helper-textarea")) return null;
	const textControl = target.closest("input, textarea");
	if (textControl && isPrimarySelectionTextControl(textControl)) {
		if (textControl.disabled || textControl.readOnly) return null;
		return textControl;
	}
	let element = target instanceof HTMLElement ? target : target.parentElement;
	while (element) {
		if (element.getAttribute("contenteditable") === "false") return null;
		if (element.isContentEditable) return element;
		element = element.parentElement;
	}
	return null;
}
async function pastePrimarySelectionTextIntoTarget(target, text, point, options = {}) {
	if (isPrimarySelectionTextControl(target)) {
		const targetStillFocused = (candidate) => candidate.ownerDocument.activeElement === candidate && (options.canContinue?.(candidate) ?? true);
		return (await pasteTextIntoTextControl(target, text, {
			source: "primary-selection",
			directMaxBytes: options.directMaxBytes,
			chunkMaxBytes: options.chunkMaxBytes,
			maxBytes: options.maxBytes,
			measureYieldAfterCodeUnits: options.measureYieldAfterCodeUnits,
			yieldToEventLoop: options.yieldToEventLoop,
			canContinue: targetStillFocused
		})).status === "pasted";
	}
	return pasteIntoContentEditable(target, text, point, {
		...options,
		canContinue: (candidate) => candidate.ownerDocument.activeElement === candidate && (options.canContinue?.(candidate) ?? true)
	});
}
var PRIMARY_SELECTION_PENDING_TARGET_TTL_MS = 750;
function resolvePrimarySelectionMiddleClickPaste(setting, userAgent = typeof navigator === "undefined" ? "" : navigator.userAgent) {
	return setting ?? isDefaultPrimarySelectionMiddleClickPasteUserAgent(userAgent);
}
function isDefaultPrimarySelectionMiddleClickPasteUserAgent(userAgent = typeof navigator === "undefined" ? "" : navigator.userAgent) {
	return isLinuxUserAgent(userAgent) || isMacUserAgent(userAgent);
}
function captureCurrentSelection() {
	const text = readCurrentPrimarySelectionText();
	if (text) setPrimarySelectionText(text);
}
function suppressEvent(event) {
	event.preventDefault();
	event.stopPropagation();
	event.stopImmediatePropagation();
}
function isTerminalNativePasteTarget(target) {
	if (!(target instanceof Element)) return false;
	return target.classList.contains("xterm-helper-textarea") || target.closest(".xterm") !== null;
}
function isPrimarySelectionPasteTargetCurrent(target) {
	const activeElement = target.ownerDocument.activeElement;
	return target.isConnected && activeElement instanceof Node && (activeElement === target || target.contains(activeElement));
}
function usePrimarySelectionPaste(enabled) {
	(0, import_react.useEffect)(() => {
		setPrimarySelectionEnabled(enabled);
		let pendingMiddleTarget = null;
		let pendingMiddleUntil = 0;
		const targetMatchesPending = (target) => {
			if (!pendingMiddleTarget || !(target instanceof Node)) return false;
			return target === pendingMiddleTarget || pendingMiddleTarget.contains(target);
		};
		const rememberPendingTarget = (event) => {
			if (event.button !== 1) return false;
			const target = findEditablePrimarySelectionPasteTarget(event.target);
			if (!target) return false;
			pendingMiddleTarget = target;
			pendingMiddleUntil = Date.now() + PRIMARY_SELECTION_PENDING_TARGET_TTL_MS;
			return true;
		};
		const suppressPendingPasteInput = (event) => {
			if (!(typeof InputEvent !== "function" || !(event instanceof InputEvent) || event.inputType === "insertFromPaste")) return;
			if (pendingMiddleTarget && Date.now() <= pendingMiddleUntil && targetMatchesPending(event.target)) {
				suppressEvent(event);
				return;
			}
			if (isTerminalNativePasteTarget(event.target) && consumePrimarySelectionNativePasteSuppression()) suppressEvent(event);
		};
		if (!enabled) {
			if (!isLinuxUserAgent()) return;
			const onMouseDown$1 = (event) => {
				rememberPendingTarget(event);
			};
			const onMouseUp$1 = (event) => {
				if (event.button === 1) event.preventDefault();
				pendingMiddleTarget = null;
			};
			const onAuxClick$1 = (event) => {
				if (event.button === 1) event.preventDefault();
			};
			document.addEventListener("mousedown", onMouseDown$1, true);
			document.addEventListener("beforeinput", suppressPendingPasteInput, true);
			document.addEventListener("paste", suppressPendingPasteInput, true);
			document.addEventListener("mouseup", onMouseUp$1, true);
			document.addEventListener("auxclick", onAuxClick$1, true);
			return () => {
				setPrimarySelectionEnabled(false);
				document.removeEventListener("mousedown", onMouseDown$1, true);
				document.removeEventListener("beforeinput", suppressPendingPasteInput, true);
				document.removeEventListener("paste", suppressPendingPasteInput, true);
				document.removeEventListener("mouseup", onMouseUp$1, true);
				document.removeEventListener("auxclick", onAuxClick$1, true);
			};
		}
		let captureTimer = null;
		const scheduleCapture = () => {
			if (captureTimer !== null) window.clearTimeout(captureTimer);
			captureTimer = window.setTimeout(() => {
				captureTimer = null;
				captureCurrentSelection();
			}, 100);
		};
		const onMouseDown = (event) => {
			rememberPendingTarget(event);
		};
		const onMouseUp = (event) => {
			if (event.button !== 1 || !pendingMiddleTarget || Date.now() > pendingMiddleUntil) {
				pendingMiddleTarget = null;
				return;
			}
			const target = pendingMiddleTarget;
			pendingMiddleTarget = null;
			suppressEvent(event);
			const point = {
				clientX: event.clientX,
				clientY: event.clientY
			};
			readPrimarySelectionText().then((text) => {
				if (!text || !isPrimarySelectionPasteTargetCurrent(target)) return;
				pastePrimarySelectionTextIntoTarget(target, text, point).catch(() => {});
			});
		};
		const onAuxClick = (event) => {
			if (event.button !== 1) return;
			if (!findEditablePrimarySelectionPasteTarget(event.target)) return;
			suppressEvent(event);
		};
		document.addEventListener("selectionchange", scheduleCapture);
		document.addEventListener("mouseup", scheduleCapture, true);
		document.addEventListener("keyup", scheduleCapture, true);
		document.addEventListener("mousedown", onMouseDown, true);
		document.addEventListener("beforeinput", suppressPendingPasteInput, true);
		document.addEventListener("paste", suppressPendingPasteInput, true);
		document.addEventListener("mouseup", onMouseUp, true);
		document.addEventListener("auxclick", onAuxClick, true);
		return () => {
			setPrimarySelectionEnabled(false);
			if (captureTimer !== null) window.clearTimeout(captureTimer);
			document.removeEventListener("selectionchange", scheduleCapture);
			document.removeEventListener("mouseup", scheduleCapture, true);
			document.removeEventListener("keyup", scheduleCapture, true);
			document.removeEventListener("mousedown", onMouseDown, true);
			document.removeEventListener("beforeinput", suppressPendingPasteInput, true);
			document.removeEventListener("paste", suppressPendingPasteInput, true);
			document.removeEventListener("mouseup", onMouseUp, true);
			document.removeEventListener("auxclick", onAuxClick, true);
		};
	}, [enabled]);
}
function pluginCommandKeybindingActionId$1(command) {
	return pluginCommandKeybindingActionId(command.pluginKey, command.id);
}
function pluginCommandKeybindingDefinition(command) {
	const defaults = command.keybindings.map((keybinding) => keybinding.key);
	return {
		id: pluginCommandKeybindingActionId$1(command),
		title: `${command.title} — ${command.pluginName}`,
		group: translate("auto.lib.pluginCommandKeybindings.group", "Plugins"),
		scope: "global",
		searchKeywords: [
			"plugin",
			"shortcut",
			command.title.toLowerCase(),
			command.pluginName.toLowerCase()
		],
		defaultBindings: {
			darwin: defaults,
			linux: defaults,
			win32: defaults
		}
	};
}
function buildPluginCommandKeybindingDefinitions(commands) {
	return commands.map(pluginCommandKeybindingDefinition);
}
function getEffectivePluginCommandKeybindings(command, platform, overrides) {
	return getEffectiveKeybindingsForDefinition(pluginCommandKeybindingDefinition(command), platform, overrides);
}
function findPluginCommandForKeybinding(commands, input, platform, overrides, hasActiveWorktree) {
	for (const command of commands) {
		if (command.context === "worktree" && !hasActiveWorktree) continue;
		if (getEffectivePluginCommandKeybindings(command, platform, overrides).some((binding) => keybindingMatchesInput(binding, input, platform))) return command;
	}
	return null;
}
export { PanelLeft as A, MAX_AGENT_HIBERNATION_IDLE_MS as C, resolveLeftSidebarStyleVariables as D, planAgentHibernationCandidates as E, applyUIZoom as O, getRepositoryHookScriptTextareaRows as S, getEffectiveAgentHibernationIdleMs as T, applyHostRename as _, usePrimarySelectionPaste as a, RepoForkIndicator as b, SearchableSetting as c, sparseDirectoriesMatch as d, getUpdateCheckClickOptions as f, removeSshTargetWithBestEffortCleanup as g, resolveSshHostRemoval as h, resolvePrimarySelectionMiddleClickPaste as i, BookOpen as j, syncZoomCSSVar as k, parseSparsePresetDirectories as l, HostRemoveDialog as m, findPluginCommandForKeybinding as n, AgentsPane as o, getUpdateCheckHint as p, isDefaultPrimarySelectionMiddleClickPasteUserAgent as r, getSettingOwnershipSummary as s, buildPluginCommandKeybindingDefinitions as t, normalizeSparseDirectoryLines as u, getHostDisplayLabelOverride as v, MIN_AGENT_HIBERNATION_IDLE_MS as w, getDetectedSetupScriptTextareaRows as x, resolveHostRemoval as y };
