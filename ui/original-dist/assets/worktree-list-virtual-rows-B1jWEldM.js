import { et as PINNED_GROUP_KEY, rt as getLineageGroupKey } from "./worktree-activation-BDsaiyMf.js";
import { n as defaultRangeExtractor } from "./esm-DQfOTgcy.js";
const GROUP_HEADER_ROW_HEIGHT = 28;
const WORKTREE_SIDEBAR_VIRTUAL_ROW_GAP = 6;
var SECONDARY_GROUP_HEADER_TOP_MARGIN = 4;
var IMPORTED_WORKTREES_LINE_ROW_HEIGHT = 36;
var PENDING_CREATION_ROW_HEIGHT = 56;
var FOLDER_WORKSPACE_ROW_HEIGHT = 64;
function getRenderRowKey(row) {
	if (row.type === "host-header") return `host:${row.hostId}`;
	if (row.type === "header") return row.hostId ? `hdr:${row.hostId}:${row.key}` : `hdr:${row.key}`;
	if (row.type === "lineage-group") return `lineage-group:${row.key}`;
	if (row.type === "imported-worktrees-card") return `imported:${row.key}`;
	if (row.type === "new-external-worktrees-inbox") return `inbox:${row.key}`;
	if (row.type === "pending-creation") return `pending:${row.creationId}`;
	if (row.type === "folder-workspace") return `folder-workspace:${row.folderWorkspace.id}`;
	return `wt:${row.rowKey}`;
}
function buildLineageRowRekeyMap(rows) {
	const rekeyed = /* @__PURE__ */ new Map();
	for (const row of rows) {
		if (row.type === "lineage-group") {
			const groupKey = getRenderRowKey(row);
			for (const member of row.rows) rekeyed.set(`wt:${member.rowKey}`, groupKey);
			continue;
		}
		if (row.type !== "item") continue;
		rekeyed.set(`lineage-group:${row.sectionKey}:${getLineageGroupKey(row.worktree.id)}`, getRenderRowKey(row));
	}
	return rekeyed;
}
function shouldUseHeaderTopSpacing(args) {
	const previousRenderRow = args.rows[args.index - 1];
	const followsCollapsedPinnedHeader = previousRenderRow?.type === "header" && previousRenderRow.key === "pinned";
	return args.index !== args.firstHeaderIndex && !followsCollapsedPinnedHeader;
}
function estimateRenderRowSize(rows, index, firstHeaderIndex, _activeStickyHeaderIndex) {
	const row = rows[index];
	if (row?.type === "host-header") return 32 + (shouldUseHeaderTopSpacing({
		rows,
		index,
		firstHeaderIndex
	}) ? SECONDARY_GROUP_HEADER_TOP_MARGIN : 0);
	if (row?.type === "header") return 28 + (shouldUseHeaderTopSpacing({
		rows,
		index,
		firstHeaderIndex
	}) ? SECONDARY_GROUP_HEADER_TOP_MARGIN : 0);
	if (row?.type === "lineage-group") return 100 + Math.max(0, row.rows.length - 1) * 96;
	if (row?.type === "imported-worktrees-card" || row?.type === "new-external-worktrees-inbox") return IMPORTED_WORKTREES_LINE_ROW_HEIGHT;
	if (row?.type === "pending-creation") return PENDING_CREATION_ROW_HEIGHT;
	if (row?.type === "folder-workspace") return FOLDER_WORKSPACE_ROW_HEIGHT;
	return 116;
}
function getVirtualRowTransform(start) {
	return `translateY(${start}px)`;
}
function pruneStaleVirtualRowElementCache({ activeRowKeys, virtualizer }) {
	virtualizer.measureElement(null);
	for (const [key, element] of virtualizer.elementsCache) {
		const rowKey = String(key);
		if (activeRowKeys.has(rowKey) || element.isConnected) continue;
		virtualizer.elementsCache.delete(key);
	}
}
function getStickyHeaderIndexes(rows) {
	const indexes = [];
	rows.forEach((row, index) => {
		if (row.type === "host-header" || row.type === "header" && (row.projectGroupDepth ?? 0) === 0) indexes.push(index);
	});
	return indexes;
}
function getHostStickyIndexes(rows, sticky) {
	return sticky.filter((index) => rows[index]?.type === "host-header");
}
function getActiveStickyIndexesForScroll(args) {
	const hostIndexes = getHostStickyIndexes(args.rows, args.stickyHeaderIndexes);
	const resolveWithHandoff = (candidates, pinnedOffset, fallbackToCandidate) => {
		const candidateIndex = getActiveStickyHeaderIndex(candidates, args.rangeStartIndex);
		if (candidateIndex === null) return null;
		const candidate = args.virtualItems.find((item) => item.index === candidateIndex);
		if (!candidate) {
			const previous$1 = getPreviousStickyHeaderIndex(candidates, candidateIndex);
			if (previous$1 !== null) {
				if (args.virtualItems.find((item) => item.index === previous$1)) return previous$1;
			}
			return fallbackToCandidate ? candidateIndex : null;
		}
		if (args.scrollOffset + pinnedOffset >= candidate.start) return candidateIndex;
		const previous = getPreviousStickyHeaderIndex(candidates, candidateIndex);
		if (previous !== null) return previous;
		return fallbackToCandidate ? candidateIndex : null;
	};
	const hostIndex = resolveWithHandoff(hostIndexes, 0, true);
	const hostPosition = hostIndex === null ? -1 : hostIndexes.indexOf(hostIndex);
	const nextHostIndex = hostPosition >= 0 ? hostIndexes[hostPosition + 1] ?? Number.POSITIVE_INFINITY : null;
	return {
		hostIndex,
		groupIndex: resolveWithHandoff(args.stickyHeaderIndexes.filter((index) => {
			if (args.rows[index]?.type !== "header") return false;
			if (hostIndex !== null) return index > hostIndex && index < (nextHostIndex ?? Number.POSITIVE_INFINITY);
			return true;
		}), hostIndex !== null ? 36 : 0, hostIndex === null)
	};
}
function getActiveStickyHeaderIndex(stickyHeaderIndexes, rangeStartIndex) {
	for (let index = stickyHeaderIndexes.length - 1; index >= 0; index--) {
		const headerIndex = stickyHeaderIndexes[index];
		if (headerIndex <= rangeStartIndex) return headerIndex;
	}
	return null;
}
function getPreviousStickyHeaderIndex(stickyHeaderIndexes, headerIndex) {
	const currentPosition = stickyHeaderIndexes.indexOf(headerIndex);
	if (currentPosition <= 0) return null;
	return stickyHeaderIndexes[currentPosition - 1] ?? null;
}
function extractWorktreeVirtualRowIndexes(args) {
	const activeStickyHeaderIndex = getActiveStickyHeaderIndex(args.stickyHeaderIndexes, args.range.startIndex);
	if (activeStickyHeaderIndex === null) return defaultRangeExtractor(args.range);
	const previousStickyHeaderIndex = getPreviousStickyHeaderIndex(args.stickyHeaderIndexes, activeStickyHeaderIndex);
	const activeHostIndex = getActiveStickyHeaderIndex(args.rows ? getHostStickyIndexes(args.rows, args.stickyHeaderIndexes) : [], args.range.startIndex);
	return Array.from(new Set([
		activeStickyHeaderIndex,
		...previousStickyHeaderIndex === null ? [] : [previousStickyHeaderIndex],
		...activeHostIndex === null ? [] : [activeHostIndex],
		...defaultRangeExtractor(args.range)
	])).sort((a, b) => a - b);
}
function getActiveStickyHeaderIndexForScroll(args) {
	const candidateIndex = getActiveStickyHeaderIndex(args.stickyHeaderIndexes, args.rangeStartIndex);
	if (candidateIndex === null) return null;
	const candidate = args.virtualItems.find((item) => item.index === candidateIndex);
	if (!candidate) return candidateIndex;
	if (args.scrollOffset >= candidate.start) return candidateIndex;
	return getPreviousStickyHeaderIndex(args.stickyHeaderIndexes, candidateIndex) ?? candidateIndex;
}
export { extractWorktreeVirtualRowIndexes as a, getActiveStickyIndexesForScroll as c, getStickyHeaderIndexes as d, getVirtualRowTransform as f, estimateRenderRowSize as i, getPreviousStickyHeaderIndex as l, shouldUseHeaderTopSpacing as m, WORKTREE_SIDEBAR_VIRTUAL_ROW_GAP as n, getActiveStickyHeaderIndex as o, pruneStaleVirtualRowElementCache as p, buildLineageRowRekeyMap as r, getActiveStickyHeaderIndexForScroll as s, GROUP_HEADER_ROW_HEIGHT as t, getRenderRowKey as u };
