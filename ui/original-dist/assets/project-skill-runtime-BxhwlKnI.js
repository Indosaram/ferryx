import { a as translate } from "./jsx-runtime-Cv_nyRjc.js";
import { lc as resolveWindowsShellStartupFamily } from "./store-CgXrfmaH.js";
function getProjectSkillDiscoveryTarget(projectRuntime) {
	return projectRuntime ? { projectRuntime } : void 0;
}
function getProjectAgentSkillRuntime(projectRuntime, currentPlatform) {
	if (!projectRuntime) return;
	if (projectRuntime.status === "repair-required") return getWslAgentSkillRuntime(projectRuntime.repair.preferredRuntime.distro);
	if (projectRuntime.runtime.kind === "wsl") return getWslAgentSkillRuntime(projectRuntime.runtime.distro);
	return {
		runtime: "host",
		label: currentPlatform === "win32" ? "Windows" : "This device"
	};
}
function getProjectAgentSkillTerminalShellOverride(currentPlatform, settings, runtime) {
	if (currentPlatform !== "win32") return;
	if (runtime?.runtime === "wsl") return "powershell.exe";
	return resolveWindowsShellStartupFamily(settings?.terminalWindowsShell) === "posix" ? "powershell.exe" : void 0;
}
function getProjectSkillInstallDisabledReason(projectRuntime) {
	if (projectRuntime?.status !== "repair-required") return null;
	switch (projectRuntime.repair.reason) {
		case "wsl-unavailable": return translate("auto.lib.projectSkillRuntime.wslUnavailable", "Project runtime needs WSL before this skill can be installed.");
		case "wsl-distro-required": return translate("auto.lib.projectSkillRuntime.distroRequired", "Select a WSL distro for this project before installing this skill.");
		case "wsl-distro-missing": return translate("auto.lib.projectSkillRuntime.distroMissing", "The selected WSL distro is unavailable. Choose an available distro or switch this project to Windows.");
	}
}
function getWslAgentSkillRuntime(distro) {
	return {
		runtime: "wsl",
		wslDistro: distro,
		label: distro ? `WSL ${distro}` : translate("auto.lib.projectSkillRuntime.wslDefault", "WSL default")
	};
}
export { getProjectSkillInstallDisabledReason as i, getProjectAgentSkillTerminalShellOverride as n, getProjectSkillDiscoveryTarget as r, getProjectAgentSkillRuntime as t };
