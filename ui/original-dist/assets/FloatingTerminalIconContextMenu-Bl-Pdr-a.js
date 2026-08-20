import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { l as createLucideIcon } from "./button-DszXJEV6.js";
import { t as EyeOff } from "./eye-off-4tMqi6LV.js";
import { t as useAppStore } from "./store-CgXrfmaH.js";
import { i as DropdownMenuItem, l as DropdownMenuSeparator, m as DropdownMenuTrigger, r as DropdownMenuContent, t as DropdownMenu } from "./dropdown-menu-Dth6LPK-.js";
var PanelBottom = createLucideIcon("panel-bottom", [["rect", {
	width: "18",
	height: "18",
	x: "3",
	y: "3",
	rx: "2",
	key: "afitv7"
}], ["path", {
	d: "M3 15h18",
	key: "5xshup"
}]]);
var PanelTop = createLucideIcon("panel-top", [["rect", {
	width: "18",
	height: "18",
	x: "3",
	y: "3",
	rx: "2",
	key: "afitv7"
}], ["path", {
	d: "M3 9h18",
	key: "1pudct"
}]]);
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function FloatingTerminalIconContextMenu({ children, currentLocation, className, style }) {
	const updateSettings = useAppStore((s) => s.updateSettings);
	const [open, setOpen] = (0, import_react.useState)(false);
	const [menuPoint, setMenuPoint] = (0, import_react.useState)({
		x: 0,
		y: 0
	});
	const reopenFrameRef = (0, import_react.useRef)(null);
	const setTriggerRef = (0, import_react.useCallback)((node) => {
		if (node === null) {
			if (reopenFrameRef.current !== null) {
				window.cancelAnimationFrame(reopenFrameRef.current);
				reopenFrameRef.current = null;
			}
		}
	}, []);
	const moveAction = (0, import_react.useMemo)(() => {
		if (currentLocation === "floating-button") return {
			icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PanelBottom, { className: "size-3.5" }),
			label: translate("auto.components.floating.terminal.FloatingTerminalIconContextMenu.0ee79e0674", "Move to Status Bar"),
			location: "status-bar"
		};
		return {
			icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PanelTop, { className: "size-3.5" }),
			label: translate("auto.components.floating.terminal.FloatingTerminalIconContextMenu.763f5fa2c1", "Move to Floating Button"),
			location: "floating-button"
		};
	}, [currentLocation]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		ref: setTriggerRef,
		className,
		style,
		"data-floating-terminal-toggle": true,
		onContextMenuCapture: (event) => {
			event.preventDefault();
			event.stopPropagation();
			setMenuPoint({
				x: event.clientX,
				y: event.clientY
			});
			setOpen(false);
			if (reopenFrameRef.current !== null) window.cancelAnimationFrame(reopenFrameRef.current);
			reopenFrameRef.current = window.requestAnimationFrame(() => {
				reopenFrameRef.current = null;
				setOpen(true);
			});
		},
		onContextMenu: (event) => {
			event.preventDefault();
			event.stopPropagation();
		},
		children
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenu, {
		open,
		onOpenChange: setOpen,
		modal: false,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuTrigger, {
			asChild: true,
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				"aria-hidden": true,
				tabIndex: -1,
				className: "pointer-events-none fixed size-px opacity-0",
				style: {
					left: menuPoint.x,
					top: menuPoint.y
				}
			})
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuContent, {
			className: "w-52",
			sideOffset: 0,
			align: "start",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
					className: "whitespace-nowrap",
					onSelect: () => {
						updateSettings({ floatingTerminalTriggerLocation: moveAction.location });
					},
					children: [moveAction.icon, moveAction.label]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
					className: "whitespace-nowrap",
					onSelect: () => {
						useAppStore.getState().recordFeatureInteraction("floating-workspace-hidden");
						updateSettings({ floatingTerminalEnabled: false });
					},
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(EyeOff, { className: "size-3.5" }), translate("auto.components.floating.terminal.FloatingTerminalIconContextMenu.8e7d775287", "Hide Floating Workspace")]
				})
			]
		})]
	})] });
}
export { FloatingTerminalIconContextMenu as t };
