import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate } from "./jsx-runtime-Cv_nyRjc.js";
import { l as createLucideIcon } from "./button-DszXJEV6.js";
import { t as CircleAlert } from "./circle-alert-keRTpMg-.js";
import { t as CircleDashed } from "./circle-dashed-Df2rqHLJ.js";
import { t as Circle } from "./circle-DumnR8X3.js";
import { fg as DEFAULT_WORKSPACE_STATUS_COLOR_ID, pg as DEFAULT_WORKSPACE_STATUS_ICON_ID, to as measureClipboardTextByteLength } from "./store-CgXrfmaH.js";
import { t as createLocalizedCatalog } from "./localized-catalog-DubKHKUR.js";
var Ban = createLucideIcon("ban", [["circle", {
	cx: "12",
	cy: "12",
	r: "10",
	key: "1mglay"
}], ["path", {
	d: "M4.929 4.929 19.07 19.071",
	key: "196cmz"
}]]);
var CircleDot = createLucideIcon("circle-dot", [["circle", {
	cx: "12",
	cy: "12",
	r: "10",
	key: "1mglay"
}], ["circle", {
	cx: "12",
	cy: "12",
	r: "1",
	key: "41hilf"
}]]);
var CircleEllipsis = createLucideIcon("circle-ellipsis", [
	["circle", {
		cx: "12",
		cy: "12",
		r: "10",
		key: "1mglay"
	}],
	["path", {
		d: "M17 12h.01",
		key: "1m0b6t"
	}],
	["path", {
		d: "M12 12h.01",
		key: "1mp3jc"
	}],
	["path", {
		d: "M7 12h.01",
		key: "eqddd0"
	}]
]);
var CirclePause = createLucideIcon("circle-pause", [
	["circle", {
		cx: "12",
		cy: "12",
		r: "10",
		key: "1mglay"
	}],
	["line", {
		x1: "10",
		x2: "10",
		y1: "15",
		y2: "9",
		key: "c1nkhi"
	}],
	["line", {
		x1: "14",
		x2: "14",
		y1: "15",
		y2: "9",
		key: "h65svq"
	}]
]);
var CirclePlay = createLucideIcon("circle-play", [["path", {
	d: "M9 9.003a1 1 0 0 1 1.517-.859l4.997 2.997a1 1 0 0 1 0 1.718l-4.997 2.997A1 1 0 0 1 9 14.996z",
	key: "kmsa83"
}], ["circle", {
	cx: "12",
	cy: "12",
	r: "10",
	key: "1mglay"
}]]);
var Flag = createLucideIcon("flag", [["path", {
	d: "M4 22V4a1 1 0 0 1 .4-.8A6 6 0 0 1 8 2c3 0 5 2 7.333 2q2 0 3.067-.8A1 1 0 0 1 20 4v10a1 1 0 0 1-.4.8A6 6 0 0 1 16 16c-3 0-5-2-8-2a6 6 0 0 0-4 1.528",
	key: "1jaruq"
}]]);
var Timer = createLucideIcon("timer", [
	["line", {
		x1: "10",
		x2: "14",
		y1: "2",
		y2: "2",
		key: "14vaq8"
	}],
	["line", {
		x1: "12",
		x2: "15",
		y1: "14",
		y2: "11",
		key: "17fdiu"
	}],
	["circle", {
		cx: "12",
		cy: "14",
		r: "8",
		key: "1e1u0o"
	}]
]);
var import_react = /* @__PURE__ */ __toESM(require_react());
function ConductorDoneIcon({ className }) {
	return import_react.createElement("svg", {
		className,
		viewBox: "0 0 12 12",
		fill: "none",
		"aria-hidden": true
	}, import_react.createElement("circle", {
		cx: 6,
		cy: 6,
		r: 5.1,
		fill: "currentColor"
	}), import_react.createElement("path", {
		d: "M4 6.05 5.25 7.25 8.05 4.7",
		stroke: "white",
		strokeWidth: 1.25,
		strokeLinecap: "round",
		strokeLinejoin: "round"
	}));
}
function ConductorReviewIcon({ className }) {
	return import_react.createElement("svg", {
		className,
		viewBox: "0 0 12 12",
		fill: "none",
		"aria-hidden": true
	}, import_react.createElement("circle", {
		cx: 6,
		cy: 6,
		r: 4.9,
		fill: "var(--background)",
		stroke: "currentColor",
		strokeWidth: 1.45
	}), import_react.createElement("path", {
		d: "M4.15 6.05 5.25 7.05 7.7 4.75",
		stroke: "currentColor",
		strokeWidth: 1.2,
		strokeLinecap: "round",
		strokeLinejoin: "round"
	}));
}
function ConductorProgressIcon({ className }) {
	return import_react.createElement("svg", {
		className,
		viewBox: "0 0 12 12",
		fill: "none",
		"aria-hidden": true
	}, import_react.createElement("circle", {
		cx: 6,
		cy: 6,
		r: 4.9,
		fill: "var(--background)",
		stroke: "currentColor",
		strokeWidth: 1.45
	}), import_react.createElement("path", {
		d: "M6 3.75v2.7",
		stroke: "currentColor",
		strokeWidth: 1.25,
		strokeLinecap: "round"
	}));
}
const getWorkspaceStatusIconOptions = createLocalizedCatalog(() => [
	{
		id: "circle",
		label: translate("auto.components.sidebar.workspace.status.b4a7101fe1", "Circle"),
		icon: Circle
	},
	{
		id: "circle-dot",
		label: translate("auto.components.sidebar.workspace.status.a702bc08d4", "Dot"),
		icon: CircleDot
	},
	{
		id: "circle-progress",
		label: translate("auto.components.sidebar.workspace.status.226d1e7773", "Progress"),
		icon: ConductorProgressIcon
	},
	{
		id: "circle-dashed",
		label: translate("auto.components.sidebar.workspace.status.821d156f54", "Dashed"),
		icon: CircleDashed
	},
	{
		id: "circle-ellipsis",
		label: translate("auto.components.sidebar.workspace.status.5f9ca31a84", "Waiting"),
		icon: CircleEllipsis
	},
	{
		id: "git-pull-request",
		label: translate("auto.components.sidebar.workspace.status.409528031f", "Review"),
		icon: ConductorReviewIcon
	},
	{
		id: "timer",
		label: translate("auto.components.sidebar.workspace.status.251c817bdd", "Timer"),
		icon: Timer
	},
	{
		id: "flag",
		label: translate("auto.components.sidebar.workspace.status.6380517b10", "Flag"),
		icon: Flag
	},
	{
		id: "circle-alert",
		label: translate("auto.components.sidebar.workspace.status.642da473f2", "Alert"),
		icon: CircleAlert
	},
	{
		id: "circle-pause",
		label: translate("auto.components.sidebar.workspace.status.111db162bf", "Paused"),
		icon: CirclePause
	},
	{
		id: "circle-play",
		label: translate("auto.components.sidebar.workspace.status.2c19d1db33", "Play"),
		icon: CirclePlay
	},
	{
		id: "circle-check",
		label: translate("auto.components.sidebar.workspace.status.6b8285b8dd", "Done"),
		icon: ConductorDoneIcon
	},
	{
		id: "ban",
		label: translate("auto.components.sidebar.workspace.status.93ac840dcb", "Blocked"),
		icon: Ban
	},
	{
		id: "conductor-done",
		label: translate("auto.components.sidebar.workspace.status.6b8285b8dd", "Done"),
		icon: ConductorDoneIcon
	},
	{
		id: "conductor-review",
		label: translate("auto.components.sidebar.workspace.status.6c1efa2cf8", "In review"),
		icon: ConductorReviewIcon
	},
	{
		id: "conductor-progress",
		label: translate("auto.components.sidebar.workspace.status.cb387159f6", "In progress"),
		icon: ConductorProgressIcon
	}
]);
const WORKSPACE_STATUS_DRAG_TYPE = "application/x-orca-worktree-id";
const WORKSPACE_STATUS_DRAG_IDS_TYPE = "application/x-orca-worktree-ids";
function writeWorkspaceDragData(dataTransfer, worktreeIdOrIds) {
	const worktreeIds = Array.isArray(worktreeIdOrIds) ? worktreeIdOrIds : [worktreeIdOrIds];
	const [firstWorktreeId] = worktreeIds;
	if (!firstWorktreeId) return;
	dataTransfer.effectAllowed = "move";
	dataTransfer.setData(WORKSPACE_STATUS_DRAG_TYPE, firstWorktreeId);
	dataTransfer.setData(WORKSPACE_STATUS_DRAG_IDS_TYPE, JSON.stringify(worktreeIds));
	dataTransfer.setData("text/plain", firstWorktreeId);
}
function readWorkspaceDragData(dataTransfer) {
	const typed = readWorkspaceStatusDragPayload(dataTransfer, WORKSPACE_STATUS_DRAG_TYPE);
	if (typed.status === "ok") return typed.value;
	if (typed.status === "too-large") return null;
	const plain = readWorkspaceStatusDragPayload(dataTransfer, "text/plain");
	if (plain.status === "ok") return plain.value;
	return null;
}
function readWorkspaceDragDataIds(dataTransfer) {
	const rawIds = readWorkspaceStatusDragPayload(dataTransfer, WORKSPACE_STATUS_DRAG_IDS_TYPE);
	if (rawIds.status === "too-large") return [];
	if (rawIds.status === "ok") try {
		const parsed = JSON.parse(rawIds.value);
		if (Array.isArray(parsed)) return collectWorkspaceStatusDragIds(parsed) ?? [];
	} catch {}
	const singleId = readWorkspaceDragData(dataTransfer);
	return singleId ? [singleId] : [];
}
function collectWorkspaceStatusDragIds(values) {
	const ids = [];
	for (const value of values) {
		if (typeof value !== "string" || value.length === 0) continue;
		if (ids.length >= 512) return null;
		ids.push(value);
	}
	return ids;
}
function hasWorkspaceDragData(dataTransfer) {
	const types = Array.from(dataTransfer.types);
	return hasBoundedWorkspaceStatusDragPayload(dataTransfer, types, "application/x-orca-worktree-ids") || hasBoundedWorkspaceStatusDragPayload(dataTransfer, types, "application/x-orca-worktree-id") || hasBoundedWorkspaceStatusDragPayload(dataTransfer, types, "text/plain");
}
function readWorkspaceStatusDragPayload(dataTransfer, type) {
	const value = dataTransfer.getData(type);
	if (!value) return { status: "empty" };
	if (value.length > 16384 || measureClipboardTextByteLength(value, { stopAfterBytes: 16384 }).exceededLimit) return { status: "too-large" };
	return {
		status: "ok",
		value
	};
}
function hasBoundedWorkspaceStatusDragPayload(dataTransfer, types, type) {
	return types.includes(type) && readWorkspaceStatusDragPayload(dataTransfer, type).status === "ok";
}
const getWorkspaceStatusColorOptions = createLocalizedCatalog(() => [
	{
		id: "neutral",
		label: translate("auto.components.sidebar.workspace.status.52e3c6e2a4", "Neutral"),
		tone: "text-muted-foreground",
		swatch: "bg-muted-foreground",
		border: "border-t-muted-foreground/45",
		laneTint: "bg-background/55"
	},
	{
		id: "blue",
		label: translate("auto.components.sidebar.workspace.status.fc3b92756c", "Blue"),
		tone: "text-blue-600 dark:text-blue-300",
		swatch: "bg-blue-500",
		border: "border-t-blue-500/70",
		laneTint: "bg-blue-500/[0.04]"
	},
	{
		id: "sky",
		label: translate("auto.components.sidebar.workspace.status.6437a8c253", "Sky"),
		tone: "text-sky-600 dark:text-sky-300",
		swatch: "bg-sky-500",
		border: "border-t-sky-500/70",
		laneTint: "bg-sky-500/[0.04]"
	},
	{
		id: "violet",
		label: translate("auto.components.sidebar.workspace.status.1b81da243a", "Violet"),
		tone: "text-violet-600 dark:text-violet-300",
		swatch: "bg-violet-500",
		border: "border-t-violet-500/70",
		laneTint: "bg-violet-500/[0.04]"
	},
	{
		id: "amber",
		label: translate("auto.components.sidebar.workspace.status.7cebab6d4a", "Amber"),
		tone: "text-amber-700 dark:text-amber-200",
		swatch: "bg-amber-500",
		border: "border-t-amber-500/70",
		laneTint: "bg-amber-500/[0.04]"
	},
	{
		id: "emerald",
		label: translate("auto.components.sidebar.workspace.status.ddf25b6262", "Emerald"),
		tone: "text-emerald-700 dark:text-emerald-200",
		swatch: "bg-emerald-500",
		border: "border-t-emerald-500/70",
		laneTint: "bg-emerald-500/[0.04]"
	},
	{
		id: "rose",
		label: translate("auto.components.sidebar.workspace.status.7adb43ecf0", "Rose"),
		tone: "text-rose-600 dark:text-rose-300",
		swatch: "bg-rose-500",
		border: "border-t-rose-500/70",
		laneTint: "bg-rose-500/[0.04]"
	},
	{
		id: "zinc",
		label: translate("auto.components.sidebar.workspace.status.caabd5ca85", "Zinc"),
		tone: "text-zinc-600 dark:text-zinc-300",
		swatch: "bg-zinc-500",
		border: "border-t-zinc-500/70",
		laneTint: "bg-zinc-500/[0.04]"
	},
	{
		id: "conductor-done",
		label: translate("auto.components.sidebar.workspace.status.895f381714", "Conductor Done"),
		tone: "text-[#c7a594]",
		swatch: "bg-[#c7a594]",
		border: "border-t-[#c7a594]/70",
		laneTint: "bg-[#c7a594]/[0.04]"
	},
	{
		id: "conductor-review",
		label: translate("auto.components.sidebar.workspace.status.caebe3c10f", "Conductor Review"),
		tone: "text-[#16a34a]",
		swatch: "bg-[#16a34a]",
		border: "border-t-[#16a34a]/70",
		laneTint: "bg-[#16a34a]/[0.04]"
	},
	{
		id: "conductor-progress",
		label: translate("auto.components.sidebar.workspace.status.1a9383112b", "Conductor Progress"),
		tone: "text-[#d4a300]",
		swatch: "bg-[#d4a300]",
		border: "border-t-[#d4a300]/70",
		laneTint: "bg-[#d4a300]/[0.04]"
	}
]);
function getFallbackColorOption() {
	return getWorkspaceStatusColorOptions()[0] ?? {
		id: "neutral",
		label: translate("auto.components.sidebar.workspace.status.52e3c6e2a4", "Neutral"),
		tone: "text-muted-foreground",
		swatch: "bg-muted-foreground",
		border: "border-t-muted-foreground/45",
		laneTint: "bg-background/55"
	};
}
function getFallbackIconOption() {
	return getWorkspaceStatusIconOptions()[1] ?? {
		id: "circle-dot",
		label: translate("auto.components.sidebar.workspace.status.a702bc08d4", "Dot"),
		icon: CircleDot
	};
}
var DEFAULT_STATUS_VISUALS = {
	todo: {
		color: "neutral",
		icon: "circle"
	},
	"in-progress": {
		color: "conductor-progress",
		icon: "conductor-progress"
	},
	"in-review": {
		color: "conductor-review",
		icon: "conductor-review"
	},
	completed: {
		color: "conductor-done",
		icon: "conductor-done"
	}
};
function getWorkspaceStatusVisualMeta(status) {
	const statusId = typeof status === "string" ? status : status.id;
	const visual = typeof status === "string" ? DEFAULT_STATUS_VISUALS[status] : status;
	const colorId = visual?.color ?? DEFAULT_STATUS_VISUALS[statusId]?.color;
	const iconId = visual?.icon ?? DEFAULT_STATUS_VISUALS[statusId]?.icon;
	const color = getWorkspaceStatusColorOptions().find((option) => option.id === colorId) ?? getWorkspaceStatusColorOptions().find((option) => option.id === "neutral") ?? getFallbackColorOption();
	const iconOptions = getWorkspaceStatusIconOptions();
	const icon = iconOptions.find((option) => option.id === iconId) ?? iconOptions.find((option) => option.id === "circle-dot") ?? getFallbackIconOption();
	return {
		tone: color.tone,
		swatch: color.swatch,
		border: color.border,
		laneTint: color.laneTint,
		icon: icon.icon
	};
}
export { writeWorkspaceDragData as a, ConductorProgressIcon as c, Flag as d, CirclePlay as f, readWorkspaceDragDataIds as i, ConductorReviewIcon as l, Ban as m, getWorkspaceStatusVisualMeta as n, getWorkspaceStatusIconOptions as o, CircleDot as p, hasWorkspaceDragData as r, ConductorDoneIcon as s, getWorkspaceStatusColorOptions as t, Timer as u };
