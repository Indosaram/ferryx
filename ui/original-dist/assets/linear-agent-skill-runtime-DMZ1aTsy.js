import { a as translate } from "./jsx-runtime-Cv_nyRjc.js";
import { C as ORCA_LINEAR_SKILL_NAME, _ as LINEAR_TICKETS_SKILL_UPDATE_COMMAND, g as LINEAR_TICKETS_SKILL_NAME, w as ORCA_LINEAR_SKILL_UPDATE_COMMAND } from "./use-active-skill-discovery-runtime-target-CdctmJyj.js";
import { n as hasInstalledAgentSkill, t as GLOBAL_AGENT_SKILL_SOURCE_KINDS } from "./useInstalledAgentSkills-BYdWqfUf.js";
import { n as getProjectAgentSkillTerminalShellOverride } from "./project-skill-runtime-BxhwlKnI.js";
function getLinearAgentSkillUpdateTarget(skills, installed) {
	const canonicalSkillInstalled = hasInstalledAgentSkill(skills, ORCA_LINEAR_SKILL_NAME, { sourceKinds: GLOBAL_AGENT_SKILL_SOURCE_KINDS });
	const legacySkillInstalled = hasInstalledAgentSkill(skills, LINEAR_TICKETS_SKILL_NAME, { sourceKinds: GLOBAL_AGENT_SKILL_SOURCE_KINDS });
	return !installed || canonicalSkillInstalled || !legacySkillInstalled ? {
		skillName: ORCA_LINEAR_SKILL_NAME,
		command: ORCA_LINEAR_SKILL_UPDATE_COMMAND
	} : {
		skillName: LINEAR_TICKETS_SKILL_NAME,
		command: LINEAR_TICKETS_SKILL_UPDATE_COMMAND
	};
}
function getLinearAgentSkillUpdateCommand(skills, installed) {
	return getLinearAgentSkillUpdateTarget(skills, installed).command;
}
function getLinearAgentSkillSetupMissingLabel(cliAvailable, skillInstalled) {
	if (!cliAvailable && !skillInstalled) return translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.missingCliAndSkill", "Orca CLI and Linear agent skill are missing.");
	if (!cliAvailable) return translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.missingCli", "Orca CLI is missing.");
	return translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.missingSkill", "Linear agent skill is missing.");
}
function getLinearAgentSkillSetupToastTitle(cliAvailable, skillInstalled) {
	if (!cliAvailable && !skillInstalled) return translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.toastMissingCliAndSkill", "Orca CLI and Linear skill are missing");
	if (!cliAvailable) return translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.toastMissingCli", "Orca CLI is missing");
	return translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.toastMissingSkill", "Linear skill is missing");
}
function getLinearAgentSkillSetupToastDescription(cliAvailable, skillInstalled, remote, agentRuntime) {
	const baseDescription = getLinearAgentSkillSetupToastBaseDescription(cliAvailable, skillInstalled);
	if (remote) return translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.toastRemoteDescription", "{{value0}} Remote agent environments may need their own setup.", { value0: baseDescription });
	if (agentRuntime.runtime === "wsl") return translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.toastWslDescription", "{{value0}} This setup runs in the selected WSL agent runtime.", { value0: baseDescription });
	return baseDescription;
}
function getLinearAgentSkillSetupToastBaseDescription(cliAvailable, skillInstalled) {
	if (!cliAvailable && !skillInstalled) return translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.toastInstallCliAndSkillDescription", "Install the Orca CLI and the Linear skill to enable your agents to read and edit Linear tasks.");
	if (!cliAvailable) return translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.toastInstallCliDescription", "Install the Orca CLI to enable your agents to read and edit Linear tasks.");
	return translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.toastInstallSkillDescription", "Install the Linear skill to enable your agents to read and edit Linear tasks through the Orca CLI.");
}
function getLinearAgentSkillSetupInlineRuntimeCopy(remote, agentRuntime) {
	if (remote) return translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.remoteCopy", "This installs host setup; remote agent environments may need separate setup.");
	if (agentRuntime.runtime === "wsl") return translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.wslCopy", "Install it for WSL agent handoffs from linked Linear work.");
	return translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.hostCopy", "Install it for host agent handoffs from linked Linear work.");
}
var LOCAL_DISMISS_STORAGE_KEY_PREFIX = "orca.linearTicketsSkill.setupDismissed";
function getCurrentPlatform() {
	if (navigator.userAgent.includes("Windows")) return "win32";
	return navigator.userAgent.includes("Linux") ? "linux" : "darwin";
}
function getLinearPromptAgentRuntime(settings, currentPlatform, remote, projectRuntime) {
	if (remote) return {
		runtime: "host",
		label: currentPlatform === "win32" ? "Windows" : "This device"
	};
	const resolvedProjectRuntime = getProjectAgentRuntime(projectRuntime, currentPlatform);
	if (resolvedProjectRuntime) return resolvedProjectRuntime;
	const selectedRuntime = settings?.localAgentRuntime ?? "host";
	if (currentPlatform === "win32" && selectedRuntime === "wsl") {
		const selectedDistro = settings?.localAgentWslDistro?.trim() || null;
		return {
			runtime: "wsl",
			wslDistro: selectedDistro,
			label: selectedDistro ? `WSL ${selectedDistro}` : translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.wslLabel", "WSL default")
		};
	}
	return {
		runtime: "host",
		label: currentPlatform === "win32" ? "Windows" : "This device"
	};
}
function getProjectAgentRuntime(projectRuntime, currentPlatform) {
	if (!projectRuntime) return null;
	if (projectRuntime.status === "repair-required") return getWslAgentRuntime(projectRuntime.repair.preferredRuntime.distro);
	if (projectRuntime.runtime.kind === "wsl") return getWslAgentRuntime(projectRuntime.runtime.distro);
	return {
		runtime: "host",
		label: currentPlatform === "win32" ? "Windows" : "This device"
	};
}
function getWslAgentRuntime(distro) {
	return {
		runtime: "wsl",
		wslDistro: distro,
		label: distro ? `WSL ${distro}` : translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.wslLabel", "WSL default")
	};
}
function getLinearPromptTerminalShellOverride(currentPlatform, settings, runtime) {
	return getProjectAgentSkillTerminalShellOverride(currentPlatform, settings, runtime);
}
function getLinearPromptSetupCheckIdentity(args) {
	return JSON.stringify({
		remote: args.remote,
		runtime: args.runtime.runtime,
		wslDistro: args.runtime.wslDistro ?? null,
		projectRuntime: getProjectRuntimeIdentity(args.projectRuntime),
		activeRuntimeEnvironmentId: args.activeRuntimeEnvironmentId ?? null
	});
}
function getLinearPromptSkillDiscoveryTarget(runtime, projectRuntime) {
	if (projectRuntime) return { projectRuntime };
	return runtime.runtime === "wsl" ? {
		runtime: "wsl",
		wslDistro: runtime.wslDistro
	} : void 0;
}
function getLocalDismissStorageKey(runtime) {
	if (runtime.runtime !== "wsl") return `${LOCAL_DISMISS_STORAGE_KEY_PREFIX}.host`;
	return `${LOCAL_DISMISS_STORAGE_KEY_PREFIX}.wsl.${runtime.wslDistro?.trim() || "default"}`;
}
function readLocalDismissed(storageKey) {
	if (typeof window === "undefined") return false;
	return localStorage.getItem(storageKey) === "1";
}
function getProjectRuntimeIdentity(projectRuntime) {
	if (!projectRuntime) return null;
	return projectRuntime.status === "resolved" ? projectRuntime.runtime.cacheKey : projectRuntime.repair.cacheKey;
}
export { getLinearPromptTerminalShellOverride as a, getLinearAgentSkillSetupInlineRuntimeCopy as c, getLinearAgentSkillSetupToastTitle as d, getLinearAgentSkillUpdateCommand as f, getLinearPromptSkillDiscoveryTarget as i, getLinearAgentSkillSetupMissingLabel as l, getLinearPromptAgentRuntime as n, getLocalDismissStorageKey as o, getLinearAgentSkillUpdateTarget as p, getLinearPromptSetupCheckIdentity as r, readLocalDismissed as s, getCurrentPlatform as t, getLinearAgentSkillSetupToastDescription as u };
