import { t as useAppStore } from "./store-CgXrfmaH.js";
import { t as requestContextualTourWhenReady } from "./request-contextual-tour-when-ready-C7EM2Vly.js";
function openWorkspaceCreationComposerWithTourHandoff() {
	const state = useAppStore.getState();
	const shouldHandoffFromAgentSessionsTour = state.repos.length > 0 && state.activeContextualTourId === "workspace-agent-sessions" && state.activeContextualTourStepIndex === 1;
	if (shouldHandoffFromAgentSessionsTour && state.activeContextualTourSource) {
		state.detachContextualTourSource("workspace-agent-sessions", state.activeContextualTourSource);
		state.completeContextualTour("workspace-agent-sessions");
	}
	state.openModal("new-workspace-composer", {
		telemetrySource: "sidebar",
		...shouldHandoffFromAgentSessionsTour ? { contextualTourSource: "workspace_creation_modal" } : {}
	});
	if (!shouldHandoffFromAgentSessionsTour) return;
	if (state.contextualToursSeenIds.includes("workspace-creation")) return;
	requestContextualTourWhenReady({
		id: "workspace-creation",
		source: "workspace_creation_modal",
		wasFeaturePreviouslyInteracted: false,
		waitForActiveTourToClear: true,
		shouldContinue: () => useAppStore.getState().activeModal === "new-workspace-composer"
	});
}
const CONTEXTUAL_TOUR_ENABLE_AUTO_WORKSPACE_NAME_EVENT = "orca:contextual-tour-enable-auto-workspace-name";
export { openWorkspaceCreationComposerWithTourHandoff as n, CONTEXTUAL_TOUR_ENABLE_AUTO_WORKSPACE_NAME_EVENT as t };
