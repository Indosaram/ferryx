import { a as REQUEST_ACTIVE_TERMINAL_PANE_SPLIT_EVENT } from "./terminal-C-BGupDh.js";
function requestActiveTerminalPaneSplit(detail) {
	window.dispatchEvent(new CustomEvent(REQUEST_ACTIVE_TERMINAL_PANE_SPLIT_EVENT, { detail }));
}
export { requestActiveTerminalPaneSplit as t };
