import { a as track } from "./telemetry-ZyUPyKMD.js";
import { r as getFeatureWallSetupSectionId, s as isFeatureWallSetupStepId } from "./feature-wall-setup-steps-D7ga1-7b.js";
const FEATURE_EDUCATION_SOURCES = [
	"workspace_board_visible",
	"workspace_agent_sessions_visible",
	"browser_visible",
	"tasks_open",
	"automations_open",
	"floating_workspace_visible",
	"workspace_creation_visible",
	"workspace_creation_modal",
	"setup_guide_parallel_work",
	"unknown"
];
const SETUP_GUIDE_SOURCES = [
	"sidebar",
	"contextual_tour",
	"settings",
	"feature_wall",
	"help_menu",
	"unknown"
];
function normalizeFeatureEducationSource(value) {
	return FEATURE_EDUCATION_SOURCES.includes(value) ? value : "unknown";
}
function normalizeSetupGuideSource(value) {
	return SETUP_GUIDE_SOURCES.includes(value) ? value : "unknown";
}
var SETUP_GUIDE_TELEMETRY_COMPLETED_STEPS_STORAGE_KEY = "orca.setupGuideTelemetryCompletedSteps.v1";
var TERMINAL_PANE_SPLIT_TELEMETRY_STORAGE_KEY = "orca.terminalPaneSplitTelemetry.v1";
function trackContextualTourShown(args) {
	emitFeatureEducationTelemetry("contextual_tour_shown", {
		tour_id: args.tourId,
		source: normalizeFeatureEducationSource(args.source),
		was_feature_previously_interacted: args.wasFeaturePreviouslyInteracted
	});
}
function trackContextualTourOutcome(args) {
	emitFeatureEducationTelemetry("contextual_tour_outcome", {
		tour_id: args.tourId,
		source: normalizeFeatureEducationSource(args.source),
		outcome: args.outcome,
		steps_seen: clampTourStepCount(args.stepsSeen),
		total_steps: clampTourStepCount(args.totalSteps, 1),
		...args.furthestStepIndex !== void 0 && args.definedStepCount !== void 0 ? {
			furthest_step_index: clampTourStepCount(args.furthestStepIndex, 1),
			defined_step_count: clampTourStepCount(args.definedStepCount, 1)
		} : {}
	});
}
function trackSetupGuideOpened(args) {
	const source = normalizeSetupGuideSource(args.source);
	emitFeatureEducationTelemetry("setup_guide_opened", {
		source,
		initial_completed_count: clampSetupGuideStepCount(args.initialCompletedCount),
		total_steps: 8,
		first_incomplete_step_id: args.firstIncompleteStepId
	});
	return source;
}
function trackSetupGuideClosed(args) {
	const initialCompletedCount = clampSetupGuideStepCount(args.initialCompletedCount);
	const finalCompletedCount = Math.max(initialCompletedCount, clampSetupGuideStepCount(args.finalCompletedCount));
	emitFeatureEducationTelemetry("setup_guide_closed", {
		source: args.source,
		outcome: args.outcome,
		initial_completed_count: initialCompletedCount,
		final_completed_count: finalCompletedCount,
		total_steps: 8,
		active_step_id: args.activeStepId
	});
}
function trackSetupGuideStepCompleted(args) {
	emitFeatureEducationTelemetry("setup_guide_step_completed", {
		step_id: args.stepId,
		section_id: getSetupGuideStepSection(args.stepId),
		completed_count: clampSetupGuideStepCount(args.completedCount, 1),
		total_steps: 8,
		setup_guide_visible: args.setupGuideVisible
	});
}
function trackTerminalPaneSplit(args) {
	if (!reserveTerminalPaneSplitTelemetry(args.source, args.direction)) return;
	emitFeatureEducationTelemetry("terminal_pane_split", {
		source: args.source,
		direction: args.direction
	});
}
function readEmittedSetupGuideStepIds() {
	if (globalThis.localStorage === void 0) return /* @__PURE__ */ new Set();
	try {
		const raw = JSON.parse(globalThis.localStorage.getItem(SETUP_GUIDE_TELEMETRY_COMPLETED_STEPS_STORAGE_KEY) ?? "[]");
		if (!Array.isArray(raw)) return /* @__PURE__ */ new Set();
		return new Set(raw.filter(isFeatureWallSetupStepId));
	} catch {
		return /* @__PURE__ */ new Set();
	}
}
function persistEmittedSetupGuideStepId(id) {
	if (globalThis.localStorage === void 0) return;
	try {
		const next = readEmittedSetupGuideStepIds();
		next.add(id);
		globalThis.localStorage.setItem(SETUP_GUIDE_TELEMETRY_COMPLETED_STEPS_STORAGE_KEY, JSON.stringify([...next]));
	} catch {}
}
function reserveTerminalPaneSplitTelemetry(source, direction) {
	if (globalThis.localStorage === void 0) return true;
	try {
		const emitted = readTerminalPaneSplitTelemetryKeys();
		const key = getTerminalPaneSplitTelemetryKey(source, direction, /* @__PURE__ */ new Date());
		if (emitted.has(key)) return false;
		emitted.add(key);
		globalThis.localStorage.setItem(TERMINAL_PANE_SPLIT_TELEMETRY_STORAGE_KEY, JSON.stringify([...emitted].slice(-32)));
		return true;
	} catch {
		return true;
	}
}
function getSetupGuideStepSection(id) {
	return getFeatureWallSetupSectionId(id);
}
function emitFeatureEducationTelemetry(name, props) {
	track(name, props);
}
function clampTourStepCount(value, min = 0) {
	if (!Number.isFinite(value)) return min;
	return Math.min(8, Math.max(min, Math.round(value)));
}
function clampSetupGuideStepCount(value, min = 0) {
	if (!Number.isFinite(value)) return min;
	return Math.min(8, Math.max(min, Math.round(value)));
}
function readTerminalPaneSplitTelemetryKeys() {
	const raw = JSON.parse(globalThis.localStorage?.getItem(TERMINAL_PANE_SPLIT_TELEMETRY_STORAGE_KEY) ?? "[]");
	if (!Array.isArray(raw)) return /* @__PURE__ */ new Set();
	return new Set(raw.filter((value) => typeof value === "string"));
}
function getTerminalPaneSplitTelemetryKey(source, direction, date) {
	return `${Number.isFinite(date.getTime()) ? date.toISOString().slice(0, 10) : (/* @__PURE__ */ new Date(0)).toISOString().slice(0, 10)}:${source}:${direction}`;
}
export { trackSetupGuideClosed as a, trackTerminalPaneSplit as c, trackContextualTourShown as i, readEmittedSetupGuideStepIds as n, trackSetupGuideOpened as o, trackContextualTourOutcome as r, trackSetupGuideStepCompleted as s, persistEmittedSetupGuideStepId as t };
