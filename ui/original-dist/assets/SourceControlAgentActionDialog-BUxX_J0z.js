import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn, t as Button } from "./button-DszXJEV6.js";
import { t as CircleCheck } from "./circle-check-CmH3uVJy.js";
import { t as Info } from "./info-D3uaWrfJ.js";
import { t as RefreshCw } from "./refresh-cw-BU_ChOig.js";
import { t as RotateCcw } from "./rotate-ccw-hK0RKgaG.js";
import { t as Settings } from "./settings-BX3azETW.js";
import { t as Sparkles } from "./sparkles-DTr27w4B.js";
import { $d as buildAgentDraftLaunchPlan, Ah as normalizeSourceControlAiSettings, Fh as DEFAULT_SOURCE_CONTROL_ACTION_COMMAND_TEMPLATES, Ip as isWindowsAbsolutePathLike, Jh as isCustomAgentId, Ld as resolveInitialNativeChatSessionOptions, Oa as focusTerminalTabSurface, Vp as isWslUncPath, cc as resolveLocalWindowsAgentStartupShell, eg as isTuiAgentEnabled, jh as resolveSourceControlActionRecipe, kh as normalizeRepoSourceControlAiOverrides, t as useAppStore, tf as buildAgentStartupPlan, uf as planAgentCliArgsSuffix, zh as renderSourceControlActionCommandTemplate } from "./store-CgXrfmaH.js";
import { t as TriangleAlert } from "./triangle-alert-HrLt1y9s.js";
import { dt as TUI_AGENT_CONFIG } from "./agent-status-3vUKbY6l.js";
import { n as toast } from "./dist-DgqligFk.js";
import { t as Label } from "./label-D-n9s_wS.js";
import { a as SelectTrigger, n as SelectContent, o as SelectValue, r as SelectItem, t as Select } from "./select-B67U0C6J.js";
import { f as useRepoById } from "./selectors-XOBeaOSb.js";
import { m as CLIENT_PLATFORM } from "./native-chat-session-option-cache-DGE3h47U.js";
import { a as DialogFooter, i as DialogDescription, o as DialogHeader, r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
import { t as Input } from "./input-DV5rpysh.js";
import { n as getAgentCatalog } from "./agent-catalog-CBF2CV5Q.js";
import { t as AgentCombobox } from "./AgentCombobox-pxD8XmwH.js";
import { t as launchAgentInNewTab } from "./launch-agent-in-new-tab-44JGNfKl.js";
import { a as pickSourceControlLaunchAgent, r as SourceControlActionVariableChips, s as resolveSourceControlLaunchAgentScope } from "./source-control-ai-recipe-save-CCtetPLa.js";
function resolveSourceControlLaunchPlatform(args) {
	const path = args.worktreePath?.trim() ?? "";
	if (typeof args.connectionId === "string") return path && isWindowsAbsolutePathLike(path) && !isWslUncPath(path) ? "win32" : "linux";
	if (args.projectRuntime?.status === "repair-required") return args.projectRuntime.repair.preferredRuntime.kind === "wsl" ? "linux" : CLIENT_PLATFORM;
	if (args.projectRuntime?.status === "resolved" && args.projectRuntime.runtime.kind === "wsl") return "linux";
	if (path && isWslUncPath(path)) return "linux";
	return CLIENT_PLATFORM;
}
function normalizeSourceControlActionRecipeForComparison(actionId, recipe) {
	return {
		agentId: recipe?.agentId ?? null,
		commandInputTemplate: typeof recipe?.commandInputTemplate === "string" ? recipe.commandInputTemplate.trim() : DEFAULT_SOURCE_CONTROL_ACTION_COMMAND_TEMPLATES[actionId],
		agentArgs: typeof recipe?.agentArgs === "string" ? recipe.agentArgs.trim() : ""
	};
}
function sourceControlActionRecipesMatch(left, right) {
	return left.agentId === right.agentId && left.commandInputTemplate === right.commandInputTemplate && left.agentArgs === right.agentArgs;
}
function readSavedSourceControlActionRecipeAtTarget(input) {
	if (input.target.type === "repo") {
		if (!normalizeRepoSourceControlAiOverrides(input.repo?.sourceControlAi)?.actionOverrides?.[input.actionId]) return null;
		return resolveSourceControlActionRecipe({
			actionId: input.actionId,
			settings: input.settings,
			repo: input.repo
		});
	}
	return normalizeSourceControlAiSettings(input.settings?.sourceControlAi, input.settings?.commitMessageAi).actions?.[input.actionId] ?? null;
}
function readSavedCustomAgentCommandAtTarget(input) {
	if (input.target.type === "repo") return normalizeRepoSourceControlAiOverrides(input.repo?.sourceControlAi)?.customAgentCommand?.trim() ?? "";
	return normalizeSourceControlAiSettings(input.settings?.sourceControlAi, input.settings?.commitMessageAi).customAgentCommand.trim();
}
function sourceControlActionRecipeMatchesTarget(input) {
	const savedRecipe = readSavedSourceControlActionRecipeAtTarget(input);
	if (!savedRecipe) return false;
	const current = normalizeSourceControlActionRecipeForComparison(input.actionId, input.recipe);
	if (!sourceControlActionRecipesMatch(current, normalizeSourceControlActionRecipeForComparison(input.actionId, savedRecipe))) return false;
	if (!isCustomAgentId(current.agentId)) return true;
	return (input.customAgentCommand ?? "").trim() === readSavedCustomAgentCommandAtTarget(input);
}
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function sourceControlLaunchSaveTargetFromValue(value, repo) {
	if (value === "repo" && repo?.id) return {
		type: "repo",
		repoId: repo.id
	};
	if (value === "global") return { type: "global" };
	return null;
}
function SourceControlAgentActionDialogForm({ actionId, baseCommandInput, agentScopeNote, agentOptions, selectedAgent, hasEnabledAgents, detecting, statusCopy, agentArgs, commandTemplate, savedCommandInputTemplate, saveLaunchRecipe, saveTargetValue, saveTargets, settings, repo, canSaveAgentDefault, deliveryPlan, canStart, isStarting, startLabel, onSelectedAgentChange, onAgentArgsChange, onCommandTemplateChange, onSaveLaunchRecipeChange, onSaveAgentDefaultChange, onOpenSettings, onCancel, onStart }) {
	const defaultCommandTemplate = savedCommandInputTemplate ?? "{basePrompt}";
	const commandTemplateIncludesBasePrompt = commandTemplate.includes("{basePrompt}");
	const selectedRecipe = selectedAgent ? {
		agentId: selectedAgent,
		commandInputTemplate: commandTemplate,
		agentArgs
	} : null;
	const selectedSaveTarget = sourceControlLaunchSaveTargetFromValue(saveTargetValue, repo);
	const selectedLaunchRecipeAlreadySaved = Boolean(selectedRecipe && selectedSaveTarget && sourceControlActionRecipeMatchesTarget({
		actionId,
		target: selectedSaveTarget,
		recipe: selectedRecipe,
		settings,
		repo
	}));
	const showSaveLaunchRecipe = Boolean(canSaveAgentDefault && selectedAgent);
	const saveScopeTargets = saveTargets.filter((target) => target.value !== "none");
	const effectiveStartLabel = showSaveLaunchRecipe && saveLaunchRecipe && !selectedLaunchRecipeAlreadySaved ? translate("auto.components.right.sidebar.SourceControlAgentActionDialogForm.5421a96acb", "Save & start agent") : startLabel;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex min-h-0 flex-col gap-4",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "min-h-0 min-w-0 max-h-[min(60vh,31rem)] space-y-4 overflow-y-auto pr-1 scrollbar-sleek",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "space-y-2",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
							className: "text-xs",
							children: translate("auto.components.right.sidebar.SourceControlAgentActionDialogForm.15c5d85706", "Agent")
						}),
						hasEnabledAgents || selectedAgent ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentCombobox, {
							agents: agentOptions,
							value: selectedAgent,
							onValueChange: onSelectedAgentChange,
							allowNarrowTrigger: true,
							triggerClassName: "w-full"
						}) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex items-center justify-between gap-3 rounded-md border border-border bg-muted/30 px-3 py-2 text-xs text-muted-foreground",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: detecting ? translate("auto.components.right.sidebar.SourceControlAgentActionDialogForm.c7ff8cef11", "Detecting agents...") : translate("auto.components.right.sidebar.SourceControlAgentActionDialogForm.1d47db9bf0", "No enabled agents") }), onOpenSettings ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
								type: "button",
								variant: "ghost",
								size: "xs",
								onClick: onOpenSettings,
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Settings, { className: "size-3.5" }), translate("auto.components.right.sidebar.SourceControlAgentActionDialogForm.b99c33cec5", "Settings")]
							}) : null]
						}),
						statusCopy ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
							className: "flex items-start gap-1.5 text-[11px] text-destructive",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "mt-px size-3 shrink-0" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: statusCopy })]
						}) : null
					]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "space-y-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
						htmlFor: "source-control-agent-cli-args",
						className: "text-xs",
						children: translate("auto.components.right.sidebar.SourceControlAgentActionDialogForm.bc8dc39f4b", "CLI arguments")
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
						id: "source-control-agent-cli-args",
						value: agentArgs,
						spellCheck: false,
						placeholder: translate("auto.components.right.sidebar.SourceControlAgentActionDialogForm.fe119187bb", "--model sonnet"),
						onChange: (event) => onAgentArgsChange(event.target.value),
						className: "h-8 font-mono text-xs"
					})]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "space-y-2",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex items-start justify-between gap-3",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "min-w-0",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
									htmlFor: "source-control-agent-command-input",
									className: "text-xs",
									children: translate("auto.components.right.sidebar.SourceControlAgentActionDialogForm.f4f3c9ca4a", "Prompt template")
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
									className: "mt-1 text-[11px] leading-4 text-muted-foreground",
									children: translate("auto.components.right.sidebar.SourceControlAgentActionDialogForm.5c75b24735", "Customize what the agent receives before Orca starts it.")
								})]
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
								type: "button",
								variant: "ghost",
								size: "xs",
								disabled: commandTemplate === defaultCommandTemplate,
								onClick: () => onCommandTemplateChange(defaultCommandTemplate),
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RotateCcw, { className: "size-3.5" }), translate("auto.components.right.sidebar.SourceControlAgentActionDialogForm.7ec6abbf2a", "Reset")]
							})]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("textarea", {
							id: "source-control-agent-command-input",
							rows: 7,
							value: commandTemplate,
							onChange: (event) => onCommandTemplateChange(event.target.value),
							className: "box-border min-h-[6.5rem] min-w-0 w-full max-w-full resize-y rounded-md border border-border bg-background px-2.5 py-2 font-mono text-xs text-foreground outline-none placeholder:text-muted-foreground/70 focus-visible:ring-1 focus-visible:ring-ring",
							spellCheck: false
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SourceControlActionVariableChips, {
							actionId,
							variablePreviews: { basePrompt: baseCommandInput },
							onInsert: (variable) => {
								onCommandTemplateChange(`${commandTemplate}${commandTemplate.endsWith("\n") || commandTemplate.length === 0 ? "" : " "}{${variable}}`);
							}
						}),
						!commandTemplateIncludesBasePrompt ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
							className: "flex items-start gap-1.5 rounded-md border border-destructive/30 bg-destructive/5 px-2.5 py-2 text-[11px] leading-4 text-destructive",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "mt-px size-3 shrink-0" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.right.sidebar.SourceControlAgentActionDialogForm.23280cbab1", "This template does not include {basePrompt}, so the agent will not receive Orca's default prompt.") })]
						}) : null
					]
				}),
				showSaveLaunchRecipe && agentScopeNote ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-start gap-1.5 rounded-md border border-border bg-muted/30 px-2.5 py-2 text-[11px] leading-4 text-muted-foreground",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Info, { className: "mt-px size-3 shrink-0" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.right.sidebar.SourceControlAgentActionDialogForm.repoAgentOverrideNote", "This repository overrides your global default ({{global}}) and currently runs {{effective}}. Save to this repository to change what runs here.", {
						effective: agentScopeNote.effectiveAgentLabel,
						global: agentScopeNote.globalAgentLabel
					}) })]
				}) : null,
				showSaveLaunchRecipe ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: cn("space-y-2 rounded-md border border-border bg-background p-3", saveLaunchRecipe && "border-foreground shadow-[inset_0_0_0_1px_var(--foreground)]"),
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("label", {
						className: "grid cursor-pointer grid-cols-[1rem_1fr] items-start gap-2.5",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("input", {
							type: "checkbox",
							checked: saveLaunchRecipe,
							onChange: (event) => onSaveLaunchRecipeChange(event.target.checked),
							className: "mt-0.5 size-3.5 accent-foreground"
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "block text-xs font-semibold",
							children: selectedLaunchRecipeAlreadySaved ? translate("auto.components.right.sidebar.SourceControlAgentActionDialogForm.b0da3a4d3e", "Launch recipe already saved") : translate("auto.components.right.sidebar.SourceControlAgentActionDialogForm.c29f9cf266", "Save this prompt and don't show this review next time")
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "mt-0.5 block text-[11px] leading-4 text-muted-foreground",
							children: selectedLaunchRecipeAlreadySaved ? translate("auto.components.right.sidebar.SourceControlAgentActionDialogForm.bff4795a6d", "Change the agent, arguments, or prompt template to update the saved recipe.") : translate("auto.components.right.sidebar.SourceControlAgentActionDialogForm.6cefcdfba1", "You can change it later in Source Control AI settings.")
						})] })]
					}), saveLaunchRecipe ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "grid grid-cols-[5.5rem_1fr] items-center gap-2 border-t border-border pt-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-[11px] text-muted-foreground",
							children: translate("auto.components.right.sidebar.SourceControlAgentActionDialogForm.013c9ac04a", "Save for")
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Select, {
							value: saveTargetValue,
							onValueChange: onSaveAgentDefaultChange,
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SelectTrigger, {
								size: "sm",
								className: "h-8 w-full text-xs",
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SelectValue, {})
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SelectContent, { children: saveScopeTargets.map((target) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SelectItem, {
								value: target.value,
								children: target.label
							}, target.value)) })]
						})]
					}) : null]
				}) : null,
				deliveryPlan.status !== "idle" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: cn("rounded-md border px-3 py-2 text-xs", deliveryPlan.status === "error" ? "border-destructive/30 bg-destructive/5 text-destructive" : "border-border bg-muted/30 text-muted-foreground"),
					children: deliveryPlan.status === "error" ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
						className: "inline-flex items-start gap-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "mt-px size-3.5 shrink-0" }), deliveryPlan.error]
					}) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "space-y-1.5",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "flex items-start gap-2 text-foreground",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CircleCheck, { className: "mt-px size-3.5 shrink-0 text-status-success" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: deliveryPlan.summary })]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "truncate font-mono text-[11px]",
								children: [
									translate("auto.components.right.sidebar.SourceControlAgentActionDialogForm.1bc0bdbb5e", "Launch:"),
									" ",
									deliveryPlan.commandLabel
								]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "text-[11px]",
								children: deliveryPlan.caveat
							})
						]
					})
				}) : null
			]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogFooter, {
			className: "flex-wrap gap-2 sm:justify-end",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				type: "button",
				variant: "secondary",
				size: "sm",
				onClick: onCancel,
				children: translate("auto.components.right.sidebar.SourceControlAgentActionDialogForm.ea4788705e", "Cancel")
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
				type: "button",
				size: "sm",
				disabled: !canStart,
				onClick: onStart,
				children: [isStarting ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RefreshCw, { className: "size-4 animate-spin" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Sparkles, { className: "size-4" }), effectiveStartLabel]
			})]
		})]
	});
}
function isSourceControlAgentDetectedAndEnabled(agent, detectedAgents, disabledAgents) {
	return Boolean(agent && detectedAgents.includes(agent) && isTuiAgentEnabled(agent, disabledAgents));
}
function buildSourceControlAgentSaveTargets(repoId) {
	const targets = [{
		value: "none",
		label: translate("auto.components.right.sidebar.SourceControlAgentActionDialog.994cddd1f7", "Don't save")
	}];
	if (repoId) targets.push({
		value: "repo",
		label: translate("auto.components.right.sidebar.SourceControlAgentActionDialog.808cfe0a3b", "This repository")
	});
	targets.push({
		value: "global",
		label: translate("auto.components.right.sidebar.SourceControlAgentActionDialog.38b899cc02", "All repositories")
	});
	return targets;
}
function buildSourceControlAgentConnectionErrorPlan() {
	return {
		status: "error",
		error: translate("auto.components.right.sidebar.SourceControlAgentActionDialog.c075d00de1", "Unable to resolve the workspace connection.")
	};
}
function resolveSourceControlAgentSaveTarget(saveTargetValue, repoId) {
	if (saveTargetValue === "repo" && repoId) return {
		type: "repo",
		repoId
	};
	if (saveTargetValue === "global") return { type: "global" };
	return null;
}
function buildSourceControlAgentStatusCopy(args) {
	const { selectedAgent, selectedAgentUnavailable, connectionUnavailable, hasEnabledAgents, detecting } = args;
	if (selectedAgentUnavailable) return `${getAgentCatalog().find((entry) => entry.id === selectedAgent)?.label ?? selectedAgent} is not enabled or was not detected on this workspace host.`;
	if (connectionUnavailable) return "Unable to resolve the workspace connection.";
	if (!hasEnabledAgents && !detecting) return "No enabled agents were detected on this workspace host.";
	return null;
}
var NO_SAVED_RECEIPT_KEY = "__no_saved_receipt__";
function buildSavedLaunchRecipe(input) {
	if (!input.savedAgentId) return null;
	return {
		agentId: input.savedAgentId,
		commandInputTemplate: input.savedCommandInputTemplate ?? "{basePrompt}",
		agentArgs: input.savedAgentArgs ?? ""
	};
}
function getMatchedSavedReceiptTargetValue(input) {
	if (!input.recipe) return null;
	if (input.repoId && input.repo && sourceControlActionRecipeMatchesTarget({
		actionId: input.actionId,
		target: {
			type: "repo",
			repoId: input.repoId
		},
		recipe: input.recipe,
		settings: input.settings,
		repo: input.repo
	})) return "repo";
	if (sourceControlActionRecipeMatchesTarget({
		actionId: input.actionId,
		target: { type: "global" },
		recipe: input.recipe,
		settings: input.settings,
		repo: input.repo
	})) return "global";
	return null;
}
function buildReceiptKey(input) {
	return JSON.stringify([
		input.actionId,
		input.targetValue,
		input.savedAgentId,
		input.savedCommandInputTemplate ?? "{basePrompt}",
		input.savedAgentArgs ?? "",
		input.repoId ?? null,
		input.connectionId ?? null,
		input.worktreeId ?? null,
		input.baseCommandInput
	]);
}
function useSavedSourceControlAgentActionAutoStart({ open, openCycle, detectionReady, actionId, baseCommandInput, savedAgentId, savedCommandInputTemplate, savedAgentArgs, settings, repo, repoId, worktreeId, connectionId, selectedAgent, trimmedCommandInput, connectionUnavailable, detecting, isStarting, detectedAgents, disabledAgents, onAutoStart }) {
	const autoStartedOpenCycleRef = (0, import_react.useRef)(0);
	const [receiptState, setReceiptState] = (0, import_react.useState)(null);
	const savedLaunchRecipe = (0, import_react.useMemo)(() => buildSavedLaunchRecipe({
		savedAgentId,
		savedCommandInputTemplate,
		savedAgentArgs
	}), [
		savedAgentArgs,
		savedAgentId,
		savedCommandInputTemplate
	]);
	const matchedSavedReceiptTargetValue = (0, import_react.useMemo)(() => getMatchedSavedReceiptTargetValue({
		actionId,
		recipe: savedLaunchRecipe,
		settings,
		repo,
		repoId
	}), [
		actionId,
		repo,
		repoId,
		savedLaunchRecipe,
		settings
	]);
	const receiptKey = (0, import_react.useMemo)(() => {
		if (!savedAgentId || !matchedSavedReceiptTargetValue) return null;
		return buildReceiptKey({
			actionId,
			targetValue: matchedSavedReceiptTargetValue,
			savedAgentId,
			savedCommandInputTemplate,
			savedAgentArgs,
			repoId,
			connectionId,
			worktreeId,
			baseCommandInput
		});
	}, [
		actionId,
		baseCommandInput,
		connectionId,
		matchedSavedReceiptTargetValue,
		repoId,
		savedAgentArgs,
		savedAgentId,
		savedCommandInputTemplate,
		worktreeId
	]);
	const currentReceiptState = receiptState?.openCycle === openCycle ? receiptState : null;
	const consideredDifferentReceipt = Boolean(currentReceiptState && receiptKey && currentReceiptState.receiptKey !== receiptKey);
	const autoLaunchPending = Boolean(open && matchedSavedReceiptTargetValue && receiptKey && !consideredDifferentReceipt && !currentReceiptState?.revealed);
	(0, import_react.useEffect)(() => {
		if (!open) {
			autoStartedOpenCycleRef.current = 0;
			setReceiptState(null);
			return;
		}
		if (receiptState?.openCycle !== openCycle) setReceiptState({
			openCycle,
			receiptKey: receiptKey ?? NO_SAVED_RECEIPT_KEY,
			revealed: !receiptKey
		});
		if (!matchedSavedReceiptTargetValue || !receiptKey || !savedAgentId) return;
		if (receiptState?.openCycle === openCycle && receiptState.receiptKey !== receiptKey) return;
		if (receiptState?.openCycle === openCycle && receiptState.revealed) return;
		const revealDialog = () => {
			setReceiptState({
				openCycle,
				receiptKey,
				revealed: true
			});
		};
		if (!detectionReady || detecting || isStarting) return;
		if (selectedAgent !== savedAgentId || !trimmedCommandInput || connectionUnavailable || !isSourceControlAgentDetectedAndEnabled(savedAgentId, detectedAgents, disabledAgents)) {
			revealDialog();
			return;
		}
		if (autoStartedOpenCycleRef.current === openCycle) return;
		autoStartedOpenCycleRef.current = openCycle;
		onAutoStart({
			detectedAgents,
			saveTargetValue: matchedSavedReceiptTargetValue
		}).then((launched) => {
			if (!launched) revealDialog();
		}).catch(() => {
			revealDialog();
		});
	}, [
		connectionUnavailable,
		detectedAgents,
		detectionReady,
		detecting,
		disabledAgents,
		isStarting,
		matchedSavedReceiptTargetValue,
		onAutoStart,
		open,
		openCycle,
		receiptKey,
		receiptState,
		savedAgentId,
		selectedAgent,
		trimmedCommandInput
	]);
	return {
		autoLaunchPending,
		matchedSavedReceiptTargetValue
	};
}
function planSourceControlAgentActionLaunch(args) {
	const agent = args.agent;
	if (!agent) return {
		ok: false,
		error: translate("auto.lib.source.control.agent.action.plan.a7ac8717c7", "Choose an agent before starting.")
	};
	if (!isTuiAgentEnabled(agent, args.disabledAgents)) return {
		ok: false,
		error: translate("auto.lib.source.control.agent.action.plan.b96e091fc9", "The selected agent is disabled in Settings.")
	};
	if (!args.detectedAgents.includes(agent)) return {
		ok: false,
		error: translate("auto.lib.source.control.agent.action.plan.8eb541cc83", "The selected agent was not detected on this workspace host.")
	};
	const trimmedInput = args.commandInput.trim();
	if (!trimmedInput) return {
		ok: false,
		error: translate("auto.lib.source.control.agent.action.plan.46f1a2c9bd", "Command input is empty.")
	};
	const cmdOverrides = args.cmdOverrides ?? {};
	const platform = args.platform ?? CLIENT_PLATFORM;
	const isRemote = args.isRemote ?? false;
	const shell = resolveLocalWindowsAgentStartupShell({
		platform,
		isRemote,
		terminalWindowsShell: args.terminalWindowsShell
	}) ?? (platform === "win32" ? "powershell" : "posix");
	const plannedArgs = planAgentCliArgsSuffix(args.agentArgs, shell);
	if (!plannedArgs.ok) return {
		ok: false,
		error: plannedArgs.error
	};
	let startupPlan = null;
	let delivery;
	if (args.promptDelivery === "submit-after-ready") {
		startupPlan = buildAgentStartupPlan({
			agent,
			prompt: "",
			cmdOverrides,
			platform,
			shell,
			isRemote,
			agentArgs: args.agentArgs,
			sessionOptions: args.sessionOptions,
			allowEmptyPromptLaunch: true
		});
		delivery = "paste-submit";
	} else if (args.promptDelivery === "draft") {
		const draftLaunchPlan = buildAgentDraftLaunchPlan({
			agent,
			draft: trimmedInput,
			cmdOverrides,
			platform,
			shell,
			isRemote,
			agentArgs: args.agentArgs,
			sessionOptions: args.sessionOptions
		});
		if (draftLaunchPlan) {
			startupPlan = {
				agent: draftLaunchPlan.agent,
				launchCommand: draftLaunchPlan.launchCommand,
				expectedProcess: draftLaunchPlan.expectedProcess,
				followupPrompt: null,
				launchConfig: draftLaunchPlan.launchConfig,
				...draftLaunchPlan.sessionOptions ? { sessionOptions: draftLaunchPlan.sessionOptions } : {},
				...draftLaunchPlan.startupCommandDelivery ? { startupCommandDelivery: draftLaunchPlan.startupCommandDelivery } : {},
				...draftLaunchPlan.env ? { env: draftLaunchPlan.env } : {}
			};
			delivery = "draft-native";
		} else {
			startupPlan = buildAgentStartupPlan({
				agent,
				prompt: "",
				cmdOverrides,
				platform,
				shell,
				isRemote,
				agentArgs: args.agentArgs,
				sessionOptions: args.sessionOptions,
				allowEmptyPromptLaunch: true
			});
			delivery = "draft-paste";
		}
	} else if (TUI_AGENT_CONFIG[agent].promptInjectionMode === "stdin-after-start") {
		startupPlan = buildAgentStartupPlan({
			agent,
			prompt: "",
			cmdOverrides,
			platform,
			shell,
			isRemote,
			agentArgs: args.agentArgs,
			sessionOptions: args.sessionOptions,
			allowEmptyPromptLaunch: true
		});
		delivery = "draft-paste";
	} else {
		startupPlan = buildAgentStartupPlan({
			agent,
			prompt: trimmedInput,
			cmdOverrides,
			platform,
			shell,
			isRemote,
			agentArgs: args.agentArgs,
			sessionOptions: args.sessionOptions,
			allowEmptyPromptLaunch: false
		});
		delivery = "argv";
	}
	if (!startupPlan) return {
		ok: false,
		error: translate("auto.lib.source.control.agent.action.plan.3f0ea9aa0d", "Could not build the agent launch command.")
	};
	const summary = delivery === "paste-submit" ? "The agent starts with no prompt, then Orca pastes and submits the command input after the TUI is ready." : delivery === "draft-native" ? "The command input is prefilled as an editable draft by the agent launch command." : delivery === "draft-paste" ? "The agent starts with no prompt, then Orca pastes the command input as an editable draft after the TUI is ready." : "The command input is included in the launch command and submitted as the first turn.";
	return {
		ok: true,
		plan: startupPlan,
		delivery,
		commandLabel: startupPlan.launchCommand,
		summary,
		caveat: "This check builds Orca’s launch plan only. PATH, binary availability, account setup, and terminal startup failures are still caught by the real launch watchdog."
	};
}
function buildSourceControlAgentDeliveryPlan({ selectedAgent, commandInput, agentArgs, promptDelivery, detectedAgents, connectionUnavailable, launchPlatform, isRemote }) {
	if (connectionUnavailable) return buildSourceControlAgentConnectionErrorPlan();
	const settings = useAppStore.getState().settings;
	const result = planSourceControlAgentActionLaunch({
		agent: selectedAgent,
		commandInput,
		agentArgs,
		sessionOptions: selectedAgent ? resolveInitialNativeChatSessionOptions(settings, {
			agent: selectedAgent,
			promptDelivery,
			launchDraftText: commandInput.trim(),
			nativeChatTranscriptIsLocalReadable: !isRemote
		}) : void 0,
		promptDelivery,
		detectedAgents,
		disabledAgents: settings?.disabledTuiAgents,
		cmdOverrides: settings?.agentCmdOverrides,
		terminalWindowsShell: settings?.terminalWindowsShell,
		platform: launchPlatform,
		isRemote
	});
	if (!result.ok) return {
		status: "error",
		error: result.error
	};
	return {
		status: "success",
		summary: result.summary,
		commandLabel: result.commandLabel,
		caveat: result.caveat
	};
}
async function runSourceControlAgentActionStart({ selectedAgent, trimmedCommandInput, agentArgs, commandTemplate, saveTargetValue, actionId, repoId, settings, repo, worktreeId, groupId, promptDelivery, launchPlatform, launchSource, onStart, onSaveAgentDefault, onLaunchAccepted, onLaunchAborted, onLaunched, onClose }) {
	let launched = false;
	let launchFailureNotified = false;
	let launchAcceptedNotified = false;
	const notifyLaunchAccepted = () => {
		if (launchAcceptedNotified) return;
		launchAcceptedNotified = true;
		onLaunchAccepted?.();
	};
	if (onStart) {
		launched = await onStart({
			agent: selectedAgent,
			commandInput: trimmedCommandInput,
			agentArgs
		});
		if (launched) notifyLaunchAccepted();
	} else if (worktreeId) {
		const result = launchAgentInNewTab({
			agent: selectedAgent,
			worktreeId,
			groupId: groupId ?? worktreeId,
			prompt: trimmedCommandInput,
			agentArgs,
			promptDelivery,
			launchPlatform,
			launchSource
		});
		launched = Boolean(result);
		if (result?.tabId) focusTerminalTabSurface(result.tabId);
		if (launched) notifyLaunchAccepted();
		if (result?.promptDeliveryResult) try {
			const deliveryResult = await result.promptDeliveryResult;
			launched = deliveryResult.delivered;
			launchFailureNotified = deliveryResult.failureNotified;
		} catch (error) {
			console.error("promptDeliveryResult rejected", error);
			launched = false;
		}
	}
	if (!launched) {
		if (launchAcceptedNotified) onLaunchAborted?.();
		if (!launchFailureNotified) toast.error(translate("auto.components.right.sidebar.SourceControlAgentActionDialog.8e856842d1", "Could not start the selected agent."));
		return false;
	}
	const saveTarget = resolveSourceControlAgentSaveTarget(saveTargetValue, repoId);
	const launchRecipe = {
		agentId: selectedAgent,
		commandInputTemplate: commandTemplate,
		agentArgs
	};
	const launchRecipeAlreadySaved = Boolean(saveTarget && sourceControlActionRecipeMatchesTarget({
		actionId,
		target: saveTarget,
		recipe: launchRecipe,
		settings,
		repo
	}));
	if (saveTarget && onSaveAgentDefault && !launchRecipeAlreadySaved) try {
		await onSaveAgentDefault(saveTarget, actionId, launchRecipe);
	} catch (error) {
		console.error("onSaveAgentDefault failed", error);
	}
	onLaunched?.();
	onClose();
	return true;
}
function useSourceControlAgentActionStart({ selectedAgent, commandInput, trimmedCommandInput, agentArgs, commandTemplate, saveLaunchRecipe, saveTargetValue, actionId, repoId, settings, repo, worktreeId, groupId, promptDelivery, launchPlatform, isRemote, launchSource, connectionUnavailable, refreshDetectedAgents, onStart, onSaveAgentDefault, onLaunchAccepted, onLaunchAborted, onLaunched, onClose }) {
	const [deliveryPlan, setDeliveryPlan] = (0, import_react.useState)({ status: "idle" });
	const [isStarting, setIsStarting] = (0, import_react.useState)(false);
	const isStartingRef = (0, import_react.useRef)(false);
	const resetDeliveryPlan = (0, import_react.useCallback)(() => setDeliveryPlan({ status: "idle" }), []);
	const buildPlan = (0, import_react.useCallback)(async (agentsOverride) => {
		return buildSourceControlAgentDeliveryPlan({
			selectedAgent,
			commandInput,
			agentArgs,
			promptDelivery,
			detectedAgents: agentsOverride ?? await refreshDetectedAgents(),
			connectionUnavailable,
			launchPlatform,
			isRemote
		});
	}, [
		agentArgs,
		commandInput,
		connectionUnavailable,
		promptDelivery,
		refreshDetectedAgents,
		selectedAgent,
		launchPlatform,
		isRemote
	]);
	const startWithDetectedAgents = (0, import_react.useCallback)(async ({ detectedAgents: nextAgents, saveTargetValueOverride }) => {
		if (!selectedAgent || isStartingRef.current) return false;
		if (connectionUnavailable) {
			setDeliveryPlan(buildSourceControlAgentConnectionErrorPlan());
			return false;
		}
		isStartingRef.current = true;
		setIsStarting(true);
		try {
			const nextPlan = await buildPlan(nextAgents);
			if (nextPlan.status === "error") {
				setDeliveryPlan(nextPlan);
				return false;
			}
			setDeliveryPlan(nextPlan);
			return await runSourceControlAgentActionStart({
				selectedAgent,
				trimmedCommandInput,
				agentArgs,
				commandTemplate,
				saveTargetValue: saveLaunchRecipe ? saveTargetValueOverride ?? saveTargetValue : "none",
				actionId,
				repoId,
				settings,
				repo,
				worktreeId,
				groupId,
				promptDelivery,
				launchPlatform,
				launchSource,
				onStart,
				onSaveAgentDefault,
				onLaunchAccepted,
				onLaunchAborted,
				onLaunched,
				onClose: () => {
					resetDeliveryPlan();
					onClose();
				}
			});
		} finally {
			isStartingRef.current = false;
			setIsStarting(false);
		}
	}, [
		actionId,
		agentArgs,
		buildPlan,
		commandTemplate,
		connectionUnavailable,
		groupId,
		launchSource,
		launchPlatform,
		onClose,
		onLaunchAborted,
		onLaunchAccepted,
		onLaunched,
		onSaveAgentDefault,
		onStart,
		promptDelivery,
		resetDeliveryPlan,
		repo,
		repoId,
		saveLaunchRecipe,
		saveTargetValue,
		settings,
		selectedAgent,
		trimmedCommandInput,
		worktreeId
	]);
	return {
		deliveryPlan,
		resetDeliveryPlan,
		isStarting,
		handleStart: (0, import_react.useCallback)(async () => {
			if (!selectedAgent || isStartingRef.current) return;
			await startWithDetectedAgents({ detectedAgents: await refreshDetectedAgents() });
		}, [
			refreshDetectedAgents,
			selectedAgent,
			startWithDetectedAgents
		]),
		startWithDetectedAgents
	};
}
var DEFAULT_SAVE_TARGET_VALUE = "global";
function useSourceControlAgentActionDialog({ open, onOpenChange, actionId, baseCommandInput, savedCommandInputTemplate, savedAgentArgs, worktreeId, groupId, connectionId, repoId, promptDelivery = "submit-after-ready", launchPlatform, launchSource, savedAgentId, onSaveAgentDefault, onLaunchAccepted, onLaunchAborted, onLaunched, onStart }) {
	const settings = useAppStore((state) => state.settings);
	const repo = useRepoById(repoId ?? null);
	const launchAgentScope = (0, import_react.useMemo)(() => resolveSourceControlLaunchAgentScope({
		settings,
		repo,
		actionId
	}), [
		actionId,
		repo,
		settings
	]);
	const defaultSaveTargetValue = launchAgentScope.overridesGlobalAgent && repoId ? "repo" : DEFAULT_SAVE_TARGET_VALUE;
	const ensureDetectedAgents = useAppStore((state) => state.ensureDetectedAgents);
	const ensureRemoteDetectedAgents = useAppStore((state) => state.ensureRemoteDetectedAgents);
	const [commandTemplate, setCommandTemplate] = (0, import_react.useState)(savedCommandInputTemplate ?? "{basePrompt}");
	const [agentArgs, setAgentArgs] = (0, import_react.useState)(savedAgentArgs ?? "");
	const [selectedAgent, setSelectedAgent] = (0, import_react.useState)(savedAgentId ?? null);
	const [detectedAgents, setDetectedAgents] = (0, import_react.useState)([]);
	const [detecting, setDetecting] = (0, import_react.useState)(false);
	const openCycleRef = (0, import_react.useRef)(0);
	const wasOpenRef = (0, import_react.useRef)(false);
	const [openCycle, setOpenCycle] = (0, import_react.useState)(0);
	const [detectedOpenCycle, setDetectedOpenCycle] = (0, import_react.useState)(null);
	const saveTargets = (0, import_react.useMemo)(() => buildSourceControlAgentSaveTargets(repoId), [repoId]);
	const [saveLaunchRecipe, setSaveLaunchRecipe] = (0, import_react.useState)(true);
	const [saveTargetValue, setSaveTargetValue] = (0, import_react.useState)(defaultSaveTargetValue);
	const disabledAgents = settings?.disabledTuiAgents;
	const connectionUnavailable = Boolean(worktreeId && connectionId === void 0);
	const refreshDetectedAgents = (0, import_react.useCallback)(async () => {
		if (connectionUnavailable) {
			setDetectedAgents([]);
			setDetecting(false);
			return [];
		}
		setDetecting(true);
		try {
			const nextAgents = typeof connectionId === "string" ? await ensureRemoteDetectedAgents(connectionId) : await ensureDetectedAgents();
			setDetectedAgents(nextAgents);
			return nextAgents;
		} finally {
			setDetecting(false);
		}
	}, [
		connectionId,
		connectionUnavailable,
		ensureDetectedAgents,
		ensureRemoteDetectedAgents
	]);
	(0, import_react.useEffect)(() => {
		if (!open) {
			wasOpenRef.current = false;
			return;
		}
		const cycle = wasOpenRef.current ? openCycleRef.current : openCycleRef.current + 1;
		if (!wasOpenRef.current) {
			openCycleRef.current = cycle;
			setOpenCycle(cycle);
		}
		wasOpenRef.current = true;
		setDetectedOpenCycle(null);
		setCommandTemplate(savedCommandInputTemplate ?? "{basePrompt}");
		setAgentArgs(savedAgentArgs ?? "");
		setSelectedAgent(savedAgentId ?? null);
		setSaveLaunchRecipe(true);
		setSaveTargetValue(defaultSaveTargetValue);
		let stale = false;
		refreshDetectedAgents().then((nextAgents) => {
			if (stale || openCycleRef.current !== cycle) return;
			setSelectedAgent((current) => current ?? pickSourceControlLaunchAgent({
				savedAgent: savedAgentId,
				defaultAgent: settings?.defaultTuiAgent,
				detectedAgents: nextAgents,
				disabledAgents
			}));
			setDetectedOpenCycle(cycle);
		});
		return () => {
			stale = true;
		};
	}, [
		defaultSaveTargetValue,
		disabledAgents,
		open,
		refreshDetectedAgents,
		savedAgentId,
		savedAgentArgs,
		savedCommandInputTemplate,
		repoId,
		settings?.defaultTuiAgent
	]);
	const closeDialog = (0, import_react.useCallback)(() => onOpenChange(false), [onOpenChange]);
	const enabledDetectedAgents = (0, import_react.useMemo)(() => detectedAgents.filter((agent) => isTuiAgentEnabled(agent, disabledAgents)), [detectedAgents, disabledAgents]);
	const agentOptions = (0, import_react.useMemo)(() => getAgentCatalog().filter((entry) => enabledDetectedAgents.includes(entry.id) || entry.id === selectedAgent), [enabledDetectedAgents, selectedAgent]);
	const selectedAgentUnavailable = Boolean(selectedAgent && !isSourceControlAgentDetectedAndEnabled(selectedAgent, detectedAgents, disabledAgents));
	const hasEnabledAgents = enabledDetectedAgents.length > 0;
	const commandInput = renderSourceControlActionCommandTemplate(commandTemplate, { basePrompt: baseCommandInput });
	const trimmedCommandInput = commandInput.trim();
	const { deliveryPlan, resetDeliveryPlan, isStarting, handleStart, startWithDetectedAgents } = useSourceControlAgentActionStart({
		selectedAgent,
		commandInput,
		trimmedCommandInput,
		agentArgs,
		commandTemplate,
		saveLaunchRecipe,
		saveTargetValue,
		actionId,
		repoId,
		settings,
		repo,
		worktreeId,
		groupId,
		promptDelivery,
		launchPlatform,
		isRemote: typeof connectionId === "string",
		launchSource,
		connectionUnavailable,
		refreshDetectedAgents,
		onStart,
		onSaveAgentDefault,
		onLaunchAccepted,
		onLaunchAborted,
		onLaunched,
		onClose: closeDialog
	});
	const canStart = Boolean(trimmedCommandInput) && Boolean(selectedAgent) && !selectedAgentUnavailable && !connectionUnavailable && !detecting && !isStarting;
	const handleOpenChange = (0, import_react.useCallback)((nextOpen) => {
		if (!nextOpen) {
			resetDeliveryPlan();
			setSaveLaunchRecipe(true);
			setSaveTargetValue(defaultSaveTargetValue);
		}
		onOpenChange(nextOpen);
	}, [
		defaultSaveTargetValue,
		onOpenChange,
		resetDeliveryPlan
	]);
	const { autoLaunchPending } = useSavedSourceControlAgentActionAutoStart({
		open,
		openCycle,
		detectionReady: detectedOpenCycle === openCycle,
		actionId,
		baseCommandInput,
		savedAgentId,
		savedCommandInputTemplate,
		savedAgentArgs,
		settings,
		repo,
		repoId,
		worktreeId,
		connectionId,
		selectedAgent,
		trimmedCommandInput,
		connectionUnavailable,
		detecting,
		isStarting,
		detectedAgents,
		disabledAgents,
		onAutoStart: ({ detectedAgents: agentsForLaunch, saveTargetValue: matchedTargetValue }) => startWithDetectedAgents({
			detectedAgents: agentsForLaunch,
			saveTargetValueOverride: matchedTargetValue
		})
	});
	const statusCopy = buildSourceControlAgentStatusCopy({
		selectedAgent,
		selectedAgentUnavailable,
		connectionUnavailable,
		hasEnabledAgents,
		detecting
	});
	const resetPlanAfter = (0, import_react.useCallback)((apply) => (value) => {
		apply(value);
		resetDeliveryPlan();
	}, [resetDeliveryPlan]);
	const onSelectedAgentChange = (0, import_react.useMemo)(() => resetPlanAfter(setSelectedAgent), [resetPlanAfter]);
	const onAgentArgsChange = (0, import_react.useMemo)(() => resetPlanAfter(setAgentArgs), [resetPlanAfter]);
	const onCommandTemplateChange = (0, import_react.useMemo)(() => resetPlanAfter(setCommandTemplate), [resetPlanAfter]);
	const onSaveLaunchRecipeChange = (0, import_react.useMemo)(() => resetPlanAfter(setSaveLaunchRecipe), [resetPlanAfter]);
	const agentScopeNote = (0, import_react.useMemo)(() => {
		if (!launchAgentScope.overridesGlobalAgent) return null;
		const catalog = getAgentCatalog();
		const labelFor = (agentId) => catalog.find((entry) => entry.id === agentId)?.label ?? agentId ?? "";
		return {
			effectiveAgentLabel: labelFor(launchAgentScope.effectiveAgentId),
			globalAgentLabel: labelFor(launchAgentScope.globalAgentId)
		};
	}, [launchAgentScope]);
	return {
		handleOpenChange,
		shouldRenderDialog: !autoLaunchPending,
		agentScopeNote,
		agentOptions,
		selectedAgent,
		hasEnabledAgents,
		detecting,
		statusCopy,
		agentArgs,
		commandTemplate,
		saveLaunchRecipe,
		saveTargetValue,
		saveTargets,
		settings,
		repo,
		deliveryPlan,
		canStart,
		isStarting,
		onSelectedAgentChange,
		onAgentArgsChange,
		onCommandTemplateChange,
		onSaveLaunchRecipeChange,
		onSaveAgentDefaultChange: setSaveTargetValue,
		handleStart
	};
}
function SourceControlAgentActionDialog(props) {
	const { open, actionId, title, description, baseCommandInput, savedCommandInputTemplate, onOpenSettings, startLabel = "Start agent", onSaveAgentDefault } = props;
	const { handleOpenChange, shouldRenderDialog, agentScopeNote, agentOptions, selectedAgent, hasEnabledAgents, detecting, statusCopy, agentArgs, commandTemplate, saveLaunchRecipe, saveTargetValue, saveTargets, settings, repo, deliveryPlan, canStart, isStarting, onSelectedAgentChange, onAgentArgsChange, onCommandTemplateChange, onSaveLaunchRecipeChange, onSaveAgentDefaultChange, handleStart } = useSourceControlAgentActionDialog(props);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
		open,
		onOpenChange: handleOpenChange,
		children: shouldRenderDialog ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogContent, {
			className: "flex max-h-[min(82vh,42rem)] min-w-0 flex-col overflow-hidden sm:max-w-2xl",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, {
				className: "shrink-0",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, {
					className: "text-sm",
					children: title
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, {
					className: "text-xs",
					children: description
				})]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SourceControlAgentActionDialogForm, {
				actionId,
				baseCommandInput,
				agentScopeNote,
				agentOptions,
				selectedAgent,
				hasEnabledAgents,
				detecting,
				statusCopy,
				agentArgs,
				commandTemplate,
				savedCommandInputTemplate,
				saveLaunchRecipe,
				saveTargetValue,
				saveTargets,
				settings,
				repo,
				canSaveAgentDefault: Boolean(onSaveAgentDefault),
				deliveryPlan,
				canStart,
				isStarting,
				startLabel,
				onSelectedAgentChange,
				onAgentArgsChange,
				onCommandTemplateChange,
				onSaveLaunchRecipeChange,
				onSaveAgentDefaultChange,
				onOpenSettings,
				onCancel: () => handleOpenChange(false),
				onStart: () => void handleStart()
			})]
		}) : null
	});
}
export { sourceControlActionRecipeMatchesTarget as n, resolveSourceControlLaunchPlatform as r, SourceControlAgentActionDialog as t };
