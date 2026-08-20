import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn } from "./button-DszXJEV6.js";
import { t as Send } from "./send-CyCdniMF.js";
import { t as Sparkles } from "./sparkles-DTr27w4B.js";
import { t as useAppStore } from "./store-CgXrfmaH.js";
import { a as DropdownMenuLabel, d as DropdownMenuSub, f as DropdownMenuSubContent, m as DropdownMenuTrigger, p as DropdownMenuSubTrigger, r as DropdownMenuContent, t as DropdownMenu } from "./dropdown-menu-Dth6LPK-.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
import { t as ReviewNotesSendMenuContent } from "./ReviewNotesSendMenuContent-DnAssgZQ.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var ENABLED_SEND_TOOLTIP = "Send notes to an agent";
function buildNotesSendTargetModeId(modeIdParts) {
	return `note-send:${modeIdParts.map((part) => `${part.length}:${part}`).join("|")}`;
}
function NotesSendMenu({ worktreeId, groupId, modeIdParts, scopes, defaultScopeId, source = "diff-notes", targetModeLabel, triggerClassName, triggerLabel, triggerCount, actionLabel, disabledTooltip = "All notes sent", iconClassName = "size-3.5", align = "end", openRequestNonce = null, onOpenRequestHandled, onDelivered }) {
	const openAgentSendPopoverTargetMode = useAppStore((s) => s.openAgentSendPopoverTargetMode);
	const closeAgentSendPopoverTargetMode = useAppStore((s) => s.closeAgentSendPopoverTargetMode);
	const activeTargetModeId = useAppStore((s) => s.agentSendPopoverTargetMode?.id ?? null);
	const [sendMenuOpen, setSendMenuOpen] = (0, import_react.useState)(false);
	const targetModeId = (0, import_react.useMemo)(() => buildNotesSendTargetModeId(modeIdParts), [modeIdParts]);
	const enabledScopes = (0, import_react.useMemo)(() => scopes.filter((scope) => scope.notes.length > 0), [scopes]);
	const defaultScope = (0, import_react.useMemo)(() => {
		return enabledScopes.find((scope) => scope.id === defaultScopeId) ?? enabledScopes[0] ?? null;
	}, [defaultScopeId, enabledScopes]);
	const hasDeliverableNotes = enabledScopes.length > 0;
	const markDelivered = (0, import_react.useCallback)((notes) => {
		onDelivered(notes);
	}, [onDelivered]);
	const openTargetMode = (0, import_react.useCallback)((scope) => {
		if (scope.notes.length === 0) return;
		openAgentSendPopoverTargetMode({
			id: targetModeId,
			worktreeId,
			source,
			prompt: scope.prompt,
			label: targetModeLabel ?? scope.label,
			launchSource: "notes_send",
			onPromptDelivered: () => markDelivered(scope.notes)
		});
	}, [
		markDelivered,
		openAgentSendPopoverTargetMode,
		source,
		targetModeId,
		targetModeLabel,
		worktreeId
	]);
	const handleOpenChange = (0, import_react.useCallback)((open) => {
		setSendMenuOpen(open);
		if (open) {
			if (defaultScope) openTargetMode(defaultScope);
		} else closeAgentSendPopoverTargetMode(targetModeId);
	}, [
		closeAgentSendPopoverTargetMode,
		defaultScope,
		openTargetMode,
		targetModeId
	]);
	const effectiveSendMenuOpen = sendMenuOpen && activeTargetModeId === targetModeId;
	if (sendMenuOpen && activeTargetModeId !== targetModeId) setSendMenuOpen(false);
	(0, import_react.useEffect)(() => () => {
		closeAgentSendPopoverTargetMode(targetModeId);
	}, [closeAgentSendPopoverTargetMode, targetModeId]);
	(0, import_react.useEffect)(() => {
		if (openRequestNonce == null) return;
		if (hasDeliverableNotes && defaultScope) handleOpenChange(true);
		onOpenRequestHandled?.();
	}, [
		openRequestNonce,
		hasDeliverableNotes,
		defaultScope,
		handleOpenChange,
		onOpenRequestHandled
	]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenu, {
		modal: false,
		open: effectiveSendMenuOpen,
		onOpenChange: handleOpenChange,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
			asChild: true,
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuTrigger, {
				asChild: true,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
					type: "button",
					className: cn("inline-flex items-center justify-center rounded text-muted-foreground transition-colors hover:bg-accent hover:text-foreground disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-muted-foreground", triggerClassName),
					disabled: !hasDeliverableNotes,
					title: hasDeliverableNotes ? ENABLED_SEND_TOOLTIP : disabledTooltip,
					"aria-label": triggerLabel ? translate("auto.components.editor.NotesSendMenu.433928cd9f", "Send {{value0}} to an agent", { value0: triggerLabel }) : ENABLED_SEND_TOOLTIP,
					onMouseDown: (event) => event.stopPropagation(),
					onClick: (event) => event.stopPropagation(),
					children: [
						triggerLabel ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Sparkles, { className: "size-3 text-violet-500 dark:text-violet-400" }),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "whitespace-nowrap",
								children: triggerLabel
							}),
							triggerCount !== void 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "rounded-full bg-background/80 px-1 text-[10px] tabular-nums text-muted-foreground",
								children: triggerCount
							}) : null,
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "mx-0.5 h-3 w-px bg-border/70",
								"aria-hidden": true
							})
						] }) : null,
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Send, { className: iconClassName }),
						actionLabel ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "whitespace-nowrap",
							children: actionLabel
						}) : null
					]
				})
			})
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
			side: "bottom",
			sideOffset: 6,
			children: hasDeliverableNotes ? ENABLED_SEND_TOOLTIP : disabledTooltip
		})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuContent, {
			align,
			className: "min-w-[220px]",
			onInteractOutside: preventAgentSendTargetOutsideDismiss,
			onPointerDownOutside: preventAgentSendTargetOutsideDismiss,
			children: scopes.length > 1 ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuLabel, { children: translate("auto.components.editor.NotesSendMenu.44dc5e60a6", "Send notes") }), scopes.map((scope) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuSub, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSubTrigger, {
				disabled: scope.notes.length === 0,
				className: "[&>svg:last-child]:ml-0",
				onPointerEnter: () => openTargetMode(scope),
				onFocus: () => openTargetMode(scope),
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(NoteScopeMenuRow, {
					label: scope.label,
					count: scope.notes.length
				})
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSubContent, {
				className: "min-w-[180px]",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ReviewNotesSendMenuContent, {
					worktreeId,
					groupId,
					prompt: scope.prompt,
					promptDelivery: "submit-after-ready",
					launchSource: "notes_send",
					onPromptDelivered: () => markDelivered(scope.notes)
				})
			})] }, scope.id))] }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ReviewNotesSendMenuContent, {
				worktreeId,
				groupId,
				prompt: defaultScope?.prompt ?? "",
				promptDelivery: "submit-after-ready",
				launchSource: "notes_send",
				onPromptDelivered: () => {
					if (defaultScope) markDelivered(defaultScope.notes);
				}
			})
		})]
	});
}
function preventAgentSendTargetOutsideDismiss(event) {
	const target = event.detail.originalEvent.target;
	if (!(target instanceof Element)) return;
	if (target.closest("[data-agent-send-target=\"eligible\"], [data-agent-send-target=\"disabled\"], [data-agent-send-target=\"sending\"]")) event.preventDefault();
}
function NoteScopeMenuRow({ label, count }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
		className: "grid min-w-0 flex-1 grid-cols-[minmax(0,1fr)_auto] items-center gap-3",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "truncate",
			children: label
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "text-[11px] tabular-nums text-muted-foreground",
			children: count
		})]
	});
}
export { NotesSendMenu as t };
