import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as Button } from "./button-DszXJEV6.js";
import { t as FloatingTerminalIconContextMenu } from "./FloatingTerminalIconContextMenu-Bl-Pdr-a.js";
import { t as PanelsTopLeft } from "./panels-top-left-N4Rk5Xar.js";
import { t as useAppStore } from "./store-CgXrfmaH.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
import { o as selectFloatingWorkspaceHasUnread } from "./selectors-XOBeaOSb.js";
import { s as useShortcutLabel } from "./useShortcutLabel-C-KRYtlB.js";
const FLOATING_WORKSPACE_GUEST_CLOSE_EVENT = "orca:floating-workspace-guest-close";
const FLOATING_WORKSPACE_GUEST_SELECT_INDEX_EVENT = "orca:floating-workspace-guest-select-index";
function dispatchFloatingWorkspaceGuestClose(detail) {
	if (typeof window === "undefined") return;
	window.dispatchEvent(new CustomEvent(FLOATING_WORKSPACE_GUEST_CLOSE_EVENT, { detail }));
}
function dispatchFloatingWorkspaceGuestSelectIndex(detail) {
	if (typeof window === "undefined") return;
	window.dispatchEvent(new CustomEvent(FLOATING_WORKSPACE_GUEST_SELECT_INDEX_EVENT, { detail }));
}
var TRIGGER_SIZE = 36;
var DEFAULT_RIGHT_GAP = 24;
var DEFAULT_BOTTOM_GAP = 72;
var DRAG_MARGIN = 8;
var TITLEBAR_SAFE_TOP = 36;
const FLOATING_TERMINAL_TRIGGER_POSITION_STORAGE_KEY = "orca-floating-terminal-trigger-position-v2";
function getViewport() {
	return {
		width: typeof window === "undefined" ? 1200 : window.innerWidth,
		height: typeof window === "undefined" ? 800 : window.innerHeight
	};
}
function isFiniteCoordinate(value) {
	return typeof value === "number" && Number.isFinite(value);
}
function isAnchorX(value) {
	return value === "left" || value === "right";
}
function isAnchorY(value) {
	return value === "top" || value === "bottom";
}
function getWindowStorage$1() {
	return typeof window !== "undefined" && window.localStorage !== void 0 ? window.localStorage : null;
}
function isAnchoredTriggerPosition(position) {
	return "anchorX" in position;
}
function getDefaultFloatingTerminalTriggerCommittedPosition() {
	return {
		anchorX: "right",
		anchorY: "bottom",
		offsetX: DEFAULT_RIGHT_GAP,
		offsetY: DEFAULT_BOTTOM_GAP
	};
}
function getDefaultFloatingTerminalTriggerPosition() {
	return clampFloatingTerminalTriggerPosition(resolveFloatingTerminalTriggerCommittedPosition(getDefaultFloatingTerminalTriggerCommittedPosition()));
}
function clampFloatingTerminalTriggerPosition(position) {
	const viewport = getViewport();
	const maxLeft = Math.max(DRAG_MARGIN, viewport.width - TRIGGER_SIZE - DRAG_MARGIN);
	const maxTop = Math.max(TITLEBAR_SAFE_TOP, viewport.height - TRIGGER_SIZE - DRAG_MARGIN);
	return {
		left: Math.min(Math.max(DRAG_MARGIN, position.left), maxLeft),
		top: Math.min(Math.max(TITLEBAR_SAFE_TOP, position.top), maxTop)
	};
}
function hasUsableFloatingTerminalTriggerViewport() {
	const viewport = getViewport();
	return viewport.width >= TRIGGER_SIZE + DRAG_MARGIN * 2 && viewport.height >= TRIGGER_SIZE + TITLEBAR_SAFE_TOP + DRAG_MARGIN;
}
function resolveFloatingTerminalTriggerCommittedPosition(position) {
	if (!isAnchoredTriggerPosition(position)) return position;
	const viewport = getViewport();
	return {
		left: position.anchorX === "left" ? position.offsetX : viewport.width - TRIGGER_SIZE - position.offsetX,
		top: position.anchorY === "top" ? position.offsetY : viewport.height - TRIGGER_SIZE - position.offsetY
	};
}
function anchorFloatingTerminalTriggerPosition(position) {
	if (!hasUsableFloatingTerminalTriggerViewport()) return null;
	const viewport = getViewport();
	const anchorX = position.left + TRIGGER_SIZE / 2 <= viewport.width / 2 ? "left" : "right";
	const anchorY = position.top + TRIGGER_SIZE / 2 <= viewport.height / 2 ? "top" : "bottom";
	return {
		anchorX,
		anchorY,
		offsetX: anchorX === "left" ? position.left : viewport.width - position.left - TRIGGER_SIZE,
		offsetY: anchorY === "top" ? position.top : viewport.height - position.top - TRIGGER_SIZE
	};
}
function shouldReconcileFloatingTerminalTriggerPosition(source) {
	return source === "default" || hasUsableFloatingTerminalTriggerViewport();
}
function resolveFloatingTerminalTriggerPosition(position, source) {
	if (source === "default") return getDefaultFloatingTerminalTriggerPosition();
	return clampFloatingTerminalTriggerPosition(resolveFloatingTerminalTriggerCommittedPosition(position));
}
function parseFloatingTerminalTriggerPosition(serialized) {
	if (!serialized) return null;
	try {
		const parsed = JSON.parse(serialized);
		if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) return null;
		const record = parsed;
		if (isAnchorX(record.anchorX) && isAnchorY(record.anchorY) && isFiniteCoordinate(record.offsetX) && isFiniteCoordinate(record.offsetY)) return {
			anchorX: record.anchorX,
			anchorY: record.anchorY,
			offsetX: record.offsetX,
			offsetY: record.offsetY
		};
		if (!isFiniteCoordinate(record.left) || !isFiniteCoordinate(record.top)) return null;
		return {
			left: record.left,
			top: record.top
		};
	} catch {
		return null;
	}
}
function readPersistedFloatingTerminalTriggerPosition() {
	try {
		return parseFloatingTerminalTriggerPosition(getWindowStorage$1()?.getItem("orca-floating-terminal-trigger-position-v2") ?? null);
	} catch {
		return null;
	}
}
function persistFloatingTerminalTriggerPosition(position) {
	try {
		getWindowStorage$1()?.setItem(FLOATING_TERMINAL_TRIGGER_POSITION_STORAGE_KEY, JSON.stringify(position));
	} catch {}
}
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var FLOATING_TERMINAL_TRIGGER_DRAG_THRESHOLD = 4;
function readInitialTriggerPosition() {
	const defaultCommittedPosition = getDefaultFloatingTerminalTriggerCommittedPosition();
	const defaultPosition = getDefaultFloatingTerminalTriggerPosition();
	if (typeof window === "undefined") return {
		committedPosition: defaultCommittedPosition,
		position: defaultPosition,
		source: "default"
	};
	const persistedPosition = readPersistedFloatingTerminalTriggerPosition();
	return persistedPosition ? {
		committedPosition: persistedPosition,
		position: shouldReconcileFloatingTerminalTriggerPosition("user") ? resolveFloatingTerminalTriggerPosition(persistedPosition, "user") : resolveFloatingTerminalTriggerCommittedPosition(persistedPosition),
		source: "user"
	} : {
		committedPosition: defaultCommittedPosition,
		position: defaultPosition,
		source: "default"
	};
}
function FloatingTerminalToggleButton({ open, onToggle }) {
	const shortcutLabel = useShortcutLabel("floatingTerminal.toggle");
	const hasFloatingUnread = useAppStore(selectFloatingWorkspaceHasUnread);
	const showAttentionDot = !open && hasFloatingUnread;
	const initialPositionState = (0, import_react.useRef)(null);
	if (initialPositionState.current === null) initialPositionState.current = readInitialTriggerPosition();
	const positionSourceRef = (0, import_react.useRef)(initialPositionState.current.source);
	const committedPositionRef = (0, import_react.useRef)(initialPositionState.current.committedPosition);
	const [position, setPosition] = (0, import_react.useState)(initialPositionState.current.position);
	const dragRef = (0, import_react.useRef)(null);
	const stagedPositionRef = (0, import_react.useRef)(null);
	const suppressClickRef = (0, import_react.useRef)(false);
	const previewPosition = (0, import_react.useCallback)((nextPosition) => {
		const clamped = clampFloatingTerminalTriggerPosition(nextPosition);
		stagedPositionRef.current = clamped;
		setPosition(clamped);
	}, []);
	const commitPosition = (0, import_react.useCallback)((nextPosition) => {
		stagedPositionRef.current = null;
		const clamped = clampFloatingTerminalTriggerPosition(nextPosition);
		setPosition(clamped);
		const anchoredPosition = anchorFloatingTerminalTriggerPosition(clamped);
		if (!anchoredPosition) return;
		committedPositionRef.current = anchoredPosition;
		positionSourceRef.current = "user";
		persistFloatingTerminalTriggerPosition(anchoredPosition);
	}, []);
	const reconcilePosition = (0, import_react.useCallback)(() => {
		setPosition((current) => {
			if (!shouldReconcileFloatingTerminalTriggerPosition(positionSourceRef.current)) return current;
			return resolveFloatingTerminalTriggerPosition(committedPositionRef.current, positionSourceRef.current);
		});
	}, []);
	(0, import_react.useLayoutEffect)(() => {
		reconcilePosition();
	}, [reconcilePosition]);
	(0, import_react.useEffect)(() => {
		const handleResize = () => reconcilePosition();
		window.addEventListener("resize", handleResize);
		return () => window.removeEventListener("resize", handleResize);
	}, [reconcilePosition]);
	const handlePointerDown = (event) => {
		if (event.button !== 0) return;
		dragRef.current = {
			pointerId: event.pointerId,
			startX: event.clientX,
			startY: event.clientY,
			left: position.left,
			top: position.top,
			moved: false
		};
		event.currentTarget.setPointerCapture(event.pointerId);
	};
	const handlePointerMove = (event) => {
		const drag = dragRef.current;
		if (!drag || drag.pointerId !== event.pointerId) return;
		const dx = event.clientX - drag.startX;
		const dy = event.clientY - drag.startY;
		if (!drag.moved && Math.hypot(dx, dy) < FLOATING_TERMINAL_TRIGGER_DRAG_THRESHOLD) return;
		drag.moved = true;
		previewPosition({
			left: drag.left + dx,
			top: drag.top + dy
		});
	};
	const handlePointerEnd = (event) => {
		const drag = dragRef.current;
		if (!drag || drag.pointerId !== event.pointerId) return;
		suppressClickRef.current = drag.moved;
		if (drag.moved && stagedPositionRef.current) commitPosition(stagedPositionRef.current);
		dragRef.current = null;
	};
	const handleClick = (event) => {
		if (suppressClickRef.current) {
			suppressClickRef.current = false;
			event.preventDefault();
			event.stopPropagation();
			return;
		}
		onToggle();
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(FloatingTerminalIconContextMenu, {
		currentLocation: "floating-button",
		className: "fixed z-[46]",
		style: {
			left: position.left,
			top: position.top
		},
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
			asChild: true,
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
				type: "button",
				variant: "outline",
				size: "icon",
				className: "relative cursor-grab rounded-lg border-transparent text-foreground bg-card shadow-[0_4px_12px_rgb(0_0_0_/_0.22),0_0_0_1px_color-mix(in_srgb,var(--foreground)_12%,transparent)] hover:-translate-y-0.5 hover:bg-accent active:translate-y-0 active:cursor-grabbing dark:bg-accent dark:shadow-[0_6px_16px_rgb(0_0_0_/_0.55),0_0_0_1px_rgb(255_255_255_/_0.22)] dark:hover:bg-[color-mix(in_srgb,var(--accent)_82%,white)]",
				"data-floating-terminal-toggle": true,
				"aria-label": open ? translate("auto.components.floating.terminal.FloatingTerminalToggleButton.5785dd9148", "Minimize floating workspace") : showAttentionDot ? translate("auto.components.floating.terminal.FloatingTerminalToggleButton.4cb418b991", "Show floating workspace, new activity") : translate("auto.components.floating.terminal.FloatingTerminalToggleButton.3b04b065b5", "Show floating workspace"),
				"aria-pressed": open,
				onPointerDown: handlePointerDown,
				onPointerMove: handlePointerMove,
				onPointerUp: handlePointerEnd,
				onPointerCancel: handlePointerEnd,
				onClick: handleClick,
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(PanelsTopLeft, { className: "size-4" }), showAttentionDot ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					"aria-hidden": true,
					"data-floating-terminal-attention": true,
					className: "pointer-events-none absolute right-1 top-1 size-2 rounded-full bg-amber-500 ring-2 ring-card dark:ring-accent"
				}) : null]
			})
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
			side: "left",
			sideOffset: 6,
			children: translate("auto.components.floating.terminal.FloatingTerminalToggleButton.bfe7809a70", "{{value0}} floating workspace ({{value1}})", {
				value0: open ? "Minimize" : "Show",
				value1: shortcutLabel
			})
		})] })
	});
}
const FLOATING_TERMINAL_PANEL_VIEW_STATE_STORAGE_KEY = "orca-floating-terminal-panel-view-state-v1";
function getWindowStorage() {
	return typeof window === "undefined" ? null : window.localStorage;
}
function readPersistedFloatingTerminalPanelViewState() {
	try {
		const serialized = getWindowStorage()?.getItem(FLOATING_TERMINAL_PANEL_VIEW_STATE_STORAGE_KEY);
		if (!serialized) return null;
		const parsed = JSON.parse(serialized);
		if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) return null;
		const record = parsed;
		return {
			open: record.open === true,
			maximized: record.maximized === true
		};
	} catch {
		return null;
	}
}
function persistFloatingTerminalPanelViewState(state) {
	try {
		getWindowStorage()?.setItem(FLOATING_TERMINAL_PANEL_VIEW_STATE_STORAGE_KEY, JSON.stringify(state));
	} catch {}
}
function persistFloatingTerminalPanelOpen(open) {
	persistFloatingTerminalPanelViewState({
		maximized: readPersistedFloatingTerminalPanelViewState()?.maximized === true,
		open
	});
}
function persistFloatingTerminalPanelMaximized(maximized) {
	persistFloatingTerminalPanelViewState({
		open: readPersistedFloatingTerminalPanelViewState()?.open === true,
		maximized
	});
}
export { FLOATING_WORKSPACE_GUEST_CLOSE_EVENT as a, dispatchFloatingWorkspaceGuestSelectIndex as c, FloatingTerminalToggleButton as i, persistFloatingTerminalPanelOpen as n, FLOATING_WORKSPACE_GUEST_SELECT_INDEX_EVENT as o, readPersistedFloatingTerminalPanelViewState as r, dispatchFloatingWorkspaceGuestClose as s, persistFloatingTerminalPanelMaximized as t };
