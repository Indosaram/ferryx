import { a as translate } from "./jsx-runtime-Cv_nyRjc.js";
import { Bp as resolveRuntimePath, Hp as parseWslUncPath, Pm as FLOATING_TERMINAL_WORKTREE_ID, Ro as parseRemoteRuntimePtyId, _a as hasCachedWindowsTerminalCapabilities, da as getLocalProjectExecutionRuntimeContext, dc as resolveLocalWindowsTerminalRuntimeOptions, dd as parseAppSshPtyId, ha as getCachedWindowsTerminalCapabilities, op as getActiveRuntimeTarget, t as useAppStore, uc as isWslShellName, vd as parseWorkspaceKey, xa as getRendererAppPlatform } from "./store-CgXrfmaH.js";
import { y as isShellProcess } from "./agent-status-3vUKbY6l.js";
import { i as recognizeAgentProcess, o as confirmRuntimeTerminalForegroundProcess, s as inspectRuntimeTerminalProcess, t as isAgentForegroundWrapperProcess } from "./agent-process-recognition-BB0O3DaN.js";
const TOGGLE_FLOATING_TERMINAL_EVENT = "orca-toggle-floating-terminal";
var openMaximizedIntentAt = null;
var OPEN_MAXIMIZED_INTENT_TTL_MS = 2e3;
function requestFloatingTerminalOpenMaximized() {
	openMaximizedIntentAt = Date.now();
}
function consumeFloatingTerminalOpenMaximizedIntent() {
	if (openMaximizedIntentAt === null) return false;
	const requestedAt = openMaximizedIntentAt;
	openMaximizedIntentAt = null;
	return Date.now() - requestedAt <= OPEN_MAXIMIZED_INTENT_TTL_MS;
}
function normalizeProcessName(processName) {
	if (!processName) return null;
	return processName.toLowerCase().replace(/\.exe$/, "");
}
function isCodexForegroundProcess(processName) {
	const normalized = normalizeProcessName(processName);
	if (!normalized) return false;
	return normalized === "codex" || normalized.startsWith("codex-");
}
function isCodexRestartEligiblePane(args) {
	const { foregroundProcess, hasChildProcesses, unavailable } = args.inspection;
	if (unavailable === true) return false;
	if (isCodexForegroundProcess(foregroundProcess)) return true;
	if (args.launchAgent !== "codex" || foregroundProcess === null || !hasChildProcesses) return false;
	if (isShellProcess(foregroundProcess)) return false;
	return recognizeAgentProcess(foregroundProcess) !== null || isAgentForegroundWrapperProcess(foregroundProcess);
}
function normalizeCodexAccountSelectionTarget(target) {
	if (target?.runtime === "wsl") return {
		runtime: "wsl",
		wslDistro: normalizeWslDistro(target.wslDistro)
	};
	return {
		runtime: "host",
		wslDistro: null
	};
}
function getCodexSelectionLaneKey(target) {
	const normalized = normalizeCodexAccountSelectionTarget(target);
	return normalized.runtime === "host" ? "host" : `wsl:${getWslSelectionKey(normalized.wslDistro)}`;
}
function getWslSelectionKey(wslDistro) {
	return normalizeWslDistro(wslDistro) ?? "__default__";
}
function normalizeWslDistro(wslDistro) {
	const trimmed = wslDistro?.trim();
	return trimmed ? trimmed : null;
}
function resolveTerminalStartupCwd(worktreePath, requestedCwd, missingDirFallback) {
	const trimmedCwd = requestedCwd?.trim();
	if (!trimmedCwd) return;
	const resolvedCwd = resolveRuntimePath(worktreePath, trimmedCwd);
	if (missingDirFallback && resolvedCwd !== worktreePath && !missingDirFallback.directoryExists(resolvedCwd) && missingDirFallback.directoryExists(worktreePath)) {
		missingDirFallback.onFallbackToWorkspaceRoot?.(resolvedCwd);
		return worktreePath;
	}
	return resolvedCwd;
}
var RUNTIME_ENVIRONMENT_LANE_PREFIX = "env:";
var SSH_CONNECTION_LANE_KEY = "ssh-connection";
var UNATTRIBUTED_REMOTE_LANE_KEY = "remote-runtime";
var HOST_LANE_KEY = "host";
var WSL_LANE_PREFIX = "wsl:";
function isLocalCodexSelectionLaneKey(laneKey) {
	return laneKey === HOST_LANE_KEY || laneKey.startsWith(WSL_LANE_PREFIX);
}
function isForeignMachineCodexPtyId(ptyId) {
	return parseRemoteRuntimePtyId(ptyId) !== null || parseAppSshPtyId(ptyId) !== null;
}
function getCodexAccountSwitchLaneMatcher(args) {
	const runtimeTarget = getActiveRuntimeTarget(args.settings);
	if (runtimeTarget.kind === "environment") {
		const environmentLaneKey = `${RUNTIME_ENVIRONMENT_LANE_PREFIX}${runtimeTarget.environmentId}`;
		return (laneKey) => laneKey === environmentLaneKey;
	}
	const normalized = normalizeCodexAccountSelectionTarget(args.target);
	if (args.clearsEveryWslDistro && normalized.runtime === "wsl" && normalized.wslDistro === null) return (laneKey) => laneKey.startsWith(WSL_LANE_PREFIX);
	const switchLaneKey = getCodexSelectionLaneKey(normalized);
	return (laneKey) => laneKey === switchLaneKey;
}
function resolveCodexPaneSelectionLane(args) {
	const recorded = args.recordedLaneKey?.trim();
	if (!(Boolean(recorded) && isLocalCodexSelectionLaneKey(recorded) && !isForeignMachineCodexPtyId(args.ptyId))) {
		const laneKey = resolveCodexPaneSelectionLaneKey(args);
		return {
			laneKey,
			source: "derived",
			derivedLaneKey: laneKey
		};
	}
	const derivedLaneKey = deriveLaneKeyForDiagnostics(args);
	if (derivedLaneKey !== null && derivedLaneKey !== recorded) console.warn("[codex-lane] recorded launch lane disagrees with the derived one:", {
		ptyId: args.ptyId,
		recorded,
		derived: derivedLaneKey
	});
	return {
		laneKey: recorded,
		source: "recorded",
		derivedLaneKey
	};
}
function deriveLaneKeyForDiagnostics(args) {
	try {
		return resolveCodexPaneSelectionLaneKey(args);
	} catch {
		return null;
	}
}
function resolveCodexPaneSelectionLaneKey(args) {
	const remoteParts = parseRemoteRuntimePtyId(args.ptyId);
	if (remoteParts !== null) {
		const runtimeTarget = getActiveRuntimeTarget(args.state.settings);
		const environmentId = remoteParts.environmentId?.trim() || (runtimeTarget.kind === "environment" ? runtimeTarget.environmentId : null);
		return environmentId ? `${RUNTIME_ENVIRONMENT_LANE_PREFIX}${environmentId}` : UNATTRIBUTED_REMOTE_LANE_KEY;
	}
	if (parseAppSshPtyId(args.ptyId) !== null) return SSH_CONNECTION_LANE_KEY;
	return getCodexSelectionLaneKey(resolveLocalPaneSelectionTarget(args));
}
function resolveLocalPaneSelectionTarget(args) {
	const paneCwd = resolvePaneCwd(args);
	const wslPath = paneCwd ? parseWslUncPath(paneCwd) : null;
	if (wslPath) return {
		runtime: "wsl",
		wslDistro: wslPath.distro
	};
	const terminalRuntime = resolveLocalPaneTerminalRuntime(args);
	if (isWslShellName(terminalRuntime.shellOverride)) return {
		runtime: "wsl",
		wslDistro: terminalRuntime.terminalWindowsWslDistro
	};
	return { runtime: "host" };
}
function resolvePaneCwd(args) {
	if (args.tab.worktreeId === "global-floating-terminal") return null;
	const workspacePath = getWorkspacePath(args.state, args.tab.worktreeId);
	if (!workspacePath) return null;
	return resolveTerminalStartupCwd(workspacePath, args.tab.startupCwd) ?? workspacePath;
}
function resolveLocalPaneTerminalRuntime(args) {
	if (getRendererAppPlatform() !== "win32") return {
		shellOverride: args.tab.shellOverride,
		terminalWindowsWslDistro: null
	};
	const capabilities = hasCachedWindowsTerminalCapabilities() ? getCachedWindowsTerminalCapabilities() : null;
	const projectRuntime = getLocalProjectExecutionRuntimeContext(args.state, args.tab.worktreeId, void 0, {
		wslAvailable: capabilities?.wslAvailable,
		availableWslDistros: capabilities?.wslDistros ?? null
	});
	if (projectRuntime?.status === "repair-required") return {
		shellOverride: "wsl.exe",
		terminalWindowsWslDistro: projectRuntime.repair.preferredRuntime.distro
	};
	return resolveLocalWindowsTerminalRuntimeOptions({
		requestedShellOverride: args.tab.shellOverride,
		settings: args.state.settings ?? void 0,
		projectRuntime
	});
}
function getWorkspacePath(state, worktreeId) {
	const parsed = parseWorkspaceKey(worktreeId);
	if (parsed?.type === "folder") return (state.folderWorkspaces ?? []).find((workspace) => workspace.id === parsed.folderWorkspaceId)?.folderPath ?? null;
	return Object.values(state.worktreesByRepo ?? {}).flat().find((entry) => entry.id === worktreeId)?.path ?? null;
}
const CODEX_ACCOUNT_RESTART_STARTUP = {
	command: "codex",
	startupCommandDelivery: "shell-ready",
	launchAgent: "codex"
};
async function readRecordedCodexPaneLanes(ptyIds) {
	const localPtyIds = ptyIds.filter((ptyId) => !isForeignMachineCodexPtyId(ptyId));
	if (localPtyIds.length === 0) return {};
	const listRecordedPaneLanes = window.api.codexAccounts.listRecordedPaneLanes;
	if (typeof listRecordedPaneLanes !== "function") return {};
	return await listRecordedPaneLanes({ ptyIds: localPtyIds }).catch(() => ({}));
}
async function isConfirmedCodexForegroundDespiteShellReading(state, ptyId, launchAgent, inspection) {
	if (launchAgent !== "codex" || inspection.unavailable === true || inspection.foregroundProcess === null || !isShellProcess(inspection.foregroundProcess)) return false;
	return isCodexForegroundProcess(await confirmRuntimeTerminalForegroundProcess(state.settings, ptyId));
}
async function scanCodexPanes(state, args) {
	const panes = Object.values(state.tabsByWorktree).flat().flatMap((tab) => (state.ptyIdsByTabId[tab.id] ?? []).filter((ptyId) => args.ptyIdFilter === null || args.ptyIdFilter.has(ptyId)).map((ptyId) => ({
		tab,
		ptyId
	})));
	const recordedLanes = await readRecordedCodexPaneLanes(panes.map((pane) => pane.ptyId));
	return Promise.all(panes.map(async ({ tab, ptyId }) => {
		const lane = resolveCodexPaneSelectionLane({
			state,
			tab,
			ptyId,
			recordedLaneKey: recordedLanes[ptyId]
		});
		if (!args.isLaneInScope(lane.laneKey)) return {
			ptyId,
			eligible: false,
			inconclusive: false,
			launchedCodex: false,
			notified: false,
			laneKey: lane.laneKey,
			laneSource: lane.source
		};
		const inspection = await inspectRuntimeTerminalProcess(state.settings, ptyId).then((result) => result, () => null);
		return {
			ptyId,
			eligible: inspection !== null && (isCodexRestartEligiblePane({
				inspection,
				launchAgent: tab.launchAgent
			}) || await isConfirmedCodexForegroundDespiteShellReading(state, ptyId, tab.launchAgent, inspection)),
			inconclusive: inspection === null || inspection.unavailable === true,
			launchedCodex: tab.launchAgent === "codex",
			notified: false,
			laneKey: lane.laneKey,
			laneSource: lane.source
		};
	}));
}
async function markLiveCodexSessionsForRestart(args) {
	const state = useAppStore.getState();
	const liveCodexSessionPtyIds = (await scanCodexPanes(state, {
		ptyIdFilter: null,
		isLaneInScope: getCodexAccountSwitchLaneMatcher({
			settings: state.settings,
			target: args.target,
			clearsEveryWslDistro: args.clearsEveryWslDistro
		})
	})).filter((scan) => scan.eligible).map((scan) => scan.ptyId);
	if (liveCodexSessionPtyIds.length === 0) return;
	const currentState = useAppStore.getState();
	const restoredRouteNoticePtyIds = liveCodexSessionPtyIds.filter((ptyId) => currentState.codexRestartNoticeByPtyId[ptyId]?.homeRouteChanged === true);
	const restoredRouteNoticePtyIdSet = new Set(restoredRouteNoticePtyIds);
	const authoritativeStalePanes = restoredRouteNoticePtyIds.length === 0 ? null : await window.api.codexAccounts.listStalePanes({ ptyIds: restoredRouteNoticePtyIds }).catch(() => null);
	const authoritativeStaleByPtyId = authoritativeStalePanes ? new Map(authoritativeStalePanes.map((pane) => [pane.ptyId, pane])) : null;
	if (authoritativeStaleByPtyId) {
		for (const ptyId of restoredRouteNoticePtyIds) if (!authoritativeStaleByPtyId.has(ptyId)) useAppStore.getState().clearCodexRestartNotice(ptyId);
	}
	useAppStore.getState().markCodexRestartNotices(liveCodexSessionPtyIds.flatMap((ptyId) => {
		if (authoritativeStaleByPtyId && restoredRouteNoticePtyIdSet.has(ptyId)) {
			const stalePane = authoritativeStaleByPtyId.get(ptyId);
			if (!stalePane) return [];
			return [{
				ptyId,
				previousAccountLabel: args.previousAccountLabel,
				nextAccountLabel: args.nextAccountLabel,
				previousAccountId: stalePane.launchAccountId,
				nextAccountId: stalePane.activeAccountId,
				homeRouteChanged: stalePane.reason === "home-route-change"
			}];
		}
		return [{
			ptyId,
			previousAccountLabel: args.previousAccountLabel,
			nextAccountLabel: args.nextAccountLabel,
			...args.previousAccountId === void 0 ? {} : { previousAccountId: args.previousAccountId },
			...args.nextAccountId === void 0 ? {} : { nextAccountId: args.nextAccountId }
		}];
	}));
}
async function markRestoredStaleCodexSessionsForRestart(args) {
	const scans = await scanCodexPanes(useAppStore.getState(), {
		ptyIdFilter: args?.ptyIds ? new Set(args.ptyIds) : null,
		isLaneInScope: isLocalCodexSelectionLaneKey
	});
	const liveCodexSessionPtyIds = scans.filter((scan) => scan.eligible).map((scan) => scan.ptyId);
	if (liveCodexSessionPtyIds.length === 0) return scans;
	const stalePanes = await window.api.codexAccounts.listStalePanes({ ptyIds: liveCodexSessionPtyIds });
	if (stalePanes.length === 0) return scans;
	const resolveAccountLabel = await createCodexAccountLabelResolver();
	const noticedPtyIds = useAppStore.getState().markCodexRestartNotices(stalePanes.map((pane) => ({
		ptyId: pane.ptyId,
		previousAccountLabel: resolveAccountLabel(pane.launchAccountId),
		nextAccountLabel: resolveAccountLabel(pane.activeAccountId),
		previousAccountId: pane.launchAccountId,
		nextAccountId: pane.activeAccountId,
		...pane.reason === "home-route-change" ? { homeRouteChanged: true } : {}
	})));
	const notifiedPtyIds = new Set(noticedPtyIds);
	return scans.map((scan) => notifiedPtyIds.has(scan.ptyId) ? {
		...scan,
		notified: true
	} : scan);
}
function resolveCodexRestartPromptAccountLabel(accounts, accountId) {
	if (accountId == null) return translate("auto.lib.codex.session.restart.4bd4a3a9c7", "System default");
	const account = accounts.find((entry) => entry.id === accountId);
	if (!account) return translate("auto.lib.codex.session.restart.9f0b1c2d3e", "Codex account");
	return accounts.some((entry) => entry.id !== account.id && entry.email === account.email) && account.workspaceLabel ? `${account.email} (${account.workspaceLabel})` : account.email;
}
async function createCodexAccountLabelResolver() {
	const accounts = await window.api.codexAccounts.list().catch(() => null);
	return (accountId) => resolveCodexRestartPromptAccountLabel(accounts?.accounts ?? [], accountId);
}
export { isForeignMachineCodexPtyId as a, requestFloatingTerminalOpenMaximized as c, resolveCodexRestartPromptAccountLabel as i, markLiveCodexSessionsForRestart as n, TOGGLE_FLOATING_TERMINAL_EVENT as o, markRestoredStaleCodexSessionsForRestart as r, consumeFloatingTerminalOpenMaximizedIntent as s, CODEX_ACCOUNT_RESTART_STARTUP as t };
