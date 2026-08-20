import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { Sd as getRuntimeScopedSkillDiscoveryKey, Td as INSTALLED_AGENT_SKILLS_REFRESHED_EVENT, bd as discoverInstalledAgentSkills, wd as INSTALLED_AGENT_SKILLS_CHANGED_EVENT, xd as getCachedSkillDiscovery } from "./store-CgXrfmaH.js";
import { t as useMountedRef } from "./useMountedRef-1omUd-IV.js";
import { E as ORCHESTRATION_SKILL_NAME, s as markOrchestrationSetupComplete, t as useActiveSkillDiscoveryRuntimeTarget } from "./use-active-skill-discovery-runtime-target-CdctmJyj.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var UNRESOLVED_RUNTIME_DISCOVERY_KEY = "runtime:unresolved";
const GLOBAL_AGENT_SKILL_SOURCE_KINDS = ["home"];
function normalizeSkillName(value) {
	return value.trim().toLowerCase();
}
function isOrchestrationSkillName(skillName) {
	return normalizeSkillName(skillName) === ORCHESTRATION_SKILL_NAME;
}
function basenameFromPath(pathValue) {
	return pathValue.split(/[\\/]/).findLast(Boolean) ?? pathValue;
}
function hasInstalledAgentSkill(skills, skillName, options = {}) {
	return hasInstalledAgentSkillNamed(skills, [skillName], options);
}
function hasInstalledAgentSkillNamed(skills, skillNames, options = {}) {
	const expected = new Set(skillNames.map(normalizeSkillName));
	return skills.some((skill) => {
		if (!skill.installed) return false;
		if (options.sourceKinds && !options.sourceKinds.includes(skill.sourceKind)) return false;
		return expected.has(normalizeSkillName(skill.name)) || expected.has(normalizeSkillName(basenameFromPath(skill.directoryPath)));
	});
}
function notifyInstalledAgentSkillsRefreshed() {
	if (typeof window !== "undefined") window.dispatchEvent(new CustomEvent(INSTALLED_AGENT_SKILLS_REFRESHED_EVENT));
}
function useInstalledAgentSkill(skillName, options = {}) {
	return useInstalledAgentSkillNames([skillName], options);
}
function useInstalledAgentSkillNames(skillNames, options = {}) {
	const { enabled = true, discoveryTarget, sourceKinds } = options;
	const skillNamesKey = skillNames.map(normalizeSkillName).join("\n");
	const candidateSkillNames = (0, import_react.useMemo)(() => skillNamesKey.split("\n"), [skillNamesKey]);
	const runtimeTarget = useActiveSkillDiscoveryRuntimeTarget();
	const discoveryTargetKey = runtimeTarget ? getRuntimeScopedSkillDiscoveryKey(runtimeTarget, discoveryTarget) : UNRESOLVED_RUNTIME_DISCOVERY_KEY;
	const [latchedDiscoveryTarget, setLatchedDiscoveryTarget] = (0, import_react.useState)({
		key: discoveryTargetKey,
		target: discoveryTarget
	});
	if (latchedDiscoveryTarget.key !== discoveryTargetKey) setLatchedDiscoveryTarget({
		key: discoveryTargetKey,
		target: discoveryTarget
	});
	const stableDiscoveryTarget = latchedDiscoveryTarget.key === discoveryTargetKey ? latchedDiscoveryTarget.target : discoveryTarget;
	const cachedDiscovery = getCachedSkillDiscovery(discoveryTargetKey);
	const [result, setResult] = (0, import_react.useState)(cachedDiscovery);
	const [loading, setLoading] = (0, import_react.useState)(enabled && !cachedDiscovery);
	const [error, setError] = (0, import_react.useState)(null);
	const currentDiscoveryTargetKeyRef = (0, import_react.useRef)(discoveryTargetKey);
	const refreshGenerationRef = (0, import_react.useRef)(0);
	const stateResetInputRef = (0, import_react.useRef)({
		discoveryTargetKey,
		enabled
	});
	currentDiscoveryTargetKeyRef.current = discoveryTargetKey;
	const mountedRef = useMountedRef();
	let resultForRender = result;
	let loadingForRender = loading;
	let errorForRender = error;
	if (stateResetInputRef.current.discoveryTargetKey !== discoveryTargetKey || stateResetInputRef.current.enabled !== enabled) {
		const nextCachedDiscovery = getCachedSkillDiscovery(discoveryTargetKey);
		const nextLoading = enabled && !nextCachedDiscovery;
		stateResetInputRef.current = {
			discoveryTargetKey,
			enabled
		};
		resultForRender = nextCachedDiscovery;
		loadingForRender = nextLoading;
		errorForRender = null;
		setResult(nextCachedDiscovery);
		setLoading(nextLoading);
		setError(null);
	}
	const refresh = (0, import_react.useCallback)(async (force = true, showLoading = true) => {
		const requestDiscoveryTargetKey = discoveryTargetKey;
		const requestGeneration = ++refreshGenerationRef.current;
		const writeIfCurrent = (write) => {
			if (mountedRef.current && requestGeneration === refreshGenerationRef.current && currentDiscoveryTargetKeyRef.current === requestDiscoveryTargetKey) write();
		};
		if (!enabled) {
			writeIfCurrent(() => {
				setLoading(false);
			});
			return false;
		}
		if (showLoading) writeIfCurrent(() => {
			setLoading(true);
		});
		if (!runtimeTarget) return false;
		let installedAfterRefresh = false;
		try {
			const next = await discoverInstalledAgentSkills(force, stableDiscoveryTarget, runtimeTarget);
			installedAfterRefresh = hasInstalledAgentSkillNamed(next.skills, candidateSkillNames, { sourceKinds });
			writeIfCurrent(() => {
				setResult(next);
				setError(null);
			});
		} catch (refreshError) {
			writeIfCurrent(() => {
				setError(refreshError instanceof Error ? refreshError.message : "Could not scan installed skills.");
			});
		} finally {
			writeIfCurrent(() => {
				setLoading(false);
			});
		}
		return installedAfterRefresh;
	}, [
		candidateSkillNames,
		discoveryTargetKey,
		enabled,
		mountedRef,
		runtimeTarget,
		sourceKinds,
		stableDiscoveryTarget
	]);
	(0, import_react.useEffect)(() => {
		refresh(false);
	}, [refresh]);
	(0, import_react.useEffect)(() => {
		if (!enabled) return;
		const refreshFromInstall = () => {
			refresh(true);
		};
		const refreshQuietly = () => {
			refresh(false, false);
		};
		window.addEventListener("focus", refreshQuietly);
		window.addEventListener(INSTALLED_AGENT_SKILLS_CHANGED_EVENT, refreshFromInstall);
		window.addEventListener(INSTALLED_AGENT_SKILLS_REFRESHED_EVENT, refreshQuietly);
		return () => {
			window.removeEventListener("focus", refreshQuietly);
			window.removeEventListener(INSTALLED_AGENT_SKILLS_CHANGED_EVENT, refreshFromInstall);
			window.removeEventListener(INSTALLED_AGENT_SKILLS_REFRESHED_EVENT, refreshQuietly);
		};
	}, [enabled, refresh]);
	const skills = (0, import_react.useMemo)(() => enabled && resultForRender ? resultForRender.skills : [], [enabled, resultForRender]);
	const sources = (0, import_react.useMemo)(() => enabled && resultForRender ? resultForRender.sources : [], [enabled, resultForRender]);
	const installed = (0, import_react.useMemo)(() => enabled ? hasInstalledAgentSkillNamed(skills, candidateSkillNames, { sourceKinds }) : false, [
		candidateSkillNames,
		enabled,
		skills,
		sourceKinds
	]);
	(0, import_react.useEffect)(() => {
		if (installed && candidateSkillNames.some(isOrchestrationSkillName)) markOrchestrationSetupComplete();
	}, [candidateSkillNames, installed]);
	const forceRefresh = (0, import_react.useCallback)(() => refresh(true), [refresh]);
	return {
		installed,
		loading: loadingForRender,
		settled: enabled && resultForRender !== null,
		error: errorForRender,
		skills,
		sources,
		refresh: forceRefresh
	};
}
export { useInstalledAgentSkillNames as a, useInstalledAgentSkill as i, hasInstalledAgentSkill as n, notifyInstalledAgentSkillsRefreshed as r, GLOBAL_AGENT_SKILL_SOURCE_KINDS as t };
