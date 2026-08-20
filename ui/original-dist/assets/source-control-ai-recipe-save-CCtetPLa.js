import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as Button } from "./button-DszXJEV6.js";
import { t as Braces } from "./braces-h3B7q7My.js";
import { $h as filterEnabledTuiAgents, Ah as normalizeSourceControlAiSettings, Fh as DEFAULT_SOURCE_CONTROL_ACTION_COMMAND_TEMPLATES, Ih as SOURCE_CONTROL_ACTION_IDS, Jh as isCustomAgentId, Rh as SOURCE_CONTROL_TEXT_ACTION_IDS, jh as resolveSourceControlActionRecipe, kh as normalizeRepoSourceControlAiOverrides } from "./store-CgXrfmaH.js";
import { n as HoverCardContent, r as HoverCardTrigger, t as HoverCard } from "./hover-card-DP92-D-b.js";
import { n as getAgentCatalog } from "./agent-catalog-CBF2CV5Q.js";
function readSourceControlLaunchRecipeAgentId(recipe) {
	const agentId = recipe?.agentId;
	return agentId && !isCustomAgentId(agentId) ? agentId : null;
}
function pickSourceControlLaunchAgent(args) {
	const enabledAgents = filterEnabledTuiAgents(args.detectedAgents, args.disabledAgents);
	if (args.savedAgent && enabledAgents.includes(args.savedAgent)) return args.savedAgent;
	if (args.defaultAgent && args.defaultAgent !== "blank" && enabledAgents.includes(args.defaultAgent)) return args.defaultAgent;
	return getAgentCatalog().find((entry) => enabledAgents.includes(entry.id))?.id ?? null;
}
function resolveSourceControlLaunchAgentScope(input) {
	const effectiveAgentId = readSourceControlLaunchRecipeAgentId(resolveSourceControlActionRecipe({
		settings: input.settings,
		repo: input.repo,
		actionId: input.actionId
	}));
	const globalRecipeAgentId = readSourceControlLaunchRecipeAgentId(resolveSourceControlActionRecipe({
		settings: input.settings,
		repo: null,
		actionId: input.actionId
	}));
	const defaultTuiAgent = input.settings?.defaultTuiAgent;
	const globalAgentId = globalRecipeAgentId ?? (defaultTuiAgent && defaultTuiAgent !== "blank" ? defaultTuiAgent : null);
	return {
		effectiveAgentId,
		globalAgentId,
		overridesGlobalAgent: normalizeRepoSourceControlAiOverrides(input.repo?.sourceControlAi)?.actionOverrides?.[input.actionId]?.agentId !== void 0 && effectiveAgentId !== null && effectiveAgentId !== globalAgentId
	};
}
function hasActionOverride(overrides, actionId) {
	return Object.hasOwn(overrides ?? {}, actionId);
}
function readRecipeOverrideFields(recipe) {
	const fields = [];
	if (Object.hasOwn(recipe ?? {}, "agentId")) fields.push("agent");
	if (Object.hasOwn(recipe ?? {}, "commandInputTemplate")) fields.push("commandTemplate");
	if (Object.hasOwn(recipe ?? {}, "agentArgs")) fields.push("agentArgs");
	return fields;
}
function summarizeReposOverridingActionRecipe(input) {
	const overrides = [];
	for (const repo of input.repos) {
		const actionOverrides = normalizeRepoSourceControlAiOverrides(repo.sourceControlAi)?.actionOverrides;
		if (!hasActionOverride(actionOverrides, input.actionId)) continue;
		overrides.push({
			repoId: repo.id,
			repoName: repo.displayName,
			fields: readRecipeOverrideFields(actionOverrides?.[input.actionId])
		});
	}
	return {
		count: overrides.length,
		overrides
	};
}
const SOURCE_CONTROL_ACTION_VARIABLES = {
	commitMessage: [
		"basePrompt",
		"branch",
		"stagedFiles",
		"stagedPatch",
		"linkedIssue"
	],
	pullRequest: [
		"basePrompt",
		"branch",
		"baseBranch",
		"currentTitle",
		"currentBody",
		"commitSummary",
		"changedFiles",
		"patch",
		"linkedIssue"
	],
	branchName: [
		"basePrompt",
		"firstPrompt",
		"assistantMessage"
	],
	fixCommitFailure: ["basePrompt"],
	fixPushFailure: ["basePrompt"],
	fixChecks: ["basePrompt"],
	resolveConflicts: ["basePrompt"],
	resolveComments: ["basePrompt"]
};
const SOURCE_CONTROL_ACTION_VARIABLE_INFO = {
	basePrompt: {
		description: "Orca’s built-in prompt for this action, including the context Orca knows how to gather safely.",
		example: "Commit messages include staged diff guidance; PR details include branch comparison guidance; fix actions include the failure summary."
	},
	branch: {
		description: "The current source-control branch name.",
		example: "feature/source-control-ai-recipes"
	},
	stagedFiles: {
		description: "A newline-separated list of staged files for commit-message generation.",
		example: "M src/shared/source-control-ai.ts\nA src/shared/source-control-ai-actions.ts"
	},
	stagedPatch: {
		description: "The staged git patch used for commit-message generation.",
		example: "diff --git a/src/app.ts b/src/app.ts\n+addActionRecipeDefaults()"
	},
	baseBranch: {
		description: "The target branch selected in the Create PR composer.",
		example: "main"
	},
	currentTitle: {
		description: "The PR title currently typed in the composer before generation starts.",
		example: "Improve Source Control AI customization"
	},
	currentBody: {
		description: "The PR description currently typed in the composer before generation starts.",
		example: "Adds configurable agents and command templates for Source Control actions."
	},
	commitSummary: {
		description: "A newline-separated list of commits on the branch compared to the base.",
		example: "a1b2c3d Add action recipe defaults\nd4e5f6a Render command templates"
	},
	changedFiles: {
		description: "A summary of files changed between the branch and the base branch.",
		example: "src/shared/source-control-ai-actions.ts | 24 +++++\nsrc/main/text-generation.ts | 8 +-"
	},
	patch: {
		description: "The branch diff against the base branch used for PR-details generation.",
		example: "diff --git a/src/app.ts b/src/app.ts\n+renderSourceControlActionCommandTemplate()"
	},
	firstPrompt: {
		description: "The first user request that created the Orca workspace.",
		example: "Fix CI and commit the result"
	},
	assistantMessage: {
		description: "The initial agent response, when Orca has one available.",
		example: "I will inspect the failing check, patch the issue, and run tests."
	},
	linkedIssue: {
		description: "The GitHub issue number linked to this workspace. Empty when no GitHub issue is linked (including GitLab-linked workspaces). Prefer instructional templates: a bare \"Fixes #{linkedIssue}\" becomes \"Fixes #\" when unlinked.",
		example: "123"
	}
};
function isLinkedIssueNumber(linkedIssue) {
	return typeof linkedIssue === "number" && Number.isSafeInteger(linkedIssue) && linkedIssue > 0;
}
function formatLinkedIssueTemplateValue(linkedIssue) {
	return isLinkedIssueNumber(linkedIssue) ? String(linkedIssue) : "";
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function hasVariablePreview(variablePreviews, variable) {
	return Boolean(variablePreviews && Object.hasOwn(variablePreviews, variable) && variablePreviews[variable] !== void 0 && variablePreviews[variable] !== null);
}
function SourceControlVariableSample({ label, value }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-1",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "text-[10px] font-semibold uppercase tracking-wide text-muted-foreground",
			children: label
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("pre", {
			className: "rounded-sm bg-background/60 p-2 whitespace-pre-wrap break-words font-mono text-[11px] leading-relaxed",
			children: value || translate("auto.components.source.control.SourceControlActionVariableChips.4bf6d88039", "(empty)")
		})]
	});
}
function SourceControlVariableDetails({ variable, preview }) {
	if (preview !== void 0 && variable === "basePrompt") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("pre", {
		className: "whitespace-pre-wrap break-words font-mono text-[11px] leading-relaxed",
		children: preview || translate("auto.components.source.control.SourceControlActionVariableChips.4bf6d88039", "(empty)")
	});
	const info = SOURCE_CONTROL_ACTION_VARIABLE_INFO[variable];
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "max-w-80 space-y-2 text-left leading-relaxed",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-0.5",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "font-mono text-[11px]",
					children: `{${variable}}`
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "text-muted-foreground",
					children: info.description
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SourceControlVariableSample, {
				label: translate("auto.components.source.control.SourceControlActionVariableChips.6b921a0ac2", "Example"),
				value: info.example
			}),
			preview !== void 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SourceControlVariableSample, {
				label: translate("auto.components.source.control.SourceControlActionVariableChips.7377483644", "This workspace"),
				value: preview
			}) : null
		]
	});
}
function SourceControlActionVariableChips({ actionId, disabled = false, variablePreviews, onInsert }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex flex-wrap items-center gap-1.5",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
			className: "inline-flex items-center gap-1 text-[11px] text-muted-foreground",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Braces, { className: "size-3" }), translate("auto.components.source.control.SourceControlActionVariableChips.1b77798d5f", "Variables")]
		}), SOURCE_CONTROL_ACTION_VARIABLES[actionId].map((variable) => {
			const preview = hasVariablePreview(variablePreviews, variable) ? variablePreviews?.[variable] : void 0;
			return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(HoverCard, {
				openDelay: 150,
				closeDelay: 120,
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(HoverCardTrigger, {
					asChild: true,
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "inline-flex",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							type: "button",
							variant: "ghost",
							size: "xs",
							disabled,
							className: "h-5 rounded px-1.5 font-mono text-[10px]",
							onClick: () => onInsert(variable),
							children: `{${variable}}`
						})
					})
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(HoverCardContent, {
					side: "top",
					sideOffset: 6,
					collisionPadding: 12,
					className: "scrollbar-sleek max-h-[min(18rem,calc(100vh-2rem))] w-[min(32rem,calc(100vw-2rem))] overflow-y-auto p-2 text-left text-xs",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SourceControlVariableDetails, {
						variable,
						preview
					})
				})]
			}, variable);
		})]
	});
}
var TEXT_ACTION_ID_SET = new Set(SOURCE_CONTROL_TEXT_ACTION_IDS);
function hasEntries(value) {
	return Object.keys(value ?? {}).length > 0;
}
function normalizeStringRecord(value) {
	const normalized = {};
	for (const [key, item] of Object.entries(value ?? {})) if (typeof item === "string") normalized[key] = item;
	return hasEntries(normalized) ? normalized : void 0;
}
function normalizeBooleanRecord(value) {
	const normalized = {};
	for (const [key, item] of Object.entries(value ?? {})) if (typeof item === "boolean") normalized[key] = item;
	return hasEntries(normalized) ? normalized : void 0;
}
function normalizeCompleteRecipe(actionId, recipe) {
	if (!recipe) return;
	const commandInputTemplate = typeof recipe.commandInputTemplate === "string" ? recipe.commandInputTemplate : DEFAULT_SOURCE_CONTROL_ACTION_COMMAND_TEMPLATES[actionId];
	const rawAgentArgs = recipe.agentArgs;
	const agentArgs = typeof rawAgentArgs === "string" ? rawAgentArgs.trim() : void 0;
	return {
		agentId: recipe.agentId ?? null,
		commandInputTemplate,
		...agentArgs !== void 0 ? { agentArgs } : {}
	};
}
function normalizeActionOverrides(overrides) {
	const normalized = {};
	for (const actionId of SOURCE_CONTROL_ACTION_IDS) {
		const recipe = normalizeCompleteRecipe(actionId, overrides?.[actionId]);
		if (recipe) normalized[actionId] = recipe;
	}
	return hasEntries(normalized) ? normalized : void 0;
}
function normalizeWritableRepoSourceControlAiOverrides(value) {
	const readCompatible = normalizeRepoSourceControlAiOverrides(value);
	if (!readCompatible) return;
	const writable = {};
	if (typeof readCompatible.enabled === "boolean") writable.enabled = readCompatible.enabled;
	if (typeof readCompatible.customAgentCommand === "string") {
		const customAgentCommand = readCompatible.customAgentCommand.trim();
		if (customAgentCommand) writable.customAgentCommand = customAgentCommand;
	}
	if (readCompatible.modelOverridesByOperation) writable.modelOverridesByOperation = readCompatible.modelOverridesByOperation;
	const instructionsByOperation = normalizeStringRecord(readCompatible.instructionsByOperation);
	if (instructionsByOperation) writable.instructionsByOperation = instructionsByOperation;
	const actionOverrides = normalizeActionOverrides(readCompatible.actionOverrides);
	if (actionOverrides) writable.actionOverrides = actionOverrides;
	const prCreationDefaults = normalizeBooleanRecord(readCompatible.prCreationDefaults);
	if (prCreationDefaults) writable.prCreationDefaults = prCreationDefaults;
	return Object.keys(writable).length > 0 ? writable : void 0;
}
function toSourceControlAiRepoUpdate(value) {
	const sourceControlAi = normalizeWritableRepoSourceControlAiOverrides(value);
	return sourceControlAi ? { sourceControlAi } : { sourceControlAi: null };
}
function dropLegacyInstructionForAction(value, actionId) {
	if (!TEXT_ACTION_ID_SET.has(actionId) || !value.instructionsByOperation) return value;
	const instructionsByOperation = { ...value.instructionsByOperation };
	delete instructionsByOperation[actionId];
	return {
		...value,
		instructionsByOperation: hasEntries(instructionsByOperation) ? instructionsByOperation : void 0
	};
}
function normalizeRecipeForSave(actionId, recipe) {
	return normalizeCompleteRecipe(actionId, recipe) ?? {
		agentId: null,
		commandInputTemplate: DEFAULT_SOURCE_CONTROL_ACTION_COMMAND_TEMPLATES[actionId]
	};
}
function saveSourceControlActionRecipe(input) {
	const savedRecipe = normalizeRecipeForSave(input.actionId, input.recipe);
	if (input.target.type === "global") {
		const current = normalizeSourceControlAiSettings(input.settings.sourceControlAi, input.settings.commitMessageAi);
		return {
			target: { type: "global" },
			sourceControlAi: {
				...current,
				...typeof input.customAgentCommand === "string" ? { customAgentCommand: input.customAgentCommand } : {},
				actions: {
					...current.actions,
					[input.actionId]: savedRecipe
				}
			}
		};
	}
	const currentRepoAi = normalizeWritableRepoSourceControlAiOverrides(input.repo?.sourceControlAi);
	const next = dropLegacyInstructionForAction({
		...currentRepoAi,
		...typeof input.customAgentCommand === "string" ? { customAgentCommand: input.customAgentCommand } : {},
		actionOverrides: {
			...currentRepoAi?.actionOverrides,
			[input.actionId]: savedRecipe
		}
	}, input.actionId);
	return {
		target: input.target,
		update: toSourceControlAiRepoUpdate(next)
	};
}
export { pickSourceControlLaunchAgent as a, summarizeReposOverridingActionRecipe as c, formatLinkedIssueTemplateValue as i, toSourceControlAiRepoUpdate as n, readSourceControlLaunchRecipeAgentId as o, SourceControlActionVariableChips as r, resolveSourceControlLaunchAgentScope as s, saveSourceControlActionRecipe as t };
