const __vite__mapDeps=(i,m=__vite__mapDeps,d=(m.f||(m.f=["./AgentMapProjectContextMenu-DBIl0DQp.js","./button-DszXJEV6.js","./jsx-runtime-Cv_nyRjc.js","./preload-helper-Cgw39-ka.js","./chunk-Dhmk_5SA.js","./react-Da2TLWQy.js","./context-menu-D4RKI7hR.js","./dist-CUdeCwrc.js","./dist-Ca8cIakR.js","./classPrivateFieldGet2-CvaeS1Sp.js","./dist-BsNIAh1s.js","./floating-ui.dom-i2UEqmZo.js","./dist-BvH-oDES.js","./dist-DGfr86jh.js","./dist-DW1EJH6e.js","./react-dom-Da8MQai-.js","./dist-B1f0G6s_.js","./dist-G_cmV6EA.js","./es2015-B5WZ-7WO.js","./chevron-right-CZtMe6Ev.js","./circle-DumnR8X3.js","./plus-Db0kWPVa.js","./store-CgXrfmaH.js","./defineProperty-BAtR-r70.js","./dist-DgqligFk.js","./plugin-manifest-Bs-50M_g.js","./useMountedRef-1omUd-IV.js","./agent-status-3vUKbY6l.js","./agent-kind-Dfx6MnkP.js","./telemetry-ZyUPyKMD.js","./repo-header-create-state-CAuZRBON.js","./ssh-connection-recoverability-CNHp0WBp.js","./AgentMapWorkspaceContextMenu-l4QeMc3y.js","./shallow-BpOhx1Gc.js","./lazy-with-retry-pSZJrSfN.js","./selectors-XOBeaOSb.js"])))=>i.map(i=>d[i]);
import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as lazyWithRetry } from "./lazy-with-retry-pSZJrSfN.js";
import { l as createLucideIcon, n as cn, t as Button } from "./button-DszXJEV6.js";
import { t as RepoIconGlyph } from "./repo-icon-Dcv6msBx.js";
import { t as Minus } from "./minus-Byrkh1sN.js";
import { t as Moon } from "./moon-Cw6GyiDZ.js";
import { t as Plus } from "./plus-Db0kWPVa.js";
import { n as agentTypeToIconAgent, st as parseExecutionHostId } from "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import { t as __vitePreload } from "./preload-helper-Cgw39-ka.js";
import "./agent-kind-Dfx6MnkP.js";
import "./es2015-B5WZ-7WO.js";
import { d as ContextMenuSubTrigger, f as ContextMenuTrigger, i as ContextMenuLabel, l as ContextMenuSub, n as ContextMenuContent, r as ContextMenuItem, s as ContextMenuSeparator, t as ContextMenu, u as ContextMenuSubContent } from "./context-menu-D4RKI7hR.js";
import { i as PopoverTrigger, r as PopoverContent, t as Popover } from "./popover-CgR1mzy7.js";
import "./tooltip-DPmd1AoJ.js";
import "./localized-catalog-DubKHKUR.js";
import "./AgentWorkingSpinner-BpnTWNKF.js";
import { n as agentStateLabel, t as AgentStateDot } from "./AgentStateDot-DFt63YGw.js";
import "./icons-jFAuHbv9.js";
import { r as getAgentLabel, t as AgentIcon } from "./agent-catalog-CBF2CV5Q.js";
import { o as dashboardCardDisplayState } from "./dashboard-snapshot-B9IiTV8p.js";
import { t as DashboardHostBadge } from "./DashboardHostBadge-DXORvYCI.js";
import { n as filterAgentMapCards } from "./agent-map-filter-CTyDhUZY.js";
import { t as usePrefersReducedMotion } from "./usePrefersReducedMotion-TEdW-TWP.js";
import { a as agentMapWorktreeIdentityFromParts, i as agentMapWorktreeIdentity, n as agentMapWorkspaceIdentity, r as agentMapWorkspaceTopologyIdentity, t as agentMapCardTopologyIdentity } from "./agent-map-workspace-identity-DOM8S3VE.js";
var Focus = createLucideIcon("focus", [
	["circle", {
		cx: "12",
		cy: "12",
		r: "3",
		key: "1v7zrd"
	}],
	["path", {
		d: "M3 7V5a2 2 0 0 1 2-2h2",
		key: "aa7l1z"
	}],
	["path", {
		d: "M17 3h2a2 2 0 0 1 2 2v2",
		key: "4qcy5o"
	}],
	["path", {
		d: "M21 17v2a2 2 0 0 1-2 2h-2",
		key: "6vwrx8"
	}],
	["path", {
		d: "M7 21H5a2 2 0 0 1-2-2v-2",
		key: "ioqczr"
	}]
]);
var import_react = /* @__PURE__ */ __toESM(require_react());
function agentMapDurationMinutes(card, now) {
	if (!Number.isFinite(card.startedAt) || card.startedAt <= 0) return 0;
	const end = card.finishedAt && card.finishedAt >= card.startedAt ? card.finishedAt : now;
	return Math.max(0, (end - card.startedAt) / 6e4);
}
function agentMapNodeStatus(card) {
	return dashboardCardDisplayState(card);
}
var GOLDEN_ANGLE = 2.399963229728653;
function stableHash(value) {
	let hash = 2166136261;
	for (let index = 0; index < value.length; index += 1) {
		hash ^= value.charCodeAt(index);
		hash = Math.imul(hash, 16777619);
	}
	return hash >>> 0;
}
function placeAgentMapAgents({ worktreeId, cards, radius, agentRadius, now }) {
	const availableRadius = Math.max(0, radius - agentRadius - 6);
	const sorted = [...cards].sort((a, b) => a.paneKey < b.paneKey ? -1 : a.paneKey > b.paneKey ? 1 : 0);
	const capacity = Math.ceil(Math.sqrt(Math.max(1, sorted.length))) ** 2;
	const angleOffset = stableHash(worktreeId) / 4294967295 * Math.PI * 2;
	return sorted.map((card, index) => {
		const orbit = sorted.length === 1 ? 0 : Math.sqrt((index + .5) / capacity) * availableRadius;
		const angle = angleOffset + index * GOLDEN_ANGLE;
		return {
			card,
			x: Math.cos(angle) * orbit,
			y: Math.sin(angle) * orbit,
			radius: agentRadius,
			durationMinutes: agentMapDurationMinutes(card, now),
			status: agentMapNodeStatus(card)
		};
	});
}
const AGENT_MAP_PACKING_SCORE_TOLERANCE = .001;
var PACKING_GRID_SIZE = 128;
function packingGridLevel(radius) {
	return Math.max(0, Math.ceil(Math.log2((radius * 2 + 8) / PACKING_GRID_SIZE)));
}
function addAgentMapPackingCircle(index, circle) {
	const level = packingGridLevel(circle.radius);
	let grid = index.get(level);
	if (!grid) {
		grid = {
			cells: /* @__PURE__ */ new Map(),
			cellSize: PACKING_GRID_SIZE * 2 ** level
		};
		index.set(level, grid);
	}
	const left = Math.floor((circle.x - circle.radius) / grid.cellSize);
	const right = Math.floor((circle.x + circle.radius) / grid.cellSize);
	const top = Math.floor((circle.y - circle.radius) / grid.cellSize);
	const bottom = Math.floor((circle.y + circle.radius) / grid.cellSize);
	for (let x = left; x <= right; x += 1) {
		let column = grid.cells.get(x);
		if (!column) {
			column = /* @__PURE__ */ new Map();
			grid.cells.set(x, column);
		}
		for (let y = top; y <= bottom; y += 1) {
			const cell = column.get(y);
			if (cell) cell.push(circle);
			else column.set(y, [circle]);
		}
	}
}
function agentMapPackingCircleOverlaps(candidate, index) {
	const searchRadius = candidate.radius + 8;
	const checked = /* @__PURE__ */ new Set();
	for (const grid of index.values()) {
		const left = Math.floor((candidate.x - searchRadius) / grid.cellSize);
		const right = Math.floor((candidate.x + searchRadius) / grid.cellSize);
		const top = Math.floor((candidate.y - searchRadius) / grid.cellSize);
		const bottom = Math.floor((candidate.y + searchRadius) / grid.cellSize);
		for (let x = left; x <= right; x += 1) {
			const column = grid.cells.get(x);
			if (!column) continue;
			for (let y = top; y <= bottom; y += 1) for (const circle of column.get(y) ?? []) {
				if (checked.has(circle)) continue;
				checked.add(circle);
				if (Math.hypot(candidate.x - circle.x, candidate.y - circle.y) < candidate.radius + circle.radius + 8 - .001) return true;
			}
		}
	}
	return false;
}
var PACKING_ANGLE_STEPS = 72;
var MAX_PACKING_CANDIDATE_ANCHORS = 128;
var MAX_DIRECT_OVERLAP_WORKTREES = 4;
var LARGE_PACKING_THRESHOLD = 256;
var VERY_LARGE_PACKING_THRESHOLD = 512;
var SCORE_TOLERANCE = AGENT_MAP_PACKING_SCORE_TOLERANCE;
var CENTER_DIRECTIONS = [
	[-1, -1],
	[0, -1],
	[1, -1],
	[-1, 0],
	[1, 0],
	[-1, 1],
	[0, 1],
	[1, 1]
];
function compareStable$5(a, b) {
	return a < b ? -1 : a > b ? 1 : 0;
}
function hashFraction(value) {
	let hash = 2166136261;
	for (let index = 0; index < value.length; index += 1) {
		hash ^= value.charCodeAt(index);
		hash = Math.imul(hash, 16777619);
	}
	return (hash >>> 0) / 4294967295;
}
function placedWorktreesOverlap(candidate, placed) {
	return placed.some((worktree) => Math.hypot(candidate.x - worktree.x, candidate.y - worktree.y) < candidate.radius + worktree.radius + 8 - SCORE_TOLERANCE);
}
function comparePackingScores(a, b, placed) {
	for (const key of ["enclosingRadius", "distanceFromCenter"]) if (Math.abs(a[key] - b[key]) > SCORE_TOLERANCE) return a[key] - b[key];
	a.neighborDistance ?? (a.neighborDistance = placed.reduce((sum, other) => sum + Math.hypot(a.x - other.x, a.y - other.y), 0));
	b.neighborDistance ?? (b.neighborDistance = placed.reduce((sum, other) => sum + Math.hypot(b.x - other.x, b.y - other.y), 0));
	return Math.abs(a.neighborDistance - b.neighborDistance) > SCORE_TOLERANCE ? a.neighborDistance - b.neighborDistance : 0;
}
function compareBoundaryAnchors(a, b) {
	return Math.hypot(b.x, b.y) + b.radius - (Math.hypot(a.x, a.y) + a.radius) || compareStable$5(a.id, b.id);
}
function addBoundaryAnchor(boundaryAnchors, worktree, maxAnchors) {
	let low = 0;
	let high = boundaryAnchors.length;
	while (low < high) {
		const middle = low + high >>> 1;
		if (compareBoundaryAnchors(worktree, boundaryAnchors[middle]) < 0) high = middle;
		else low = middle + 1;
	}
	boundaryAnchors.splice(low, 0, worktree);
	if (boundaryAnchors.length > maxAnchors) boundaryAnchors.pop();
}
function placePackedWorktree(worktree, placed, boundaryAnchors, spatialIndex, currentRadius, searchBudget) {
	let best;
	const anchors = placed.length <= searchBudget.candidateAnchors ? placed : boundaryAnchors;
	const scoreNeighbors = searchBudget.candidateAnchors === MAX_PACKING_CANDIDATE_ANCHORS ? placed : anchors;
	for (const anchor of anchors) {
		const orbit = anchor.radius + worktree.radius + 8;
		const angleOffset = hashFraction(`${worktree.id}:${anchor.id}`) * Math.PI * 2;
		for (let step = 0; step < searchBudget.angleSteps; step += 1) {
			const angle = angleOffset + step / searchBudget.angleSteps * Math.PI * 2;
			const x = anchor.x + Math.cos(angle) * orbit;
			const y = anchor.y + Math.sin(angle) * orbit;
			const overlapCandidate = {
				x,
				y,
				radius: worktree.radius
			};
			if (spatialIndex ? agentMapPackingCircleOverlaps(overlapCandidate, spatialIndex) : placedWorktreesOverlap(overlapCandidate, placed)) continue;
			const distanceFromCenter = Math.hypot(x, y);
			const candidate = {
				x,
				y,
				enclosingRadius: Math.max(currentRadius, distanceFromCenter + worktree.radius),
				distanceFromCenter
			};
			if (!best || comparePackingScores(candidate, best, scoreNeighbors) < 0) best = candidate;
		}
	}
	if (best) {
		worktree.x = best.x;
		worktree.y = best.y;
		return;
	}
	let fallbackX = Number.NEGATIVE_INFINITY;
	for (const candidate of placed) fallbackX = Math.max(fallbackX, candidate.x + candidate.radius);
	worktree.x = fallbackX + worktree.radius + 8;
	worktree.y = 0;
}
function enclosingRadius(worktrees, x, y) {
	let radius = 0;
	for (const worktree of worktrees) radius = Math.max(radius, Math.hypot(worktree.x - x, worktree.y - y) + worktree.radius);
	return radius;
}
function packingSearchBudget(count) {
	if (count > VERY_LARGE_PACKING_THRESHOLD) return {
		angleSteps: 16,
		candidateAnchors: 12
	};
	if (count > LARGE_PACKING_THRESHOLD) return {
		angleSteps: 24,
		candidateAnchors: 64
	};
	return {
		angleSteps: PACKING_ANGLE_STEPS,
		candidateAnchors: MAX_PACKING_CANDIDATE_ANCHORS
	};
}
function findEnclosingCenter(worktrees, bounds) {
	let x = (bounds.left + bounds.right) / 2;
	let y = (bounds.top + bounds.bottom) / 2;
	let radius = enclosingRadius(worktrees, x, y);
	let step = Math.max(bounds.right - bounds.left, bounds.bottom - bounds.top) / 4;
	while (step > SCORE_TOLERANCE) {
		let improved = false;
		for (const [dx, dy] of CENTER_DIRECTIONS) {
			const candidateX = x + dx * step;
			const candidateY = y + dy * step;
			const candidateRadius = enclosingRadius(worktrees, candidateX, candidateY);
			if (candidateRadius < radius - SCORE_TOLERANCE) {
				x = candidateX;
				y = candidateY;
				radius = candidateRadius;
				improved = true;
			}
		}
		if (!improved) step /= 2;
	}
	return {
		x,
		y
	};
}
function packAgentMapWorktrees(worktrees) {
	const packed = [...worktrees].sort((a, b) => b.radius - a.radius || compareStable$5(a.id, b.id));
	const placed = [];
	const boundaryAnchors = [];
	const searchBudget = packingSearchBudget(packed.length);
	const spatialIndex = packed.length > MAX_DIRECT_OVERLAP_WORKTREES ? /* @__PURE__ */ new Map() : null;
	let currentRadius = 0;
	for (const worktree of packed) {
		if (placed.length > 0) placePackedWorktree(worktree, placed, boundaryAnchors, spatialIndex, currentRadius, searchBudget);
		placed.push(worktree);
		addBoundaryAnchor(boundaryAnchors, worktree, searchBudget.candidateAnchors);
		if (spatialIndex) addAgentMapPackingCircle(spatialIndex, worktree);
		currentRadius = Math.max(currentRadius, Math.hypot(worktree.x, worktree.y) + worktree.radius);
	}
	if (packed.length === 0) return packed;
	let left = Number.POSITIVE_INFINITY;
	let right = Number.NEGATIVE_INFINITY;
	let top = Number.POSITIVE_INFINITY;
	let bottom = Number.NEGATIVE_INFINITY;
	for (const worktree of packed) {
		left = Math.min(left, worktree.x - worktree.radius);
		right = Math.max(right, worktree.x + worktree.radius);
		top = Math.min(top, worktree.y - worktree.radius);
		bottom = Math.max(bottom, worktree.y + worktree.radius);
	}
	const center = findEnclosingCenter(packed, {
		left,
		right,
		top,
		bottom
	});
	for (const worktree of packed) {
		worktree.x -= center.x;
		worktree.y -= center.y;
	}
	return packed.sort((a, b) => compareStable$5(a.id, b.id));
}
var HORIZONTAL_GAP = 54;
var VERTICAL_GAP = 58;
var FAMILY_PADDING = 8;
var WORKTREE_PADDING = 6;
var COMPACT_FANOUT_THRESHOLD = 12;
var MAX_EXACT_LINEAGE_AGENTS = 256;
function compareStable$4(a, b) {
	return a < b ? -1 : a > b ? 1 : 0;
}
function encloseFamily$1(id, agents, nodeRadius) {
	let left = Number.POSITIVE_INFINITY;
	let right = Number.NEGATIVE_INFINITY;
	let top = Number.POSITIVE_INFINITY;
	let bottom = Number.NEGATIVE_INFINITY;
	for (const agent of agents) {
		left = Math.min(left, agent.x - nodeRadius);
		right = Math.max(right, agent.x + nodeRadius);
		top = Math.min(top, agent.y - nodeRadius);
		bottom = Math.max(bottom, agent.y + nodeRadius);
	}
	const centerX = (left + right) / 2;
	const centerY = (top + bottom) / 2;
	let radius = 0;
	for (const agent of agents) {
		agent.x -= centerX;
		agent.y -= centerY;
		radius = Math.max(radius, Math.hypot(agent.x, agent.y) + nodeRadius + FAMILY_PADDING);
	}
	return {
		id,
		x: 0,
		y: 0,
		radius,
		agents
	};
}
function buildCompactFanoutFamily(root, children, nodeRadius, emitted) {
	const columns = Math.ceil(Math.sqrt(children.length));
	const width = (Math.min(columns, children.length) - 1) * HORIZONTAL_GAP;
	const agents = [{
		card: root,
		x: 0,
		y: 0
	}];
	emitted.add(root.paneKey);
	for (const [index, child] of children.entries()) {
		emitted.add(child.paneKey);
		agents.push({
			card: child,
			x: index % columns * HORIZONTAL_GAP - width / 2,
			y: (Math.floor(index / columns) + 1) * VERTICAL_GAP
		});
	}
	return encloseFamily$1(root.paneKey, agents, nodeRadius);
}
function buildFamily$1(root, childrenByParent, nodeRadius, emitted) {
	const agents = [];
	let leafIndex = 0;
	const rootChildren = (childrenByParent.get(root.paneKey) ?? []).filter((child) => !emitted.has(child.paneKey));
	if (rootChildren.length >= COMPACT_FANOUT_THRESHOLD && rootChildren.every((child) => (childrenByParent.get(child.paneKey) ?? []).length === 0)) return buildCompactFanoutFamily(root, rootChildren, nodeRadius, emitted);
	const placeSubtree = (card, depth, ancestors) => {
		if (ancestors.has(card.paneKey) || emitted.has(card.paneKey)) return leafIndex++ * HORIZONTAL_GAP;
		emitted.add(card.paneKey);
		const nextAncestors = new Set(ancestors);
		nextAncestors.add(card.paneKey);
		const childXs = (childrenByParent.get(card.paneKey) ?? []).filter((child) => !nextAncestors.has(child.paneKey) && !emitted.has(child.paneKey)).map((child) => placeSubtree(child, depth + 1, nextAncestors));
		const x = childXs.length > 0 ? (Math.min(...childXs) + Math.max(...childXs)) / 2 : leafIndex++ * HORIZONTAL_GAP;
		agents.push({
			card,
			x,
			y: depth * VERTICAL_GAP
		});
		return x;
	};
	placeSubtree(root, 0, /* @__PURE__ */ new Set());
	return encloseFamily$1(root.paneKey, agents, nodeRadius);
}
function layoutBoundedLineage$1(sorted, childrenByParent, childPaneKeys, nodeRadius) {
	const levels = [];
	const emitted = /* @__PURE__ */ new Set();
	const roots = sorted.filter((card) => !childPaneKeys.has(card.paneKey));
	for (const seed of [...roots, ...sorted]) {
		if (emitted.has(seed.paneKey)) continue;
		const stack = [{
			card: seed,
			depth: 0
		}];
		while (stack.length > 0) {
			const entry = stack.pop();
			if (emitted.has(entry.card.paneKey)) continue;
			emitted.add(entry.card.paneKey);
			const level = levels[entry.depth] ?? [];
			levels[entry.depth] = level;
			level.push(entry.card);
			const children = childrenByParent.get(entry.card.paneKey) ?? [];
			for (let index = children.length - 1; index >= 0; index -= 1) if (!emitted.has(children[index].paneKey)) stack.push({
				card: children[index],
				depth: entry.depth + 1
			});
		}
	}
	const agents = [];
	let rowIndex = 0;
	for (const level of levels) {
		const columns = Math.ceil(Math.sqrt(level.length));
		for (let rowStart = 0; rowStart < level.length; rowStart += columns) {
			const row = level.slice(rowStart, rowStart + columns);
			const width = (row.length - 1) * HORIZONTAL_GAP;
			for (const [index, card] of row.entries()) agents.push({
				card,
				x: index * HORIZONTAL_GAP - width / 2,
				y: rowIndex * VERTICAL_GAP
			});
			rowIndex += 1;
		}
	}
	const family = encloseFamily$1(sorted[0].paneKey, agents, nodeRadius);
	family.agents.sort((a, b) => compareStable$4(a.card.paneKey, b.card.paneKey));
	return {
		agents: family.agents,
		radius: Math.max(52, family.radius + WORKTREE_PADDING)
	};
}
function layoutAgentMapLineage(cards, nodeRadius) {
	const sorted = [...cards].sort((a, b) => compareStable$4(a.paneKey, b.paneKey));
	const cardsByPaneKey = new Map(sorted.map((card) => [card.paneKey, card]));
	const childrenByParent = /* @__PURE__ */ new Map();
	const childPaneKeys = /* @__PURE__ */ new Set();
	for (const card of sorted) {
		const parentPaneKey = card.parentPaneKey;
		if (!parentPaneKey || parentPaneKey === card.paneKey || !cardsByPaneKey.has(parentPaneKey)) continue;
		childPaneKeys.add(card.paneKey);
		childrenByParent.set(parentPaneKey, [...childrenByParent.get(parentPaneKey) ?? [], card]);
	}
	if (childPaneKeys.size === 0) return null;
	if (sorted.length > MAX_EXACT_LINEAGE_AGENTS) return layoutBoundedLineage$1(sorted, childrenByParent, childPaneKeys, nodeRadius);
	const emitted = /* @__PURE__ */ new Set();
	const roots = sorted.filter((card) => !childPaneKeys.has(card.paneKey));
	const families = [];
	for (const root of roots) if (!emitted.has(root.paneKey)) families.push(buildFamily$1(root, childrenByParent, nodeRadius, emitted));
	for (const card of sorted) if (!emitted.has(card.paneKey)) families.push(buildFamily$1(card, childrenByParent, nodeRadius, emitted));
	const packed = packAgentMapWorktrees(families);
	return {
		agents: packed.flatMap((family) => family.agents.map((agent) => ({
			...agent,
			x: family.x + agent.x,
			y: family.y + agent.y
		}))).sort((a, b) => compareStable$4(a.card.paneKey, b.card.paneKey)),
		radius: Math.max(52, ...packed.map((family) => Math.hypot(family.x, family.y) + family.radius + WORKTREE_PADDING))
	};
}
function emptyStatusCounts$1() {
	return {
		working: 0,
		blocked: 0,
		waiting: 0,
		done: 0,
		idle: 0
	};
}
function refreshAgentMapMetadata(geometry, cards, workspaces, now) {
	const cardsByPaneKey = new Map(cards.map((card) => [card.paneKey, card]));
	const workspacesById = new Map(workspaces.map((workspace) => [agentMapWorkspaceIdentity(workspace), workspace]));
	const projects = geometry.projects.map((project) => {
		let projectName = project.name;
		let agentCount = 0;
		const worktrees = project.worktrees.map((worktree) => {
			const workspace = workspacesById.get(worktree.id);
			if (workspace) projectName = workspace.repoName;
			let worktreeName = workspace?.worktreeName ?? worktree.name;
			let workspaceKind = workspace?.workspaceKind ?? worktree.workspaceKind;
			let hostKind = workspace?.hostKind ?? worktree.hostKind;
			let hostLabel = workspace?.hostLabel ?? worktree.hostLabel;
			const statusCounts = emptyStatusCounts$1();
			const agents = worktree.agents.flatMap((agent) => {
				const card = cardsByPaneKey.get(agent.card.paneKey);
				if (!card) return [];
				projectName = card.repoName;
				worktreeName = card.worktreeName;
				workspaceKind = card.workspaceKind ?? "worktree";
				hostKind = card.hostKind ?? hostKind;
				hostLabel = card.hostLabel ?? hostLabel;
				agentCount += 1;
				statusCounts[agentMapNodeStatus(card)] += 1;
				return [{
					...agent,
					card,
					durationMinutes: agentMapDurationMinutes(card, now),
					status: agentMapNodeStatus(card)
				}];
			});
			return {
				...worktree,
				name: worktreeName,
				workspaceKind,
				hostKind,
				hostLabel,
				agents,
				statusCounts,
				quiet: statusCounts.idle === agents.length
			};
		});
		return {
			...project,
			name: projectName,
			worktrees,
			agentCount
		};
	});
	return {
		...geometry,
		projects
	};
}
var LINEAGE_VERTICAL_GAP = 28;
var MAX_HIERARCHICAL_CLUSTER_FANOUT = 12;
var MAX_EXACT_LINEAGE_WORKTREES = 256;
function compareStable$3(a, b) {
	return a < b ? -1 : a > b ? 1 : 0;
}
function encloseFamily(id, worktrees) {
	const left = Math.min(...worktrees.map((worktree) => worktree.x - worktree.radius));
	const right = Math.max(...worktrees.map((worktree) => worktree.x + worktree.radius));
	const top = Math.min(...worktrees.map((worktree) => worktree.y - worktree.radius));
	const bottom = Math.max(...worktrees.map((worktree) => worktree.y + worktree.radius));
	const centerX = (left + right) / 2;
	const centerY = (top + bottom) / 2;
	for (const worktree of worktrees) {
		worktree.x -= centerX;
		worktree.y -= centerY;
	}
	return {
		id,
		x: 0,
		y: 0,
		radius: Math.max(...worktrees.map((worktree) => Math.hypot(worktree.x, worktree.y) + worktree.radius)),
		worktrees
	};
}
function buildFamily(root, childrenByParent, emitted, ancestors) {
	emitted.add(root.id);
	const nextAncestors = new Set(ancestors);
	nextAncestors.add(root.id);
	const children = (childrenByParent.get(root.id) ?? []).filter((child) => !nextAncestors.has(child.id) && !emitted.has(child.id));
	if (children.length === 0) return {
		id: root.id,
		x: 0,
		y: 0,
		radius: root.radius,
		worktrees: [{
			...root,
			x: 0,
			y: 0
		}]
	};
	const childFamilies = packAgentMapWorktrees(children.map((child) => buildExactFamily(child, childrenByParent, emitted)));
	const childLeft = Math.min(...childFamilies.map((family) => family.x - family.radius));
	const childRight = Math.max(...childFamilies.map((family) => family.x + family.radius));
	const childTop = Math.min(...childFamilies.map((family) => family.y - family.radius));
	const childOffsetX = -(childLeft + childRight) / 2;
	const childOffsetY = root.radius + LINEAGE_VERTICAL_GAP - childTop;
	const worktrees = [{
		...root,
		x: 0,
		y: 0
	}];
	for (const family of childFamilies) for (const worktree of family.worktrees) worktrees.push({
		...worktree,
		x: worktree.x + family.x + childOffsetX,
		y: worktree.y + family.y + childOffsetY
	});
	return encloseFamily(root.id, worktrees);
}
function collectLinearFamily(root, childrenByParent, emitted) {
	const worktrees = [];
	const ancestors = /* @__PURE__ */ new Set();
	let current = root;
	while (current) {
		worktrees.push(current);
		ancestors.add(current.id);
		const children = (childrenByParent.get(current.id) ?? []).filter((child) => !ancestors.has(child.id) && !emitted.has(child.id));
		if (children.length > 1) return null;
		current = children[0];
	}
	return worktrees;
}
function buildLinearFamily(worktrees) {
	const positioned = worktrees.map((worktree) => ({
		...worktree,
		x: 0,
		y: 0
	}));
	let radius = worktrees.at(-1)?.radius ?? 0;
	for (let index = worktrees.length - 2; index >= 0; index -= 1) {
		positioned[index].y = -(radius + LINEAGE_VERTICAL_GAP / 2);
		radius += worktrees[index].radius + LINEAGE_VERTICAL_GAP / 2;
	}
	let familyCenterY = 0;
	for (let index = 0; index < positioned.length; index += 1) {
		positioned[index].y += familyCenterY;
		familyCenterY += worktrees[index].radius + LINEAGE_VERTICAL_GAP / 2;
	}
	return {
		id: worktrees[0].id,
		x: 0,
		y: 0,
		radius,
		worktrees: positioned
	};
}
function buildExactFamily(root, childrenByParent, emitted) {
	const linearFamily = collectLinearFamily(root, childrenByParent, emitted);
	if (!linearFamily) return buildFamily(root, childrenByParent, emitted, /* @__PURE__ */ new Set());
	for (const worktree of linearFamily) emitted.add(worktree.id);
	return buildLinearFamily(linearFamily);
}
function layoutBoundedLineage(sorted, childrenByParent, childIds) {
	const levels = [];
	const emitted = /* @__PURE__ */ new Set();
	const roots = sorted.filter((worktree) => !childIds.has(worktree.id));
	for (const seed of [...roots, ...sorted]) {
		if (emitted.has(seed.id)) continue;
		const stack = [{
			depth: 0,
			worktree: seed
		}];
		while (stack.length > 0) {
			const entry = stack.pop();
			if (emitted.has(entry.worktree.id)) continue;
			emitted.add(entry.worktree.id);
			const level = levels[entry.depth] ?? [];
			levels[entry.depth] = level;
			level.push(entry.worktree);
			const children = childrenByParent.get(entry.worktree.id) ?? [];
			for (let index = children.length - 1; index >= 0; index -= 1) if (!emitted.has(children[index].id)) stack.push({
				depth: entry.depth + 1,
				worktree: children[index]
			});
		}
	}
	const positioned = [];
	let y = 0;
	let previousMaxRadius = 0;
	let hasPositionedRow = false;
	for (const level of levels) {
		const columns = Math.ceil(Math.sqrt(level.length));
		for (let rowStart = 0; rowStart < level.length; rowStart += columns) {
			const row = level.slice(rowStart, rowStart + columns);
			let maxRadius = 0;
			let width = -8;
			for (const worktree of row) {
				maxRadius = Math.max(maxRadius, worktree.radius);
				width += worktree.radius * 2 + 8;
			}
			if (hasPositionedRow) y += previousMaxRadius + maxRadius + LINEAGE_VERTICAL_GAP;
			let x = -width / 2;
			for (const worktree of row) {
				positioned.push({
					...worktree,
					x: x + worktree.radius,
					y
				});
				x += worktree.radius * 2 + 8;
			}
			previousMaxRadius = maxRadius;
			hasPositionedRow = true;
		}
	}
	let left = Number.POSITIVE_INFINITY;
	let right = Number.NEGATIVE_INFINITY;
	let top = Number.POSITIVE_INFINITY;
	let bottom = Number.NEGATIVE_INFINITY;
	for (const worktree of positioned) {
		left = Math.min(left, worktree.x - worktree.radius);
		right = Math.max(right, worktree.x + worktree.radius);
		top = Math.min(top, worktree.y - worktree.radius);
		bottom = Math.max(bottom, worktree.y + worktree.radius);
	}
	const centerX = (left + right) / 2;
	const centerY = (top + bottom) / 2;
	return positioned.map((worktree) => ({
		...worktree,
		x: worktree.x - centerX,
		y: worktree.y - centerY
	})).sort((a, b) => compareStable$3(a.id, b.id));
}
function layoutAgentMapWorktreeLineage(worktrees) {
	const sorted = [...worktrees].sort((a, b) => compareStable$3(a.id, b.id));
	const worktreesById = new Map(sorted.map((worktree) => [worktree.id, worktree]));
	const clusterChildCounts = /* @__PURE__ */ new Map();
	for (const worktree of sorted) if (worktree.clusterParentId && worktreesById.has(worktree.clusterParentId)) clusterChildCounts.set(worktree.clusterParentId, (clusterChildCounts.get(worktree.clusterParentId) ?? 0) + 1);
	const childrenByParent = /* @__PURE__ */ new Map();
	const childIds = /* @__PURE__ */ new Set();
	for (const worktree of sorted) {
		const clusterParentId = worktree.clusterParentId;
		const parentId = clusterParentId && (clusterChildCounts.get(clusterParentId) ?? 0) <= MAX_HIERARCHICAL_CLUSTER_FANOUT ? clusterParentId : worktree.parentId;
		if (!parentId || parentId === worktree.id || !worktreesById.has(parentId)) continue;
		childIds.add(worktree.id);
		const siblings = childrenByParent.get(parentId);
		if (siblings) siblings.push(worktree);
		else childrenByParent.set(parentId, [worktree]);
	}
	if (sorted.length > MAX_EXACT_LINEAGE_WORKTREES) return layoutBoundedLineage(sorted, childrenByParent, childIds);
	const emitted = /* @__PURE__ */ new Set();
	const families = [];
	for (const root of sorted.filter((worktree) => !childIds.has(worktree.id))) if (!emitted.has(root.id)) families.push(buildExactFamily(root, childrenByParent, emitted));
	for (const worktree of sorted) if (!emitted.has(worktree.id)) families.push(buildExactFamily(worktree, childrenByParent, emitted));
	return packAgentMapWorktrees(families).flatMap((family) => family.worktrees.map((worktree) => ({
		...worktree,
		x: worktree.x + family.x,
		y: worktree.y + family.y
	}))).sort((a, b) => compareStable$3(a.id, b.id));
}
var PROJECT_GAP = 32;
function placeUnlinkedProjects(projects) {
	let cursorX = 0;
	return projects.map((project) => {
		const positioned = {
			...project,
			x: cursorX + project.radius,
			y: 0
		};
		cursorX += project.radius * 2 + PROJECT_GAP;
		return positioned;
	});
}
function placeAgentMapProjects(projects, minimumWidth, minimumHeight, worldMargin) {
	const positioned = projects.some((project) => project.clusterParentId) ? layoutAgentMapWorktreeLineage(projects) : placeUnlinkedProjects(projects);
	const left = Math.min(...positioned.map((project) => project.x - project.radius));
	const right = Math.max(...positioned.map((project) => project.x + project.radius));
	const top = Math.min(...positioned.map((project) => project.y - project.radius));
	const bottom = Math.max(...positioned.map((project) => project.y + project.radius));
	const naturalWidth = right - left + worldMargin * 2;
	const naturalHeight = bottom - top + worldMargin * 2;
	const width = Math.max(minimumWidth, naturalWidth);
	const height = Math.max(minimumHeight, naturalHeight);
	const offsetX = worldMargin - left + (width - naturalWidth) / 2;
	const offsetY = worldMargin - top + (height - naturalHeight) / 2;
	return {
		projects: positioned.map((project) => ({
			...project,
			x: project.x + offsetX,
			y: project.y + offsetY
		})),
		width,
		height
	};
}
function compareStable$2(a, b) {
	return a < b ? -1 : a > b ? 1 : 0;
}
function selectAgentMapSpawnParentContainer(cards, cardsByPaneKey, containerIdentity) {
	const ownContainerId = cards[0] ? containerIdentity(cards[0]) : void 0;
	const linkCounts = /* @__PURE__ */ new Map();
	for (const card of cards) {
		const parent = card.parentPaneKey ? cardsByPaneKey.get(card.parentPaneKey) : void 0;
		const parentContainerId = parent ? containerIdentity(parent) : void 0;
		if (!parentContainerId || parentContainerId === ownContainerId) continue;
		linkCounts.set(parentContainerId, (linkCounts.get(parentContainerId) ?? 0) + 1);
	}
	return [...linkCounts].sort(([leftId, leftCount], [rightId, rightCount]) => rightCount !== leftCount ? rightCount - leftCount : compareStable$2(leftId, rightId)).at(0)?.[0];
}
function agentMapWorktreeHost(cards, workspace) {
	const executionHostId = workspace?.executionHostId ?? cards[0]?.executionHostId;
	const parsedHost = parseExecutionHostId(executionHostId);
	return {
		executionHostId,
		hostKind: parsedHost?.kind === "ssh" ? "ssh" : parsedHost?.kind === "runtime" ? "remote" : workspace?.hostKind ?? cards[0]?.hostKind,
		hostLabel: workspace?.hostLabel ?? cards[0]?.hostLabel
	};
}
const AGENT_MAP_LINEAGE_RELATION = "orchestration";
var PROJECT_PADDING = 12;
var WORLD_MARGIN = 32;
var RING_CONTENT_OFFSET = 40 / 2;
function compareStable$1(a, b) {
	return a < b ? -1 : a > b ? 1 : 0;
}
function agentMapTopologyKey(cards, workspaces = []) {
	return [...cards.map((card) => `a:${agentMapCardTopologyIdentity(card)}`), ...workspaces.map((workspace) => `w:${agentMapWorkspaceTopologyIdentity(workspace)}`)].sort(compareStable$1).join("|");
}
function shouldAggregateAgentMapWorktree(worktree, zoom, allowAggregation = true) {
	return allowAggregation && zoom < 1.15 && worktree.quiet && worktree.agents.length > 3;
}
function emptyStatusCounts() {
	return {
		working: 0,
		blocked: 0,
		waiting: 0,
		done: 0,
		idle: 0
	};
}
function worktreeRadius(agentCount) {
	return Math.max(52, 24 + Math.ceil(Math.sqrt(Math.max(1, agentCount))) * 28);
}
function buildLocalWorktree(id, cards, now, workspace) {
	const lineageLayout = layoutAgentMapLineage(cards, 20);
	const contentRadius = lineageLayout?.radius ?? worktreeRadius(cards.length);
	const radius = contentRadius + RING_CONTENT_OFFSET;
	const statusCounts = emptyStatusCounts();
	for (const card of cards) statusCounts[agentMapNodeStatus(card)] += 1;
	const host = agentMapWorktreeHost(cards, workspace);
	const executionHostId = host.executionHostId;
	const parentWorktreeId = workspace?.parentWorktreeId ?? cards[0]?.parentWorktreeId;
	return {
		id,
		parentId: parentWorktreeId ? agentMapWorktreeIdentityFromParts(parentWorktreeId, executionHostId) : void 0,
		worktreeId: workspace?.worktreeId ?? cards[0]?.worktreeId ?? id,
		...host,
		name: workspace?.worktreeName ?? cards[0]?.worktreeName ?? id,
		workspaceKind: workspace?.workspaceKind ?? cards[0]?.workspaceKind ?? "worktree",
		x: 0,
		y: 0,
		radius,
		agents: (lineageLayout?.agents.map(({ card, x, y }) => ({
			card,
			x,
			y,
			radius: 20,
			durationMinutes: agentMapDurationMinutes(card, now),
			status: agentMapNodeStatus(card)
		})) ?? placeAgentMapAgents({
			worktreeId: id,
			cards,
			radius: contentRadius,
			agentRadius: 20,
			now
		})).map((agent) => ({
			...agent,
			y: agent.y + RING_CONTENT_OFFSET
		})),
		statusCounts,
		quiet: statusCounts.idle === cards.length
	};
}
function buildLocalProject(id, cards, workspaces, cardsByPaneKey, now) {
	const byWorktree = /* @__PURE__ */ new Map();
	for (const card of cards) {
		const identity = agentMapWorktreeIdentity(card);
		const current = byWorktree.get(identity);
		if (current) current.push(card);
		else byWorktree.set(identity, [card]);
	}
	const workspacesById = new Map(workspaces.map((workspace) => [agentMapWorkspaceIdentity(workspace), workspace]));
	for (const workspaceId of workspacesById.keys()) if (!byWorktree.has(workspaceId)) byWorktree.set(workspaceId, []);
	const positionedWorktrees = layoutAgentMapWorktreeLineage([...byWorktree.entries()].sort(([a], [b]) => compareStable$1(a, b)).map(([worktreeId, worktreeCards]) => ({
		...buildLocalWorktree(worktreeId, worktreeCards, now, workspacesById.get(worktreeId)),
		clusterParentId: selectAgentMapSpawnParentContainer(worktreeCards, cardsByPaneKey, agentMapWorktreeIdentity)
	})));
	const contentRadius = Math.max(84, ...positionedWorktrees.map((worktree) => Math.hypot(worktree.x, worktree.y) + worktree.radius + PROJECT_PADDING));
	const worktrees = positionedWorktrees.map((worktree) => ({
		...worktree,
		y: worktree.y + RING_CONTENT_OFFSET
	}));
	return {
		id,
		name: cards[0]?.repoName ?? workspaces[0]?.repoName ?? id,
		x: 0,
		y: 0,
		clusterParentId: selectAgentMapSpawnParentContainer(cards, cardsByPaneKey, (card) => card.repoId),
		radius: contentRadius + RING_CONTENT_OFFSET,
		worktrees,
		agentCount: cards.length
	};
}
function deriveAgentMapLayout(cards, now, workspaces = []) {
	const topologyKey = agentMapTopologyKey(cards, workspaces);
	if (cards.length === 0 && workspaces.length === 0) return {
		projects: [],
		width: 900,
		height: 560,
		topologyKey
	};
	const byProject = /* @__PURE__ */ new Map();
	for (const card of cards) {
		const current = byProject.get(card.repoId) ?? {
			cards: [],
			workspaces: []
		};
		current.cards.push(card);
		byProject.set(card.repoId, current);
	}
	for (const workspace of workspaces) {
		const current = byProject.get(workspace.repoId) ?? {
			cards: [],
			workspaces: []
		};
		current.workspaces.push(workspace);
		byProject.set(workspace.repoId, current);
	}
	const cardsByPaneKey = new Map(cards.map((card) => [card.paneKey, card]));
	const framed = placeAgentMapProjects([...byProject.entries()].sort(([a], [b]) => compareStable$1(a, b)).map(([projectId, project]) => buildLocalProject(projectId, project.cards, project.workspaces, cardsByPaneKey, now)), 900, 560, WORLD_MARGIN);
	return {
		projects: framed.projects.map((project) => {
			return {
				...project,
				worktrees: project.worktrees.map((worktree) => ({
					...worktree,
					x: project.x + worktree.x,
					y: project.y + worktree.y,
					agents: worktree.agents.map((agent) => ({
						...agent,
						x: project.x + worktree.x + agent.x,
						y: project.y + worktree.y + agent.y
					}))
				}))
			};
		}),
		width: framed.width,
		height: framed.height,
		topologyKey
	};
}
function updateAgentMapLayout(cache, cards, now, workspaces = []) {
	const topologyKey = agentMapTopologyKey(cards, workspaces);
	if (!cache || cache.topologyKey !== topologyKey) {
		const geometry = deriveAgentMapLayout(cards, now, workspaces);
		return {
			cache: {
				topologyKey,
				geometry,
				packingGeneration: (cache?.packingGeneration ?? 0) + 1
			},
			layout: geometry
		};
	}
	return {
		cache,
		layout: refreshAgentMapMetadata(cache.geometry, cards, workspaces, now)
	};
}
var WORKTREE_LABEL_FONT_PX = 12;
var PROJECT_LABEL_FONT_PX = 13;
var COUNT_FONT_PX = 11;
var GLYPH_WIDTH_RATIO = .56;
var UPPERCASE_GLYPH_WIDTH_RATIO = .66;
var ASCENT_RATIO = .8;
var DESCENT_RATIO = .2;
var PROJECT_LABEL_ICON_PX = 16;
var LABEL_GAP_X_PX = 3;
var LABEL_GAP_Y_PX = 1;
var AGENT_LABEL_CLEARANCE_PX = 3;
var WORKTREE_LABEL_BASELINE = 18;
var COUNT_BASELINE = 32;
var PROJECT_NAME_TOP = 3;
var PROJECT_NAME_BOTTOM = 21;
var MAX_LABEL_CANDIDATES = 600;
var DECLUTTER_GRID_PX = 96;
function textWidth(text, fontPx, ratio = GLYPH_WIDTH_RATIO) {
	return text.length * fontPx * ratio;
}
function boxesOverlap(a, b) {
	return a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;
}
function addBox(grid, box) {
	const left = Math.floor(box.left / DECLUTTER_GRID_PX);
	const right = Math.floor(box.right / DECLUTTER_GRID_PX);
	const top = Math.floor(box.top / DECLUTTER_GRID_PX);
	const bottom = Math.floor(box.bottom / DECLUTTER_GRID_PX);
	for (let x = left; x <= right; x += 1) {
		let column = grid.get(x);
		if (!column) {
			column = /* @__PURE__ */ new Map();
			grid.set(x, column);
		}
		for (let y = top; y <= bottom; y += 1) {
			const cell = column.get(y);
			if (cell) cell.push(box);
			else column.set(y, [box]);
		}
	}
}
function collides(grid, box) {
	const left = Math.floor(box.left / DECLUTTER_GRID_PX);
	const right = Math.floor(box.right / DECLUTTER_GRID_PX);
	const top = Math.floor(box.top / DECLUTTER_GRID_PX);
	const bottom = Math.floor(box.bottom / DECLUTTER_GRID_PX);
	for (let x = left; x <= right; x += 1) {
		const column = grid.get(x);
		if (!column) continue;
		for (let y = top; y <= bottom; y += 1) for (const placed of column.get(y) ?? []) if (boxesOverlap(box, placed)) return true;
	}
	return false;
}
function centeredBox(centerX, anchorY, scale, width, localTop, localBottom) {
	const halfWidth = (width / 2 + LABEL_GAP_X_PX) * scale;
	return {
		left: centerX - halfWidth,
		right: centerX + halfWidth,
		top: anchorY + (localTop - LABEL_GAP_Y_PX) * scale,
		bottom: anchorY + (localBottom + LABEL_GAP_Y_PX) * scale
	};
}
function baselineBox(centerX, anchorY, scale, text, fontPx, baseline, widthRatio = GLYPH_WIDTH_RATIO) {
	return centeredBox(centerX, anchorY, scale, textWidth(text, fontPx, widthRatio), baseline - fontPx * ASCENT_RATIO, baseline + fontPx * DESCENT_RATIO);
}
function labelPriority(worktree) {
	return (worktree.statusCounts.blocked + worktree.statusCounts.waiting) * 1e6 + worktree.statusCounts.working * 1e4 + worktree.agents.length;
}
function isLabelCandidate(worktree, mapScale) {
	return !worktree.quiet || worktree.radius * mapScale >= 56;
}
function compareStable(a, b) {
	return a < b ? -1 : a > b ? 1 : 0;
}
function addAgentExclusionBoxes(grid, layout, mapScale) {
	const clearance = AGENT_LABEL_CLEARANCE_PX / Math.max(mapScale, .001);
	for (const project of layout.projects) for (const worktree of project.worktrees) for (const agent of worktree.agents) {
		if (!agent) continue;
		const radius = agent.radius + clearance;
		addBox(grid, {
			left: agent.x - radius,
			right: agent.x + radius,
			top: agent.y - radius,
			bottom: agent.y + radius
		});
	}
}
function selectVisibleAgentMapLabels(layout, labelScale, mapScale) {
	const agentGrid = /* @__PURE__ */ new Map();
	addAgentExclusionBoxes(agentGrid, layout, mapScale);
	const grid = /* @__PURE__ */ new Map();
	addAgentExclusionBoxes(grid, layout, mapScale);
	for (const project of layout.projects) {
		const name = project.name.toUpperCase();
		addBox(grid, centeredBox(project.x, project.y - project.radius, labelScale, PROJECT_LABEL_ICON_PX + textWidth(name, PROJECT_LABEL_FONT_PX, UPPERCASE_GLYPH_WIDTH_RATIO), PROJECT_NAME_TOP, PROJECT_NAME_BOTTOM));
	}
	const candidates = [];
	for (const project of layout.projects) for (const worktree of project.worktrees) if (isLabelCandidate(worktree, mapScale)) candidates.push(worktree);
	candidates.sort((a, b) => {
		const byPriority = labelPriority(b) - labelPriority(a);
		if (byPriority !== 0) return byPriority;
		const byRadius = b.radius - a.radius;
		return byRadius !== 0 ? byRadius : compareStable(a.id, b.id);
	});
	const worktreeIds = /* @__PURE__ */ new Set();
	for (const worktree of candidates.slice(0, MAX_LABEL_CANDIDATES)) {
		const box = baselineBox(worktree.x, worktree.y - worktree.radius, labelScale, worktree.name, WORKTREE_LABEL_FONT_PX, WORKTREE_LABEL_BASELINE);
		if (collides(agentGrid, box)) continue;
		if (collides(grid, box)) continue;
		addBox(grid, box);
		worktreeIds.add(worktree.id);
	}
	const projectCountIds = /* @__PURE__ */ new Set();
	for (const project of layout.projects) {
		const count = `${project.agentCount} AGENTS · ${project.worktrees.length} WORKSPACES`;
		const box = baselineBox(project.x, project.y - project.radius, labelScale, count, COUNT_FONT_PX, COUNT_BASELINE, UPPERCASE_GLYPH_WIDTH_RATIO);
		if (collides(grid, box)) continue;
		addBox(grid, box);
		projectCountIds.add(project.id);
	}
	return {
		worktreeIds,
		projectCountIds
	};
}
var CHEVRON_SPACING = 8;
var CHEVRON_DEPTH = 3.5;
var CHEVRON_HALF_WIDTH = 2.25;
var MAX_CHEVRONS_PER_PATH = 32;
function svgNumber(value) {
	return Math.round(value * 1e3) / 1e3;
}
function agentMapLineageChevronPath(points) {
	const segments = [];
	let totalLength = 0;
	for (let index = 1; index < points.length; index += 1) {
		const start = points[index - 1];
		const end = points[index];
		const dx = end.x - start.x;
		const dy = end.y - start.y;
		const length = Math.hypot(dx, dy);
		if (length === 0) continue;
		segments.push({
			start,
			unitX: dx / length,
			unitY: dy / length,
			length
		});
		totalLength += length;
	}
	if (segments.length === 0 || totalLength < CHEVRON_DEPTH * 2) return points[0] ? `M ${svgNumber(points[0].x)} ${svgNumber(points[0].y)}` : "";
	const chevronCount = Math.min(MAX_CHEVRONS_PER_PATH, Math.max(1, Math.floor(totalLength / CHEVRON_SPACING)));
	const commands = [];
	let segmentIndex = 0;
	let segmentStartDistance = 0;
	for (let index = 0; index < chevronCount; index += 1) {
		const distance = totalLength * (index + 1) / (chevronCount + 1);
		while (segmentIndex < segments.length - 1 && distance > segmentStartDistance + segments[segmentIndex].length) {
			segmentStartDistance += segments[segmentIndex].length;
			segmentIndex += 1;
		}
		const segment = segments[segmentIndex];
		const offset = distance - segmentStartDistance;
		const tipX = segment.start.x + segment.unitX * offset;
		const tipY = segment.start.y + segment.unitY * offset;
		const backX = tipX - segment.unitX * CHEVRON_DEPTH;
		const backY = tipY - segment.unitY * CHEVRON_DEPTH;
		const perpendicularX = -segment.unitY * CHEVRON_HALF_WIDTH;
		const perpendicularY = segment.unitX * CHEVRON_HALF_WIDTH;
		commands.push(`M ${svgNumber(backX + perpendicularX)} ${svgNumber(backY + perpendicularY)} L ${svgNumber(tipX)} ${svgNumber(tipY)} L ${svgNumber(backX - perpendicularX)} ${svgNumber(backY - perpendicularY)}`);
	}
	return commands.join(" ");
}
function agentMapDirectLineageChevronPath(parent, child) {
	const dx = child.x - parent.x;
	const dy = child.y - parent.y;
	const distance = Math.hypot(dx, dy);
	if (distance <= parent.radius + child.radius) return agentMapLineageChevronPath([parent]);
	const unitX = dx / distance;
	const unitY = dy / distance;
	return agentMapLineageChevronPath([{
		x: parent.x + unitX * parent.radius,
		y: parent.y + unitY * parent.radius
	}, {
		x: child.x - unitX * child.radius,
		y: child.y - unitY * child.radius
	}]);
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
const AgentMapWorktreeLabel = (0, import_react.memo)(function AgentMapWorktreeLabel$1({ worktree, visible, active, labelScale, mapScale }) {
	const showCount = active || visible && worktree.radius * mapScale >= 80;
	const agentCountText = translate("dashboardPopout.map.agentCount", worktree.agents.length === 1 ? "{{count}} agent" : "{{count}} agents", { count: worktree.agents.length });
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("g", {
		className: `agent-map-worktree-label-group${visible ? " is-visible" : ""}${active ? " is-active" : ""}${showCount ? " is-count-visible" : ""}${worktree.motionState ? ` is-${worktree.motionState}` : ""}`,
		transform: `translate(${worktree.x} ${worktree.y - worktree.radius}) scale(${labelScale})`,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("text", {
			className: "agent-map-worktree-label",
			y: 18,
			children: worktree.name
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("text", {
			className: "agent-map-worktree-count",
			y: 32,
			children: agentCountText
		})]
	});
});
function agentMapWorktreeActiveStatus(counts) {
	if (counts.blocked > 0) return "blocked";
	if (counts.waiting > 0) return "waiting";
	return counts.working > 0 ? "working" : null;
}
function formatDuration(minutes) {
	if (minutes < 1) return translate("dashboardPopout.card.time.justNow", "just now");
	if (minutes < 60) return translate("dashboardPopout.card.time.minutes", "{{count}}m", { count: Math.floor(minutes) });
	return translate("dashboardPopout.card.time.hours", "{{count}}h", { count: Math.floor(minutes / 60) });
}
function lineagePath(parent, child) {
	return agentMapDirectLineageChevronPath(parent, child);
}
function agentName(card) {
	return card.conversationName ?? (card.task.trim() || card.agentType);
}
function agentMapAttentionMarkerScale(mapScale) {
	const inverseScale = 1 / Math.max(mapScale, .001);
	return Math.max(1, inverseScale ** .72, inverseScale * .5);
}
function WorktreeDetails({ project, worktree, launchableAgents, onSelectAgent, onSpawnAgent, onDone }) {
	const activeCount = worktree.statusCounts.working + worktree.statusCounts.blocked + worktree.statusCounts.waiting;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(PopoverContent, {
		align: "center",
		sideOffset: 10,
		className: "w-80 p-0",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("header", {
				className: "border-b border-border px-3 py-2.5",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "block truncate text-[11px] text-muted-foreground",
						children: project.name
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("strong", {
						className: "block truncate text-[13px]",
						children: worktree.name
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "mt-1 block text-[11px] text-muted-foreground",
						children: translate("dashboardPopout.map.worktreeSummary", "{{total}} agents · {{active}} active · {{done}} done", {
							count: worktree.agents.length,
							defaultValue_one: "{{total}} agent · {{active}} active · {{done}} done",
							defaultValue_other: "{{total}} agents · {{active}} active · {{done}} done",
							total: worktree.agents.length,
							active: activeCount,
							done: worktree.statusCounts.done
						})
					})
				]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", {
				className: onSpawnAgent ? "border-b border-border px-2 py-2" : "px-2 py-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h3", {
					className: "mb-1 px-1 text-[11px] font-semibold text-muted-foreground",
					children: translate("dashboardPopout.map.runningAgents", "Agents")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "scrollbar-sleek max-h-56 space-y-0.5 overflow-y-auto",
					children: worktree.agents.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "px-2 py-1.5 text-[11px] text-muted-foreground",
						children: translate("dashboardPopout.map.noWorkspaceAgents", "No agents in this workspace.")
					}) : worktree.agents.map((agent) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
						type: "button",
						className: "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none",
						onClick: () => {
							onSelectAgent(agent.card);
							onDone();
						},
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentIcon, {
								agent: agentTypeToIconAgent(agent.card.agentType),
								size: 14
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
								className: "min-w-0 flex-1",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "block truncate text-[12px] font-medium",
									children: agentName(agent.card)
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
									className: "block truncate text-[11px] text-muted-foreground",
									children: [
										agentStateLabel(agent.status),
										" · ",
										formatDuration(agent.durationMinutes)
									]
								})]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentStateDot, {
								state: agent.status,
								size: "md"
							})
						]
					}, agent.card.paneKey))
				})]
			}),
			onSpawnAgent ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", {
				className: "px-3 py-2.5",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h3", {
					className: "mb-1.5 text-[11px] font-semibold text-muted-foreground",
					children: translate("dashboardPopout.map.spawnAgent", "Start a new agent")
				}), launchableAgents && launchableAgents.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "flex flex-wrap gap-1.5",
					children: launchableAgents.map((agent) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
						type: "button",
						variant: "outline",
						size: "xs",
						className: "gap-1.5",
						onClick: () => {
							onSpawnAgent({
								worktreeId: worktree.worktreeId,
								agent
							});
							onDone();
						},
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Plus, { className: "size-3" }),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentIcon, {
								agent,
								size: 12
							}),
							getAgentLabel(agent)
						]
					}, agent))
				}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "text-[11px] text-muted-foreground",
					children: translate("dashboardPopout.map.noLaunchableAgents", "No enabled agents detected.")
				})]
			}) : null
		]
	});
}
const AgentMapWorktreeRingNode = (0, import_react.memo)(function AgentMapWorktreeRingNode$1({ project, worktree, zoom, mapScale, held, selectedPaneKey, allowAggregation, showOrchestrationLinks, launchableAgents, nodeRefs, onSelectAgent, onSpawnAgent, onOpenWorkspaceContextMenu, onLabelHoverChange, onLabelFocusChange, onAgentKeyDown }) {
	const [detailsOpen, setDetailsOpen] = (0, import_react.useState)(false);
	const exiting = project.motionState === "exiting" || worktree.motionState === "exiting";
	const selected = worktree.agents.some((agent) => agent.card.paneKey === selectedPaneKey);
	const activeStatus = agentMapWorktreeActiveStatus(worktree.statusCounts);
	const aggregate = !selected && shouldAggregateAgentMapWorktree(worktree, zoom, allowAggregation);
	const agentsByPaneKey = new Map(worktree.agents.map((agent) => [agent.card.paneKey, agent]));
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Popover, {
		open: detailsOpen && !exiting,
		onOpenChange: setDetailsOpen,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("g", {
			className: `agent-map-worktree-group${worktree.motionState ? ` is-${worktree.motionState}` : ""}${held ? " is-held" : ""}`,
			"data-agent-map-worktree-id": worktree.id,
			"aria-hidden": exiting || void 0,
			onPointerEnter: () => onLabelHoverChange(worktree.id, true),
			onPointerLeave: () => onLabelHoverChange(worktree.id, false),
			onFocus: () => onLabelFocusChange(worktree.id, true),
			onBlur: (event) => {
				if (!event.currentTarget.contains(event.relatedTarget)) onLabelFocusChange(worktree.id, false);
			},
			children: [
				activeStatus ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", {
					className: `agent-map-worktree-status-glow fleet-status-${activeStatus}`,
					"data-agent-map-worktree-status-glow": "",
					"data-worktree-active-status": activeStatus,
					cx: worktree.x,
					cy: worktree.y,
					r: worktree.radius,
					"aria-hidden": "true"
				}) : null,
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(PopoverTrigger, {
					asChild: true,
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", {
						className: `agent-map-worktree-ring${activeStatus ? ` is-${activeStatus}` : ""}${selected ? " is-selected" : ""}${detailsOpen ? " is-open" : ""}`,
						"data-agent-map-worktree": "",
						"data-agent-count": worktree.agents.length,
						cx: worktree.x,
						cy: worktree.y,
						r: worktree.radius,
						role: "button",
						tabIndex: exiting ? -1 : 0,
						"aria-hidden": exiting || void 0,
						"aria-label": worktree.workspaceKind === "folder" ? translate("dashboardPopout.map.openFolderWorkspace", "Open {{workspace}} folder workspace details", { workspace: worktree.name }) : translate("dashboardPopout.map.openWorktree", "Open {{worktree}} worktree details", { worktree: worktree.name }),
						onKeyDown: (event) => {
							if (exiting) return;
							if (event.key === "Enter" || event.key === " ") {
								event.preventDefault();
								setDetailsOpen((open) => !open);
							}
						},
						onContextMenu: onOpenWorkspaceContextMenu && !exiting ? (event) => {
							event.preventDefault();
							event.stopPropagation();
							setDetailsOpen(false);
							onOpenWorkspaceContextMenu(event, worktree);
						} : void 0
					})
				}),
				aggregate ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("g", {
					className: "agent-map-aggregate-node",
					transform: `translate(${worktree.x} ${worktree.y + 7})`,
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", { r: Math.min(26, 12 + Math.sqrt(worktree.agents.length) * 2) }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("text", {
						y: 3,
						children: worktree.agents.length
					})]
				}) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("g", {
					className: "agent-map-lineage-links",
					"aria-hidden": true,
					children: (showOrchestrationLinks ? worktree.agents : []).map((child) => {
						const parent = child.card.parentPaneKey ? agentsByPaneKey.get(child.card.parentPaneKey) : void 0;
						if (!parent || parent.card.paneKey === child.card.paneKey) return null;
						return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", {
							className: `agent-map-lineage-link${child.motionState === "exiting" || parent.motionState === "exiting" ? " is-exiting" : child.motionState === "entering" || parent.motionState === "entering" ? " is-entering" : ""}`,
							"data-agent-map-lineage-link": "",
							"data-agent-map-lineage-relation": AGENT_MAP_LINEAGE_RELATION,
							"data-parent-pane-key": parent.card.paneKey,
							"data-child-pane-key": child.card.paneKey,
							d: lineagePath(parent, child)
						}, child.card.paneKey);
					})
				}), worktree.agents.map((agent) => {
					const iconSize = Math.max(12, Math.min(22, agent.radius * 1.05));
					const agentExiting = exiting || agent.motionState === "exiting";
					const hasStatusGlow = agent.status === "working" || agent.status === "waiting" || agent.status === "blocked";
					return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("g", {
						ref: (node) => {
							if (node) nodeRefs.current.set(agent.card.paneKey, node);
							else nodeRefs.current.delete(agent.card.paneKey);
						},
						"data-agent-map-agent": "",
						"data-agent-provider": agent.card.agentType,
						role: "button",
						tabIndex: agentExiting ? -1 : 0,
						"aria-hidden": agentExiting || void 0,
						"aria-pressed": selectedPaneKey === agent.card.paneKey,
						"aria-label": `${agentName(agent.card)}, ${agentStateLabel(agent.status)}${agent.card.unseen ? ", unread" : ""}, ${formatDuration(agent.durationMinutes)}, ${worktree.name}, ${project.name}`,
						className: `agent-map-agent-node fleet-status-${agent.status}${selectedPaneKey === agent.card.paneKey ? " is-selected" : ""}${agent.motionState ? ` is-${agent.motionState}` : ""}`,
						transform: `translate(${agent.x} ${agent.y})`,
						onClick: (event) => {
							if (agentExiting) return;
							event.currentTarget.focus();
							onSelectAgent(agent.card);
						},
						onKeyDown: (event) => {
							if (!agentExiting) onAgentKeyDown(event, agent);
						},
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("g", {
							className: "agent-map-agent-visual",
							children: [
								hasStatusGlow ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", {
									className: `agent-map-agent-status-glow fleet-status-${agent.status}`,
									"data-agent-map-agent-status-glow": "",
									"data-agent-active-status": agent.status,
									r: agent.radius + 1,
									"aria-hidden": "true"
								}) : null,
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", {
									className: "agent-map-agent-hit",
									r: Math.max(10, agent.radius + 3)
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", {
									className: "agent-map-agent-mark",
									r: agent.radius
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("foreignObject", {
									className: "agent-map-agent-icon",
									x: -iconSize / 2,
									y: -iconSize / 2,
									width: iconSize,
									height: iconSize,
									children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentIcon, {
										agent: agentTypeToIconAgent(agent.card.agentType),
										size: iconSize
									}) })
								}),
								agent.card.unseen ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", {
									className: "agent-map-agent-unread-mark",
									"data-agent-unread-marker": "",
									cx: -agent.radius * Math.SQRT1_2,
									cy: -agent.radius * Math.SQRT1_2,
									r: agent.radius * .225 * agentMapAttentionMarkerScale(mapScale),
									vectorEffect: "none",
									"aria-hidden": "true"
								}) : null
							]
						})
					}, agent.card.paneKey);
				})] })
			]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorktreeDetails, {
			project,
			worktree,
			launchableAgents,
			onSelectAgent,
			onSpawnAgent,
			onDone: () => setDetailsOpen(false)
		})]
	});
});
function worktreeLineagePath(parent, child) {
	const startY = parent.y + parent.radius;
	const endY = child.y - child.radius;
	const branchY = (startY + endY) / 2;
	return `M ${parent.x} ${startY} C ${parent.x} ${branchY} ${child.x} ${branchY} ${child.x} ${endY}`;
}
function agentLineagePath(parent, child) {
	return agentMapDirectLineageChevronPath(parent, child);
}
const AgentMapScene = (0, import_react.memo)(function AgentMapScene$1({ layout, repoIconsByRepoId, zoom, labelScale, mapScale, heldProjectId, heldWorktreeId, selectedPaneKey, allowAggregation, showOrchestrationLinks, launchableAgentsByWorktreeId, nodeRefs, onSelectAgent, onSpawnAgent, onOpenProjectContextMenu, onOpenWorkspaceContextMenu, onAgentKeyDown }) {
	const [hoveredWorktreeId, setHoveredWorktreeId] = (0, import_react.useState)(null);
	const [focusedWorktreeId, setFocusedWorktreeId] = (0, import_react.useState)(null);
	const activeWorktreeId = heldWorktreeId ?? hoveredWorktreeId ?? focusedWorktreeId;
	const handleLabelHoverChange = (0, import_react.useCallback)((worktreeId, active) => {
		setHoveredWorktreeId((current) => active ? worktreeId : current === worktreeId ? null : current);
	}, []);
	const handleLabelFocusChange = (0, import_react.useCallback)((worktreeId, active) => {
		setFocusedWorktreeId((current) => active ? worktreeId : current === worktreeId ? null : current);
	}, []);
	const visibleLabels = (0, import_react.useMemo)(() => selectVisibleAgentMapLabels(layout, labelScale, mapScale), [
		labelScale,
		layout,
		mapScale
	]);
	const activeWorktree = (0, import_react.useMemo)(() => {
		if (!activeWorktreeId) return null;
		for (const project of layout.projects) for (const worktree of project.worktrees) if (worktree.id === activeWorktreeId) return worktree;
		return null;
	}, [activeWorktreeId, layout]);
	const visibleAgentsByPaneKey = (0, import_react.useMemo)(() => {
		const agents = /* @__PURE__ */ new Map();
		for (const project of layout.projects) for (const worktree of project.worktrees) {
			if (!worktree.agents.some((agent) => agent.card.paneKey === selectedPaneKey) && shouldAggregateAgentMapWorktree(worktree, zoom, allowAggregation)) continue;
			for (const agent of worktree.agents) agents.set(agent.card.paneKey, {
				agent,
				worktreeId: worktree.id
			});
		}
		return agents;
	}, [
		allowAggregation,
		layout,
		selectedPaneKey,
		zoom
	]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [layout.projects.map((project) => {
		const worktreesById = new Map(project.worktrees.map((worktree) => [worktree.id, worktree]));
		const projectLabelHalfWidth = project.radius * mapScale;
		const projectHostsById = /* @__PURE__ */ new Map();
		for (const worktree of project.worktrees) if (worktree.hostKind === "ssh" || worktree.hostKind === "remote") projectHostsById.set(`${worktree.hostKind}:${worktree.executionHostId ?? ""}`, worktree);
		const projectHosts = [...projectHostsById.values()];
		const projectCountText = translate("dashboardPopout.map.projectCount", "{{agents}} agents · {{workspaces}} workspaces", {
			agents: project.agentCount,
			workspaces: project.worktrees.length
		}).toUpperCase();
		const crossWorktreeLineage = !showOrchestrationLinks ? [] : project.worktrees.flatMap((worktree) => worktree.agents.flatMap((child) => {
			const parent = child.card.parentPaneKey ? visibleAgentsByPaneKey.get(child.card.parentPaneKey) : void 0;
			const childLocation = visibleAgentsByPaneKey.get(child.card.paneKey);
			return parent && childLocation && parent.worktreeId !== childLocation.worktreeId ? [{
				parent: parent.agent,
				child
			}] : [];
		}));
		return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("g", {
			className: `agent-map-project-node${project.motionState ? ` is-${project.motionState}` : ""}${heldProjectId === project.id ? " is-held" : ""}`,
			"data-agent-map-project-id": project.id,
			"aria-hidden": project.motionState === "exiting" || void 0,
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", {
					className: "agent-map-project-ring",
					"data-agent-map-project": "",
					cx: project.x,
					cy: project.y,
					r: project.radius,
					onContextMenu: onOpenProjectContextMenu ? (event) => {
						event.preventDefault();
						event.stopPropagation();
						onOpenProjectContextMenu(event, project);
					} : void 0
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("g", {
					className: "agent-map-worktree-lineage-links",
					"aria-hidden": true,
					children: project.worktrees.map((child) => {
						const parent = child.parentId ? worktreesById.get(child.parentId) : void 0;
						return !parent || child.y <= parent.y ? null : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", {
							className: `agent-map-worktree-lineage-link${child.motionState === "exiting" || parent.motionState === "exiting" ? " is-exiting" : child.motionState === "entering" || parent.motionState === "entering" ? " is-entering" : ""}`,
							"data-agent-map-worktree-lineage-link": "",
							"data-parent-worktree-id": parent.worktreeId,
							"data-child-worktree-id": child.worktreeId,
							d: worktreeLineagePath(parent, child)
						}, child.id);
					})
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("g", {
					className: "agent-map-lineage-links",
					"aria-hidden": true,
					children: crossWorktreeLineage.map(({ parent, child }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", {
						className: `agent-map-lineage-link is-cross-worktree${parent.motionState === "exiting" || child.motionState === "exiting" ? " is-exiting" : parent.motionState === "entering" || child.motionState === "entering" ? " is-entering" : ""}`,
						"data-agent-map-lineage-link": "",
						"data-agent-map-cross-worktree-lineage-link": "",
						"data-agent-map-lineage-relation": AGENT_MAP_LINEAGE_RELATION,
						"data-parent-pane-key": parent.card.paneKey,
						"data-child-pane-key": child.card.paneKey,
						d: agentLineagePath(parent, child)
					}, child.card.paneKey))
				}),
				project.worktrees.map((worktree) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentMapWorktreeRingNode, {
					project,
					worktree,
					zoom,
					mapScale,
					held: heldWorktreeId === worktree.id,
					selectedPaneKey,
					allowAggregation,
					showOrchestrationLinks,
					launchableAgents: launchableAgentsByWorktreeId?.[worktree.worktreeId],
					nodeRefs,
					onSelectAgent,
					onSpawnAgent,
					onOpenWorkspaceContextMenu,
					onLabelHoverChange: handleLabelHoverChange,
					onLabelFocusChange: handleLabelFocusChange,
					onAgentKeyDown
				}, worktree.id)),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("g", {
					className: "agent-map-worktree-label-layer",
					children: project.worktrees.map((worktree) => worktree.id === activeWorktreeId ? null : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentMapWorktreeLabel, {
						worktree,
						visible: visibleLabels.worktreeIds.has(worktree.id),
						active: false,
						labelScale,
						mapScale
					}, worktree.id))
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("g", {
					transform: `translate(${project.x} ${project.y - project.radius}) scale(${labelScale})`,
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("foreignObject", {
						className: "agent-map-project-label-frame",
						x: -projectLabelHalfWidth,
						y: 3,
						width: projectLabelHalfWidth * 2,
						height: 18,
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "agent-map-project-label",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RepoIconGlyph, {
									repoIcon: repoIconsByRepoId?.[project.id] ?? null,
									className: "size-3 shrink-0",
									iconClassName: "size-3"
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "agent-map-project-name min-w-0 truncate",
									children: project.name.toUpperCase()
								}),
								projectHosts.map((host) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DashboardHostBadge, {
									hostKind: host.hostKind,
									executionHostId: host.executionHostId,
									hostLabel: host.hostLabel,
									keyboardFocusable: true,
									className: "agent-map-project-host-badge"
								}, `${host.hostKind}:${host.executionHostId ?? ""}`))
							]
						})
					}), visibleLabels.projectCountIds.has(project.id) ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("text", {
						className: "agent-map-project-count",
						y: 32,
						children: projectCountText
					}) : null]
				})
			]
		}, project.id);
	}), activeWorktree ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("g", {
		className: "agent-map-worktree-hover-label-layer",
		"data-agent-map-hover-label": "",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentMapWorktreeLabel, {
			worktree: activeWorktree,
			visible: visibleLabels.worktreeIds.has(activeWorktree.id),
			active: true,
			labelScale,
			mapScale
		})
	}) : null] });
});
const MIN_ZOOM = .7;
var AGENT_FOCUS_RADIUS_PX = 24;
function clamp(value, minimum, maximum) {
	return Math.max(minimum, Math.min(maximum, value));
}
function agentFocusZoom(layout, width, height) {
	const aspect = width / Math.max(1, height);
	const baseWidth = Math.max(layout.width, layout.height * aspect);
	return clamp(Math.max(2, baseWidth * AGENT_FOCUS_RADIUS_PX / (Math.max(1, width) * 20)), MIN_ZOOM, 24);
}
function AgentMapViewportControls({ zoom, onFit, onZoomIn, onZoomOut }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "absolute bottom-3 left-3 flex items-center gap-0.5 rounded-md border border-border bg-card p-0.5 shadow-xs",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				type: "button",
				variant: "ghost",
				size: "icon-xs",
				"aria-label": translate("dashboardPopout.map.zoomOut", "Zoom out"),
				onClick: onZoomOut,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Minus, { className: "size-3" })
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
				className: "min-w-12 text-center text-[10px] text-muted-foreground tabular-nums",
				children: [Math.round(zoom * 100), "%"]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				type: "button",
				variant: "ghost",
				size: "icon-xs",
				"aria-label": translate("dashboardPopout.map.zoomIn", "Zoom in"),
				onClick: onZoomIn,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Plus, { className: "size-3" })
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
				type: "button",
				variant: "ghost",
				size: "xs",
				onClick: onFit,
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Focus, { className: "size-3" }), translate("dashboardPopout.map.fit", "Fit")]
			})
		]
	});
}
function agentMapAgents(layout) {
	return layout.projects.flatMap((project) => project.worktrees.flatMap((worktree) => worktree.agents));
}
function navigableAgentMapAgents(layout, zoom, allowAggregation, selectedPaneKey) {
	return layout.projects.flatMap((project) => project.worktrees.flatMap((worktree) => {
		return !worktree.agents.some((agent) => agent.card.paneKey === selectedPaneKey) && shouldAggregateAgentMapWorktree(worktree, zoom, allowAggregation) ? [] : worktree.agents;
	}));
}
function nextDirectionalAgent(current, agents, direction) {
	let best = null;
	for (const candidate of agents) {
		if (candidate.card.paneKey === current.card.paneKey) continue;
		const dx = candidate.x - current.x;
		const dy = candidate.y - current.y;
		const forward = dx * direction.x + dy * direction.y;
		if (forward <= 0) continue;
		const score = forward + Math.abs(dx * direction.y - dy * direction.x) * 2;
		if (!best || score < best.score) best = {
			agent: candidate,
			score
		};
	}
	return best?.agent ?? null;
}
function AgentMapSnapshotWorkspaceMenu({ request, onOpenChange, onSpawnAgent, onSleepWorkspace }) {
	const triggerRef = (0, import_react.useRef)(null);
	(0, import_react.useEffect)(() => {
		triggerRef.current?.dispatchEvent(new MouseEvent("contextmenu", {
			bubbles: true,
			cancelable: true,
			clientX: request.clientX,
			clientY: request.clientY,
			button: 2
		}));
	}, [request]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "pointer-events-none absolute inset-0",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ContextMenu, {
			onOpenChange,
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ContextMenuTrigger, {
				asChild: true,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					ref: triggerRef,
					"aria-hidden": true
				})
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ContextMenuContent, { children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ContextMenuLabel, {
					className: "truncate",
					children: request.worktreeName
				}),
				onSpawnAgent ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ContextMenuSub, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ContextMenuSubTrigger, {
					disabled: request.launchableAgents.length === 0,
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Plus, { className: "size-3.5" }), translate("dashboardPopout.map.spawnAgent", "Start a new agent")]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ContextMenuSubContent, { children: request.launchableAgents.map((agent) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ContextMenuItem, {
					onSelect: () => onSpawnAgent({
						worktreeId: request.worktreeId,
						agent
					}),
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentIcon, {
						agent,
						size: 14
					}), getAgentLabel(agent)]
				}, agent)) })] }) : null,
				onSpawnAgent && onSleepWorkspace ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ContextMenuSeparator, {}) : null,
				onSleepWorkspace ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ContextMenuItem, {
					onSelect: () => onSleepWorkspace({ worktreeId: request.worktreeId }),
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Moon, { className: "size-3.5" }), translate("dashboardPopout.map.sleepWorkspace", "Sleep")]
				}) : null
			] })]
		})
	});
}
var AgentMapProjectContextMenu = lazyWithRetry(() => __vitePreload(() => import("./AgentMapProjectContextMenu-DBIl0DQp.js"), __vite__mapDeps([0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31]), import.meta.url).then((module) => ({ default: module.AgentMapProjectContextMenu })), { reloadKey: "agent-map-project-context-menu" });
function AgentMapProjectContextMenuLoader({ request, onOpenChange }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(import_react.Suspense, {
		fallback: null,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentMapProjectContextMenu, {
			request,
			onOpenChange
		})
	});
}
var AgentMapWorkspaceContextMenu = lazyWithRetry(() => __vitePreload(() => import("./AgentMapWorkspaceContextMenu-l4QeMc3y.js"), __vite__mapDeps([32,3,1,2,4,5,22,23,24,15,25,26,27,28,29,33,34,35]), import.meta.url).then((module) => ({ default: module.AgentMapWorkspaceContextMenu })), { reloadKey: "agent-map-workspace-context-menu" });
function AgentMapWorkspaceContextMenuLoader({ request, onOpenChange, onLifecycleComplete }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(import_react.Suspense, {
		fallback: null,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentMapWorkspaceContextMenu, {
			request,
			onOpenChange,
			onLifecycleComplete
		})
	});
}
function useAgentMapContextMenus({ enabled, launchableAgentsByWorktreeId, onOpenChange, onSpawnAgent, onSleepWorkspace }) {
	const requestIdRef = (0, import_react.useRef)(0);
	const [workspaceRequest, setWorkspaceRequest] = (0, import_react.useState)(null);
	const [projectRequest, setProjectRequest] = (0, import_react.useState)(null);
	const [snapshotRequest, setSnapshotRequest] = (0, import_react.useState)(null);
	const snapshotMenuEnabled = !enabled && (onSpawnAgent !== void 0 || onSleepWorkspace !== void 0);
	const openSnapshotWorkspaceMenu = (0, import_react.useCallback)((event, worktree) => {
		requestIdRef.current += 1;
		setSnapshotRequest({
			id: requestIdRef.current,
			worktreeId: worktree.worktreeId,
			worktreeName: worktree.name,
			launchableAgents: launchableAgentsByWorktreeId?.[worktree.worktreeId] ?? [],
			clientX: event.clientX,
			clientY: event.clientY
		});
	}, [launchableAgentsByWorktreeId]);
	const openWorkspaceContextMenu = (0, import_react.useCallback)((event, worktree) => {
		requestIdRef.current += 1;
		setProjectRequest(null);
		setWorkspaceRequest({
			id: requestIdRef.current,
			worktreeId: worktree.worktreeId,
			executionHostId: worktree.executionHostId,
			clientX: event.clientX,
			clientY: event.clientY,
			altKey: event.altKey
		});
	}, []);
	const openProjectContextMenu = (0, import_react.useCallback)((event, project) => {
		requestIdRef.current += 1;
		setWorkspaceRequest(null);
		setProjectRequest({
			id: requestIdRef.current,
			projectId: project.id,
			clientX: event.clientX,
			clientY: event.clientY
		});
	}, []);
	const handleWorkspaceLifecycleComplete = (0, import_react.useCallback)(() => {
		setWorkspaceRequest(null);
	}, []);
	const handleProjectOpenChange = (0, import_react.useCallback)((open) => {
		onOpenChange?.(open);
		if (!open) setProjectRequest(null);
	}, [onOpenChange]);
	const handleSnapshotOpenChange = (0, import_react.useCallback)((open) => {
		onOpenChange?.(open);
		if (!open) setSnapshotRequest(null);
	}, [onOpenChange]);
	return {
		contextMenus: snapshotMenuEnabled ? snapshotRequest ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentMapSnapshotWorkspaceMenu, {
			request: snapshotRequest,
			onOpenChange: handleSnapshotOpenChange,
			onSpawnAgent,
			onSleepWorkspace
		}) : null : enabled ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [workspaceRequest ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentMapWorkspaceContextMenuLoader, {
			request: workspaceRequest,
			onOpenChange,
			onLifecycleComplete: handleWorkspaceLifecycleComplete
		}) : null, projectRequest ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentMapProjectContextMenuLoader, {
			request: projectRequest,
			onOpenChange: handleProjectOpenChange
		}) : null] }) : null,
		onOpenProjectContextMenu: enabled ? openProjectContextMenu : void 0,
		onOpenWorkspaceContextMenu: enabled ? openWorkspaceContextMenu : snapshotMenuEnabled ? openSnapshotWorkspaceMenu : void 0
	};
}
function useAgentMapCanvasSize(containerRef, onResize) {
	const [size, setSize] = (0, import_react.useState)({
		width: 800,
		height: 560
	});
	(0, import_react.useEffect)(() => {
		const container = containerRef.current;
		if (!container || typeof ResizeObserver === "undefined") return;
		const measure = () => {
			const next = container.getBoundingClientRect();
			if (next.width <= 0 || next.height <= 0) return;
			onResize();
			setSize((current) => current.width === next.width && current.height === next.height ? current : {
				width: next.width,
				height: next.height
			});
		};
		measure();
		const observer = new ResizeObserver(measure);
		observer.observe(container);
		return () => observer.disconnect();
	}, [containerRef, onResize]);
	return size;
}
function closestId(target, attribute) {
	return target.closest(`[${attribute}]`)?.getAttribute(attribute) ?? null;
}
function useAgentMapPointerHold(dragRef) {
	const [held, setHeld] = (0, import_react.useState)(null);
	const hold = (0, import_react.useCallback)((target) => {
		const projectId = closestId(target, "data-agent-map-project-id");
		const worktreeId = closestId(target, "data-agent-map-worktree-id");
		setHeld(projectId === null && worktreeId === null ? null : {
			projectId,
			worktreeId
		});
	}, []);
	const release = (0, import_react.useCallback)(() => {
		setHeld((current) => current === null ? current : null);
	}, []);
	return {
		held,
		hold,
		release,
		clearDrag: (0, import_react.useCallback)((pointerId) => {
			if (dragRef.current?.pointerId !== pointerId) return false;
			dragRef.current = null;
			release();
			return true;
		}, [dragRef, release])
	};
}
function allAgentIds(layout) {
	return new Set(layout.projects.flatMap((project) => project.worktrees.flatMap((worktree) => worktree.agents.map((agent) => agent.card.paneKey))));
}
function allWorktreeIds(layout) {
	return new Set(layout.projects.flatMap((project) => project.worktrees.map((worktree) => worktree.id)));
}
function retainMotionState(previous, next) {
	return {
		...next,
		motionState: !previous ? "entering" : previous.motionState === "entering" ? "entering" : void 0
	};
}
function reconcileAgents(previous, next, nextAgentIds) {
	const previousById = new Map(previous.agents.map((agent) => [agent.card.paneKey, agent]));
	const nextIds = new Set(next.agents.map((agent) => agent.card.paneKey));
	const agents = next.agents.map((agent) => retainMotionState(previousById.get(agent.card.paneKey), agent));
	for (const agent of previous.agents) if (!nextIds.has(agent.card.paneKey) && !nextAgentIds.has(agent.card.paneKey)) agents.push({
		...agent,
		motionState: "exiting"
	});
	return agents;
}
function enteringWorktree(worktree) {
	return {
		...worktree,
		motionState: "entering",
		agents: worktree.agents.map((agent) => ({
			...agent,
			motionState: void 0
		}))
	};
}
function exitingWorktree(worktree) {
	return {
		...worktree,
		motionState: "exiting",
		agents: worktree.agents.map((agent) => ({
			...agent,
			motionState: void 0
		}))
	};
}
function reconcileWorktrees(previous, next, nextAgentIds, nextWorktreeIds) {
	const previousById = new Map(previous.worktrees.map((worktree) => [worktree.id, worktree]));
	const nextIds = new Set(next.worktrees.map((worktree) => worktree.id));
	const worktrees = next.worktrees.map((worktree) => {
		const previousWorktree = previousById.get(worktree.id);
		if (!previousWorktree) return enteringWorktree(worktree);
		return {
			...retainMotionState(previousWorktree, worktree),
			agents: reconcileAgents(previousWorktree, worktree, nextAgentIds)
		};
	});
	for (const worktree of previous.worktrees) if (!nextIds.has(worktree.id) && !nextWorktreeIds.has(worktree.id)) worktrees.push(exitingWorktree(worktree));
	return worktrees;
}
function enteringProject(project) {
	return {
		...project,
		motionState: "entering",
		worktrees: project.worktrees.map((worktree) => ({
			...worktree,
			motionState: void 0,
			agents: worktree.agents.map((agent) => ({
				...agent,
				motionState: void 0
			}))
		}))
	};
}
function exitingProject(project) {
	return {
		...project,
		motionState: "exiting",
		worktrees: project.worktrees.map((worktree) => ({
			...worktree,
			motionState: void 0,
			agents: worktree.agents.map((agent) => ({
				...agent,
				motionState: void 0
			}))
		}))
	};
}
function reconcileAgentMapMotionLayout(previous, next) {
	const previousById = new Map(previous.projects.map((project) => [project.id, project]));
	const nextProjectIds = new Set(next.projects.map((project) => project.id));
	const nextAgentIds = allAgentIds(next);
	const nextWorktreeIds = allWorktreeIds(next);
	const projects = next.projects.map((project) => {
		const previousProject = previousById.get(project.id);
		if (!previousProject) return enteringProject(project);
		return {
			...retainMotionState(previousProject, project),
			worktrees: reconcileWorktrees(previousProject, project, nextAgentIds, nextWorktreeIds)
		};
	});
	for (const project of previous.projects) if (!nextProjectIds.has(project.id)) projects.push(exitingProject(project));
	return {
		...next,
		projects
	};
}
function motionNodeSignature(layout, motionState) {
	const nodeIds = layout.projects.flatMap((project) => [...project.motionState === motionState ? [`project:${project.id}`] : [], ...project.worktrees.flatMap((worktree) => [...worktree.motionState === motionState ? [`worktree:${worktree.id}`] : [], ...worktree.agents.filter((agent) => agent.motionState === motionState).map((agent) => `agent:${agent.card.paneKey}`)])]);
	return nodeIds.length > 0 ? JSON.stringify(nodeIds) : "";
}
function clearEnteringAgentMapLayout(layout) {
	return {
		...layout,
		projects: layout.projects.map((project) => ({
			...project,
			motionState: project.motionState === "entering" ? void 0 : project.motionState,
			worktrees: project.worktrees.map((worktree) => ({
				...worktree,
				motionState: worktree.motionState === "entering" ? void 0 : worktree.motionState,
				agents: worktree.agents.map((agent) => ({
					...agent,
					motionState: agent.motionState === "entering" ? void 0 : agent.motionState
				}))
			}))
		}))
	};
}
function pruneExitingAgentMapLayout(layout) {
	return {
		...layout,
		projects: layout.projects.filter((project) => project.motionState !== "exiting").map((project) => ({
			...project,
			worktrees: project.worktrees.filter((worktree) => worktree.motionState !== "exiting").map((worktree) => ({
				...worktree,
				agents: worktree.agents.filter((agent) => agent.motionState !== "exiting")
			}))
		}))
	};
}
function useAgentMapMotionLayout(layout, reducedMotion) {
	const [motionState, setMotionState] = (0, import_react.useState)(() => ({
		inputLayout: layout,
		reducedMotion,
		motionLayout: layout
	}));
	const enterTimerRef = (0, import_react.useRef)(null);
	const exitTimerRef = (0, import_react.useRef)(null);
	let motionLayout = motionState.motionLayout;
	if (motionState.inputLayout !== layout || motionState.reducedMotion !== reducedMotion) {
		motionLayout = reducedMotion ? layout : reconcileAgentMapMotionLayout(motionState.motionLayout, layout);
		setMotionState({
			inputLayout: layout,
			reducedMotion,
			motionLayout
		});
	}
	const { enteringSignature, exitingSignature } = (0, import_react.useMemo)(() => ({
		enteringSignature: motionNodeSignature(motionLayout, "entering"),
		exitingSignature: motionNodeSignature(motionLayout, "exiting")
	}), [motionLayout]);
	(0, import_react.useEffect)(() => {
		if (enterTimerRef.current) {
			clearTimeout(enterTimerRef.current);
			enterTimerRef.current = null;
		}
		if (reducedMotion || !enteringSignature) return;
		enterTimerRef.current = setTimeout(() => {
			enterTimerRef.current = null;
			setMotionState((previous) => ({
				...previous,
				motionLayout: clearEnteringAgentMapLayout(previous.motionLayout)
			}));
		}, 420);
		return () => {
			if (enterTimerRef.current) {
				clearTimeout(enterTimerRef.current);
				enterTimerRef.current = null;
			}
		};
	}, [enteringSignature, reducedMotion]);
	(0, import_react.useEffect)(() => {
		if (exitTimerRef.current) {
			clearTimeout(exitTimerRef.current);
			exitTimerRef.current = null;
		}
		if (reducedMotion || !exitingSignature) return;
		exitTimerRef.current = setTimeout(() => {
			exitTimerRef.current = null;
			setMotionState((previous) => ({
				...previous,
				motionLayout: pruneExitingAgentMapLayout(previous.motionLayout)
			}));
		}, 260);
		return () => {
			if (exitTimerRef.current) {
				clearTimeout(exitTimerRef.current);
				exitTimerRef.current = null;
			}
		};
	}, [exitingSignature, reducedMotion]);
	return motionLayout;
}
function useAgentMapSelectedFocus({ agents, selectedPaneKey, viewportRef, resolveFocusZoom, animateViewport, stopViewportTransition }) {
	const focusedAgentRef = (0, import_react.useRef)(null);
	(0, import_react.useEffect)(() => {
		const selected = agents.find((agent) => agent.card.paneKey === selectedPaneKey);
		if (!selectedPaneKey || !selected) {
			focusedAgentRef.current = null;
			stopViewportTransition();
			return;
		}
		const targetZoom = resolveFocusZoom();
		const focused = focusedAgentRef.current;
		if (focused?.paneKey === selectedPaneKey && focused.x === selected.x && focused.y === selected.y && focused.zoom === targetZoom) return;
		focusedAgentRef.current = {
			paneKey: selectedPaneKey,
			x: selected.x,
			y: selected.y,
			zoom: targetZoom
		};
		animateViewport(viewportRef.current, {
			center: {
				x: selected.x,
				y: selected.y
			},
			zoom: targetZoom
		});
	}, [
		agents,
		animateViewport,
		resolveFocusZoom,
		selectedPaneKey,
		stopViewportTransition,
		viewportRef
	]);
}
function interpolate(from, to, progress) {
	return from + (to - from) * progress;
}
function startAgentMapViewportTransition({ from, to, durationMs, onFrame, onComplete }) {
	let frameId = null;
	let startedAt = null;
	let cancelled = false;
	const tick = (now) => {
		if (cancelled) return;
		startedAt ?? (startedAt = now);
		const progress = Math.min(1, (now - startedAt) / durationMs);
		const eased = 1 - (1 - progress) ** 3;
		onFrame({
			center: {
				x: interpolate(from.center.x, to.center.x, eased),
				y: interpolate(from.center.y, to.center.y, eased)
			},
			zoom: interpolate(from.zoom, to.zoom, eased)
		});
		if (progress < 1) frameId = requestAnimationFrame(tick);
		else {
			frameId = null;
			onComplete?.();
		}
	};
	frameId = requestAnimationFrame(tick);
	return () => {
		cancelled = true;
		if (frameId !== null) cancelAnimationFrame(frameId);
	};
}
function useAgentMapViewportTransition({ durationMs, reducedMotion, onFrame }) {
	const cancelRef = (0, import_react.useRef)(null);
	const stop = (0, import_react.useCallback)(() => {
		cancelRef.current?.();
		cancelRef.current = null;
	}, []);
	const animate = (0, import_react.useCallback)((from, to) => {
		stop();
		if (reducedMotion) {
			onFrame(to);
			return;
		}
		let cancel = () => {};
		cancel = startAgentMapViewportTransition({
			from,
			to,
			durationMs,
			onFrame,
			onComplete: () => {
				if (cancelRef.current === cancel) cancelRef.current = null;
			}
		});
		cancelRef.current = cancel;
	}, [
		durationMs,
		onFrame,
		reducedMotion,
		stop
	]);
	(0, import_react.useEffect)(() => stop, [stop]);
	return {
		animate,
		stop
	};
}
var AGENT_FOCUS_DURATION_MS = 240;
const AgentMapCanvas = (0, import_react.forwardRef)(function AgentMapCanvas$1({ layout, repoIconsByRepoId, selectedPaneKey, allowAggregation, showOrchestrationLinks, launchableAgentsByWorktreeId, workspaceContextMenusEnabled = false, onWorkspaceContextMenuOpenChange, onSelectAgent, onSpawnAgent, onSleepWorkspace }, forwardedRef) {
	const containerRef = (0, import_react.useRef)(null);
	const svgRef = (0, import_react.useRef)(null);
	const nodeRefs = (0, import_react.useRef)(/* @__PURE__ */ new Map());
	const dragRef = (0, import_react.useRef)(null);
	const viewportFrameRef = (0, import_react.useRef)(null);
	const pendingViewportRef = (0, import_react.useRef)(null);
	const interactionBoundsRef = (0, import_react.useRef)(null);
	const hasShownProjectsRef = (0, import_react.useRef)(layout.projects.length > 0);
	const { held, hold, release: releaseHold, clearDrag } = useAgentMapPointerHold(dragRef);
	const size = useAgentMapCanvasSize(containerRef, (0, import_react.useCallback)(() => {
		interactionBoundsRef.current = null;
	}, []));
	const [viewport, setViewport] = (0, import_react.useState)({
		center: {
			x: layout.width / 2,
			y: layout.height / 2
		},
		zoom: 1
	});
	const prefersReducedMotion = usePrefersReducedMotion();
	const motionLayout = useAgentMapMotionLayout(layout, prefersReducedMotion);
	const viewportRef = (0, import_react.useRef)(viewport);
	const { contextMenus, onOpenProjectContextMenu, onOpenWorkspaceContextMenu } = useAgentMapContextMenus({
		enabled: workspaceContextMenusEnabled,
		launchableAgentsByWorktreeId,
		onOpenChange: onWorkspaceContextMenuOpenChange,
		onSpawnAgent,
		onSleepWorkspace
	});
	const { center, zoom } = viewport;
	const agents = (0, import_react.useMemo)(() => agentMapAgents(layout), [layout]);
	const navigableAgents = (0, import_react.useMemo)(() => navigableAgentMapAgents(layout, zoom, allowAggregation, selectedPaneKey), [
		allowAggregation,
		layout,
		selectedPaneKey,
		zoom
	]);
	const hasProjects = layout.projects.length > 0;
	const hasMotionProjects = motionLayout.projects.length > 0;
	const aspect = size.width / Math.max(1, size.height);
	const baseWidth = Math.max(layout.width, layout.height * aspect);
	const baseHeight = baseWidth / aspect;
	const viewWidth = baseWidth / zoom;
	const viewHeight = baseHeight / zoom;
	const mapScale = size.width / viewWidth;
	const labelScale = Math.max(1, 1 / mapScale);
	const viewBox = `${center.x - viewWidth / 2} ${center.y - viewHeight / 2} ${viewWidth} ${viewHeight}`;
	const focusZoom = agentFocusZoom(layout, size.width, size.height);
	const resolveFocusZoom = (0, import_react.useCallback)(() => {
		const bounds = containerRef.current?.getBoundingClientRect();
		return bounds && bounds.width > 0 && bounds.height > 0 ? agentFocusZoom(layout, bounds.width, bounds.height) : focusZoom;
	}, [focusZoom, layout]);
	const commitViewport = (0, import_react.useCallback)((next) => {
		viewportRef.current = next;
		pendingViewportRef.current = null;
		interactionBoundsRef.current = null;
		setViewport(next);
	}, []);
	const { animate: animateViewport, stop: stopViewportTransition } = useAgentMapViewportTransition({
		durationMs: AGENT_FOCUS_DURATION_MS,
		reducedMotion: prefersReducedMotion,
		onFrame: commitViewport
	});
	useAgentMapSelectedFocus({
		agents,
		selectedPaneKey,
		viewportRef,
		resolveFocusZoom,
		animateViewport,
		stopViewportTransition
	});
	const applyViewport = (0, import_react.useCallback)((next) => {
		stopViewportTransition();
		commitViewport(next);
	}, [commitViewport, stopViewportTransition]);
	const scheduleViewport = (0, import_react.useCallback)((next) => {
		stopViewportTransition();
		viewportRef.current = next;
		pendingViewportRef.current = next;
		if (viewportFrameRef.current !== null) return;
		viewportFrameRef.current = requestAnimationFrame(() => {
			viewportFrameRef.current = null;
			interactionBoundsRef.current = null;
			const pending = pendingViewportRef.current;
			pendingViewportRef.current = null;
			if (pending) setViewport(pending);
		});
	}, [stopViewportTransition]);
	const fit = (0, import_react.useCallback)(() => {
		applyViewport({
			center: {
				x: layout.width / 2,
				y: layout.height / 2
			},
			zoom: 1
		});
	}, [
		applyViewport,
		layout.height,
		layout.width
	]);
	const focusProject = (0, import_react.useCallback)((project) => {
		const projectWidth = project.radius * 2.5;
		const projectHeight = project.radius * 2.5;
		applyViewport({
			center: {
				x: project.x,
				y: project.y
			},
			zoom: clamp(Math.min(baseWidth / projectWidth, baseHeight / projectHeight), MIN_ZOOM, 24)
		});
	}, [
		applyViewport,
		baseHeight,
		baseWidth
	]);
	(0, import_react.useImperativeHandle)(forwardedRef, () => ({
		fit,
		focusProject
	}), [fit, focusProject]);
	(0, import_react.useEffect)(() => {
		if (hasProjects && !hasShownProjectsRef.current) {
			hasShownProjectsRef.current = true;
			fit();
		}
	}, [fit, hasProjects]);
	(0, import_react.useEffect)(() => () => {
		if (viewportFrameRef.current !== null) cancelAnimationFrame(viewportFrameRef.current);
	}, []);
	const handleAgentKeyDown = (0, import_react.useCallback)((event, agent) => {
		if (event.key === "Enter" || event.key === " ") {
			event.preventDefault();
			onSelectAgent(agent.card);
			return;
		}
		const direction = event.key === "ArrowLeft" ? {
			x: -1,
			y: 0
		} : event.key === "ArrowRight" ? {
			x: 1,
			y: 0
		} : event.key === "ArrowUp" ? {
			x: 0,
			y: -1
		} : event.key === "ArrowDown" ? {
			x: 0,
			y: 1
		} : null;
		if (!direction) return;
		event.preventDefault();
		const next = nextDirectionalAgent(agent, navigableAgents, direction);
		nodeRefs.current.get(next?.card.paneKey ?? "")?.focus();
	}, [navigableAgents, onSelectAgent]);
	const zoomAt = (0, import_react.useCallback)((nextZoom, clientX, clientY) => {
		const clampedZoom = clamp(nextZoom, MIN_ZOOM, 24);
		if (clientX === void 0 || clientY === void 0) {
			applyViewport({
				...viewportRef.current,
				zoom: clampedZoom
			});
			return;
		}
		const bounds = interactionBoundsRef.current ?? svgRef.current?.getBoundingClientRect() ?? null;
		if (!bounds || bounds.width <= 0 || bounds.height <= 0) {
			applyViewport({
				...viewportRef.current,
				zoom: clampedZoom
			});
			return;
		}
		interactionBoundsRef.current = bounds;
		const current = viewportRef.current;
		const currentWidth = baseWidth / current.zoom;
		const currentHeight = baseHeight / current.zoom;
		const anchorX = current.center.x - currentWidth / 2 + (clientX - bounds.left) / bounds.width * currentWidth;
		const anchorY = current.center.y - currentHeight / 2 + (clientY - bounds.top) / bounds.height * currentHeight;
		const nextWidth = baseWidth / clampedZoom;
		const nextHeight = baseHeight / clampedZoom;
		const xRatio = (clientX - bounds.left) / bounds.width;
		const yRatio = (clientY - bounds.top) / bounds.height;
		scheduleViewport({
			center: {
				x: anchorX - (xRatio - .5) * nextWidth,
				y: anchorY - (yRatio - .5) * nextHeight
			},
			zoom: clampedZoom
		});
	}, [
		applyViewport,
		baseHeight,
		baseWidth,
		scheduleViewport
	]);
	(0, import_react.useEffect)(() => {
		if (!hasProjects) return;
		const svg = svgRef.current;
		if (!svg) return;
		const handleWheel = (event) => {
			event.preventDefault();
			zoomAt(viewportRef.current.zoom * Math.exp(-event.deltaY * .0015), event.clientX, event.clientY);
		};
		svg.addEventListener("wheel", handleWheel, { passive: false });
		return () => svg.removeEventListener("wheel", handleWheel);
	}, [hasProjects, zoomAt]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		ref: containerRef,
		className: "agent-map-canvas relative min-h-0 flex-1 overflow-hidden",
		children: [
			!hasMotionProjects ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "absolute inset-0 grid place-items-center text-center text-xs text-muted-foreground",
				children: translate("dashboardPopout.map.empty", "No agents match the current filters.")
			}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("svg", {
				ref: svgRef,
				className: "absolute inset-0 size-full cursor-grab touch-none select-none active:cursor-grabbing",
				viewBox,
				"aria-label": translate("dashboardPopout.map.canvasLabel", "Nested project, workspace, and agent map"),
				onPointerDown: (event) => {
					if (event.button !== 0 || dragRef.current) return;
					if (event.target.closest("[data-agent-map-agent], .agent-map-worktree-ring")) return;
					const bounds = event.currentTarget.getBoundingClientRect();
					if (bounds.width <= 0 || bounds.height <= 0) return;
					const current = viewportRef.current;
					dragRef.current = {
						pointerId: event.pointerId,
						point: {
							x: event.clientX,
							y: event.clientY
						},
						center: current.center,
						worldPerPixelX: baseWidth / current.zoom / bounds.width,
						worldPerPixelY: baseHeight / current.zoom / bounds.height
					};
					hold(event.target);
					event.currentTarget.setPointerCapture(event.pointerId);
				},
				onPointerMove: (event) => {
					const drag = dragRef.current;
					if (!drag) {
						releaseHold();
						return;
					}
					if (drag.pointerId !== event.pointerId) return;
					scheduleViewport({
						center: {
							x: drag.center.x - (event.clientX - drag.point.x) * drag.worldPerPixelX,
							y: drag.center.y - (event.clientY - drag.point.y) * drag.worldPerPixelY
						},
						zoom: viewportRef.current.zoom
					});
				},
				onPointerUp: (event) => {
					if (clearDrag(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
				},
				onPointerCancel: (event) => {
					clearDrag(event.pointerId);
				},
				onLostPointerCapture: (event) => {
					clearDrag(event.pointerId);
				},
				onPointerLeave: () => {
					if (!dragRef.current) releaseHold();
				},
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentMapScene, {
					layout: motionLayout,
					repoIconsByRepoId,
					zoom,
					labelScale,
					mapScale,
					heldProjectId: held?.projectId ?? null,
					heldWorktreeId: held?.worktreeId ?? null,
					selectedPaneKey,
					allowAggregation,
					showOrchestrationLinks,
					launchableAgentsByWorktreeId,
					nodeRefs,
					onSelectAgent,
					onSpawnAgent,
					onOpenProjectContextMenu,
					onOpenWorkspaceContextMenu,
					onAgentKeyDown: handleAgentKeyDown
				})
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentMapViewportControls, {
				zoom,
				onFit: fit,
				onZoomIn: () => zoomAt(viewportRef.current.zoom * 1.25),
				onZoomOut: () => zoomAt(viewportRef.current.zoom / 1.25)
			}),
			contextMenus
		]
	});
});
var HOST_FILTERS = [
	"all",
	"local",
	"ssh",
	"wsl",
	"remote"
];
var ALL_AGENT_STATES = new Set([
	"attention",
	"working",
	"done",
	"idle"
]);
var EMPTY_WORKSPACES = [];
function hostFilterLabel(filter) {
	switch (filter) {
		case "all": return translate("dashboardPopout.map.host.all", "All hosts");
		case "local": return translate("dashboardPopout.map.host.local", "Local");
		case "ssh": return translate("dashboardPopout.map.host.ssh", "SSH");
		case "wsl": return translate("dashboardPopout.map.host.wsl", "WSL");
		case "remote": return translate("dashboardPopout.map.host.remote", "Remote");
	}
}
function AgentMap({ cards, workspaces = EMPTY_WORKSPACES, repoIconsByRepoId, now, className, compact = false, selectedPaneKey = null, enabledStates = ALL_AGENT_STATES, showOrchestrationLinks = true, launchableAgentsByWorktreeId, workspaceContextMenusEnabled = false, onWorkspaceContextMenuOpenChange, onOpenTerminal, onSpawnAgent, onSleepWorkspace }) {
	const canvasRef = (0, import_react.useRef)(null);
	const layoutCacheRef = (0, import_react.useRef)(null);
	const [hostFilter, setHostFilter] = (0, import_react.useState)("all");
	const hostCounts = (0, import_react.useMemo)(() => {
		const counts = {
			local: 0,
			ssh: 0,
			wsl: 0,
			remote: 0
		};
		for (const card of cards) counts[card.hostKind ?? "local"] += 1;
		for (const workspace of workspaces) counts[workspace.hostKind] += 1;
		return counts;
	}, [cards, workspaces]);
	const visibleCards = (0, import_react.useMemo)(() => filterAgentMapCards({
		cards,
		enabledStates,
		hostFilter
	}), [
		cards,
		enabledStates,
		hostFilter
	]);
	const visibleWorkspaces = (0, import_react.useMemo)(() => hostFilter === "all" ? workspaces : workspaces.filter((workspace) => workspace.hostKind === hostFilter), [hostFilter, workspaces]);
	const layoutResult = (0, import_react.useMemo)(() => updateAgentMapLayout(layoutCacheRef.current, visibleCards, now, visibleWorkspaces), [
		visibleCards,
		visibleWorkspaces,
		now
	]);
	(0, import_react.useEffect)(() => {
		layoutCacheRef.current = layoutResult.cache;
	}, [layoutResult.cache]);
	const layout = layoutResult.layout;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("section", {
		className: cn("flex min-h-0 flex-1", className),
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex min-w-0 flex-1 flex-col",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("header", {
				className: "flex min-h-12 shrink-0 items-center gap-3 border-b border-border px-3 py-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("strong", {
					className: "min-w-0 truncate text-xs",
					children: translate("dashboardPopout.map.filters.canvasSummary", "{{shown}} of {{total}} agents shown", {
						shown: visibleCards.length,
						total: cards.length
					})
				}), !compact ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "ml-auto flex items-center gap-0.5 rounded-md border border-border p-0.5",
					role: "group",
					"aria-label": translate("dashboardPopout.map.hostFilter", "Host filter"),
					children: HOST_FILTERS.filter((option) => option === "all" || hostCounts[option] > 0).map((option) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						type: "button",
						variant: "ghost",
						size: "xs",
						"aria-pressed": hostFilter === option,
						onClick: () => setHostFilter(option),
						className: cn("h-6 px-2 text-[10px]", hostFilter === option && "bg-accent text-accent-foreground"),
						children: hostFilterLabel(option)
					}, option))
				}) : null]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentMapCanvas, {
				ref: canvasRef,
				layout,
				repoIconsByRepoId,
				selectedPaneKey,
				allowAggregation: true,
				showOrchestrationLinks,
				launchableAgentsByWorktreeId,
				workspaceContextMenusEnabled,
				onWorkspaceContextMenuOpenChange,
				onSelectAgent: onOpenTerminal,
				onSpawnAgent,
				onSleepWorkspace
			})]
		})
	});
}
export { AgentMap };
