const __vite__mapDeps=(i,m=__vite__mapDeps,d=(m.f||(m.f=["./python-CjKcLBa-.js","./editor.api2-DX_-Ye6K.js","./defineProperty-BAtR-r70.js","./chunk-Dhmk_5SA.js","./editor-B6UfNlAV.css"])))=>i.map(i=>d[i]);
import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn } from "./button-DszXJEV6.js";
import { t as useAppStore } from "./store-CgXrfmaH.js";
import { t as __vitePreload } from "./preload-helper-Cgw39-ka.js";
import { i as resolveEditorFontFamily, n as computeEditorFontSize } from "./editor-font-zoom-2F4BKkDZ.js";
import { n as resolveDocumentTheme } from "./document-theme-66WaD9Gm.js";
import { h as languages, p as editor } from "./editor.api2-DX_-Ye6K.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var pythonLanguageRegistrationPromise = null;
async function ensureColorizationLanguage(language) {
	if (language !== "python") return;
	pythonLanguageRegistrationPromise ?? (pythonLanguageRegistrationPromise = __vitePreload(async () => {
		const { conf, language: pythonTokens } = await import("./python-CjKcLBa-.js");
		return {
			conf,
			language: pythonTokens
		};
	}, __vite__mapDeps([0,1,2,3,4]), import.meta.url).then(({ conf, language: pythonTokens }) => {
		if (!languages.getLanguages().some((item) => item.id === "python")) languages.register({
			id: "python",
			extensions: [".py", ".pyw"],
			aliases: ["Python", "py"]
		});
		languages.setLanguageConfiguration("python", conf);
		languages.setMonarchTokensProvider("python", pythonTokens);
	}));
	await pythonLanguageRegistrationPromise;
}
function MonacoCodeExcerpt({ lines, firstLineNumber, highlightedStartLine, highlightedEndLine, language }) {
	const settings = useAppStore((s) => s.settings);
	const editorFontZoomLevel = useAppStore((s) => s.editorFontZoomLevel);
	const editorFontSize = computeEditorFontSize(settings?.terminalFontSize ?? 13, editorFontZoomLevel);
	const fontFamily = resolveEditorFontFamily(settings);
	const isDark = resolveDocumentTheme(settings?.theme ?? "system");
	const code = (0, import_react.useMemo)(() => lines.join("\n"), [lines]);
	const [htmlLines, setHtmlLines] = (0, import_react.useState)(() => lines.map(() => ""));
	(0, import_react.useEffect)(() => {
		editor.setTheme(isDark ? "vs-dark" : "vs");
	}, [isDark]);
	(0, import_react.useEffect)(() => {
		if (lines.length === 0) {
			setHtmlLines([]);
			return;
		}
		let cancelled = false;
		ensureColorizationLanguage(language).catch(() => void 0).then(() => editor.colorize(code, language, { tabSize: 2 })).then((html) => {
			if (cancelled) return;
			setHtmlLines(html.split("<br/>").slice(0, lines.length));
		});
		return () => {
			cancelled = true;
		};
	}, [
		code,
		language,
		lines
	]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "overflow-x-auto py-1 text-[12px] leading-5",
		style: {
			fontFamily,
			fontSize: editorFontSize
		},
		children: lines.map((codeLine, index) => {
			const lineNumber = firstLineNumber + index;
			const isCommentedLine = lineNumber >= highlightedStartLine && lineNumber <= highlightedEndLine;
			const html = htmlLines[index] || (codeLine ? void 0 : "&nbsp;");
			return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: cn("flex font-mono", isCommentedLine && "bg-emerald-500/10"),
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "w-12 shrink-0 select-none border-r border-border/40 px-2 text-right text-muted-foreground tabular-nums",
					children: lineNumber
				}), html ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("code", {
					className: "min-w-max flex-1 whitespace-pre px-3 text-foreground",
					dangerouslySetInnerHTML: { __html: html }
				}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("code", {
					className: "min-w-max flex-1 whitespace-pre px-3 text-foreground",
					children: codeLine || " "
				})]
			}, lineNumber);
		})
	});
}
export { MonacoCodeExcerpt as t };
