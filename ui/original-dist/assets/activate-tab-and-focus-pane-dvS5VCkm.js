import { t as useAppStore } from "./store-CgXrfmaH.js";
import { r as FOCUS_TERMINAL_PANE_EVENT } from "./terminal-C-BGupDh.js";
var pendingFocusPaneFrameId = null;
function cancelPendingFocusPaneFrame() {
	if (pendingFocusPaneFrameId !== null) {
		cancelAnimationFrame(pendingFocusPaneFrameId);
		pendingFocusPaneFrameId = null;
	}
}
function activateTabAndFocusPane(tabId, leafId, opts) {
	const { setActiveTab, setActiveTabType } = useAppStore.getState();
	setActiveTabType("terminal");
	setActiveTab(tabId);
	cancelPendingFocusPaneFrame();
	if (leafId === null) return;
	pendingFocusPaneFrameId = requestAnimationFrame(() => {
		pendingFocusPaneFrameId = null;
		const detail = {
			tabId,
			leafId,
			...opts?.ackPaneKeyOnSuccess ? { ackPaneKeyOnSuccess: opts.ackPaneKeyOnSuccess } : {},
			...opts?.flashFocusedPane ? { flashFocusedPane: true } : {},
			...opts?.scrollToBottomIfOutputSinceLastView ? { scrollToBottomIfOutputSinceLastView: true } : {}
		};
		window.dispatchEvent(new CustomEvent(FOCUS_TERMINAL_PANE_EVENT, { detail }));
	});
}
export { activateTabAndFocusPane as t };
