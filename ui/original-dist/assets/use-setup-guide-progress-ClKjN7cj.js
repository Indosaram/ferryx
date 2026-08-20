import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { Ei as hasFeatureInteraction, Md as hasEffectiveSetupCommand, ma as localPreflightContextKey, op as getActiveRuntimeTarget, qg as isGitRepoKind, rd as getProviderRuntimeContextKey, t as useAppStore, ua as getLocalPreflightContext, yu as checkRuntimeHooks } from "./store-CgXrfmaH.js";
import { t as FEATURE_WALL_SETUP_STEPS } from "./feature-wall-setup-steps-D7ga1-7b.js";
import { E as ORCHESTRATION_SKILL_NAME, b as ORCA_CLI_SKILL_NAME, u as COMPUTER_USE_SKILL_NAME } from "./use-active-skill-discovery-runtime-target-CdctmJyj.js";
import { i as useInstalledAgentSkill, t as GLOBAL_AGENT_SKILL_SOURCE_KINDS } from "./useInstalledAgentSkills-BYdWqfUf.js";
import { t as useActiveProjectSkillRuntime } from "./useActiveProjectSkillRuntime-CZKRPBhf.js";
import { t as deriveIntegrationConnectionStatus } from "./use-integration-connection-status-B2txbF2j.js";
function countAvailableNonMainWorktrees(worktreesByRepo) {
	return Object.values(worktreesByRepo).reduce((sum, worktrees) => sum + worktrees.filter((worktree) => !worktree.isMainWorktree && typeof worktree.path === "string" && worktree.path).length, 0);
}
function getFeatureWallSetupProgress(input) {
	const agentCapabilitiesDone = input.browserUseSkillInstalled && input.computerUseSkillInstalled && (input.computerUsePermissionsReady || input.computerUseUnavailable === true) && input.orchestrationSkillInstalled;
	const stepDone = {
		"default-agent": Boolean(input.settings?.defaultTuiAgent) && input.settings?.defaultTuiAgent !== "blank",
		"add-two-repos": input.gitRepoCount >= 2,
		notifications: input.settings?.notifications.enabled === true && input.settings.notifications.agentTaskComplete === true,
		"two-worktrees": countAvailableNonMainWorktrees(input.worktreesByRepo) >= 1,
		browser: hasFeatureInteraction(input.featureInteractions, "browser"),
		"task-sources": input.hasConnectedTaskSource,
		"agent-capabilities": agentCapabilitiesDone,
		"setup-script": input.hasSetupScript
	};
	return {
		ready: input.ready ?? true,
		stepDone,
		coreDoneCount: FEATURE_WALL_SETUP_STEPS.filter((step) => stepDone[step.id]).length,
		coreTotal: FEATURE_WALL_SETUP_STEPS.length
	};
}
var import_react = /* @__PURE__ */ __toESM(require_react());
function useSetupGuideBrowserMilestoneProgress(rawProgress, historicalSplitTerminalDone) {
	const setupGuideSidebarDismissed = useAppStore((s) => s.setupGuideSidebarDismissed);
	const browserMilestoneMigrated = useAppStore((s) => s.setupGuideBrowserMilestoneMigrated);
	const browserMilestoneLegacyComplete = useAppStore((s) => s.setupGuideBrowserMilestoneLegacyComplete);
	const markBrowserMilestoneMigrated = useAppStore((s) => s.markSetupGuideBrowserMilestoneMigrated);
	const pendingLegacyComplete = !browserMilestoneMigrated && rawProgress.ready ? shouldMarkBrowserMilestoneLegacyComplete({
		stepDone: rawProgress.stepDone,
		historicalSplitTerminalDone,
		setupGuideSidebarDismissed
	}) : false;
	const effectiveLegacyComplete = browserMilestoneLegacyComplete || pendingLegacyComplete;
	(0, import_react.useEffect)(() => {
		if (browserMilestoneMigrated || !rawProgress.ready) return;
		markBrowserMilestoneMigrated(pendingLegacyComplete);
	}, [
		browserMilestoneMigrated,
		markBrowserMilestoneMigrated,
		pendingLegacyComplete,
		rawProgress.ready
	]);
	return (0, import_react.useMemo)(() => getSetupGuideBrowserMilestoneAwareProgress(rawProgress, effectiveLegacyComplete), [effectiveLegacyComplete, rawProgress]);
}
function shouldMarkBrowserMilestoneLegacyComplete(input) {
	if (input.setupGuideSidebarDismissed) return true;
	return input.historicalSplitTerminalDone && FEATURE_WALL_SETUP_STEPS.every((step) => step.id === "browser" || input.stepDone[step.id]);
}
function getSetupGuideBrowserMilestoneAwareProgress(progress, browserMilestoneLegacyComplete) {
	if (!browserMilestoneLegacyComplete) return progress;
	const stepDone = Object.fromEntries(FEATURE_WALL_SETUP_STEPS.map((step) => [step.id, true]));
	return {
		...progress,
		stepDone,
		coreDoneCount: FEATURE_WALL_SETUP_STEPS.length,
		coreTotal: FEATURE_WALL_SETUP_STEPS.length
	};
}
const INITIAL_SETUP_SCRIPT_PROBE_STATE = {
	signature: null,
	ready: false,
	hasSetupScript: false
};
function getSetupScriptProbeSignature(settings, orderedGitRepos) {
	if (!settings) return null;
	const target = getActiveRuntimeTarget(settings);
	return JSON.stringify({
		runtime: target.kind === "environment" ? target.environmentId : "local",
		repos: orderedGitRepos.map((repo) => ({
			id: repo.id,
			commandSourcePolicy: repo.hookSettings?.commandSourcePolicy ?? null,
			setup: repo.hookSettings?.scripts?.setup ?? null
		}))
	});
}
function getCurrentSetupScriptProbeState(current, signature) {
	if (current.signature === signature) return current;
	return {
		signature,
		ready: false,
		hasSetupScript: false
	};
}
function getSetupGuideProgressReady(input) {
	return input.refreshEnabled && input.settingsLoaded && input.preflightStatusChecked && input.linearStatusChecked && input.jiraStatusChecked && !input.browserUseSkillDiscoveryLoading && !input.computerUseSkillDiscoveryLoading && !input.orchestrationSkillDiscoveryLoading && input.setupScriptProbeReady && (!input.computerUseSkillInstalled || input.computerUsePermissionStatusChecked);
}
function getComputerUsePermissionSetupState(status) {
	return {
		ready: status !== null && status.helperUnavailableReason === null && status.permissions.every((permission) => permission.status !== "not-granted"),
		unavailable: status !== null && status.helperUnavailableReason !== null
	};
}
var setupScriptProbeCacheListeners = /* @__PURE__ */ new Set();
var setupScriptProbeCache = INITIAL_SETUP_SCRIPT_PROBE_STATE;
function readSetupScriptProbeCache() {
	return setupScriptProbeCache;
}
function subscribeSetupScriptProbeCache(listener) {
	setupScriptProbeCacheListeners.add(listener);
	return () => {
		setupScriptProbeCacheListeners.delete(listener);
	};
}
function setSetupScriptProbeCache(next) {
	if (setupScriptProbeCache.signature === next.signature && setupScriptProbeCache.ready === next.ready && setupScriptProbeCache.hasSetupScript === next.hasSetupScript) return;
	setupScriptProbeCache = next;
	for (const listener of setupScriptProbeCacheListeners) listener();
}
var SETUP_SCRIPT_PROBE_SETTLE_TIMEOUT_MS = 15e3;
function useSetupGuideProgress(shouldRefreshCoreState, orchestrationSkillInstalled, browserUseSkillInstalled) {
	const settings = useAppStore((s) => s.settings);
	const featureInteractions = useAppStore((s) => s.featureInteractions);
	const worktreesByRepo = useAppStore((s) => s.worktreesByRepo);
	const preflightStatus = useAppStore((s) => s.preflightStatus);
	const preflightStatusChecked = useAppStore((s) => s.preflightStatusChecked);
	const preflightStatusContextKey = useAppStore((s) => s.preflightStatusContextKey);
	const preflightStatusError = useAppStore((s) => s.preflightStatusError);
	const preflightStatusLoading = useAppStore((s) => s.preflightStatusLoading);
	const refreshPreflightStatus = useAppStore((s) => s.refreshPreflightStatus);
	const activeSkillRuntime = useActiveProjectSkillRuntime();
	const linearStatus = useAppStore((s) => s.linearStatus);
	const linearStatusChecked = useAppStore((s) => s.linearStatusChecked);
	const linearStatusContextKey = useAppStore((s) => s.linearStatusContextKey);
	const checkLinearConnection = useAppStore((s) => s.checkLinearConnection);
	const jiraStatus = useAppStore((s) => s.jiraStatus);
	const jiraStatusChecked = useAppStore((s) => s.jiraStatusChecked);
	const jiraStatusContextKey = useAppStore((s) => s.jiraStatusContextKey);
	const checkJiraConnection = useAppStore((s) => s.checkJiraConnection);
	const repos = useAppStore((s) => s.repos);
	const activeRepoId = useAppStore((s) => s.activeRepoId);
	const expectedPreflightContextKey = useAppStore((s) => localPreflightContextKey(getLocalPreflightContext(s)));
	const setupScriptProbe = (0, import_react.useSyncExternalStore)(subscribeSetupScriptProbeCache, readSetupScriptProbeCache, readSetupScriptProbeCache);
	const [computerUsePermissionsReady, setComputerUsePermissionsReady] = (0, import_react.useState)(false);
	const [computerUsePermissionStatusChecked, setComputerUsePermissionStatusChecked] = (0, import_react.useState)(false);
	const [computerUseUnavailable, setComputerUseUnavailable] = (0, import_react.useState)(false);
	const { installed: detectedBrowserUseSkillInstalled, loading: detectedBrowserUseSkillLoading } = useInstalledAgentSkill(ORCA_CLI_SKILL_NAME, {
		enabled: shouldRefreshCoreState,
		discoveryTarget: activeSkillRuntime.discoveryTarget,
		sourceKinds: GLOBAL_AGENT_SKILL_SOURCE_KINDS
	});
	const { installed: computerUseSkillInstalled, loading: computerUseSkillLoading } = useInstalledAgentSkill(COMPUTER_USE_SKILL_NAME, {
		enabled: shouldRefreshCoreState,
		discoveryTarget: activeSkillRuntime.discoveryTarget,
		sourceKinds: GLOBAL_AGENT_SKILL_SOURCE_KINDS
	});
	const { installed: detectedOrchestrationSkillInstalled, loading: detectedOrchestrationSkillLoading } = useInstalledAgentSkill(ORCHESTRATION_SKILL_NAME, {
		enabled: shouldRefreshCoreState,
		discoveryTarget: activeSkillRuntime.discoveryTarget,
		sourceKinds: GLOBAL_AGENT_SKILL_SOURCE_KINDS
	});
	const providerRuntimeContextKey = getProviderRuntimeContextKey(settings);
	const linearStatusCurrent = linearStatusContextKey === providerRuntimeContextKey;
	const jiraStatusCurrent = jiraStatusContextKey === providerRuntimeContextKey;
	const preflightStatusCurrent = preflightStatusContextKey === expectedPreflightContextKey;
	(0, import_react.useEffect)(() => {
		if (!shouldRefreshCoreState) return;
		if (!preflightStatusCurrent || !preflightStatusChecked) refreshPreflightStatus();
		if (!linearStatusCurrent || !linearStatusChecked) checkLinearConnection();
		if (!jiraStatusCurrent || !jiraStatusChecked) checkJiraConnection();
	}, [
		checkJiraConnection,
		checkLinearConnection,
		jiraStatusCurrent,
		jiraStatusChecked,
		jiraStatusContextKey,
		linearStatusCurrent,
		linearStatusChecked,
		linearStatusContextKey,
		expectedPreflightContextKey,
		preflightStatusContextKey,
		preflightStatusCurrent,
		preflightStatusChecked,
		providerRuntimeContextKey,
		refreshPreflightStatus,
		shouldRefreshCoreState
	]);
	const orderedGitRepos = (0, import_react.useMemo)(() => {
		const gitRepos = repos.filter(isGitRepoKind);
		const activeRepo = activeRepoId ? gitRepos.find((repo) => repo.id === activeRepoId) ?? null : null;
		return activeRepo ? [activeRepo, ...gitRepos.filter((repo) => repo.id !== activeRepo.id)] : gitRepos;
	}, [activeRepoId, repos]);
	const setupScriptProbeSignature = (0, import_react.useMemo)(() => getSetupScriptProbeSignature(settings, orderedGitRepos), [orderedGitRepos, settings]);
	const activeSetupScriptProbeSignatureRef = (0, import_react.useRef)(setupScriptProbeSignature);
	activeSetupScriptProbeSignatureRef.current = setupScriptProbeSignature;
	(0, import_react.useEffect)(() => {
		if (!shouldRefreshCoreState || !settings || setupScriptProbeSignature === null) return;
		const signature = setupScriptProbeSignature;
		let stale = false;
		const timeoutId = window.setTimeout(() => {
			if (activeSetupScriptProbeSignatureRef.current === signature) setSetupScriptProbeCache({
				signature,
				ready: true,
				hasSetupScript: false
			});
		}, SETUP_SCRIPT_PROBE_SETTLE_TIMEOUT_MS);
		const settle = (hasSetupScript) => {
			window.clearTimeout(timeoutId);
			if (activeSetupScriptProbeSignatureRef.current === signature) setSetupScriptProbeCache({
				signature,
				ready: true,
				hasSetupScript
			});
		};
		async function refreshSetupScriptState() {
			for (const repo of orderedGitRepos) {
				const hooksResult = await checkRuntimeHooks(settings, repo.id).catch(() => null);
				if (stale) return;
				if (hooksResult && hasEffectiveSetupCommand(repo, hooksResult)) {
					settle(true);
					return;
				}
			}
			settle(false);
		}
		refreshSetupScriptState();
		return () => {
			stale = true;
			window.clearTimeout(timeoutId);
		};
	}, [
		orderedGitRepos,
		settings,
		setupScriptProbeSignature,
		shouldRefreshCoreState
	]);
	const readComputerUsePermissions = (0, import_react.useCallback)(async (isStale) => {
		const status = await window.api.computerUsePermissions.getStatus().catch(() => null);
		if (isStale()) return;
		const permissionState = getComputerUsePermissionSetupState(status);
		setComputerUsePermissionStatusChecked(true);
		setComputerUsePermissionsReady(permissionState.ready);
		setComputerUseUnavailable(permissionState.unavailable);
	}, []);
	(0, import_react.useEffect)(() => {
		if (!shouldRefreshCoreState || !computerUseSkillInstalled) {
			setComputerUsePermissionStatusChecked(false);
			setComputerUsePermissionsReady(false);
			setComputerUseUnavailable(false);
			return;
		}
		let stale = false;
		const refreshComputerUsePermissions = () => {
			readComputerUsePermissions(() => stale);
		};
		refreshComputerUsePermissions();
		const handleFocus = () => {
			refreshComputerUsePermissions();
		};
		const handleVisibilityChange = () => {
			if (document.visibilityState === "visible") refreshComputerUsePermissions();
		};
		window.addEventListener("focus", handleFocus);
		document.addEventListener("visibilitychange", handleVisibilityChange);
		return () => {
			stale = true;
			window.removeEventListener("focus", handleFocus);
			document.removeEventListener("visibilitychange", handleVisibilityChange);
		};
	}, [
		computerUseSkillInstalled,
		readComputerUsePermissions,
		shouldRefreshCoreState
	]);
	const taskSourceStatus = deriveIntegrationConnectionStatus({
		preflightStatus,
		preflightStatusChecked,
		preflightStatusContextKey,
		preflightStatusError,
		preflightStatusLoading,
		expectedPreflightContextKey,
		linearStatus,
		linearStatusChecked,
		linearStatusContextKey,
		jiraStatus,
		jiraStatusChecked,
		jiraStatusContextKey,
		providerRuntimeContextKey
	});
	const hasConnectedTaskSource = taskSourceStatus.trackerConnected;
	const gitRepoCount = orderedGitRepos.length;
	const currentSetupScriptProbe = getCurrentSetupScriptProbeState(setupScriptProbe, setupScriptProbeSignature);
	const currentComputerUsePermissionStatusChecked = shouldRefreshCoreState && computerUseSkillInstalled ? computerUsePermissionStatusChecked : false;
	const currentComputerUsePermissionsReady = shouldRefreshCoreState && computerUseSkillInstalled ? computerUsePermissionsReady : false;
	const currentComputerUseUnavailable = shouldRefreshCoreState && computerUseSkillInstalled ? computerUseUnavailable : false;
	const ready = getSetupGuideProgressReady({
		refreshEnabled: shouldRefreshCoreState,
		settingsLoaded: settings !== null,
		preflightStatusChecked: !taskSourceStatus.checking,
		linearStatusChecked: true,
		jiraStatusChecked: true,
		browserUseSkillDiscoveryLoading: detectedBrowserUseSkillLoading,
		computerUseSkillDiscoveryLoading: computerUseSkillLoading,
		orchestrationSkillDiscoveryLoading: detectedOrchestrationSkillLoading,
		setupScriptProbeReady: currentSetupScriptProbe.ready,
		computerUseSkillInstalled,
		computerUsePermissionStatusChecked: currentComputerUsePermissionStatusChecked
	});
	return useSetupGuideBrowserMilestoneProgress((0, import_react.useMemo)(() => getFeatureWallSetupProgress({
		ready,
		settings,
		featureInteractions,
		hasConnectedTaskSource,
		browserUseSkillInstalled: browserUseSkillInstalled || detectedBrowserUseSkillInstalled,
		computerUseSkillInstalled,
		computerUsePermissionsReady: currentComputerUsePermissionsReady,
		computerUseUnavailable: currentComputerUseUnavailable,
		orchestrationSkillInstalled: orchestrationSkillInstalled || detectedOrchestrationSkillInstalled,
		gitRepoCount,
		worktreesByRepo,
		hasSetupScript: currentSetupScriptProbe.hasSetupScript
	}), [
		browserUseSkillInstalled,
		ready,
		currentComputerUseUnavailable,
		currentComputerUsePermissionsReady,
		computerUseSkillInstalled,
		detectedBrowserUseSkillInstalled,
		detectedOrchestrationSkillInstalled,
		featureInteractions,
		gitRepoCount,
		hasConnectedTaskSource,
		currentSetupScriptProbe.hasSetupScript,
		orchestrationSkillInstalled,
		settings,
		worktreesByRepo
	]), hasFeatureInteraction(featureInteractions, "terminal-pane-split"));
}
export { useSetupGuideProgress as t };
