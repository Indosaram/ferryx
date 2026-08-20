import { a as translate } from "./jsx-runtime-Cv_nyRjc.js";
import { Ca as normalizeGlobalWindowsRuntimeDefault, Sa as deriveGlobalWindowsRuntimeDefaultFromLegacySettings, lc as resolveWindowsShellStartupFamily, t as useAppStore, uc as isWslShellName } from "./store-CgXrfmaH.js";
import { n as toast } from "./dist-DgqligFk.js";
import { O as buildAgentFeatureSkillInstallCommand } from "./use-active-skill-discovery-runtime-target-CdctmJyj.js";
import { n as getProjectAgentSkillTerminalShellOverride } from "./project-skill-runtime-BxhwlKnI.js";
const AGENT_SKILL_CLI_PREREQUISITE_NOTICE = "Before opening setup, Orca may show a system prompt to register the Orca CLI command on PATH.";
const CLI_PREREQUISITE_REGISTRATION_TOAST = "Orca needs to register its CLI on PATH.";
const CLI_PREREQUISITE_REGISTRATION_TOAST_DESCRIPTION = "Approve the system prompt so skill setup can use the Orca CLI command.";
function isOrcaCliAvailableOnPath(status) {
	return status?.state === "installed" && status.pathConfigured === true;
}
async function ensureOrcaCliAvailableForAgentSkillTerminal({ onStatusChange, registrationPromptDelayMs = 700 } = {}) {
	try {
		const status = await window.api.cli.getInstallStatus();
		onStatusChange?.(status);
		if (!status.supported) {
			showCliPrerequisiteWarning(status);
			return status;
		}
		if (status.pathConfigured === null) {
			showCliPrerequisiteWarning(status);
			return status;
		}
		if (status.state !== "installed" || status.pathConfigured === false) {
			await showOrcaCliRegistrationPromptToast(registrationPromptDelayMs);
			const next = await window.api.cli.install();
			onStatusChange?.(next);
			showCliPrerequisiteWarning(next);
			return next;
		}
		return status;
	} catch (error) {
		toast.error(error instanceof Error ? error.message : translate("auto.lib.agent.skill.cli.prerequisite.8d6eedf97e", "Failed to register the Orca CLI in PATH."));
		return null;
	}
}
async function showOrcaCliRegistrationPromptToast(delayMs = 700) {
	toast.message(CLI_PREREQUISITE_REGISTRATION_TOAST, { description: CLI_PREREQUISITE_REGISTRATION_TOAST_DESCRIPTION });
	await delay(delayMs);
}
function delay(ms) {
	if (ms <= 0) return Promise.resolve();
	return new Promise((resolve) => window.setTimeout(resolve, ms));
}
function showCliPrerequisiteWarning(status) {
	if (!status.supported) {
		toast.warning(translate("auto.lib.agent.skill.cli.prerequisite.2db0bd7515", "Orca CLI registration is unavailable"), { description: status.detail ?? translate("auto.lib.agent.skill.cli.prerequisite.15cbedc3e3", "Install the Orca CLI before running agent skill setup.") });
		return;
	}
	if (status.state !== "installed") {
		toast.warning(translate("auto.lib.agent.skill.cli.prerequisite.e99d7dc36f", "Orca CLI registration needs attention"), { description: status.detail ?? translate("auto.lib.agent.skill.cli.prerequisite.15cbedc3e3", "Install the Orca CLI before running agent skill setup.") });
		return;
	}
	if (status.pathConfigured === null) {
		toast.warning(translate("auto.lib.agent.skill.cli.prerequisite.windowsPathUnknown", "Orca could not check your Windows user PATH"), { description: status.detail ?? translate("auto.lib.agent.skill.cli.prerequisite.refreshCliRegistration", "Refresh CLI registration status and try again.") });
		return;
	}
	if (status.pathConfigured === false) toast.warning(translate("auto.lib.agent.skill.cli.prerequisite.79371593b0", "Orca CLI is not visible on PATH yet"), { description: status.detail ?? translate("auto.lib.agent.skill.cli.prerequisite.0f116999f1", "Restart your shell or add the Orca CLI directory to PATH before setup.") });
}
function quotePowerShellLiteral(value) {
	return `'${value.replace(/'/g, "''")}'`;
}
function quotePowerShellNativeArgument(value) {
	return quotePowerShellLiteral(value.replace(/(\\*)"/g, "$1$1\\\""));
}
function quotePosixShell(value) {
	return `'${value.replace(/'/g, "'\\''")}'`;
}
function buildWslLoginShellCommand(command) {
	const quotedCommand = quotePosixShell(command);
	return [
		"_orca_wsl_shell=$(getent passwd \"$(id -un)\" 2>/dev/null | cut -d: -f7)",
		"if [ -z \"$_orca_wsl_shell\" ] || [ ! -x \"$_orca_wsl_shell\" ]; then",
		"  _orca_wsl_shell=\"${SHELL:-/bin/bash}\"",
		"fi",
		"if [ -z \"$_orca_wsl_shell\" ] || [ ! -x \"$_orca_wsl_shell\" ]; then",
		"  _orca_wsl_shell=/bin/sh",
		"fi",
		"_orca_wsl_shell_name=$(basename \"$_orca_wsl_shell\" | tr \"[:upper:]\" \"[:lower:]\")",
		"case \"$_orca_wsl_shell_name\" in",
		`  sh|dash) exec "$_orca_wsl_shell" -lc ${quotedCommand} ;;`,
		`  bash|zsh|ksh|mksh|ash) exec "$_orca_wsl_shell" -ilc ${quotedCommand} ;;`,
		`  *) exec /bin/sh -lc ${quotedCommand} ;;`,
		"esac"
	].join("\n");
}
var LOCAL_HOST_AGENT_RUNTIME = {
	runtime: "host",
	label: ""
};
function getHostRuntimeLabel() {
	return navigator.userAgent.includes("Windows") ? "Windows" : "This device";
}
function getSelectedAgentRuntime(settings, wslSupportedPlatform, wslAvailable, wslCapabilitiesLoading) {
	const defaultRuntime = normalizeGlobalWindowsRuntimeDefault(settings.localWindowsRuntimeDefault ?? deriveGlobalWindowsRuntimeDefaultFromLegacySettings(settings, { wslAvailable: wslCapabilitiesLoading ? void 0 : wslAvailable }).defaultRuntime);
	if (wslSupportedPlatform && defaultRuntime.kind === "wsl") {
		const selectedDistro = defaultRuntime.distro?.trim() || null;
		return {
			runtime: "wsl",
			wslDistro: selectedDistro,
			label: selectedDistro ? `WSL ${selectedDistro}` : translate("auto.components.settings.CliSkillRuntimeSetup.c47127f222", "WSL default")
		};
	}
	return {
		runtime: "host",
		label: getHostRuntimeLabel()
	};
}
function encodeWslLoginShellScript(command) {
	const bytes = new TextEncoder().encode(buildWslLoginShellCommand(command));
	let binary = "";
	for (const byte of bytes) binary += String.fromCharCode(byte);
	return btoa(binary);
}
function getWslCliDistroRequest(runtime) {
	return runtime?.runtime === "wsl" && runtime.wslDistro?.trim() ? { distro: runtime.wslDistro.trim() } : void 0;
}
function buildSkillCommandForRuntime(command, runtime, currentPlatform = getSkillCommandPlatform()) {
	const resolvedRuntime = runtime ?? LOCAL_HOST_AGENT_RUNTIME;
	const normalizedCommand = normalizeWindowsSkillUpdateCommand(command, resolvedRuntime, currentPlatform);
	if (resolvedRuntime.runtime !== "wsl") return wrapWindowsSkillCommandWithNpxPrerequisite(normalizedCommand, currentPlatform, "copied-command");
	return normalizedCommand;
}
function normalizeWindowsSkillUpdateCommand(command, runtime, currentPlatform) {
	if (runtime.runtime === "wsl" || currentPlatform !== "win32") return command;
	const trimmedCommand = command.trim();
	const updateMatch = /^npx\s+skills\s+update\s+([A-Za-z0-9_-]+)\s+--global$/i.exec(trimmedCommand);
	if (!updateMatch) return command;
	return buildAgentFeatureSkillInstallCommand([updateMatch[1]]);
}
function buildSkillSetupTerminalCommand(copiedCommand, effectiveShell, runtime, currentPlatform = getSkillCommandPlatform()) {
	const wslNative = isWslShellName(effectiveShell) ? decodeWslSetupTerminalCommand(copiedCommand) : null;
	if (wslNative) return wslNative;
	if (!isSetupTerminalForcedToPowerShell(effectiveShell)) return copiedCommand;
	if (runtime?.runtime === "wsl" && currentPlatform === "win32") return buildPowerShellWslSkillCommand(copiedCommand, runtime);
	return wrapWindowsSkillCommandWithNpxPrerequisite(copiedCommand, currentPlatform, "orca-setup-terminal");
}
function buildPowerShellWslSkillCommand(command, runtime) {
	const distroArg = runtime.wslDistro?.trim() ? ` -d ${quotePowerShellLiteral(runtime.wslDistro.trim())}` : "";
	const encodedScript = encodeWslLoginShellScript(command);
	const visibleCommand = command.replace(/[\r\n]+/g, " ");
	return `& { $PSNativeCommandArgumentPassing = 'Legacy'; ${`wsl.exe${distroArg} -- sh -c ${quotePowerShellNativeArgument(`eval "\`printf %s ${encodedScript} | base64 -d\`"`)}`} } # Runs: ${visibleCommand}`;
}
function decodeWslSetupTerminalCommand(command) {
	if (!command.startsWith("& { $PSNativeCommandArgumentPassing = 'Legacy'; wsl.exe") || !command.includes(" } # Runs: ")) return null;
	const encoded = /-- sh -c 'eval \\"`printf %s ([A-Za-z0-9+/=]+) \| base64 -d`\\"'/.exec(command)?.[1];
	if (!encoded) return null;
	try {
		const binary = atob(encoded);
		const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0));
		return new TextDecoder().decode(bytes);
	} catch {
		return null;
	}
}
function isSetupTerminalForcedToPowerShell(terminalShellOverride) {
	const trimmedOverride = terminalShellOverride?.trim();
	return Boolean(trimmedOverride) && resolveWindowsShellStartupFamily(trimmedOverride) === "powershell";
}
function wrapWindowsSkillCommandWithNpxPrerequisite(command, currentPlatform, target) {
	const trimmedCommand = command.trim();
	if (currentPlatform !== "win32" || isRemoteRuntimeEnvironmentFocused() || target === "copied-command" && isPosixFamilyWindowsShellConfigured() || !/^npx\s+skills\s+(?:add|update)\b/i.test(trimmedCommand)) return command;
	return `cmd.exe /d /s /c "where.exe npx >nul 2>nul & if errorlevel 1 (echo ERROR: npx was not found. Install Node.js LTS from https://nodejs.org/ to get npx. & echo Then close this terminal and start skill setup again - a new terminal picks up the updated PATH. & exit /b 1) else (${trimmedCommand})"`;
}
function isPosixFamilyWindowsShellConfigured() {
	return resolveWindowsShellStartupFamily(useAppStore.getState().settings?.terminalWindowsShell) === "posix";
}
function isRemoteRuntimeEnvironmentFocused() {
	return Boolean(useAppStore.getState().settings?.activeRuntimeEnvironmentId?.trim());
}
function getSkillCommandPlatform() {
	const platform = typeof window === "undefined" ? void 0 : window.api?.platform?.get?.()?.platform;
	if (platform) return platform;
	const userAgent = typeof navigator === "undefined" ? "" : navigator.userAgent;
	if (userAgent.includes("Windows")) return "win32";
	if (userAgent.includes("Mac")) return "darwin";
	return "linux";
}
function getSkillDiscoveryTargetForRuntime(runtime) {
	return runtime.runtime === "wsl" ? {
		runtime: "wsl",
		wslDistro: runtime.wslDistro ?? null
	} : void 0;
}
function getAgentSkillTerminalShellOverride(currentPlatform, settings, runtime) {
	return getProjectAgentSkillTerminalShellOverride(currentPlatform, settings, runtime);
}
async function ensureWslCliAvailableForAgentSkillTerminal(runtime) {
	const args = getWslCliDistroRequest(runtime);
	try {
		const status = await window.api.cli.getWslInstallStatus(args);
		if (!status.supported) {
			toast.warning(translate("auto.components.settings.CliSkillRuntimeSetup.775a4cfbb8", "WSL shell command registration is unavailable"), { description: status.detail ?? translate("auto.components.settings.CliSkillRuntimeSetup.fc0fcf72fd", "Register the WSL shell command before skill setup.") });
			return status;
		}
		if (status.pathConfigured === null) {
			toast.warning(translate("auto.components.settings.CliSkillRuntimeSetup.windowsPathUnknown", "WSL shell command PATH could not be checked"), { description: status.detail ?? translate("auto.components.settings.CliSkillRuntimeSetup.refreshCliRegistration", "Refresh CLI registration status and try again.") });
			return status;
		}
		if (status.state !== "installed" || status.pathConfigured === false) {
			await showOrcaCliRegistrationPromptToast();
			const next = await window.api.cli.installWsl(args);
			if (!isOrcaCliAvailableOnPath(next)) toast.warning(translate("auto.components.settings.CliSkillRuntimeSetup.3728a94fb6", "WSL shell command needs attention"), { description: next.detail ?? translate("auto.components.settings.CliSkillRuntimeSetup.fc0fcf72fd", "Register the WSL shell command before skill setup.") });
			return next;
		}
		return status;
	} catch (error) {
		toast.error(error instanceof Error ? error.message : translate("auto.components.settings.CliSkillRuntimeSetup.0ed08febc5", "Failed to register the WSL shell command."));
		return null;
	}
}
export { getSelectedAgentRuntime as a, AGENT_SKILL_CLI_PREREQUISITE_NOTICE as c, showOrcaCliRegistrationPromptToast as d, getAgentSkillTerminalShellOverride as i, ensureOrcaCliAvailableForAgentSkillTerminal as l, buildSkillSetupTerminalCommand as n, getSkillDiscoveryTargetForRuntime as o, ensureWslCliAvailableForAgentSkillTerminal as r, getWslCliDistroRequest as s, buildSkillCommandForRuntime as t, isOrcaCliAvailableOnPath as u };
