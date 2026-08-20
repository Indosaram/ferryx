import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn } from "./button-DszXJEV6.js";
import { t as Check } from "./check-Lb2n4tDb.js";
import { t as ChevronsUpDown } from "./chevrons-up-down-avw2FWhd.js";
import { t as CircleX } from "./circle-x-Cl9fp3Vy.js";
import { $a as isClipboardTextByteLengthOverLimit, Km as getDefaultRepoHookSettings, Om as DEFAULT_APP_FONT_FAMILY, ah as DESKTOP_TERMINAL_SCROLLBACK_ROW_PRESETS, t_ as normalizeColor } from "./store-CgXrfmaH.js";
import { t as Label } from "./label-D-n9s_wS.js";
import { n as PopoverAnchor, r as PopoverContent, t as Popover } from "./popover-CgR1mzy7.js";
import { t as ScrollArea } from "./scroll-area-DifvZO0h.js";
import { t as Switch } from "./switch-NhZdOYtg.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
import { t as Input } from "./input-DV5rpysh.js";
const UI_ZOOM_STEP = .5;
const UI_ZOOM_MIN = -3;
const UI_ZOOM_MAX = 5;
function stepUIZoomLevel(current, direction) {
	if (direction === "reset") return 0;
	const next = direction === "in" ? current + UI_ZOOM_STEP : current - UI_ZOOM_STEP;
	return Math.max(-3, Math.min(5, next));
}
const DEFAULT_REPO_HOOK_SETTINGS = getDefaultRepoHookSettings();
const SCROLLBACK_PRESETS_ROWS = DESKTOP_TERMINAL_SCROLLBACK_ROW_PRESETS;
function zoomLevelToPercent(level) {
	return Math.round(100 * 1.2 ** level);
}
function mergeFontSuggestions(systemFonts, previousFonts) {
	return Array.from(new Set([
		DEFAULT_APP_FONT_FAMILY,
		...systemFonts,
		...previousFonts
	]));
}
function getFallbackTerminalFonts() {
	const nav = typeof navigator !== "undefined" ? navigator : null;
	const normalizedPlatform = (nav ? nav.userAgentData?.platform ?? nav.platform ?? "" : "").toLowerCase();
	if (normalizedPlatform.includes("mac")) return [
		"SF Mono",
		"Menlo",
		"Monaco",
		"JetBrains Mono",
		"Fira Code"
	];
	if (normalizedPlatform.includes("win")) return [
		"Cascadia Mono",
		"Consolas",
		"Lucida Console",
		"JetBrains Mono",
		"Fira Code"
	];
	return [
		"JetBrains Mono",
		"Fira Code",
		"DejaVu Sans Mono",
		"Liberation Mono",
		"Ubuntu Mono",
		"Noto Sans Mono"
	];
}
const SETTINGS_FORM_OPTION_QUERY_MAX_BYTES = 2 * 1024;
function isSettingsFormOptionQueryTooLarge(query, maxBytes = SETTINGS_FORM_OPTION_QUERY_MAX_BYTES) {
	return isClipboardTextByteLengthOverLimit(query, maxBytes);
}
function normalizeSettingsFormOptionQuery(query) {
	if (isSettingsFormOptionQueryTooLarge(query)) return null;
	return query.trim().toLowerCase();
}
function filterTerminalThemeOptions(themeOptions, query) {
	const normalizedQuery = normalizeSettingsFormOptionQuery(query);
	if (normalizedQuery === null) return [];
	if (!normalizedQuery) return [...themeOptions];
	return themeOptions.filter((theme) => `${theme.label} ${theme.sourceLabel ?? ""} `.toLowerCase().includes(normalizedQuery));
}
function filterFontSuggestions(suggestions, query) {
	const normalizedQuery = normalizeSettingsFormOptionQuery(query);
	if (normalizedQuery === null) return [];
	if (!normalizedQuery) return [...suggestions];
	const startsWith = [];
	const includes = [];
	for (const font of suggestions) {
		const normalizedFont = font.toLowerCase();
		if (normalizedFont.startsWith(normalizedQuery)) startsWith.push(font);
		else if (normalizedFont.includes(normalizedQuery)) includes.push(font);
	}
	return [...startsWith, ...includes];
}
function getRenderedFontSuggestions(suggestions, highlightedIndex, limit = 320) {
	const cappedLength = Math.min(suggestions.length, limit);
	if (cappedLength <= 0) return [];
	const sourceIndexes = Array.from({ length: cappedLength }, (_value, index) => index);
	if (highlightedIndex >= cappedLength && highlightedIndex < suggestions.length) sourceIndexes[cappedLength - 1] = highlightedIndex;
	return sourceIndexes.map((sourceIndex) => ({
		font: suggestions[sourceIndex] ?? "",
		sourceIndex
	}));
}
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function FontAutocomplete({ value, suggestions, onChange, placeholder = "SF Mono", onRequestSuggestions, onPreviewFontFamily }) {
	const [query, setQuery] = (0, import_react.useState)(value);
	const [prevValue, setPrevValue] = (0, import_react.useState)(value);
	const [open, setOpen] = (0, import_react.useState)(false);
	const [highlightedIndex, setHighlightedIndex] = (0, import_react.useState)(-1);
	const [isFilteringQuery, setIsFilteringQuery] = (0, import_react.useState)(false);
	const inputRef = (0, import_react.useRef)(null);
	const rootRef = (0, import_react.useRef)(null);
	const previewFontFamilyRef = (0, import_react.useRef)(onPreviewFontFamily);
	const listboxId = (0, import_react.useId)();
	(0, import_react.useEffect)(() => {
		previewFontFamilyRef.current = onPreviewFontFamily;
	}, [onPreviewFontFamily]);
	const setRootNode = (0, import_react.useCallback)((element) => {
		rootRef.current = element;
		if (!element) previewFontFamilyRef.current?.(null);
	}, []);
	if (value !== prevValue) {
		setPrevValue(value);
		setQuery(value);
		if (value !== query) setIsFilteringQuery(false);
	}
	const requestSuggestions = (0, import_react.useCallback)(() => {
		onRequestSuggestions?.();
	}, [onRequestSuggestions]);
	const handleOpenChange = (nextOpen) => {
		setOpen(nextOpen);
		if (nextOpen) requestSuggestions();
		if (!nextOpen) setIsFilteringQuery(false);
	};
	const normalizedQuery = query.trim().toLowerCase();
	const normalizedValue = value.trim().toLowerCase();
	const filteredSuggestions = (0, import_react.useMemo)(() => filterFontSuggestions(suggestions, query), [suggestions, query]);
	const visibleSuggestions = !isFilteringQuery && normalizedQuery === normalizedValue ? suggestions : filteredSuggestions;
	const renderedSuggestions = (0, import_react.useMemo)(() => getRenderedFontSuggestions(visibleSuggestions, highlightedIndex), [visibleSuggestions, highlightedIndex]);
	const [prevVisibleSuggestions, setPrevVisibleSuggestions] = (0, import_react.useState)(visibleSuggestions);
	const [prevOpen, setPrevOpen] = (0, import_react.useState)(open);
	const [prevHighlightedValue, setPrevHighlightedValue] = (0, import_react.useState)(value);
	if (visibleSuggestions !== prevVisibleSuggestions || open !== prevOpen || value !== prevHighlightedValue) {
		setPrevVisibleSuggestions(visibleSuggestions);
		setPrevOpen(open);
		setPrevHighlightedValue(value);
		if (!open || visibleSuggestions.length === 0) setHighlightedIndex(-1);
		else {
			const selectedIndex = visibleSuggestions.indexOf(value);
			setHighlightedIndex(Math.max(selectedIndex, 0));
		}
	}
	(0, import_react.useEffect)(() => {
		if (!onPreviewFontFamily) return;
		if (!open || highlightedIndex < 0) {
			onPreviewFontFamily(null);
			return;
		}
		onPreviewFontFamily(visibleSuggestions[highlightedIndex] ?? null);
	}, [
		visibleSuggestions,
		highlightedIndex,
		onPreviewFontFamily,
		open
	]);
	const commitValue = (nextValue) => {
		setQuery(nextValue);
		setIsFilteringQuery(false);
		onChange(nextValue);
		setOpen(false);
	};
	const focusInput = () => {
		inputRef.current?.focus();
	};
	const popoverAvailableHeightStyle = { maxHeight: "var(--radix-popover-content-available-height)" };
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		ref: setRootNode,
		className: "relative max-w-sm",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Popover, {
			open,
			onOpenChange: handleOpenChange,
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(PopoverAnchor, {
				asChild: true,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "relative",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
						ref: inputRef,
						value: query,
						onChange: (e) => {
							const next = e.target.value;
							requestSuggestions();
							setQuery(next);
							setIsFilteringQuery(true);
							onChange(next);
							setOpen(true);
						},
						onFocus: () => {
							requestSuggestions();
							setIsFilteringQuery(false);
							setOpen(true);
						},
						onKeyDown: (e) => {
							if (e.key === "Escape") {
								if (open) {
									e.preventDefault();
									setOpen(false);
									setIsFilteringQuery(false);
								}
								return;
							}
							if (e.key === "ArrowDown") {
								e.preventDefault();
								setOpen(true);
								if (visibleSuggestions.length > 0) setHighlightedIndex((current) => current < 0 ? 0 : Math.min(current + 1, visibleSuggestions.length - 1));
								return;
							}
							if (e.key === "ArrowUp") {
								e.preventDefault();
								setOpen(true);
								if (visibleSuggestions.length > 0) setHighlightedIndex((current) => current < 0 ? visibleSuggestions.length - 1 : Math.max(current - 1, 0));
								return;
							}
							if (e.key === "Enter" && open && highlightedIndex >= 0) {
								const highlightedFont = visibleSuggestions[highlightedIndex];
								if (highlightedFont) {
									e.preventDefault();
									commitValue(highlightedFont);
								}
							}
						},
						placeholder,
						className: "pr-18",
						role: "combobox",
						"aria-autocomplete": "list",
						"aria-expanded": open,
						"aria-controls": listboxId,
						"aria-activedescendant": open && highlightedIndex >= 0 ? `${listboxId}-option-${highlightedIndex}` : void 0
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "absolute inset-y-0 right-2 flex items-center gap-1",
						children: [query ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
							type: "button",
							onMouseDown: (e) => e.preventDefault(),
							onClick: () => {
								setQuery("");
								setIsFilteringQuery(false);
								onChange("");
								setOpen(true);
								focusInput();
							},
							className: "rounded-sm p-1 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground",
							"aria-label": translate("auto.components.settings.SettingsFormControls.a4ff6143f8", "Clear font selection"),
							title: translate("auto.components.settings.SettingsFormControls.74bcecd5ec", "Clear"),
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CircleX, { className: "size-3.5" })
						}) : null, /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
							type: "button",
							onMouseDown: (e) => e.preventDefault(),
							onClick: () => {
								const nextOpen = !open;
								setOpen(nextOpen);
								if (!nextOpen) setIsFilteringQuery(false);
								if (nextOpen) {
									requestSuggestions();
									focusInput();
								}
							},
							className: "rounded-sm p-1 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground",
							"aria-label": translate("auto.components.settings.SettingsFormControls.c766f8ac75", "Toggle font suggestions"),
							title: translate("auto.components.settings.SettingsFormControls.b55371ea18", "Fonts"),
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronsUpDown, { className: "size-3.5" })
						})]
					})]
				})
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PopoverContent, {
				align: "start",
				className: "w-[var(--radix-popover-trigger-width)]",
				onOpenAutoFocus: (e) => e.preventDefault(),
				onCloseAutoFocus: (e) => e.preventDefault(),
				onInteractOutside: (e) => {
					if (rootRef.current?.contains(e.target)) e.preventDefault();
				},
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ScrollArea, {
					className: renderedSuggestions.length > 8 ? "h-64" : void 0,
					style: popoverAvailableHeightStyle,
					viewportProps: { style: popoverAvailableHeightStyle },
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						id: listboxId,
						role: "listbox",
						className: "p-1",
						children: visibleSuggestions.length > 0 ? renderedSuggestions.map(({ font, sourceIndex }) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
							type: "button",
							id: `${listboxId}-option-${sourceIndex}`,
							role: "option",
							"aria-selected": sourceIndex === highlightedIndex,
							ref: (element) => {
								if (element && sourceIndex === highlightedIndex) element.scrollIntoView({ block: "nearest" });
							},
							onMouseDown: (e) => e.preventDefault(),
							onMouseEnter: () => setHighlightedIndex(sourceIndex),
							onClick: () => commitValue(font),
							className: `flex w-full items-center justify-between rounded-sm px-3 py-2 text-left text-sm transition-colors ${sourceIndex === highlightedIndex ? "bg-accent text-accent-foreground" : "hover:bg-muted/60"}`,
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "truncate",
								children: font
							}), font === value ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, { className: "ml-3 size-4 shrink-0" }) : null]
						}, font)) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "px-3 py-3 text-sm text-muted-foreground",
							children: translate("auto.components.settings.SettingsFormControls.42a4d15a30", "No matching fonts.")
						})
					})
				})
			})]
		})
	});
}
function ThemePicker({ label, description, selectedTheme, themeOptions, query, onQueryChange, onSelectTheme, importedHighlightSignal }) {
	const importedGroupRef = (0, import_react.useRef)(null);
	const [highlightImported, setHighlightImported] = (0, import_react.useState)(false);
	(0, import_react.useEffect)(() => {
		if (!importedHighlightSignal) return;
		importedGroupRef.current?.scrollIntoView({
			behavior: "smooth",
			block: "nearest"
		});
		setHighlightImported(true);
		const timer = setTimeout(() => setHighlightImported(false), 2e3);
		return () => clearTimeout(timer);
	}, [importedHighlightSignal]);
	const themeQuery = query.trim();
	const shouldShowThemeQueryLabel = themeQuery.length > 0 && !isSettingsFormOptionQueryTooLarge(themeQuery);
	const matchingThemes = filterTerminalThemeOptions(themeOptions, query);
	const selectedThemeLabel = themeOptions.find((option) => option.value === selectedTheme)?.label ?? selectedTheme;
	const groupedThemes = [{
		label: translate("auto.components.settings.SettingsFormControls.builtin_themes", "Built-in"),
		themes: matchingThemes.filter((theme) => theme.group === "built-in").slice(0, 80)
	}, {
		label: translate("auto.components.settings.SettingsFormControls.imported_themes", "Imported"),
		themes: matchingThemes.filter((theme) => theme.group === "imported").slice(0, 80)
	}].filter((group) => group.themes.length > 0);
	const visibleThemeCount = groupedThemes.reduce((sum, group) => sum + group.themes.length, 0);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-3",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-1",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, { children: label }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "text-xs text-muted-foreground",
					children: description
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
				value: query,
				onChange: (e) => onQueryChange(e.target.value),
				placeholder: translate("auto.components.settings.SettingsFormControls.search_terminal_themes", "Search terminal themes")
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "rounded-lg border border-border/50",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-center justify-between border-b border-border/50 px-3 py-2 text-xs text-muted-foreground",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", { children: [
						translate("auto.components.settings.SettingsFormControls.fbb428db98", "Selected:"),
						" ",
						selectedThemeLabel
					] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", { children: [
						translate("auto.components.settings.SettingsFormControls.4e11f87ca6", "Showing"),
						" ",
						visibleThemeCount,
						shouldShowThemeQueryLabel ? translate("auto.components.settings.SettingsFormControls.c822571b2e", " matching \"{{value0}}\"", { value0: themeQuery }) : translate("auto.components.settings.SettingsFormControls.cb330ef7f8", " of {{value0}}", { value0: themeOptions.length })
					] })]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ScrollArea, {
					className: "h-64",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "space-y-1 p-2",
						children: [groupedThemes.map((group) => {
							const isImported = group.label === translate("auto.components.settings.SettingsFormControls.imported_themes", "Imported");
							return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								ref: isImported ? importedGroupRef : void 0,
								className: cn("space-y-1 rounded-md transition-colors duration-500", isImported && highlightImported && "bg-accent/40 ring-1 ring-accent"),
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
									className: "px-3 pt-2 text-[11px] font-semibold uppercase tracking-[0.05em] text-muted-foreground",
									children: group.label
								}), group.themes.map((theme) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
									onClick: () => onSelectTheme(theme.value),
									className: cn("flex w-full items-center justify-between gap-3 rounded-md px-3 py-2 text-left text-sm transition-colors", selectedTheme === theme.value ? "bg-accent font-medium text-accent-foreground" : "hover:bg-accent"),
									children: [
										/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
											className: "min-w-0 flex-1",
											children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
												className: "block truncate",
												children: theme.label
											}), theme.sourceLabel ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
												className: "block truncate text-[11px] font-normal text-muted-foreground",
												children: [translate("auto.components.settings.SettingsFormControls.imported_from", "Imported from {{value0}}", { value0: theme.sourceLabel }), theme.mode && theme.mode !== "unknown" ? ` · ${theme.mode}` : ""]
											}) : null]
										}),
										theme.group === "imported" && theme.previewTheme && selectedTheme !== theme.value ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
											className: "flex shrink-0 overflow-hidden rounded-sm border border-border/60",
											children: [
												theme.previewTheme.black,
												theme.previewTheme.red,
												theme.previewTheme.green,
												theme.previewTheme.yellow,
												theme.previewTheme.blue,
												theme.previewTheme.magenta,
												theme.previewTheme.cyan,
												theme.previewTheme.white
											].map((color, index) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
												className: "h-3 w-2",
												style: { backgroundColor: color ?? "transparent" }
											}, index))
										}) : null,
										selectedTheme === theme.value ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
											className: "ml-3 shrink-0 text-[11px] uppercase tracking-[0.16em]",
											children: translate("auto.components.settings.SettingsFormControls.9119fb2268", "Current")
										}) : null
									]
								}, theme.value))]
							}, group.label);
						}), visibleThemeCount === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "px-3 py-6 text-sm text-muted-foreground",
							children: translate("auto.components.settings.SettingsFormControls.ceefb9d7f1", "No themes found.")
						}) : null]
					})
				})]
			})
		]
	});
}
function SettingsSwitch({ checked, onChange, ariaLabel, ariaLabelledBy, disabled }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Switch, {
		checked,
		"aria-label": ariaLabel,
		"aria-labelledby": ariaLabelledBy,
		disabled,
		onCheckedChange: onChange
	});
}
function SettingsRow({ label, description, control, className, labelId, alignTop }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: cn("flex gap-4", description ? "py-3" : "py-2", alignTop ? "items-start" : "items-center justify-between", className),
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: cn("min-w-0 flex-1", description ? "space-y-1" : "space-y-0.5"),
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
				id: labelId,
				className: "select-text",
				children: label
			}), description ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				className: "select-text text-xs text-muted-foreground",
				children: description
			}) : null]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "shrink-0",
			children: control
		})]
	});
}
function SettingsSwitchRow({ label, description, checked, onChange, className, ariaLabel, disabled }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsRow, {
		label,
		description,
		className,
		control: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsSwitch, {
			checked,
			onChange,
			disabled,
			ariaLabel: ariaLabel ?? (typeof label === "string" ? label : void 0)
		})
	});
}
function SettingsSegmentedControl({ value, onChange, options, ariaLabel, size = "md", equalWidth = false }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		role: "radiogroup",
		"aria-label": ariaLabel,
		className: cn("inline-flex items-center rounded-md border border-border bg-background/50 p-0.5", equalWidth && "w-full"),
		children: options.map((opt) => {
			const active = opt.value === value;
			const button = /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				type: "button",
				role: "radio",
				"aria-checked": active,
				"aria-label": opt.ariaLabel,
				"aria-disabled": opt.disabled,
				onClick: () => {
					if (!opt.disabled) onChange(opt.value);
				},
				className: cn("rounded-sm text-center outline-none transition-colors focus-visible:ring-[3px] focus-visible:ring-ring/50", size === "sm" ? "px-2.5 py-0.5 text-xs" : "px-3 py-1 text-sm", equalWidth && "flex-1", active ? "bg-accent font-medium text-accent-foreground" : opt.disabled ? "cursor-not-allowed text-muted-foreground/50" : "text-muted-foreground hover:text-foreground"),
				children: opt.label
			}, String(opt.value));
			if (opt.tooltip == null) return button;
			return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
				asChild: true,
				children: button
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, { children: opt.tooltip })] }, String(opt.value));
		})
	});
}
function SettingsBadge({ tone = "neutral", children, className }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		className: cn("inline-flex items-center gap-1 rounded-full border px-1.5 py-0.5 text-[10px] font-medium", tone === "accent" ? "border-foreground/20 bg-foreground/10 text-foreground" : tone === "muted" ? "border-border/40 bg-muted/30 text-muted-foreground" : "border-border/50 bg-background/50 text-foreground/80", className),
		children
	});
}
function SettingsSubsectionHeader({ title, description, action, className }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: cn("flex items-start justify-between gap-3", className),
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "space-y-1",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h3", {
				className: "text-sm font-semibold",
				children: title
			}), description ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				className: "text-xs text-muted-foreground",
				children: description
			}) : null]
		}), action ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "shrink-0",
			children: action
		}) : null]
	});
}
function ColorField({ label, description, value, fallback, onChange }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsRow, {
		label,
		description,
		control: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex items-center gap-2",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("input", {
				type: "color",
				value: normalizeColor(value, fallback),
				onChange: (e) => onChange(e.target.value),
				className: "h-8 w-10 rounded-md border border-input bg-transparent p-1"
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
				value,
				onChange: (e) => onChange(e.target.value),
				placeholder: fallback,
				className: "w-32 text-xs"
			})]
		})
	});
}
function NumberField({ label, description, value, defaultValue, min, max, step = 1, onChange, suffix }) {
	const [draft, setDraft] = (0, import_react.useState)(Number.isFinite(value) ? String(value) : "");
	const [prevValue, setPrevValue] = (0, import_react.useState)(value);
	if (value !== prevValue) {
		setPrevValue(value);
		setDraft(Number.isFinite(value) ? String(value) : "");
	}
	const commit = () => {
		const trimmed = draft.trim();
		if (trimmed === "") {
			setDraft(Number.isFinite(value) ? String(value) : "");
			return;
		}
		const next = Number(trimmed);
		if (Number.isFinite(next)) {
			const clamped = Math.min(max, Math.max(min, next));
			onChange(clamped);
			setDraft(String(clamped));
		} else setDraft(Number.isFinite(value) ? String(value) : "");
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsRow, {
		label,
		description: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [description, defaultValue !== void 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
			className: "ml-1 text-muted-foreground/70",
			children: [
				translate("auto.components.settings.SettingsFormControls.b661b034ec", "· Default:"),
				" ",
				defaultValue
			]
		}) : null] }),
		control: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex items-center gap-2",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
				type: "number",
				min,
				max,
				step,
				value: draft,
				onChange: (e) => setDraft(e.target.value),
				onBlur: commit,
				onKeyDown: (e) => {
					if (e.key === "Enter") commit();
				},
				className: "number-input-clean w-24 tabular-nums"
			}), suffix ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "shrink-0 text-xs text-muted-foreground",
				children: suffix
			}) : null]
		})
	});
}
export { UI_ZOOM_MIN as _, SettingsSegmentedControl as a, SettingsSwitchRow as c, DEFAULT_REPO_HOOK_SETTINGS as d, SCROLLBACK_PRESETS_ROWS as f, UI_ZOOM_MAX as g, zoomLevelToPercent as h, SettingsRow as i, ThemePicker as l, mergeFontSuggestions as m, NumberField as n, SettingsSubsectionHeader as o, getFallbackTerminalFonts as p, SettingsBadge as r, SettingsSwitch as s, ColorField as t, FontAutocomplete as u, UI_ZOOM_STEP as v, stepUIZoomLevel as y };
