import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { r as Slot } from "./button-DszXJEV6.js";
import { $a as isClipboardTextByteLengthOverLimit } from "./store-CgXrfmaH.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
import { t as compareFileNames } from "./file-name-sort-EN_9pleA.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var CURSOR_TOOLTIP_GAP = 18;
function cursorTooltipOffsets(pointer, trigger) {
	return {
		align: pointer.x - trigger.left,
		side: pointer.y + CURSOR_TOOLTIP_GAP - trigger.bottom
	};
}
function splitTrailingSegment(path) {
	const separatorIndex = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
	return separatorIndex === -1 ? {
		directory: "",
		filename: path
	} : {
		directory: path.slice(0, separatorIndex + 1),
		filename: path.slice(separatorIndex + 1)
	};
}
function FilePathCursorTooltip({ children, path }) {
	const triggerRef = import_react.useRef(null);
	const pointerRef = import_react.useRef(null);
	const [open, setOpen] = import_react.useState(false);
	const [offset, setOffset] = import_react.useState({
		align: 0,
		side: 0
	});
	import_react.useLayoutEffect(() => {
		const rect = triggerRef.current?.getBoundingClientRect();
		const pointer = pointerRef.current;
		if (!open || !rect || !pointer) return;
		const next = cursorTooltipOffsets(pointer, rect);
		setOffset((current) => current.align === next.align && current.side === next.side ? current : next);
	}, [open]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, {
		open,
		onOpenChange: setOpen,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
			asChild: true,
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Slot, {
				ref: triggerRef,
				onPointerMove: (event) => {
					if (open) return;
					pointerRef.current = {
						x: event.clientX,
						y: event.clientY
					};
				},
				children
			})
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
			side: "bottom",
			align: "start",
			sideOffset: offset.side,
			alignOffset: offset.align,
			showArrow: false,
			className: "max-w-[min(90vw,800px)] rounded-md border border-border/80 bg-popover px-2 py-1 text-[11px] leading-[15px] break-words text-popover-foreground shadow-[0_10px_24px_rgba(0,0,0,0.18)]",
			children: path
		})]
	});
}
const QUICK_OPEN_QUERY_MAX_BYTES = 2 * 1024;
function prepareQuickOpenFiles(files) {
	return files.map((path, inputIndex) => {
		const searchPath = path.replace(/\\/g, "/");
		const lastSlash = searchPath.lastIndexOf("/");
		return {
			path,
			lowerPath: searchPath.toLowerCase(),
			lowerFilename: searchPath.slice(lastSlash + 1).toLowerCase(),
			inputIndex
		};
	});
}
var preparedQuickOpenFiles = /* @__PURE__ */ new WeakMap();
function getPreparedQuickOpenFiles(files) {
	const cached = preparedQuickOpenFiles.get(files);
	if (cached) return cached;
	const prepared = prepareQuickOpenFiles(files);
	preparedQuickOpenFiles.set(files, prepared);
	return prepared;
}
function isQuickOpenQueryTooLarge(query, maxBytes = QUICK_OPEN_QUERY_MAX_BYTES) {
	return isClipboardTextByteLengthOverLimit(query, maxBytes);
}
function rankQuickOpenFiles(query, files, limit = 50) {
	if (limit <= 0) return [];
	if (isQuickOpenQueryTooLarge(query)) return [];
	const normalizedQuery = query.trim().replace(/\\/g, "/").toLowerCase();
	if (!normalizedQuery) {
		const results$1 = [];
		for (const file of files) retainTopResult(results$1, {
			path: file.path,
			score: 0,
			inputIndex: file.inputIndex
		}, limit);
		return finalizeResults(results$1);
	}
	const results = [];
	for (const file of files) {
		const score = fuzzyMatchIndexedFile(normalizedQuery, file);
		if (score === -1) continue;
		retainTopResult(results, {
			path: file.path,
			score,
			inputIndex: file.inputIndex
		}, limit);
	}
	return finalizeResults(results);
}
function fuzzyMatchIndexedFile(query, file) {
	let qi = 0;
	let score = 0;
	let lastMatchIdx = -1;
	for (let ti = 0; ti < file.lowerPath.length && qi < query.length; ti++) if (file.lowerPath[ti] === query[qi]) {
		const gap = lastMatchIdx === -1 ? 0 : ti - lastMatchIdx - 1;
		score += gap;
		if (ti > 0 && (file.lowerPath[ti - 1] === "/" || file.lowerPath[ti - 1] === "." || file.lowerPath[ti - 1] === "-")) score -= 5;
		lastMatchIdx = ti;
		qi++;
	}
	if (qi < query.length) return -1;
	if (file.lowerFilename.includes(query)) score -= 100;
	return score;
}
function retainTopResult(heap, candidate, limit) {
	if (heap.length === limit && compareRankedResult(candidate, heap[0]) >= 0) return;
	if (heap.length < limit) {
		heap.push(candidate);
		siftResultUp(heap, heap.length - 1);
		return;
	}
	heap[0] = candidate;
	siftResultDown(heap);
}
function siftResultUp(heap, startIndex) {
	let index = startIndex;
	while (index > 0) {
		const parentIndex = Math.floor((index - 1) / 2);
		if (compareRankedResult(heap[index], heap[parentIndex]) <= 0) return;
		[heap[index], heap[parentIndex]] = [heap[parentIndex], heap[index]];
		index = parentIndex;
	}
}
function siftResultDown(heap) {
	let index = 0;
	while (true) {
		const leftIndex = index * 2 + 1;
		if (leftIndex >= heap.length) return;
		const rightIndex = leftIndex + 1;
		const worseChildIndex = rightIndex < heap.length && compareRankedResult(heap[rightIndex], heap[leftIndex]) > 0 ? rightIndex : leftIndex;
		if (compareRankedResult(heap[worseChildIndex], heap[index]) <= 0) return;
		[heap[index], heap[worseChildIndex]] = [heap[worseChildIndex], heap[index]];
		index = worseChildIndex;
	}
}
function finalizeResults(results) {
	return results.sort(compareRankedResult).map(({ path, score }) => ({
		path,
		score
	}));
}
function compareRankedResult(a, b) {
	return a.score - b.score || compareFileNames(a.path, b.path) || a.inputIndex - b.inputIndex;
}
export { FilePathCursorTooltip as a, rankQuickOpenFiles as i, isQuickOpenQueryTooLarge as n, splitTrailingSegment as o, prepareQuickOpenFiles as r, getPreparedQuickOpenFiles as t };
