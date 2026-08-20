import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as init_defineProperty, t as _defineProperty } from "./defineProperty-BAtR-r70.js";
import { l as createLucideIcon, n as cn } from "./button-DszXJEV6.js";
import { t as ArrowDown } from "./arrow-down-Dmgw82W4.js";
import { t as ArrowLeft } from "./arrow-left-BpDalf_n.js";
import { t as ArrowRight } from "./arrow-right-ct5UxmKv.js";
import { t as ArrowUp } from "./arrow-up-CCUzfqnh.js";
import { t as MessageSquare } from "./message-square-DzGigs-c.js";
import { t as Minimize2 } from "./minimize-2-2v01SFqN.js";
import { n as PanelBottomClose, t as PanelRightClose } from "./panel-right-close-DCJo-Y53.js";
import { t as PanelLeftClose } from "./panel-left-close-B9t5w2Nh.js";
import { t as Pencil } from "./pencil-CLc9a5do.js";
import { t as PinOff } from "./pin-off-CP-untcQ.js";
import { t as Pin } from "./pin-K26SGNXp.js";
import { t as SquareTerminal } from "./square-terminal-rgWG-Apn.js";
import { Dc as dirname, Ec as basename, Eu as getExecutionHostIdForWorktree, Gc as isTerminalLeafId, I as buildDiffEditorFileId, Ip as isWindowsAbsolutePathLike, J as notifyHostOfMirroredEditorClose, Jc as parsePaneKey, Kc as makePaneKey, L as buildOwnedEditorFileId, Ou as getRuntimeEnvironmentIdForWorktree, Pm as FLOATING_TERMINAL_WORKTREE_ID, Pp as isPathInsideOrEqual, R as resolveEditorFileIdForOwner, Ro as parseRemoteRuntimePtyId, Rp as normalizeRuntimePathSeparators, Rt as detectLanguage, Ru as isPaneColumnSplitDropNoOp, Xi as worktreeUsesRemoteConnection, fl as acquireWebviewsDragPassthrough, ht as renameRuntimePath, kc as joinPath, t as useAppStore, zp as relativePathInsideRoot } from "./store-CgXrfmaH.js";
import { t as X } from "./x-BrGKE4uz.js";
import { A as stripLeadingAgentTitleDecoration, D as resolveExplicitTerminalTitleAgentType, E as isClaudeIdentityFrameTitle, F as isOpenCodeNativeTitle, n as agentTypeToIconAgent, y as isShellProcess } from "./agent-status-3vUKbY6l.js";
import { t as require_react_dom } from "./react-dom-Da8MQai-.js";
import { n as toast } from "./dist-DgqligFk.js";
import { d as DropdownMenuSub, f as DropdownMenuSubContent, i as DropdownMenuItem, l as DropdownMenuSeparator, m as DropdownMenuTrigger, p as DropdownMenuSubTrigger, r as DropdownMenuContent, t as DropdownMenu, u as DropdownMenuShortcut } from "./dropdown-menu-Dth6LPK-.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
import { d as isWebRuntimeSessionActive, f as moveWebRuntimeSessionTab } from "./web-runtime-session-CN2syA39.js";
import { i as resolveCompatibleAgentTypeForOwner, t as resolvePaneAgentOwner } from "./pane-agent-owner-BPfoVAtS.js";
import { g as requestEditorSaveQuiesce } from "./editor-autosave-C_Vljs6z.js";
import { i as useOptionalShortcutLabel, r as formatShortcutLabel } from "./useShortcutLabel-C-KRYtlB.js";
import { t as Input } from "./input-DV5rpysh.js";
import { t as isImeCompositionKeyDown } from "./ime-composition-keyboard-event-HdRxQ6x2.js";
import { i as FilledBellIcon } from "./WorktreeCardHelpers-Detnezco.js";
import { t as AgentStateDot } from "./AgentStateDot-DFt63YGw.js";
import { t as AgentIcon } from "./agent-catalog-CBF2CV5Q.js";
import { c as settleEditorPathMove, n as verifyLatchedMoveDestinations, s as beginEditorPathMove } from "./useEditorExternalWatch-DJjNRIQG.js";
import { n as getFileExplorerOperationOwner, t as captureFileExplorerOperationGuard } from "./file-explorer-operation-owner-C4AAHFB5.js";
import { t as requestActiveTerminalPaneSplit } from "./request-active-terminal-pane-split-So9AiZw3.js";
import { t as ShellIcon } from "./shell-icons-DdfqjDKR.js";
import { a as terminalTabHasUnreadActivity, i as terminalTabActivityToAgentDotState, n as resolveTerminalTabActivityStatus, t as isTerminalTabActivityLive } from "./terminal-tab-activity-status-BaKqQAEL.js";
var Columns2 = createLucideIcon("columns-2", [["rect", {
	width: "18",
	height: "18",
	x: "3",
	y: "3",
	rx: "2",
	key: "afitv7"
}], ["path", {
	d: "M12 3v18",
	key: "108xh3"
}]]);
var ListX = createLucideIcon("list-x", [
	["path", {
		d: "M16 5H3",
		key: "m91uny"
	}],
	["path", {
		d: "M11 12H3",
		key: "51ecnj"
	}],
	["path", {
		d: "M16 19H3",
		key: "zzsher"
	}],
	["path", {
		d: "m15.5 9.5 5 5",
		key: "ytk86i"
	}],
	["path", {
		d: "m20.5 9.5-5 5",
		key: "17o44f"
	}]
]);
var activeTabStripPointerGestureCount = 0;
function beginTabStripPointerGesture() {
	activeTabStripPointerGestureCount += 1;
	let released = false;
	return () => {
		if (released) return;
		released = true;
		activeTabStripPointerGestureCount = Math.max(0, activeTabStripPointerGestureCount - 1);
	};
}
function isTabStripPointerGestureActive() {
	return activeTabStripPointerGestureCount > 0;
}
var import_react = /* @__PURE__ */ __toESM(require_react());
function useCombinedRefs() {
	for (var _len = arguments.length, refs = new Array(_len), _key = 0; _key < _len; _key++) refs[_key] = arguments[_key];
	return (0, import_react.useMemo)(() => (node) => {
		refs.forEach((ref) => ref(node));
	}, refs);
}
var canUseDOM = typeof window !== "undefined" && typeof window.document !== "undefined" && typeof window.document.createElement !== "undefined";
function isWindow(element) {
	const elementString = Object.prototype.toString.call(element);
	return elementString === "[object Window]" || elementString === "[object global]";
}
function isNode(node) {
	return "nodeType" in node;
}
function getWindow(target) {
	var _target$ownerDocument, _target$ownerDocument2;
	if (!target) return window;
	if (isWindow(target)) return target;
	if (!isNode(target)) return window;
	return (_target$ownerDocument = (_target$ownerDocument2 = target.ownerDocument) == null ? void 0 : _target$ownerDocument2.defaultView) != null ? _target$ownerDocument : window;
}
function isDocument(node) {
	const { Document: Document$1 } = getWindow(node);
	return node instanceof Document$1;
}
function isHTMLElement(node) {
	if (isWindow(node)) return false;
	return node instanceof getWindow(node).HTMLElement;
}
function isSVGElement(node) {
	return node instanceof getWindow(node).SVGElement;
}
function getOwnerDocument$1(target) {
	if (!target) return document;
	if (isWindow(target)) return target.document;
	if (!isNode(target)) return document;
	if (isDocument(target)) return target;
	if (isHTMLElement(target) || isSVGElement(target)) return target.ownerDocument;
	return document;
}
var useIsomorphicLayoutEffect = canUseDOM ? import_react.useLayoutEffect : import_react.useEffect;
function useEvent(handler) {
	const handlerRef = (0, import_react.useRef)(handler);
	useIsomorphicLayoutEffect(() => {
		handlerRef.current = handler;
	});
	return (0, import_react.useCallback)(function() {
		for (var _len = arguments.length, args = new Array(_len), _key = 0; _key < _len; _key++) args[_key] = arguments[_key];
		return handlerRef.current == null ? void 0 : handlerRef.current(...args);
	}, []);
}
function useInterval() {
	const intervalRef = (0, import_react.useRef)(null);
	return [(0, import_react.useCallback)((listener, duration) => {
		intervalRef.current = setInterval(listener, duration);
	}, []), (0, import_react.useCallback)(() => {
		if (intervalRef.current !== null) {
			clearInterval(intervalRef.current);
			intervalRef.current = null;
		}
	}, [])];
}
function useLatestValue(value, dependencies) {
	if (dependencies === void 0) dependencies = [value];
	const valueRef = (0, import_react.useRef)(value);
	useIsomorphicLayoutEffect(() => {
		if (valueRef.current !== value) valueRef.current = value;
	}, dependencies);
	return valueRef;
}
function useLazyMemo(callback, dependencies) {
	const valueRef = (0, import_react.useRef)();
	return (0, import_react.useMemo)(() => {
		const newValue = callback(valueRef.current);
		valueRef.current = newValue;
		return newValue;
	}, [...dependencies]);
}
function useNodeRef(onChange) {
	const onChangeHandler = useEvent(onChange);
	const node = (0, import_react.useRef)(null);
	return [node, (0, import_react.useCallback)((element) => {
		if (element !== node.current) onChangeHandler?.(element, node.current);
		node.current = element;
	}, [])];
}
function usePrevious(value) {
	const ref = (0, import_react.useRef)();
	(0, import_react.useEffect)(() => {
		ref.current = value;
	}, [value]);
	return ref.current;
}
var ids = {};
function useUniqueId(prefix, value) {
	return (0, import_react.useMemo)(() => {
		if (value) return value;
		const id = ids[prefix] == null ? 0 : ids[prefix] + 1;
		ids[prefix] = id;
		return prefix + "-" + id;
	}, [prefix, value]);
}
function createAdjustmentFn(modifier) {
	return function(object) {
		for (var _len = arguments.length, adjustments = new Array(_len > 1 ? _len - 1 : 0), _key = 1; _key < _len; _key++) adjustments[_key - 1] = arguments[_key];
		return adjustments.reduce((accumulator, adjustment) => {
			const entries = Object.entries(adjustment);
			for (const [key$1, valueAdjustment] of entries) {
				const value = accumulator[key$1];
				if (value != null) accumulator[key$1] = value + modifier * valueAdjustment;
			}
			return accumulator;
		}, { ...object });
	};
}
var add = /* @__PURE__ */ createAdjustmentFn(1);
var subtract = /* @__PURE__ */ createAdjustmentFn(-1);
function hasViewportRelativeCoordinates(event) {
	return "clientX" in event && "clientY" in event;
}
function isKeyboardEvent(event) {
	if (!event) return false;
	const { KeyboardEvent } = getWindow(event.target);
	return KeyboardEvent && event instanceof KeyboardEvent;
}
function isTouchEvent(event) {
	if (!event) return false;
	const { TouchEvent } = getWindow(event.target);
	return TouchEvent && event instanceof TouchEvent;
}
function getEventCoordinates(event) {
	if (isTouchEvent(event)) {
		if (event.touches && event.touches.length) {
			const { clientX: x, clientY: y } = event.touches[0];
			return {
				x,
				y
			};
		} else if (event.changedTouches && event.changedTouches.length) {
			const { clientX: x, clientY: y } = event.changedTouches[0];
			return {
				x,
				y
			};
		}
	}
	if (hasViewportRelativeCoordinates(event)) return {
		x: event.clientX,
		y: event.clientY
	};
	return null;
}
var CSS$1 = /* @__PURE__ */ Object.freeze({
	Translate: { toString(transform) {
		if (!transform) return;
		const { x, y } = transform;
		return "translate3d(" + (x ? Math.round(x) : 0) + "px, " + (y ? Math.round(y) : 0) + "px, 0)";
	} },
	Scale: { toString(transform) {
		if (!transform) return;
		const { scaleX, scaleY } = transform;
		return "scaleX(" + scaleX + ") scaleY(" + scaleY + ")";
	} },
	Transform: { toString(transform) {
		if (!transform) return;
		return [CSS$1.Translate.toString(transform), CSS$1.Scale.toString(transform)].join(" ");
	} },
	Transition: { toString(_ref) {
		let { property, duration, easing } = _ref;
		return property + " " + duration + "ms " + easing;
	} }
});
var SELECTOR = "a,frame,iframe,input:not([type=hidden]):not(:disabled),select:not(:disabled),textarea:not(:disabled),button:not(:disabled),*[tabindex]";
function findFirstFocusableNode(element) {
	if (element.matches(SELECTOR)) return element;
	return element.querySelector(SELECTOR);
}
var hiddenStyles = { display: "none" };
function HiddenText(_ref) {
	let { id, value } = _ref;
	return import_react.createElement("div", {
		id,
		style: hiddenStyles
	}, value);
}
function LiveRegion(_ref) {
	let { id, announcement, ariaLiveType = "assertive" } = _ref;
	return import_react.createElement("div", {
		id,
		style: {
			position: "fixed",
			top: 0,
			left: 0,
			width: 1,
			height: 1,
			margin: -1,
			border: 0,
			padding: 0,
			overflow: "hidden",
			clip: "rect(0 0 0 0)",
			clipPath: "inset(100%)",
			whiteSpace: "nowrap"
		},
		role: "status",
		"aria-live": ariaLiveType,
		"aria-atomic": true
	}, announcement);
}
function useAnnouncement() {
	const [announcement, setAnnouncement] = (0, import_react.useState)("");
	return {
		announce: (0, import_react.useCallback)((value) => {
			if (value != null) setAnnouncement(value);
		}, []),
		announcement
	};
}
var import_react_dom = require_react_dom();
var DndMonitorContext = /* @__PURE__ */ (0, import_react.createContext)(null);
function useDndMonitor(listener) {
	const registerListener = (0, import_react.useContext)(DndMonitorContext);
	(0, import_react.useEffect)(() => {
		if (!registerListener) throw new Error("useDndMonitor must be used within a children of <DndContext>");
		return registerListener(listener);
	}, [listener, registerListener]);
}
function useDndMonitorProvider() {
	const [listeners] = (0, import_react.useState)(() => /* @__PURE__ */ new Set());
	const registerListener = (0, import_react.useCallback)((listener) => {
		listeners.add(listener);
		return () => listeners.delete(listener);
	}, [listeners]);
	return [(0, import_react.useCallback)((_ref) => {
		let { type, event } = _ref;
		listeners.forEach((listener) => {
			var _listener$type;
			return (_listener$type = listener[type]) == null ? void 0 : _listener$type.call(listener, event);
		});
	}, [listeners]), registerListener];
}
var defaultScreenReaderInstructions = { draggable: "\n    To pick up a draggable item, press the space bar.\n    While dragging, use the arrow keys to move the item.\n    Press space again to drop the item in its new position, or press escape to cancel.\n  " };
var defaultAnnouncements = {
	onDragStart(_ref) {
		let { active } = _ref;
		return "Picked up draggable item " + active.id + ".";
	},
	onDragOver(_ref2) {
		let { active, over } = _ref2;
		if (over) return "Draggable item " + active.id + " was moved over droppable area " + over.id + ".";
		return "Draggable item " + active.id + " is no longer over a droppable area.";
	},
	onDragEnd(_ref3) {
		let { active, over } = _ref3;
		if (over) return "Draggable item " + active.id + " was dropped over droppable area " + over.id;
		return "Draggable item " + active.id + " was dropped.";
	},
	onDragCancel(_ref4) {
		let { active } = _ref4;
		return "Dragging was cancelled. Draggable item " + active.id + " was dropped.";
	}
};
function Accessibility(_ref) {
	let { announcements = defaultAnnouncements, container, hiddenTextDescribedById, screenReaderInstructions = defaultScreenReaderInstructions } = _ref;
	const { announce, announcement } = useAnnouncement();
	const liveRegionId = useUniqueId("DndLiveRegion");
	const [mounted, setMounted] = (0, import_react.useState)(false);
	(0, import_react.useEffect)(() => {
		setMounted(true);
	}, []);
	useDndMonitor((0, import_react.useMemo)(() => ({
		onDragStart(_ref2) {
			let { active } = _ref2;
			announce(announcements.onDragStart({ active }));
		},
		onDragMove(_ref3) {
			let { active, over } = _ref3;
			if (announcements.onDragMove) announce(announcements.onDragMove({
				active,
				over
			}));
		},
		onDragOver(_ref4) {
			let { active, over } = _ref4;
			announce(announcements.onDragOver({
				active,
				over
			}));
		},
		onDragEnd(_ref5) {
			let { active, over } = _ref5;
			announce(announcements.onDragEnd({
				active,
				over
			}));
		},
		onDragCancel(_ref6) {
			let { active, over } = _ref6;
			announce(announcements.onDragCancel({
				active,
				over
			}));
		}
	}), [announce, announcements]));
	if (!mounted) return null;
	const markup = import_react.createElement(import_react.Fragment, null, import_react.createElement(HiddenText, {
		id: hiddenTextDescribedById,
		value: screenReaderInstructions.draggable
	}), import_react.createElement(LiveRegion, {
		id: liveRegionId,
		announcement
	}));
	return container ? (0, import_react_dom.createPortal)(markup, container) : markup;
}
var Action;
(function(Action$1) {
	Action$1["DragStart"] = "dragStart";
	Action$1["DragMove"] = "dragMove";
	Action$1["DragEnd"] = "dragEnd";
	Action$1["DragCancel"] = "dragCancel";
	Action$1["DragOver"] = "dragOver";
	Action$1["RegisterDroppable"] = "registerDroppable";
	Action$1["SetDroppableDisabled"] = "setDroppableDisabled";
	Action$1["UnregisterDroppable"] = "unregisterDroppable";
})(Action || (Action = {}));
function noop() {}
function useSensor(sensor, options) {
	return (0, import_react.useMemo)(() => ({
		sensor,
		options: options != null ? options : {}
	}), [sensor, options]);
}
function useSensors() {
	for (var _len = arguments.length, sensors = new Array(_len), _key = 0; _key < _len; _key++) sensors[_key] = arguments[_key];
	return (0, import_react.useMemo)(() => [...sensors].filter((sensor) => sensor != null), [...sensors]);
}
var defaultCoordinates = /* @__PURE__ */ Object.freeze({
	x: 0,
	y: 0
});
function distanceBetween(p1, p2) {
	return Math.sqrt(Math.pow(p1.x - p2.x, 2) + Math.pow(p1.y - p2.y, 2));
}
function getRelativeTransformOrigin(event, rect) {
	const eventCoordinates = getEventCoordinates(event);
	if (!eventCoordinates) return "0 0";
	const transformOrigin = {
		x: (eventCoordinates.x - rect.left) / rect.width * 100,
		y: (eventCoordinates.y - rect.top) / rect.height * 100
	};
	return transformOrigin.x + "% " + transformOrigin.y + "%";
}
function sortCollisionsAsc(_ref, _ref2) {
	let { data: { value: a } } = _ref;
	let { data: { value: b } } = _ref2;
	return a - b;
}
function sortCollisionsDesc(_ref3, _ref4) {
	let { data: { value: a } } = _ref3;
	let { data: { value: b } } = _ref4;
	return b - a;
}
function cornersOfRectangle(_ref5) {
	let { left, top, height, width } = _ref5;
	return [
		{
			x: left,
			y: top
		},
		{
			x: left + width,
			y: top
		},
		{
			x: left,
			y: top + height
		},
		{
			x: left + width,
			y: top + height
		}
	];
}
function getFirstCollision(collisions, property) {
	if (!collisions || collisions.length === 0) return null;
	const [firstCollision] = collisions;
	return property ? firstCollision[property] : firstCollision;
}
function centerOfRectangle(rect, left, top) {
	if (left === void 0) left = rect.left;
	if (top === void 0) top = rect.top;
	return {
		x: left + rect.width * .5,
		y: top + rect.height * .5
	};
}
var closestCenter = (_ref) => {
	let { collisionRect, droppableRects, droppableContainers } = _ref;
	const centerRect = centerOfRectangle(collisionRect, collisionRect.left, collisionRect.top);
	const collisions = [];
	for (const droppableContainer of droppableContainers) {
		const { id } = droppableContainer;
		const rect = droppableRects.get(id);
		if (rect) {
			const distBetween = distanceBetween(centerOfRectangle(rect), centerRect);
			collisions.push({
				id,
				data: {
					droppableContainer,
					value: distBetween
				}
			});
		}
	}
	return collisions.sort(sortCollisionsAsc);
};
function getIntersectionRatio(entry, target) {
	const top = Math.max(target.top, entry.top);
	const left = Math.max(target.left, entry.left);
	const right = Math.min(target.left + target.width, entry.left + entry.width);
	const bottom = Math.min(target.top + target.height, entry.top + entry.height);
	const width = right - left;
	const height = bottom - top;
	if (left < right && top < bottom) {
		const targetArea = target.width * target.height;
		const entryArea = entry.width * entry.height;
		const intersectionArea = width * height;
		const intersectionRatio = intersectionArea / (targetArea + entryArea - intersectionArea);
		return Number(intersectionRatio.toFixed(4));
	}
	return 0;
}
var rectIntersection = (_ref) => {
	let { collisionRect, droppableRects, droppableContainers } = _ref;
	const collisions = [];
	for (const droppableContainer of droppableContainers) {
		const { id } = droppableContainer;
		const rect = droppableRects.get(id);
		if (rect) {
			const intersectionRatio = getIntersectionRatio(rect, collisionRect);
			if (intersectionRatio > 0) collisions.push({
				id,
				data: {
					droppableContainer,
					value: intersectionRatio
				}
			});
		}
	}
	return collisions.sort(sortCollisionsDesc);
};
function isPointWithinRect(point, rect) {
	const { top, left, bottom, right } = rect;
	return top <= point.y && point.y <= bottom && left <= point.x && point.x <= right;
}
var pointerWithin = (_ref) => {
	let { droppableContainers, droppableRects, pointerCoordinates } = _ref;
	if (!pointerCoordinates) return [];
	const collisions = [];
	for (const droppableContainer of droppableContainers) {
		const { id } = droppableContainer;
		const rect = droppableRects.get(id);
		if (rect && isPointWithinRect(pointerCoordinates, rect)) {
			const distances = cornersOfRectangle(rect).reduce((accumulator, corner) => {
				return accumulator + distanceBetween(pointerCoordinates, corner);
			}, 0);
			const effectiveDistance = Number((distances / 4).toFixed(4));
			collisions.push({
				id,
				data: {
					droppableContainer,
					value: effectiveDistance
				}
			});
		}
	}
	return collisions.sort(sortCollisionsAsc);
};
function adjustScale(transform, rect1, rect2) {
	return {
		...transform,
		scaleX: rect1 && rect2 ? rect1.width / rect2.width : 1,
		scaleY: rect1 && rect2 ? rect1.height / rect2.height : 1
	};
}
function getRectDelta(rect1, rect2) {
	return rect1 && rect2 ? {
		x: rect1.left - rect2.left,
		y: rect1.top - rect2.top
	} : defaultCoordinates;
}
function createRectAdjustmentFn(modifier) {
	return function adjustClientRect(rect) {
		for (var _len = arguments.length, adjustments = new Array(_len > 1 ? _len - 1 : 0), _key = 1; _key < _len; _key++) adjustments[_key - 1] = arguments[_key];
		return adjustments.reduce((acc, adjustment) => ({
			...acc,
			top: acc.top + modifier * adjustment.y,
			bottom: acc.bottom + modifier * adjustment.y,
			left: acc.left + modifier * adjustment.x,
			right: acc.right + modifier * adjustment.x
		}), { ...rect });
	};
}
var getAdjustedRect = /* @__PURE__ */ createRectAdjustmentFn(1);
function parseTransform(transform) {
	if (transform.startsWith("matrix3d(")) {
		const transformArray = transform.slice(9, -1).split(/, /);
		return {
			x: +transformArray[12],
			y: +transformArray[13],
			scaleX: +transformArray[0],
			scaleY: +transformArray[5]
		};
	} else if (transform.startsWith("matrix(")) {
		const transformArray = transform.slice(7, -1).split(/, /);
		return {
			x: +transformArray[4],
			y: +transformArray[5],
			scaleX: +transformArray[0],
			scaleY: +transformArray[3]
		};
	}
	return null;
}
function inverseTransform(rect, transform, transformOrigin) {
	const parsedTransform = parseTransform(transform);
	if (!parsedTransform) return rect;
	const { scaleX, scaleY, x: translateX, y: translateY } = parsedTransform;
	const x = rect.left - translateX - (1 - scaleX) * parseFloat(transformOrigin);
	const y = rect.top - translateY - (1 - scaleY) * parseFloat(transformOrigin.slice(transformOrigin.indexOf(" ") + 1));
	const w = scaleX ? rect.width / scaleX : rect.width;
	const h = scaleY ? rect.height / scaleY : rect.height;
	return {
		width: w,
		height: h,
		top: y,
		right: x + w,
		bottom: y + h,
		left: x
	};
}
var defaultOptions = { ignoreTransform: false };
function getClientRect(element, options) {
	if (options === void 0) options = defaultOptions;
	let rect = element.getBoundingClientRect();
	if (options.ignoreTransform) {
		const { transform, transformOrigin } = getWindow(element).getComputedStyle(element);
		if (transform) rect = inverseTransform(rect, transform, transformOrigin);
	}
	const { top, left, width, height, bottom, right } = rect;
	return {
		top,
		left,
		width,
		height,
		bottom,
		right
	};
}
function getTransformAgnosticClientRect(element) {
	return getClientRect(element, { ignoreTransform: true });
}
function getWindowClientRect(element) {
	const width = element.innerWidth;
	const height = element.innerHeight;
	return {
		top: 0,
		left: 0,
		right: width,
		bottom: height,
		width,
		height
	};
}
function isFixed(node, computedStyle) {
	if (computedStyle === void 0) computedStyle = getWindow(node).getComputedStyle(node);
	return computedStyle.position === "fixed";
}
function isScrollable(element, computedStyle) {
	if (computedStyle === void 0) computedStyle = getWindow(element).getComputedStyle(element);
	const overflowRegex = /(auto|scroll|overlay)/;
	return [
		"overflow",
		"overflowX",
		"overflowY"
	].some((property) => {
		const value = computedStyle[property];
		return typeof value === "string" ? overflowRegex.test(value) : false;
	});
}
function getScrollableAncestors(element, limit) {
	const scrollParents = [];
	function findScrollableAncestors(node) {
		if (limit != null && scrollParents.length >= limit) return scrollParents;
		if (!node) return scrollParents;
		if (isDocument(node) && node.scrollingElement != null && !scrollParents.includes(node.scrollingElement)) {
			scrollParents.push(node.scrollingElement);
			return scrollParents;
		}
		if (!isHTMLElement(node) || isSVGElement(node)) return scrollParents;
		if (scrollParents.includes(node)) return scrollParents;
		const computedStyle = getWindow(element).getComputedStyle(node);
		if (node !== element) {
			if (isScrollable(node, computedStyle)) scrollParents.push(node);
		}
		if (isFixed(node, computedStyle)) return scrollParents;
		return findScrollableAncestors(node.parentNode);
	}
	if (!element) return scrollParents;
	return findScrollableAncestors(element);
}
function getFirstScrollableAncestor(node) {
	const [firstScrollableAncestor] = getScrollableAncestors(node, 1);
	return firstScrollableAncestor != null ? firstScrollableAncestor : null;
}
function getScrollableElement(element) {
	if (!canUseDOM || !element) return null;
	if (isWindow(element)) return element;
	if (!isNode(element)) return null;
	if (isDocument(element) || element === getOwnerDocument$1(element).scrollingElement) return window;
	if (isHTMLElement(element)) return element;
	return null;
}
function getScrollXCoordinate(element) {
	if (isWindow(element)) return element.scrollX;
	return element.scrollLeft;
}
function getScrollYCoordinate(element) {
	if (isWindow(element)) return element.scrollY;
	return element.scrollTop;
}
function getScrollCoordinates(element) {
	return {
		x: getScrollXCoordinate(element),
		y: getScrollYCoordinate(element)
	};
}
var Direction;
(function(Direction$1) {
	Direction$1[Direction$1["Forward"] = 1] = "Forward";
	Direction$1[Direction$1["Backward"] = -1] = "Backward";
})(Direction || (Direction = {}));
function isDocumentScrollingElement(element) {
	if (!canUseDOM || !element) return false;
	return element === document.scrollingElement;
}
function getScrollPosition(scrollingContainer) {
	const minScroll = {
		x: 0,
		y: 0
	};
	const dimensions = isDocumentScrollingElement(scrollingContainer) ? {
		height: window.innerHeight,
		width: window.innerWidth
	} : {
		height: scrollingContainer.clientHeight,
		width: scrollingContainer.clientWidth
	};
	const maxScroll = {
		x: scrollingContainer.scrollWidth - dimensions.width,
		y: scrollingContainer.scrollHeight - dimensions.height
	};
	return {
		isTop: scrollingContainer.scrollTop <= minScroll.y,
		isLeft: scrollingContainer.scrollLeft <= minScroll.x,
		isBottom: scrollingContainer.scrollTop >= maxScroll.y,
		isRight: scrollingContainer.scrollLeft >= maxScroll.x,
		maxScroll,
		minScroll
	};
}
var defaultThreshold = {
	x: .2,
	y: .2
};
function getScrollDirectionAndSpeed(scrollContainer, scrollContainerRect, _ref, acceleration, thresholdPercentage) {
	let { top, left, right, bottom } = _ref;
	if (acceleration === void 0) acceleration = 10;
	if (thresholdPercentage === void 0) thresholdPercentage = defaultThreshold;
	const { isTop, isBottom, isLeft, isRight } = getScrollPosition(scrollContainer);
	const direction = {
		x: 0,
		y: 0
	};
	const speed = {
		x: 0,
		y: 0
	};
	const threshold = {
		height: scrollContainerRect.height * thresholdPercentage.y,
		width: scrollContainerRect.width * thresholdPercentage.x
	};
	if (!isTop && top <= scrollContainerRect.top + threshold.height) {
		direction.y = Direction.Backward;
		speed.y = acceleration * Math.abs((scrollContainerRect.top + threshold.height - top) / threshold.height);
	} else if (!isBottom && bottom >= scrollContainerRect.bottom - threshold.height) {
		direction.y = Direction.Forward;
		speed.y = acceleration * Math.abs((scrollContainerRect.bottom - threshold.height - bottom) / threshold.height);
	}
	if (!isRight && right >= scrollContainerRect.right - threshold.width) {
		direction.x = Direction.Forward;
		speed.x = acceleration * Math.abs((scrollContainerRect.right - threshold.width - right) / threshold.width);
	} else if (!isLeft && left <= scrollContainerRect.left + threshold.width) {
		direction.x = Direction.Backward;
		speed.x = acceleration * Math.abs((scrollContainerRect.left + threshold.width - left) / threshold.width);
	}
	return {
		direction,
		speed
	};
}
function getScrollElementRect(element) {
	if (element === document.scrollingElement) {
		const { innerWidth, innerHeight } = window;
		return {
			top: 0,
			left: 0,
			right: innerWidth,
			bottom: innerHeight,
			width: innerWidth,
			height: innerHeight
		};
	}
	const { top, left, right, bottom } = element.getBoundingClientRect();
	return {
		top,
		left,
		right,
		bottom,
		width: element.clientWidth,
		height: element.clientHeight
	};
}
function getScrollOffsets(scrollableAncestors) {
	return scrollableAncestors.reduce((acc, node) => {
		return add(acc, getScrollCoordinates(node));
	}, defaultCoordinates);
}
function getScrollXOffset(scrollableAncestors) {
	return scrollableAncestors.reduce((acc, node) => {
		return acc + getScrollXCoordinate(node);
	}, 0);
}
function getScrollYOffset(scrollableAncestors) {
	return scrollableAncestors.reduce((acc, node) => {
		return acc + getScrollYCoordinate(node);
	}, 0);
}
function scrollIntoViewIfNeeded(element, measure) {
	if (measure === void 0) measure = getClientRect;
	if (!element) return;
	const { top, left, bottom, right } = measure(element);
	if (!getFirstScrollableAncestor(element)) return;
	if (bottom <= 0 || right <= 0 || top >= window.innerHeight || left >= window.innerWidth) element.scrollIntoView({
		block: "center",
		inline: "center"
	});
}
var properties = [[
	"x",
	["left", "right"],
	getScrollXOffset
], [
	"y",
	["top", "bottom"],
	getScrollYOffset
]];
var Rect = class {
	constructor(rect, element) {
		this.rect = void 0;
		this.width = void 0;
		this.height = void 0;
		this.top = void 0;
		this.bottom = void 0;
		this.right = void 0;
		this.left = void 0;
		const scrollableAncestors = getScrollableAncestors(element);
		const scrollOffsets = getScrollOffsets(scrollableAncestors);
		this.rect = { ...rect };
		this.width = rect.width;
		this.height = rect.height;
		for (const [axis, keys, getScrollOffset] of properties) for (const key$1 of keys) Object.defineProperty(this, key$1, {
			get: () => {
				const currentOffsets = getScrollOffset(scrollableAncestors);
				const scrollOffsetsDeltla = scrollOffsets[axis] - currentOffsets;
				return this.rect[key$1] + scrollOffsetsDeltla;
			},
			enumerable: true
		});
		Object.defineProperty(this, "rect", { enumerable: false });
	}
};
var Listeners = class {
	constructor(target) {
		this.target = void 0;
		this.listeners = [];
		this.removeAll = () => {
			this.listeners.forEach((listener) => {
				var _this$target;
				return (_this$target = this.target) == null ? void 0 : _this$target.removeEventListener(...listener);
			});
		};
		this.target = target;
	}
	add(eventName, handler, options) {
		var _this$target2;
		(_this$target2 = this.target) == null || _this$target2.addEventListener(eventName, handler, options);
		this.listeners.push([
			eventName,
			handler,
			options
		]);
	}
};
function getEventListenerTarget(target) {
	const { EventTarget } = getWindow(target);
	return target instanceof EventTarget ? target : getOwnerDocument$1(target);
}
function hasExceededDistance$1(delta, measurement) {
	const dx = Math.abs(delta.x);
	const dy = Math.abs(delta.y);
	if (typeof measurement === "number") return Math.sqrt(dx ** 2 + dy ** 2) > measurement;
	if ("x" in measurement && "y" in measurement) return dx > measurement.x && dy > measurement.y;
	if ("x" in measurement) return dx > measurement.x;
	if ("y" in measurement) return dy > measurement.y;
	return false;
}
var EventName;
(function(EventName$1) {
	EventName$1["Click"] = "click";
	EventName$1["DragStart"] = "dragstart";
	EventName$1["Keydown"] = "keydown";
	EventName$1["ContextMenu"] = "contextmenu";
	EventName$1["Resize"] = "resize";
	EventName$1["SelectionChange"] = "selectionchange";
	EventName$1["VisibilityChange"] = "visibilitychange";
})(EventName || (EventName = {}));
function preventDefault$1(event) {
	event.preventDefault();
}
function stopPropagation$1(event) {
	event.stopPropagation();
}
var KeyboardCode;
(function(KeyboardCode$1) {
	KeyboardCode$1["Space"] = "Space";
	KeyboardCode$1["Down"] = "ArrowDown";
	KeyboardCode$1["Right"] = "ArrowRight";
	KeyboardCode$1["Left"] = "ArrowLeft";
	KeyboardCode$1["Up"] = "ArrowUp";
	KeyboardCode$1["Esc"] = "Escape";
	KeyboardCode$1["Enter"] = "Enter";
	KeyboardCode$1["Tab"] = "Tab";
})(KeyboardCode || (KeyboardCode = {}));
var defaultKeyboardCodes = {
	start: [KeyboardCode.Space, KeyboardCode.Enter],
	cancel: [KeyboardCode.Esc],
	end: [
		KeyboardCode.Space,
		KeyboardCode.Enter,
		KeyboardCode.Tab
	]
};
var defaultKeyboardCoordinateGetter = (event, _ref) => {
	let { currentCoordinates } = _ref;
	switch (event.code) {
		case KeyboardCode.Right: return {
			...currentCoordinates,
			x: currentCoordinates.x + 25
		};
		case KeyboardCode.Left: return {
			...currentCoordinates,
			x: currentCoordinates.x - 25
		};
		case KeyboardCode.Down: return {
			...currentCoordinates,
			y: currentCoordinates.y + 25
		};
		case KeyboardCode.Up: return {
			...currentCoordinates,
			y: currentCoordinates.y - 25
		};
	}
};
var KeyboardSensor = class {
	constructor(props) {
		this.props = void 0;
		this.autoScrollEnabled = false;
		this.referenceCoordinates = void 0;
		this.listeners = void 0;
		this.windowListeners = void 0;
		this.props = props;
		const { event: { target } } = props;
		this.props = props;
		this.listeners = new Listeners(getOwnerDocument$1(target));
		this.windowListeners = new Listeners(getWindow(target));
		this.handleKeyDown = this.handleKeyDown.bind(this);
		this.handleCancel = this.handleCancel.bind(this);
		this.attach();
	}
	attach() {
		this.handleStart();
		this.windowListeners.add(EventName.Resize, this.handleCancel);
		this.windowListeners.add(EventName.VisibilityChange, this.handleCancel);
		setTimeout(() => this.listeners.add(EventName.Keydown, this.handleKeyDown));
	}
	handleStart() {
		const { activeNode, onStart } = this.props;
		const node = activeNode.node.current;
		if (node) scrollIntoViewIfNeeded(node);
		onStart(defaultCoordinates);
	}
	handleKeyDown(event) {
		if (isKeyboardEvent(event)) {
			const { active, context, options } = this.props;
			const { keyboardCodes = defaultKeyboardCodes, coordinateGetter = defaultKeyboardCoordinateGetter, scrollBehavior = "smooth" } = options;
			const { code } = event;
			if (keyboardCodes.end.includes(code)) {
				this.handleEnd(event);
				return;
			}
			if (keyboardCodes.cancel.includes(code)) {
				this.handleCancel(event);
				return;
			}
			const { collisionRect } = context.current;
			const currentCoordinates = collisionRect ? {
				x: collisionRect.left,
				y: collisionRect.top
			} : defaultCoordinates;
			if (!this.referenceCoordinates) this.referenceCoordinates = currentCoordinates;
			const newCoordinates = coordinateGetter(event, {
				active,
				context: context.current,
				currentCoordinates
			});
			if (newCoordinates) {
				const coordinatesDelta = subtract(newCoordinates, currentCoordinates);
				const scrollDelta = {
					x: 0,
					y: 0
				};
				const { scrollableAncestors } = context.current;
				for (const scrollContainer of scrollableAncestors) {
					const direction = event.code;
					const { isTop, isRight, isLeft, isBottom, maxScroll, minScroll } = getScrollPosition(scrollContainer);
					const scrollElementRect = getScrollElementRect(scrollContainer);
					const clampedCoordinates = {
						x: Math.min(direction === KeyboardCode.Right ? scrollElementRect.right - scrollElementRect.width / 2 : scrollElementRect.right, Math.max(direction === KeyboardCode.Right ? scrollElementRect.left : scrollElementRect.left + scrollElementRect.width / 2, newCoordinates.x)),
						y: Math.min(direction === KeyboardCode.Down ? scrollElementRect.bottom - scrollElementRect.height / 2 : scrollElementRect.bottom, Math.max(direction === KeyboardCode.Down ? scrollElementRect.top : scrollElementRect.top + scrollElementRect.height / 2, newCoordinates.y))
					};
					const canScrollX = direction === KeyboardCode.Right && !isRight || direction === KeyboardCode.Left && !isLeft;
					const canScrollY = direction === KeyboardCode.Down && !isBottom || direction === KeyboardCode.Up && !isTop;
					if (canScrollX && clampedCoordinates.x !== newCoordinates.x) {
						const newScrollCoordinates = scrollContainer.scrollLeft + coordinatesDelta.x;
						const canScrollToNewCoordinates = direction === KeyboardCode.Right && newScrollCoordinates <= maxScroll.x || direction === KeyboardCode.Left && newScrollCoordinates >= minScroll.x;
						if (canScrollToNewCoordinates && !coordinatesDelta.y) {
							scrollContainer.scrollTo({
								left: newScrollCoordinates,
								behavior: scrollBehavior
							});
							return;
						}
						if (canScrollToNewCoordinates) scrollDelta.x = scrollContainer.scrollLeft - newScrollCoordinates;
						else scrollDelta.x = direction === KeyboardCode.Right ? scrollContainer.scrollLeft - maxScroll.x : scrollContainer.scrollLeft - minScroll.x;
						if (scrollDelta.x) scrollContainer.scrollBy({
							left: -scrollDelta.x,
							behavior: scrollBehavior
						});
						break;
					} else if (canScrollY && clampedCoordinates.y !== newCoordinates.y) {
						const newScrollCoordinates = scrollContainer.scrollTop + coordinatesDelta.y;
						const canScrollToNewCoordinates = direction === KeyboardCode.Down && newScrollCoordinates <= maxScroll.y || direction === KeyboardCode.Up && newScrollCoordinates >= minScroll.y;
						if (canScrollToNewCoordinates && !coordinatesDelta.x) {
							scrollContainer.scrollTo({
								top: newScrollCoordinates,
								behavior: scrollBehavior
							});
							return;
						}
						if (canScrollToNewCoordinates) scrollDelta.y = scrollContainer.scrollTop - newScrollCoordinates;
						else scrollDelta.y = direction === KeyboardCode.Down ? scrollContainer.scrollTop - maxScroll.y : scrollContainer.scrollTop - minScroll.y;
						if (scrollDelta.y) scrollContainer.scrollBy({
							top: -scrollDelta.y,
							behavior: scrollBehavior
						});
						break;
					}
				}
				this.handleMove(event, add(subtract(newCoordinates, this.referenceCoordinates), scrollDelta));
			}
		}
	}
	handleMove(event, coordinates) {
		const { onMove } = this.props;
		event.preventDefault();
		onMove(coordinates);
	}
	handleEnd(event) {
		const { onEnd } = this.props;
		event.preventDefault();
		this.detach();
		onEnd();
	}
	handleCancel(event) {
		const { onCancel } = this.props;
		event.preventDefault();
		this.detach();
		onCancel();
	}
	detach() {
		this.listeners.removeAll();
		this.windowListeners.removeAll();
	}
};
KeyboardSensor.activators = [{
	eventName: "onKeyDown",
	handler: (event, _ref, _ref2) => {
		let { keyboardCodes = defaultKeyboardCodes, onActivation } = _ref;
		let { active } = _ref2;
		const { code } = event.nativeEvent;
		if (keyboardCodes.start.includes(code)) {
			const activator = active.activatorNode.current;
			if (activator && event.target !== activator) return false;
			event.preventDefault();
			onActivation?.({ event: event.nativeEvent });
			return true;
		}
		return false;
	}
}];
function isDistanceConstraint$1(constraint) {
	return Boolean(constraint && "distance" in constraint);
}
function isDelayConstraint$1(constraint) {
	return Boolean(constraint && "delay" in constraint);
}
var AbstractPointerSensor = class {
	constructor(props, events$3, listenerTarget) {
		var _getEventCoordinates;
		if (listenerTarget === void 0) listenerTarget = getEventListenerTarget(props.event.target);
		this.props = void 0;
		this.events = void 0;
		this.autoScrollEnabled = true;
		this.document = void 0;
		this.activated = false;
		this.initialCoordinates = void 0;
		this.timeoutId = null;
		this.listeners = void 0;
		this.documentListeners = void 0;
		this.windowListeners = void 0;
		this.props = props;
		this.events = events$3;
		const { event } = props;
		const { target } = event;
		this.props = props;
		this.events = events$3;
		this.document = getOwnerDocument$1(target);
		this.documentListeners = new Listeners(this.document);
		this.listeners = new Listeners(listenerTarget);
		this.windowListeners = new Listeners(getWindow(target));
		this.initialCoordinates = (_getEventCoordinates = getEventCoordinates(event)) != null ? _getEventCoordinates : defaultCoordinates;
		this.handleStart = this.handleStart.bind(this);
		this.handleMove = this.handleMove.bind(this);
		this.handleEnd = this.handleEnd.bind(this);
		this.handleCancel = this.handleCancel.bind(this);
		this.handleKeydown = this.handleKeydown.bind(this);
		this.removeTextSelection = this.removeTextSelection.bind(this);
		this.attach();
	}
	attach() {
		const { events: events$3, props: { options: { activationConstraint, bypassActivationConstraint } } } = this;
		this.listeners.add(events$3.move.name, this.handleMove, { passive: false });
		this.listeners.add(events$3.end.name, this.handleEnd);
		if (events$3.cancel) this.listeners.add(events$3.cancel.name, this.handleCancel);
		this.windowListeners.add(EventName.Resize, this.handleCancel);
		this.windowListeners.add(EventName.DragStart, preventDefault$1);
		this.windowListeners.add(EventName.VisibilityChange, this.handleCancel);
		this.windowListeners.add(EventName.ContextMenu, preventDefault$1);
		this.documentListeners.add(EventName.Keydown, this.handleKeydown);
		if (activationConstraint) {
			if (bypassActivationConstraint != null && bypassActivationConstraint({
				event: this.props.event,
				activeNode: this.props.activeNode,
				options: this.props.options
			})) return this.handleStart();
			if (isDelayConstraint$1(activationConstraint)) {
				this.timeoutId = setTimeout(this.handleStart, activationConstraint.delay);
				this.handlePending(activationConstraint);
				return;
			}
			if (isDistanceConstraint$1(activationConstraint)) {
				this.handlePending(activationConstraint);
				return;
			}
		}
		this.handleStart();
	}
	detach() {
		this.listeners.removeAll();
		this.windowListeners.removeAll();
		setTimeout(this.documentListeners.removeAll, 50);
		if (this.timeoutId !== null) {
			clearTimeout(this.timeoutId);
			this.timeoutId = null;
		}
	}
	handlePending(constraint, offset) {
		const { active, onPending } = this.props;
		onPending(active, constraint, this.initialCoordinates, offset);
	}
	handleStart() {
		const { initialCoordinates } = this;
		const { onStart } = this.props;
		if (initialCoordinates) {
			this.activated = true;
			this.documentListeners.add(EventName.Click, stopPropagation$1, { capture: true });
			this.removeTextSelection();
			this.documentListeners.add(EventName.SelectionChange, this.removeTextSelection);
			onStart(initialCoordinates);
		}
	}
	handleMove(event) {
		var _getEventCoordinates2;
		const { activated, initialCoordinates, props } = this;
		const { onMove, options: { activationConstraint } } = props;
		if (!initialCoordinates) return;
		const coordinates = (_getEventCoordinates2 = getEventCoordinates(event)) != null ? _getEventCoordinates2 : defaultCoordinates;
		const delta = subtract(initialCoordinates, coordinates);
		if (!activated && activationConstraint) {
			if (isDistanceConstraint$1(activationConstraint)) {
				if (activationConstraint.tolerance != null && hasExceededDistance$1(delta, activationConstraint.tolerance)) return this.handleCancel();
				if (hasExceededDistance$1(delta, activationConstraint.distance)) return this.handleStart();
			}
			if (isDelayConstraint$1(activationConstraint)) {
				if (hasExceededDistance$1(delta, activationConstraint.tolerance)) return this.handleCancel();
			}
			this.handlePending(activationConstraint, delta);
			return;
		}
		if (event.cancelable) event.preventDefault();
		onMove(coordinates);
	}
	handleEnd() {
		const { onAbort, onEnd } = this.props;
		this.detach();
		if (!this.activated) onAbort(this.props.active);
		onEnd();
	}
	handleCancel() {
		const { onAbort, onCancel } = this.props;
		this.detach();
		if (!this.activated) onAbort(this.props.active);
		onCancel();
	}
	handleKeydown(event) {
		if (event.code === KeyboardCode.Esc) this.handleCancel();
	}
	removeTextSelection() {
		var _this$document$getSel;
		(_this$document$getSel = this.document.getSelection()) == null || _this$document$getSel.removeAllRanges();
	}
};
var events = {
	cancel: { name: "pointercancel" },
	move: { name: "pointermove" },
	end: { name: "pointerup" }
};
var PointerSensor = class extends AbstractPointerSensor {
	constructor(props) {
		const { event } = props;
		const listenerTarget = getOwnerDocument$1(event.target);
		super(props, events, listenerTarget);
	}
};
PointerSensor.activators = [{
	eventName: "onPointerDown",
	handler: (_ref, _ref2) => {
		let { nativeEvent: event } = _ref;
		let { onActivation } = _ref2;
		if (!event.isPrimary || event.button !== 0) return false;
		onActivation?.({ event });
		return true;
	}
}];
var events$1 = {
	move: { name: "mousemove" },
	end: { name: "mouseup" }
};
var MouseButton;
(function(MouseButton$1) {
	MouseButton$1[MouseButton$1["RightClick"] = 2] = "RightClick";
})(MouseButton || (MouseButton = {}));
var MouseSensor = class extends AbstractPointerSensor {
	constructor(props) {
		super(props, events$1, getOwnerDocument$1(props.event.target));
	}
};
MouseSensor.activators = [{
	eventName: "onMouseDown",
	handler: (_ref, _ref2) => {
		let { nativeEvent: event } = _ref;
		let { onActivation } = _ref2;
		if (event.button === MouseButton.RightClick) return false;
		onActivation?.({ event });
		return true;
	}
}];
var events$2 = {
	cancel: { name: "touchcancel" },
	move: { name: "touchmove" },
	end: { name: "touchend" }
};
var TouchSensor = class extends AbstractPointerSensor {
	constructor(props) {
		super(props, events$2);
	}
	static setup() {
		window.addEventListener(events$2.move.name, noop$1, {
			capture: false,
			passive: false
		});
		return function teardown() {
			window.removeEventListener(events$2.move.name, noop$1);
		};
		function noop$1() {}
	}
};
TouchSensor.activators = [{
	eventName: "onTouchStart",
	handler: (_ref, _ref2) => {
		let { nativeEvent: event } = _ref;
		let { onActivation } = _ref2;
		const { touches } = event;
		if (touches.length > 1) return false;
		onActivation?.({ event });
		return true;
	}
}];
var AutoScrollActivator;
(function(AutoScrollActivator$1) {
	AutoScrollActivator$1[AutoScrollActivator$1["Pointer"] = 0] = "Pointer";
	AutoScrollActivator$1[AutoScrollActivator$1["DraggableRect"] = 1] = "DraggableRect";
})(AutoScrollActivator || (AutoScrollActivator = {}));
var TraversalOrder;
(function(TraversalOrder$1) {
	TraversalOrder$1[TraversalOrder$1["TreeOrder"] = 0] = "TreeOrder";
	TraversalOrder$1[TraversalOrder$1["ReversedTreeOrder"] = 1] = "ReversedTreeOrder";
})(TraversalOrder || (TraversalOrder = {}));
function useAutoScroller(_ref) {
	let { acceleration, activator = AutoScrollActivator.Pointer, canScroll, draggingRect, enabled, interval = 5, order = TraversalOrder.TreeOrder, pointerCoordinates, scrollableAncestors, scrollableAncestorRects, delta, threshold } = _ref;
	const scrollIntent = useScrollIntent({
		delta,
		disabled: !enabled
	});
	const [setAutoScrollInterval, clearAutoScrollInterval] = useInterval();
	const scrollSpeed = (0, import_react.useRef)({
		x: 0,
		y: 0
	});
	const scrollDirection = (0, import_react.useRef)({
		x: 0,
		y: 0
	});
	const rect = (0, import_react.useMemo)(() => {
		switch (activator) {
			case AutoScrollActivator.Pointer: return pointerCoordinates ? {
				top: pointerCoordinates.y,
				bottom: pointerCoordinates.y,
				left: pointerCoordinates.x,
				right: pointerCoordinates.x
			} : null;
			case AutoScrollActivator.DraggableRect: return draggingRect;
		}
	}, [
		activator,
		draggingRect,
		pointerCoordinates
	]);
	const scrollContainerRef = (0, import_react.useRef)(null);
	const autoScroll = (0, import_react.useCallback)(() => {
		const scrollContainer = scrollContainerRef.current;
		if (!scrollContainer) return;
		const scrollLeft = scrollSpeed.current.x * scrollDirection.current.x;
		const scrollTop = scrollSpeed.current.y * scrollDirection.current.y;
		scrollContainer.scrollBy(scrollLeft, scrollTop);
	}, []);
	const sortedScrollableAncestors = (0, import_react.useMemo)(() => order === TraversalOrder.TreeOrder ? [...scrollableAncestors].reverse() : scrollableAncestors, [order, scrollableAncestors]);
	(0, import_react.useEffect)(() => {
		if (!enabled || !scrollableAncestors.length || !rect) {
			clearAutoScrollInterval();
			return;
		}
		for (const scrollContainer of sortedScrollableAncestors) {
			if ((canScroll == null ? void 0 : canScroll(scrollContainer)) === false) continue;
			const scrollContainerRect = scrollableAncestorRects[scrollableAncestors.indexOf(scrollContainer)];
			if (!scrollContainerRect) continue;
			const { direction, speed } = getScrollDirectionAndSpeed(scrollContainer, scrollContainerRect, rect, acceleration, threshold);
			for (const axis of ["x", "y"]) if (!scrollIntent[axis][direction[axis]]) {
				speed[axis] = 0;
				direction[axis] = 0;
			}
			if (speed.x > 0 || speed.y > 0) {
				clearAutoScrollInterval();
				scrollContainerRef.current = scrollContainer;
				setAutoScrollInterval(autoScroll, interval);
				scrollSpeed.current = speed;
				scrollDirection.current = direction;
				return;
			}
		}
		scrollSpeed.current = {
			x: 0,
			y: 0
		};
		scrollDirection.current = {
			x: 0,
			y: 0
		};
		clearAutoScrollInterval();
	}, [
		acceleration,
		autoScroll,
		canScroll,
		clearAutoScrollInterval,
		enabled,
		interval,
		JSON.stringify(rect),
		JSON.stringify(scrollIntent),
		setAutoScrollInterval,
		scrollableAncestors,
		sortedScrollableAncestors,
		scrollableAncestorRects,
		JSON.stringify(threshold)
	]);
}
var defaultScrollIntent = {
	x: {
		[Direction.Backward]: false,
		[Direction.Forward]: false
	},
	y: {
		[Direction.Backward]: false,
		[Direction.Forward]: false
	}
};
function useScrollIntent(_ref2) {
	let { delta, disabled } = _ref2;
	const previousDelta = usePrevious(delta);
	return useLazyMemo((previousIntent) => {
		if (disabled || !previousDelta || !previousIntent) return defaultScrollIntent;
		const direction = {
			x: Math.sign(delta.x - previousDelta.x),
			y: Math.sign(delta.y - previousDelta.y)
		};
		return {
			x: {
				[Direction.Backward]: previousIntent.x[Direction.Backward] || direction.x === -1,
				[Direction.Forward]: previousIntent.x[Direction.Forward] || direction.x === 1
			},
			y: {
				[Direction.Backward]: previousIntent.y[Direction.Backward] || direction.y === -1,
				[Direction.Forward]: previousIntent.y[Direction.Forward] || direction.y === 1
			}
		};
	}, [
		disabled,
		delta,
		previousDelta
	]);
}
function useCachedNode(draggableNodes, id) {
	const draggableNode = id != null ? draggableNodes.get(id) : void 0;
	const node = draggableNode ? draggableNode.node.current : null;
	return useLazyMemo((cachedNode) => {
		var _ref;
		if (id == null) return null;
		return (_ref = node != null ? node : cachedNode) != null ? _ref : null;
	}, [node, id]);
}
function useCombineActivators(sensors, getSyntheticHandler) {
	return (0, import_react.useMemo)(() => sensors.reduce((accumulator, sensor) => {
		const { sensor: Sensor } = sensor;
		const sensorActivators = Sensor.activators.map((activator) => ({
			eventName: activator.eventName,
			handler: getSyntheticHandler(activator.handler, sensor)
		}));
		return [...accumulator, ...sensorActivators];
	}, []), [sensors, getSyntheticHandler]);
}
var MeasuringStrategy;
(function(MeasuringStrategy$1) {
	MeasuringStrategy$1[MeasuringStrategy$1["Always"] = 0] = "Always";
	MeasuringStrategy$1[MeasuringStrategy$1["BeforeDragging"] = 1] = "BeforeDragging";
	MeasuringStrategy$1[MeasuringStrategy$1["WhileDragging"] = 2] = "WhileDragging";
})(MeasuringStrategy || (MeasuringStrategy = {}));
var MeasuringFrequency;
(function(MeasuringFrequency$1) {
	MeasuringFrequency$1["Optimized"] = "optimized";
})(MeasuringFrequency || (MeasuringFrequency = {}));
var defaultValue = /* @__PURE__ */ new Map();
function useDroppableMeasuring(containers, _ref) {
	let { dragging, dependencies, config } = _ref;
	const [queue, setQueue] = (0, import_react.useState)(null);
	const { frequency, measure, strategy } = config;
	const containersRef = (0, import_react.useRef)(containers);
	const disabled = isDisabled();
	const disabledRef = useLatestValue(disabled);
	const measureDroppableContainers = (0, import_react.useCallback)(function(ids$1) {
		if (ids$1 === void 0) ids$1 = [];
		if (disabledRef.current) return;
		setQueue((value) => {
			if (value === null) return ids$1;
			return value.concat(ids$1.filter((id) => !value.includes(id)));
		});
	}, [disabledRef]);
	const timeoutId = (0, import_react.useRef)(null);
	const droppableRects = useLazyMemo((previousValue) => {
		if (disabled && !dragging) return defaultValue;
		if (!previousValue || previousValue === defaultValue || containersRef.current !== containers || queue != null) {
			const map = /* @__PURE__ */ new Map();
			for (let container of containers) {
				if (!container) continue;
				if (queue && queue.length > 0 && !queue.includes(container.id) && container.rect.current) {
					map.set(container.id, container.rect.current);
					continue;
				}
				const node = container.node.current;
				const rect = node ? new Rect(measure(node), node) : null;
				container.rect.current = rect;
				if (rect) map.set(container.id, rect);
			}
			return map;
		}
		return previousValue;
	}, [
		containers,
		queue,
		dragging,
		disabled,
		measure
	]);
	(0, import_react.useEffect)(() => {
		containersRef.current = containers;
	}, [containers]);
	(0, import_react.useEffect)(() => {
		if (disabled) return;
		measureDroppableContainers();
	}, [dragging, disabled]);
	(0, import_react.useEffect)(() => {
		if (queue && queue.length > 0) setQueue(null);
	}, [JSON.stringify(queue)]);
	(0, import_react.useEffect)(() => {
		if (disabled || typeof frequency !== "number" || timeoutId.current !== null) return;
		timeoutId.current = setTimeout(() => {
			measureDroppableContainers();
			timeoutId.current = null;
		}, frequency);
	}, [
		frequency,
		disabled,
		measureDroppableContainers,
		...dependencies
	]);
	return {
		droppableRects,
		measureDroppableContainers,
		measuringScheduled: queue != null
	};
	function isDisabled() {
		switch (strategy) {
			case MeasuringStrategy.Always: return false;
			case MeasuringStrategy.BeforeDragging: return dragging;
			default: return !dragging;
		}
	}
}
function useInitialValue(value, computeFn) {
	return useLazyMemo((previousValue) => {
		if (!value) return null;
		if (previousValue) return previousValue;
		return typeof computeFn === "function" ? computeFn(value) : value;
	}, [computeFn, value]);
}
function useInitialRect(node, measure) {
	return useInitialValue(node, measure);
}
function useMutationObserver(_ref) {
	let { callback, disabled } = _ref;
	const handleMutations = useEvent(callback);
	const mutationObserver = (0, import_react.useMemo)(() => {
		if (disabled || typeof window === "undefined" || typeof window.MutationObserver === "undefined") return;
		const { MutationObserver } = window;
		return new MutationObserver(handleMutations);
	}, [handleMutations, disabled]);
	(0, import_react.useEffect)(() => {
		return () => mutationObserver == null ? void 0 : mutationObserver.disconnect();
	}, [mutationObserver]);
	return mutationObserver;
}
function useResizeObserver(_ref) {
	let { callback, disabled } = _ref;
	const handleResize = useEvent(callback);
	const resizeObserver = (0, import_react.useMemo)(() => {
		if (disabled || typeof window === "undefined" || typeof window.ResizeObserver === "undefined") return;
		const { ResizeObserver } = window;
		return new ResizeObserver(handleResize);
	}, [disabled]);
	(0, import_react.useEffect)(() => {
		return () => resizeObserver == null ? void 0 : resizeObserver.disconnect();
	}, [resizeObserver]);
	return resizeObserver;
}
function defaultMeasure(element) {
	return new Rect(getClientRect(element), element);
}
function useRect(element, measure, fallbackRect) {
	if (measure === void 0) measure = defaultMeasure;
	const [rect, setRect] = (0, import_react.useState)(null);
	function measureRect() {
		setRect((currentRect) => {
			if (!element) return null;
			if (element.isConnected === false) {
				var _ref;
				return (_ref = currentRect != null ? currentRect : fallbackRect) != null ? _ref : null;
			}
			const newRect = measure(element);
			if (JSON.stringify(currentRect) === JSON.stringify(newRect)) return currentRect;
			return newRect;
		});
	}
	const mutationObserver = useMutationObserver({ callback(records) {
		if (!element) return;
		for (const record of records) {
			const { type, target } = record;
			if (type === "childList" && target instanceof HTMLElement && target.contains(element)) {
				measureRect();
				break;
			}
		}
	} });
	const resizeObserver = useResizeObserver({ callback: measureRect });
	useIsomorphicLayoutEffect(() => {
		measureRect();
		if (element) {
			resizeObserver?.observe(element);
			mutationObserver?.observe(document.body, {
				childList: true,
				subtree: true
			});
		} else {
			resizeObserver?.disconnect();
			mutationObserver?.disconnect();
		}
	}, [element]);
	return rect;
}
function useRectDelta(rect) {
	return getRectDelta(rect, useInitialValue(rect));
}
var defaultValue$1 = [];
function useScrollableAncestors(node) {
	const previousNode = (0, import_react.useRef)(node);
	const ancestors = useLazyMemo((previousValue) => {
		if (!node) return defaultValue$1;
		if (previousValue && previousValue !== defaultValue$1 && node && previousNode.current && node.parentNode === previousNode.current.parentNode) return previousValue;
		return getScrollableAncestors(node);
	}, [node]);
	(0, import_react.useEffect)(() => {
		previousNode.current = node;
	}, [node]);
	return ancestors;
}
function useScrollOffsets(elements) {
	const [scrollCoordinates, setScrollCoordinates] = (0, import_react.useState)(null);
	const prevElements = (0, import_react.useRef)(elements);
	const handleScroll = (0, import_react.useCallback)((event) => {
		const scrollingElement = getScrollableElement(event.target);
		if (!scrollingElement) return;
		setScrollCoordinates((scrollCoordinates$1) => {
			if (!scrollCoordinates$1) return null;
			scrollCoordinates$1.set(scrollingElement, getScrollCoordinates(scrollingElement));
			return new Map(scrollCoordinates$1);
		});
	}, []);
	(0, import_react.useEffect)(() => {
		const previousElements = prevElements.current;
		if (elements !== previousElements) {
			cleanup(previousElements);
			const entries = elements.map((element) => {
				const scrollableElement = getScrollableElement(element);
				if (scrollableElement) {
					scrollableElement.addEventListener("scroll", handleScroll, { passive: true });
					return [scrollableElement, getScrollCoordinates(scrollableElement)];
				}
				return null;
			}).filter((entry) => entry != null);
			setScrollCoordinates(entries.length ? new Map(entries) : null);
			prevElements.current = elements;
		}
		return () => {
			cleanup(elements);
			cleanup(previousElements);
		};
		function cleanup(elements$1) {
			elements$1.forEach((element) => {
				getScrollableElement(element)?.removeEventListener("scroll", handleScroll);
			});
		}
	}, [handleScroll, elements]);
	return (0, import_react.useMemo)(() => {
		if (elements.length) return scrollCoordinates ? Array.from(scrollCoordinates.values()).reduce((acc, coordinates) => add(acc, coordinates), defaultCoordinates) : getScrollOffsets(elements);
		return defaultCoordinates;
	}, [elements, scrollCoordinates]);
}
function useScrollOffsetsDelta(scrollOffsets, dependencies) {
	if (dependencies === void 0) dependencies = [];
	const initialScrollOffsets = (0, import_react.useRef)(null);
	(0, import_react.useEffect)(() => {
		initialScrollOffsets.current = null;
	}, dependencies);
	(0, import_react.useEffect)(() => {
		const hasScrollOffsets = scrollOffsets !== defaultCoordinates;
		if (hasScrollOffsets && !initialScrollOffsets.current) initialScrollOffsets.current = scrollOffsets;
		if (!hasScrollOffsets && initialScrollOffsets.current) initialScrollOffsets.current = null;
	}, [scrollOffsets]);
	return initialScrollOffsets.current ? subtract(scrollOffsets, initialScrollOffsets.current) : defaultCoordinates;
}
function useSensorSetup(sensors) {
	(0, import_react.useEffect)(() => {
		if (!canUseDOM) return;
		const teardownFns = sensors.map((_ref) => {
			let { sensor } = _ref;
			return sensor.setup == null ? void 0 : sensor.setup();
		});
		return () => {
			for (const teardown of teardownFns) teardown?.();
		};
	}, sensors.map((_ref2) => {
		let { sensor } = _ref2;
		return sensor;
	}));
}
function useSyntheticListeners(listeners, id) {
	return (0, import_react.useMemo)(() => {
		return listeners.reduce((acc, _ref) => {
			let { eventName, handler } = _ref;
			acc[eventName] = (event) => {
				handler(event, id);
			};
			return acc;
		}, {});
	}, [listeners, id]);
}
function useWindowRect(element) {
	return (0, import_react.useMemo)(() => element ? getWindowClientRect(element) : null, [element]);
}
var defaultValue$2 = [];
function useRects(elements, measure) {
	if (measure === void 0) measure = getClientRect;
	const [firstElement] = elements;
	const windowRect = useWindowRect(firstElement ? getWindow(firstElement) : null);
	const [rects, setRects] = (0, import_react.useState)(defaultValue$2);
	function measureRects() {
		setRects(() => {
			if (!elements.length) return defaultValue$2;
			return elements.map((element) => isDocumentScrollingElement(element) ? windowRect : new Rect(measure(element), element));
		});
	}
	const resizeObserver = useResizeObserver({ callback: measureRects });
	useIsomorphicLayoutEffect(() => {
		resizeObserver?.disconnect();
		measureRects();
		elements.forEach((element) => resizeObserver == null ? void 0 : resizeObserver.observe(element));
	}, [elements]);
	return rects;
}
function getMeasurableNode(node) {
	if (!node) return null;
	if (node.children.length > 1) return node;
	const firstChild = node.children[0];
	return isHTMLElement(firstChild) ? firstChild : node;
}
function useDragOverlayMeasuring(_ref) {
	let { measure } = _ref;
	const [rect, setRect] = (0, import_react.useState)(null);
	const resizeObserver = useResizeObserver({ callback: (0, import_react.useCallback)((entries) => {
		for (const { target } of entries) if (isHTMLElement(target)) {
			setRect((rect$1) => {
				const newRect = measure(target);
				return rect$1 ? {
					...rect$1,
					width: newRect.width,
					height: newRect.height
				} : newRect;
			});
			break;
		}
	}, [measure]) });
	const [nodeRef, setRef] = useNodeRef((0, import_react.useCallback)((element) => {
		const node = getMeasurableNode(element);
		resizeObserver?.disconnect();
		if (node) resizeObserver?.observe(node);
		setRect(node ? measure(node) : null);
	}, [measure, resizeObserver]));
	return (0, import_react.useMemo)(() => ({
		nodeRef,
		rect,
		setRef
	}), [
		rect,
		nodeRef,
		setRef
	]);
}
var defaultSensors = [{
	sensor: PointerSensor,
	options: {}
}, {
	sensor: KeyboardSensor,
	options: {}
}];
var defaultData = { current: {} };
var defaultMeasuringConfiguration = {
	draggable: { measure: getTransformAgnosticClientRect },
	droppable: {
		measure: getTransformAgnosticClientRect,
		strategy: MeasuringStrategy.WhileDragging,
		frequency: MeasuringFrequency.Optimized
	},
	dragOverlay: { measure: getClientRect }
};
var DroppableContainersMap = class extends Map {
	get(id) {
		var _super$get;
		return id != null ? (_super$get = super.get(id)) != null ? _super$get : void 0 : void 0;
	}
	toArray() {
		return Array.from(this.values());
	}
	getEnabled() {
		return this.toArray().filter((_ref) => {
			let { disabled } = _ref;
			return !disabled;
		});
	}
	getNodeFor(id) {
		var _this$get$node$curren, _this$get;
		return (_this$get$node$curren = (_this$get = this.get(id)) == null ? void 0 : _this$get.node.current) != null ? _this$get$node$curren : void 0;
	}
};
var defaultPublicContext = {
	activatorEvent: null,
	active: null,
	activeNode: null,
	activeNodeRect: null,
	collisions: null,
	containerNodeRect: null,
	draggableNodes: /* @__PURE__ */ new Map(),
	droppableRects: /* @__PURE__ */ new Map(),
	droppableContainers: /* @__PURE__ */ new DroppableContainersMap(),
	over: null,
	dragOverlay: {
		nodeRef: { current: null },
		rect: null,
		setRef: noop
	},
	scrollableAncestors: [],
	scrollableAncestorRects: [],
	measuringConfiguration: defaultMeasuringConfiguration,
	measureDroppableContainers: noop,
	windowRect: null,
	measuringScheduled: false
};
var defaultInternalContext = {
	activatorEvent: null,
	activators: [],
	active: null,
	activeNodeRect: null,
	ariaDescribedById: { draggable: "" },
	dispatch: noop,
	draggableNodes: /* @__PURE__ */ new Map(),
	over: null,
	measureDroppableContainers: noop
};
var InternalContext = /* @__PURE__ */ (0, import_react.createContext)(defaultInternalContext);
var PublicContext = /* @__PURE__ */ (0, import_react.createContext)(defaultPublicContext);
function getInitialState() {
	return {
		draggable: {
			active: null,
			initialCoordinates: {
				x: 0,
				y: 0
			},
			nodes: /* @__PURE__ */ new Map(),
			translate: {
				x: 0,
				y: 0
			}
		},
		droppable: { containers: new DroppableContainersMap() }
	};
}
function reducer(state, action) {
	switch (action.type) {
		case Action.DragStart: return {
			...state,
			draggable: {
				...state.draggable,
				initialCoordinates: action.initialCoordinates,
				active: action.active
			}
		};
		case Action.DragMove:
			if (state.draggable.active == null) return state;
			return {
				...state,
				draggable: {
					...state.draggable,
					translate: {
						x: action.coordinates.x - state.draggable.initialCoordinates.x,
						y: action.coordinates.y - state.draggable.initialCoordinates.y
					}
				}
			};
		case Action.DragEnd:
		case Action.DragCancel: return {
			...state,
			draggable: {
				...state.draggable,
				active: null,
				initialCoordinates: {
					x: 0,
					y: 0
				},
				translate: {
					x: 0,
					y: 0
				}
			}
		};
		case Action.RegisterDroppable: {
			const { element } = action;
			const { id } = element;
			const containers = new DroppableContainersMap(state.droppable.containers);
			containers.set(id, element);
			return {
				...state,
				droppable: {
					...state.droppable,
					containers
				}
			};
		}
		case Action.SetDroppableDisabled: {
			const { id, key: key$1, disabled } = action;
			const element = state.droppable.containers.get(id);
			if (!element || key$1 !== element.key) return state;
			const containers = new DroppableContainersMap(state.droppable.containers);
			containers.set(id, {
				...element,
				disabled
			});
			return {
				...state,
				droppable: {
					...state.droppable,
					containers
				}
			};
		}
		case Action.UnregisterDroppable: {
			const { id, key: key$1 } = action;
			const element = state.droppable.containers.get(id);
			if (!element || key$1 !== element.key) return state;
			const containers = new DroppableContainersMap(state.droppable.containers);
			containers.delete(id);
			return {
				...state,
				droppable: {
					...state.droppable,
					containers
				}
			};
		}
		default: return state;
	}
}
function RestoreFocus(_ref) {
	let { disabled } = _ref;
	const { active, activatorEvent, draggableNodes } = (0, import_react.useContext)(InternalContext);
	const previousActivatorEvent = usePrevious(activatorEvent);
	const previousActiveId = usePrevious(active == null ? void 0 : active.id);
	(0, import_react.useEffect)(() => {
		if (disabled) return;
		if (!activatorEvent && previousActivatorEvent && previousActiveId != null) {
			if (!isKeyboardEvent(previousActivatorEvent)) return;
			if (document.activeElement === previousActivatorEvent.target) return;
			const draggableNode = draggableNodes.get(previousActiveId);
			if (!draggableNode) return;
			const { activatorNode, node } = draggableNode;
			if (!activatorNode.current && !node.current) return;
			requestAnimationFrame(() => {
				for (const element of [activatorNode.current, node.current]) {
					if (!element) continue;
					const focusableNode = findFirstFocusableNode(element);
					if (focusableNode) {
						focusableNode.focus();
						break;
					}
				}
			});
		}
	}, [
		activatorEvent,
		disabled,
		draggableNodes,
		previousActiveId,
		previousActivatorEvent
	]);
	return null;
}
function applyModifiers(modifiers, _ref) {
	let { transform, ...args } = _ref;
	return modifiers != null && modifiers.length ? modifiers.reduce((accumulator, modifier) => {
		return modifier({
			transform: accumulator,
			...args
		});
	}, transform) : transform;
}
function useMeasuringConfiguration(config) {
	return (0, import_react.useMemo)(() => ({
		draggable: {
			...defaultMeasuringConfiguration.draggable,
			...config == null ? void 0 : config.draggable
		},
		droppable: {
			...defaultMeasuringConfiguration.droppable,
			...config == null ? void 0 : config.droppable
		},
		dragOverlay: {
			...defaultMeasuringConfiguration.dragOverlay,
			...config == null ? void 0 : config.dragOverlay
		}
	}), [
		config == null ? void 0 : config.draggable,
		config == null ? void 0 : config.droppable,
		config == null ? void 0 : config.dragOverlay
	]);
}
function useLayoutShiftScrollCompensation(_ref) {
	let { activeNode, measure, initialRect, config = true } = _ref;
	const initialized = (0, import_react.useRef)(false);
	const { x, y } = typeof config === "boolean" ? {
		x: config,
		y: config
	} : config;
	useIsomorphicLayoutEffect(() => {
		if (!x && !y || !activeNode) {
			initialized.current = false;
			return;
		}
		if (initialized.current || !initialRect) return;
		const node = activeNode == null ? void 0 : activeNode.node.current;
		if (!node || node.isConnected === false) return;
		const rectDelta = getRectDelta(measure(node), initialRect);
		if (!x) rectDelta.x = 0;
		if (!y) rectDelta.y = 0;
		initialized.current = true;
		if (Math.abs(rectDelta.x) > 0 || Math.abs(rectDelta.y) > 0) {
			const firstScrollableAncestor = getFirstScrollableAncestor(node);
			if (firstScrollableAncestor) firstScrollableAncestor.scrollBy({
				top: rectDelta.y,
				left: rectDelta.x
			});
		}
	}, [
		activeNode,
		x,
		y,
		initialRect,
		measure
	]);
}
var ActiveDraggableContext = /* @__PURE__ */ (0, import_react.createContext)({
	...defaultCoordinates,
	scaleX: 1,
	scaleY: 1
});
var Status;
(function(Status$1) {
	Status$1[Status$1["Uninitialized"] = 0] = "Uninitialized";
	Status$1[Status$1["Initializing"] = 1] = "Initializing";
	Status$1[Status$1["Initialized"] = 2] = "Initialized";
})(Status || (Status = {}));
var DndContext = /* @__PURE__ */ (0, import_react.memo)(function DndContext$1(_ref) {
	var _sensorContext$curren, _dragOverlay$nodeRef$, _dragOverlay$rect, _over$rect;
	let { id, accessibility, autoScroll = true, children, sensors = defaultSensors, collisionDetection: collisionDetection$1 = rectIntersection, measuring, modifiers, ...props } = _ref;
	const [state, dispatch] = (0, import_react.useReducer)(reducer, void 0, getInitialState);
	const [dispatchMonitorEvent, registerMonitorListener] = useDndMonitorProvider();
	const [status, setStatus] = (0, import_react.useState)(Status.Uninitialized);
	const isInitialized = status === Status.Initialized;
	const { draggable: { active: activeId, nodes: draggableNodes, translate: translate$1 }, droppable: { containers: droppableContainers } } = state;
	const node = activeId != null ? draggableNodes.get(activeId) : null;
	const activeRects = (0, import_react.useRef)({
		initial: null,
		translated: null
	});
	const active = (0, import_react.useMemo)(() => {
		var _node$data;
		return activeId != null ? {
			id: activeId,
			data: (_node$data = node == null ? void 0 : node.data) != null ? _node$data : defaultData,
			rect: activeRects
		} : null;
	}, [activeId, node]);
	const activeRef = (0, import_react.useRef)(null);
	const [activeSensor, setActiveSensor] = (0, import_react.useState)(null);
	const [activatorEvent, setActivatorEvent] = (0, import_react.useState)(null);
	const latestProps = useLatestValue(props, Object.values(props));
	const draggableDescribedById = useUniqueId("DndDescribedBy", id);
	const enabledDroppableContainers = (0, import_react.useMemo)(() => droppableContainers.getEnabled(), [droppableContainers]);
	const measuringConfiguration = useMeasuringConfiguration(measuring);
	const { droppableRects, measureDroppableContainers, measuringScheduled } = useDroppableMeasuring(enabledDroppableContainers, {
		dragging: isInitialized,
		dependencies: [translate$1.x, translate$1.y],
		config: measuringConfiguration.droppable
	});
	const activeNode = useCachedNode(draggableNodes, activeId);
	const activationCoordinates = (0, import_react.useMemo)(() => activatorEvent ? getEventCoordinates(activatorEvent) : null, [activatorEvent]);
	const autoScrollOptions = getAutoScrollerOptions();
	const initialActiveNodeRect = useInitialRect(activeNode, measuringConfiguration.draggable.measure);
	useLayoutShiftScrollCompensation({
		activeNode: activeId != null ? draggableNodes.get(activeId) : null,
		config: autoScrollOptions.layoutShiftCompensation,
		initialRect: initialActiveNodeRect,
		measure: measuringConfiguration.draggable.measure
	});
	const activeNodeRect = useRect(activeNode, measuringConfiguration.draggable.measure, initialActiveNodeRect);
	const containerNodeRect = useRect(activeNode ? activeNode.parentElement : null);
	const sensorContext = (0, import_react.useRef)({
		activatorEvent: null,
		active: null,
		activeNode,
		collisionRect: null,
		collisions: null,
		droppableRects,
		draggableNodes,
		draggingNode: null,
		draggingNodeRect: null,
		droppableContainers,
		over: null,
		scrollableAncestors: [],
		scrollAdjustedTranslate: null
	});
	const overNode = droppableContainers.getNodeFor((_sensorContext$curren = sensorContext.current.over) == null ? void 0 : _sensorContext$curren.id);
	const dragOverlay = useDragOverlayMeasuring({ measure: measuringConfiguration.dragOverlay.measure });
	const draggingNode = (_dragOverlay$nodeRef$ = dragOverlay.nodeRef.current) != null ? _dragOverlay$nodeRef$ : activeNode;
	const draggingNodeRect = isInitialized ? (_dragOverlay$rect = dragOverlay.rect) != null ? _dragOverlay$rect : activeNodeRect : null;
	const usesDragOverlay = Boolean(dragOverlay.nodeRef.current && dragOverlay.rect);
	const nodeRectDelta = useRectDelta(usesDragOverlay ? null : activeNodeRect);
	const windowRect = useWindowRect(draggingNode ? getWindow(draggingNode) : null);
	const scrollableAncestors = useScrollableAncestors(isInitialized ? overNode != null ? overNode : activeNode : null);
	const scrollableAncestorRects = useRects(scrollableAncestors);
	const modifiedTranslate = applyModifiers(modifiers, {
		transform: {
			x: translate$1.x - nodeRectDelta.x,
			y: translate$1.y - nodeRectDelta.y,
			scaleX: 1,
			scaleY: 1
		},
		activatorEvent,
		active,
		activeNodeRect,
		containerNodeRect,
		draggingNodeRect,
		over: sensorContext.current.over,
		overlayNodeRect: dragOverlay.rect,
		scrollableAncestors,
		scrollableAncestorRects,
		windowRect
	});
	const pointerCoordinates = activationCoordinates ? add(activationCoordinates, translate$1) : null;
	const scrollOffsets = useScrollOffsets(scrollableAncestors);
	const scrollAdjustment = useScrollOffsetsDelta(scrollOffsets);
	const activeNodeScrollDelta = useScrollOffsetsDelta(scrollOffsets, [activeNodeRect]);
	const scrollAdjustedTranslate = add(modifiedTranslate, scrollAdjustment);
	const collisionRect = draggingNodeRect ? getAdjustedRect(draggingNodeRect, modifiedTranslate) : null;
	const collisions = active && collisionRect ? collisionDetection$1({
		active,
		collisionRect,
		droppableRects,
		droppableContainers: enabledDroppableContainers,
		pointerCoordinates
	}) : null;
	const overId = getFirstCollision(collisions, "id");
	const [over, setOver] = (0, import_react.useState)(null);
	const transform = adjustScale(usesDragOverlay ? modifiedTranslate : add(modifiedTranslate, activeNodeScrollDelta), (_over$rect = over == null ? void 0 : over.rect) != null ? _over$rect : null, activeNodeRect);
	const activeSensorRef = (0, import_react.useRef)(null);
	const instantiateSensor = (0, import_react.useCallback)((event, _ref2) => {
		let { sensor: Sensor, options } = _ref2;
		if (activeRef.current == null) return;
		const activeNode$1 = draggableNodes.get(activeRef.current);
		if (!activeNode$1) return;
		const activatorEvent$1 = event.nativeEvent;
		activeSensorRef.current = new Sensor({
			active: activeRef.current,
			activeNode: activeNode$1,
			event: activatorEvent$1,
			options,
			context: sensorContext,
			onAbort(id$1) {
				if (!draggableNodes.get(id$1)) return;
				const { onDragAbort } = latestProps.current;
				const event$1 = { id: id$1 };
				onDragAbort?.(event$1);
				dispatchMonitorEvent({
					type: "onDragAbort",
					event: event$1
				});
			},
			onPending(id$1, constraint, initialCoordinates, offset) {
				if (!draggableNodes.get(id$1)) return;
				const { onDragPending } = latestProps.current;
				const event$1 = {
					id: id$1,
					constraint,
					initialCoordinates,
					offset
				};
				onDragPending?.(event$1);
				dispatchMonitorEvent({
					type: "onDragPending",
					event: event$1
				});
			},
			onStart(initialCoordinates) {
				const id$1 = activeRef.current;
				if (id$1 == null) return;
				const draggableNode = draggableNodes.get(id$1);
				if (!draggableNode) return;
				const { onDragStart } = latestProps.current;
				const event$1 = {
					activatorEvent: activatorEvent$1,
					active: {
						id: id$1,
						data: draggableNode.data,
						rect: activeRects
					}
				};
				(0, import_react_dom.unstable_batchedUpdates)(() => {
					onDragStart?.(event$1);
					setStatus(Status.Initializing);
					dispatch({
						type: Action.DragStart,
						initialCoordinates,
						active: id$1
					});
					dispatchMonitorEvent({
						type: "onDragStart",
						event: event$1
					});
					setActiveSensor(activeSensorRef.current);
					setActivatorEvent(activatorEvent$1);
				});
			},
			onMove(coordinates) {
				dispatch({
					type: Action.DragMove,
					coordinates
				});
			},
			onEnd: createHandler(Action.DragEnd),
			onCancel: createHandler(Action.DragCancel)
		});
		function createHandler(type) {
			return async function handler() {
				const { active: active$1, collisions: collisions$1, over: over$1, scrollAdjustedTranslate: scrollAdjustedTranslate$1 } = sensorContext.current;
				let event$1 = null;
				if (active$1 && scrollAdjustedTranslate$1) {
					const { cancelDrop } = latestProps.current;
					event$1 = {
						activatorEvent: activatorEvent$1,
						active: active$1,
						collisions: collisions$1,
						delta: scrollAdjustedTranslate$1,
						over: over$1
					};
					if (type === Action.DragEnd && typeof cancelDrop === "function") {
						if (await Promise.resolve(cancelDrop(event$1))) type = Action.DragCancel;
					}
				}
				activeRef.current = null;
				(0, import_react_dom.unstable_batchedUpdates)(() => {
					dispatch({ type });
					setStatus(Status.Uninitialized);
					setOver(null);
					setActiveSensor(null);
					setActivatorEvent(null);
					activeSensorRef.current = null;
					const eventName = type === Action.DragEnd ? "onDragEnd" : "onDragCancel";
					if (event$1) {
						const handler$1 = latestProps.current[eventName];
						handler$1?.(event$1);
						dispatchMonitorEvent({
							type: eventName,
							event: event$1
						});
					}
				});
			};
		}
	}, [draggableNodes]);
	const activators = useCombineActivators(sensors, (0, import_react.useCallback)((handler, sensor) => {
		return (event, active$1) => {
			const nativeEvent = event.nativeEvent;
			const activeDraggableNode = draggableNodes.get(active$1);
			if (activeRef.current !== null || !activeDraggableNode || nativeEvent.dndKit || nativeEvent.defaultPrevented) return;
			const activationContext = { active: activeDraggableNode };
			if (handler(event, sensor.options, activationContext) === true) {
				nativeEvent.dndKit = { capturedBy: sensor.sensor };
				activeRef.current = active$1;
				instantiateSensor(event, sensor);
			}
		};
	}, [draggableNodes, instantiateSensor]));
	useSensorSetup(sensors);
	useIsomorphicLayoutEffect(() => {
		if (activeNodeRect && status === Status.Initializing) setStatus(Status.Initialized);
	}, [activeNodeRect, status]);
	(0, import_react.useEffect)(() => {
		const { onDragMove } = latestProps.current;
		const { active: active$1, activatorEvent: activatorEvent$1, collisions: collisions$1, over: over$1 } = sensorContext.current;
		if (!active$1 || !activatorEvent$1) return;
		const event = {
			active: active$1,
			activatorEvent: activatorEvent$1,
			collisions: collisions$1,
			delta: {
				x: scrollAdjustedTranslate.x,
				y: scrollAdjustedTranslate.y
			},
			over: over$1
		};
		(0, import_react_dom.unstable_batchedUpdates)(() => {
			onDragMove?.(event);
			dispatchMonitorEvent({
				type: "onDragMove",
				event
			});
		});
	}, [scrollAdjustedTranslate.x, scrollAdjustedTranslate.y]);
	(0, import_react.useEffect)(() => {
		const { active: active$1, activatorEvent: activatorEvent$1, collisions: collisions$1, droppableContainers: droppableContainers$1, scrollAdjustedTranslate: scrollAdjustedTranslate$1 } = sensorContext.current;
		if (!active$1 || activeRef.current == null || !activatorEvent$1 || !scrollAdjustedTranslate$1) return;
		const { onDragOver } = latestProps.current;
		const overContainer = droppableContainers$1.get(overId);
		const over$1 = overContainer && overContainer.rect.current ? {
			id: overContainer.id,
			rect: overContainer.rect.current,
			data: overContainer.data,
			disabled: overContainer.disabled
		} : null;
		const event = {
			active: active$1,
			activatorEvent: activatorEvent$1,
			collisions: collisions$1,
			delta: {
				x: scrollAdjustedTranslate$1.x,
				y: scrollAdjustedTranslate$1.y
			},
			over: over$1
		};
		(0, import_react_dom.unstable_batchedUpdates)(() => {
			setOver(over$1);
			onDragOver?.(event);
			dispatchMonitorEvent({
				type: "onDragOver",
				event
			});
		});
	}, [overId]);
	useIsomorphicLayoutEffect(() => {
		sensorContext.current = {
			activatorEvent,
			active,
			activeNode,
			collisionRect,
			collisions,
			droppableRects,
			draggableNodes,
			draggingNode,
			draggingNodeRect,
			droppableContainers,
			over,
			scrollableAncestors,
			scrollAdjustedTranslate
		};
		activeRects.current = {
			initial: draggingNodeRect,
			translated: collisionRect
		};
	}, [
		active,
		activeNode,
		collisions,
		collisionRect,
		draggableNodes,
		draggingNode,
		draggingNodeRect,
		droppableRects,
		droppableContainers,
		over,
		scrollableAncestors,
		scrollAdjustedTranslate
	]);
	useAutoScroller({
		...autoScrollOptions,
		delta: translate$1,
		draggingRect: collisionRect,
		pointerCoordinates,
		scrollableAncestors,
		scrollableAncestorRects
	});
	const publicContext = (0, import_react.useMemo)(() => {
		return {
			active,
			activeNode,
			activeNodeRect,
			activatorEvent,
			collisions,
			containerNodeRect,
			dragOverlay,
			draggableNodes,
			droppableContainers,
			droppableRects,
			over,
			measureDroppableContainers,
			scrollableAncestors,
			scrollableAncestorRects,
			measuringConfiguration,
			measuringScheduled,
			windowRect
		};
	}, [
		active,
		activeNode,
		activeNodeRect,
		activatorEvent,
		collisions,
		containerNodeRect,
		dragOverlay,
		draggableNodes,
		droppableContainers,
		droppableRects,
		over,
		measureDroppableContainers,
		scrollableAncestors,
		scrollableAncestorRects,
		measuringConfiguration,
		measuringScheduled,
		windowRect
	]);
	const internalContext = (0, import_react.useMemo)(() => {
		return {
			activatorEvent,
			activators,
			active,
			activeNodeRect,
			ariaDescribedById: { draggable: draggableDescribedById },
			dispatch,
			draggableNodes,
			over,
			measureDroppableContainers
		};
	}, [
		activatorEvent,
		activators,
		active,
		activeNodeRect,
		dispatch,
		draggableDescribedById,
		draggableNodes,
		over,
		measureDroppableContainers
	]);
	return import_react.createElement(DndMonitorContext.Provider, { value: registerMonitorListener }, import_react.createElement(InternalContext.Provider, { value: internalContext }, import_react.createElement(PublicContext.Provider, { value: publicContext }, import_react.createElement(ActiveDraggableContext.Provider, { value: transform }, children)), import_react.createElement(RestoreFocus, { disabled: (accessibility == null ? void 0 : accessibility.restoreFocus) === false })), import_react.createElement(Accessibility, {
		...accessibility,
		hiddenTextDescribedById: draggableDescribedById
	}));
	function getAutoScrollerOptions() {
		const activeSensorDisablesAutoscroll = (activeSensor == null ? void 0 : activeSensor.autoScrollEnabled) === false;
		const autoScrollGloballyDisabled = typeof autoScroll === "object" ? autoScroll.enabled === false : autoScroll === false;
		const enabled = isInitialized && !activeSensorDisablesAutoscroll && !autoScrollGloballyDisabled;
		if (typeof autoScroll === "object") return {
			...autoScroll,
			enabled
		};
		return { enabled };
	}
});
var NullContext = /* @__PURE__ */ (0, import_react.createContext)(null);
var defaultRole = "button";
var ID_PREFIX$1 = "Draggable";
function useDraggable(_ref) {
	let { id, data, disabled = false, attributes } = _ref;
	const key$1 = useUniqueId(ID_PREFIX$1);
	const { activators, activatorEvent, active, activeNodeRect, ariaDescribedById, draggableNodes, over } = (0, import_react.useContext)(InternalContext);
	const { role = defaultRole, roleDescription = "draggable", tabIndex = 0 } = attributes != null ? attributes : {};
	const isDragging = (active == null ? void 0 : active.id) === id;
	const transform = (0, import_react.useContext)(isDragging ? ActiveDraggableContext : NullContext);
	const [node, setNodeRef] = useNodeRef();
	const [activatorNode, setActivatorNodeRef] = useNodeRef();
	const listeners = useSyntheticListeners(activators, id);
	const dataRef = useLatestValue(data);
	useIsomorphicLayoutEffect(() => {
		draggableNodes.set(id, {
			id,
			key: key$1,
			node,
			activatorNode,
			data: dataRef
		});
		return () => {
			const node$1 = draggableNodes.get(id);
			if (node$1 && node$1.key === key$1) draggableNodes.delete(id);
		};
	}, [draggableNodes, id]);
	return {
		active,
		activatorEvent,
		activeNodeRect,
		attributes: (0, import_react.useMemo)(() => ({
			role,
			tabIndex,
			"aria-disabled": disabled,
			"aria-pressed": isDragging && role === defaultRole ? true : void 0,
			"aria-roledescription": roleDescription,
			"aria-describedby": ariaDescribedById.draggable
		}), [
			disabled,
			role,
			tabIndex,
			isDragging,
			roleDescription,
			ariaDescribedById.draggable
		]),
		isDragging,
		listeners: disabled ? void 0 : listeners,
		node,
		over,
		setNodeRef,
		setActivatorNodeRef,
		transform
	};
}
function useDndContext() {
	return (0, import_react.useContext)(PublicContext);
}
var ID_PREFIX$1$1 = "Droppable";
var defaultResizeObserverConfig = { timeout: 25 };
function useDroppable(_ref) {
	let { data, disabled = false, id, resizeObserverConfig } = _ref;
	const key$1 = useUniqueId(ID_PREFIX$1$1);
	const { active, dispatch, over, measureDroppableContainers } = (0, import_react.useContext)(InternalContext);
	const previous = (0, import_react.useRef)({ disabled });
	const resizeObserverConnected = (0, import_react.useRef)(false);
	const rect = (0, import_react.useRef)(null);
	const callbackId = (0, import_react.useRef)(null);
	const { disabled: resizeObserverDisabled, updateMeasurementsFor, timeout: resizeObserverTimeout } = {
		...defaultResizeObserverConfig,
		...resizeObserverConfig
	};
	const ids$1 = useLatestValue(updateMeasurementsFor != null ? updateMeasurementsFor : id);
	const resizeObserver = useResizeObserver({
		callback: (0, import_react.useCallback)(() => {
			if (!resizeObserverConnected.current) {
				resizeObserverConnected.current = true;
				return;
			}
			if (callbackId.current != null) clearTimeout(callbackId.current);
			callbackId.current = setTimeout(() => {
				measureDroppableContainers(Array.isArray(ids$1.current) ? ids$1.current : [ids$1.current]);
				callbackId.current = null;
			}, resizeObserverTimeout);
		}, [resizeObserverTimeout]),
		disabled: resizeObserverDisabled || !active
	});
	const [nodeRef, setNodeRef] = useNodeRef((0, import_react.useCallback)((newElement, previousElement) => {
		if (!resizeObserver) return;
		if (previousElement) {
			resizeObserver.unobserve(previousElement);
			resizeObserverConnected.current = false;
		}
		if (newElement) resizeObserver.observe(newElement);
	}, [resizeObserver]));
	const dataRef = useLatestValue(data);
	(0, import_react.useEffect)(() => {
		if (!resizeObserver || !nodeRef.current) return;
		resizeObserver.disconnect();
		resizeObserverConnected.current = false;
		resizeObserver.observe(nodeRef.current);
	}, [nodeRef, resizeObserver]);
	(0, import_react.useEffect)(() => {
		dispatch({
			type: Action.RegisterDroppable,
			element: {
				id,
				key: key$1,
				disabled,
				node: nodeRef,
				rect,
				data: dataRef
			}
		});
		return () => dispatch({
			type: Action.UnregisterDroppable,
			key: key$1,
			id
		});
	}, [id]);
	(0, import_react.useEffect)(() => {
		if (disabled !== previous.current.disabled) {
			dispatch({
				type: Action.SetDroppableDisabled,
				id,
				key: key$1,
				disabled
			});
			previous.current.disabled = disabled;
		}
	}, [
		id,
		key$1,
		disabled,
		dispatch
	]);
	return {
		active,
		rect,
		isOver: (over == null ? void 0 : over.id) === id,
		node: nodeRef,
		over,
		setNodeRef
	};
}
function AnimationManager(_ref) {
	let { animation, children } = _ref;
	const [clonedChildren, setClonedChildren] = (0, import_react.useState)(null);
	const [element, setElement] = (0, import_react.useState)(null);
	const previousChildren = usePrevious(children);
	if (!children && !clonedChildren && previousChildren) setClonedChildren(previousChildren);
	useIsomorphicLayoutEffect(() => {
		if (!element) return;
		const key$1 = clonedChildren == null ? void 0 : clonedChildren.key;
		const id = clonedChildren == null ? void 0 : clonedChildren.props.id;
		if (key$1 == null || id == null) {
			setClonedChildren(null);
			return;
		}
		Promise.resolve(animation(id, element)).then(() => {
			setClonedChildren(null);
		});
	}, [
		animation,
		clonedChildren,
		element
	]);
	return import_react.createElement(import_react.Fragment, null, children, clonedChildren ? (0, import_react.cloneElement)(clonedChildren, { ref: setElement }) : null);
}
var defaultTransform = {
	x: 0,
	y: 0,
	scaleX: 1,
	scaleY: 1
};
function NullifiedContextProvider(_ref) {
	let { children } = _ref;
	return import_react.createElement(InternalContext.Provider, { value: defaultInternalContext }, import_react.createElement(ActiveDraggableContext.Provider, { value: defaultTransform }, children));
}
var baseStyles = {
	position: "fixed",
	touchAction: "none"
};
var defaultTransition$1 = (activatorEvent) => {
	return isKeyboardEvent(activatorEvent) ? "transform 250ms ease" : void 0;
};
var PositionedOverlay = /* @__PURE__ */ (0, import_react.forwardRef)((_ref, ref) => {
	let { as, activatorEvent, adjustScale: adjustScale$1, children, className, rect, style, transform, transition = defaultTransition$1 } = _ref;
	if (!rect) return null;
	const scaleAdjustedTransform = adjustScale$1 ? transform : {
		...transform,
		scaleX: 1,
		scaleY: 1
	};
	const styles = {
		...baseStyles,
		width: rect.width,
		height: rect.height,
		top: rect.top,
		left: rect.left,
		transform: CSS$1.Transform.toString(scaleAdjustedTransform),
		transformOrigin: adjustScale$1 && activatorEvent ? getRelativeTransformOrigin(activatorEvent, rect) : void 0,
		transition: typeof transition === "function" ? transition(activatorEvent) : transition,
		...style
	};
	return import_react.createElement(as, {
		className,
		style: styles,
		ref
	}, children);
});
var defaultDropAnimationSideEffects = (options) => (_ref) => {
	let { active, dragOverlay } = _ref;
	const originalStyles = {};
	const { styles, className } = options;
	if (styles != null && styles.active) for (const [key$1, value] of Object.entries(styles.active)) {
		if (value === void 0) continue;
		originalStyles[key$1] = active.node.style.getPropertyValue(key$1);
		active.node.style.setProperty(key$1, value);
	}
	if (styles != null && styles.dragOverlay) for (const [key$1, value] of Object.entries(styles.dragOverlay)) {
		if (value === void 0) continue;
		dragOverlay.node.style.setProperty(key$1, value);
	}
	if (className != null && className.active) active.node.classList.add(className.active);
	if (className != null && className.dragOverlay) dragOverlay.node.classList.add(className.dragOverlay);
	return function cleanup() {
		for (const [key$1, value] of Object.entries(originalStyles)) active.node.style.setProperty(key$1, value);
		if (className != null && className.active) active.node.classList.remove(className.active);
	};
};
var defaultKeyframeResolver = (_ref2) => {
	let { transform: { initial, final } } = _ref2;
	return [{ transform: CSS$1.Transform.toString(initial) }, { transform: CSS$1.Transform.toString(final) }];
};
var defaultDropAnimationConfiguration = {
	duration: 250,
	easing: "ease",
	keyframes: defaultKeyframeResolver,
	sideEffects: /* @__PURE__ */ defaultDropAnimationSideEffects({ styles: { active: { opacity: "0" } } })
};
function useDropAnimation(_ref3) {
	let { config, draggableNodes, droppableContainers, measuringConfiguration } = _ref3;
	return useEvent((id, node) => {
		if (config === null) return;
		const activeDraggable = draggableNodes.get(id);
		if (!activeDraggable) return;
		const activeNode = activeDraggable.node.current;
		if (!activeNode) return;
		const measurableNode = getMeasurableNode(node);
		if (!measurableNode) return;
		const { transform } = getWindow(node).getComputedStyle(node);
		const parsedTransform = parseTransform(transform);
		if (!parsedTransform) return;
		const animation = typeof config === "function" ? config : createDefaultDropAnimation(config);
		scrollIntoViewIfNeeded(activeNode, measuringConfiguration.draggable.measure);
		return animation({
			active: {
				id,
				data: activeDraggable.data,
				node: activeNode,
				rect: measuringConfiguration.draggable.measure(activeNode)
			},
			draggableNodes,
			dragOverlay: {
				node,
				rect: measuringConfiguration.dragOverlay.measure(measurableNode)
			},
			droppableContainers,
			measuringConfiguration,
			transform: parsedTransform
		});
	});
}
function createDefaultDropAnimation(options) {
	const { duration, easing, sideEffects, keyframes } = {
		...defaultDropAnimationConfiguration,
		...options
	};
	return (_ref4) => {
		let { active, dragOverlay, transform, ...rest } = _ref4;
		if (!duration) return;
		const delta = {
			x: dragOverlay.rect.left - active.rect.left,
			y: dragOverlay.rect.top - active.rect.top
		};
		const scale = {
			scaleX: transform.scaleX !== 1 ? active.rect.width * transform.scaleX / dragOverlay.rect.width : 1,
			scaleY: transform.scaleY !== 1 ? active.rect.height * transform.scaleY / dragOverlay.rect.height : 1
		};
		const finalTransform = {
			x: transform.x - delta.x,
			y: transform.y - delta.y,
			...scale
		};
		const animationKeyframes = keyframes({
			...rest,
			active,
			dragOverlay,
			transform: {
				initial: transform,
				final: finalTransform
			}
		});
		const [firstKeyframe] = animationKeyframes;
		const lastKeyframe = animationKeyframes[animationKeyframes.length - 1];
		if (JSON.stringify(firstKeyframe) === JSON.stringify(lastKeyframe)) return;
		const cleanup = sideEffects == null ? void 0 : sideEffects({
			active,
			dragOverlay,
			...rest
		});
		const animation = dragOverlay.node.animate(animationKeyframes, {
			duration,
			easing,
			fill: "forwards"
		});
		return new Promise((resolve) => {
			animation.onfinish = () => {
				cleanup?.();
				resolve();
			};
		});
	};
}
var key = 0;
function useKey(id) {
	return (0, import_react.useMemo)(() => {
		if (id == null) return;
		key++;
		return key;
	}, [id]);
}
var DragOverlay = /* @__PURE__ */ import_react.memo((_ref) => {
	let { adjustScale: adjustScale$1 = false, children, dropAnimation: dropAnimationConfig, style, transition, modifiers, wrapperElement = "div", className, zIndex = 999 } = _ref;
	const { activatorEvent, active, activeNodeRect, containerNodeRect, draggableNodes, droppableContainers, dragOverlay, over, measuringConfiguration, scrollableAncestors, scrollableAncestorRects, windowRect } = useDndContext();
	const transform = (0, import_react.useContext)(ActiveDraggableContext);
	const key$1 = useKey(active == null ? void 0 : active.id);
	const modifiedTransform = applyModifiers(modifiers, {
		activatorEvent,
		active,
		activeNodeRect,
		containerNodeRect,
		draggingNodeRect: dragOverlay.rect,
		over,
		overlayNodeRect: dragOverlay.rect,
		scrollableAncestors,
		scrollableAncestorRects,
		transform,
		windowRect
	});
	const initialRect = useInitialValue(activeNodeRect);
	const dropAnimation = useDropAnimation({
		config: dropAnimationConfig,
		draggableNodes,
		droppableContainers,
		measuringConfiguration
	});
	const ref = initialRect ? dragOverlay.setRef : void 0;
	return import_react.createElement(NullifiedContextProvider, null, import_react.createElement(AnimationManager, { animation: dropAnimation }, active && key$1 ? import_react.createElement(PositionedOverlay, {
		key: key$1,
		id: active.id,
		ref,
		as: wrapperElement,
		activatorEvent,
		adjustScale: adjustScale$1,
		className,
		transition,
		rect: initialRect,
		style: {
			zIndex,
			...style
		},
		transform: modifiedTransform
	}, children) : null));
});
function arrayMove(array, from, to) {
	const newArray = array.slice();
	newArray.splice(to < 0 ? newArray.length + to : to, 0, newArray.splice(from, 1)[0]);
	return newArray;
}
function getSortedRects(items, rects) {
	return items.reduce((accumulator, id, index) => {
		const rect = rects.get(id);
		if (rect) accumulator[index] = rect;
		return accumulator;
	}, Array(items.length));
}
function isValidIndex(index) {
	return index !== null && index >= 0;
}
function itemsEqual(a, b) {
	if (a === b) return true;
	if (a.length !== b.length) return false;
	for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
	return true;
}
function normalizeDisabled(disabled) {
	if (typeof disabled === "boolean") return {
		draggable: disabled,
		droppable: disabled
	};
	return disabled;
}
var rectSortingStrategy = (_ref) => {
	let { rects, activeIndex, overIndex, index } = _ref;
	const newRects = arrayMove(rects, overIndex, activeIndex);
	const oldRect = rects[index];
	const newRect = newRects[index];
	if (!newRect || !oldRect) return null;
	return {
		x: newRect.left - oldRect.left,
		y: newRect.top - oldRect.top,
		scaleX: newRect.width / oldRect.width,
		scaleY: newRect.height / oldRect.height
	};
};
var ID_PREFIX = "Sortable";
var Context = /* @__PURE__ */ import_react.createContext({
	activeIndex: -1,
	containerId: ID_PREFIX,
	disableTransforms: false,
	items: [],
	overIndex: -1,
	useDragOverlay: false,
	sortedRects: [],
	strategy: rectSortingStrategy,
	disabled: {
		draggable: false,
		droppable: false
	}
});
function SortableContext(_ref) {
	let { children, id, items: userDefinedItems, strategy = rectSortingStrategy, disabled: disabledProp = false } = _ref;
	const { active, dragOverlay, droppableRects, over, measureDroppableContainers } = useDndContext();
	const containerId = useUniqueId(ID_PREFIX, id);
	const useDragOverlay = Boolean(dragOverlay.rect !== null);
	const items = (0, import_react.useMemo)(() => userDefinedItems.map((item) => typeof item === "object" && "id" in item ? item.id : item), [userDefinedItems]);
	const isDragging = active != null;
	const activeIndex = active ? items.indexOf(active.id) : -1;
	const overIndex = over ? items.indexOf(over.id) : -1;
	const previousItemsRef = (0, import_react.useRef)(items);
	const itemsHaveChanged = !itemsEqual(items, previousItemsRef.current);
	const disableTransforms = overIndex !== -1 && activeIndex === -1 || itemsHaveChanged;
	const disabled = normalizeDisabled(disabledProp);
	useIsomorphicLayoutEffect(() => {
		if (itemsHaveChanged && isDragging) measureDroppableContainers(items);
	}, [
		itemsHaveChanged,
		items,
		isDragging,
		measureDroppableContainers
	]);
	(0, import_react.useEffect)(() => {
		previousItemsRef.current = items;
	}, [items]);
	const contextValue = (0, import_react.useMemo)(() => ({
		activeIndex,
		containerId,
		disabled,
		disableTransforms,
		items,
		overIndex,
		useDragOverlay,
		sortedRects: getSortedRects(items, droppableRects),
		strategy
	}), [
		activeIndex,
		containerId,
		disabled.draggable,
		disabled.droppable,
		disableTransforms,
		items,
		overIndex,
		droppableRects,
		useDragOverlay,
		strategy
	]);
	return import_react.createElement(Context.Provider, { value: contextValue }, children);
}
var defaultNewIndexGetter = (_ref) => {
	let { id, items, activeIndex, overIndex } = _ref;
	return arrayMove(items, activeIndex, overIndex).indexOf(id);
};
var defaultAnimateLayoutChanges = (_ref2) => {
	let { containerId, isSorting, wasDragging, index, items, newIndex, previousItems, previousContainerId, transition } = _ref2;
	if (!transition || !wasDragging) return false;
	if (previousItems !== items && index === newIndex) return false;
	if (isSorting) return true;
	return newIndex !== index && containerId === previousContainerId;
};
var defaultTransition = {
	duration: 200,
	easing: "ease"
};
var transitionProperty = "transform";
var disabledTransition = /* @__PURE__ */ CSS$1.Transition.toString({
	property: transitionProperty,
	duration: 0,
	easing: "linear"
});
var defaultAttributes = { roleDescription: "sortable" };
function useDerivedTransform(_ref) {
	let { disabled, index, node, rect } = _ref;
	const [derivedTransform, setDerivedtransform] = (0, import_react.useState)(null);
	const previousIndex = (0, import_react.useRef)(index);
	useIsomorphicLayoutEffect(() => {
		if (!disabled && index !== previousIndex.current && node.current) {
			const initial = rect.current;
			if (initial) {
				const current = getClientRect(node.current, { ignoreTransform: true });
				const delta = {
					x: initial.left - current.left,
					y: initial.top - current.top,
					scaleX: initial.width / current.width,
					scaleY: initial.height / current.height
				};
				if (delta.x || delta.y) setDerivedtransform(delta);
			}
		}
		if (index !== previousIndex.current) previousIndex.current = index;
	}, [
		disabled,
		index,
		node,
		rect
	]);
	(0, import_react.useEffect)(() => {
		if (derivedTransform) setDerivedtransform(null);
	}, [derivedTransform]);
	return derivedTransform;
}
function useSortable(_ref) {
	let { animateLayoutChanges = defaultAnimateLayoutChanges, attributes: userDefinedAttributes, disabled: localDisabled, data: customData, getNewIndex = defaultNewIndexGetter, id, strategy: localStrategy, resizeObserverConfig, transition = defaultTransition } = _ref;
	const { items, containerId, activeIndex, disabled: globalDisabled, disableTransforms, sortedRects, overIndex, useDragOverlay, strategy: globalStrategy } = (0, import_react.useContext)(Context);
	const disabled = normalizeLocalDisabled(localDisabled, globalDisabled);
	const index = items.indexOf(id);
	const data = (0, import_react.useMemo)(() => ({
		sortable: {
			containerId,
			index,
			items
		},
		...customData
	}), [
		containerId,
		customData,
		index,
		items
	]);
	const itemsAfterCurrentSortable = (0, import_react.useMemo)(() => items.slice(items.indexOf(id)), [items, id]);
	const { rect, node, isOver, setNodeRef: setDroppableNodeRef } = useDroppable({
		id,
		data,
		disabled: disabled.droppable,
		resizeObserverConfig: {
			updateMeasurementsFor: itemsAfterCurrentSortable,
			...resizeObserverConfig
		}
	});
	const { active, activatorEvent, activeNodeRect, attributes, setNodeRef: setDraggableNodeRef, listeners, isDragging, over, setActivatorNodeRef, transform } = useDraggable({
		id,
		data,
		attributes: {
			...defaultAttributes,
			...userDefinedAttributes
		},
		disabled: disabled.draggable
	});
	const setNodeRef = useCombinedRefs(setDroppableNodeRef, setDraggableNodeRef);
	const isSorting = Boolean(active);
	const displaceItem = isSorting && !disableTransforms && isValidIndex(activeIndex) && isValidIndex(overIndex);
	const shouldDisplaceDragSource = !useDragOverlay && isDragging;
	const dragSourceDisplacement = shouldDisplaceDragSource && displaceItem ? transform : null;
	const finalTransform = displaceItem ? dragSourceDisplacement != null ? dragSourceDisplacement : (localStrategy != null ? localStrategy : globalStrategy)({
		rects: sortedRects,
		activeNodeRect,
		activeIndex,
		overIndex,
		index
	}) : null;
	const newIndex = isValidIndex(activeIndex) && isValidIndex(overIndex) ? getNewIndex({
		id,
		items,
		activeIndex,
		overIndex
	}) : index;
	const activeId = active == null ? void 0 : active.id;
	const previous = (0, import_react.useRef)({
		activeId,
		items,
		newIndex,
		containerId
	});
	const itemsHaveChanged = items !== previous.current.items;
	const shouldAnimateLayoutChanges = animateLayoutChanges({
		active,
		containerId,
		isDragging,
		isSorting,
		id,
		index,
		items,
		newIndex: previous.current.newIndex,
		previousItems: previous.current.items,
		previousContainerId: previous.current.containerId,
		transition,
		wasDragging: previous.current.activeId != null
	});
	const derivedTransform = useDerivedTransform({
		disabled: !shouldAnimateLayoutChanges,
		index,
		node,
		rect
	});
	(0, import_react.useEffect)(() => {
		if (isSorting && previous.current.newIndex !== newIndex) previous.current.newIndex = newIndex;
		if (containerId !== previous.current.containerId) previous.current.containerId = containerId;
		if (items !== previous.current.items) previous.current.items = items;
	}, [
		isSorting,
		newIndex,
		containerId,
		items
	]);
	(0, import_react.useEffect)(() => {
		if (activeId === previous.current.activeId) return;
		if (activeId != null && previous.current.activeId == null) {
			previous.current.activeId = activeId;
			return;
		}
		const timeoutId = setTimeout(() => {
			previous.current.activeId = activeId;
		}, 50);
		return () => clearTimeout(timeoutId);
	}, [activeId]);
	return {
		active,
		activeIndex,
		attributes,
		data,
		rect,
		index,
		newIndex,
		items,
		isOver,
		isSorting,
		isDragging,
		listeners,
		node,
		overIndex,
		over,
		setNodeRef,
		setActivatorNodeRef,
		setDroppableNodeRef,
		setDraggableNodeRef,
		transform: derivedTransform != null ? derivedTransform : finalTransform,
		transition: getTransition()
	};
	function getTransition() {
		if (derivedTransform || itemsHaveChanged && previous.current.newIndex === index) return disabledTransition;
		if (shouldDisplaceDragSource && !isKeyboardEvent(activatorEvent) || !transition) return;
		if (isSorting || shouldAnimateLayoutChanges) return CSS$1.Transition.toString({
			...transition,
			property: transitionProperty
		});
	}
}
function normalizeLocalDisabled(localDisabled, globalDisabled) {
	var _localDisabled$dragga, _localDisabled$droppa;
	if (typeof localDisabled === "boolean") return {
		draggable: localDisabled,
		droppable: false
	};
	return {
		draggable: (_localDisabled$dragga = localDisabled == null ? void 0 : localDisabled.draggable) != null ? _localDisabled$dragga : globalDisabled.draggable,
		droppable: (_localDisabled$droppa = localDisabled == null ? void 0 : localDisabled.droppable) != null ? _localDisabled$droppa : globalDisabled.droppable
	};
}
KeyboardCode.Down, KeyboardCode.Right, KeyboardCode.Up, KeyboardCode.Left;
var NO_PANES = Object.freeze([]);
var EMPTY_INDEX = /* @__PURE__ */ new Map();
function appendPane(index, tabId, pane) {
	const panes = index.get(tabId);
	if (panes) panes.push(pane);
	else index.set(tabId, [pane]);
}
var cachedStatusSource = null;
var cachedLiveIndex = EMPTY_INDEX;
var cachedCompletedIndex = EMPTY_INDEX;
function indexAgentStatus(source) {
	if (source === cachedStatusSource) return;
	const live = /* @__PURE__ */ new Map();
	const completed = /* @__PURE__ */ new Map();
	for (const [paneKey, entry] of Object.entries(source)) {
		const agent = agentTypeToIconAgent(entry?.agentType);
		if (!agent) continue;
		const parsed = parsePaneKey(paneKey);
		if (!parsed) continue;
		appendPane(entry.state === "done" ? completed : live, parsed.tabId, {
			leafId: parsed.leafId,
			agent
		});
	}
	cachedLiveIndex = live;
	cachedCompletedIndex = completed;
	cachedStatusSource = source;
}
function selectLiveTabAgentPanes(agentStatusByPaneKey, tabId) {
	indexAgentStatus(agentStatusByPaneKey);
	return cachedLiveIndex.get(tabId) ?? NO_PANES;
}
function selectCompletedTabAgentPanes(agentStatusByPaneKey, tabId) {
	indexAgentStatus(agentStatusByPaneKey);
	return cachedCompletedIndex.get(tabId) ?? NO_PANES;
}
var cachedRetainedSource = null;
var cachedRetainedIndex = EMPTY_INDEX;
function selectRetainedTabAgentPanes(retainedAgentsByPaneKey, tabId) {
	if (retainedAgentsByPaneKey !== cachedRetainedSource) {
		const retainedIndex = /* @__PURE__ */ new Map();
		for (const [paneKey, retained] of Object.entries(retainedAgentsByPaneKey)) {
			const agent = agentTypeToIconAgent(retained?.agentType);
			if (!agent) continue;
			const parsed = parsePaneKey(paneKey);
			if (!parsed) continue;
			appendPane(retainedIndex, parsed.tabId, {
				leafId: parsed.leafId,
				agent
			});
		}
		cachedRetainedIndex = retainedIndex;
		cachedRetainedSource = retainedAgentsByPaneKey;
	}
	return cachedRetainedIndex.get(tabId) ?? NO_PANES;
}
function firstTabAgentExcludingLeaf(panes, excludedLeafId) {
	for (const pane of panes) if (pane.leafId !== excludedLeafId) return pane.agent;
	return null;
}
function resolveFocusedTabAgent(agentStatusByPaneKey, layout, tabId) {
	const activeLeafId = layout?.activeLeafId;
	if (activeLeafId && isTerminalLeafId(activeLeafId)) return agentFromStatusEntry(agentStatusByPaneKey[makePaneKey(tabId, activeLeafId)]);
	return resolveAnyTabAgent(agentStatusByPaneKey, tabId);
}
function resolveSiblingTabAgent(agentStatusByPaneKey, layout, tabId) {
	const activeLeafId = layout?.activeLeafId && isTerminalLeafId(layout.activeLeafId) ? layout.activeLeafId : null;
	if (!activeLeafId) return null;
	return resolveAnyTabAgent(agentStatusByPaneKey, tabId, activeLeafId);
}
function resolveAnyTabAgent(agentStatusByPaneKey, tabId, excludedLeafId) {
	return firstTabAgentExcludingLeaf(selectLiveTabAgentPanes(agentStatusByPaneKey, tabId), excludedLeafId);
}
function agentFromStatusEntry(entry) {
	if (!entry || entry.state === "done") return null;
	return agentTypeToIconAgent(entry.agentType);
}
function resolveFocusedCompletedTabAgent(agentStatusByPaneKey, layout, tabId) {
	const activeLeafId = layout?.activeLeafId;
	if (activeLeafId && isTerminalLeafId(activeLeafId)) return completedAgentFromStatusEntry(agentStatusByPaneKey[makePaneKey(tabId, activeLeafId)]);
	return resolveAnyCompletedTabAgent(agentStatusByPaneKey, tabId);
}
function resolveSiblingCompletedTabAgent(agentStatusByPaneKey, layout, tabId) {
	const activeLeafId = layout?.activeLeafId && isTerminalLeafId(layout.activeLeafId) ? layout.activeLeafId : null;
	if (!activeLeafId) return null;
	return resolveAnyCompletedTabAgent(agentStatusByPaneKey, tabId, activeLeafId);
}
function resolveAnyCompletedTabAgent(agentStatusByPaneKey, tabId, excludedLeafId) {
	return firstTabAgentExcludingLeaf(selectCompletedTabAgentPanes(agentStatusByPaneKey, tabId), excludedLeafId);
}
function completedAgentFromStatusEntry(entry) {
	if (!entry || entry.state !== "done") return null;
	return agentTypeToIconAgent(entry.agentType);
}
function resolveFocusedRetainedTabAgent(retainedAgentsByPaneKey, layout, tabId) {
	const activeLeafId = layout?.activeLeafId;
	if (activeLeafId && isTerminalLeafId(activeLeafId)) return agentFromRetainedEntry(retainedAgentsByPaneKey[makePaneKey(tabId, activeLeafId)]);
	return resolveAnyRetainedTabAgent(retainedAgentsByPaneKey, tabId);
}
function resolveSiblingRetainedTabAgent(retainedAgentsByPaneKey, layout, tabId) {
	const activeLeafId = layout?.activeLeafId && isTerminalLeafId(layout.activeLeafId) ? layout.activeLeafId : null;
	if (!activeLeafId) return null;
	return resolveAnyRetainedTabAgent(retainedAgentsByPaneKey, tabId, activeLeafId);
}
function resolveAnyRetainedTabAgent(retainedAgentsByPaneKey, tabId, excludedLeafId) {
	return firstTabAgentExcludingLeaf(selectRetainedTabAgentPanes(retainedAgentsByPaneKey, tabId), excludedLeafId);
}
function agentFromRetainedEntry(entry) {
	return agentTypeToIconAgent(entry?.agentType);
}
function titleShowsNoAgent(title, defaultTitle) {
	const trimmed = title.trim();
	return trimmed.length > 0 && (isShellProcess(trimmed) || trimmed === defaultTitle?.trim());
}
function resolveSignalAgentForLaunchOwner(signalAgent, launchAgent) {
	if (!signalAgent) return null;
	return resolveCompatibleAgentTypeForOwner(signalAgent, launchAgent) ?? signalAgent;
}
function resolveLaunchedAgentExitEvidence(args) {
	if (args.hookAgent || args.siblingHookAgent || args.processAgent) return false;
	if (!args.isRemote && args.processShellForeground && args.hasObservedAgentSignal) return true;
	if (!titleShowsNoAgent(args.title, args.defaultTitle)) return false;
	return args.hasCompletedHook || !args.isRemote && args.hasObservedAgentSignal;
}
function resolveTabAgentFromSignals(args) {
	const launchAgent = args.launchAgent ?? null;
	const owner = resolvePaneAgentOwner({
		launchAgent,
		hookAgent: args.hookAgent,
		completedHookAgent: args.focusedCompletedHookAgent,
		sleepingSessionAgent: args.sleepingSessionAgent
	});
	const liveFocusedIdentity = resolveSignalAgentForLaunchOwner(args.hookAgent, owner);
	const liveSiblingIdentity = resolveSignalAgentForLaunchOwner(args.siblingHookAgent, launchAgent);
	const processProvesShell = !args.isRemote && args.processShellForeground === true;
	const hasCompletedHook = (args.focusedCompletedHookAgent ?? null) !== null;
	const noAgentTitle = titleShowsNoAgent(args.title, args.defaultTitle);
	const idleFocusedIdentity = !args.isRemote && (noAgentTitle || processProvesShell) && hasCompletedHook ? null : resolveSignalAgentForLaunchOwner(args.focusedCompletedHookAgent, owner);
	const idleSiblingIdentity = resolveSignalAgentForLaunchOwner(args.siblingCompletedHookAgent, launchAgent);
	const sleepingSessionAgent = args.sleepingSessionAgent ?? null;
	const explicitTitleAgent = resolveSignalAgentForLaunchOwner(resolveExplicitTerminalTitleAgentType(args.title), owner);
	const priorIdentity = idleFocusedIdentity ?? launchAgent;
	const nativeOpenCodeTitle = explicitTitleAgent === "opencode" && isOpenCodeNativeTitle(args.title);
	const titleClaimsIdentity = explicitTitleAgent !== "claude" || isClaudeIdentityFrameTitle(args.title);
	const titleReclaimsReusedPane = priorIdentity !== null && explicitTitleAgent !== null && explicitTitleAgent !== priorIdentity && titleClaimsIdentity && (args.hasObservedAgentSignal || hasCompletedHook || nativeOpenCodeTitle);
	const titleAgent = processProvesShell || sleepingSessionAgent || nativeOpenCodeTitle && idleFocusedIdentity !== null ? null : titleReclaimsReusedPane ? explicitTitleAgent : priorIdentity ? null : explicitTitleAgent;
	const activeLaunchAgent = resolveLaunchedAgentExitEvidence({
		title: args.title,
		defaultTitle: args.defaultTitle,
		isRemote: args.isRemote,
		hasObservedAgentSignal: args.hasObservedAgentSignal,
		hookAgent: liveFocusedIdentity,
		siblingHookAgent: liveSiblingIdentity,
		hasCompletedHook,
		processAgent: args.processAgent,
		processShellForeground: args.processShellForeground
	}) ? null : launchAgent;
	const processAgent = resolveSignalAgentForLaunchOwner(args.processAgent, owner);
	return liveFocusedIdentity ?? processAgent ?? titleAgent ?? idleFocusedIdentity ?? sleepingSessionAgent ?? activeLaunchAgent ?? liveSiblingIdentity ?? idleSiblingIdentity;
}
function useTabAgent(tab) {
	const focusedHookAgent = useAppStore((s) => resolveFocusedTabAgent(s.agentStatusByPaneKey, s.terminalLayoutsByTabId[tab.id], tab.id));
	const siblingHookAgent = useAppStore((s) => resolveSiblingTabAgent(s.agentStatusByPaneKey, s.terminalLayoutsByTabId[tab.id], tab.id));
	const focusedCompletedHookAgent = useAppStore((s) => resolveFocusedCompletedTabAgent(s.agentStatusByPaneKey, s.terminalLayoutsByTabId[tab.id], tab.id) ?? resolveFocusedRetainedTabAgent(s.retainedAgentsByPaneKey, s.terminalLayoutsByTabId[tab.id], tab.id));
	const siblingCompletedHookAgent = useAppStore((s) => resolveSiblingCompletedTabAgent(s.agentStatusByPaneKey, s.terminalLayoutsByTabId[tab.id], tab.id) ?? resolveSiblingRetainedTabAgent(s.retainedAgentsByPaneKey, s.terminalLayoutsByTabId[tab.id], tab.id));
	const hasCompletedHook = focusedCompletedHookAgent !== null;
	const clearTabLaunchAgent = useAppStore((s) => s.clearTabLaunchAgent);
	const focusedPaneKey = useAppStore((s) => {
		const activeLeafId = s.terminalLayoutsByTabId[tab.id]?.activeLeafId;
		return activeLeafId && isTerminalLeafId(activeLeafId) ? makePaneKey(tab.id, activeLeafId) : null;
	});
	const processAgent = useAppStore((s) => focusedPaneKey ? s.paneForegroundAgentByPaneKey[focusedPaneKey]?.agent ?? null : null);
	const processShellForeground = useAppStore((s) => focusedPaneKey ? Boolean(s.paneForegroundAgentByPaneKey[focusedPaneKey]?.shellForeground) : false);
	const sleepingSessionAgent = useAppStore((s) => focusedPaneKey ? s.sleepingAgentSessionsByPaneKey[focusedPaneKey]?.agent ?? null : null);
	const ptyId = useAppStore((s) => {
		const layout = s.terminalLayoutsByTabId[tab.id];
		const activeLeafId = layout?.activeLeafId;
		const leafPty = activeLeafId ? layout?.ptyIdsByLeafId?.[activeLeafId] : void 0;
		if (leafPty) return leafPty;
		const ptyIds = s.ptyIdsByTabId[tab.id] ?? [];
		return ptyIds.length === 1 ? ptyIds[0] : null;
	});
	const completedHookScopeKnown = useAppStore((s) => {
		const layout = s.terminalLayoutsByTabId[tab.id];
		if (layout?.activeLeafId && isTerminalLeafId(layout.activeLeafId)) return true;
		return (s.ptyIdsByTabId[tab.id] ?? []).length <= 1;
	});
	const hasRemoteRuntimePty = useAppStore((s) => {
		const layout = s.terminalLayoutsByTabId[tab.id];
		const ptyIds = new Set(s.ptyIdsByTabId[tab.id] ?? []);
		for (const ptyId$1 of Object.values(layout?.ptyIdsByLeafId ?? {})) ptyIds.add(ptyId$1);
		return [...ptyIds].some((ptyId$1) => parseRemoteRuntimePtyId(ptyId$1) !== null);
	});
	const isRemoteLike = useAppStore((s) => worktreeUsesRemoteConnection(s, tab.worktreeId)) || hasRemoteRuntimePty;
	const [hasObservedAgentSignal, setHasObservedAgentSignal] = (0, import_react.useState)(false);
	const hasObservedAgentSignalRef = (0, import_react.useRef)(false);
	const signalGenerationRef = (0, import_react.useRef)(null);
	const completedHookEvidence = hasCompletedHook && completedHookScopeKnown;
	(0, import_react.useEffect)(() => {
		const generation = `${ptyId ?? ""}|${String(isRemoteLike)}`;
		if (signalGenerationRef.current !== generation) {
			signalGenerationRef.current = generation;
			hasObservedAgentSignalRef.current = false;
			setHasObservedAgentSignal(false);
		}
		const explicitTitleAgent = resolveExplicitTerminalTitleAgentType(tab.title);
		const fallbackAgentSignal = tab.launchAgent ? explicitTitleAgent === tab.launchAgent : Boolean(explicitTitleAgent || siblingHookAgent);
		if (focusedHookAgent || completedHookEvidence || processAgent || fallbackAgentSignal) {
			hasObservedAgentSignalRef.current = true;
			setHasObservedAgentSignal(true);
		}
	}, [
		ptyId,
		isRemoteLike,
		focusedHookAgent,
		completedHookEvidence,
		processAgent,
		siblingHookAgent,
		tab.launchAgent,
		tab.title
	]);
	(0, import_react.useEffect)(() => {
		if (!tab.launchAgent) return;
		if (resolveLaunchedAgentExitEvidence({
			title: tab.title,
			defaultTitle: tab.defaultTitle,
			isRemote: isRemoteLike,
			hasObservedAgentSignal: hasObservedAgentSignal && hasObservedAgentSignalRef.current,
			hookAgent: focusedHookAgent,
			siblingHookAgent,
			hasCompletedHook: completedHookEvidence,
			processAgent,
			processShellForeground
		})) clearTabLaunchAgent(tab.id);
	}, [
		clearTabLaunchAgent,
		completedHookEvidence,
		focusedHookAgent,
		siblingHookAgent,
		hasObservedAgentSignal,
		isRemoteLike,
		processAgent,
		processShellForeground,
		tab.defaultTitle,
		tab.id,
		tab.launchAgent,
		tab.title
	]);
	return resolveTabAgentFromSignals({
		hasObservedAgentSignal,
		isRemote: isRemoteLike,
		title: tab.title,
		defaultTitle: tab.defaultTitle,
		hookAgent: focusedHookAgent,
		siblingHookAgent,
		focusedCompletedHookAgent,
		siblingCompletedHookAgent,
		processAgent,
		processShellForeground,
		sleepingSessionAgent,
		launchAgent: tab.launchAgent
	});
}
function getDropIndicatorClasses(dropIndicator) {
	if (dropIndicator === "left") return "before:absolute before:inset-y-0 before:left-0 before:w-[2px] before:bg-blue-500 before:z-10 before:content-['']";
	if (dropIndicator === "right") return "after:absolute after:inset-y-0 after:right-0 after:w-[2px] after:bg-blue-500 after:z-10 after:content-['']";
	return "";
}
const ACTIVE_TAB_INDICATOR_CLASSES = "pointer-events-none absolute inset-x-0 bottom-0 h-[2px] bg-[color-mix(in_srgb,var(--foreground)_60%,var(--card))] z-10";
function getTabRootStateClasses(isActive) {
	return isActive ? "bg-[color-mix(in_srgb,var(--foreground)_6%,var(--card))] text-foreground" : "bg-card text-muted-foreground hover:text-foreground";
}
function getTabStripBorderClasses(hasTabsToRight, options) {
	return [
		options?.includeTopBorder ?? true ? "border-t" : "",
		hasTabsToRight ? "border-r" : "",
		"border-border"
	].filter(Boolean).join(" ");
}
function preventMiddleButtonDefault(event) {
	if (event.button === 1) event.preventDefault();
}
function mirrorWebRuntimeTabMove(args) {
	const environmentId = getRuntimeEnvironmentIdForWorktree(useAppStore.getState(), args.worktreeId);
	if (!isWebRuntimeSessionActive(environmentId)) return;
	moveWebRuntimeSessionTab({
		...args,
		environmentId
	});
}
function canMoveTabToNewPaneColumnFromState(state, unifiedTabId, groupId) {
	for (const [worktreeId, tabs] of Object.entries(state.unifiedTabsByWorktree)) {
		const tab = tabs.find((candidate) => candidate.id === unifiedTabId);
		if (!tab || tab.groupId !== groupId) continue;
		const group = (state.groupsByWorktree[worktreeId] ?? []).find((candidate) => candidate.id === groupId);
		if (!group) return false;
		return group.tabOrder.length > 1;
	}
	return false;
}
function canMoveTabToNewPaneColumn(unifiedTabId, groupId) {
	return canMoveTabToNewPaneColumnFromState(useAppStore.getState(), unifiedTabId, groupId);
}
function moveTabToNewPaneColumn(args) {
	const state = useAppStore.getState();
	const worktreeId = Object.entries(state.unifiedTabsByWorktree).find(([, tabs]) => tabs.some((candidate) => candidate.id === args.unifiedTabId && candidate.groupId === args.groupId))?.[0];
	if (!worktreeId || !canMoveTabToNewPaneColumnFromState(state, args.unifiedTabId, args.groupId)) return false;
	const moved = state.dropUnifiedTab(args.unifiedTabId, {
		groupId: args.groupId,
		splitDirection: args.direction
	});
	if (moved) mirrorWebRuntimeTabMove({
		kind: "split",
		worktreeId,
		tabId: args.unifiedTabId,
		targetGroupId: args.groupId,
		splitDirection: args.direction
	});
	return moved;
}
const TAB_CONTEXT_MENU_CONTENT_CLASS = "min-w-[13rem] max-w-[calc(100vw-1rem)] whitespace-nowrap";
const TAB_CONTEXT_SUBMENU_CONTENT_CLASS = "max-w-[calc(100vw-1rem)] whitespace-nowrap";
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var PANE_COLUMN_DIRECTIONS = [
	"right",
	"left",
	"down",
	"up"
];
function paneColumnDirectionIcon(direction) {
	switch (direction) {
		case "right": return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowRight, { className: "size-3.5 shrink-0" });
		case "left": return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowLeft, { className: "size-3.5 shrink-0" });
		case "down": return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowDown, { className: "size-3.5 shrink-0" });
		case "up": return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowUp, { className: "size-3.5 shrink-0" });
	}
}
function paneColumnDirectionLabel(direction) {
	switch (direction) {
		case "right": return translate("auto.components.tab.bar.TabWorkspaceLayoutMenuSection.right", "Right");
		case "left": return translate("auto.components.tab.bar.TabWorkspaceLayoutMenuSection.left", "Left");
		case "down": return translate("auto.components.tab.bar.TabWorkspaceLayoutMenuSection.down", "Down");
		case "up": return translate("auto.components.tab.bar.TabWorkspaceLayoutMenuSection.up", "Up");
	}
}
function TabWorkspaceLayoutMenuSection({ unifiedTabId, groupId, trailingSeparator = false }) {
	if (!canMoveTabToNewPaneColumn(unifiedTabId, groupId)) return null;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuSub, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuSubTrigger, {
		className: "[&>svg:last-child]:size-3.5",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Columns2, { className: "size-3.5 shrink-0" }), translate("auto.components.tab.bar.TabWorkspaceLayoutMenuSection.moveToPaneColumn", "Move Tab to Split")]
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSubContent, {
		className: TAB_CONTEXT_SUBMENU_CONTENT_CLASS,
		children: PANE_COLUMN_DIRECTIONS.map((direction) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
			onSelect: () => {
				moveTabToNewPaneColumn({
					unifiedTabId,
					groupId,
					direction
				});
			},
			children: [paneColumnDirectionIcon(direction), paneColumnDirectionLabel(direction)]
		}, direction))
	})] }), trailingSeparator ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {}) : null] });
}
function TerminalTabSplitMenuSection({ unifiedTabId, groupId, tabId, isActive, onActivate, splitRightShortcut, splitDownShortcut, trailingSeparator = false }) {
	const splitActiveTerminalPane = (direction) => {
		if (!isActive) onActivate(tabId);
		requestActiveTerminalPaneSplit({
			tabId,
			direction
		});
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
		/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TabWorkspaceLayoutMenuSection, {
			unifiedTabId,
			groupId
		}),
		/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuSub, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuSubTrigger, {
			className: "[&>svg:last-child]:size-3.5",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SquareTerminal, { className: "size-3.5 shrink-0" }), translate("auto.components.tab.bar.TerminalTabSplitMenuSection.splitTerminal", "Split terminal")]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuSubContent, {
			className: cn("min-w-[12rem]", TAB_CONTEXT_SUBMENU_CONTENT_CLASS),
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
				onSelect: () => splitActiveTerminalPane("vertical"),
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(PanelRightClose, { className: "size-3.5 shrink-0" }),
					translate("auto.components.tab.bar.SortableTabContextMenu.splitTerminalRight", "Split terminal right"),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuShortcut, { children: splitRightShortcut })
				]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
				onSelect: () => splitActiveTerminalPane("horizontal"),
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(PanelBottomClose, { className: "size-3.5 shrink-0" }),
					translate("auto.components.tab.bar.SortableTabContextMenu.splitTerminalDown", "Split terminal down"),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuShortcut, { children: splitDownShortcut })
				]
			})]
		})] }),
		trailingSeparator ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {}) : null
	] });
}
var TAB_COLORS = [
	{
		get label() {
			return translate("auto.components.tab.bar.SortableTabContextMenu.20baa43c05", "None");
		},
		value: null
	},
	{
		get label() {
			return translate("auto.components.tab.bar.SortableTabContextMenu.cb3eadefd2", "Blue");
		},
		value: "#3b82f6"
	},
	{
		get label() {
			return translate("auto.components.tab.bar.SortableTabContextMenu.c2d8b0991f", "Purple");
		},
		value: "#a855f7"
	},
	{
		get label() {
			return translate("auto.components.tab.bar.SortableTabContextMenu.03cf6dab1a", "Pink");
		},
		value: "#ec4899"
	},
	{
		get label() {
			return translate("auto.components.tab.bar.SortableTabContextMenu.620aec6729", "Red");
		},
		value: "#ef4444"
	},
	{
		get label() {
			return translate("auto.components.tab.bar.SortableTabContextMenu.a47629b3cf", "Orange");
		},
		value: "#f97316"
	},
	{
		get label() {
			return translate("auto.components.tab.bar.SortableTabContextMenu.69682e2ce4", "Yellow");
		},
		value: "#eab308"
	},
	{
		get label() {
			return translate("auto.components.tab.bar.SortableTabContextMenu.be905e9b0a", "Green");
		},
		value: "#22c55e"
	},
	{
		get label() {
			return translate("auto.components.tab.bar.SortableTabContextMenu.845576bed1", "Teal");
		},
		value: "#14b8a6"
	},
	{
		get label() {
			return translate("auto.components.tab.bar.SortableTabContextMenu.7703990447", "Gray");
		},
		value: "#9ca3af"
	}
];
function SortableTabContextMenu({ tab, unifiedTabId, groupId, isActive, open, point, tabCount, hasTabsToRight, hasTabsToLeft, isPinned, onOpenChange, onActivate, onClose, onCloseOthers, onCloseToRight, onCloseToLeft, onRenameOpen, onSetTabColor, onTogglePin, canToggleViewMode = false, isChatView = false, onToggleViewMode }) {
	const keybindings = useAppStore((state) => state.keybindings);
	const splitRightShortcut = formatShortcutLabel("terminal.splitRight", keybindings);
	const splitDownShortcut = formatShortcutLabel("terminal.splitDown", keybindings);
	const closeShortcut = useOptionalShortcutLabel("tab.close");
	const renameShortcut = useOptionalShortcutLabel("tab.rename");
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenu, {
		open,
		onOpenChange,
		modal: false,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuTrigger, {
			asChild: true,
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				"aria-hidden": true,
				tabIndex: -1,
				className: "pointer-events-none fixed size-px opacity-0",
				style: {
					left: point.x,
					top: point.y
				}
			})
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuContent, {
			className: TAB_CONTEXT_MENU_CONTENT_CLASS,
			sideOffset: 0,
			align: "start",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TerminalTabSplitMenuSection, {
					unifiedTabId,
					groupId,
					tabId: tab.id,
					isActive,
					onActivate,
					splitRightShortcut,
					splitDownShortcut
				}),
				canToggleViewMode && onToggleViewMode ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
					onSelect: onToggleViewMode,
					children: [isChatView ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SquareTerminal, { className: "size-3.5 shrink-0" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MessageSquare, { className: "size-3.5 shrink-0" }), isChatView ? translate("components.tab.bar.SortableTabContextMenu.switchToTerminalView", "Switch to terminal view") : translate("components.tab.bar.SortableTabContextMenu.switchToChatView", "Switch to chat view")]
				})] }) : null,
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
					onSelect: onTogglePin,
					children: [isPinned ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PinOff, { className: "size-3.5 shrink-0" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Pin, { className: "size-3.5 shrink-0" }), isPinned ? translate("auto.components.tab.bar.SortableTabContextMenu.417722e9c2", "Unpin Tab") : translate("auto.components.tab.bar.SortableTabContextMenu.60f958ec75", "Pin Tab")]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
					onSelect: () => !isPinned && onClose(tab.id),
					disabled: isPinned,
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "size-3.5" }),
						translate("auto.components.tab.bar.SortableTabContextMenu.89359a36f7", "Close"),
						closeShortcut ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuShortcut, { children: closeShortcut }) : null
					]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
					onSelect: () => onCloseOthers(tab.id),
					disabled: tabCount <= 1,
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ListX, { className: "size-3.5" }), translate("auto.components.tab.bar.SortableTabContextMenu.8d16f9cd30", "Close Others")]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
					onSelect: () => onCloseToRight(tab.id),
					disabled: !hasTabsToRight,
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(PanelRightClose, { className: "size-3.5" }), translate("auto.components.tab.bar.SortableTabContextMenu.c1ee099c7e", "Close Tabs To The Right")]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
					onSelect: () => onCloseToLeft(tab.id),
					disabled: !hasTabsToLeft,
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(PanelLeftClose, { className: "size-3.5" }), translate("components.tab.bar.SortableTabContextMenu.closeTabsToLeft", "Close Tabs To The Left")]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
					onSelect: onRenameOpen,
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Pencil, { className: "size-3.5" }),
						translate("auto.components.tab.bar.SortableTabContextMenu.2f697b3c31", "Change Title"),
						renameShortcut ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuShortcut, { children: renameShortcut }) : null
					]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "px-2 pt-1.5 pb-1",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "text-xs font-medium text-muted-foreground mb-1.5",
						children: translate("auto.components.tab.bar.SortableTabContextMenu.35e8892fd0", "Tab Color")
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "flex flex-wrap gap-2",
						children: TAB_COLORS.map((color) => {
							return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuItem, {
								className: `relative h-4 w-4 min-w-4 p-0 rounded-full border ${tab.color === color.value ? "ring-1 ring-foreground/70 ring-offset-1 ring-offset-popover" : ""} ${color.value ? "border-transparent" : "border-muted-foreground/50 bg-transparent"}`,
								style: color.value ? { backgroundColor: color.value } : void 0,
								onSelect: () => {
									onSetTabColor(tab.id, color.value);
								},
								children: color.value === null && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "absolute block h-px w-3 rotate-45 bg-muted-foreground/80" })
							}, color.label);
						})
					})]
				})
			]
		})]
	});
}
const TAB_CONTAINER_WIDTH_CLASSES = "w-[180px] min-w-[88px] min-[1280px]:w-[220px]";
const TAB_LABEL_WIDTH_CLASSES = "min-w-0 flex-1 truncate";
function resolveTabInsertion(event, isTabDragData$1, getDragCenter) {
	const overData = event.over?.data.current;
	const activeData = event.active.data.current;
	if (!event.over || !isTabDragData$1(activeData) || !isTabDragData$1(overData)) return null;
	if (activeData.unifiedTabId === overData.unifiedTabId) return null;
	const center = getDragCenter(event);
	if (!center) return null;
	const midpoint = event.over.rect.left + event.over.rect.width / 2;
	return {
		groupId: overData.groupId,
		visibleTabId: overData.visibleTabId,
		side: center.x < midpoint ? "left" : "right"
	};
}
function resolveTabIndicatorEdges(orderedVisibleTabIds, hoveredTabInsertion) {
	if (!hoveredTabInsertion || orderedVisibleTabIds.length === 0) return [];
	const hoveredIndex = orderedVisibleTabIds.indexOf(hoveredTabInsertion.visibleTabId);
	if (hoveredIndex === -1) return [];
	const insertionIndex = hoveredIndex + (hoveredTabInsertion.side === "right" ? 1 : 0);
	if (insertionIndex < orderedVisibleTabIds.length) return [{
		visibleTabId: orderedVisibleTabIds[insertionIndex],
		side: "left"
	}];
	return [{
		visibleTabId: orderedVisibleTabIds[insertionIndex - 1],
		side: "right"
	}];
}
function equal(a, b) {
	if (a === b) return true;
	return a !== null && b !== null && a.groupId === b.groupId && a.visibleTabId === b.visibleTabId && a.side === b.side;
}
function useHoveredTabInsertion(isTabDragData$1, getDragCenter) {
	const [hoveredTabInsertion, setHoveredTabInsertion] = (0, import_react.useState)(null);
	return {
		hoveredTabInsertion,
		update: (0, import_react.useCallback)((event) => {
			const next = resolveTabInsertion(event, isTabDragData$1, getDragCenter);
			setHoveredTabInsertion((prev) => equal(prev, next) ? prev : next);
		}, [isTabDragData$1, getDragCenter]),
		clear: (0, import_react.useCallback)(() => setHoveredTabInsertion(null), [])
	};
}
function previewActiveSurfacePatch(state, worktreeId, groupId, tabId) {
	if (state.activeWorktreeId !== worktreeId || !tabId) return {};
	const unifiedTab = (state.unifiedTabsByWorktree[worktreeId] ?? []).find((tab) => tab.id === tabId && tab.groupId === groupId);
	if (!unifiedTab) return {};
	const nextActiveTabTypeByWorktree = (tabType) => ({
		...state.activeTabTypeByWorktree,
		[worktreeId]: tabType
	});
	if (unifiedTab.contentType === "terminal") return {
		activeTabId: unifiedTab.entityId,
		activeTabType: "terminal",
		activeTabIdByWorktree: {
			...state.activeTabIdByWorktree,
			[worktreeId]: unifiedTab.entityId
		},
		activeTabTypeByWorktree: nextActiveTabTypeByWorktree("terminal")
	};
	if (unifiedTab.contentType === "browser") return {
		activeBrowserTabId: unifiedTab.entityId,
		activeTabType: "browser",
		activeBrowserTabIdByWorktree: {
			...state.activeBrowserTabIdByWorktree,
			[worktreeId]: unifiedTab.entityId
		},
		activeTabTypeByWorktree: nextActiveTabTypeByWorktree("browser")
	};
	if (unifiedTab.contentType === "simulator") return {
		activeTabType: "simulator",
		activeTabTypeByWorktree: nextActiveTabTypeByWorktree("simulator")
	};
	return {
		activeFileId: unifiedTab.entityId,
		activeTabType: "editor",
		activeFileIdByWorktree: {
			...state.activeFileIdByWorktree,
			[worktreeId]: unifiedTab.entityId
		},
		activeTabTypeByWorktree: nextActiveTabTypeByWorktree("editor")
	};
}
function captureTabDragActivationSnapshot(worktreeId) {
	const state = useAppStore.getState();
	const groups = state.groupsByWorktree[worktreeId] ?? [];
	return {
		activeGroupId: state.activeGroupIdByWorktree[worktreeId] ?? null,
		activeTabIdByGroup: Object.fromEntries(groups.map((group) => [group.id, group.activeTabId]))
	};
}
function applyDragPreviewTab({ worktreeId, groupId, tabId, activeGroupId }) {
	useAppStore.setState((state) => {
		const groups = state.groupsByWorktree[worktreeId] ?? [];
		const groupUnchanged = groups.find((group) => group.id === groupId)?.activeTabId === tabId;
		const focusUnchanged = (state.activeGroupIdByWorktree[worktreeId] ?? null) === activeGroupId;
		const surfacePatch = previewActiveSurfacePatch(state, worktreeId, groupId, tabId);
		if (groupUnchanged && focusUnchanged) return Object.keys(surfacePatch).length > 0 ? surfacePatch : {};
		const next = { ...surfacePatch };
		if (!groupUnchanged) next.groupsByWorktree = {
			...state.groupsByWorktree,
			[worktreeId]: groups.map((group) => group.id === groupId ? {
				...group,
				activeTabId: tabId
			} : group)
		};
		if (!focusUnchanged) next.activeGroupIdByWorktree = {
			...state.activeGroupIdByWorktree,
			[worktreeId]: activeGroupId
		};
		return next;
	});
}
function restoreTabDragActivationSnapshot(worktreeId, snapshot) {
	useAppStore.setState((state) => {
		const groups = state.groupsByWorktree[worktreeId] ?? [];
		const groupsUnchanged = groups.every((group) => (snapshot.activeTabIdByGroup[group.id] ?? null) === group.activeTabId);
		const focusUnchanged = (state.activeGroupIdByWorktree[worktreeId] ?? null) === snapshot.activeGroupId;
		const next = {};
		if (!groupsUnchanged) next.groupsByWorktree = {
			...state.groupsByWorktree,
			[worktreeId]: groups.map((group) => ({
				...group,
				activeTabId: snapshot.activeTabIdByGroup[group.id] ?? null
			}))
		};
		if (!focusUnchanged) if (snapshot.activeGroupId === null) {
			const nextActiveGroupIdByWorktree = { ...state.activeGroupIdByWorktree };
			delete nextActiveGroupIdByWorktree[worktreeId];
			next.activeGroupIdByWorktree = nextActiveGroupIdByWorktree;
		} else next.activeGroupIdByWorktree = {
			...state.activeGroupIdByWorktree,
			[worktreeId]: snapshot.activeGroupId
		};
		const restoredGroupId = snapshot.activeGroupId;
		if (restoredGroupId) {
			const restoredTabId = snapshot.activeTabIdByGroup[restoredGroupId] ?? null;
			Object.assign(next, previewActiveSurfacePatch(state, worktreeId, restoredGroupId, restoredTabId));
		}
		if (Object.keys(next).length === 0) return {};
		return next;
	});
}
function restoreSourceGroupActiveTabAfterCrossGroupDrop({ worktreeId, snapshot, sourceGroupId, movedTabId }) {
	const preDragActiveTabId = snapshot.activeTabIdByGroup[sourceGroupId] ?? null;
	if (preDragActiveTabId === movedTabId) return;
	useAppStore.setState((state) => {
		const groups = state.groupsByWorktree[worktreeId] ?? [];
		const sourceGroup = groups.find((group) => group.id === sourceGroupId);
		if (!sourceGroup || sourceGroup.activeTabId === preDragActiveTabId) return {};
		return { groupsByWorktree: {
			...state.groupsByWorktree,
			[worktreeId]: groups.map((group) => group.id === sourceGroupId ? {
				...group,
				activeTabId: preDragActiveTabId
			} : group)
		} };
	});
}
function canDropTabIntoPaneBody({ activeDrag, groupsByWorktree, overGroupId, worktreeId }) {
	if (!activeDrag || activeDrag.worktreeId !== worktreeId) return false;
	const overGroup = (groupsByWorktree[worktreeId] ?? []).find((group) => group.id === overGroupId);
	if (!overGroup) return false;
	return activeDrag.groupId !== overGroupId || overGroup.tabOrder.length > 1;
}
function isTabDragData(value) {
	return Boolean(value) && typeof value === "object" && value.kind === "tab";
}
function isPaneDropData(value) {
	return Boolean(value) && typeof value === "object" && value.kind === "pane-body";
}
function resolveDragPreviewTabId({ activeDrag, overData, preDragActiveTabIdByGroup, lastHoveredTabPreview = null }) {
	const sourceGroupId = activeDrag.groupId;
	const sourcePreDragTabId = preDragActiveTabIdByGroup[sourceGroupId] ?? null;
	if (isTabDragData(overData) && overData.unifiedTabId !== activeDrag.unifiedTabId) return {
		groupId: sourceGroupId,
		tabId: sourcePreDragTabId
	};
	if (isPaneDropData(overData)) {
		if (lastHoveredTabPreview?.groupId === overData.groupId && lastHoveredTabPreview.tabId) return lastHoveredTabPreview;
		if (overData.groupId === sourceGroupId) return {
			groupId: sourceGroupId,
			tabId: sourcePreDragTabId
		};
		return {
			groupId: overData.groupId,
			tabId: preDragActiveTabIdByGroup[overData.groupId] ?? null
		};
	}
	return {
		groupId: sourceGroupId,
		tabId: sourcePreDragTabId
	};
}
function resolveSourceGroupRestoreOnDrop(activeData, targetGroupId, restoreSnapshot) {
	if (restoreSnapshot || activeData.groupId === targetGroupId) return;
	return activeData;
}
function getDragPointer(event) {
	const activator = event.activatorEvent;
	if (activator && typeof activator === "object" && "clientX" in activator && "clientY" in activator && typeof activator.clientX === "number" && typeof activator.clientY === "number") return {
		x: activator.clientX + event.delta.x,
		y: activator.clientY + event.delta.y
	};
	const initial = event.active.rect.current.initial;
	if (!initial) return null;
	return {
		x: initial.left + initial.width / 2 + event.delta.x,
		y: initial.top + initial.height / 2 + event.delta.y
	};
}
init_defineProperty();
var DEFAULT_COORDINATES = {
	x: 0,
	y: 0
};
var TAB_DRAG_EARLY_MOVE_CONFIRMATION_MS = 50;
var TAB_DRAG_CONFIRMED_DISTANCE_SAMPLE_COUNT = 2;
var ListenerBag = class {
	constructor() {
		_defineProperty(this, "listeners", []);
		_defineProperty(this, "removeAll", () => {
			for (const { eventName, handler, options, target } of this.listeners) target.removeEventListener(eventName, handler, options);
			this.listeners.length = 0;
		});
	}
	add(target, eventName, handler, options) {
		if (!target) return;
		const listener = handler;
		target.addEventListener(eventName, listener, options);
		this.listeners.push({
			eventName,
			handler: listener,
			options,
			target
		});
	}
};
function isDistanceConstraint(constraint) {
	return "distance" in constraint;
}
function isDelayConstraint(constraint) {
	return "delay" in constraint;
}
function getOwnerDocument(target) {
	if (target instanceof Document) return target;
	if (target instanceof Node) return target.ownerDocument ?? document;
	return document;
}
function getPointerCoordinates(event) {
	if ("clientX" in event && "clientY" in event) {
		const pointerEvent = event;
		return {
			x: pointerEvent.clientX,
			y: pointerEvent.clientY
		};
	}
	return null;
}
function subtractCoordinates(start, current) {
	return {
		x: start.x - current.x,
		y: start.y - current.y
	};
}
function hasExceededDistance(delta, measurement) {
	const dx = Math.abs(delta.x);
	const dy = Math.abs(delta.y);
	if (typeof measurement === "number") return Math.hypot(dx, dy) > measurement;
	if ("x" in measurement && "y" in measurement) return dx > measurement.x && dy > measurement.y;
	if ("x" in measurement) return dx > measurement.x;
	if ("y" in measurement) return dy > measurement.y;
	return false;
}
function shouldActivateTabDragFromDistanceSample({ elapsedMs, overThresholdSampleCount }) {
	return elapsedMs >= TAB_DRAG_EARLY_MOVE_CONFIRMATION_MS || overThresholdSampleCount >= TAB_DRAG_CONFIRMED_DISTANCE_SAMPLE_COUNT;
}
var TabDragPointerSensor = class {
	constructor(props) {
		_defineProperty(this, "autoScrollEnabled", true);
		_defineProperty(this, "activated", false);
		_defineProperty(this, "document", void 0);
		_defineProperty(this, "initialCoordinates", void 0);
		_defineProperty(this, "pointerDownTime", performance.now());
		_defineProperty(this, "props", void 0);
		_defineProperty(this, "documentListeners", new ListenerBag());
		_defineProperty(this, "pointerListeners", new ListenerBag());
		_defineProperty(this, "windowListeners", new ListenerBag());
		_defineProperty(this, "overThresholdSampleCount", 0);
		_defineProperty(this, "timeoutId", null);
		this.props = props;
		this.document = getOwnerDocument(props.event.target);
		this.initialCoordinates = getPointerCoordinates(props.event) ?? DEFAULT_COORDINATES;
		this.handleStart = this.handleStart.bind(this);
		this.handleMove = this.handleMove.bind(this);
		this.handleEnd = this.handleEnd.bind(this);
		this.handleCancel = this.handleCancel.bind(this);
		this.handleKeydown = this.handleKeydown.bind(this);
		this.removeTextSelection = this.removeTextSelection.bind(this);
		this.attach();
	}
	attach() {
		const win = this.document.defaultView;
		const { activationConstraint, bypassActivationConstraint } = this.props.options;
		this.pointerListeners.add(this.document, "pointermove", this.handleMove, { passive: false });
		this.pointerListeners.add(this.document, "pointerup", this.handleEnd);
		this.pointerListeners.add(this.document, "pointercancel", this.handleCancel);
		this.windowListeners.add(win, "resize", this.handleCancel);
		this.windowListeners.add(win, "dragstart", preventDefault);
		this.windowListeners.add(win, "visibilitychange", this.handleCancel);
		this.windowListeners.add(win, "contextmenu", preventDefault);
		this.windowListeners.add(win, "focus", this.handleCancel);
		this.documentListeners.add(this.document, "keydown", this.handleKeydown);
		if (!activationConstraint) {
			this.handleStart();
			return;
		}
		if (bypassActivationConstraint?.({
			activeNode: this.props.activeNode,
			event: this.props.event,
			options: this.props.options
		})) {
			this.handleStart();
			return;
		}
		if (isDelayConstraint(activationConstraint)) {
			this.timeoutId = window.setTimeout(this.handleStart, activationConstraint.delay);
			this.handlePending(activationConstraint);
			return;
		}
		this.handlePending(activationConstraint);
	}
	detach() {
		this.pointerListeners.removeAll();
		this.windowListeners.removeAll();
		window.setTimeout(this.documentListeners.removeAll, 50);
		if (this.timeoutId !== null) {
			window.clearTimeout(this.timeoutId);
			this.timeoutId = null;
		}
	}
	handlePending(constraint, offset) {
		this.props.onPending(this.props.active, constraint, this.initialCoordinates, offset);
	}
	handleStart() {
		if (this.activated) return;
		this.activated = true;
		this.documentListeners.add(this.document, "click", stopPropagation, { capture: true });
		this.removeTextSelection();
		this.documentListeners.add(this.document, "selectionchange", this.removeTextSelection);
		this.props.onStart(this.initialCoordinates);
	}
	handleMove(event) {
		const coordinates = getPointerCoordinates(event);
		const { activationConstraint } = this.props.options;
		if (!coordinates) return;
		const delta = subtractCoordinates(this.initialCoordinates, coordinates);
		if (!this.activated && activationConstraint) {
			if (isDistanceConstraint(activationConstraint)) {
				if (activationConstraint.tolerance != null && hasExceededDistance(delta, activationConstraint.tolerance)) {
					this.handleCancel();
					return;
				}
				if (hasExceededDistance(delta, activationConstraint.distance)) {
					this.overThresholdSampleCount += 1;
					if (shouldActivateTabDragFromDistanceSample({
						elapsedMs: performance.now() - this.pointerDownTime,
						overThresholdSampleCount: this.overThresholdSampleCount
					})) {
						this.handleStart();
						return;
					}
				} else this.overThresholdSampleCount = 0;
			}
			if (isDelayConstraint(activationConstraint) && hasExceededDistance(delta, activationConstraint.tolerance)) {
				this.handleCancel();
				return;
			}
			this.handlePending(activationConstraint, delta);
			return;
		}
		if (event.cancelable) event.preventDefault();
		this.props.onMove(coordinates);
	}
	handleEnd() {
		this.detach();
		if (!this.activated) this.props.onAbort(this.props.active);
		this.props.onEnd();
	}
	handleCancel() {
		this.detach();
		if (!this.activated) this.props.onAbort(this.props.active);
		this.props.onCancel();
	}
	handleKeydown(event) {
		if (event.code === "Escape") this.handleCancel();
	}
	removeTextSelection() {
		this.document.getSelection()?.removeAllRanges();
	}
};
_defineProperty(TabDragPointerSensor, "activators", [{
	eventName: "onPointerDown",
	handler: ({ nativeEvent: event }, { onActivation }) => {
		if (!event.isPrimary || event.button !== 0) return false;
		onActivation?.({ event });
		return true;
	}
}]);
function preventDefault(event) {
	event.preventDefault();
}
function stopPropagation(event) {
	event.stopPropagation();
}
function resolveDropZone(rect, point) {
	const localX = point.x - rect.left;
	const localY = point.y - rect.top;
	const edgeWidthThreshold = rect.width * .1;
	const edgeHeightThreshold = rect.height * .1;
	const splitWidthThreshold = rect.width / 3;
	if (localX > edgeWidthThreshold && localX < rect.width - edgeWidthThreshold && localY > edgeHeightThreshold && localY < rect.height - edgeHeightThreshold) return "center";
	if (localX < splitWidthThreshold) return "left";
	if (localX > splitWidthThreshold * 2) return "right";
	return localY < rect.height / 2 ? "up" : "down";
}
function resolvePaneColumnEdgeZone(panelRect, point, options) {
	const localX = point.x - panelRect.left;
	const horizontalEdge = panelRect.width * .2;
	if (localX < horizontalEdge) return "left";
	if (localX > panelRect.width - horizontalEdge) return "right";
	const tabStripHeight = options?.tabStripHeightPx ?? 32;
	const tabStripBottom = panelRect.top + tabStripHeight;
	if (point.y < tabStripBottom) return null;
	const bodyRect = options?.bodyRect ?? {
		left: panelRect.left,
		top: tabStripBottom,
		width: panelRect.width,
		height: Math.max(0, panelRect.height - tabStripHeight)
	};
	if (bodyRect.height <= 0) return null;
	const bodyLocalY = point.y - bodyRect.top;
	const verticalEdge = bodyRect.height * .2;
	if (bodyLocalY < verticalEdge) return "up";
	if (bodyLocalY > bodyRect.height - verticalEdge) return "down";
	return null;
}
function escapeCssAttrValue(value) {
	if (typeof CSS !== "undefined" && typeof CSS.escape === "function") return CSS.escape(value);
	return value.replace(/\\/g, "\\\\").replace(/"/g, "\\\"");
}
function getTabGroupBodyElement(groupId, worktreeId) {
	const escapedGroupId = escapeCssAttrValue(groupId);
	const escapedWorktreeId = escapeCssAttrValue(worktreeId);
	return document.querySelector(`[data-tab-group-body-id="${escapedGroupId}"][data-worktree-id="${escapedWorktreeId}"]`);
}
function getTabGroupPanelRect(groupId, worktreeId) {
	return getTabGroupBodyElement(groupId, worktreeId)?.parentElement?.getBoundingClientRect() ?? null;
}
function getTabGroupBodyRect(groupId, worktreeId) {
	return getTabGroupBodyElement(groupId, worktreeId)?.getBoundingClientRect() ?? null;
}
function captureTabGroupPanelGeometrySnapshot(worktreeId) {
	const escapedWorktreeId = escapeCssAttrValue(worktreeId);
	const bodies = document.querySelectorAll(`[data-tab-group-body-id][data-worktree-id="${escapedWorktreeId}"]`);
	const entries = [];
	for (const body of bodies) {
		const groupId = body.dataset.tabGroupBodyId;
		const panelElement = body.parentElement;
		if (!groupId || !panelElement) continue;
		entries.push({
			groupId,
			panelRect: panelElement.getBoundingClientRect(),
			bodyRect: body.getBoundingClientRect()
		});
	}
	return {
		entries,
		byGroupId: new Map(entries.map((entry) => [entry.groupId, entry]))
	};
}
function findTabGroupPanelUnderPointer(worktreeId, pointer, options = {}) {
	if (options.geometry) {
		for (const entry of options.geometry.entries) {
			const { panelRect } = entry;
			if (pointer.x >= panelRect.left && pointer.x <= panelRect.right && pointer.y >= panelRect.top && pointer.y <= panelRect.bottom) return {
				groupId: entry.groupId,
				panelRect
			};
		}
		return null;
	}
	const getPanelRect = options.getPanelRect ?? getTabGroupPanelRect;
	const escapedWorktreeId = escapeCssAttrValue(worktreeId);
	const bodies = document.querySelectorAll(`[data-tab-group-body-id][data-worktree-id="${escapedWorktreeId}"]`);
	for (const body of bodies) {
		const groupId = body.dataset.tabGroupBodyId;
		if (!groupId) continue;
		const panelRect = getPanelRect(groupId, worktreeId);
		if (!panelRect) continue;
		if (pointer.x >= panelRect.left && pointer.x <= panelRect.right && pointer.y >= panelRect.top && pointer.y <= panelRect.bottom) return {
			groupId,
			panelRect
		};
	}
	return null;
}
function resolvePanelEdgePaneColumnSplit({ activeDrag, targetGroupId, worktreeId, pointer, groupsByWorktree, layoutByWorktree, panelRect: providedPanelRect, bodyRect: providedBodyRect }) {
	const panelRect = providedPanelRect ?? getTabGroupPanelRect(targetGroupId, worktreeId);
	if (!panelRect) return null;
	if (pointer.x < panelRect.left || pointer.x > panelRect.left + panelRect.width || pointer.y < panelRect.top || pointer.y > panelRect.top + panelRect.height) return null;
	const zone = resolvePaneColumnEdgeZone(panelRect, pointer, {
		bodyRect: providedBodyRect ?? getTabGroupBodyRect(targetGroupId, worktreeId) ?? null,
		tabStripHeightPx: 32
	});
	if (!zone) return null;
	const sourceGroup = (groupsByWorktree[worktreeId] ?? []).find((group) => group.id === activeDrag.groupId);
	if (isPaneColumnSplitDropNoOp({
		sourceGroupId: activeDrag.groupId,
		targetGroupId,
		splitDirection: zone,
		sourceTabCount: sourceGroup?.tabOrder.length ?? 0,
		layout: layoutByWorktree[worktreeId]
	})) return null;
	if (activeDrag.groupId === targetGroupId) {
		if (!canDropTabIntoPaneBody({
			activeDrag,
			groupsByWorktree,
			overGroupId: targetGroupId,
			worktreeId
		})) return null;
	}
	return {
		groupId: targetGroupId,
		zone
	};
}
function resolveActivePaneColumnSplitTarget({ event, groupsByWorktree, layoutByWorktree, worktreeId, getDragPointer: getDragPointer$1, geometry }) {
	const activeData = event.active.data.current;
	const pointer = getDragPointer$1(event);
	if (!isTabDragData(activeData) || !pointer) return null;
	const overData = event.over?.data.current;
	const panelHit = findTabGroupPanelUnderPointer(worktreeId, pointer, { geometry });
	if (isTabDragData(overData)) {
		if (!panelHit || pointer.y < panelHit.panelRect.top + 32) return null;
	}
	const targetGroupId = panelHit?.groupId ?? (isTabDragData(overData) ? overData.groupId : null) ?? (isPaneDropData(overData) ? overData.groupId : null);
	if (!targetGroupId) return null;
	const targetGeometry = geometry?.byGroupId.get(targetGroupId);
	const panelRect = panelHit?.groupId === targetGroupId ? panelHit.panelRect : targetGeometry?.panelRect;
	const splitTarget = resolvePanelEdgePaneColumnSplit({
		activeDrag: activeData,
		targetGroupId,
		worktreeId,
		pointer,
		groupsByWorktree,
		layoutByWorktree,
		panelRect,
		bodyRect: targetGeometry?.bodyRect
	});
	return splitTarget ? {
		...splitTarget,
		panelRect
	} : null;
}
var collisionDetection = (args) => {
	const pointerCollisions = pointerWithin(args);
	return pointerCollisions.length > 0 ? pointerCollisions : closestCenter(args);
};
function getTabPaneBodyDroppableId(groupId) {
	return `tab-group-pane-body:${groupId}`;
}
function getTabDragActivationDistance(enabled) {
	return enabled ? 12 : Number.MAX_SAFE_INTEGER;
}
function useTabDragSplit({ worktreeId, enabled = true }) {
	const reorderUnifiedTabs = useAppStore((state) => state.reorderUnifiedTabs);
	const dropUnifiedTab = useAppStore((state) => state.dropUnifiedTab);
	const [activeDrag, setActiveDrag] = (0, import_react.useState)(null);
	const [hoveredDropTarget, setHoveredDropTarget] = (0, import_react.useState)(null);
	const releaseWebviewDragPassthroughRef = (0, import_react.useRef)(null);
	const preDragActivationSnapshotRef = (0, import_react.useRef)(null);
	const lastPreviewRef = (0, import_react.useRef)(null);
	const lastHoveredTabPreviewRef = (0, import_react.useRef)(null);
	const tabDragActiveRef = (0, import_react.useRef)(false);
	const dragGeometryRef = (0, import_react.useRef)(null);
	const releaseMissedEndFallbackRef = (0, import_react.useRef)(null);
	const tabInsertion = useHoveredTabInsertion(isTabDragData, getDragPointer);
	const sensors = useSensors(useSensor(TabDragPointerSensor, { activationConstraint: { distance: getTabDragActivationDistance(enabled) } }));
	const releaseWebviewDragPassthrough = (0, import_react.useCallback)(() => {
		releaseWebviewDragPassthroughRef.current?.();
		releaseWebviewDragPassthroughRef.current = null;
	}, []);
	const releaseMissedEndFallback = (0, import_react.useCallback)(() => {
		releaseMissedEndFallbackRef.current?.();
		releaseMissedEndFallbackRef.current = null;
	}, []);
	const clearDragStateRef = (0, import_react.useRef)(() => {});
	const installMissedEndFallback = (0, import_react.useCallback)(() => {
		releaseMissedEndFallback();
		let cleanupTimer = null;
		const clearIfDndMissedEnd = () => {
			if (cleanupTimer !== null) window.clearTimeout(cleanupTimer);
			cleanupTimer = window.setTimeout(() => {
				cleanupTimer = null;
				if (tabDragActiveRef.current) clearDragStateRef.current();
			}, 0);
		};
		window.addEventListener("pointerup", clearIfDndMissedEnd);
		window.addEventListener("pointercancel", clearIfDndMissedEnd);
		window.addEventListener("blur", clearIfDndMissedEnd);
		window.addEventListener("focus", clearIfDndMissedEnd);
		releaseMissedEndFallbackRef.current = () => {
			if (cleanupTimer !== null) window.clearTimeout(cleanupTimer);
			window.removeEventListener("pointerup", clearIfDndMissedEnd);
			window.removeEventListener("pointercancel", clearIfDndMissedEnd);
			window.removeEventListener("blur", clearIfDndMissedEnd);
			window.removeEventListener("focus", clearIfDndMissedEnd);
		};
	}, [releaseMissedEndFallback]);
	const acquireWebviewDragPassthrough = (0, import_react.useCallback)(() => {
		releaseWebviewDragPassthrough();
		releaseWebviewDragPassthroughRef.current = acquireWebviewsDragPassthrough();
	}, [releaseWebviewDragPassthrough]);
	const setDragRootNode = (0, import_react.useCallback)((node) => {
		if (node) return;
		releaseWebviewDragPassthrough();
		releaseMissedEndFallback();
	}, [releaseMissedEndFallback, releaseWebviewDragPassthrough]);
	const clearDragState = (0, import_react.useCallback)(() => {
		tabDragActiveRef.current = false;
		releaseWebviewDragPassthrough();
		releaseMissedEndFallback();
		setActiveDrag(null);
		setHoveredDropTarget(null);
		tabInsertion.clear();
		preDragActivationSnapshotRef.current = null;
		lastPreviewRef.current = null;
		lastHoveredTabPreviewRef.current = null;
		dragGeometryRef.current = null;
	}, [
		releaseMissedEndFallback,
		releaseWebviewDragPassthrough,
		tabInsertion
	]);
	clearDragStateRef.current = clearDragState;
	const restorePreDragActivation = (0, import_react.useCallback)(() => {
		const snapshot = preDragActivationSnapshotRef.current;
		if (!snapshot) return;
		restoreTabDragActivationSnapshot(worktreeId, snapshot);
	}, [worktreeId]);
	const restoreSourceGroupAfterCrossGroupDrop = (0, import_react.useCallback)((activeData) => {
		const snapshot = preDragActivationSnapshotRef.current;
		if (!snapshot) return;
		restoreSourceGroupActiveTabAfterCrossGroupDrop({
			worktreeId,
			snapshot,
			sourceGroupId: activeData.groupId,
			movedTabId: activeData.unifiedTabId
		});
	}, [worktreeId]);
	const finishDrag = (0, import_react.useCallback)((restoreSnapshot, activeData) => {
		if (restoreSnapshot) restorePreDragActivation();
		else if (activeData) restoreSourceGroupAfterCrossGroupDrop(activeData);
		clearDragState();
	}, [
		clearDragState,
		restorePreDragActivation,
		restoreSourceGroupAfterCrossGroupDrop
	]);
	const updateDragPreviewActivation = (0, import_react.useCallback)((event, activeData) => {
		const snapshot = preDragActivationSnapshotRef.current;
		if (!snapshot) return;
		const overData = event.over?.data.current;
		if (isTabDragData(overData) && overData.unifiedTabId !== activeData.unifiedTabId) lastHoveredTabPreviewRef.current = {
			groupId: overData.groupId,
			tabId: overData.unifiedTabId
		};
		const preview = resolveDragPreviewTabId({
			activeDrag: activeData,
			overData,
			preDragActiveTabIdByGroup: snapshot.activeTabIdByGroup,
			lastHoveredTabPreview: lastHoveredTabPreviewRef.current
		});
		const lastPreview = lastPreviewRef.current;
		if (lastPreview?.groupId === preview.groupId && lastPreview.tabId === preview.tabId) return;
		lastPreviewRef.current = preview;
		applyDragPreviewTab({
			worktreeId,
			groupId: preview.groupId,
			tabId: preview.tabId,
			activeGroupId: preview.groupId
		});
	}, [worktreeId]);
	const updateHoveredDropTargetFromSplit = (0, import_react.useCallback)((splitTarget) => {
		if (!splitTarget) {
			setHoveredDropTarget((prev) => prev === null ? prev : null);
			return;
		}
		setHoveredDropTarget((prev) => {
			if (prev?.groupId === splitTarget.groupId && prev?.zone === splitTarget.zone) return prev;
			return {
				groupId: splitTarget.groupId,
				zone: splitTarget.zone,
				panelRect: splitTarget.panelRect
			};
		});
	}, []);
	const handleDragUpdate = (0, import_react.useCallback)((event) => {
		const activeData = event.active.data.current;
		if (isTabDragData(activeData) && activeData.worktreeId === worktreeId) updateDragPreviewActivation(event, activeData);
		const state = useAppStore.getState();
		const splitTarget = resolveActivePaneColumnSplitTarget({
			event,
			groupsByWorktree: state.groupsByWorktree,
			layoutByWorktree: state.layoutByWorktree,
			worktreeId,
			getDragPointer,
			geometry: dragGeometryRef.current
		});
		updateHoveredDropTargetFromSplit(splitTarget);
		if (splitTarget) tabInsertion.clear();
		else tabInsertion.update(event);
	}, [
		tabInsertion,
		updateDragPreviewActivation,
		updateHoveredDropTargetFromSplit,
		worktreeId
	]);
	const onDragStart = (0, import_react.useCallback)((event) => {
		const dragData = event.active.data.current;
		if (!isTabDragData(dragData) || dragData.worktreeId !== worktreeId) {
			clearDragState();
			return;
		}
		setActiveDrag(dragData);
		tabDragActiveRef.current = true;
		installMissedEndFallback();
		dragGeometryRef.current = captureTabGroupPanelGeometrySnapshot(worktreeId);
		preDragActivationSnapshotRef.current = captureTabDragActivationSnapshot(worktreeId);
		acquireWebviewDragPassthrough();
	}, [
		acquireWebviewDragPassthrough,
		clearDragState,
		installMissedEndFallback,
		worktreeId
	]);
	const onDragMove = (0, import_react.useCallback)((event) => {
		handleDragUpdate(event);
	}, [handleDragUpdate]);
	const onDragOver = (0, import_react.useCallback)((_event) => {}, []);
	const onDragEnd = (0, import_react.useCallback)((event) => {
		const activeData = event.active.data.current;
		const overData = event.over?.data.current;
		let shouldRestorePreDragActivation = true;
		if (!isTabDragData(activeData) || activeData.worktreeId !== worktreeId) {
			finishDrag(true);
			return;
		}
		const state = useAppStore.getState();
		const paneColumnSplit = resolveActivePaneColumnSplitTarget({
			event,
			groupsByWorktree: state.groupsByWorktree,
			layoutByWorktree: state.layoutByWorktree,
			worktreeId,
			getDragPointer,
			geometry: dragGeometryRef.current
		});
		if (paneColumnSplit) {
			if (dropUnifiedTab(activeData.unifiedTabId, {
				groupId: paneColumnSplit.groupId,
				splitDirection: paneColumnSplit.zone
			})) {
				shouldRestorePreDragActivation = false;
				mirrorWebRuntimeTabMove({
					kind: "split",
					worktreeId,
					tabId: activeData.unifiedTabId,
					targetGroupId: paneColumnSplit.groupId,
					splitDirection: paneColumnSplit.zone
				});
			}
			finishDrag(shouldRestorePreDragActivation, resolveSourceGroupRestoreOnDrop(activeData, paneColumnSplit.groupId, shouldRestorePreDragActivation));
			return;
		}
		if (!event.over) {
			finishDrag(true);
			return;
		}
		if (isTabDragData(overData)) {
			if (activeData.unifiedTabId === overData.unifiedTabId) {
				finishDrag(true);
				return;
			}
			const targetGroup = (state.groupsByWorktree[worktreeId] ?? []).find((group) => group.id === overData.groupId);
			if (!targetGroup) {
				finishDrag(true);
				return;
			}
			const insertion = resolveTabInsertion(event, isTabDragData, getDragPointer);
			if (!insertion) {
				finishDrag(true);
				return;
			}
			const overIndex = targetGroup.tabOrder.indexOf(overData.unifiedTabId);
			const rawInsertIndex = overIndex + (insertion.side === "right" ? 1 : 0);
			if (activeData.groupId === overData.groupId) {
				const oldIndex = targetGroup.tabOrder.indexOf(activeData.unifiedTabId);
				const nextIndex = oldIndex < rawInsertIndex ? rawInsertIndex - 1 : rawInsertIndex;
				if (oldIndex !== -1 && oldIndex !== nextIndex) {
					const nextOrder = targetGroup.tabOrder.filter((id) => id !== activeData.unifiedTabId);
					nextOrder.splice(nextIndex, 0, activeData.unifiedTabId);
					reorderUnifiedTabs(overData.groupId, nextOrder);
					mirrorWebRuntimeTabMove({
						kind: "reorder",
						worktreeId,
						tabId: activeData.unifiedTabId,
						targetGroupId: overData.groupId,
						tabOrder: nextOrder
					});
				}
			} else {
				const index = overIndex === -1 ? targetGroup.tabOrder.length : rawInsertIndex;
				if (dropUnifiedTab(activeData.unifiedTabId, {
					groupId: overData.groupId,
					index
				})) {
					shouldRestorePreDragActivation = false;
					mirrorWebRuntimeTabMove({
						kind: "move-to-group",
						worktreeId,
						tabId: activeData.unifiedTabId,
						targetGroupId: overData.groupId,
						index
					});
				}
			}
			finishDrag(shouldRestorePreDragActivation, resolveSourceGroupRestoreOnDrop(activeData, overData.groupId, shouldRestorePreDragActivation));
			return;
		}
		if (isPaneDropData(overData)) {
			if (activeData.groupId !== overData.groupId) {
				if (dropUnifiedTab(activeData.unifiedTabId, { groupId: overData.groupId })) {
					shouldRestorePreDragActivation = false;
					mirrorWebRuntimeTabMove({
						kind: "move-to-group",
						worktreeId,
						tabId: activeData.unifiedTabId,
						targetGroupId: overData.groupId
					});
				}
			}
		}
		finishDrag(shouldRestorePreDragActivation, isPaneDropData(overData) ? resolveSourceGroupRestoreOnDrop(activeData, overData.groupId, shouldRestorePreDragActivation) : void 0);
	}, [
		dropUnifiedTab,
		finishDrag,
		reorderUnifiedTabs,
		worktreeId
	]);
	const onDragCancel = (0, import_react.useCallback)(() => {
		finishDrag(true);
	}, [finishDrag]);
	return {
		activeDrag,
		collisionDetection,
		hoveredDropTarget,
		hoveredTabInsertion: tabInsertion.hoveredTabInsertion,
		isTabDragActiveRef: tabDragActiveRef,
		onDragCancel,
		onDragEnd,
		onDragMove,
		onDragOver,
		onDragStart,
		sensors,
		setDragRootNode
	};
}
function useTabStripPointerActivation({ onActivate, disabled = false }) {
	const onActivateRef = (0, import_react.useRef)(onActivate);
	onActivateRef.current = onActivate;
	const cleanupRef = (0, import_react.useRef)(null);
	(0, import_react.useEffect)(() => () => cleanupRef.current?.(), []);
	return { onPointerDown: (0, import_react.useCallback)((event, dragListener) => {
		if (disabled || event.button !== 0) return;
		dragListener?.(event);
		cleanupRef.current?.();
		const startX = event.clientX;
		const startY = event.clientY;
		const releaseTabStripPointerGesture = beginTabStripPointerGesture();
		const cleanup = () => {
			window.removeEventListener("pointerup", onPointerUp);
			window.removeEventListener("pointercancel", onPointerCancel);
			window.removeEventListener("blur", onPointerCancel);
			window.removeEventListener("focus", onPointerCancel);
			releaseTabStripPointerGesture();
			cleanupRef.current = null;
		};
		const onPointerUp = (upEvent) => {
			const wasDrag = Math.hypot(upEvent.clientX - startX, upEvent.clientY - startY) >= 12;
			cleanup();
			if (!wasDrag) onActivateRef.current();
		};
		const onPointerCancel = () => {
			cleanup();
		};
		window.addEventListener("pointerup", onPointerUp);
		window.addEventListener("pointercancel", onPointerCancel);
		window.addEventListener("blur", onPointerCancel);
		window.addEventListener("focus", onPointerCancel);
		cleanupRef.current = cleanup;
	}, [disabled]) };
}
function TerminalTabAgentIdentityIcon({ agent, isActive, className }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		className: cn("inline-flex", !isActive && "opacity-70", className),
		"data-agent-icon": agent,
		"aria-hidden": true,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentIcon, {
			agent,
			size: 12
		})
	});
}
function TerminalTabLeadingIcon({ agent, activityStatus, shell, showUnreadActivity, isActive }) {
	if (showUnreadActivity) return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
		"data-testid": "tab-activity-bell",
		"aria-label": translate("auto.components.tab.bar.TerminalTabLeadingIcon.7ab2964bea", "Unread agent completion"),
		className: "mr-1 inline-flex shrink-0 items-center gap-1",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(FilledBellIcon, { className: "size-3 text-amber-500 drop-shadow-sm" }), agent ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TerminalTabAgentIdentityIcon, {
			agent,
			isActive
		}) : null]
	});
	const dotState = terminalTabActivityToAgentDotState(activityStatus);
	if (dotState) return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
		"data-testid": "tab-agent-activity-indicator",
		"data-agent-activity-status": activityStatus,
		className: "mr-1 inline-flex shrink-0 items-center gap-1",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentStateDot, {
			state: dotState,
			size: "md"
		}), agent ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TerminalTabAgentIdentityIcon, {
			agent,
			isActive
		}) : null]
	});
	if (agent) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TerminalTabAgentIdentityIcon, {
		agent,
		isActive,
		className: "mr-1 shrink-0"
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		className: `mr-1 inline-flex shrink-0 ${isActive ? "" : "opacity-70"}`,
		"data-shell-icon": shell ?? "generic",
		"aria-hidden": true,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ShellIcon, {
			shell,
			size: 12
		})
	});
}
const CLOSE_ALL_CONTEXT_MENUS_EVENT = "orca-close-all-context-menus";
function SortableTab({ tab, unifiedTabId, groupId, tabCount, hasTabsToRight, hasTabsToLeft, isActive, isPinned, isExpanded, onActivate, onClose, onCloseOthers, onCloseToRight, onCloseToLeft, onSetCustomTitle, onSetTabColor, onTogglePin, onToggleExpand, dragData, dropIndicator, includeTopTabBorder = true, canToggleViewMode = false, isChatView = false, onToggleViewMode }) {
	const hasUnreadActivity = useAppStore((s) => terminalTabHasUnreadActivity({
		terminalTabId: tab.id,
		unreadTerminalTabs: s.unreadTerminalTabs,
		unreadAgentCompletionPanes: s.unreadAgentCompletionPanes
	}));
	const activityStatus = useAppStore((s) => resolveTerminalTabActivityStatus({
		tab,
		agentStatusByPaneKey: s.agentStatusByPaneKey,
		agentStatusEpoch: s.agentStatusEpoch,
		runtimePaneTitlesByTabId: s.runtimePaneTitlesByTabId,
		ptyIdsByTabId: s.ptyIdsByTabId,
		terminalLayout: s.terminalLayoutsByTabId?.[tab.id]
	}));
	const renamingTabId = useAppStore((s) => s.renamingTabId);
	const setRenamingTabId = useAppStore((s) => s.setRenamingTabId);
	const shellForIcon = tab.shellOverride;
	const tabAgent = useTabAgent(tab);
	const displayTitle = tab.customTitle ?? (tabAgent ? stripLeadingAgentTitleDecoration(tab.title) : tab.title);
	const { attributes, listeners, setNodeRef } = useSortable({
		id: tab.id,
		data: {
			...dragData,
			agent: tabAgent
		}
	});
	const [menuOpen, setMenuOpen] = (0, import_react.useState)(false);
	const [menuPoint, setMenuPoint] = (0, import_react.useState)({
		x: 0,
		y: 0
	});
	const [isEditing, setIsEditing] = (0, import_react.useState)(false);
	const showUnreadActivity = hasUnreadActivity && !isEditing && !isTerminalTabActivityLive(activityStatus);
	const [renameValue, setRenameValue] = (0, import_react.useState)("");
	const renameFocusFrameRef = (0, import_react.useRef)(null);
	const committedOrCancelledRef = (0, import_react.useRef)(false);
	const handleRenameOpen = (0, import_react.useCallback)(() => {
		committedOrCancelledRef.current = false;
		setRenameValue(tab.customTitle ?? tab.title);
		setIsEditing(true);
	}, [tab.customTitle, tab.title]);
	const commitRename = (0, import_react.useCallback)(() => {
		if (committedOrCancelledRef.current) return;
		committedOrCancelledRef.current = true;
		const trimmed = renameValue.trim();
		onSetCustomTitle(tab.id, trimmed.length > 0 ? trimmed : null);
		setIsEditing(false);
	}, [
		renameValue,
		onSetCustomTitle,
		tab.id
	]);
	const cancelRename = (0, import_react.useCallback)(() => {
		committedOrCancelledRef.current = true;
		setIsEditing(false);
	}, []);
	const setRenameInputElement = (0, import_react.useCallback)((input) => {
		if (renameFocusFrameRef.current !== null) {
			cancelAnimationFrame(renameFocusFrameRef.current);
			renameFocusFrameRef.current = null;
		}
		if (!input) return;
		renameFocusFrameRef.current = requestAnimationFrame(() => {
			renameFocusFrameRef.current = null;
			input.focus();
			input.select();
		});
	}, []);
	(0, import_react.useEffect)(() => {
		if (renamingTabId !== tab.id) return;
		handleRenameOpen();
		setRenamingTabId(null);
	}, [
		renamingTabId,
		tab.id,
		handleRenameOpen,
		setRenamingTabId
	]);
	(0, import_react.useEffect)(() => {
		const closeMenu = () => setMenuOpen(false);
		window.addEventListener(CLOSE_ALL_CONTEXT_MENUS_EVENT, closeMenu);
		return () => window.removeEventListener(CLOSE_ALL_CONTEXT_MENUS_EVENT, closeMenu);
	}, []);
	(0, import_react.useEffect)(() => {
		if (!menuOpen) return;
		const dismiss = () => setMenuOpen(false);
		window.addEventListener("blur", dismiss);
		return () => window.removeEventListener("blur", dismiss);
	}, [menuOpen]);
	const dragListeners = isEditing ? void 0 : listeners;
	const { onPointerDown: onTabPointerDown } = useTabStripPointerActivation({
		onActivate: (0, import_react.useCallback)(() => {
			onActivate(tab.id);
		}, [onActivate, tab.id]),
		disabled: isEditing
	});
	const closeShortcut = useOptionalShortcutLabel("tab.close");
	const closeLabel = translate("auto.components.tab.bar.SortableTab.95db5f2f7d", "Close tab");
	const tabTitle = tab.customTitle ?? tab.title;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: TAB_CONTAINER_WIDTH_CLASSES,
		onContextMenuCapture: (event) => {
			event.preventDefault();
			window.dispatchEvent(new Event(CLOSE_ALL_CONTEXT_MENUS_EVENT));
			setMenuPoint({
				x: event.clientX,
				y: event.clientY
			});
			setMenuOpen(true);
		},
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			ref: setNodeRef,
			"data-testid": "sortable-tab",
			"data-tab-id": tab.id,
			"data-tab-title": tabTitle,
			"data-pinned": isPinned ? "true" : "false",
			"data-active": isActive ? "true" : "false",
			"data-agent-activity-status": activityStatus,
			...attributes,
			...dragListeners,
			className: `group relative flex items-center h-full px-1.5 text-xs cursor-pointer select-none outline-none focus:outline-none focus-visible:outline-none ${getTabStripBorderClasses(hasTabsToRight, { includeTopBorder: includeTopTabBorder })} ${getDropIndicatorClasses(dropIndicator ?? null)} ${getTabRootStateClasses(isActive)}`,
			onDoubleClick: (e) => {
				if (isEditing) return;
				e.stopPropagation();
				handleRenameOpen();
			},
			onPointerDown: (e) => {
				onTabPointerDown(e, dragListeners?.onPointerDown);
			},
			onMouseDown: (e) => {
				if (e.button === 1) e.preventDefault();
			},
			onMouseUp: preventMiddleButtonDefault,
			onAuxClick: (e) => {
				if (isEditing) return;
				if (e.button === 1) {
					e.preventDefault();
					e.stopPropagation();
					if (isPinned) return;
					onClose(tab.id);
				}
			},
			children: [
				isActive && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "pointer-events-none absolute inset-x-0 bottom-0 h-[2px] bg-[color-mix(in_srgb,var(--foreground)_60%,var(--card))] z-10",
					"aria-hidden": true
				}),
				showUnreadActivity && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					"aria-hidden": true,
					className: "pointer-events-none absolute inset-0 bg-amber-500/10"
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TerminalTabLeadingIcon, {
					agent: tabAgent,
					activityStatus,
					shell: shellForIcon,
					showUnreadActivity,
					isActive
				}),
				isPinned && !isEditing && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Pin, {
					className: "mr-1 size-3 shrink-0 text-muted-foreground",
					"aria-hidden": true
				}),
				isEditing ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
					ref: setRenameInputElement,
					"data-tab-rename-input": "true",
					value: renameValue,
					"aria-label": translate("auto.components.tab.bar.SortableTab.ab19f603eb", "Rename tab {{value0}}", { value0: tabTitle }),
					onChange: (event) => setRenameValue(event.target.value),
					onBlur: commitRename,
					onKeyDown: (event) => {
						if (isImeCompositionKeyDown(event)) return;
						if (event.key === "Enter") {
							event.preventDefault();
							commitRename();
						} else if (event.key === "Escape") {
							event.preventDefault();
							cancelRename();
						}
					},
					onPointerDown: (event) => event.stopPropagation(),
					onMouseDown: (event) => {
						event.stopPropagation();
						if (event.button === 1) event.preventDefault();
					},
					onClick: (event) => event.stopPropagation(),
					onDoubleClick: (event) => event.stopPropagation(),
					onAuxClick: (event) => event.stopPropagation(),
					className: "mr-1 h-5 min-w-[72px] flex-1 px-1 py-0 text-xs",
					spellCheck: false
				}) : isEditing || menuOpen ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: `${TAB_LABEL_WIDTH_CLASSES} mr-1`,
					children: displayTitle
				}) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
					asChild: true,
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: `${TAB_LABEL_WIDTH_CLASSES} mr-1`,
						children: displayTitle
					})
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
					side: "bottom",
					sideOffset: 6,
					className: "max-w-80 whitespace-normal break-words text-left",
					children: displayTitle
				})] }),
				tab.color && !isEditing && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "mr-1.5 size-2 rounded-full shrink-0",
					style: { backgroundColor: tab.color }
				}),
				isExpanded && !isEditing && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
					className: `mr-1 flex items-center justify-center w-4 h-4 rounded-sm shrink-0 ${isActive ? "text-muted-foreground hover:text-foreground hover:bg-muted" : "text-transparent group-hover:text-muted-foreground hover:!text-foreground hover:!bg-muted"}`,
					onPointerDown: (e) => e.stopPropagation(),
					onClick: (e) => {
						e.stopPropagation();
						onToggleExpand(tab.id);
					},
					title: translate("auto.components.tab.bar.SortableTab.fdb2691425", "Collapse pane"),
					"aria-label": translate("auto.components.tab.bar.SortableTab.fdb2691425", "Collapse pane"),
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Minimize2, { className: "w-3 h-3" })
				}),
				!isEditing && !isPinned && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
					asChild: true,
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
						className: `relative z-10 flex items-center justify-center w-4 h-4 rounded-sm shrink-0 ${isActive ? "text-muted-foreground hover:text-foreground hover:bg-muted focus-visible:text-foreground focus-visible:bg-muted" : "text-transparent group-hover:text-muted-foreground hover:!text-foreground hover:!bg-muted focus-visible:!text-foreground focus-visible:!bg-muted"}`,
						"aria-label": translate("auto.components.tab.bar.SortableTab.6df69d9388", "Close tab {{value0}}", { value0: tabTitle }),
						type: "button",
						"data-tab-close-button": "true",
						onPointerDown: (e) => {
							if (e.button === 0) e.stopPropagation();
						},
						onMouseDown: (e) => {
							if (e.button === 0) e.stopPropagation();
						},
						onClick: (e) => {
							e.preventDefault();
							e.stopPropagation();
							onClose(tab.id);
						},
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "w-3 h-3" })
					})
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
					side: "bottom",
					sideOffset: 6,
					children: closeShortcut ? `${closeLabel} (${closeShortcut})` : closeLabel
				})] })
			]
		})
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SortableTabContextMenu, {
		tab,
		unifiedTabId,
		groupId,
		isActive,
		open: menuOpen,
		point: menuPoint,
		tabCount,
		hasTabsToRight,
		hasTabsToLeft,
		isPinned,
		onOpenChange: setMenuOpen,
		onActivate,
		onClose,
		onCloseOthers,
		onCloseToRight,
		onCloseToLeft,
		onRenameOpen: handleRenameOpen,
		onSetTabColor,
		onTogglePin,
		canToggleViewMode,
		isChatView,
		onToggleViewMode
	})] });
}
var MAX_STEPS = 50;
var past = [];
var future = [];
function commitFileExplorerOp(op) {
	past.push(op);
	if (past.length > MAX_STEPS) past.shift();
	future.length = 0;
}
function clearFileExplorerUndoHistory() {
	past.length = 0;
	future.length = 0;
}
async function undoFileExplorer() {
	const op = past.pop();
	if (!op) return false;
	await op.undo();
	future.push(op);
	return true;
}
async function redoFileExplorer() {
	const op = future.pop();
	if (!op) return false;
	await op.redo();
	past.push(op);
	return true;
}
function fileExplorerHasUndo() {
	return past.length > 0;
}
function fileExplorerHasRedo() {
	return future.length > 0;
}
function isAbsolutePathLike(path) {
	return path.startsWith("/") || /^[A-Za-z]:[\\/]/.test(path) || path.startsWith("\\\\");
}
function stripTrailingSeparators(path) {
	if (path === "/" || /^[A-Za-z]:[\\/]?$/.test(path)) return normalizeRuntimePathSeparators(path);
	return isWindowsAbsolutePathLike(path) ? normalizeRuntimePathSeparators(path).replace(/\/+$/, "") : path.replace(/\/+$/, "");
}
function foldSeparatorsForFlavor(fragment, flavorSource) {
	return isWindowsAbsolutePathLike(flavorSource) ? normalizeRuntimePathSeparators(fragment) : fragment;
}
function deriveRelativeRootFromOpenFile(filePath, relativePath) {
	const normalizedFilePath = stripTrailingSeparators(filePath);
	const normalizedRelativePath = foldSeparatorsForFlavor(relativePath, filePath).replace(/^\/+/, "");
	if (!normalizedRelativePath || isAbsolutePathLike(relativePath)) {
		const separatorIndex$1 = normalizedFilePath.lastIndexOf("/");
		return separatorIndex$1 <= 0 ? "/" : normalizedFilePath.slice(0, separatorIndex$1);
	}
	const suffix = `/${normalizedRelativePath}`;
	if (normalizedFilePath.endsWith(suffix)) return stripTrailingSeparators(normalizedFilePath.slice(0, -suffix.length) || "/");
	const base = basename(normalizedFilePath);
	if (base && normalizedRelativePath === base) {
		const separatorIndex$1 = normalizedFilePath.lastIndexOf("/");
		return separatorIndex$1 <= 0 ? "/" : normalizedFilePath.slice(0, separatorIndex$1);
	}
	const separatorIndex = normalizedFilePath.lastIndexOf("/");
	return separatorIndex <= 0 ? "/" : normalizedFilePath.slice(0, separatorIndex);
}
function splitAbsolutePath(path) {
	const normalized = stripTrailingSeparators(path);
	const driveMatch = /^([A-Za-z]:)(?:\/(.*))?$/.exec(normalized);
	if (driveMatch) return {
		prefix: driveMatch[1].toLowerCase(),
		segments: (driveMatch[2] ?? "").split("/").filter(Boolean)
	};
	if (normalized.startsWith("//")) {
		const segments = normalized.slice(2).split("/").filter(Boolean);
		const server = (segments[0] ?? "").toLowerCase();
		return {
			prefix: server === "wsl.localhost" || server === "wsl$" ? `//wsl/${(segments[1] ?? "").toLowerCase()}` : `//${segments.slice(0, 2).join("/").toLowerCase()}`,
			segments: segments.slice(2)
		};
	}
	if (normalized.startsWith("/")) return {
		prefix: "/",
		segments: normalized.slice(1).split("/").filter(Boolean)
	};
	return {
		prefix: "",
		segments: normalized.split("/").filter(Boolean)
	};
}
function computeMovedPath(fromPath, toPath, filePath) {
	const suffix = relativePathInsideRoot(fromPath, filePath);
	if (suffix === null) return toPath + filePath.slice(fromPath.length);
	if (suffix === "") return toPath;
	if (isWindowsAbsolutePathLike(toPath)) {
		const separator = toPath.includes("\\") ? "\\" : "/";
		return `${toPath.replace(/[\\/]+$/, "")}${separator}${suffix.split("/").join(separator)}`;
	}
	return `${toPath.replace(/\/+$/, "")}/${suffix}`;
}
function getRelativePathFromRoot(rootPath, candidatePath) {
	const insideRoot = relativePathInsideRoot(rootPath, candidatePath);
	if (insideRoot !== null) return insideRoot;
	const root = splitAbsolutePath(rootPath);
	const candidate = splitAbsolutePath(candidatePath);
	if (root.prefix !== candidate.prefix) return foldSeparatorsForFlavor(candidatePath, candidatePath);
	const foldCase = /^[a-z]:$/i.test(root.prefix) || root.prefix.startsWith("//") && !root.prefix.startsWith("//wsl/");
	const sameSegment = (a, b) => foldCase ? a.toLowerCase() === b.toLowerCase() : a === b;
	let commonSegmentCount = 0;
	while (commonSegmentCount < root.segments.length && commonSegmentCount < candidate.segments.length && sameSegment(root.segments[commonSegmentCount], candidate.segments[commonSegmentCount])) commonSegmentCount += 1;
	return [...Array.from({ length: root.segments.length - commonSegmentCount }, () => ".."), ...candidate.segments.slice(commonSegmentCount)].join("/");
}
function getUpdatedRelativePath({ filePath, relativePath, worktreeId, updatedPath, initiatingWorktreeId, initiatingWorktreePath }) {
	const worktreeRelative = relativePathInsideRoot(initiatingWorktreePath, filePath);
	const normalizedRelativePath = foldSeparatorsForFlavor(relativePath, filePath).replace(/^\/+/, "");
	return getRelativePathFromRoot((initiatingWorktreeId !== void 0 ? worktreeId === initiatingWorktreeId : worktreeId !== "global-floating-terminal" && worktreeRelative !== null && foldSeparatorsForFlavor(worktreeRelative, filePath) === normalizedRelativePath) ? initiatingWorktreePath : deriveRelativeRootFromOpenFile(filePath, relativePath), updatedPath);
}
function remapOpenEditorTabsForPathChange({ fromPath, toPath, worktreePath, worktreeId, moveOperationId }) {
	const state = useAppStore.getState();
	const initiatingHostId = getExecutionHostIdForWorktree(state, worktreeId);
	const filesToMove = state.openFiles.filter((file) => isPathInsideOrEqual(fromPath, file.filePath) && getExecutionHostIdForWorktree(state, file.worktreeId) === initiatingHostId);
	if (filesToMove.length === 0) return { ok: true };
	const updatedPathOf = (file) => computeMovedPath(fromPath, toPath, file.filePath);
	const relativeOf = (file) => getUpdatedRelativePath({
		filePath: file.filePath,
		relativePath: file.relativePath,
		worktreeId: file.worktreeId,
		updatedPath: updatedPathOf(file),
		initiatingWorktreeId: worktreeId,
		initiatingWorktreePath: worktreePath
	});
	const ownerKeyOf = (file) => `${file.worktreeId}::${file.runtimeEnvironmentId?.trim() || ""}`;
	const plainPathOwner = /* @__PURE__ */ new Map();
	const reservedSourceId = (file) => {
		const updatedPath = updatedPathOf(file);
		const ownerKey = ownerKeyOf(file);
		const claimed = plainPathOwner.get(updatedPath);
		if (claimed === ownerKey) return updatedPath;
		if (claimed !== void 0) return buildOwnedEditorFileId(updatedPath, file.worktreeId, file.runtimeEnvironmentId);
		const id = resolveEditorFileIdForOwner(state, updatedPath, file.worktreeId, file.runtimeEnvironmentId, ["edit"]);
		if (id === updatedPath) plainPathOwner.set(updatedPath, ownerKey);
		return id;
	};
	const rekeys = [];
	const newEditIdByOldId = /* @__PURE__ */ new Map();
	for (const file of filesToMove) {
		if (file.mode !== "edit") continue;
		const newId = reservedSourceId(file);
		newEditIdByOldId.set(file.id, newId);
		rekeys.push({
			oldFileId: file.id,
			newFileId: newId,
			oldFilePath: file.filePath,
			newFilePath: updatedPathOf(file),
			newRelativePath: relativeOf(file),
			newLanguage: detectLanguage(basename(updatedPathOf(file))),
			consumeUntitled: file.isUntitled === true && file.filePath === fromPath
		});
	}
	for (const file of filesToMove) {
		if (file.mode !== "markdown-preview") continue;
		const newSourceFileId = (file.markdownPreviewSourceFileId ? newEditIdByOldId.get(file.markdownPreviewSourceFileId) : void 0) ?? reservedSourceId(file);
		rekeys.push({
			oldFileId: file.id,
			newFileId: `markdown-preview::${newSourceFileId}`,
			oldFilePath: file.filePath,
			newFilePath: updatedPathOf(file),
			newRelativePath: relativeOf(file),
			newMarkdownPreviewSourceFileId: newSourceFileId
		});
	}
	for (const file of filesToMove) {
		if (file.mode !== "diff" || file.diffSource !== "staged" && file.diffSource !== "unstaged") continue;
		const newRelativePath = relativeOf(file);
		rekeys.push({
			oldFileId: file.id,
			newFileId: buildDiffEditorFileId(file.worktreeId, file.diffSource, newRelativePath, file.runtimeEnvironmentId),
			oldFilePath: file.filePath,
			newFilePath: updatedPathOf(file),
			newRelativePath
		});
	}
	if (rekeys.length === 0) return { ok: true };
	return useAppStore.getState().rekeyOpenFilesForPathChange({
		rekeys,
		moveOperationId
	});
}
var moveOperationCounter = 0;
async function executeOpenEditorPathMove(args) {
	const { context, fromPath, toPath, worktreeId, worktreePath } = args;
	const operationId = `editor-move-${moveOperationCounter += 1}`;
	const moveState = useAppStore.getState();
	const initiatingHostId = getExecutionHostIdForWorktree(moveState, worktreeId);
	const affected = moveState.openFiles.filter((f) => isPathInsideOrEqual(fromPath, f.filePath) && getExecutionHostIdForWorktree(moveState, f.worktreeId) === initiatingHostId);
	const ownerSubOps = [];
	const scopes = /* @__PURE__ */ new Map();
	const addScope = (wtId, owner) => {
		scopes.set(`${wtId}::${owner ?? "local"}`, {
			worktreeId: wtId,
			owner
		});
	};
	addScope(worktreeId, getRuntimeEnvironmentIdForWorktree(moveState, worktreeId));
	for (const f of affected) addScope(f.worktreeId, f.runtimeEnvironmentId?.trim() || null);
	for (const [key$1, scope] of scopes) {
		const subOperationId = `${operationId}::${key$1}`;
		ownerSubOps.push(subOperationId);
		beginEditorPathMove({
			operationId: subOperationId,
			worktreeId: scope.worktreeId,
			runtimeEnvironmentId: scope.owner,
			sourcePaths: [fromPath]
		});
	}
	await Promise.all(affected.map((f) => requestEditorSaveQuiesce({ fileId: f.id })));
	try {
		await renameRuntimePath(context, fromPath, toPath);
	} catch (err) {
		for (const subOperationId of ownerSubOps) settleEditorPathMove(subOperationId);
		throw err;
	}
	try {
		const mirrorState = useAppStore.getState();
		const mirroredAffected = affected.filter((f) => f.mirroredFromRuntimeSession);
		const rekeyResult = remapOpenEditorTabsForPathChange({
			fromPath,
			toPath,
			worktreePath,
			worktreeId,
			moveOperationId: operationId
		});
		if (!rekeyResult.ok) {
			let rollbackError;
			try {
				await renameRuntimePath(context, toPath, fromPath);
			} catch (err) {
				rollbackError = err;
			}
			const base = `Could not retarget open editors for the move (${rekeyResult.reason}).`;
			throw new Error(rollbackError ? `${base} The on-disk move could not be undone and the file may remain at the new path: ${rollbackError instanceof Error ? rollbackError.message : String(rollbackError)}` : base);
		}
		for (const file of mirroredAffected) notifyHostOfMirroredEditorClose(mirrorState, file.worktreeId, file.id);
	} finally {
		for (const subOperationId of ownerSubOps) settleEditorPathMove(subOperationId);
	}
	const gatedTabIds = useAppStore.getState().openFiles.filter((f) => f.pendingSelfMoveEcho?.operationId === operationId).map((f) => f.id);
	if (gatedTabIds.length > 0) verifyLatchedMoveDestinations(worktreePath, context.connectionId, gatedTabIds);
}
function extractIpcErrorMessage(err, fallback) {
	if (!(err instanceof Error)) return fallback;
	const match = err.message.match(/Error invoking remote method '[^']*': (?:Error: )?(.+)/);
	return match ? match[1] : err.message;
}
async function renameFileOnDisk(args) {
	const { oldPath, newName, worktreeId, worktreePath, refreshDir } = args;
	const trimmed = newName.trim();
	if (!trimmed) return;
	const existingName = basename(oldPath);
	if (trimmed === existingName) return;
	const parentDir = dirname(oldPath);
	const newPath = joinPath(parentDir, trimmed);
	const operationGuard = captureFileExplorerOperationGuard(worktreeId, args.operationOwner ?? getFileExplorerOperationOwner(worktreeId));
	const operationRoute = operationGuard.route;
	const fileContext = {
		settings: operationRoute.settings,
		worktreeId,
		worktreePath,
		connectionId: operationRoute.connectionId,
		expectedExecutionHostId: operationRoute.expectedExecutionHostId,
		expectedSshTargetId: operationRoute.expectedSshTargetId,
		expectedSshConnectionGeneration: operationRoute.expectedSshConnectionGeneration
	};
	try {
		operationGuard.assertCurrent();
		await executeOpenEditorPathMove({
			context: fileContext,
			fromPath: oldPath,
			toPath: newPath,
			worktreeId,
			worktreePath
		});
		commitFileExplorerOp({
			undo: async () => {
				operationGuard.assertCurrent();
				await executeOpenEditorPathMove({
					context: fileContext,
					fromPath: newPath,
					toPath: oldPath,
					worktreeId,
					worktreePath
				});
				if (refreshDir) await refreshDir(parentDir);
			},
			redo: async () => {
				operationGuard.assertCurrent();
				await executeOpenEditorPathMove({
					context: fileContext,
					fromPath: oldPath,
					toPath: newPath,
					worktreeId,
					worktreePath
				});
				if (refreshDir) await refreshDir(parentDir);
			}
		});
	} catch (err) {
		toast.error(extractIpcErrorMessage(err, `Failed to rename '${existingName}'.`));
	}
	if (refreshDir) await refreshDir(parentDir);
}
export { useDroppable as A, getDropIndicatorClasses as C, useSortable as D, SortableContext as E, ListX as M, Columns2 as N, DndContext as O, ACTIVE_TAB_INDICATOR_CLASSES as S, getTabStripBorderClasses as T, TAB_CONTAINER_WIDTH_CLASSES as _, commitFileExplorerOp as a, TAB_CONTEXT_MENU_CONTENT_CLASS as b, redoFileExplorer as c, SortableTab as d, useTabStripPointerActivation as f, resolveTabIndicatorEdges as g, resolveDropZone as h, clearFileExplorerUndoHistory as i, isTabStripPointerGestureActive as j, DragOverlay as k, undoFileExplorer as l, useTabDragSplit as m, renameFileOnDisk as n, fileExplorerHasRedo as o, getTabPaneBodyDroppableId as p, executeOpenEditorPathMove as r, fileExplorerHasUndo as s, extractIpcErrorMessage as t, CLOSE_ALL_CONTEXT_MENUS_EVENT as u, TAB_LABEL_WIDTH_CLASSES as v, getTabRootStateClasses as w, preventMiddleButtonDefault as x, TabWorkspaceLayoutMenuSection as y };
