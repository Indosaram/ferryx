import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as Button } from "./button-DszXJEV6.js";
import "./workspace-status-wl52y3xd.js";
import { t as CircleCheck } from "./circle-check-CmH3uVJy.js";
import "./OnboardingInlineCommandTerminal-B5SNC4Sp.js";
import { t as EyeOff } from "./eye-off-4tMqi6LV.js";
import "./worktree-activation-BDsaiyMf.js";
import { t as Info } from "./info-D3uaWrfJ.js";
import "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import "./checkbox-PAbetBh2.js";
import "./context-menu-D4RKI7hR.js";
import "./dropdown-menu-Dth6LPK-.js";
import "./label-D-n9s_wS.js";
import "./popover-CgR1mzy7.js";
import "./select-B67U0C6J.js";
import "./switch-NhZdOYtg.js";
import "./toggle-CoxCWEA5.js";
import "./toggle-group-CZlhA2tW.js";
import { i as TooltipTrigger, n as TooltipContent, r as TooltipProvider, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
import "./useMountedRef-1omUd-IV.js";
import "./remote-runtime-pty-recovery-state-CcyktY20.js";
import "./selectors-XOBeaOSb.js";
import "./web-runtime-session-CN2syA39.js";
import "./agent-paste-draft-C2PA7vXu.js";
import "./agent-process-recognition-BB0O3DaN.js";
import "./terminal-pty-input-transaction-2UskR-Bm.js";
import "./web-session-tabs-sync-CYKZbAxS.js";
import "./pane-agent-owner-BPfoVAtS.js";
import "./native-chat-session-option-cache-DGE3h47U.js";
import "./github-links-C1M8w9wX.js";
import "./connection-context-BUPsamzR.js";
import "./localized-catalog-DubKHKUR.js";
import "./delete-worktree-flow-RxB6NScm.js";
import "./preserved-branch-batch-toast-DHxeGO1o.js";
import "./codex-session-restart-DLDE2Yzk.js";
import "./activate-tab-and-focus-pane-dvS5VCkm.js";
import "./terminal-appearance-D3oO-Ew5.js";
import "./stale-agent-row-D6vAh16E.js";
import "./terminal-tab-actions-BaU95skQ.js";
import "./agent-status-connection-ownership-D5nXPHBo.js";
import "./badge-BBptl5GG.js";
import "./command-D8Tw17HJ.js";
import "./RepoBadgeLabel-BMcVlWTu.js";
import "./useShortcutLabel-C-KRYtlB.js";
import "./ShortcutKeyCombo-Ch456Md0.js";
import "./feature-wall-setup-steps-D7ga1-7b.js";
import "./use-active-skill-discovery-runtime-target-CdctmJyj.js";
import "./useInstalledAgentSkills-BYdWqfUf.js";
import "./project-skill-runtime-BxhwlKnI.js";
import { a as DialogFooter, i as DialogDescription, o as DialogHeader, r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
import "./input-DV5rpysh.js";
import "./ime-composition-keyboard-event-HdRxQ6x2.js";
import "./lib-CtirWBBB.js";
import "./lib-D08jHVMa.js";
import "./purify.es-C_rn83UJ.js";
import "./MermaidBlock-gW3wAx0A.js";
import "./CommentMarkdown-bsrexQcY.js";
import "./ssh-connect-verb-CzMNDLCH.js";
import "./ssh-connect-in-flight-PkUz5iol.js";
import { c as AGENT_SKILL_CLI_PREREQUISITE_NOTICE, u as isOrcaCliAvailableOnPath } from "./CliSkillRuntimeSetup-BpNtfsr6.js";
import "./icons-jFAuHbv9.js";
import "./agent-catalog-CBF2CV5Q.js";
import "./crash-diagnostics-DaEtfKCs.js";
import "./workspace-file-drag-BnROcEA_.js";
import "./use-system-prefers-dark-QSo6mmSW.js";
import "./skill-freshness-_9VFY729.js";
import "./AgentCombobox-pxD8XmwH.js";
import "./text-control-paste-PhBVbE2p.js";
import "./paste-payload-metadata-pr3nuODB.js";
import "./ssh-mutation-expectation-BCkNAxVH.js";
import "./primary-selection-BsidtYsF.js";
import "./file-search-selection-D5svLyqM.js";
import "./terminal-shortcut-policy-BOkUsz_T.js";
import "./launch-agent-in-new-tab-44JGNfKl.js";
import "./workspace-browser-tab-open-aiO4ahIP.js";
import "./useDaemonActions-Dxlrlfjm.js";
import "./find-query-bounds-BKNiI6IV.js";
import "./feature-education-telemetry-DPRGAVBD.js";
import "./terminal-keyboard-protocol-De0UZ6qG.js";
import "./AgentSessionContinuationDialog-DJoQpqjB.js";
import { t as IntegrationStatusPill } from "./integration-status-pill-CCHE4njv.js";
import { t as AgentSkillSetupPanel } from "./AgentSkillSetupPanel-C5ndTiLM.js";
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function LinearAgentSkillSetupDialog({ open, showSuccess, successDescription, missingLabel, command, installedCommand, terminalShellOverride, terminalRuntime, installed, loading, error, getPrerequisiteStatus, onBeforeOpenTerminal, onRecheck, onOpenChange, onDismissPermanently, onDone }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
		open,
		onOpenChange,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogContent, {
			className: "gap-0 overflow-hidden p-0 sm:max-w-[640px]",
			children: showSuccess ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "px-6 pt-6 pr-14",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, {
					className: "gap-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, { children: translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.successTitle", "Linear ticket access is ready") }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, { children: successDescription })]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "mt-4 flex items-center gap-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CircleCheck, { className: "size-4 shrink-0 text-muted-foreground" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(IntegrationStatusPill, {
						tone: "connected",
						children: translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.successStatus", "Linear ticket access ready")
					})]
				})]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogFooter, {
				className: "px-6 pt-5 pb-6",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					type: "button",
					size: "sm",
					onClick: onDone,
					children: translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.done", "Done")
				})
			})] }) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "px-6 pt-6 pr-20",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, {
						className: "sr-only",
						children: translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.modalTitle", "Enable Linear ticket access")
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, {
						className: "sr-only",
						children: translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.modalDescription", "Install the Linear skill from a terminal.")
					})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-start gap-2 text-base font-semibold leading-snug text-foreground",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Info, { className: "mt-0.5 size-4 shrink-0 text-muted-foreground" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", { children: translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.modalPrompt", "Enable agents to read and edit the attached Linear ticket.") })]
					})]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentSkillSetupPanel, {
					className: "px-6 pt-4 pb-6",
					variant: "inline",
					hideHeader: true,
					title: translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.modalTitle", "Enable Linear ticket access"),
					description: missingLabel,
					command,
					installedCommand,
					terminalTitle: translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.terminalTitle", "Install Linear agent skill"),
					terminalAriaLabel: translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.terminalAria", "Linear agent skill installer terminal"),
					terminalWorktreeId: "sidebar-linear-agent-skill-setup",
					terminalHeightPx: 240,
					terminalShellOverride,
					terminalRuntime,
					installed,
					loading,
					error,
					installLabel: translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.install", "Install CLI & Skill"),
					installVariant: "default",
					preInstallNotice: AGENT_SKILL_CLI_PREREQUISITE_NOTICE,
					getPrerequisiteStatus,
					isPrerequisiteAvailable: isOrcaCliAvailableOnPath,
					onBeforeOpenTerminal,
					onRecheck
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipProvider, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
					asChild: true,
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						type: "button",
						variant: "ghost",
						size: "icon-xs",
						"aria-label": translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.dontShowAgain", "Don't show again"),
						onClick: onDismissPermanently,
						className: "absolute top-3 right-10 text-muted-foreground",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(EyeOff, { className: "size-4" })
					})
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
					side: "top",
					sideOffset: 4,
					children: translate("auto.components.sidebar.LinearAgentSkillSetupPrompt.dontShowAgain", "Don't show again")
				})] }) })
			] })
		})
	});
}
var LinearAgentSkillSetupDialog_default = LinearAgentSkillSetupDialog;
export { LinearAgentSkillSetupDialog, LinearAgentSkillSetupDialog_default as default };
