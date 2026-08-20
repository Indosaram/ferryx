import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { l as createLucideIcon } from "./button-DszXJEV6.js";
import { t as Check } from "./check-Lb2n4tDb.js";
import { t as Info } from "./info-D3uaWrfJ.js";
import { t as LoaderCircle } from "./loader-circle-CRZpWdsi.js";
import { n as toast } from "./dist-DgqligFk.js";
import { t as Checkbox } from "./checkbox-PAbetBh2.js";
import { a as DropdownMenuLabel, l as DropdownMenuSeparator } from "./dropdown-menu-Dth6LPK-.js";
import { t as Label } from "./label-D-n9s_wS.js";
import { t as useMountedRef } from "./useMountedRef-1omUd-IV.js";
import { b as ORCA_CLI_SKILL_NAME } from "./use-active-skill-discovery-runtime-target-CdctmJyj.js";
import { i as useInstalledAgentSkill, t as GLOBAL_AGENT_SKILL_SOURCE_KINDS } from "./useInstalledAgentSkills-BYdWqfUf.js";
import { l as ensureOrcaCliAvailableForAgentSkillTerminal, u as isOrcaCliAvailableOnPath } from "./CliSkillRuntimeSetup-BpNtfsr6.js";
var Import = createLucideIcon("import", [
	["path", {
		d: "M12 3v12",
		key: "1x0j5s"
	}],
	["path", {
		d: "m8 11 4 4 4-4",
		key: "1dohi6"
	}],
	["path", {
		d: "M8 5H4a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2h-4",
		key: "1ywtjm"
	}]
]);
function formatCookieImportWarning(warning) {
	switch (warning.code) {
		case "restart-fallback-unavailable": return warning.loadedCookies === 0 ? translate("auto.lib.browser.cookie.import.toast.restartFallbackUnavailableNone", "None of the {{value0}} cookies could be loaded, and the restart fallback was unavailable. The previous cookies for this profile were replaced. Try the import again.", { value0: warning.failedCookies }) : translate("auto.lib.browser.cookie.import.toast.restartFallbackUnavailablePartial", "Imported {{value0}} of {{value1}} cookies. The rest could not be loaded, and the restart fallback was unavailable. Try the import again.", {
			value0: warning.loadedCookies,
			value1: warning.loadedCookies + warning.failedCookies
		});
	}
}
function emitGoogleCookieImportWarning(summary, executionHostLabel) {
	if (!summary.googleCookiesSkipped) return;
	toast.warning(translate("auto.lib.browser.cookie.import.toast.googleCookiesSkipped", "Google cookies were not imported. Open a browser in Orca on {{value0}} with this profile, then sign into Google.", { value0: executionHostLabel }), { duration: 12e3 });
}
function emitBrowserCookieImportToast(summary, successMessage, executionHostLabel) {
	const warning = summary.warning;
	if (warning) toast.warning(formatCookieImportWarning(warning));
	else toast.success(successMessage);
	emitGoogleCookieImportWarning(summary, executionHostLabel);
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function BrowserCookieImportDisclosure() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuLabel, {
		className: "flex max-w-64 items-start gap-2 whitespace-normal py-2 font-normal",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Info, {
			"aria-hidden": true,
			className: "mt-0.5 size-3.5 shrink-0"
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
			className: "min-w-0",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "block font-medium leading-4 text-foreground",
				children: translate("auto.components.BrowserCookieImportDisclosure.title", "Google logins aren't imported")
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "block leading-4 text-muted-foreground",
				children: translate("auto.components.BrowserCookieImportDisclosure.description", "Sign in to Google directly in Orca.")
			})]
		})]
	})] });
}
var import_react = /* @__PURE__ */ __toESM(require_react());
function BrowserProfileUserAgentOption({ checked, onCheckedChange }) {
	const id = (0, import_react.useId)();
	const descriptionId = `${id}-description`;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex items-start gap-3 rounded-md border border-border/70 p-3",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Checkbox, {
			id,
			checked,
			onCheckedChange: (nextChecked) => onCheckedChange(nextChecked === true),
			"aria-describedby": descriptionId,
			className: "mt-0.5"
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "space-y-1",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
				htmlFor: id,
				className: "text-sm",
				children: translate("auto.components.browser.profile.user.agent.option.04af3dc12b", "Use unmodified user agent")
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				id: descriptionId,
				className: "text-xs text-muted-foreground",
				children: translate("auto.components.browser.profile.user.agent.option.5bf47a3c91", "May improve Google sign-in, but can reduce compatibility with bot-protected sites.")
			})]
		})]
	});
}
function StepBadge({ index, state }) {
	if (state === "done") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "flex size-6 shrink-0 items-center justify-center rounded-full bg-emerald-500/15 text-emerald-600 dark:text-emerald-400",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, { className: "size-3.5" })
	});
	if (state === "in-progress") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "flex size-6 shrink-0 items-center justify-center rounded-full bg-muted text-muted-foreground",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3.5 animate-spin" })
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "flex size-6 shrink-0 items-center justify-center rounded-full border border-border/70 text-xs font-medium text-muted-foreground",
		children: index
	});
}
function getMobileEmulatorCliPathNeedsAttention(status) {
	return status?.state === "installed" && status.pathConfigured === false;
}
function getMobileEmulatorCliStepBadgeState(input) {
	if (input.cliEnabled) return "done";
	if (input.cliBusy || input.cliPathNeedsAttention) return "in-progress";
	return "pending";
}
function shouldShowMobileEmulatorSkillPreInstallNotice(input) {
	return !input.cliSkillInstalled && !input.cliEnabled;
}
function getCliActionLabel(status, busy) {
	if (busy) return translate("auto.components.emulator.pane.use.mobile.emulator.agent.setup.state.fdcca1ec75", "Registering...");
	if (isOrcaCliAvailableOnPath(status)) return translate("auto.components.emulator.pane.use.mobile.emulator.agent.setup.state.69fb2c2289", "Enabled");
	if (status?.state === "installed") return translate("auto.components.emulator.pane.use.mobile.emulator.agent.setup.state.c6705092ba", "Fix PATH");
	return translate("auto.components.emulator.pane.use.mobile.emulator.agent.setup.state.7c1b6bdb1e", "Enable");
}
function useMobileEmulatorAgentSetupState(enabled = true) {
	const [cliInstallStatus, setCliInstallStatus] = (0, import_react.useState)(null);
	const [cliLoading, setCliLoading] = (0, import_react.useState)(true);
	const [cliBusy, setCliBusy] = (0, import_react.useState)(false);
	const [setupRechecking, setSetupRechecking] = (0, import_react.useState)(false);
	const mountedRef = useMountedRef();
	const { installed: cliSkillInstalled, loading: cliSkillLoading, error: cliSkillError, refresh: refreshCliSkill } = useInstalledAgentSkill(ORCA_CLI_SKILL_NAME, {
		enabled,
		sourceKinds: GLOBAL_AGENT_SKILL_SOURCE_KINDS
	});
	const refreshCliStatus = (0, import_react.useCallback)(async () => {
		setCliLoading(true);
		try {
			const status = await window.api.cli.getInstallStatus();
			if (mountedRef.current) setCliInstallStatus(status);
		} catch (error) {
			if (mountedRef.current) {
				toast.error(error instanceof Error ? error.message : translate("auto.components.emulator.pane.use.mobile.emulator.agent.setup.state.51074ccb05", "Failed to load CLI status."));
				setCliInstallStatus(null);
			}
		} finally {
			if (mountedRef.current) setCliLoading(false);
		}
	}, [mountedRef]);
	(0, import_react.useEffect)(() => {
		if (!enabled) return;
		refreshCliStatus();
	}, [enabled, refreshCliStatus]);
	(0, import_react.useEffect)(() => {
		if (!enabled) return;
		const handleFocus = () => {
			refreshCliStatus();
			refreshCliSkill();
		};
		window.addEventListener("focus", handleFocus);
		return () => window.removeEventListener("focus", handleFocus);
	}, [
		enabled,
		refreshCliSkill,
		refreshCliStatus
	]);
	const cliEnabled = isOrcaCliAvailableOnPath(cliInstallStatus);
	const cliPathNeedsAttention = getMobileEmulatorCliPathNeedsAttention(cliInstallStatus);
	const cliSupported = cliInstallStatus?.supported ?? false;
	const completedCount = [cliEnabled, cliSkillInstalled].filter(Boolean).length;
	const step2Blocked = !cliEnabled && !cliSkillInstalled;
	const setupComplete = cliEnabled && cliSkillInstalled;
	const statusReady = !cliLoading && !cliSkillLoading;
	const recheckSetup = (0, import_react.useCallback)(async () => {
		if (setupRechecking) return;
		setSetupRechecking(true);
		try {
			const [cliStatus, skillInstalled] = await Promise.all([window.api.cli.getInstallStatus(), refreshCliSkill()]);
			if (mountedRef.current) setCliInstallStatus(cliStatus);
			const cliReady = isOrcaCliAvailableOnPath(cliStatus);
			if (!mountedRef.current) return;
			if (cliReady && skillInstalled) {
				toast.success(translate("auto.components.emulator.pane.use.mobile.emulator.agent.setup.state.35dea1ae12", "Agent control is ready."));
				return;
			}
			if (skillInstalled) {
				toast.message(translate("auto.components.emulator.pane.use.mobile.emulator.agent.setup.state.9dff3a6338", "Skill is installed. Enable the Orca CLI to finish setup."));
				return;
			}
			if (cliReady) {
				toast.message(translate("auto.components.emulator.pane.use.mobile.emulator.agent.setup.state.15986a1080", "Orca CLI is ready. Install the skill to finish setup."));
				return;
			}
			toast.message(translate("auto.components.emulator.pane.use.mobile.emulator.agent.setup.state.4c26913def", "Still not set up. Complete both steps to enable agent control."));
		} catch (error) {
			if (mountedRef.current) toast.error(error instanceof Error ? error.message : translate("auto.components.emulator.pane.use.mobile.emulator.agent.setup.state.c94ff11e91", "Could not re-check setup status."));
		} finally {
			if (mountedRef.current) setSetupRechecking(false);
		}
	}, [
		mountedRef,
		refreshCliSkill,
		setupRechecking
	]);
	const handleEnableCli = (0, import_react.useCallback)(async () => {
		setCliBusy(true);
		try {
			const next = await ensureOrcaCliAvailableForAgentSkillTerminal({ onStatusChange: setCliInstallStatus });
			if (mountedRef.current && isOrcaCliAvailableOnPath(next)) toast.success(translate("auto.components.emulator.pane.use.mobile.emulator.agent.setup.state.2b519eed94", "Registered the Orca CLI in PATH."));
		} finally {
			if (mountedRef.current) setCliBusy(false);
		}
	}, [mountedRef]);
	return {
		cliActionLabel: getCliActionLabel(cliInstallStatus, cliBusy),
		cliBusy,
		cliEnabled,
		cliInstallStatus,
		cliPathNeedsAttention,
		cliLoading,
		cliSkillError,
		cliSkillInstalled,
		cliSkillLoading,
		cliSupported,
		completedCount,
		handleEnableCli,
		recheckSetup,
		refreshCliSkill,
		setupComplete,
		setupRechecking,
		statusReady,
		step2Blocked
	};
}
export { BrowserProfileUserAgentOption as a, Import as c, StepBadge as i, getMobileEmulatorCliStepBadgeState as n, BrowserCookieImportDisclosure as o, shouldShowMobileEmulatorSkillPreInstallNotice as r, emitBrowserCookieImportToast as s, useMobileEmulatorAgentSetupState as t };
