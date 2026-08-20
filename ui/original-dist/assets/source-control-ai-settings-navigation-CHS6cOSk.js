import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { l as createLucideIcon, n as cn, t as Button } from "./button-DszXJEV6.js";
import { t as ChevronDown } from "./chevron-down-BRkP96Md.js";
import { t as RefreshCw } from "./refresh-cw-BU_ChOig.js";
import { t as SlidersHorizontal } from "./sliders-horizontal-D-rA728l.js";
import { i as DropdownMenuItem, m as DropdownMenuTrigger, r as DropdownMenuContent, t as DropdownMenu } from "./dropdown-menu-Dth6LPK-.js";
import { i as getRepositorySourceControlAiSectionId } from "./repository-settings-targets-Ld1Q60A4.js";
import { t as SourceControlAgentActionDialog } from "./SourceControlAgentActionDialog-BUxX_J0z.js";
var Sparkle = createLucideIcon("sparkle", [["path", {
	d: "M11.017 2.814a1 1 0 0 1 1.966 0l1.051 5.558a2 2 0 0 0 1.594 1.594l5.558 1.051a1 1 0 0 1 0 1.966l-5.558 1.051a2 2 0 0 0-1.594 1.594l-1.051 5.558a1 1 0 0 1-1.966 0l-1.051-5.558a2 2 0 0 0-1.594-1.594l-5.558-1.051a1 1 0 0 1 0-1.966l5.558-1.051a2 2 0 0 0 1.594-1.594z",
	key: "1s2grr"
}]]);
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function SourceControlFixSplitButton({ label, actionId, dialogTitle, dialogDescription, launchSource, contextUnavailableLabel, primaryTitle, primaryAriaLabel, chevronTitle, chevronAriaLabel, worktreeId, groupId, connectionId, repoId, launchPlatform, prompt, isLaunching, disabledReason, variant, size, iconClassName, primaryClassName, chevronClassName, savedAgentId, savedCommandInputTemplate, savedAgentArgs, onSaveAgentDefault, onOpenSettings, onFixWithDefaultAgent, onPromptDelivered }) {
	const [composerOpen, setComposerOpen] = (0, import_react.useState)(false);
	const canLaunch = Boolean(worktreeId && groupId && prompt && !disabledReason);
	const dividerClass = variant === "default" ? "border-primary-foreground/20" : "border-border";
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenu, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex shrink-0 items-stretch",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
			type: "button",
			variant,
			size,
			className: cn("rounded-r-none", primaryClassName),
			disabled: isLaunching || !canLaunch,
			title: disabledReason ?? primaryTitle,
			"aria-label": primaryAriaLabel,
			onClick: () => void onFixWithDefaultAgent(),
			children: [isLaunching ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RefreshCw, { className: cn(iconClassName, "animate-spin") }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Sparkle, { className: iconClassName }), label]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuTrigger, {
			asChild: true,
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				type: "button",
				variant,
				size,
				className: cn("rounded-l-none border-l", dividerClass, chevronClassName),
				disabled: isLaunching || !canLaunch,
				title: chevronTitle,
				"aria-label": chevronAriaLabel,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronDown, { className: iconClassName })
			})
		})]
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuContent, {
		align: "end",
		className: "min-w-[210px] p-1",
		children: worktreeId && groupId && prompt && !disabledReason ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
			onSelect: () => setComposerOpen(true),
			className: "gap-2 rounded-[7px] px-2 py-1.5 text-[12px] leading-5 font-medium",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SlidersHorizontal, { className: "size-4 text-muted-foreground" }), translate("auto.components.right.sidebar.SourceControl.f0a2dc9e46", "Customize launch...")]
		}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuItem, {
			disabled: true,
			children: contextUnavailableLabel
		})
	})] }), worktreeId && groupId && prompt ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SourceControlAgentActionDialog, {
		open: composerOpen,
		onOpenChange: setComposerOpen,
		actionId,
		title: dialogTitle,
		description: dialogDescription,
		baseCommandInput: prompt,
		worktreeId,
		groupId,
		connectionId,
		repoId,
		promptDelivery: "submit-after-ready",
		launchPlatform,
		launchSource,
		savedAgentId,
		savedCommandInputTemplate,
		savedAgentArgs,
		onSaveAgentDefault,
		onOpenSettings,
		onLaunched: onPromptDelivered
	}) : null] });
}
function openSourceControlAiSettingsTarget({ activeRepo, openSettingsTarget, openSettingsPage }) {
	if (activeRepo) openSettingsTarget({
		pane: "repo",
		repoId: activeRepo.id,
		sectionId: getRepositorySourceControlAiSectionId(activeRepo.id)
	});
	else openSettingsTarget({
		pane: "git",
		repoId: null,
		sectionId: "source-control-ai-settings"
	});
	openSettingsPage();
}
export { SourceControlFixSplitButton as n, openSourceControlAiSettingsTarget as t };
