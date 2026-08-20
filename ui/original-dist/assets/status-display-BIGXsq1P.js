import { Ac as normalizeRelativePath, kc as joinPath } from "./store-CgXrfmaH.js";
import { t as splitPathSegments } from "./path-tree-C_1ToHfK.js";
const STATUS_LABELS = {
	modified: "M",
	added: "A",
	deleted: "D",
	renamed: "R",
	untracked: "U",
	copied: "C"
};
const STATUS_COLORS = {
	modified: "var(--git-decoration-modified)",
	added: "var(--git-decoration-added)",
	deleted: "var(--git-decoration-deleted)",
	renamed: "var(--git-decoration-renamed)",
	untracked: "var(--git-decoration-untracked)",
	copied: "var(--git-decoration-copied)"
};
var STATUS_PRIORITY = {
	deleted: 5,
	modified: 4,
	added: 3,
	untracked: 3,
	renamed: 2,
	copied: 1
};
function getDominantStatus(statuses) {
	let dominantStatus = null;
	let dominantPriority = -1;
	for (const status of statuses) {
		const priority = STATUS_PRIORITY[status];
		if (priority > dominantPriority) {
			dominantStatus = status;
			dominantPriority = priority;
		}
	}
	return dominantStatus;
}
function buildStatusMap(entries) {
	const statusByPath = /* @__PURE__ */ new Map();
	for (const entry of entries) {
		const path = normalizeRelativePath(entry.path);
		const existing = statusByPath.get(path);
		const resolved = existing ? getDominantStatus([existing, entry.status]) ?? entry.status : entry.status;
		statusByPath.set(path, resolved);
	}
	return statusByPath;
}
function buildFolderStatusMap(entries) {
	const folderStatuses = /* @__PURE__ */ new Map();
	for (const entry of entries) {
		if (!shouldPropagateStatus(entry.status)) continue;
		const segments = splitPathSegments(entry.path);
		if (segments.length <= 1) continue;
		let currentPath = "";
		for (const segment of segments.slice(0, -1)) {
			currentPath = currentPath ? joinPath(currentPath, segment) : segment;
			const statuses = folderStatuses.get(currentPath);
			if (statuses) statuses.push(entry.status);
			else folderStatuses.set(currentPath, [entry.status]);
		}
	}
	return new Map(Array.from(folderStatuses.entries()).map(([folderPath, statuses]) => [folderPath, getDominantStatus(statuses)]));
}
function shouldPropagateStatus(status) {
	return status !== "deleted";
}
function isPathIgnored(ignored, relativePath) {
	if (ignored.size === 0) return false;
	if (ignored.has(relativePath)) return true;
	let candidate = relativePath;
	for (;;) {
		const idx = candidate.lastIndexOf("/");
		if (idx <= 0) return false;
		candidate = candidate.slice(0, idx);
		if (ignored.has(candidate)) return true;
	}
}
function shouldShowIgnoredDecoration(nodeStatus, ignored, relativePath) {
	return !nodeStatus && isPathIgnored(ignored, relativePath);
}
function buildIgnoredSet(ignoredPaths) {
	const set = /* @__PURE__ */ new Set();
	if (!ignoredPaths) return set;
	for (const rawPath of ignoredPaths) {
		const trimmed = rawPath.endsWith("/") ? rawPath.slice(0, -1) : rawPath;
		set.add(normalizeRelativePath(trimmed));
	}
	return set;
}
export { buildStatusMap as a, buildIgnoredSet as i, STATUS_LABELS as n, isPathIgnored as o, buildFolderStatusMap as r, shouldShowIgnoredDecoration as s, STATUS_COLORS as t };
