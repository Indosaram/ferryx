import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var WORKSPACE_BOARD_KEEP_OPEN_SELECTOR = [
	"[data-workspace-board-trigger]",
	"[data-workspace-board-preserve-open]",
	"[data-workspace-status-appearance-popover]",
	"[data-contextual-tour-overlay]",
	"[data-contextual-tour-panel]",
	"[data-radix-popper-content-wrapper]",
	"[data-slot=\"dropdown-menu-content\"]",
	"[data-slot=\"context-menu-content\"]",
	"[data-slot=\"popover-content\"]",
	"[data-slot=\"dialog-content\"]",
	"[data-slot=\"dialog-overlay\"]",
	"[data-sonner-toast]",
	"[role=\"dialog\"][data-state=\"open\"]",
	"[role=\"alertdialog\"][data-state=\"open\"]",
	"[role=\"menu\"][data-state=\"open\"]"
].join(", ");
function isWorkspaceBoardKeepOpenTarget(target) {
	const element = target instanceof Element ? target : target instanceof Node ? target.parentElement : null;
	return Boolean(element?.closest(WORKSPACE_BOARD_KEEP_OPEN_SELECTOR));
}
function useWorkspaceKanbanOutsideDismiss(params) {
	const { open, boardRef, preserveOpenForMenu, onOpenChange } = params;
	(0, import_react.useEffect)(() => {
		if (!open) return;
		const handlePointerDown = (event) => {
			const content = boardRef.current?.closest("[data-slot=\"sheet-content\"]");
			if (!content || preserveOpenForMenu) return;
			if (event.target instanceof Node && content.contains(event.target)) return;
			if (isWorkspaceBoardKeepOpenTarget(event.target)) return;
			const rect = content.getBoundingClientRect();
			if (event.clientX > rect.right && event.clientY >= rect.top && event.clientY <= rect.bottom) onOpenChange(false);
		};
		document.addEventListener("pointerdown", handlePointerDown, true);
		return () => document.removeEventListener("pointerdown", handlePointerDown, true);
	}, [
		boardRef,
		onOpenChange,
		open,
		preserveOpenForMenu
	]);
}
const WORKSPACE_TOP_CHROME_HEIGHT = 36;
const STATUS_BAR_RESERVE_HEIGHT = 24;
export { useWorkspaceKanbanOutsideDismiss as i, WORKSPACE_TOP_CHROME_HEIGHT as n, isWorkspaceBoardKeepOpenTarget as r, STATUS_BAR_RESERVE_HEIGHT as t };
