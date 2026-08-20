import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { l as createLucideIcon, n as cn, t as Button } from "./button-DszXJEV6.js";
import { t as Braces } from "./braces-h3B7q7My.js";
import { t as CircleAlert } from "./circle-alert-keRTpMg-.js";
import { t as FileCodeCorner } from "./file-code-corner-UKtQpxid.js";
import { t as LoaderCircle } from "./loader-circle-CRZpWdsi.js";
import { t as Play } from "./play-8NM3T-Fw.js";
import { t as Save } from "./save-DszBi16-.js";
import { Vu as createBrowserUuid, m_ as Trash2, t as useAppStore } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
import "./useMountedRef-1omUd-IV.js";
import { t as getConnectionId } from "./connection-context-BUPsamzR.js";
import { a as resolveEditorFontFamilyOrInherit, i as resolveEditorFontFamily, n as computeEditorFontSize } from "./editor-font-zoom-2F4BKkDZ.js";
import { n as registerPendingEditorFlush } from "./editor-pending-flush-sUX7yPKj.js";
import { o as useShortcutKeyDetails } from "./useShortcutLabel-C-KRYtlB.js";
import { t as ShortcutKeyCombo } from "./ShortcutKeyCombo-Ch456Md0.js";
import { a as DialogFooter, i as DialogDescription, o as DialogHeader, r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
import { t as remarkGfm } from "./lib-CtirWBBB.js";
import { r as rehypeRaw, s as Markdown, t as rehypeSanitize } from "./lib-D08jHVMa.js";
import { t as purify } from "./purify.es-C_rn83UJ.js";
import "./text-control-paste-PhBVbE2p.js";
import "./paste-payload-metadata-pr3nuODB.js";
import { n as resolveDocumentTheme } from "./document-theme-66WaD9Gm.js";
import { p as editor } from "./editor.api2-DX_-Ye6K.js";
import "./workers-D5nLH-xK.js";
import "./monaco.contribution-BINL69Me.js";
import { n as Ft } from "./monaco-setup-CKSA6ArO.js";
import "./editor.main-BGL6BKIn.js";
import { a as installMonacoEditorFindShortcut, r as installEditorSaveShortcut, t as editorShortcutMatches } from "./editor-shortcuts-Cg6u73ie.js";
import { a as setWithLRU, i as scrollTopCache } from "./scroll-cache-B8ebRfkp.js";
import { t as MonacoCodeExcerpt } from "./MonacoCodeExcerpt-C9RniPVg.js";
var ArrowDownToLine = createLucideIcon("arrow-down-to-line", [
	["path", {
		d: "M12 17V3",
		key: "1cwfxf"
	}],
	["path", {
		d: "m6 11 6 6 6-6",
		key: "12ii2o"
	}],
	["path", {
		d: "M19 21H5",
		key: "150jfl"
	}]
]);
var ArrowUpToLine = createLucideIcon("arrow-up-to-line", [
	["path", {
		d: "M5 3h14",
		key: "7usisc"
	}],
	["path", {
		d: "m18 13-6-6-6 6",
		key: "1kf1n9"
	}],
	["path", {
		d: "M12 7v14",
		key: "1akyts"
	}]
]);
var MoveDown = createLucideIcon("move-down", [["path", {
	d: "M8 18L12 22L16 18",
	key: "cskvfv"
}], ["path", {
	d: "M12 2V22",
	key: "r89rzk"
}]]);
var MoveUp = createLucideIcon("move-up", [["path", {
	d: "M8 6L12 2L16 6",
	key: "1yvkyx"
}], ["path", {
	d: "M12 2V22",
	key: "r89rzk"
}]]);
var import_react = /* @__PURE__ */ __toESM(require_react());
const IPYNB_CODE_CELL_EDITOR_HEIGHT_SCAN_CODE_UNITS = 64 * 1024;
const IPYNB_CODE_CELL_PREVIEW_SCAN_CODE_UNITS = 64 * 1024;
const IPYNB_CODE_CELL_PREVIEW_LINE_MAX_CODE_UNITS = 8 * 1024;
var IPYNB_CODE_CELL_EDITOR_MIN_HEIGHT_PX = 96;
var IPYNB_CODE_CELL_EDITOR_MAX_HEIGHT_PX = 520;
var LINE_FEED_CODE_UNIT = 10;
var CARRIAGE_RETURN_CODE_UNIT = 13;
function getIpynbCodeCellEditorHeight(source, fontSize) {
	const rowHeight = Math.max(1, fontSize + 8);
	const rowCount = countIpynbCodeCellRowsForHeight(source, Math.ceil(IPYNB_CODE_CELL_EDITOR_MAX_HEIGHT_PX / rowHeight));
	return Math.min(IPYNB_CODE_CELL_EDITOR_MAX_HEIGHT_PX, Math.max(IPYNB_CODE_CELL_EDITOR_MIN_HEIGHT_PX, rowCount * rowHeight));
}
function getIpynbCodeCellPreviewLines(source) {
	if (source.length === 0) return [""];
	const lines = [];
	const scanLength = Math.min(source.length, IPYNB_CODE_CELL_PREVIEW_SCAN_CODE_UNITS);
	let lineStart = 0;
	for (let index = 0; index < scanLength; index += 1) {
		if (source.charCodeAt(index) !== LINE_FEED_CODE_UNIT) continue;
		lines.push(sliceIpynbCodeCellPreviewLine(source, lineStart, index));
		if (lines.length >= 200) return lines;
		lineStart = index + 1;
	}
	if (lineStart < scanLength) lines.push(sliceIpynbCodeCellPreviewLine(source, lineStart, scanLength));
	return lines.length > 0 ? lines : [""];
}
function countIpynbCodeCellRowsForHeight(source, capRows) {
	if (source.length === 0) return 3;
	const scanLength = Math.min(source.length, IPYNB_CODE_CELL_EDITOR_HEIGHT_SCAN_CODE_UNITS);
	let rowCount = 2;
	for (let index = 0; index < scanLength; index += 1) {
		if (source.charCodeAt(index) !== LINE_FEED_CODE_UNIT) continue;
		rowCount += 1;
		if (rowCount >= capRows) return rowCount;
	}
	return Math.max(3, rowCount);
}
function sliceIpynbCodeCellPreviewLine(source, lineStart, lineEnd) {
	const normalizedLineEnd = lineEnd > lineStart && source.charCodeAt(lineEnd - 1) === CARRIAGE_RETURN_CODE_UNIT ? lineEnd - 1 : lineEnd;
	return source.slice(lineStart, Math.min(normalizedLineEnd, lineStart + IPYNB_CODE_CELL_PREVIEW_LINE_MAX_CODE_UNITS));
}
var DISPLAY_MIME_ORDER = [
	"text/html",
	"image/png",
	"image/jpeg",
	"image/jpg",
	"image/svg+xml",
	"application/json",
	"text/markdown",
	"text/plain"
];
var JUPYTER_LANGUAGE_TO_MONACO_LANGUAGE = {
	"c#": "csharp",
	"f#": "fsharp",
	"q#": "qsharp",
	"c++11": "cpp",
	"c++12": "cpp",
	"c++14": "cpp",
	"c++": "cpp"
};
function isRecord(value) {
	return typeof value === "object" && value !== null && !Array.isArray(value);
}
function concatIpynbMultilineString(value) {
	if (Array.isArray(value)) {
		let result = "";
		for (let i = 0; i < value.length; i += 1) {
			const item = String(value[i] ?? "");
			result += i < value.length - 1 && !item.endsWith("\n") ? `${item}\n` : item;
		}
		return result.replace(/\r\n/g, "\n");
	}
	return String(value ?? "").replace(/\r\n/g, "\n");
}
function translateKernelLanguageToMonaco(language) {
	const normalized = (language ?? "python").toLowerCase();
	if (normalized.length === 2 && normalized.endsWith("#")) return `${normalized.slice(0, 1)}sharp`;
	return JUPYTER_LANGUAGE_TO_MONACO_LANGUAGE[normalized] ?? normalized;
}
function getPreferredLanguage(content) {
	const metadata = isRecord(content.metadata) ? content.metadata : {};
	const languageInfo = isRecord(metadata.language_info) ? metadata.language_info : {};
	const kernelSpec = isRecord(metadata.kernelspec) ? metadata.kernelspec : {};
	return translateKernelLanguageToMonaco(typeof languageInfo.name === "string" ? languageInfo.name : typeof kernelSpec.language === "string" ? kernelSpec.language : "python");
}
function getKernelName(content) {
	const metadata = isRecord(content.metadata) ? content.metadata : {};
	const kernelSpec = isRecord(metadata.kernelspec) ? metadata.kernelspec : {};
	return typeof kernelSpec.display_name === "string" ? kernelSpec.display_name : typeof kernelSpec.name === "string" ? kernelSpec.name : null;
}
function getCellLanguage(cell, fallback) {
	const metadata = isRecord(cell.metadata) ? cell.metadata : {};
	const vscode = isRecord(metadata.vscode) ? metadata.vscode : {};
	return typeof vscode.languageId === "string" ? vscode.languageId : fallback;
}
function parseDisplayItems(data) {
	if (!isRecord(data)) return [];
	return Object.entries(data).map(([mime, value]) => ({
		mime,
		value
	})).sort((a, b) => {
		const aIndex = DISPLAY_MIME_ORDER.indexOf(a.mime);
		const bIndex = DISPLAY_MIME_ORDER.indexOf(b.mime);
		return (aIndex === -1 ? 100 : aIndex) - (bIndex === -1 ? 100 : bIndex);
	});
}
function parseOutput(rawOutput) {
	if (!isRecord(rawOutput) || typeof rawOutput.output_type !== "string") return null;
	if (rawOutput.output_type === "stream") return {
		kind: "stream",
		name: typeof rawOutput.name === "string" ? rawOutput.name : "stdout",
		text: concatIpynbMultilineString(rawOutput.text)
	};
	if (rawOutput.output_type === "error") return {
		kind: "error",
		name: typeof rawOutput.ename === "string" ? rawOutput.ename : "",
		message: typeof rawOutput.evalue === "string" ? rawOutput.evalue : "",
		traceback: concatIpynbMultilineString(rawOutput.traceback)
	};
	return {
		kind: "display",
		outputType: rawOutput.output_type,
		executionCount: typeof rawOutput.execution_count === "number" ? rawOutput.execution_count : null,
		items: parseDisplayItems(rawOutput.data)
	};
}
function parseCell(rawCell, fallbackLanguage) {
	if (!isRecord(rawCell)) return null;
	const kind = rawCell.cell_type === "markdown" || rawCell.cell_type === "raw" || rawCell.cell_type === "code" ? rawCell.cell_type : null;
	if (kind === null) return null;
	const outputs = Array.isArray(rawCell.outputs) ? rawCell.outputs.map(parseOutput).filter((output) => output !== null) : [];
	return {
		id: typeof rawCell.id === "string" ? rawCell.id : null,
		kind,
		language: kind === "code" ? getCellLanguage(rawCell, fallbackLanguage) : kind,
		source: concatIpynbMultilineString(rawCell.source),
		executionCount: typeof rawCell.execution_count === "number" ? rawCell.execution_count : null,
		outputs
	};
}
function parseIpynb(content) {
	const parsed = JSON.parse(content);
	if (!isRecord(parsed)) throw new Error("Notebook root must be a JSON object");
	if (!Array.isArray(parsed.cells)) throw new Error("Notebook is missing a cells array");
	const language = getPreferredLanguage(parsed);
	const cells = parsed.cells.map((cell) => parseCell(cell, language)).filter((cell) => cell !== null);
	return {
		language,
		kernelName: getKernelName(parsed),
		nbformat: typeof parsed.nbformat === "number" ? `${parsed.nbformat}.${typeof parsed.nbformat_minor === "number" ? parsed.nbformat_minor : 0}` : "unknown",
		cells
	};
}
function splitIpynbSource(source) {
	if (!source) return [];
	const lines = [];
	let lineStart = 0;
	for (let index = 0; index < source.length; index += 1) {
		if (source.charCodeAt(index) !== 10) continue;
		lines.push(source.slice(lineStart, index + 1));
		lineStart = index + 1;
	}
	if (lineStart < source.length) lines.push(source.slice(lineStart));
	return lines;
}
function parseNotebookRoot(content) {
	const parsed = JSON.parse(content);
	if (!isRecord(parsed)) throw new Error("Notebook root must be a JSON object");
	if (!Array.isArray(parsed.cells)) throw new Error("Notebook is missing a cells array");
	return parsed;
}
function ensureCell(root, index) {
	const cells = root.cells;
	if (!Array.isArray(cells) || !isRecord(cells[index])) throw new Error("Notebook cell no longer exists");
	return cells[index];
}
function serializeNotebook(root) {
	return `${JSON.stringify(root, null, 1)}\n`;
}
function updateIpynbCellSources(content, updates) {
	if (updates.length === 0) return content;
	const root = parseNotebookRoot(content);
	for (const update of updates) ensureCell(root, update.index).source = splitIpynbSource(update.source);
	return serializeNotebook(root);
}
function updateIpynbCellKind(content, index, kind, fallbackLanguage) {
	const root = parseNotebookRoot(content);
	const cell = ensureCell(root, index);
	cell.cell_type = kind;
	if (kind === "code") {
		cell.outputs = Array.isArray(cell.outputs) ? cell.outputs : [];
		cell.execution_count = typeof cell.execution_count === "number" ? cell.execution_count : null;
		cell.metadata = isRecord(cell.metadata) ? cell.metadata : {};
		const metadata = cell.metadata;
		metadata.vscode = {
			...isRecord(metadata.vscode) ? metadata.vscode : {},
			languageId: fallbackLanguage
		};
	} else {
		delete cell.outputs;
		delete cell.execution_count;
	}
	return serializeNotebook(root);
}
function insertIpynbCell(content, index, kind, language) {
	const root = parseNotebookRoot(content);
	const cells = root.cells;
	const nextCell = {
		cell_type: kind,
		id: createBrowserUuid(),
		metadata: {},
		source: []
	};
	if (kind === "code") {
		nextCell.execution_count = null;
		nextCell.outputs = [];
		nextCell.metadata = { vscode: { languageId: language } };
	}
	cells.splice(Math.min(Math.max(index, 0), cells.length), 0, nextCell);
	return serializeNotebook(root);
}
function deleteIpynbCell(content, index) {
	const root = parseNotebookRoot(content);
	const cells = root.cells;
	if (cells.length <= 1) cells.splice(0, cells.length, {
		cell_type: "code",
		id: createBrowserUuid(),
		metadata: {},
		execution_count: null,
		outputs: [],
		source: []
	});
	else cells.splice(index, 1);
	return serializeNotebook(root);
}
function moveIpynbCell(content, index, direction) {
	const root = parseNotebookRoot(content);
	const cells = root.cells;
	const nextIndex = index + direction;
	if (index < 0 || index >= cells.length || nextIndex < 0 || nextIndex >= cells.length) return content;
	const [cell] = cells.splice(index, 1);
	cells.splice(nextIndex, 0, cell);
	return serializeNotebook(root);
}
function updateIpynbCellOutputs(content, index, result) {
	const root = parseNotebookRoot(content);
	const cell = ensureCell(root, index);
	const outputs = [];
	if (result.stdout) outputs.push({
		output_type: "stream",
		name: "stdout",
		text: splitIpynbSource(result.stdout)
	});
	if (result.stderr && result.exitCode === 0 && !result.error) outputs.push({
		output_type: "stream",
		name: "stderr",
		text: splitIpynbSource(result.stderr)
	});
	if (result.error || (result.exitCode ?? 0) !== 0) {
		const message = result.error || result.stderr || `Process exited with code ${result.exitCode}`;
		outputs.push({
			output_type: "error",
			ename: "PythonError",
			evalue: message,
			traceback: splitIpynbSource(result.stderr || message)
		});
	}
	cell.outputs = outputs;
	cell.execution_count = typeof cell.execution_count === "number" ? cell.execution_count + 1 : 1;
	return serializeNotebook(root);
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var NOTEBOOK_SOURCE_COMMIT_DELAY_MS = 400;
function cancelIpynbStructuralContentFrames(frameIds) {
	for (const frameId of frameIds.current) cancelAnimationFrame(frameId);
	frameIds.current = [];
}
function requestIpynbStructuralContentFrame(frameIds, callback) {
	let completed = false;
	let frameId;
	frameId = requestAnimationFrame((timestamp) => {
		completed = true;
		if (frameId !== void 0) frameIds.current = frameIds.current.filter((pendingFrameId) => pendingFrameId !== frameId);
		callback(timestamp);
	});
	if (!completed) frameIds.current.push(frameId);
}
function createNotebookExecutionTrustState(filePath) {
	return {
		filePath,
		trustedForFile: false,
		pendingRunCellIndex: null
	};
}
function valueToText(value) {
	if (Array.isArray(value)) return value.map((item) => String(item ?? "")).join("");
	if (typeof value === "string") return value;
	if (value === void 0 || value === null) return "";
	return typeof value === "object" ? JSON.stringify(value, null, 2) : String(value);
}
function dataUriForImage(item) {
	const value = valueToText(item.value).replace(/\s/g, "");
	if (!value) return null;
	if (item.mime === "image/svg+xml") return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(valueToText(item.value))}`;
	return `data:${item.mime};base64,${value}`;
}
function NotebookCellHeader({ cell, index, running, canMoveUp, canMoveDown, onRun, onKindChange, onInsertAbove, onInsertBelow, onMoveUp, onMoveDown, onDelete }) {
	const Icon = cell.kind === "code" ? Play : cell.kind === "markdown" ? FileCodeCorner : Braces;
	const executionLabel = cell.kind === "code" ? `In [${cell.executionCount ?? " "}]:` : cell.kind;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex items-center gap-2 border-b border-border/50 bg-muted/20 px-3 py-1.5 text-xs text-muted-foreground",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { className: "size-3.5" }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "font-mono",
				children: executionLabel
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("select", {
				value: cell.kind,
				onChange: (event) => onKindChange(event.target.value),
				className: "h-7 rounded-md border border-input bg-background px-2 text-xs text-foreground",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("option", {
						value: "code",
						children: translate("auto.components.editor.IpynbViewer.7005960d73", "Code")
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("option", {
						value: "markdown",
						children: translate("auto.components.editor.IpynbViewer.1833dbbc43", "Markdown")
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("option", {
						value: "raw",
						children: translate("auto.components.editor.IpynbViewer.3e4cbf15ea", "Raw")
					})
				]
			}),
			cell.kind === "code" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(NotebookHeaderButton, {
				label: translate("auto.components.editor.IpynbViewer.859bf9fc21", "Run cell"),
				disabled: running,
				onClick: onRun,
				children: running ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3.5 animate-spin" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Play, { className: "size-3.5" })
			}) : null,
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(NotebookHeaderButton, {
				label: translate("auto.components.editor.IpynbViewer.fd8ac707bc", "Move cell up"),
				disabled: !canMoveUp,
				onClick: onMoveUp,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MoveUp, { className: "size-3.5" })
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(NotebookHeaderButton, {
				label: translate("auto.components.editor.IpynbViewer.27e064e2db", "Move cell down"),
				disabled: !canMoveDown,
				onClick: onMoveDown,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MoveDown, { className: "size-3.5" })
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(NotebookHeaderButton, {
				label: translate("auto.components.editor.IpynbViewer.53b839b8a0", "Insert code cell above"),
				onClick: () => onInsertAbove("code"),
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowUpToLine, { className: "size-3.5" })
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(NotebookHeaderButton, {
				label: translate("auto.components.editor.IpynbViewer.b4208cad7e", "Insert code cell below"),
				onClick: () => onInsertBelow("code"),
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowDownToLine, { className: "size-3.5" })
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(NotebookHeaderButton, {
				label: translate("auto.components.editor.IpynbViewer.ffc1ac2699", "Insert markdown cell above"),
				onClick: () => onInsertAbove("markdown"),
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
					className: "relative size-4",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(FileCodeCorner, { className: "absolute left-0.5 top-0.5 size-3" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MoveUp, { className: "absolute -right-0.5 -top-0.5 size-2.5" })]
				})
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(NotebookHeaderButton, {
				label: translate("auto.components.editor.IpynbViewer.b42f6a9547", "Insert markdown cell below"),
				onClick: () => onInsertBelow("markdown"),
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
					className: "relative size-4",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(FileCodeCorner, { className: "absolute left-0.5 top-0.5 size-3" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MoveDown, { className: "absolute -bottom-0.5 -right-0.5 size-2.5" })]
				})
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(NotebookHeaderButton, {
				label: translate("auto.components.editor.IpynbViewer.781abd6926", "Delete cell"),
				onClick: onDelete,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, { className: "size-3.5" })
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
				className: "ml-auto font-mono",
				children: ["#", index + 1]
			})
		]
	});
}
function NotebookHeaderButton({ label, disabled = false, shortcut, onClick, children }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
		asChild: true,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
			type: "button",
			variant: "ghost",
			size: "icon",
			className: "size-7",
			"aria-label": label,
			disabled,
			onClick,
			children
		})
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
		className: "flex items-center gap-2",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: label }), shortcut && shortcut.keys.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ShortcutKeyCombo, {
			keys: shortcut.keys,
			doubleTap: shortcut.doubleTap
		}) : null]
	}) })] });
}
function MarkdownCell({ source }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "markdown-preview-body px-4 py-3 text-sm",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Markdown, {
			remarkPlugins: [remarkGfm],
			rehypePlugins: [rehypeRaw, rehypeSanitize],
			children: source || "\xA0"
		})
	});
}
function CodeCell({ cell, source, active, onActivate, onDeactivate, onChange, onSaveRequest }) {
	const settings = useAppStore((s) => s.settings);
	const editorFontZoomLevel = useAppStore((s) => s.editorFontZoomLevel);
	const onDeactivateRef = (0, import_react.useRef)(onDeactivate);
	const onSaveRequestRef = (0, import_react.useRef)(onSaveRequest);
	onDeactivateRef.current = onDeactivate;
	onSaveRequestRef.current = onSaveRequest;
	const fontSize = computeEditorFontSize(settings?.terminalFontSize ?? 13, editorFontZoomLevel);
	const editorHeight = getIpynbCodeCellEditorHeight(source, fontSize);
	const isDark = resolveDocumentTheme(settings?.theme ?? "system");
	const lines = (0, import_react.useMemo)(() => getIpynbCodeCellPreviewLines(source), [source]);
	const handleMount = (0, import_react.useCallback)((editorInstance, monacoInstance) => {
		editorInstance.focus();
		const cleanupSaveShortcut = installEditorSaveShortcut(editorInstance.getContainerDomNode(), () => {
			onSaveRequestRef.current();
		});
		const cleanupFindShortcut = installMonacoEditorFindShortcut(editorInstance);
		const blurSub = editorInstance.onDidBlurEditorWidget(() => {
			onDeactivateRef.current();
		});
		editorInstance.onDidDispose(() => {
			cleanupSaveShortcut();
			cleanupFindShortcut();
			blurSub.dispose();
		});
		editorInstance.addCommand(monacoInstance.KeyCode.Escape, () => {
			onDeactivateRef.current();
		});
	}, []);
	(0, import_react.useEffect)(() => {
		editor.setTheme(isDark ? "vs-dark" : "vs");
	}, [isDark]);
	if (!active) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		role: "button",
		tabIndex: 0,
		className: "block w-full cursor-text bg-editor-surface text-left",
		onClick: onActivate,
		onKeyDown: (event) => {
			if (event.key === "Enter") onActivate();
		},
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MonacoCodeExcerpt, {
			lines,
			firstLineNumber: 1,
			highlightedStartLine: -1,
			highlightedEndLine: -1,
			language: cell.language
		})
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "bg-editor-surface focus-within:ring-1 focus-within:ring-ring",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Ft, {
			height: editorHeight,
			defaultLanguage: cell.language,
			language: cell.language,
			theme: isDark ? "vs-dark" : "vs",
			value: source,
			onMount: handleMount,
			onChange: (value) => onChange(value ?? ""),
			options: {
				automaticLayout: true,
				fontFamily: resolveEditorFontFamily(settings),
				fontSize,
				glyphMargin: false,
				lineNumbersMinChars: 3,
				minimap: { enabled: false },
				overviewRulerLanes: 0,
				renderLineHighlight: "none",
				scrollBeyondLastLine: false,
				wordWrap: "off"
			}
		})
	});
}
var MemoizedCodeCell = import_react.memo(CodeCell);
function getCellKey(cell, index) {
	return cell.id ?? `${index}:${cell.kind}`;
}
function hasOwnDraft(drafts, key) {
	return Object.hasOwn(drafts, key);
}
function EditableTextCell({ source, onChange }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("textarea", {
		value: source,
		onChange: (event) => onChange(event.target.value),
		className: "block min-h-24 w-full resize-y border-0 bg-background px-4 py-3 text-sm text-foreground outline-none focus:ring-1 focus:ring-ring"
	});
}
function PreformattedOutput({ text, error = false }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("pre", {
		className: cn("max-h-[420px] overflow-auto whitespace-pre-wrap px-3 py-2 font-mono text-xs leading-5 scrollbar-editor", error ? "text-destructive" : "text-foreground"),
		children: text
	});
}
function OutputItem({ item }) {
	if (item.mime === "text/html") {
		const html = purify.sanitize(valueToText(item.value), { USE_PROFILES: {
			html: true,
			svg: true,
			svgFilters: true
		} });
		return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("iframe", {
			title: translate("auto.components.editor.IpynbViewer.66a3f7d330", "Notebook HTML output"),
			sandbox: "",
			referrerPolicy: "no-referrer",
			loading: "lazy",
			className: "block h-80 w-full border-0 bg-background",
			srcDoc: html
		});
	}
	if (item.mime.startsWith("image/")) {
		const uri = dataUriForImage(item);
		if (!uri) return null;
		return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "flex max-w-full overflow-auto p-3 scrollbar-editor",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("img", {
				src: uri,
				alt: item.mime,
				className: "max-h-[520px] max-w-full object-contain"
			})
		});
	}
	if (item.mime === "application/json" || item.mime.endsWith("+json")) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PreformattedOutput, { text: typeof item.value === "string" ? item.value : JSON.stringify(item.value ?? null, null, 2) });
	if (item.mime === "text/markdown") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MarkdownCell, { source: valueToText(item.value) });
	if (item.mime.startsWith("text/") || item.mime === "application/javascript") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PreformattedOutput, { text: valueToText(item.value) });
	return null;
}
function CellOutputs({ cell }) {
	if (cell.outputs.length === 0) return null;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "border-t border-border/50 bg-background",
		children: cell.outputs.map((output, index) => {
			if (output.kind === "stream") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PreformattedOutput, { text: output.text }, index);
			if (output.kind === "error") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "border-l-2 border-destructive",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PreformattedOutput, {
					error: true,
					text: [
						output.name,
						output.message,
						output.traceback
					].filter(Boolean).join("\n")
				})
			}, index);
			const renderedItems = output.items.map((item, itemIndex) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(OutputItem, { item }, `${item.mime}-${itemIndex}`)).filter(Boolean);
			if (renderedItems.length === 0) return null;
			return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "border-b border-border/40 last:border-b-0",
				children: renderedItems
			}, index);
		})
	});
}
function IpynbViewer({ content, fileId, filePath, worktreeId, scrollCacheKey, onContentChange, onDirtyStateHint, onSave }) {
	const rootRef = (0, import_react.useRef)(null);
	const settings = useAppStore((s) => s.settings);
	const editorFontZoomLevel = useAppStore((s) => s.editorFontZoomLevel);
	const [runningCellIndex, setRunningCellIndex] = (0, import_react.useState)(null);
	const [runError, setRunError] = (0, import_react.useState)(null);
	const [editingCellKey, setEditingCellKey] = (0, import_react.useState)(null);
	const [executionTrustState, setExecutionTrustState] = (0, import_react.useState)(() => createNotebookExecutionTrustState(filePath));
	const [sourceDrafts, setSourceDrafts] = (0, import_react.useState)({});
	const sourceDraftsRef = (0, import_react.useRef)(sourceDrafts);
	const contentRef = (0, import_react.useRef)(content);
	const notebookRef = (0, import_react.useRef)(null);
	const onContentChangeRef = (0, import_react.useRef)(onContentChange);
	const onDirtyStateHintRef = (0, import_react.useRef)(onDirtyStateHint);
	const sourceCommitTimerRef = (0, import_react.useRef)(null);
	const structuralContentFrameIdsRef = (0, import_react.useRef)([]);
	const fontSize = computeEditorFontSize(13, editorFontZoomLevel);
	const parsed = (0, import_react.useMemo)(() => {
		try {
			return {
				notebook: parseIpynb(content),
				error: null
			};
		} catch (error) {
			return {
				notebook: null,
				error: error instanceof Error ? error.message : "Invalid notebook"
			};
		}
	}, [content]);
	contentRef.current = content;
	notebookRef.current = parsed.notebook;
	onContentChangeRef.current = onContentChange;
	onDirtyStateHintRef.current = onDirtyStateHint;
	if (executionTrustState.filePath !== filePath) setExecutionTrustState(createNotebookExecutionTrustState(filePath));
	const executionTrustedForFile = executionTrustState.filePath === filePath ? executionTrustState.trustedForFile : false;
	const pendingRunCellIndex = executionTrustState.filePath === filePath ? executionTrustState.pendingRunCellIndex : null;
	const setPendingRunCellIndexForFile = (nextPendingRunCellIndex) => {
		setExecutionTrustState((current) => ({
			filePath,
			trustedForFile: current.filePath === filePath ? current.trustedForFile : false,
			pendingRunCellIndex: nextPendingRunCellIndex
		}));
	};
	const trustFileForExecution = () => {
		setExecutionTrustState({
			filePath,
			trustedForFile: true,
			pendingRunCellIndex: null
		});
	};
	const materializeSourceDrafts = (0, import_react.useCallback)(() => {
		const notebook$1 = notebookRef.current;
		const drafts = sourceDraftsRef.current;
		if (!notebook$1 || Object.keys(drafts).length === 0) return contentRef.current;
		const updates = notebook$1.cells.map((cell, index) => {
			const key = getCellKey(cell, index);
			return hasOwnDraft(drafts, key) ? {
				index,
				source: drafts[key] ?? ""
			} : null;
		}).filter((update) => update !== null);
		return updateIpynbCellSources(contentRef.current, updates);
	}, []);
	const flushSourceDrafts = (0, import_react.useCallback)(() => {
		if (sourceCommitTimerRef.current !== null) {
			clearTimeout(sourceCommitTimerRef.current);
			sourceCommitTimerRef.current = null;
		}
		const nextContent = materializeSourceDrafts();
		if (nextContent !== contentRef.current) {
			contentRef.current = nextContent;
			onContentChangeRef.current(nextContent);
		}
		return nextContent;
	}, [materializeSourceDrafts]);
	const queueSourceDraftCommit = (0, import_react.useCallback)(() => {
		if (sourceCommitTimerRef.current !== null) clearTimeout(sourceCommitTimerRef.current);
		sourceCommitTimerRef.current = setTimeout(() => {
			flushSourceDrafts();
		}, NOTEBOOK_SOURCE_COMMIT_DELAY_MS);
	}, [flushSourceDrafts]);
	(0, import_react.useEffect)(() => {
		return registerPendingEditorFlush(fileId, flushSourceDrafts);
	}, [fileId, flushSourceDrafts]);
	const setRootRef = (0, import_react.useCallback)((node) => {
		rootRef.current = node;
		if (node !== null) return;
		flushSourceDrafts();
		cancelIpynbStructuralContentFrames(structuralContentFrameIdsRef);
	}, [flushSourceDrafts]);
	(0, import_react.useEffect)(() => {
		if (!parsed.notebook || Object.keys(sourceDraftsRef.current).length === 0) return;
		const nextDrafts = { ...sourceDraftsRef.current };
		let changed = false;
		parsed.notebook.cells.forEach((cell, index) => {
			const key = getCellKey(cell, index);
			if (hasOwnDraft(nextDrafts, key) && nextDrafts[key] === cell.source) {
				delete nextDrafts[key];
				changed = true;
			}
		});
		if (changed) {
			sourceDraftsRef.current = nextDrafts;
			setSourceDrafts(nextDrafts);
		}
	}, [parsed.notebook]);
	(0, import_react.useLayoutEffect)(() => {
		const container = rootRef.current;
		if (!container) return;
		let throttleTimer = null;
		const onScroll = () => {
			if (throttleTimer !== null) clearTimeout(throttleTimer);
			throttleTimer = setTimeout(() => {
				setWithLRU(scrollTopCache, scrollCacheKey, container.scrollTop);
				throttleTimer = null;
			}, 150);
		};
		container.addEventListener("scroll", onScroll, { passive: true });
		return () => {
			if (container.scrollHeight > container.clientHeight || container.scrollTop > 0) setWithLRU(scrollTopCache, scrollCacheKey, container.scrollTop);
			if (throttleTimer !== null) clearTimeout(throttleTimer);
			container.removeEventListener("scroll", onScroll);
		};
	}, [scrollCacheKey]);
	(0, import_react.useLayoutEffect)(() => {
		const container = rootRef.current;
		const targetScrollTop = scrollTopCache.get(scrollCacheKey);
		if (!container || targetScrollTop === void 0) return;
		container.scrollTop = targetScrollTop;
	}, [scrollCacheKey, content]);
	const saveNotebook = (0, import_react.useCallback)(async () => {
		await onSave(flushSourceDrafts());
	}, [flushSourceDrafts, onSave]);
	const saveShortcut = useShortcutKeyDetails("editor.save");
	const handleNotebookKeyDownCapture = (0, import_react.useCallback)((event) => {
		if (event.repeat || !editorShortcutMatches("editor.save", event)) return;
		event.preventDefault();
		event.stopPropagation();
		saveNotebook();
	}, [saveNotebook]);
	const handleNotebookPointerDownCapture = (0, import_react.useCallback)((event) => {
		if (editingCellKey === null) return;
		if ((event.target instanceof Element ? event.target : null)?.closest(".monaco-editor")) return;
		setEditingCellKey(null);
	}, [editingCellKey]);
	if (parsed.error || !parsed.notebook) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "flex h-full items-center justify-center bg-editor-surface p-6 text-sm text-muted-foreground",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex max-w-md items-start gap-3 rounded-md border border-border bg-background p-4",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CircleAlert, { className: "mt-0.5 size-4 text-destructive" }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "font-medium text-foreground",
				children: translate("auto.components.editor.IpynbViewer.c1601b23b2", "Unable to render notebook")
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "mt-1",
				children: parsed.error
			})] })]
		})
	});
	const { notebook } = parsed;
	const applyContent = (nextContent) => {
		contentRef.current = nextContent;
		onContentChange(nextContent);
	};
	const updateCellSource = (index, source) => {
		const cell = notebook.cells[index];
		if (!cell) return;
		const key = getCellKey(cell, index);
		const nextDrafts = {
			...sourceDraftsRef.current,
			[key]: source
		};
		sourceDraftsRef.current = nextDrafts;
		setSourceDrafts(nextDrafts);
		onDirtyStateHintRef.current(true);
		queueSourceDraftCommit();
	};
	const applyStructuralContentChange = (getNextContent) => {
		const latestContent = flushSourceDrafts();
		setEditingCellKey(null);
		requestIpynbStructuralContentFrame(structuralContentFrameIdsRef, () => {
			applyContent(getNextContent(latestContent));
		});
	};
	const updateCellKind = (index, kind) => {
		applyStructuralContentChange((latestContent) => updateIpynbCellKind(latestContent, index, kind, notebook.language));
	};
	const insertCell = (index, kind) => {
		applyStructuralContentChange((latestContent) => insertIpynbCell(latestContent, index, kind, notebook.language));
	};
	const moveCell = (index, direction) => {
		applyStructuralContentChange((latestContent) => moveIpynbCell(latestContent, index, direction));
	};
	const deleteCell = (index) => {
		applyStructuralContentChange((latestContent) => deleteIpynbCell(latestContent, index));
	};
	const runCell = async (index, options = {}) => {
		const latestContent = flushSourceDrafts();
		const latestNotebook = parseIpynb(latestContent);
		const cell = latestNotebook.cells[index];
		if (!cell || cell.kind !== "code" || runningCellIndex !== null) return;
		if (!executionTrustedForFile && !options.skipTrustPrompt) {
			setPendingRunCellIndexForFile(index);
			return;
		}
		setRunError(null);
		setRunningCellIndex(index);
		try {
			if (!await onSave(latestContent)) return;
			applyContent(updateIpynbCellOutputs(latestContent, index, await window.api.notebook.runPythonCell({
				filePath,
				code: cell.source,
				preamble: latestNotebook.cells.slice(0, index).filter((previousCell) => previousCell.kind === "code").map((previousCell) => previousCell.source).join("\n\n"),
				connectionId: getConnectionId(worktreeId) ?? void 0
			})));
		} catch (error) {
			setRunError(error instanceof Error ? error.message : String(error));
		} finally {
			setRunningCellIndex(null);
		}
	};
	const cancelPendingRun = () => setPendingRunCellIndexForFile(null);
	const confirmPendingRun = () => {
		const index = pendingRunCellIndex;
		trustFileForExecution();
		if (index !== null) runCell(index, { skipTrustPrompt: true });
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		ref: setRootRef,
		className: "h-full min-h-0 overflow-auto bg-editor-surface scrollbar-editor",
		style: {
			fontSize,
			fontFamily: resolveEditorFontFamilyOrInherit(settings)
		},
		onKeyDownCapture: handleNotebookKeyDownCapture,
		onPointerDownCapture: handleNotebookPointerDownCapture,
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "sticky top-0 z-10 flex items-center gap-3 border-b border-border/60 bg-background/95 px-4 py-2 text-xs text-muted-foreground backdrop-blur",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "font-medium text-foreground",
						children: filePath.split(/[/\\]/).pop()
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", { children: [
						notebook.cells.length,
						" ",
						translate("auto.components.editor.IpynbViewer.07e7d96612", "cells")
					] }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: notebook.language }),
					notebook.kernelName ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: notebook.kernelName }) : null,
					runError ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "text-destructive",
						children: runError
					}) : null,
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "ml-auto flex items-center gap-2",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(NotebookHeaderButton, {
								label: translate("auto.components.editor.IpynbViewer.15ec40a735", "Save notebook"),
								shortcut: saveShortcut,
								onClick: () => void saveNotebook(),
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Save, { className: "size-3.5" })
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "rounded-sm border border-border bg-muted px-1.5 py-0.5 font-medium text-muted-foreground",
								children: translate("auto.components.editor.IpynbViewer.329764e9fc", "BETA")
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
								className: "font-mono",
								children: [
									translate("auto.components.editor.IpynbViewer.8c3b21369a", "nbformat"),
									" ",
									notebook.nbformat
								]
							})
						]
					})
				]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "mx-auto flex max-w-[980px] flex-col gap-3 px-5 py-5",
				children: notebook.cells.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "flex items-center justify-center rounded-md border border-border bg-background p-8 text-sm text-muted-foreground",
					children: translate("auto.components.editor.IpynbViewer.d6f37a640b", "Empty notebook")
				}) : notebook.cells.map((cell, index) => {
					const cellKey = getCellKey(cell, index);
					const source = hasOwnDraft(sourceDrafts, cellKey) ? sourceDrafts[cellKey] ?? "" : cell.source;
					return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", {
						className: "overflow-hidden rounded-md border border-border bg-background",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(NotebookCellHeader, {
								cell,
								index,
								running: runningCellIndex === index,
								canMoveUp: index > 0,
								canMoveDown: index < notebook.cells.length - 1,
								onRun: () => void runCell(index),
								onKindChange: (kind) => updateCellKind(index, kind),
								onInsertAbove: (kind) => insertCell(index, kind),
								onInsertBelow: (kind) => insertCell(index + 1, kind),
								onMoveUp: () => moveCell(index, -1),
								onMoveDown: () => moveCell(index, 1),
								onDelete: () => deleteCell(index)
							}),
							cell.kind === "markdown" ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "grid gap-0 lg:grid-cols-2",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(EditableTextCell, {
									source,
									onChange: (nextSource) => updateCellSource(index, nextSource)
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "border-t border-border/50 lg:border-l lg:border-t-0",
									children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MarkdownCell, { source })
								})]
							}) : cell.kind === "code" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MemoizedCodeCell, {
								cell,
								source,
								active: editingCellKey === cellKey,
								onActivate: () => setEditingCellKey(cellKey),
								onDeactivate: () => setEditingCellKey((current) => current === cellKey ? null : current),
								onChange: (nextSource) => updateCellSource(index, nextSource),
								onSaveRequest: saveNotebook
							}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(EditableTextCell, {
								source,
								onChange: (nextSource) => updateCellSource(index, nextSource)
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CellOutputs, { cell })
						]
					}, cellKey);
				})
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
				open: pendingRunCellIndex !== null,
				onOpenChange: (open) => {
					if (!open) cancelPendingRun();
				},
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogContent, {
					className: "max-w-md sm:max-w-md",
					showCloseButton: false,
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, {
						className: "text-sm",
						children: translate("auto.components.editor.IpynbViewer.9e06ae5d36", "Run Notebook Code?")
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, {
						className: "text-xs",
						children: translate("auto.components.editor.IpynbViewer.10ed04a685", "Notebook cells execute local Python on this machine from the notebook folder. Only run cells from files you trust.")
					})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogFooter, {
						className: "gap-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							type: "button",
							variant: "outline",
							size: "sm",
							onClick: cancelPendingRun,
							children: translate("auto.components.editor.IpynbViewer.7f0d7077c6", "Cancel")
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							type: "button",
							size: "sm",
							autoFocus: true,
							onClick: confirmPendingRun,
							children: translate("auto.components.editor.IpynbViewer.859bf9fc21", "Run cell")
						})]
					})]
				})
			})
		]
	});
}
export { IpynbViewer as default };
