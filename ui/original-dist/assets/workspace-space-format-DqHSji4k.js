import { t as formatUiRelativeTime } from "./relative-time-format-BdBnutwN.js";
var BYTE_UNITS = [
	"B",
	"KB",
	"MB",
	"GB",
	"TB",
	"PB"
];
var fullDateTimeFormatter = new Intl.DateTimeFormat(void 0, {
	dateStyle: "medium",
	timeStyle: "short"
});
function formatBytes(bytes) {
	if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
	let value = bytes;
	let unitIndex = 0;
	while (value >= 1024 && unitIndex < BYTE_UNITS.length - 1) {
		value /= 1024;
		unitIndex += 1;
	}
	const precision = value >= 100 || unitIndex === 0 ? 0 : value >= 10 ? 1 : 2;
	return `${value.toFixed(precision)} ${BYTE_UNITS[unitIndex]}`;
}
function formatCompactCount(count) {
	if (!Number.isFinite(count) || count <= 0) return "0";
	if (count < 1e3) return String(count);
	if (count < 1e6) return `${(count / 1e3).toFixed(count >= 1e4 ? 0 : 1)}k`;
	return `${(count / 1e6).toFixed(count >= 1e7 ? 0 : 1)}m`;
}
function getWorkspaceSpaceScanTimeLabel(scannedAt, now = Date.now()) {
	return formatUiRelativeTime(scannedAt - now);
}
function getWorkspaceSpaceScanDateTimeLabel(scannedAt) {
	return fullDateTimeFormatter.format(new Date(scannedAt));
}
function getWorkspaceSpaceProgressLabel(progress) {
	if (!progress) return null;
	if (progress.state === "cancelling") return "Cancelling scan";
	const current = progress.currentWorktreeDisplayName ?? progress.currentRepoDisplayName ?? "workspaces";
	if (progress.totalWorktreeCount > 0) return `Scanning ${progress.scannedWorktreeCount} of ${progress.totalWorktreeCount} · ${current}`;
	if (progress.totalRepoCount > 0) return `Scanning ${progress.scannedRepoCount} of ${progress.totalRepoCount} repos · ${current}`;
	return "Scanning workspace sizes";
}
function getWorkspaceSpaceStatusLabel(status) {
	switch (status) {
		case "ok": return "Scanned";
		case "missing": return "Missing";
		case "permission-denied": return "No access";
		case "unavailable": return "Unavailable";
		case "error": return "Failed";
	}
}
function getWorkspaceSpaceBranchLabel(worktree) {
	return worktree.branch.replace(/^refs\/heads\//, "").trim() || (worktree.isMainWorktree ? "main worktree" : "detached");
}
export { getWorkspaceSpaceScanDateTimeLabel as a, getWorkspaceSpaceProgressLabel as i, formatCompactCount as n, getWorkspaceSpaceScanTimeLabel as o, getWorkspaceSpaceBranchLabel as r, getWorkspaceSpaceStatusLabel as s, formatBytes as t };
