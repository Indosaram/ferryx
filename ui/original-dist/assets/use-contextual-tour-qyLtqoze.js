import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { Ei as hasFeatureInteraction, t as useAppStore } from "./store-CgXrfmaH.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var TOUR_SOURCES = {
	"workspace-board": "workspace_board_visible",
	"workspace-agent-sessions": "workspace_agent_sessions_visible",
	browser: "browser_visible",
	tasks: "tasks_open",
	automations: "automations_open",
	"floating-workspace": "floating_workspace_visible",
	"workspace-creation": "workspace_creation_visible"
};
function createContextualTourInteractionSnapshot(args) {
	return {
		wasPreviouslyInteracted: args.wasFeaturePreviouslyInteracted ?? hasFeatureInteraction(args.featureInteractions, args.id),
		persisted: args.recordFeatureInteractionForTour ? args.recordFeatureInteraction(args.id) : args.featureInteractionPersisted ?? Promise.resolve()
	};
}
async function shouldRequestContextualTourAfterInteraction(args) {
	await args.persisted;
	return !args.isCancelled() && !args.getContextualToursSeenIds().includes(args.id);
}
function useContextualTour(id, enabled, source = TOUR_SOURCES[id], options = {}) {
	const { recordFeatureInteraction: shouldRecordFeatureInteraction = true, featureInteractionPersisted, wasFeaturePreviouslyInteracted } = options;
	const requestContextualTour = useAppStore((s) => s.requestContextualTour);
	const suppressContextualTour = useAppStore((s) => s.suppressContextualTour);
	const recordFeatureInteraction = useAppStore((s) => s.recordFeatureInteraction);
	const persistedUIReady = useAppStore((s) => s.persistedUIReady);
	const activeModal = useAppStore((s) => s.activeModal);
	const activeContextualTourId = useAppStore((s) => s.activeContextualTourId);
	const activeContextualTourSource = useAppStore((s) => s.activeContextualTourSource);
	const activeContextualTourSourceDetached = useAppStore((s) => s.activeContextualTourSourceDetached);
	const contextualToursSeenIds = useAppStore((s) => s.contextualToursSeenIds);
	const contextualToursAutoEligible = useAppStore((s) => s.contextualToursAutoEligible);
	const contextualTourShownThisSession = useAppStore((s) => s.contextualTourShownThisSession);
	const contextualToursOnboardingVisible = useAppStore((s) => s.contextualToursOnboardingVisible);
	const contextualToursBlockingSurfaceVisible = useAppStore((s) => s.contextualToursBlockingSurfaceVisible);
	const enabledInteractionSnapshotRef = (0, import_react.useRef)(null);
	(0, import_react.useEffect)(() => {
		if (!enabled || !persistedUIReady) {
			enabledInteractionSnapshotRef.current = null;
			return;
		}
		if (enabledInteractionSnapshotRef.current?.id === id && enabledInteractionSnapshotRef.current.source === source) return;
		const snapshot = createContextualTourInteractionSnapshot({
			id,
			featureInteractions: useAppStore.getState().featureInteractions,
			recordFeatureInteraction,
			recordFeatureInteractionForTour: shouldRecordFeatureInteraction,
			featureInteractionPersisted,
			wasFeaturePreviouslyInteracted
		});
		enabledInteractionSnapshotRef.current = {
			id,
			source,
			wasPreviouslyInteracted: snapshot.wasPreviouslyInteracted,
			persisted: snapshot.persisted
		};
	}, [
		enabled,
		featureInteractionPersisted,
		id,
		persistedUIReady,
		recordFeatureInteraction,
		shouldRecordFeatureInteraction,
		source,
		wasFeaturePreviouslyInteracted
	]);
	(0, import_react.useEffect)(() => {
		if (!enabled && activeContextualTourId === id && activeContextualTourSource === source && !activeContextualTourSourceDetached) suppressContextualTour(id, source);
	}, [
		activeContextualTourId,
		activeContextualTourSource,
		activeContextualTourSourceDetached,
		enabled,
		id,
		source,
		suppressContextualTour
	]);
	(0, import_react.useEffect)(() => {
		return () => {
			const state = useAppStore.getState();
			if (state.activeContextualTourId === id && state.activeContextualTourSource === source && !state.activeContextualTourSourceDetached) state.suppressContextualTour(id, source);
		};
	}, [id, source]);
	(0, import_react.useEffect)(() => {
		if (!enabled || typeof window === "undefined" || typeof document === "undefined" || !persistedUIReady || contextualToursAutoEligible !== true || contextualToursOnboardingVisible || contextualToursBlockingSurfaceVisible || activeContextualTourId !== null || contextualTourShownThisSession || contextualToursSeenIds.includes(id)) return;
		let frame = null;
		let attempts = 0;
		let requestPending = false;
		let cancelled = false;
		const request = () => {
			if (frame !== null || requestPending) return;
			requestPending = true;
			const snapshot = enabledInteractionSnapshotRef.current;
			shouldRequestContextualTourAfterInteraction({
				id,
				persisted: snapshot?.id === id && snapshot.source === source ? snapshot.persisted : Promise.resolve(),
				isCancelled: () => cancelled,
				getContextualToursSeenIds: () => useAppStore.getState().contextualToursSeenIds
			}).then((shouldRequest) => {
				requestPending = false;
				if (!shouldRequest) return;
				attempts += 1;
				frame = window.requestAnimationFrame(() => {
					frame = null;
					const latestSnapshot = enabledInteractionSnapshotRef.current;
					if (useAppStore.getState().contextualToursSeenIds.includes(id)) return;
					requestContextualTour(id, source, latestSnapshot?.id === id && latestSnapshot.source === source ? latestSnapshot.wasPreviouslyInteracted : hasFeatureInteraction(useAppStore.getState().featureInteractions, id));
				});
			});
		};
		request();
		const timeout = window.setTimeout(request, 250);
		const observer = typeof MutationObserver === "undefined" || !document.body ? null : new MutationObserver(request);
		observer?.observe(document.body, {
			subtree: true,
			childList: true,
			attributes: true,
			attributeFilter: [
				"aria-hidden",
				"class",
				"data-contextual-tour-target",
				"hidden",
				"style"
			]
		});
		const interval = window.setInterval(() => {
			if (attempts >= 20) {
				window.clearInterval(interval);
				return;
			}
			request();
		}, 500);
		return () => {
			cancelled = true;
			if (frame !== null) window.cancelAnimationFrame(frame);
			window.clearTimeout(timeout);
			window.clearInterval(interval);
			observer?.disconnect();
		};
	}, [
		activeContextualTourId,
		contextualToursBlockingSurfaceVisible,
		activeModal,
		contextualToursAutoEligible,
		contextualTourShownThisSession,
		contextualToursOnboardingVisible,
		contextualToursSeenIds,
		enabled,
		id,
		persistedUIReady,
		requestContextualTour,
		source
	]);
}
export { useContextualTour as t };
