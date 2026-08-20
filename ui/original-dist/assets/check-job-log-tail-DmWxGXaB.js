import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { l as createLucideIcon, n as cn } from "./button-DszXJEV6.js";
import { t as Check } from "./check-Lb2n4tDb.js";
import { t as Copy } from "./copy-jk2iqVkp.js";
var CircleMinus = createLucideIcon("circle-minus", [["circle", {
	cx: "12",
	cy: "12",
	r: "10",
	key: "1mglay"
}], ["path", {
	d: "M8 12h8",
	key: "1wcyev"
}]]);
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var LOG_EXCERPT_ERROR_LINE_PATTERN = /(?:##\[error\]|::error::|::error\b|\berror:|FAILED|exit code|ENOENT|EACCES|panic:|AssertionError)/i;
function getLogExcerptScrollTop(pre, logTail) {
	const lines = logTail.split(/\r?\n/);
	let targetLineIndex = lines.length - 1;
	for (let index = 0; index < lines.length; index += 1) if (LOG_EXCERPT_ERROR_LINE_PATTERN.test(lines[index] ?? "")) targetLineIndex = index;
	const lineHeight = Number.parseFloat(getComputedStyle(pre).lineHeight);
	const targetScroll = targetLineIndex * (Number.isFinite(lineHeight) ? lineHeight : 16);
	const maxScroll = Math.max(0, pre.scrollHeight - pre.clientHeight);
	if (targetLineIndex < lines.length - 1) return Math.min(maxScroll, Math.max(0, targetScroll - pre.clientHeight / 3));
	return maxScroll;
}
function CopyButton({ text, title = "Copy comment" }) {
	const [copied, setCopied] = (0, import_react.useState)(false);
	const copiedResetTimerRef = (0, import_react.useRef)(null);
	const isMountedRef = (0, import_react.useRef)(false);
	const clearCopiedResetTimer = (0, import_react.useCallback)(() => {
		if (copiedResetTimerRef.current !== null) {
			window.clearTimeout(copiedResetTimerRef.current);
			copiedResetTimerRef.current = null;
		}
	}, []);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
		ref: (0, import_react.useCallback)((node) => {
			isMountedRef.current = node !== null;
			if (node === null) clearCopiedResetTimer();
		}, [clearCopiedResetTimer]),
		className: "p-1 rounded hover:bg-accent text-muted-foreground/40 hover:text-foreground transition-colors shrink-0",
		title,
		onClick: (0, import_react.useCallback)((e) => {
			e.stopPropagation();
			window.api.ui.writeClipboardText(text).then(() => {
				if (!isMountedRef.current) return;
				clearCopiedResetTimer();
				setCopied(true);
				copiedResetTimerRef.current = window.setTimeout(() => {
					copiedResetTimerRef.current = null;
					setCopied(false);
				}, 1500);
			});
		}, [clearCopiedResetTimer, text]),
		children: copied ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, { className: "size-3" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Copy, { className: "size-3" })
	});
}
function CheckJobLogTail({ logTail, expanded = false }) {
	const logPreRef = (0, import_react.useRef)(null);
	(0, import_react.useEffect)(() => {
		const logPre = logPreRef.current;
		if (!logPre) return;
		logPre.scrollTop = getLogExcerptScrollTop(logPre, logTail);
	}, [expanded, logTail]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "mt-3 min-w-0",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "mb-1.5 flex min-w-0 items-center gap-2",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "min-w-0 flex-1 text-[10px] font-semibold uppercase tracking-wide text-muted-foreground",
				children: translate("auto.components.right.sidebar.checks.panel.content.d713f500b2", "Log excerpt")
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CopyButton, {
				text: logTail,
				title: translate("auto.components.right.sidebar.checks.panel.content.679bf2093c", "Copy log excerpt")
			})]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("pre", {
			ref: logPreRef,
			className: cn("overflow-auto whitespace-pre-wrap rounded bg-muted/40 p-3 font-mono text-xs text-muted-foreground scrollbar-sleek", expanded ? "min-h-48 max-h-[min(50vh,32rem)]" : "max-h-72"),
			children: logTail
		})]
	});
}
export { CircleMinus as n, CheckJobLogTail as t };
