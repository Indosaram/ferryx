import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import "./react-dom-Da8MQai-.js";
import { t as useVirtualizer } from "./esm-DQfOTgcy.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
const CSV_DELIMITER_SNIFF_SCAN_CODE_UNITS = 64 * 1024;
var LINE_FEED_CODE_UNIT = 10;
var CARRIAGE_RETURN_CODE_UNIT = 13;
function parseCsv(source, delimiter = ",") {
	if (source.charCodeAt(0) === 65279) source = source.slice(1);
	const rows = [];
	let row = [];
	let field = "";
	let inQuotes = false;
	let maxColumns = 0;
	let recordHasContent = false;
	const pushField = () => {
		row.push(field);
		field = "";
	};
	const pushRow = () => {
		pushField();
		if (row.length > maxColumns) maxColumns = row.length;
		rows.push(row);
		row = [];
		recordHasContent = false;
	};
	for (let i = 0; i < source.length; i += 1) {
		const ch = source[i];
		if (inQuotes) {
			if (ch === "\"") if (source[i + 1] === "\"") {
				field += "\"";
				i += 1;
			} else inQuotes = false;
			else field += ch;
			continue;
		}
		if (ch === "\"" && field === "") {
			inQuotes = true;
			recordHasContent = true;
			continue;
		}
		if (ch === delimiter) {
			pushField();
			recordHasContent = true;
			continue;
		}
		if (ch === "\r") {
			if (source[i + 1] === "\n") i += 1;
			pushRow();
			continue;
		}
		if (ch === "\n") {
			pushRow();
			continue;
		}
		field += ch;
		recordHasContent = true;
	}
	if (field.length > 0 || row.length > 0 || recordHasContent) pushRow();
	return {
		rows,
		maxColumns
	};
}
function detectCsvDelimiter(filePath, content) {
	if (filePath.toLowerCase().endsWith(".tsv")) return "	";
	let text = content;
	if (text.charCodeAt(0) === 65279) text = text.slice(1);
	const firstLine = findFirstNonEmptyCsvSniffLine(text);
	return countDelimiterOutsideQuotes(firstLine, "	") > countDelimiterOutsideQuotes(firstLine, ",") ? "	" : ",";
}
function findFirstNonEmptyCsvSniffLine(text) {
	const scanLength = Math.min(text.length, CSV_DELIMITER_SNIFF_SCAN_CODE_UNITS);
	let lineStart = 0;
	let lineHasContent = false;
	for (let index = 0; index < scanLength; index += 1) {
		const codeUnit = text.charCodeAt(index);
		if (codeUnit === LINE_FEED_CODE_UNIT || codeUnit === CARRIAGE_RETURN_CODE_UNIT) {
			if (lineHasContent) return text.slice(lineStart, index);
			if (codeUnit === CARRIAGE_RETURN_CODE_UNIT && index + 1 < scanLength && text.charCodeAt(index + 1) === LINE_FEED_CODE_UNIT) index += 1;
			lineStart = index + 1;
			lineHasContent = false;
			continue;
		}
		if (!lineHasContent && !isCsvSniffWhitespace(codeUnit)) lineHasContent = true;
	}
	return lineHasContent ? text.slice(lineStart, scanLength) : "";
}
function isCsvSniffWhitespace(codeUnit) {
	return codeUnit === 9 || codeUnit === 11 || codeUnit === 12 || codeUnit === 32 || codeUnit === 160;
}
function countDelimiterOutsideQuotes(line, delimiter) {
	let count = 0;
	let inQuotes = false;
	for (let i = 0; i < line.length; i += 1) {
		const ch = line[i];
		if (ch === "\"") {
			if (inQuotes && line[i + 1] === "\"") i += 1;
			else inQuotes = !inQuotes;
			continue;
		}
		if (!inQuotes && ch === delimiter) count += 1;
	}
	return count;
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var ROW_HEIGHT = 28;
var OVERSCAN = 12;
var MIN_COL_PX = 80;
var MAX_COL_PX = 320;
var ROW_NUMBER_COL_PX = 48;
var CHAR_PX = 7;
function CsvViewer({ content, filePath }) {
	const scrollRef = (0, import_react.useRef)(null);
	const parsed = (0, import_react.useMemo)(() => {
		return parseCsv(content, detectCsvDelimiter(filePath, content));
	}, [content, filePath]);
	const { headerRow, bodyRows } = (0, import_react.useMemo)(() => {
		if (parsed.rows.length === 0) return {
			headerRow: [],
			bodyRows: []
		};
		const [head, ...rest] = parsed.rows;
		return {
			headerRow: head ?? [],
			bodyRows: rest
		};
	}, [parsed]);
	const columnCount = parsed.maxColumns;
	const header = (0, import_react.useMemo)(() => {
		const out = [...headerRow ?? []];
		while (out.length < columnCount) out.push("");
		return out;
	}, [headerRow, columnCount]);
	const columnWidths = (0, import_react.useMemo)(() => {
		const widths = Array.from({ length: columnCount }).fill(MIN_COL_PX);
		const consider = (cell, idx) => {
			if (!cell) return;
			const w = Math.min(MAX_COL_PX, Math.max(MIN_COL_PX, cell.length * CHAR_PX + 24));
			if (w > widths[idx]) widths[idx] = w;
		};
		header.forEach(consider);
		const sampleLimit = Math.min(bodyRows.length, 200);
		for (let i = 0; i < sampleLimit; i += 1) {
			const row = bodyRows[i];
			for (let c = 0; c < columnCount; c += 1) consider(row[c], c);
		}
		return widths;
	}, [
		header,
		bodyRows,
		columnCount
	]);
	const gridTemplate = (0, import_react.useMemo)(() => `${ROW_NUMBER_COL_PX}px ${columnWidths.map((w) => `${w}px`).join(" ")}`, [columnWidths]);
	const virtualizer = useVirtualizer({
		count: bodyRows.length,
		getScrollElement: () => scrollRef.current,
		estimateSize: () => ROW_HEIGHT,
		overscan: OVERSCAN,
		getItemKey: (index) => index
	});
	if (parsed.rows.length === 0) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "flex h-full items-center justify-center text-sm text-muted-foreground",
		children: translate("auto.components.editor.CsvViewer.a233d55b77", "Empty file")
	});
	const virtualRows = virtualizer.getVirtualItems();
	const totalHeight = virtualizer.getTotalSize();
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex h-full min-h-0 flex-col",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			ref: scrollRef,
			className: "relative min-h-0 flex-1 overflow-auto scrollbar-editor font-mono text-xs",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				role: "table",
				"aria-rowcount": parsed.rows.length,
				"aria-colcount": columnCount + 1,
				className: "inline-block min-w-full",
				style: { width: "max-content" },
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					role: "row",
					"aria-rowindex": 1,
					className: "sticky top-0 z-10 grid bg-muted/90 backdrop-blur",
					style: {
						gridTemplateColumns: gridTemplate,
						height: ROW_HEIGHT
					},
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						role: "columnheader",
						className: "sticky left-0 z-20 flex items-center justify-end border-b border-r border-border/60 bg-muted/90 px-2 text-[10px] font-normal text-muted-foreground",
						children: "#"
					}), header.map((cell, idx) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						role: "columnheader",
						className: "flex items-center overflow-hidden border-b border-r border-border/60 px-2 font-medium text-foreground",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "truncate",
							title: cell,
							children: cell
						})
					}, idx))]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					style: {
						height: totalHeight,
						position: "relative"
					},
					children: virtualRows.map((vr) => {
						const row = bodyRows[vr.index] ?? [];
						return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							role: "row",
							"aria-rowindex": vr.index + 2,
							"data-index": vr.index,
							className: "group grid hover:bg-accent/40",
							style: {
								gridTemplateColumns: gridTemplate,
								position: "absolute",
								top: 0,
								left: 0,
								height: ROW_HEIGHT,
								transform: `translateY(${vr.start}px)`
							},
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								role: "rowheader",
								className: "sticky left-0 z-[5] flex items-center justify-end border-b border-r border-border/40 bg-background/95 px-2 text-[10px] text-muted-foreground group-hover:bg-accent/40",
								children: vr.index + 1
							}), Array.from({ length: columnCount }).map((_, colIdx) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								role: "cell",
								className: "flex items-center overflow-hidden border-b border-r border-border/40 px-2 text-foreground",
								title: row[colIdx] ?? "",
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "truncate",
									children: row[colIdx] ?? ""
								})
							}, colIdx))]
						}, vr.key);
					})
				})]
			})
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex items-center gap-4 border-t border-border/60 px-3 py-1 text-xs text-muted-foreground",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", { children: [
				bodyRows.length.toLocaleString(),
				" ",
				translate("auto.components.editor.CsvViewer.ac31d2cd60", "rows")
			] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", { children: [
				columnCount,
				" ",
				translate("auto.components.editor.CsvViewer.eedd0d37a7", "columns")
			] })]
		})]
	});
}
export { CsvViewer as default };
