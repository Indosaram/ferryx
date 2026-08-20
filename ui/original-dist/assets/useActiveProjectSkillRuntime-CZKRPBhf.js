import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { ba as useWindowsTerminalCapabilities, da as getLocalProjectExecutionRuntimeContext, la as getGlobalWindowsExecutionRuntimeContext, t as useAppStore } from "./store-CgXrfmaH.js";
import { t as useShallow } from "./shallow-BpOhx1Gc.js";
import { t as useActiveSkillDiscoveryRuntimeTarget } from "./use-active-skill-discovery-runtime-target-CdctmJyj.js";
import { i as getProjectSkillInstallDisabledReason, n as getProjectAgentSkillTerminalShellOverride, r as getProjectSkillDiscoveryTarget, t as getProjectAgentSkillRuntime } from "./project-skill-runtime-BxhwlKnI.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var EMPTY_ACTIVE_PROJECT_SKILL_RUNTIME = Object.freeze({
	installDisabledReason: null,
	canUseLocalSkillFreshness: false
});
function shouldUseLocalSkillFreshness(runtimeTarget, agentRuntime) {
	return runtimeTarget?.kind === "local" && agentRuntime?.runtime !== "wsl";
}
function hasLocalSkillRuntimeAuthority(runtimeTarget) {
	return runtimeTarget?.kind === "local";
}
function activeProjectSkillRuntimeIdentity(runtime) {
	return JSON.stringify(runtime);
}
function wslOnly(resolution) {
	if (!resolution) return;
	return resolution.status === "repair-required" || resolution.runtime.kind === "wsl" ? resolution : void 0;
}
function useActiveProjectSkillRuntime() {
	const runtimeState = useAppStore(useShallow((state) => ({
		activeRepoId: state.activeRepoId,
		activeWorktreeId: state.activeWorktreeId,
		projects: state.projects,
		repos: state.repos,
		settings: state.settings,
		worktreesByRepo: state.worktreesByRepo
	})));
	const currentPlatform = getCurrentPlatform();
	const windowsCapabilities = useWindowsTerminalCapabilities(currentPlatform === "win32");
	const runtimeTarget = useActiveSkillDiscoveryRuntimeTarget();
	const resolved = (0, import_react.useMemo)(() => {
		const wslContext = {
			wslAvailable: windowsCapabilities.isLoading ? void 0 : windowsCapabilities.wslAvailable,
			availableWslDistros: windowsCapabilities.isLoading ? null : windowsCapabilities.wslDistros
		};
		const projectRuntime = getLocalProjectExecutionRuntimeContext(runtimeState, void 0, currentPlatform, wslContext) ?? (hasLocalSkillRuntimeAuthority(runtimeTarget) ? wslOnly(getGlobalWindowsExecutionRuntimeContext(runtimeState, void 0, currentPlatform, wslContext)) : void 0);
		if (!projectRuntime) {
			const terminalShellOverride = hasLocalSkillRuntimeAuthority(runtimeTarget) ? getProjectAgentSkillTerminalShellOverride(currentPlatform, runtimeState.settings, void 0) : void 0;
			const canUseLocalSkillFreshness = shouldUseLocalSkillFreshness(runtimeTarget);
			if (!terminalShellOverride && !canUseLocalSkillFreshness) return EMPTY_ACTIVE_PROJECT_SKILL_RUNTIME;
			return {
				installDisabledReason: null,
				terminalShellOverride,
				canUseLocalSkillFreshness
			};
		}
		const agentRuntime = getProjectAgentSkillRuntime(projectRuntime, currentPlatform);
		return {
			projectRuntime,
			discoveryTarget: getProjectSkillDiscoveryTarget(projectRuntime),
			agentRuntime,
			terminalShellOverride: getProjectAgentSkillTerminalShellOverride(currentPlatform, runtimeState.settings, agentRuntime),
			installDisabledReason: getProjectSkillInstallDisabledReason(projectRuntime),
			canUseLocalSkillFreshness: shouldUseLocalSkillFreshness(runtimeTarget, agentRuntime)
		};
	}, [
		currentPlatform,
		runtimeState,
		runtimeTarget,
		windowsCapabilities
	]);
	const [stable, setStable] = (0, import_react.useState)(resolved);
	const stableIdentity = activeProjectSkillRuntimeIdentity(stable);
	const resolvedIdentity = activeProjectSkillRuntimeIdentity(resolved);
	if (stableIdentity !== resolvedIdentity) setStable(resolved);
	return stableIdentity === resolvedIdentity ? stable : resolved;
}
function getCurrentPlatform() {
	const platform = typeof window === "undefined" ? void 0 : window.api?.platform?.get?.()?.platform;
	if (platform) return platform;
	const userAgent = typeof navigator === "undefined" ? "" : navigator.userAgent;
	if (userAgent.includes("Windows")) return "win32";
	if (userAgent.includes("Mac")) return "darwin";
	return "linux";
}
export { useActiveProjectSkillRuntime as t };
