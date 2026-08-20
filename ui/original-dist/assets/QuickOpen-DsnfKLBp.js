import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import "./button-DszXJEV6.js";
import { t as Check } from "./check-Lb2n4tDb.js";
import { t as Copy } from "./copy-jk2iqVkp.js";
import { t as getFileTypeIcon } from "./file-type-icons-CeipsYgO.js";
import { Oa as focusTerminalTabSurface, Rt as detectLanguage, kc as joinPath, t as useAppStore } from "./store-CgXrfmaH.js";
import { t as TriangleAlert } from "./triangle-alert-HrLt1y9s.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import "./tooltip-DPmd1AoJ.js";
import "./useMountedRef-1omUd-IV.js";
import { c as useActiveWorktree } from "./selectors-XOBeaOSb.js";
import "./connection-context-BUPsamzR.js";
import { a as CommandInput, n as CommandDialog, o as CommandItem, r as CommandEmpty, s as CommandList } from "./command-D8Tw17HJ.js";
import "./file-explorer-operation-owner-C4AAHFB5.js";
import { a as FilePathCursorTooltip, i as rankQuickOpenFiles, o as splitTrailingSegment, r as prepareQuickOpenFiles } from "./quick-open-search-BdbebxW8.js";
import "./file-name-sort-EN_9pleA.js";
import { t as useRuntimeFileListForWorktree } from "./quick-open-file-list-Bel-woO3.js";
import { r as queueBrowserFocusRequest, t as ORCA_BROWSER_FOCUS_REQUEST_EVENT } from "./browser-focus-BBW6rAsQ.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
function resolveModalReturnFocusAction(captured) {
	if (!captured) return { kind: "none" };
	if (captured.tabType === "browser" && captured.browserPageId) return {
		kind: "browser",
		pageId: captured.browserPageId,
		target: captured.browserTarget
	};
	if (captured.tabType === "terminal" && captured.terminalTabId) return {
		kind: "terminal",
		tabId: captured.terminalTabId,
		leafId: captured.terminalLeafId
	};
	if (captured.tabType === "editor" && captured.worktreeId) return { kind: "editor" };
	if (captured.tabType === "simulator" && captured.worktreeId) return { kind: "simulator" };
	if (captured.worktreeId) return { kind: "surface" };
	return { kind: "none" };
}
function isRestorableFocusedElement(element) {
	return element !== null && element !== document.body && element !== document.documentElement;
}
function useModalReturnFocus(visible) {
	const capturedRef = (0, import_react.useRef)(null);
	const capturedElementRef = (0, import_react.useRef)(null);
	const skipRef = (0, import_react.useRef)(false);
	const wasVisibleRef = (0, import_react.useRef)(false);
	const outerFrameRef = (0, import_react.useRef)(null);
	const innerFrameRef = (0, import_react.useRef)(null);
	const cancelFrames = (0, import_react.useCallback)(() => {
		if (outerFrameRef.current !== null) {
			cancelAnimationFrame(outerFrameRef.current);
			outerFrameRef.current = null;
		}
		if (innerFrameRef.current !== null) {
			cancelAnimationFrame(innerFrameRef.current);
			innerFrameRef.current = null;
		}
	}, []);
	(0, import_react.useEffect)(() => cancelFrames, [cancelFrames]);
	const focusCapturedElement = (0, import_react.useCallback)(() => {
		const target = capturedElementRef.current;
		if (!isRestorableFocusedElement(target) || !target.isConnected) return false;
		target.focus();
		return document.activeElement === target || target.contains(document.activeElement);
	}, []);
	const focusFirstMatchingSurface = (0, import_react.useCallback)((selectors) => {
		cancelFrames();
		outerFrameRef.current = requestAnimationFrame(() => {
			outerFrameRef.current = null;
			innerFrameRef.current = requestAnimationFrame(() => {
				innerFrameRef.current = null;
				for (const selector of selectors) {
					const target = document.querySelector(selector);
					if (!target) continue;
					target.focus();
					if (document.activeElement === target || target.contains(document.activeElement)) return;
				}
			});
		});
	}, [cancelFrames]);
	const focusEditorSurface = (0, import_react.useCallback)(() => {
		if (focusCapturedElement()) return;
		focusFirstMatchingSurface([
			".monaco-editor textarea",
			".rich-markdown-editor[contenteditable=\"true\"]",
			".markdown-preview"
		]);
	}, [focusCapturedElement, focusFirstMatchingSurface]);
	const focusSimulatorSurface = (0, import_react.useCallback)(() => {
		if (focusCapturedElement()) return;
		focusFirstMatchingSurface(["[data-orca-emulator-frame=\"true\"] [tabindex]"]);
	}, [focusCapturedElement, focusFirstMatchingSurface]);
	const focusFallbackSurface = (0, import_react.useCallback)(() => {
		focusFirstMatchingSurface([".xterm-helper-textarea", ".monaco-editor textarea"]);
	}, [focusFirstMatchingSurface]);
	const requestBrowserFocus = (0, import_react.useCallback)((detail) => {
		queueBrowserFocusRequest(detail);
		window.dispatchEvent(new CustomEvent(ORCA_BROWSER_FOCUS_REQUEST_EVENT, { detail }));
	}, []);
	const captureReturnFocus = (0, import_react.useCallback)(() => {
		const state = useAppStore.getState();
		const worktreeId = state.activeWorktreeId;
		const tabType = state.activeTabType;
		const activeElement = document.activeElement instanceof HTMLElement ? document.activeElement : null;
		const browserPageId = worktreeId && tabType === "browser" ? (state.browserTabsByWorktree[worktreeId] ?? []).find((workspace) => workspace.id === state.activeBrowserTabId)?.activePageId ?? null : null;
		const terminalTabId = worktreeId && tabType === "terminal" ? state.activeTabIdByWorktree[worktreeId] ?? state.activeTabId : null;
		const terminalLeafId = terminalTabId ? state.terminalLayoutsByTabId[terminalTabId]?.activeLeafId ?? null : null;
		const browserTarget = tabType === "browser" && activeElement?.closest("[data-orca-browser-address-bar=\"true\"]") ? "address-bar" : "webview";
		capturedElementRef.current = isRestorableFocusedElement(activeElement) ? activeElement : null;
		capturedRef.current = {
			tabType,
			worktreeId,
			browserPageId,
			browserTarget,
			terminalTabId,
			terminalLeafId
		};
		skipRef.current = false;
	}, []);
	(0, import_react.useEffect)(() => {
		if (visible && !wasVisibleRef.current) {
			cancelFrames();
			if (!capturedRef.current) captureReturnFocus();
			skipRef.current = false;
		}
		if (!visible && wasVisibleRef.current) {
			const action = resolveModalReturnFocusAction(skipRef.current ? null : capturedRef.current);
			capturedRef.current = null;
			if (action.kind === "browser") {
				cancelFrames();
				requestBrowserFocus({
					pageId: action.pageId,
					target: action.target
				});
			} else if (action.kind === "terminal") {
				cancelFrames();
				focusTerminalTabSurface(action.tabId, action.leafId);
			} else if (action.kind === "editor") focusEditorSurface();
			else if (action.kind === "simulator") focusSimulatorSurface();
			else if (action.kind === "surface") focusFallbackSurface();
			capturedElementRef.current = null;
		}
		wasVisibleRef.current = visible;
	}, [
		visible,
		cancelFrames,
		captureReturnFocus,
		focusEditorSurface,
		focusFallbackSurface,
		focusSimulatorSurface,
		requestBrowserFocus
	]);
	return {
		captureReturnFocus,
		skipReturnFocus: (0, import_react.useCallback)(() => {
			skipRef.current = true;
		}, [])
	};
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var REMOTE_LOCATION_PHRASE = "on the remote";
function parseQuickOpenInstallRgGuidance(message) {
	const match = message.match(/^Quick Open scan too large \((.+?)\)\. Install ripgrep (on the remote|on the host running the Quick Open scan) to enable fast, gitignore-aware listing: (.+)$/);
	if (!match) return null;
	const reason = match[1];
	const location = match[2] === REMOTE_LOCATION_PHRASE ? "remote" : "local";
	const tail = match[3].trim();
	const looksLikeCommand = /^(sudo\s+)?(brew|apt|dnf|pacman|apk)\s/.test(tail);
	return {
		reason,
		location,
		command: looksLikeCommand ? tail : null,
		guidance: looksLikeCommand ? null : tail
	};
}
function QuickOpenInstallRgGuidance({ reason, location, command, guidance }) {
	const [copied, setCopied] = (0, import_react.useState)(false);
	const copiedResetTimerRef = (0, import_react.useRef)(null);
	const isMountedRef = (0, import_react.useRef)(false);
	const clearCopiedResetTimer = (0, import_react.useCallback)(() => {
		if (copiedResetTimerRef.current !== null) {
			window.clearTimeout(copiedResetTimerRef.current);
			copiedResetTimerRef.current = null;
		}
	}, []);
	const setCopyButtonRef = (0, import_react.useCallback)((node) => {
		isMountedRef.current = node !== null;
		if (node === null) clearCopiedResetTimer();
	}, [clearCopiedResetTimer]);
	const handleCopy = (0, import_react.useCallback)(() => {
		if (!command) return;
		window.api.ui.writeClipboardText(command).then(() => {
			if (!isMountedRef.current) return;
			clearCopiedResetTimer();
			setCopied(true);
			copiedResetTimerRef.current = window.setTimeout(() => {
				copiedResetTimerRef.current = null;
				setCopied(false);
			}, 1500);
		}).catch(() => {});
	}, [clearCopiedResetTimer, command]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "px-4 py-5 text-sm text-muted-foreground space-y-3",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				role: "alert",
				className: "flex items-start gap-2.5 rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2.5 text-amber-700 dark:text-amber-300",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, {
					size: 16,
					className: "mt-0.5 shrink-0",
					"aria-hidden": "true"
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
					className: "text-[13px] leading-5",
					children: [
						translate("auto.components.QuickOpen.4725b0e931", "Quick Open scan too large ("),
						reason,
						")."
					]
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", { children: [
				translate("auto.components.QuickOpen.2ca749c15d", "Install"),
				" ",
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("code", {
					className: "rounded bg-muted px-1 py-0.5 font-mono text-foreground",
					children: translate("auto.components.QuickOpen.5d80dc39bb", "ripgrep")
				}),
				" ",
				location === "remote" ? translate("auto.components.QuickOpen.1cf8561ab4", "on the remote to enable fast, gitignore-aware listing:") : translate("auto.components.QuickOpen.344f8a48dd", "on the host running the Quick Open scan to enable fast, gitignore-aware listing:")
			] }),
			command ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-center gap-2 rounded border border-border bg-muted/50 px-3 py-2 font-mono text-xs text-foreground",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "flex-1 truncate",
					children: command
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
					ref: setCopyButtonRef,
					type: "button",
					onClick: handleCopy,
					className: "flex items-center gap-1 rounded px-2 py-1 text-xs text-muted-foreground hover:bg-muted hover:text-foreground transition-colors",
					"aria-label": translate("auto.components.QuickOpen.73b44e7bde", "Copy install command"),
					children: [copied ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, { size: 12 }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Copy, { size: 12 }), copied ? translate("auto.components.QuickOpen.cf144856dc", "Copied") : translate("auto.components.QuickOpen.995be8ea22", "Copy")]
				})]
			}) : guidance ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				className: "text-[13px] leading-5 text-foreground",
				children: guidance
			}) : null
		]
	});
}
var QUICK_OPEN_CLOSE_LINGER_MS = 300;
function FooterKey({ children }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		className: "rounded-full border border-border/60 bg-muted/35 px-2 py-0.5 text-[10px] font-medium text-foreground/85",
		children
	});
}
function QuickOpen() {
	const visible = useAppStore((s) => s.activeModal === "quick-open");
	const [lingering, setLingering] = (0, import_react.useState)(visible);
	(0, import_react.useEffect)(() => {
		if (visible) {
			setLingering(true);
			return;
		}
		const timer = window.setTimeout(() => setLingering(false), QUICK_OPEN_CLOSE_LINGER_MS);
		return () => window.clearTimeout(timer);
	}, [visible]);
	if (!visible && !lingering) return null;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(QuickOpenContent, { visible });
}
function QuickOpenContent({ visible }) {
	const closeModal = useAppStore((s) => s.closeModal);
	const activeWorktreeId = useAppStore((s) => s.activeWorktreeId);
	const openFile = useAppStore((s) => s.openFile);
	const activeWorktree = useActiveWorktree();
	const [query, setQuery] = (0, import_react.useState)("");
	const deferredQuery = (0, import_react.useDeferredValue)(query);
	const { files, loading, loadError } = useRuntimeFileListForWorktree({
		enabled: visible,
		worktreeId: activeWorktreeId
	});
	const worktreePath = activeWorktree?.path ?? null;
	const { captureReturnFocus, skipReturnFocus } = useModalReturnFocus(visible);
	const [previousVisible, setPreviousVisible] = (0, import_react.useState)(visible);
	if (visible !== previousVisible) {
		setPreviousVisible(visible);
		if (visible && query !== "") setQuery("");
	}
	const indexedFiles = (0, import_react.useMemo)(() => prepareQuickOpenFiles(files), [files]);
	const filtered = (0, import_react.useMemo)(() => rankQuickOpenFiles(deferredQuery, indexedFiles), [deferredQuery, indexedFiles]);
	const handleSelect = (0, import_react.useCallback)((relativePath) => {
		if (!activeWorktreeId || !worktreePath) return;
		skipReturnFocus();
		closeModal();
		openFile({
			filePath: joinPath(worktreePath, relativePath),
			relativePath,
			worktreeId: activeWorktreeId,
			language: detectLanguage(relativePath),
			mode: "edit"
		});
	}, [
		activeWorktreeId,
		worktreePath,
		openFile,
		closeModal,
		skipReturnFocus
	]);
	const handleOpenChange = (0, import_react.useCallback)((open) => {
		if (!open) closeModal();
	}, [closeModal]);
	const handleCloseAutoFocus = (0, import_react.useCallback)((e) => {
		e.preventDefault();
	}, []);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CommandDialog, {
		open: visible,
		onOpenChange: handleOpenChange,
		shouldFilter: false,
		onOpenAutoFocus: (0, import_react.useCallback)(() => {
			captureReturnFocus();
		}, [captureReturnFocus]),
		onCloseAutoFocus: handleCloseAutoFocus,
		title: translate("auto.components.QuickOpen.ec31e058f7", "Go to file"),
		description: translate("auto.components.QuickOpen.9e97f08d0f", "Search for a file to open"),
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CommandInput, {
				placeholder: translate("auto.components.QuickOpen.1cb6ef47b7", "Go to file..."),
				value: query,
				onValueChange: setQuery,
				className: "!h-9 !py-2"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CommandList, {
				className: "p-2",
				children: loading ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "py-6 text-center text-sm text-muted-foreground",
					children: translate("auto.components.QuickOpen.722a21e1a8", "Loading files...")
				}) : loadError ? (() => {
					const guidance = parseQuickOpenInstallRgGuidance(loadError);
					return guidance ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(QuickOpenInstallRgGuidance, {
						reason: guidance.reason,
						location: guidance.location,
						command: guidance.command,
						guidance: guidance.guidance
					}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "py-6 px-4 text-center text-sm text-muted-foreground whitespace-pre-wrap",
						children: loadError
					});
				})() : filtered.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CommandEmpty, { children: translate("auto.components.QuickOpen.74e2e1b3e4", "No matching files.") }) : filtered.map((item) => {
					const { directory, filename } = splitTrailingSegment(item.path);
					const FileIcon = getFileTypeIcon(item.path);
					return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CommandItem, {
						value: item.path,
						onSelect: () => handleSelect(item.path),
						className: "min-w-0 !p-0",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(FilePathCursorTooltip, {
							path: item.path,
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "flex w-full min-w-0 items-center gap-2 px-3 py-1",
								children: [
									/* @__PURE__ */ (0, import_jsx_runtime.jsx)(FileIcon, { className: "size-3.5 shrink-0 text-muted-foreground" }),
									/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
										className: "min-w-0 max-w-full shrink-0 truncate text-foreground",
										children: filename
									}),
									directory ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
										className: "min-w-0 truncate text-muted-foreground",
										children: directory
									}) : null
								]
							})
						})
					}, item.path);
				})
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "flex items-center justify-end border-t border-border/60 px-3.5 py-2.5 text-[11px] text-muted-foreground/82",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-center gap-2",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(FooterKey, { children: translate("auto.components.QuickOpen.250e5b2dfb", "Enter") }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.QuickOpen.61b1c871a6", "Open") }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(FooterKey, { children: translate("auto.components.QuickOpen.95fccbae88", "Esc") }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.QuickOpen.73b2c581f1", "Close") }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(FooterKey, { children: "↑↓" }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.QuickOpen.1dbd3f59ff", "Move") })
					]
				})
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				"aria-live": "polite",
				className: "sr-only",
				children: deferredQuery.trim() ? translate("auto.components.QuickOpen.b227d88520", "{{value0}} files found", { value0: filtered.length }) : ""
			})
		]
	});
}
export { QuickOpen as default };
