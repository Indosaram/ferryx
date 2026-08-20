import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn, t as Button } from "./button-DszXJEV6.js";
import { t as ArrowRight } from "./arrow-right-ct5UxmKv.js";
import { t as Check } from "./check-Lb2n4tDb.js";
import { t as ChevronsUpDown } from "./chevrons-up-down-avw2FWhd.js";
import { t as Star } from "./star-qKN37Pn0.js";
import { t as Terminal } from "./terminal-Cen7Un9b.js";
import { $a as isClipboardTextByteLengthOverLimit } from "./store-CgXrfmaH.js";
import { f as ContextMenuTrigger, n as ContextMenuContent, r as ContextMenuItem, t as ContextMenu } from "./context-menu-D4RKI7hR.js";
import { i as PopoverTrigger, r as PopoverContent, t as Popover } from "./popover-CgR1mzy7.js";
import { a as CommandInput, o as CommandItem, r as CommandEmpty, s as CommandList, t as Command } from "./command-D8Tw17HJ.js";
import { t as AgentIcon } from "./agent-catalog-CBF2CV5Q.js";
var NO_MATCH = Number.POSITIVE_INFINITY;
const AGENT_PICKER_QUERY_MAX_BYTES = 2 * 1024;
function isAgentPickerQueryTooLarge(query, maxBytes = AGENT_PICKER_QUERY_MAX_BYTES) {
	return isClipboardTextByteLengthOverLimit(query, maxBytes);
}
function getAgentPickerCommandValue({ blankValue, blankMatchesQuery, currentValue, filteredAgents, rawQuery }) {
	const query = getAgentPickerSearchQuery(rawQuery);
	if (query === null) return "";
	if (!query) return currentValue ?? blankValue;
	if (blankMatchesQuery) return blankValue;
	return filteredAgents[0]?.id ?? "";
}
function searchAgentPickerEntries(agents, rawQuery) {
	const query = getAgentPickerSearchQuery(rawQuery);
	if (query === null) return [];
	if (!query) return [...agents];
	const matches = [];
	agents.forEach((agent, index) => {
		const score = scoreAgent(agent, query);
		if (score !== NO_MATCH) matches.push({
			agent,
			score,
			index
		});
	});
	matches.sort((a, b) => a.score - b.score || a.index - b.index);
	return matches.map((m) => m.agent);
}
function agentPickerBlankTerminalMatches(rawQuery) {
	const query = getAgentPickerSearchQuery(rawQuery);
	if (query === null) return false;
	if (!query) return true;
	return scoreCandidate(query, "Blank Terminal", 0) !== NO_MATCH || scoreCandidate(query, "terminal", 0) !== NO_MATCH || scoreCandidate(query, "shell", 0) !== NO_MATCH;
}
function scoreAgent(agent, query) {
	return Math.min(scoreCandidate(query, agent.label, 0), scoreCandidate(query, agent.id, 600), scoreCandidate(query, agent.cmd, 650));
}
function scoreCandidate(query, rawCandidate, baseScore) {
	const candidate = normalizeSearchText(rawCandidate);
	if (!candidate) return NO_MATCH;
	if (candidate === query) return baseScore;
	if (candidate.startsWith(query)) return baseScore + 10;
	const substringIndex = candidate.indexOf(query);
	if (substringIndex !== -1) return baseScore + 100 + substringIndex;
	const acronymScore = scoreAcronymQuery(query, rawCandidate);
	if (acronymScore !== NO_MATCH) return baseScore + 220 + acronymScore;
	const fuzzyScore = scoreFuzzyQuery(query, candidate);
	if (fuzzyScore !== NO_MATCH) return baseScore + 400 + fuzzyScore;
	return NO_MATCH;
}
function scoreAcronymQuery(query, rawCandidate) {
	const acronym = buildAcronym(rawCandidate);
	if (!acronym) return NO_MATCH;
	if (acronym === query) return 0;
	if (acronym.startsWith(query)) return 10;
	return scoreFuzzyQuery(query, acronym);
}
function buildAcronym(value) {
	const chars = [];
	let previous = "";
	for (const char of value) {
		if (!/[a-z0-9]/i.test(char)) {
			previous = char;
			continue;
		}
		if (chars.length === 0 || !/[a-z0-9]/i.test(previous) || /[a-z]/.test(previous) && /[A-Z]/.test(char)) chars.push(char.toLowerCase());
		previous = char;
	}
	return chars.join("");
}
function scoreFuzzyQuery(query, candidate) {
	let queryIndex = 0;
	let score = 0;
	let lastMatchIndex = -1;
	for (let candidateIndex = 0; candidateIndex < candidate.length && queryIndex < query.length; candidateIndex++) {
		if (candidate[candidateIndex] !== query[queryIndex]) continue;
		const gap = lastMatchIndex === -1 ? candidateIndex : candidateIndex - lastMatchIndex - 1;
		score += gap;
		if (isBoundary(candidate, candidateIndex)) score -= 4;
		lastMatchIndex = candidateIndex;
		queryIndex++;
	}
	if (queryIndex < query.length) return NO_MATCH;
	return score;
}
function isBoundary(value, index) {
	if (index === 0) return true;
	return value[index - 1] === " " || value[index - 1] === "-" || value[index - 1] === "_";
}
function normalizeSearchText(value) {
	let normalized = "";
	let pendingWhitespace = false;
	for (let index = 0; index < value.length; index += 1) {
		if (isAgentPickerWhitespace(value.charCodeAt(index))) {
			pendingWhitespace = normalized.length > 0;
			continue;
		}
		if (pendingWhitespace) {
			normalized += " ";
			pendingWhitespace = false;
		}
		normalized += value.charAt(index).toLowerCase();
	}
	return normalized;
}
function getAgentPickerSearchQuery(rawQuery) {
	if (isAgentPickerQueryTooLarge(rawQuery)) return null;
	return normalizeSearchText(rawQuery);
}
function isAgentPickerWhitespace(code) {
	return code === 32 || code >= 9 && code <= 13 || code === 160 || code === 5760 || code >= 8192 && code <= 8202 || code === 8232 || code === 8233 || code === 8239 || code === 8287 || code === 12288 || code === 65279;
}
function createAgentComboboxCommandState(commandValue) {
	return {
		commandValue,
		activeCommandValue: commandValue
	};
}
function resolveAgentComboboxCommandState(state, open, activeCommandValue) {
	if (!open || state.activeCommandValue === activeCommandValue) return state;
	return {
		commandValue: activeCommandValue,
		activeCommandValue
	};
}
function updateAgentComboboxCommandValue(state, commandValue) {
	if (state.commandValue === commandValue) return state;
	return {
		...state,
		commandValue
	};
}
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var BLANK_VALUE = "__none__";
var TRIGGER_MIN_WIDTH_CLASS = "!min-w-[260px]";
function AgentIconLabel({ icon, label }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
		className: "inline-flex min-w-0 flex-1 items-center gap-1.5",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "inline-flex size-3.5 shrink-0 items-center justify-center [&_img]:size-3.5 [&_svg]:size-3.5!",
			children: icon
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "truncate leading-none",
			children: label
		})]
	});
}
function AgentDefaultContextMenu({ children, isDefault, onSetDefault }) {
	if (!onSetDefault) return children;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ContextMenu, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ContextMenuTrigger, {
		asChild: true,
		children
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ContextMenuContent, {
		className: "z-[70]",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ContextMenuItem, {
			onSelect: onSetDefault,
			disabled: isDefault,
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Star, { className: "size-3.5" }), isDefault ? translate("auto.components.agent.AgentCombobox.1b0d6965fa", "Current default") : translate("auto.components.agent.AgentCombobox.9c6b59fe58", "Set as default")]
		})
	})] });
}
function renderItem({ key, itemValue, isChecked, isDefault, onSelect, onSetDefault, icon, label }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentDefaultContextMenu, {
		isDefault,
		onSetDefault,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CommandItem, {
			value: itemValue,
			onSelect,
			className: "items-center gap-2 px-3 py-1.5",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, { className: cn("size-4 shrink-0 text-foreground", isChecked ? "opacity-100" : "opacity-0") }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentIconLabel, {
				icon,
				label
			})]
		}, key)
	}, key);
}
function AgentCombobox({ agents, value, onValueChange, onValueSelected, onOpenManageAgents, defaultAgent, onSetDefault, triggerClassName, onTriggerEnter, allowNarrowTrigger = false, allowBlankTerminal = true, emptyLabel }) {
	const [open, setOpen] = (0, import_react.useState)(false);
	const [query, setQuery] = (0, import_react.useState)("");
	const [commandState, setCommandState] = (0, import_react.useState)(() => createAgentComboboxCommandState(""));
	const triggerRef = import_react.useRef(null);
	const inputRef = import_react.useRef(null);
	const focusFrameRef = import_react.useRef(null);
	const selectedAgent = (0, import_react.useMemo)(() => value ? agents.find((agent) => agent.id === value) ?? null : null, [agents, value]);
	const selectedDefaultPreference = value ?? (allowBlankTerminal ? "blank" : null);
	const filteredAgents = (0, import_react.useMemo)(() => searchAgentPickerEntries(agents, query), [agents, query]);
	const blankMatchesQuery = (0, import_react.useMemo)(() => allowBlankTerminal && agentPickerBlankTerminalMatches(query), [allowBlankTerminal, query]);
	const resolvedCommandState = resolveAgentComboboxCommandState(commandState, open, getAgentPickerCommandValue({
		blankValue: BLANK_VALUE,
		blankMatchesQuery,
		currentValue: value,
		filteredAgents,
		rawQuery: query
	}));
	if (resolvedCommandState !== commandState) setCommandState(resolvedCommandState);
	const commandValue = resolvedCommandState.commandValue;
	const cancelFocusFrame = (0, import_react.useCallback)(() => {
		if (focusFrameRef.current !== null) {
			cancelAnimationFrame(focusFrameRef.current);
			focusFrameRef.current = null;
		}
	}, []);
	const setInputNode = (0, import_react.useCallback)((node) => {
		if (node === null) cancelFocusFrame();
		inputRef.current = node;
	}, [cancelFocusFrame]);
	const setCommandValue = (0, import_react.useCallback)((nextCommandValue) => {
		setCommandState((current) => updateAgentComboboxCommandValue(current, nextCommandValue));
	}, []);
	const focusSearchInput = (0, import_react.useCallback)(() => {
		cancelFocusFrame();
		focusFrameRef.current = requestAnimationFrame(() => {
			focusFrameRef.current = null;
			const searchInput = inputRef.current;
			if (!searchInput) return;
			searchInput.focus();
			const end = searchInput.value.length;
			searchInput.setSelectionRange(end, end);
		});
	}, [cancelFocusFrame]);
	const handleOpenChange = (0, import_react.useCallback)((nextOpen) => {
		setOpen(nextOpen);
		if (nextOpen) {
			setCommandState(createAgentComboboxCommandState(value ?? BLANK_VALUE));
			return;
		}
		cancelFocusFrame();
		setQuery("");
	}, [cancelFocusFrame, value]);
	const handleSelect = (0, import_react.useCallback)((nextValue) => {
		onValueChange(nextValue);
		setOpen(false);
		setQuery("");
		onValueSelected?.(nextValue);
	}, [onValueChange, onValueSelected]);
	const handleTriggerKeyDown = (0, import_react.useCallback)((event) => {
		if (open) return;
		if (event.key === "Enter" && onTriggerEnter && !event.shiftKey && !event.metaKey && !event.ctrlKey && !event.altKey) {
			event.preventDefault();
			onTriggerEnter();
			return;
		}
		if (event.key === "ArrowDown" || event.key === "ArrowUp") {
			event.preventDefault();
			setCommandState(createAgentComboboxCommandState(value ?? BLANK_VALUE));
			setOpen(true);
			return;
		}
		if (event.metaKey || event.ctrlKey || event.altKey) return;
		if (event.key.length === 1 && /\S/.test(event.key)) {
			event.preventDefault();
			setCommandState(createAgentComboboxCommandState(value ?? BLANK_VALUE));
			setQuery(event.key);
			setOpen(true);
		}
	}, [
		open,
		onTriggerEnter,
		value
	]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "min-w-0 w-full",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Popover, {
			open,
			onOpenChange: handleOpenChange,
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentDefaultContextMenu, {
				isDefault: selectedDefaultPreference !== null && defaultAgent === selectedDefaultPreference,
				onSetDefault: onSetDefault && selectedDefaultPreference !== null ? () => onSetDefault(selectedDefaultPreference) : void 0,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PopoverTrigger, {
					asChild: true,
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
						ref: triggerRef,
						type: "button",
						variant: "outline",
						role: "combobox",
						"aria-expanded": open,
						onKeyDown: handleTriggerKeyDown,
						className: cn("h-8 justify-between px-3 py-0 text-xs font-normal", triggerClassName, !allowNarrowTrigger && TRIGGER_MIN_WIDTH_CLASS),
						"data-agent-combobox-root": "true",
						children: [selectedAgent ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentIconLabel, {
							icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentIcon, {
								agent: selectedAgent.id,
								size: 14
							}),
							label: selectedAgent.label
						}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentIconLabel, {
							icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Terminal, { className: "size-3.5" }),
							label: emptyLabel ?? translate("auto.components.agent.AgentCombobox.986f946354", "Blank Terminal")
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronsUpDown, { className: "size-3.5 shrink-0 opacity-50" })]
					})
				})
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PopoverContent, {
				align: "start",
				className: cn("w-[var(--radix-popover-trigger-width)] p-0", !allowNarrowTrigger && "min-w-[18rem]"),
				"data-agent-combobox-root": "true",
				onOpenAutoFocus: (event) => {
					event.preventDefault();
					focusSearchInput();
				},
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Command, {
					shouldFilter: false,
					value: commandValue,
					onValueChange: setCommandValue,
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CommandInput, {
							ref: setInputNode,
							placeholder: translate("auto.components.agent.AgentCombobox.48c6a5a9b4", "Search agents..."),
							value: query,
							onValueChange: setQuery
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CommandList, { children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CommandEmpty, { children: translate("auto.components.agent.AgentCombobox.579c768bde", "No agents match your search.") }),
							blankMatchesQuery ? renderItem({
								key: BLANK_VALUE,
								itemValue: BLANK_VALUE,
								isChecked: value === null,
								isDefault: defaultAgent === "blank",
								onSelect: () => handleSelect(null),
								onSetDefault: onSetDefault ? () => onSetDefault("blank") : void 0,
								icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Terminal, { className: "size-3.5" }),
								label: translate("auto.components.agent.AgentCombobox.986f946354", "Blank Terminal")
							}) : null,
							filteredAgents.map((agent) => renderItem({
								key: agent.id,
								itemValue: agent.id,
								isChecked: value === agent.id,
								isDefault: defaultAgent === agent.id,
								onSelect: () => handleSelect(agent.id),
								onSetDefault: onSetDefault ? () => onSetDefault(agent.id) : void 0,
								icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentIcon, { agent: agent.id }),
								label: agent.label
							}))
						] }),
						onOpenManageAgents ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "border-t border-border",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
								type: "button",
								variant: "ghost",
								onClick: onOpenManageAgents,
								onMouseDown: (event) => event.preventDefault(),
								onMouseEnter: () => setCommandValue(""),
								className: "h-9 w-full justify-start rounded-none px-3 text-xs font-normal text-muted-foreground",
								children: [translate("auto.components.agent.AgentCombobox.19522e25ee", "Manage agents"), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowRight, { className: "ml-auto size-3" })]
							})
						}) : null
					]
				})
			})]
		})
	});
}
export { AgentCombobox as t };
