import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { i as getFeatureWallSetupSteps, n as FEATURE_WALL_SETUP_STEP_IDS, o as getFirstIncompleteFeatureWallSetupStepId } from "./feature-wall-setup-steps-D7ga1-7b.js";
import { a as trackSetupGuideClosed, n as readEmittedSetupGuideStepIds, o as trackSetupGuideOpened, s as trackSetupGuideStepCompleted, t as persistEmittedSetupGuideStepId } from "./feature-education-telemetry-DPRGAVBD.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
function useSetupGuideOpenCloseTelemetry(args) {
	const setupSteps = (0, import_react.useMemo)(() => getFeatureWallSetupSteps(), []);
	const sessionRef = (0, import_react.useRef)(null);
	const snapshotRef = (0, import_react.useRef)({
		completedCount: 0,
		totalSteps: FEATURE_WALL_SETUP_STEP_IDS.length,
		activeStepId: "none"
	});
	const completedCount = countCompletedSetupSteps(args.progress.stepDone);
	const firstIncompleteStepId = getSetupGuideTelemetryFirstIncompleteStepId(args.progress);
	snapshotRef.current = {
		completedCount,
		totalSteps: setupSteps.length,
		activeStepId: args.activeStepId ?? "none"
	};
	const closeSession = (0, import_react.useCallback)((outcome) => {
		const session = sessionRef.current;
		if (!session) return;
		sessionRef.current = null;
		const snapshot = snapshotRef.current;
		trackSetupGuideClosed({
			source: session.source,
			outcome: snapshot.completedCount >= snapshot.totalSteps ? "completed" : outcome,
			initialCompletedCount: session.initialCompletedCount,
			finalCompletedCount: snapshot.completedCount,
			totalSteps: snapshot.totalSteps,
			activeStepId: snapshot.activeStepId
		});
	}, []);
	(0, import_react.useEffect)(() => {
		if (args.isOpen && !sessionRef.current) {
			sessionRef.current = {
				source: trackSetupGuideOpened({
					source: args.source,
					initialCompletedCount: completedCount,
					totalSteps: setupSteps.length,
					firstIncompleteStepId
				}),
				initialCompletedCount: completedCount
			};
			return;
		}
		if (!args.isOpen && sessionRef.current) closeSession("dismissed");
	}, [
		args.isOpen,
		args.source,
		closeSession,
		completedCount,
		firstIncompleteStepId,
		setupSteps.length
	]);
	(0, import_react.useEffect)(() => {
		return () => {
			closeSession("interrupted");
		};
	}, [closeSession]);
}
function getSetupGuideTelemetryFirstIncompleteStepId(progress) {
	return countCompletedSetupSteps(progress.stepDone) >= FEATURE_WALL_SETUP_STEP_IDS.length ? "none" : getFirstIncompleteFeatureWallSetupStepId(progress.stepDone);
}
function useSetupGuideStepCompletionTelemetry(args) {
	const stateRef = (0, import_react.useRef)(null);
	if (!stateRef.current) stateRef.current = createSetupGuideStepCompletionTelemetryState();
	(0, import_react.useEffect)(() => {
		recordSetupGuideStepCompletionTelemetry({
			state: stateRef.current,
			progress: args.progress,
			setupGuideVisible: args.setupGuideVisible
		});
	}, [
		args.progress,
		args.progress.stepDone,
		args.setupGuideVisible
	]);
}
function createSetupGuideStepCompletionTelemetryState() {
	return {
		previousDone: null,
		emitted: null
	};
}
function recordSetupGuideStepCompletionTelemetry(args) {
	if (!args.state.emitted) args.state.emitted = readEmittedSetupGuideStepIds();
	const previousDone = args.state.previousDone;
	args.state.previousDone = { ...args.progress.stepDone };
	const emitted = args.state.emitted;
	if (!previousDone || !args.setupGuideVisible) {
		persistCompletedSetupGuideStepBaselines(args.progress.stepDone, emitted);
		return;
	}
	const completedCount = countCompletedSetupSteps(args.progress.stepDone);
	for (const stepId of FEATURE_WALL_SETUP_STEP_IDS) {
		if (!args.progress.stepDone[stepId] || previousDone[stepId] || emitted.has(stepId)) continue;
		emitted.add(stepId);
		persistEmittedSetupGuideStepId(stepId);
		trackSetupGuideStepCompleted({
			stepId,
			completedCount,
			totalSteps: FEATURE_WALL_SETUP_STEP_IDS.length,
			setupGuideVisible: args.setupGuideVisible
		});
	}
}
function countCompletedSetupSteps(done) {
	return FEATURE_WALL_SETUP_STEP_IDS.filter((stepId) => done[stepId]).length;
}
function persistCompletedSetupGuideStepBaselines(done, emitted) {
	for (const stepId of FEATURE_WALL_SETUP_STEP_IDS) {
		if (!done[stepId] || emitted.has(stepId)) continue;
		emitted.add(stepId);
		persistEmittedSetupGuideStepId(stepId);
	}
}
export { useSetupGuideStepCompletionTelemetry as n, useSetupGuideOpenCloseTelemetry as t };
