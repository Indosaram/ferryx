import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { l as createLucideIcon, n as cn, t as Button } from "./button-DszXJEV6.js";
import { n as getWorkspaceStatusVisualMeta } from "./workspace-status-wl52y3xd.js";
import { t as ChevronDown } from "./chevron-down-BRkP96Md.js";
import { t as Funnel } from "./funnel-sY2hg24b.js";
import { t as Search } from "./search-DK1nVA6d.js";
import { $g as getBuiltinTheme, Cs as buildFontFamily, Go as parseTerminalKittyKeyboardFlags, Rg as resolveTerminalFontWeights, n_ as resolveEffectiveTerminalAppearance, qs as configureLazyArabicShapingJoiner, t as useAppStore } from "./store-CgXrfmaH.js";
import { t as X } from "./x-BrGKE4uz.js";
import { k as normalizeTerminalShortcutPolicy, w as keybindingMatchesAction } from "./plugin-manifest-Bs-50M_g.js";
import { n as agentTypeToIconAgent, r as formatAgentTypeLabel } from "./agent-status-3vUKbY6l.js";
import { a as DropdownMenuLabel, i as DropdownMenuItem, l as DropdownMenuSeparator, m as DropdownMenuTrigger, n as DropdownMenuCheckboxItem, r as DropdownMenuContent, t as DropdownMenu } from "./dropdown-menu-Dth6LPK-.js";
import { t as buildLocalConptyTerminalOptions } from "./windows-pty-compatibility-XujC9UTf.js";
import { t as isEditableTarget } from "./editable-target-DLhIk4Uh.js";
import { Et as resolveTerminalLigaturesEnabled, d as normalizeTerminalFastScrollSensitivity, f as normalizeTerminalScrollSensitivity, m as resolveTerminalMinimumContrastRatio, n as composeActiveTerminalTheme, p as resolveTerminalCursorInactiveStyle, s as normalizeTerminalLineHeight, u as buildDefaultTerminalOptions } from "./terminal-appearance-D3oO-Ew5.js";
import { t as getShortcutPlatform } from "./shortcut-platform-BbPBGzth.js";
import { t as ShortcutKeyCombo } from "./ShortcutKeyCombo-Ch456Md0.js";
import { r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
import { t as Input } from "./input-DV5rpysh.js";
import { n as agentStateLabel, t as AgentStateDot } from "./AgentStateDot-DFt63YGw.js";
import { t as AgentIcon } from "./agent-catalog-CBF2CV5Q.js";
import { n as useSystemPrefersDark } from "./use-system-prefers-dark-QSo6mmSW.js";
import { o as dashboardCardDisplayState } from "./dashboard-snapshot-B9IiTV8p.js";
import { t as countAgentMapCards } from "./agent-map-filter-CTyDhUZY.js";
import { D as f, E as activateOrcaTerminalUnicodeProvider, M as TerminalKittyKeyboardModeTracker, N as subscribeToTerminalUserInput, O as TerminalLigaturesAddon, P as An, S as attachTerminalMouseWheelMultiplier, T as installWindowsCtrlAltChordRepair, a as prefetchLayoutBaseCharacters, b as installTerminalImeCandidateAnchor, c as installTerminalImeCompositionTracker, f as TERMINAL_PASTE_MAX_BYTES, i as getLayoutBaseCharacterForCode, j as useEffectiveMacOptionAsAlt, l as planTerminalPasteWithYield, m as isTerminalHttpLinkActivation, n as resolveTerminalShortcutAction, o as installTerminalImeNativeTextForwarder, r as createTerminalNativeOnlyShortcutTracker, u as executeTerminalPastePlan, v as installGuardedLinkProviderRegistration, w as normalizeTerminalTuiMouseWheelMultiplier, y as b } from "./terminal-shortcut-policy-BOkUsz_T.js";
import { n as resolveTerminalPasteRuntime } from "./terminal-paste-runtime-pAfrhxQJ.js";
var SquareArrowOutUpRight = createLucideIcon("square-arrow-out-up-right", [
	["path", {
		d: "M21 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h6",
		key: "y09zxi"
	}],
	["path", {
		d: "m21 3-9 9",
		key: "mpx6sq"
	}],
	["path", {
		d: "M15 3h6v6",
		key: "1q9fwt"
	}]
]);
const EMPTY_DASHBOARD_FILTERS = {
	projects: [],
	workspaceStatuses: [],
	reviewStates: []
};
function activeDashboardFilterCount(filters) {
	return filters.projects.length + filters.workspaceStatuses.length + filters.reviewStates.length;
}
function cardSearchText(card) {
	return [
		card.worktreeName,
		card.repoName,
		card.agentType,
		card.conversationName,
		card.task,
		card.lastUserMessage,
		card.lastAgentMessage,
		card.askSummary,
		card.review ? `#${card.review.number}` : "",
		...card.subagents?.map((subagent) => subagent.name) ?? []
	].filter(Boolean).join(" ").toLocaleLowerCase();
}
function workspaceSearchText(workspace) {
	return [
		workspace.worktreeName,
		workspace.repoName,
		workspace.review ? `#${workspace.review.number}` : ""
	].filter(Boolean).join(" ").toLocaleLowerCase();
}
function workspaceMatchesFilters(workspace, normalizedQuery, filters) {
	const reviewState = workspace.review?.state ?? (workspace.hasReview ? null : "none");
	return (normalizedQuery.length === 0 || workspaceSearchText(workspace).includes(normalizedQuery)) && (filters.projects.length === 0 || filters.projects.includes(workspace.repoId)) && (filters.workspaceStatuses.length === 0 || workspace.workspaceStatusId !== void 0 && filters.workspaceStatuses.includes(workspace.workspaceStatusId)) && (filters.reviewStates.length === 0 || reviewState !== null && filters.reviewStates.includes(reviewState));
}
function filterDashboardCards(cards, query, filters) {
	const normalizedQuery = query.trim().toLocaleLowerCase();
	return cards.filter((card) => {
		const reviewState = card.review?.state ?? (card.hasReview ? null : "none");
		return (normalizedQuery.length === 0 || cardSearchText(card).includes(normalizedQuery)) && (filters.projects.length === 0 || filters.projects.includes(card.repoId)) && (filters.workspaceStatuses.length === 0 || card.workspaceStatusId !== void 0 && filters.workspaceStatuses.includes(card.workspaceStatusId)) && (filters.reviewStates.length === 0 || reviewState !== null && filters.reviewStates.includes(reviewState));
	});
}
function filterDashboardWorkspaces(workspaces, query, filters) {
	const normalizedQuery = query.trim().toLocaleLowerCase();
	return workspaces.filter((workspace) => workspaceMatchesFilters(workspace, normalizedQuery, filters));
}
function toggleDashboardFilter(values, value) {
	return values.includes(value) ? values.filter((candidate) => candidate !== value) : [...values, value];
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function ActiveChip({ label, onRemove }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
		className: "inline-flex h-[22px] items-center gap-1 rounded-full border border-border bg-muted/55 pr-1 pl-2 text-[11px]",
		children: [label, /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
			type: "button",
			onClick: onRemove,
			"aria-label": translate("dashboardPopout.filters.remove", "Remove {{label}} filter", { label }),
			className: "rounded-full text-muted-foreground hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "size-3" })
		})]
	});
}
function AgentDashboardFilterChips({ filters, projects, statuses, reviewLabel, showAgentlessWorkspaces, onProjectToggle, onStatusToggle, onReviewToggle, onAgentlessWorkspacesToggle, onClear }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex shrink-0 flex-wrap items-center gap-1 border-b border-border px-3 py-2",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "mr-0.5 text-[10px] font-semibold uppercase tracking-[0.05em] text-muted-foreground",
				children: translate("dashboardPopout.filters.active", "Filters")
			}),
			filters.projects.map((id) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ActiveChip, {
				label: projects.find((option) => option.id === id)?.label ?? id,
				onRemove: () => onProjectToggle(id)
			}, `project:${id}`)),
			filters.workspaceStatuses.map((id) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ActiveChip, {
				label: statuses.find((option) => option.id === id)?.label ?? id,
				onRemove: () => onStatusToggle(id)
			}, `status:${id}`)),
			filters.reviewStates.map((id) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ActiveChip, {
				label: reviewLabel(id),
				onRemove: () => onReviewToggle(id)
			}, `review:${id}`)),
			showAgentlessWorkspaces ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ActiveChip, {
				label: translate("dashboardPopout.map.filters.agentlessWorkspaces", "Workspaces without agents"),
				onRemove: onAgentlessWorkspacesToggle
			}) : null,
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				variant: "link",
				size: "xs",
				onClick: onClear,
				className: "h-[22px] px-1 text-[11px] text-muted-foreground",
				children: translate("dashboardPopout.filters.clear", "Clear")
			})
		]
	});
}
function FilterOptionCount({ count }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		className: "ml-auto text-[11px] tabular-nums text-muted-foreground",
		children: count
	});
}
function AgentMapContentFilterItems({ showAgentlessWorkspaces, agentlessWorkspaceCount, onShowAgentlessWorkspacesChange, showOrchestrationLinks, onShowOrchestrationLinksChange }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
		/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuLabel, { children: translate("dashboardPopout.map.filters.workspaceVisibility", "Map content") }),
		/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuCheckboxItem, {
			checked: showAgentlessWorkspaces,
			onCheckedChange: (checked) => onShowAgentlessWorkspacesChange(checked === true),
			onSelect: (event) => event.preventDefault(),
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "truncate",
				children: translate("dashboardPopout.map.filters.agentlessWorkspaces", "Workspaces without agents")
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(FilterOptionCount, { count: agentlessWorkspaceCount })]
		}),
		/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuCheckboxItem, {
			checked: showOrchestrationLinks,
			onCheckedChange: (checked) => onShowOrchestrationLinksChange(checked === true),
			onSelect: (event) => event.preventDefault(),
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "truncate",
				children: translate("dashboardPopout.map.filters.orchestrationLinks", "Orchestration links")
			})
		}),
		/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {})
	] });
}
var AGENT_STATE_ROWS = [
	{
		state: "attention",
		dotState: "waiting"
	},
	{
		state: "working",
		dotState: "working"
	},
	{
		state: "done",
		dotState: "done"
	},
	{
		state: "idle",
		dotState: "idle"
	}
];
function agentStateLabel$1(state) {
	switch (state) {
		case "attention": return translate("dashboardPopout.bucket.attention", "Needs You");
		case "working": return translate("dashboardPopout.bucket.working", "Working");
		case "done": return translate("dashboardPopout.bucket.done", "Done");
		case "idle": return translate("dashboardPopout.bucket.idle", "Idle");
	}
}
function countBy(cards, value) {
	const counts = /* @__PURE__ */ new Map();
	for (const card of cards) {
		const key = value(card);
		counts.set(key, (counts.get(key) ?? 0) + 1);
	}
	return counts;
}
function workspaceStatusOptions(cards, configured) {
	const counts = countBy(cards, (card) => card.workspaceStatusId ?? "");
	if (configured) return configured.map((option) => ({
		...option,
		count: counts.get(option.id) ?? 0
	}));
	const options = /* @__PURE__ */ new Map();
	for (const card of cards) {
		if (!card.workspaceStatusId || options.has(card.workspaceStatusId)) continue;
		options.set(card.workspaceStatusId, {
			id: card.workspaceStatusId,
			label: card.workspaceStatusLabel ?? card.workspaceStatusId,
			color: card.workspaceStatusColor,
			count: counts.get(card.workspaceStatusId) ?? 0
		});
	}
	return [...options.values()];
}
function projectOptions(cards, configured) {
	const counts = countBy(cards, (card) => card.repoId);
	if (configured) return configured.map((option) => ({
		...option,
		count: counts.get(option.id) ?? 0
	}));
	const options = /* @__PURE__ */ new Map();
	for (const card of cards) if (!options.has(card.repoId)) options.set(card.repoId, {
		id: card.repoId,
		label: card.repoName,
		count: counts.get(card.repoId) ?? 0
	});
	return [...options.values()];
}
var REVIEW_OPTIONS = [
	"open",
	"draft",
	"merged",
	"closed",
	"none"
];
function reviewStateLabel(state) {
	switch (state) {
		case "open": return translate("dashboardPopout.filters.review.open", "Open");
		case "draft": return translate("dashboardPopout.filters.review.draft", "Draft");
		case "merged": return translate("dashboardPopout.filters.review.merged", "Merged");
		case "closed": return translate("dashboardPopout.filters.review.closed", "Closed");
		case "none": return translate("dashboardPopout.filters.review.none", "No review");
	}
}
function AgentDashboardToolbar({ cards, filterOptions, filteredCount, query, onQueryChange, filters, onFiltersChange, agentStates, onAgentStateToggle, onAgentStatesReset, showAgentlessWorkspaces, agentlessWorkspaceCount = 0, onShowAgentlessWorkspacesChange, showOrchestrationLinks, onShowOrchestrationLinksChange, searchInputRef }) {
	const isMac = navigator.userAgent.includes("Mac");
	const projects = projectOptions(cards, filterOptions?.projects);
	const statuses = workspaceStatusOptions(cards, filterOptions?.workspaceStatuses);
	const reviewCounts = countBy(cards, (card) => card.review?.state ?? (card.hasReview ? "unknown" : "none"));
	const agentStateCounts = agentStates ? countAgentMapCards(cards) : null;
	const mutedStateCount = agentStates ? AGENT_STATE_ROWS.length - agentStates.size : 0;
	const activeCount = activeDashboardFilterCount(filters) + mutedStateCount + (showAgentlessWorkspaces === true ? 1 : 0) + (showOrchestrationLinks === false ? 1 : 0);
	const toggleProject = (id) => onFiltersChange({
		...filters,
		projects: toggleDashboardFilter(filters.projects, id)
	});
	const toggleStatus = (id) => onFiltersChange({
		...filters,
		workspaceStatuses: toggleDashboardFilter(filters.workspaceStatuses, id)
	});
	const toggleReview = (id) => onFiltersChange({
		...filters,
		reviewStates: toggleDashboardFilter(filters.reviewStates, id)
	});
	const clearFilters = () => {
		onFiltersChange({
			projects: [],
			workspaceStatuses: [],
			reviewStates: []
		});
		onAgentStatesReset?.();
		onShowAgentlessWorkspacesChange?.(false);
		onShowOrchestrationLinksChange?.(true);
	};
	const reviewLabel = (id) => translate("dashboardPopout.filters.reviewChip", "Review: {{state}}", { state: reviewStateLabel(id) });
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex shrink-0 items-center gap-2 border-b border-border px-3 py-2",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "relative min-w-0 flex-1",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Search, { className: "pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted-foreground" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
						ref: searchInputRef,
						value: query,
						onChange: (event) => onQueryChange(event.target.value),
						placeholder: translate("dashboardPopout.search.placeholder", "Search worktree, project, or agent…"),
						"aria-label": translate("dashboardPopout.search.label", "Search agents"),
						className: "h-7 bg-muted/55 pr-16 pl-7 text-xs"
					}),
					query ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						variant: "ghost",
						size: "icon-xs",
						onClick: () => onQueryChange(""),
						"aria-label": translate("dashboardPopout.search.clear", "Clear search"),
						className: "absolute top-1/2 right-0.5 -translate-y-1/2 text-muted-foreground",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "size-3" })
					}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ShortcutKeyCombo, {
						keys: [isMac ? "⌘" : "Ctrl", "K"],
						className: "pointer-events-none absolute top-1/2 right-1 -translate-y-1/2",
						keyCapClassName: "min-w-4 px-1 py-0 text-[9px] shadow-none",
						separatorClassName: "text-[9px] text-muted-foreground"
					})
				]
			}),
			query || activeCount > 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "shrink-0 text-[11px] tabular-nums text-muted-foreground",
				children: translate("dashboardPopout.search.results", "{{shown}} of {{total}} shown", {
					shown: filteredCount,
					total: cards.length
				})
			}) : null,
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenu, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuTrigger, {
				asChild: true,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
					variant: "outline",
					size: "xs",
					className: cn("h-7 gap-1.5 px-2 text-xs", activeCount > 0 && "border-foreground/25"),
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Funnel, { className: "size-3" }),
						translate("dashboardPopout.filters.label", "Filter"),
						activeCount > 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "rounded-full bg-foreground px-1.5 py-px text-[10px] leading-none text-background",
							children: activeCount
						}) : null,
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronDown, { className: "size-3" })
					]
				})
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuContent, {
				align: "end",
				className: "w-64",
				sideOffset: 6,
				children: [
					showAgentlessWorkspaces !== void 0 && onShowAgentlessWorkspacesChange && onShowOrchestrationLinksChange ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentMapContentFilterItems, {
						showAgentlessWorkspaces,
						agentlessWorkspaceCount,
						onShowAgentlessWorkspacesChange,
						showOrchestrationLinks: showOrchestrationLinks !== false,
						onShowOrchestrationLinksChange
					}) : null,
					agentStates && agentStateCounts ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuLabel, { children: translate("dashboardPopout.map.filters.showStates", "Agent states") }),
						AGENT_STATE_ROWS.map(({ state, dotState }) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuCheckboxItem, {
							checked: agentStates.has(state),
							onCheckedChange: () => onAgentStateToggle?.(state),
							onSelect: (event) => event.preventDefault(),
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentStateDot, {
									state: dotState,
									size: "md"
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "truncate",
									children: agentStateLabel$1(state)
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(FilterOptionCount, { count: agentStateCounts[state] })
							]
						}, state)),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {})
					] }) : null,
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuLabel, { children: translate("dashboardPopout.filters.project", "Project") }),
					projects.map((option) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuCheckboxItem, {
						checked: filters.projects.includes(option.id),
						onCheckedChange: () => toggleProject(option.id),
						onSelect: (event) => event.preventDefault(),
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "truncate",
							children: option.label
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(FilterOptionCount, { count: option.count })]
					}, option.id)),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuLabel, { children: translate("dashboardPopout.filters.workspaceStatus", "Workspace status") }),
					statuses.map((option) => {
						const meta = getWorkspaceStatusVisualMeta({
							id: option.id,
							label: option.label,
							color: option.color
						});
						return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuCheckboxItem, {
							checked: filters.workspaceStatuses.includes(option.id),
							onCheckedChange: () => toggleStatus(option.id),
							onSelect: (event) => event.preventDefault(),
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: cn("size-2 rounded-full", meta.swatch) }),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "truncate",
									children: option.label
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(FilterOptionCount, { count: option.count })
							]
						}, option.id);
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuLabel, { children: translate("dashboardPopout.filters.reviewStatus", "PR / MR status") }),
					REVIEW_OPTIONS.map((option) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuCheckboxItem, {
						checked: filters.reviewStates.includes(option),
						onCheckedChange: () => toggleReview(option),
						onSelect: (event) => event.preventDefault(),
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: reviewStateLabel(option) }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(FilterOptionCount, { count: reviewCounts.get(option) ?? 0 })]
					}, option)),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
						disabled: activeCount === 0,
						onSelect: clearFilters,
						className: "text-muted-foreground",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "size-3.5" }), translate("dashboardPopout.filters.clearAll", "Clear all filters")]
					})
				]
			})] })
		]
	}), activeCount > 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentDashboardFilterChips, {
		filters,
		projects,
		statuses,
		reviewLabel,
		showAgentlessWorkspaces: showAgentlessWorkspaces === true,
		onProjectToggle: toggleProject,
		onStatusToggle: toggleStatus,
		onReviewToggle: toggleReview,
		onAgentlessWorkspacesToggle: () => onShowAgentlessWorkspacesChange?.(false),
		onClear: clearFilters
	}) : null] });
}
function replayPreviewConnectionSnapshot(args) {
	const { snapshot, kittyKeyboardModes } = args;
	const provenFlags = parseTerminalKittyKeyboardFlags(snapshot.kittyKeyboardFlags) ?? (kittyKeyboardModes.hasProvenBaseline ? kittyKeyboardModes.snapshotFlags : void 0);
	kittyKeyboardModes.resetForSnapshot();
	if (snapshot.scrollbackAnsi) args.write(snapshot.scrollbackAnsi, false);
	if (snapshot.data) args.write(snapshot.data, false);
	if (snapshot.pendingEscapeTailAnsi) args.write(snapshot.pendingEscapeTailAnsi, false);
	if (provenFlags !== void 0) kittyKeyboardModes.restoreSnapshotFlags(provenFlags);
	for (const chunk of args.replay) args.write(chunk.data, chunk.mode === "live");
}
function buildPreviewAppearanceOptions(settings, macOptionIsMeta) {
	const cursorStyle = settings?.terminalCursorStyle ?? "block";
	const fontWeights = resolveTerminalFontWeights(settings?.terminalFontWeight);
	return {
		fontSize: settings?.terminalFontSize ?? 14,
		fontFamily: buildFontFamily(settings?.terminalFontFamily ?? ""),
		fontWeight: fontWeights.fontWeight,
		fontWeightBold: fontWeights.fontWeightBold,
		cursorStyle,
		cursorInactiveStyle: resolveTerminalCursorInactiveStyle(cursorStyle),
		cursorBlink: settings?.terminalCursorBlink ?? true,
		scrollSensitivity: normalizeTerminalScrollSensitivity(settings?.terminalScrollSensitivity),
		fastScrollSensitivity: normalizeTerminalFastScrollSensitivity(settings?.terminalFastScrollSensitivity),
		lineHeight: normalizeTerminalLineHeight(settings?.terminalLineHeight),
		wordSeparator: settings?.terminalWordSeparator,
		macOptionIsMeta,
		allowTransparency: settings?.terminalBackgroundOpacity !== void 0 && settings.terminalBackgroundOpacity < 1
	};
}
function buildPreviewTerminalOptions(args) {
	const hostCompatibility = {
		...args.terminalInput?.localWindowsConpty ? buildLocalConptyTerminalOptions(args.terminalInput.osRelease) : {},
		...args.terminalInput && !args.terminalInput.kittyKeyboardAdvertised ? { vtExtensions: { kittyKeyboard: false } } : {}
	};
	return {
		...buildDefaultTerminalOptions(),
		...buildPreviewAppearanceOptions(args.settings, args.macOptionIsMeta),
		...hostCompatibility,
		cols: args.cols,
		rows: args.rows,
		scrollback: args.scrollback,
		theme: args.theme ?? void 0,
		minimumContrastRatio: resolveTerminalMinimumContrastRatio(args.theme?.background, args.themeMode)
	};
}
var ligatureAddonsByTerminal = /* @__PURE__ */ new WeakMap();
function syncPreviewTerminalLigatures(terminal, settings) {
	const enabled = resolveTerminalLigaturesEnabled(settings?.terminalLigatures, settings?.terminalFontFamily);
	const attached = ligatureAddonsByTerminal.get(terminal);
	if (enabled === Boolean(attached)) return;
	if (!enabled) {
		try {
			attached?.dispose();
		} catch {}
		ligatureAddonsByTerminal.delete(terminal);
		return;
	}
	try {
		const addon = new TerminalLigaturesAddon();
		terminal.loadAddon(addon);
		ligatureAddonsByTerminal.set(terminal, addon);
		terminal.refresh(0, terminal.rows - 1);
	} catch {}
}
function installPreviewTerminalLinks(terminal) {
	installGuardedLinkProviderRegistration(terminal);
	terminal.loadAddon(new b((event, uri) => {
		if (!isTerminalHttpLinkActivation(event)) return;
		event.preventDefault();
		window.api.shell.openUrl(uri).catch(() => void 0);
		terminal.clearSelection();
	}));
}
function installPreviewTerminalCompatibility(terminal, deps) {
	terminal.loadAddon(new f());
	activateOrcaTerminalUnicodeProvider(terminal);
	installWindowsCtrlAltChordRepair(terminal);
	installPreviewTerminalLinks(terminal);
	syncPreviewTerminalLigatures(terminal, deps.getSettings());
	attachTerminalMouseWheelMultiplier(terminal, { getTuiMouseWheelMultiplier: () => normalizeTerminalTuiMouseWheelMultiplier(deps.getSettings()?.terminalTuiScrollSensitivity) });
	const disposeArabicShapingJoiner = configureLazyArabicShapingJoiner(terminal, () => false);
	const imeAnchorHandler = installTerminalImeCandidateAnchor(terminal);
	return () => {
		if (imeAnchorHandler && terminal.element) {
			terminal.element.removeEventListener("compositionstart", imeAnchorHandler);
			terminal.element.removeEventListener("compositionupdate", imeAnchorHandler);
		}
		disposeArabicShapingJoiner();
	};
}
function createPreviewClipboardPaster(deps) {
	return async (activeElementAtDispatch, source) => {
		let text;
		try {
			text = await window.api.ui.readClipboardText({ maxBytes: TERMINAL_PASTE_MAX_BYTES });
		} catch {
			return;
		}
		const pasteTerminal = deps.getTerminal();
		if (!pasteTerminal || !text) return;
		const targetIsCurrent = () => !deps.isDisposed() && deps.getTerminal() === pasteTerminal && activeElementAtDispatch !== null && document.activeElement === activeElementAtDispatch && deps.container.contains(activeElementAtDispatch);
		if (!targetIsCurrent()) return;
		const platform = getShortcutPlatform();
		await executeTerminalPastePlan(await planTerminalPasteWithYield({
			text,
			source,
			target: {
				kind: "terminal",
				paneId: 0,
				leafId: deps.ptyId,
				ptyId: deps.ptyId,
				runtime: resolveTerminalPasteRuntime({
					platform,
					ptyId: deps.ptyId
				})
			},
			terminalBracketedPasteMode: pasteTerminal.modes.bracketedPasteMode
		}), {
			pasteText: (pasteText) => pasteTerminal.paste(pasteText),
			writePty: (data) => window.api.terminalPreview.input(deps.ptyId, data),
			isTargetCurrent: targetIsCurrent,
			canContinue: () => true
		});
	};
}
function installPreviewImeBridge(terminal, options) {
	if (getShortcutPlatform() !== "darwin") return null;
	const compositionTracker = installTerminalImeCompositionTracker(terminal.element);
	const forwarder = installTerminalImeNativeTextForwarder({
		terminalElement: terminal.element,
		isComposing: () => compositionTracker?.isActive() ?? false,
		sendInput: (data) => terminal.input(data),
		getKittyKeyboardFlags: options.getKittyKeyboardFlags
	});
	return {
		claimKeyEvent: (event) => forwarder?.claimKeyEvent(event) ?? false,
		dispose: () => {
			forwarder?.dispose();
			compositionTracker?.dispose();
		}
	};
}
function resolvePreviewShortcutAction(event, context) {
	const isMac = context.clientPlatform === "darwin";
	const hostPlatform = context.terminalInput?.hostPlatform ?? context.clientPlatform;
	return resolveTerminalShortcutAction(event, isMac, context.macOptionAsAlt, context.optionKeyLocation, context.clientPlatform === "win32", context.keybindings, () => context.terminalInput?.localWindowsConpty === true, context.kittyKeyboardActive, getLayoutBaseCharacterForCode, () => context.terminalInput?.windowsShiftEnterEncoding ?? "alt-enter", () => hostPlatform === "win32", normalizeTerminalShortcutPolicy(context.terminalShortcutPolicy), () => context.terminalInput?.ctrlEnterCsiU === true);
}
function installPreviewTerminalKeyHandler(args) {
	const { terminal } = args;
	const platform = getShortcutPlatform();
	const consumedClipboardKeys = /* @__PURE__ */ new Set();
	const nativeOnlyShortcutTracker = createTerminalNativeOnlyShortcutTracker();
	const consumeEvent = (event) => {
		event.preventDefault();
		event.stopPropagation();
		return false;
	};
	let optionKeyLocation = 0;
	const onModifierDown = (event) => {
		if (event.key === "Alt") optionKeyLocation = event.location;
	};
	const onModifierUp = (event) => {
		if (event.key === "Alt") optionKeyLocation = 0;
	};
	const onWindowBlur = () => {
		optionKeyLocation = 0;
		nativeOnlyShortcutTracker.clear();
	};
	const onNativeOnlyShortcutCompanion = (event) => {
		if (!nativeOnlyShortcutTracker.consumeCompanion(event)) return;
		if (event.type === "keypress") event.preventDefault();
		event.stopImmediatePropagation();
	};
	const onNativeOnlyBeforeInput = (event) => {
		if (!(event instanceof InputEvent) || !nativeOnlyShortcutTracker.shouldSuppressBeforeInput(event)) return;
		event.preventDefault();
		event.stopImmediatePropagation();
	};
	if (platform === "darwin") prefetchLayoutBaseCharacters();
	window.addEventListener("keydown", onModifierDown, true);
	window.addEventListener("keyup", onModifierUp, true);
	window.addEventListener("keypress", onNativeOnlyShortcutCompanion, true);
	window.addEventListener("keyup", onNativeOnlyShortcutCompanion, true);
	window.addEventListener("beforeinput", onNativeOnlyBeforeInput, true);
	window.addEventListener("blur", onWindowBlur);
	terminal.attachCustomKeyEventHandler((event) => {
		if (args.claimImeKeyEvent(event)) return false;
		if (event.type !== "keydown") {
			const keyIdentity = event.code || event.key;
			if (consumedClipboardKeys.has(keyIdentity)) {
				if (event.type === "keyup") consumedClipboardKeys.delete(keyIdentity);
				return consumeEvent(event);
			}
			return true;
		}
		nativeOnlyShortcutTracker.prepareKeyDown(event);
		const keybindings = useAppStore.getState().keybindings;
		if (keybindingMatchesAction("terminal.copySelection", event, platform, keybindings)) {
			const selection = terminal.getSelection();
			if (!selection && platform !== "darwin" && event.ctrlKey && !event.metaKey && !event.altKey && !event.shiftKey) return true;
			const keyIdentity = event.code || event.key;
			const firstKeydown = !consumedClipboardKeys.has(keyIdentity);
			consumedClipboardKeys.add(keyIdentity);
			if (firstKeydown && selection) window.api.ui.writeTerminalClipboardText(selection).catch(() => void 0);
			return consumeEvent(event);
		}
		if (!((platform === "darwin" ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey) && !event.altKey && !event.shiftKey && event.key.toLowerCase() === "v") && keybindingMatchesAction("terminal.paste", event, platform, keybindings)) {
			const keyIdentity = event.code || event.key;
			if (!consumedClipboardKeys.has(keyIdentity)) {
				consumedClipboardKeys.add(keyIdentity);
				args.pasteClipboardText(document.activeElement, "keyboard");
			}
			return consumeEvent(event);
		}
		const action = resolvePreviewShortcutAction(event, {
			...args.getShortcutContext(),
			optionKeyLocation
		});
		if (!action) return true;
		switch (action.type) {
			case "sendInput":
				args.sendInput(action.data);
				return consumeEvent(event);
			case "scrollViewport":
				if (action.position === "top") terminal.scrollToTop();
				else terminal.scrollToBottom();
				return consumeEvent(event);
			case "selectAll":
				if (!event.repeat) {
					nativeOnlyShortcutTracker.armKeyDown(event);
					terminal.selectAll();
				}
				return consumeEvent(event);
			case "switchInputSource":
				nativeOnlyShortcutTracker.armKeyDown(event);
				event.stopImmediatePropagation();
				return false;
			case "clearActivePane":
			case "clearPaneTitle":
			case "closeActivePane":
			case "copySelection":
			case "equalizePaneSizes":
			case "focusPane":
			case "setTitle":
			case "splitActivePane":
			case "toggleExpandActivePane":
			case "toggleSearch": return consumeEvent(event);
		}
	});
	return () => {
		window.removeEventListener("keydown", onModifierDown, true);
		window.removeEventListener("keyup", onModifierUp, true);
		window.removeEventListener("keypress", onNativeOnlyShortcutCompanion, true);
		window.removeEventListener("keyup", onNativeOnlyShortcutCompanion, true);
		window.removeEventListener("beforeinput", onNativeOnlyBeforeInput, true);
		window.removeEventListener("blur", onWindowBlur);
	};
}
var FIT_REQUEST_DEBOUNCE_MS = 200;
var FIT_MIN_COLS = 20;
var FIT_MAX_COLS = 240;
var FIT_MIN_ROWS = 8;
var FIT_MAX_ROWS = 120;
function clampGridAxis(value, min, max) {
	return Math.min(max, Math.max(min, value));
}
function createPreviewGridClaim(args) {
	let lastRequestedFit = null;
	let timer = null;
	let disposed = false;
	const request = () => {
		const terminal = args.getTerminal();
		if (disposed || !terminal) return;
		const screen = args.container.querySelector(".xterm-screen");
		const box = args.container.parentElement;
		if (!screen || !box) return;
		const cellWidth = screen.offsetWidth / Math.max(1, terminal.cols);
		const cellHeight = screen.offsetHeight / Math.max(1, terminal.rows);
		if (!Number.isFinite(cellWidth) || !Number.isFinite(cellHeight) || cellWidth <= 0 || cellHeight <= 0 || box.clientWidth <= 0 || box.clientHeight <= 0) return;
		const cols = clampGridAxis(Math.floor(box.clientWidth / cellWidth), FIT_MIN_COLS, FIT_MAX_COLS);
		const rows = clampGridAxis(Math.floor(box.clientHeight / cellHeight), FIT_MIN_ROWS, FIT_MAX_ROWS);
		const fitKey = `${cols}x${rows}`;
		if (fitKey === lastRequestedFit) return;
		lastRequestedFit = fitKey;
		window.api.terminalPreview.fit(args.ptyId, cols, rows).catch(() => void 0);
	};
	const schedule = () => {
		if (disposed) return;
		if (timer) clearTimeout(timer);
		timer = setTimeout(() => {
			timer = null;
			request();
		}, FIT_REQUEST_DEBOUNCE_MS);
	};
	return {
		schedule,
		dispose: () => {
			disposed = true;
			if (timer) {
				clearTimeout(timer);
				timer = null;
			}
		}
	};
}
function installPreviewTerminalAppMenuClipboard({ container, getTerminal, pasteClipboardText }) {
	const offPaste = window.api.ui.onAppMenuPaste(() => {
		const active = document.activeElement;
		if (active && container.contains(active)) pasteClipboardText(active, "app-menu");
	});
	const offSelection = window.api.ui.onAppMenuSelectionAction((action) => {
		const active = document.activeElement;
		const terminal = getTerminal();
		if (!active || !container.contains(active) || isEditableTarget(active) || !terminal) {
			window.api.ui.performNativeSelectionAction(action);
			return;
		}
		if (action === "select-all") {
			terminal.selectAll();
			return;
		}
		const selection = terminal.getSelection();
		if (selection) window.api.ui.writeTerminalClipboardText(selection).catch(() => void 0);
		else window.api.ui.performNativeSelectionAction(action);
	});
	return () => {
		offPaste();
		offSelection();
	};
}
var import_react = /* @__PURE__ */ __toESM(require_react());
var PREVIEW_SCROLLBACK_ROWS = 24;
var PREVIEW_SCROLLBACK_BUFFER_ROWS = 1e3;
var FALLBACK_COLS = 80;
var FALLBACK_ROWS = 24;
var RESYNC_RETRY_DELAY_MS = 150;
function clamp(value, min, max) {
	return Math.min(max, Math.max(min, value));
}
function AgentTerminalPreview({ ptyId, terminalInput = null, className }) {
	const containerRef = (0, import_react.useRef)(null);
	const terminalRef = (0, import_react.useRef)(null);
	const settings = useAppStore((state) => state.settings);
	const systemPrefersDark = useSystemPrefersDark();
	const macOptionAsAlt = useEffectiveMacOptionAsAlt(settings?.terminalMacOptionAsAlt);
	const settingsRef = (0, import_react.useRef)(settings);
	const macOptionAsAltRef = (0, import_react.useRef)(macOptionAsAlt);
	const terminalInputRef = (0, import_react.useRef)(terminalInput);
	const { terminalTheme, terminalMode } = (0, import_react.useMemo)(() => {
		if (!settings) return {
			terminalTheme: null,
			terminalMode: "dark"
		};
		const appearance = resolveEffectiveTerminalAppearance(settings, systemPrefersDark);
		return {
			terminalTheme: composeActiveTerminalTheme(appearance.theme ?? getBuiltinTheme(appearance.themeName), settings),
			terminalMode: appearance.mode
		};
	}, [settings, systemPrefersDark]);
	const [ptyGone, setPtyGone] = (0, import_react.useState)(false);
	(0, import_react.useLayoutEffect)(() => {
		settingsRef.current = settings;
		macOptionAsAltRef.current = macOptionAsAlt;
		terminalInputRef.current = terminalInput;
	}, [
		settings,
		macOptionAsAlt,
		terminalInput
	]);
	(0, import_react.useEffect)(() => {
		setPtyGone(false);
		const container = containerRef.current;
		if (!container) return;
		let disposed = false;
		let terminal = null;
		let offData = null;
		let userInputDisposable = null;
		let imeBridge = null;
		let disposeKeyHandler = null;
		let disposeTerminalCompatibility = null;
		const kittyKeyboardModes = new TerminalKittyKeyboardModeTracker();
		let refreshInFlight = false;
		let refreshAgain = false;
		let retryTimer = null;
		const pendingLivePayloads = [];
		const fitToBox = () => {
			const screen = container.querySelector(".xterm-screen");
			const box = container.parentElement;
			if (!screen || !box || !terminal) return;
			const scale = Math.min(1, box.clientWidth / Math.max(1, screen.offsetWidth));
			container.style.transform = scale < 1 ? `scale(${scale})` : "";
			const cellHeight = screen.offsetHeight / Math.max(1, terminal.rows);
			const anchorTop = (terminal.buffer.active.cursorY + 1) * cellHeight * scale <= box.clientHeight;
			box.style.alignItems = anchorTop ? "flex-start" : "flex-end";
			container.style.transformOrigin = anchorTop ? "top left" : "bottom left";
		};
		let fitScheduled = false;
		const scheduleFit = () => {
			if (fitScheduled) return;
			fitScheduled = true;
			requestAnimationFrame(() => {
				fitScheduled = false;
				fitToBox();
			});
		};
		const gridClaim = createPreviewGridClaim({
			ptyId,
			container,
			getTerminal: () => terminal
		});
		const boxResizeObserver = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(() => {
			scheduleFit();
			gridClaim.schedule();
		});
		if (container.parentElement) boxResizeObserver?.observe(container.parentElement);
		boxResizeObserver?.observe(container);
		let replayDepth = 0;
		const writeReplayed = (chunk, onDone, live = false) => {
			if (live) kittyKeyboardModes.scan(chunk);
			else kittyKeyboardModes.scanReplay(chunk);
			replayDepth++;
			terminal?.write(chunk, () => {
				replayDepth--;
				scheduleFit();
				onDone?.();
			});
		};
		const writeLive = (payload) => {
			if (!terminal) {
				pendingLivePayloads.push(payload);
				return;
			}
			writeReplayed(payload.data, () => {
				if (!disposed) window.api.terminalPreview.ack(ptyId, payload.bytes);
			}, true);
		};
		const pasteClipboardText = createPreviewClipboardPaster({
			ptyId,
			container,
			getTerminal: () => terminal,
			isDisposed: () => disposed
		});
		const disposeImeNativeTextBridge = () => {
			imeBridge?.dispose();
			imeBridge = null;
		};
		const installImeNativeTextBridge = () => {
			if (terminal) imeBridge = installPreviewImeBridge(terminal, { getKittyKeyboardFlags: () => kittyKeyboardModes.flags });
		};
		const installKeyHandler = () => {
			if (!terminal) return;
			disposeKeyHandler = installPreviewTerminalKeyHandler({
				terminal,
				claimImeKeyEvent: (event) => imeBridge?.claimKeyEvent(event) ?? false,
				pasteClipboardText: (activeElement, source) => void pasteClipboardText(activeElement, source),
				sendInput: (data) => terminal?.input(data),
				getShortcutContext: () => ({
					clientPlatform: getShortcutPlatform(),
					macOptionAsAlt: macOptionAsAltRef.current,
					keybindings: useAppStore.getState().keybindings,
					terminalInput: terminalInputRef.current,
					kittyKeyboardActive: () => kittyKeyboardModes.flags > 0,
					terminalShortcutPolicy: settingsRef.current?.terminalShortcutPolicy
				})
			});
		};
		const installTerminalCompatibility = () => {
			if (!terminal) return;
			disposeTerminalCompatibility = installPreviewTerminalCompatibility(terminal, { getSettings: () => settingsRef.current });
		};
		const installInputRouting = () => {
			if (!terminal) return;
			let pendingUserInputSignals = 0;
			userInputDisposable = subscribeToTerminalUserInput(terminal, () => {
				pendingUserInputSignals = Math.min(32, pendingUserInputSignals + 1);
			});
			terminal.onData((data) => {
				const signaledUserInput = pendingUserInputSignals > 0;
				if (signaledUserInput) pendingUserInputSignals--;
				if (userInputDisposable ? !signaledUserInput : replayDepth > 0) return;
				window.api.terminalPreview.input(ptyId, data);
			});
		};
		const replayConnection = (connection, replaceExisting, requestRefresh) => {
			const snap = connection.snapshot;
			if (!terminal) {
				terminal = new An(buildPreviewTerminalOptions({
					settings: settingsRef.current,
					terminalInput: terminalInputRef.current,
					macOptionIsMeta: macOptionAsAltRef.current === "true",
					theme: terminalTheme,
					themeMode: terminalMode,
					cols: clamp(snap.cols ?? FALLBACK_COLS, 2, 500),
					rows: clamp(snap.rows ?? FALLBACK_ROWS, 2, 200),
					scrollback: PREVIEW_SCROLLBACK_BUFFER_ROWS
				}));
				try {
					terminal.open(container);
				} catch {
					terminal.dispose();
					terminal = null;
					return;
				}
				terminalRef.current = terminal;
				installTerminalCompatibility();
				installInputRouting();
				installImeNativeTextBridge();
				installKeyHandler();
			} else if (replaceExisting) {
				terminal.resize(clamp(snap.cols ?? FALLBACK_COLS, 2, 500), clamp(snap.rows ?? FALLBACK_ROWS, 2, 200));
				terminal.reset();
			}
			replayPreviewConnectionSnapshot({
				snapshot: snap,
				replay: connection.replay,
				kittyKeyboardModes,
				write: (chunk, live) => writeReplayed(chunk, void 0, live)
			});
			for (const payload of pendingLivePayloads.splice(0)) writeLive(payload);
			if (connection.resyncRequired) {
				refreshAgain = false;
				writeReplayed("", () => {
					if (disposed || retryTimer) return;
					retryTimer = setTimeout(() => {
						retryTimer = null;
						requestRefresh();
					}, RESYNC_RETRY_DELAY_MS);
				});
			} else if (refreshAgain) {
				refreshAgain = false;
				writeReplayed("", requestRefresh);
			}
			scheduleFit();
			gridClaim.schedule();
			terminal.focus();
		};
		const setup = async (replaceExisting = false) => {
			if (refreshInFlight) {
				refreshAgain = true;
				return;
			}
			refreshInFlight = true;
			const connection = await window.api.terminalPreview.connect(ptyId, { scrollbackRows: PREVIEW_SCROLLBACK_ROWS });
			if (disposed) return;
			if (!connection.snapshot) {
				refreshInFlight = false;
				setPtyGone(true);
				offData?.();
				offData = null;
				userInputDisposable?.dispose();
				userInputDisposable = null;
				disposeImeNativeTextBridge();
				disposeTerminalCompatibility?.();
				disposeTerminalCompatibility = null;
				disposeKeyHandler?.();
				disposeKeyHandler = null;
				terminal?.dispose();
				terminal = null;
				terminalRef.current = null;
				window.api.terminalPreview.unsubscribe(ptyId);
				return;
			}
			refreshInFlight = false;
			if (!connection.resyncRequired && retryTimer) {
				clearTimeout(retryTimer);
				retryTimer = null;
			}
			replayConnection(connection, replaceExisting, () => void setup(true));
		};
		const disposeAppMenuClipboard = installPreviewTerminalAppMenuClipboard({
			container,
			getTerminal: () => terminal,
			pasteClipboardText
		});
		offData = window.api.terminalPreview.onData((payload) => {
			if (payload.ptyId !== ptyId) return;
			if (payload.type === "resync") {
				setup(true);
				return;
			}
			writeLive(payload);
		});
		setup();
		return () => {
			disposed = true;
			if (retryTimer) clearTimeout(retryTimer);
			gridClaim.dispose();
			boxResizeObserver?.disconnect();
			disposeAppMenuClipboard();
			offData?.();
			userInputDisposable?.dispose();
			disposeImeNativeTextBridge();
			disposeTerminalCompatibility?.();
			disposeKeyHandler?.();
			window.api.terminalPreview.unsubscribe(ptyId);
			terminal?.dispose();
			terminalRef.current = null;
		};
	}, [
		ptyId,
		terminalTheme,
		terminalMode
	]);
	(0, import_react.useEffect)(() => {
		const terminal = terminalRef.current;
		if (!terminal) return;
		Object.assign(terminal.options, buildPreviewAppearanceOptions(settings, macOptionAsAlt === "true"));
		syncPreviewTerminalLigatures(terminal, settings);
	}, [settings, macOptionAsAlt]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: cn("relative h-[calc(100vh-140px)] w-full overflow-hidden bg-background p-1.5", className),
		style: terminalTheme?.background ? { backgroundColor: terminalTheme.background } : void 0,
		children: [ptyGone ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "absolute inset-0 flex items-center justify-center px-2.5 py-8 text-center text-[11px] text-muted-foreground",
			children: translate("dashboardPopout.terminal.closed", "No live terminal — this agent's pane has closed.")
		}) : null, /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			"aria-hidden": ptyGone || void 0,
			className: cn("flex h-full w-full items-end overflow-hidden", ptyGone && "invisible"),
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				ref: containerRef,
				className: "origin-bottom-left"
			})
		})]
	});
}
function AgentTerminalFrame({ card, title, previewClassName, onOpenChange, onReveal }) {
	const reveal = () => {
		onReveal({
			repoId: card.repoId,
			worktreeId: card.worktreeId,
			executionHostId: card.executionHostId,
			tabId: card.tabId,
			leafId: card.leafId
		});
		onOpenChange(false);
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
		/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex shrink-0 items-center gap-1.5 px-2.5 py-2",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "inline-flex shrink-0",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentIcon, {
						agent: agentTypeToIconAgent(card.agentType),
						size: 13
					})
				}),
				title,
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
					className: "text-[11px] text-muted-foreground",
					children: [
						formatAgentTypeLabel(card.agentType),
						" ·",
						" ",
						agentStateLabel(dashboardCardDisplayState(card))
					]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
					type: "button",
					variant: "ghost",
					size: "icon-xs",
					className: "ml-auto opacity-70 hover:opacity-100",
					onClick: () => onOpenChange(false),
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "size-4" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "sr-only",
						children: translate("dashboardPopout.terminal.close", "Close")
					})]
				})
			]
		}),
		card.ptyId ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentTerminalPreview, {
			ptyId: card.ptyId,
			terminalInput: card.terminalInput ?? null,
			className: previewClassName
		}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "min-h-0 flex-1 px-2.5 pb-2 text-[11px] text-muted-foreground",
			children: translate("dashboardPopout.terminal.closed", "No live terminal — this agent's pane has closed.")
		}),
		/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "flex shrink-0 items-center gap-1.5 px-2.5 py-1.5",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
				type: "button",
				variant: "outline",
				size: "xs",
				className: "ml-auto",
				onClick: reveal,
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SquareArrowOutUpRight, { className: "size-3" }), translate("dashboardPopout.terminal.focusWorktree", "Open worktree")]
			})
		})
	] });
}
function AgentTerminalDialog({ card, onOpenChange, onReveal }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
		open: card !== null,
		onOpenChange,
		children: card ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogContent, {
			"aria-describedby": void 0,
			className: "flex w-[calc(100vw-40px)] max-w-none flex-col gap-0 p-0 sm:max-w-none",
			showCloseButton: false,
			onEscapeKeyDown: (e) => {
				if (e.target instanceof HTMLElement && e.target.closest(".xterm")) e.preventDefault();
			},
			onOpenAutoFocus: (e) => {
				if (card.ptyId) e.preventDefault();
			},
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentTerminalFrame, {
				card,
				title: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, {
					className: "text-[12px] leading-normal font-semibold",
					children: card.worktreeName
				}),
				onOpenChange,
				onReveal
			})
		}) : null
	});
}
function AgentTerminalPanel({ card, onOpenChange, onReveal, className }) {
	const titleId = (0, import_react.useId)();
	(0, import_react.useEffect)(() => {
		const handleKeyDown = (event) => {
			if (event.key === "Escape" && !event.defaultPrevented && !(event.target instanceof HTMLElement && event.target.closest(".xterm"))) onOpenChange(false);
		};
		document.addEventListener("keydown", handleKeyDown);
		return () => document.removeEventListener("keydown", handleKeyDown);
	}, [onOpenChange]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("section", {
		role: "dialog",
		"data-state": "open",
		"aria-labelledby": titleId,
		className: cn("m-3 flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded-lg border border-border bg-popover text-popover-foreground shadow-[0_10px_24px_rgba(0,0,0,0.18)]", className),
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentTerminalFrame, {
			card,
			title: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("h2", {
				id: titleId,
				className: "text-[12px] leading-normal font-semibold",
				children: card.worktreeName
			}),
			previewClassName: "h-auto min-h-0 flex-1",
			onOpenChange,
			onReveal
		})
	});
}
export { filterDashboardCards as a, EMPTY_DASHBOARD_FILTERS as i, AgentTerminalPanel as n, filterDashboardWorkspaces as o, AgentDashboardToolbar as r, AgentTerminalDialog as t };
