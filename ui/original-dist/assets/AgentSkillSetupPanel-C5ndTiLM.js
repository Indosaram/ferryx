import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn, t as Button } from "./button-DszXJEV6.js";
import { t as ChevronRight } from "./chevron-right-CZtMe6Ev.js";
import { t as OnboardingInlineCommandTerminal } from "./OnboardingInlineCommandTerminal-B5SNC4Sp.js";
import { t as Copy } from "./copy-jk2iqVkp.js";
import { t as LoaderCircle } from "./loader-circle-CRZpWdsi.js";
import { t as RefreshCw } from "./refresh-cw-BU_ChOig.js";
import { t as Terminal } from "./terminal-Cen7Un9b.js";
import { t as TriangleAlert } from "./triangle-alert-HrLt1y9s.js";
import { n as toast } from "./dist-DgqligFk.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
import { t as useMountedRef } from "./useMountedRef-1omUd-IV.js";
import { r as notifyInstalledAgentSkillsRefreshed } from "./useInstalledAgentSkills-BYdWqfUf.js";
import { n as buildSkillSetupTerminalCommand, u as isOrcaCliAvailableOnPath } from "./CliSkillRuntimeSetup-BpNtfsr6.js";
import { c as useSkillFreshness, i as isSkillScanIssueNeedingAttention, o as skillPlacementParticipatesInGlobalFreshness, r as isSkillCopyNeedingAttention, s as refreshSkillFreshness } from "./skill-freshness-_9VFY729.js";
import { r as requestSkillFreshnessUpdateDialog } from "./skill-freshness-update-dialog-Bw6DvImT.js";
import { t as IntegrationStatusPill } from "./integration-status-pill-CCHE4njv.js";
function getSkillFreshnessDisplayStatus(inventory, skillName) {
	if (inventory?.eligibleUpdateNames.includes(skillName)) return "update-available";
	let hasPlacement = false;
	let hasBlockedCopy = false;
	for (const installation of inventory?.installations ?? []) {
		if (installation.name !== skillName) continue;
		if (!skillPlacementParticipatesInGlobalFreshness(installation)) continue;
		hasPlacement = true;
		if (installation.status !== "current" && installation.status !== "newer-known" && !(installation.status === "unrecognized" && installation.topology === "plugin-cache")) hasBlockedCopy = true;
	}
	if (!hasPlacement) return "installed";
	if (inventory?.scanIssues.some(isSkillScanIssueNeedingAttention)) return "needs-attention";
	return hasBlockedCopy ? "needs-attention" : "up-to-date";
}
function hasSkillCopyNeedingAttention(inventory, skillName) {
	const placements = (inventory?.installations ?? []).filter((installation) => installation.name === skillName && skillPlacementParticipatesInGlobalFreshness(installation));
	return placements.length > 0 && Boolean(inventory?.scanIssues.some(isSkillScanIssueNeedingAttention)) || placements.some(isSkillCopyNeedingAttention);
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function statusPill(status) {
	if (status === "update-available") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(IntegrationStatusPill, {
		tone: "attention",
		children: translate("auto.components.skills.SkillFreshnessStatusPill.updateAvailable", "Update available")
	});
	if (status === "needs-attention") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(IntegrationStatusPill, {
		tone: "attention",
		children: translate("auto.components.skills.SkillFreshnessStatusPill.needsAttention", "Review skill")
	});
	if (status === "up-to-date") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(IntegrationStatusPill, {
		tone: "connected",
		children: translate("auto.components.skills.SkillFreshnessStatusPill.upToDate", "Up to date")
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(IntegrationStatusPill, {
		tone: "connected",
		children: translate("auto.components.skills.SkillFreshnessStatusPill.installed", "Installed")
	});
}
function SkillFreshnessStatusPill({ skillName }) {
	const { inventory, loading, error } = useSkillFreshness();
	if (loading && !inventory) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(IntegrationStatusPill, {
		tone: "neutral",
		children: translate("auto.components.skills.SkillFreshnessStatusPill.checking", "Checking...")
	});
	if (error && !inventory) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(IntegrationStatusPill, {
		tone: "attention",
		children: translate("auto.components.skills.SkillFreshnessStatusPill.checkFailed", "Check failed")
	});
	const status = getSkillFreshnessDisplayStatus(inventory, skillName);
	const hasDetails = status === "update-available" || status === "needs-attention";
	const needsAttention = hasSkillCopyNeedingAttention(inventory, skillName);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
		className: "inline-flex items-center gap-2",
		children: [statusPill(status), hasDetails ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
			variant: "ghost",
			size: "xs",
			className: cn("gap-1 px-1.5 text-[11px]", needsAttention && "text-amber-500 hover:text-amber-500"),
			onClick: () => requestSkillFreshnessUpdateDialog(),
			children: [
				needsAttention ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "size-3" }) : null,
				translate("auto.components.skills.SkillFreshnessStatusPill.details", "Details"),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronRight, { className: "size-3" })
			]
		}) : null]
	});
}
function AgentSkillSetupFailureNotice(props) {
	if (props.exitCode === null) return null;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
		className: "mt-2 text-[12px] leading-snug text-destructive",
		children: translate("auto.components.settings.AgentSkillSetupPanel.setupCommandFailed", "The setup command exited with code {{value0}}. This error will clear after a successful retry.", { value0: props.exitCode })
	});
}
function createTerminalSnapshot(copiedCommand, shellOverride, runtime) {
	const pinnedRuntime = runtime ? { ...runtime } : void 0;
	return {
		copiedCommand,
		prepareCommandForShell: (command, effectiveShell) => buildSkillSetupTerminalCommand(command, effectiveShell, pinnedRuntime),
		shellOverride
	};
}
function syncSurfacesAfterAgentSkillRecheck(freshnessSkillName) {
	notifyInstalledAgentSkillsRefreshed();
	if (freshnessSkillName) refreshSkillFreshness();
}
function recheckSurfacesAfterAgentSkillTerminal(onRecheck, freshnessSkillName) {
	Promise.resolve(onRecheck()).then(() => {
		syncSurfacesAfterAgentSkillRecheck(freshnessSkillName);
	});
}
var import_react = /* @__PURE__ */ __toESM(require_react());
function AgentSkillSetupPanel({ title, description, command, installedCommand, terminalTitle, terminalAriaLabel, terminalWorktreeId, installed, loading, error, installDisabled = false, terminalHeightPx, terminalShellOverride: shellOverride, terminalRuntime: runtime, leading, icon, variant = "card", className, hideHeader = false, preInstallNotice, getPrerequisiteStatus, isPrerequisiteAvailable = isOrcaCliAvailableOnPath, onBeforeOpenTerminal, showInstallWhenInstalled = true, showRecheckWhenInstalled = true, installLabel, installedInstallLabel, installVariant = "outline", actionHint, openingHint, footer, onRecheck, freshnessSkillName }) {
	const resolvedInstallLabel = installLabel ?? translate("auto.components.settings.AgentSkillSetupPanel.installLabel", "Install");
	const resolvedInstalledInstallLabel = installedInstallLabel ?? translate("auto.components.settings.AgentSkillSetupPanel.updateLabel", "Update");
	const [terminalOpen, setTerminalOpen] = (0, import_react.useState)(false);
	const [terminalSnapshot, setTerminalSnapshot] = (0, import_react.useState)(null);
	const [terminalAttempt, setTerminalAttempt] = (0, import_react.useState)(0);
	const [terminalOpening, setTerminalOpening] = (0, import_react.useState)(false);
	const [setupAttemptRunning, setSetupAttemptRunning] = (0, import_react.useState)(false);
	const [setupCommandFailedCode, setSetupCommandFailedCode] = (0, import_react.useState)(null);
	const setupAttemptRunningRef = (0, import_react.useRef)(false);
	const [preInstallNoticeVisible, setPreInstallNoticeVisible] = (0, import_react.useState)(Boolean(preInstallNotice && !installed));
	const mountedRef = useMountedRef();
	const readPrerequisiteStatus = (0, import_react.useCallback)(() => (getPrerequisiteStatus ?? window.api.cli.getInstallStatus)(), [getPrerequisiteStatus]);
	const activeCommand = installed ? installedCommand ?? command : command;
	const openTerminalCommand = terminalSnapshot?.copiedCommand ?? activeCommand;
	const openSetupTerminal = () => {
		if (terminalOpening || setupAttemptRunning) return;
		const nextSnapshot = createTerminalSnapshot(activeCommand, shellOverride, runtime);
		setTerminalOpening(true);
		if (setupCommandFailedCode !== null) setTerminalOpen(false);
		(async () => {
			let shouldOpenTerminal = false;
			try {
				await onBeforeOpenTerminal?.();
				await refreshPreInstallNotice();
				shouldOpenTerminal = true;
			} catch {
				shouldOpenTerminal = false;
			} finally {
				if (mountedRef.current) {
					setTerminalOpening(false);
					if (shouldOpenTerminal) {
						setTerminalSnapshot(nextSnapshot);
						setTerminalAttempt((attempt) => attempt + 1);
						setTerminalOpen(true);
						setupAttemptRunningRef.current = true;
						setSetupAttemptRunning(true);
					}
				}
			}
		})();
	};
	const handleSetupCommandFinished = (0, import_react.useCallback)((bestEffortExitCode) => {
		if (!setupAttemptRunningRef.current) return;
		setupAttemptRunningRef.current = false;
		setSetupAttemptRunning(false);
		if (bestEffortExitCode !== null) setSetupCommandFailedCode(bestEffortExitCode === 0 ? null : bestEffortExitCode);
		recheckSurfacesAfterAgentSkillTerminal(onRecheck, freshnessSkillName);
	}, [freshnessSkillName, onRecheck]);
	const handleTerminalExit = (0, import_react.useCallback)(() => {
		const shouldRecheck = setupAttemptRunningRef.current;
		if (mountedRef.current) {
			setupAttemptRunningRef.current = false;
			setTerminalOpen(false);
			setSetupAttemptRunning(false);
		}
		shouldRecheck && recheckSurfacesAfterAgentSkillTerminal(onRecheck, freshnessSkillName);
	}, [
		freshnessSkillName,
		mountedRef,
		onRecheck
	]);
	(0, import_react.useEffect)(() => {
		if (!preInstallNotice) {
			setPreInstallNoticeVisible(false);
			return;
		}
		let canceled = false;
		const refreshCliNotice = async () => {
			try {
				const status = await readPrerequisiteStatus();
				if (!canceled) setPreInstallNoticeVisible(!isPrerequisiteAvailable(status));
			} catch {
				if (!canceled) setPreInstallNoticeVisible(true);
			}
		};
		refreshCliNotice();
		window.addEventListener("focus", refreshCliNotice);
		return () => {
			canceled = true;
			window.removeEventListener("focus", refreshCliNotice);
		};
	}, [
		isPrerequisiteAvailable,
		preInstallNotice,
		readPrerequisiteStatus
	]);
	const refreshPreInstallNotice = async () => {
		if (!preInstallNotice) return;
		try {
			const status = await readPrerequisiteStatus();
			if (mountedRef.current) setPreInstallNoticeVisible(!isPrerequisiteAvailable(status));
		} catch {
			if (mountedRef.current) setPreInstallNoticeVisible(true);
		}
	};
	const copyActiveCommand = async () => {
		try {
			await window.api.ui.writeClipboardText(openTerminalCommand);
			toast.success(translate("auto.components.settings.AgentSkillSetupPanel.copiedCommand", "Copied command."));
		} catch (error$1) {
			toast.error(error$1 instanceof Error ? error$1.message : translate("auto.components.settings.AgentSkillSetupPanel.failedToCopyCommand", "Failed to copy command."));
		}
	};
	const actionRow = /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "mt-3 flex flex-wrap items-center gap-2",
		children: [
			(!installed || showInstallWhenInstalled) && setupCommandFailedCode === null ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
				type: "button",
				variant: installVariant,
				size: "sm",
				onClick: openSetupTerminal,
				disabled: terminalOpen || installDisabled || terminalOpening,
				children: [terminalOpening ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3.5 animate-spin" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Terminal, { className: "size-3.5" }), terminalOpening ? translate("auto.components.settings.AgentSkillSetupPanel.5f818f12ab", "Preparing...") : installed ? resolvedInstalledInstallLabel : resolvedInstallLabel]
			}) : null,
			setupCommandFailedCode !== null || !installed || showRecheckWhenInstalled ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
				type: "button",
				variant: "ghost",
				size: "sm",
				className: "gap-1.5",
				onClick: () => {
					if (setupCommandFailedCode !== null) {
						openSetupTerminal();
						return;
					}
					Promise.resolve(onRecheck()).then(() => {
						syncSurfacesAfterAgentSkillRecheck(freshnessSkillName);
					});
				},
				disabled: setupCommandFailedCode !== null ? installDisabled || terminalOpening || setupAttemptRunning : loading,
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RefreshCw, { className: cn("size-3.5", (loading || terminalOpening) && "animate-spin") }), setupCommandFailedCode !== null ? translate("auto.components.settings.AgentSkillSetupPanel.retrySetup", "Retry") : translate("auto.components.settings.AgentSkillSetupPanel.c689392435", "Re-check")]
			}) : null,
			terminalOpening ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				className: "basis-full text-[12px] leading-snug text-muted-foreground",
				children: openingHint ?? translate("auto.components.settings.AgentSkillSetupPanel.4c05b9d7cb", "Preparing setup terminal.")
			}) : null
		]
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: cn("min-w-0", variant === "card" ? "rounded-xl border border-border bg-muted/20" : null, className),
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: variant === "card" ? cn("px-5 pt-5", terminalOpen ? "pb-2" : "pb-5") : "pt-1.5",
			children: [
				hideHeader ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [error ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "text-[12px] text-destructive",
					children: error
				}) : null, installed && freshnessSkillName && setupCommandFailedCode === null ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "mb-2",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SkillFreshnessStatusPill, { skillName: freshnessSkillName })
				}) : null] }) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-center gap-4",
					children: [
						leading,
						icon ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "flex size-10 shrink-0 items-center justify-center rounded-lg border border-border bg-background text-foreground",
							children: icon
						}) : null,
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "min-w-0 flex-1 self-center",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "flex flex-wrap items-center gap-x-3 gap-y-1",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h3", {
									className: "text-[15px] font-semibold leading-tight text-foreground",
									children: title
								}), setupCommandFailedCode !== null ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(IntegrationStatusPill, {
									tone: "attention",
									children: translate("auto.components.settings.AgentSkillSetupPanel.setupFailed", "Setup failed")
								}) : loading && !installed ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(IntegrationStatusPill, {
									tone: "neutral",
									children: translate("auto.components.settings.AgentSkillSetupPanel.68a468752e", "Checking...")
								}) : installed ? freshnessSkillName ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SkillFreshnessStatusPill, { skillName: freshnessSkillName }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(IntegrationStatusPill, {
									tone: "connected",
									children: translate("auto.components.settings.AgentSkillSetupPanel.9fcebceb2a", "Installed")
								}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(IntegrationStatusPill, {
									tone: "attention",
									children: translate("auto.components.settings.AgentSkillSetupPanel.5289300939", "Not installed")
								})]
							}), error ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "mt-1 text-[12px] text-destructive",
								children: error
							}) : null]
						})
					]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: cn("max-w-none", hideHeader ? null : "mt-3"),
					children: [
						description != null ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "text-[13px] leading-snug text-muted-foreground",
							children: description
						}) : null,
						actionRow,
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentSkillSetupFailureNotice, { exitCode: setupCommandFailedCode }),
						actionHint ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mt-2",
							children: actionHint
						}) : null,
						!installed && preInstallNotice && preInstallNoticeVisible ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "mt-3 text-[12px] leading-snug text-muted-foreground",
							children: preInstallNotice
						}) : null
					]
				}),
				footer ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: cn("border-t border-border/60", terminalOpen ? "mt-2 pt-4" : "mt-5 pt-5"),
					children: footer
				}) : null
			]
		}), terminalOpen && terminalSnapshot ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: cn("min-w-0 max-w-full overflow-hidden", variant === "card" ? "px-5 pb-5" : "mt-2"),
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex min-w-0 max-w-full items-center gap-2 overflow-hidden rounded-md border border-border bg-muted/35 px-3 py-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("code", {
					className: "scrollbar-sleek min-w-0 flex-1 overflow-x-auto whitespace-nowrap font-mono text-xs text-muted-foreground",
					children: openTerminalCommand
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
					asChild: true,
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						type: "button",
						variant: "ghost",
						size: "icon-sm",
						className: "shrink-0",
						"aria-label": translate("auto.components.settings.AgentSkillSetupPanel.copyCommandAria", "Copy command"),
						onClick: () => void copyActiveCommand(),
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Copy, { className: "size-4" })
					})
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
					side: "top",
					sideOffset: 4,
					children: translate("auto.components.settings.AgentSkillSetupPanel.ed197f59a2", "Copy command")
				})] })]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(OnboardingInlineCommandTerminal, {
				worktreeId: terminalWorktreeId,
				command: openTerminalCommand,
				prepareCommandForShell: terminalSnapshot.prepareCommandForShell,
				title: terminalTitle,
				description: translate("auto.components.settings.AgentSkillSetupPanel.runCommandDescription", "Press Enter to run the command."),
				ariaLabel: terminalAriaLabel,
				terminalHeightPx,
				shellOverride: terminalSnapshot.shellOverride,
				terminalTopMarginPx: 8,
				descriptionPaddingClassName: "px-4 py-2",
				autoScrollIntoView: false,
				onTerminalExit: handleTerminalExit,
				onCommandFinished: handleSetupCommandFinished
			}, terminalAttempt)]
		}) : null]
	});
}
export { SkillFreshnessStatusPill as n, getSkillFreshnessDisplayStatus as r, AgentSkillSetupPanel as t };
