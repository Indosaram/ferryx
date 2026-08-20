import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn } from "./button-DszXJEV6.js";
import { t as ChevronRight } from "./chevron-right-CZtMe6Ev.js";
import { t as Circle } from "./circle-DumnR8X3.js";
import { t as Primitive } from "./dist-DW1EJH6e.js";
import { t as createContextScope } from "./dist-BvH-oDES.js";
import { _ as createMenuScope, a as Group, c as Label, d as RadioItem, f as Root3, g as SubTrigger, h as SubContent, i as Content2$1, l as Portal, m as Sub, n as Arrow2, o as Item2$1, p as Separator, r as CheckboxItem, s as ItemIndicator, t as Anchor2, u as RadioGroup } from "./dist-Ca8cIakR.js";
import { r as composeEventHandlers, t as useControllableState } from "./dist-CUdeCwrc.js";
var import_react = /* @__PURE__ */ __toESM(require_react(), 1);
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime(), 1);
var CONTEXT_MENU_NAME = "ContextMenu";
var [createContextMenuContext, createContextMenuScope] = createContextScope(CONTEXT_MENU_NAME, [createMenuScope]);
var useMenuScope = createMenuScope();
var [ContextMenuProvider, useContextMenuContext] = createContextMenuContext(CONTEXT_MENU_NAME);
var ContextMenu$1 = (props) => {
	const { __scopeContextMenu, children, onOpenChange, open: openProp, dir, modal = true } = props;
	const hasInteractedRef = import_react.useRef(false);
	{
		const hasWarnedRef = import_react.useRef(false);
		import_react.useEffect(() => {
			if (openProp === true && !hasInteractedRef.current && !hasWarnedRef.current) {
				hasWarnedRef.current = true;
				console.warn("ContextMenu: The `open` prop has been set to `true` before the user has interacted with the trigger, so its position is indeterminate. This is likely unintended and will result in the menu being anchored to the top-left corner of the viewport.");
			}
		}, [openProp]);
	}
	const [open, setOpen] = useControllableState({
		prop: openProp,
		defaultProp: false,
		onChange: onOpenChange,
		caller: CONTEXT_MENU_NAME
	});
	const menuScope = useMenuScope(__scopeContextMenu);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ContextMenuProvider, {
		scope: __scopeContextMenu,
		open,
		onOpenChange: setOpen,
		modal,
		hasInteractedRef,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Root3, {
			...menuScope,
			dir,
			open,
			onOpenChange: setOpen,
			modal,
			children
		})
	});
};
ContextMenu$1.displayName = CONTEXT_MENU_NAME;
var TRIGGER_NAME = "ContextMenuTrigger";
var ContextMenuTrigger$1 = import_react.forwardRef((props, forwardedRef) => {
	const { __scopeContextMenu, disabled = false, ...triggerProps } = props;
	const context = useContextMenuContext(TRIGGER_NAME, __scopeContextMenu);
	const menuScope = useMenuScope(__scopeContextMenu);
	const [point, setPoint] = import_react.useState({
		x: 0,
		y: 0
	});
	const virtualRef = import_react.useMemo(() => ({ current: { getBoundingClientRect: () => DOMRect.fromRect({
		width: 0,
		height: 0,
		...point
	}) } }), [point]);
	const longPressTimerRef = import_react.useRef(0);
	const clearLongPress = import_react.useCallback(() => window.clearTimeout(longPressTimerRef.current), []);
	const handleOpen = (event) => {
		context.hasInteractedRef.current = true;
		setPoint({
			x: event.clientX,
			y: event.clientY
		});
		context.onOpenChange(true);
	};
	import_react.useEffect(() => clearLongPress, [clearLongPress]);
	import_react.useEffect(() => void (disabled && clearLongPress()), [disabled, clearLongPress]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Anchor2, {
		...menuScope,
		virtualRef
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Primitive.span, {
		"data-state": context.open ? "open" : "closed",
		"data-disabled": disabled ? "" : void 0,
		...triggerProps,
		ref: forwardedRef,
		style: {
			WebkitTouchCallout: "none",
			...props.style
		},
		onContextMenu: disabled ? props.onContextMenu : composeEventHandlers(props.onContextMenu, (event) => {
			clearLongPress();
			handleOpen(event);
			event.preventDefault();
		}),
		onPointerDown: disabled ? props.onPointerDown : composeEventHandlers(props.onPointerDown, whenTouchOrPen((event) => {
			clearLongPress();
			if (context.open) context.onOpenChange(false);
			longPressTimerRef.current = window.setTimeout(() => handleOpen(event), 700);
		})),
		onPointerMove: disabled ? props.onPointerMove : composeEventHandlers(props.onPointerMove, whenTouchOrPen(clearLongPress)),
		onPointerCancel: disabled ? props.onPointerCancel : composeEventHandlers(props.onPointerCancel, whenTouchOrPen(clearLongPress)),
		onPointerUp: disabled ? props.onPointerUp : composeEventHandlers(props.onPointerUp, whenTouchOrPen(clearLongPress))
	})] });
});
ContextMenuTrigger$1.displayName = TRIGGER_NAME;
var PORTAL_NAME = "ContextMenuPortal";
var ContextMenuPortal = (props) => {
	const { __scopeContextMenu, ...portalProps } = props;
	const menuScope = useMenuScope(__scopeContextMenu);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Portal, {
		...menuScope,
		...portalProps
	});
};
ContextMenuPortal.displayName = PORTAL_NAME;
var CONTENT_NAME = "ContextMenuContent";
var ContextMenuContent$1 = import_react.forwardRef((props, forwardedRef) => {
	const { __scopeContextMenu, ...contentProps } = props;
	const context = useContextMenuContext(CONTENT_NAME, __scopeContextMenu);
	const menuScope = useMenuScope(__scopeContextMenu);
	const hasInteractedOutsideRef = import_react.useRef(false);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Content2$1, {
		...menuScope,
		...contentProps,
		ref: forwardedRef,
		side: "right",
		sideOffset: 2,
		align: "start",
		onCloseAutoFocus: (event) => {
			props.onCloseAutoFocus?.(event);
			if (!event.defaultPrevented && hasInteractedOutsideRef.current) event.preventDefault();
			hasInteractedOutsideRef.current = false;
		},
		onInteractOutside: (event) => {
			props.onInteractOutside?.(event);
			if (!event.defaultPrevented && !context.modal) hasInteractedOutsideRef.current = true;
		},
		style: {
			...props.style,
			"--radix-context-menu-content-transform-origin": "var(--radix-popper-transform-origin)",
			"--radix-context-menu-content-available-width": "var(--radix-popper-available-width)",
			"--radix-context-menu-content-available-height": "var(--radix-popper-available-height)",
			"--radix-context-menu-trigger-width": "var(--radix-popper-anchor-width)",
			"--radix-context-menu-trigger-height": "var(--radix-popper-anchor-height)"
		}
	});
});
ContextMenuContent$1.displayName = CONTENT_NAME;
var GROUP_NAME = "ContextMenuGroup";
var ContextMenuGroup = import_react.forwardRef((props, forwardedRef) => {
	const { __scopeContextMenu, ...groupProps } = props;
	const menuScope = useMenuScope(__scopeContextMenu);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Group, {
		...menuScope,
		...groupProps,
		ref: forwardedRef
	});
});
ContextMenuGroup.displayName = GROUP_NAME;
var LABEL_NAME = "ContextMenuLabel";
var ContextMenuLabel$1 = import_react.forwardRef((props, forwardedRef) => {
	const { __scopeContextMenu, ...labelProps } = props;
	const menuScope = useMenuScope(__scopeContextMenu);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
		...menuScope,
		...labelProps,
		ref: forwardedRef
	});
});
ContextMenuLabel$1.displayName = LABEL_NAME;
var ITEM_NAME = "ContextMenuItem";
var ContextMenuItem$1 = import_react.forwardRef((props, forwardedRef) => {
	const { __scopeContextMenu, ...itemProps } = props;
	const menuScope = useMenuScope(__scopeContextMenu);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Item2$1, {
		...menuScope,
		...itemProps,
		ref: forwardedRef
	});
});
ContextMenuItem$1.displayName = ITEM_NAME;
var CHECKBOX_ITEM_NAME = "ContextMenuCheckboxItem";
var ContextMenuCheckboxItem = import_react.forwardRef((props, forwardedRef) => {
	const { __scopeContextMenu, ...checkboxItemProps } = props;
	const menuScope = useMenuScope(__scopeContextMenu);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CheckboxItem, {
		...menuScope,
		...checkboxItemProps,
		ref: forwardedRef
	});
});
ContextMenuCheckboxItem.displayName = CHECKBOX_ITEM_NAME;
var RADIO_GROUP_NAME = "ContextMenuRadioGroup";
var ContextMenuRadioGroup$1 = import_react.forwardRef((props, forwardedRef) => {
	const { __scopeContextMenu, ...radioGroupProps } = props;
	const menuScope = useMenuScope(__scopeContextMenu);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RadioGroup, {
		...menuScope,
		...radioGroupProps,
		ref: forwardedRef
	});
});
ContextMenuRadioGroup$1.displayName = RADIO_GROUP_NAME;
var RADIO_ITEM_NAME = "ContextMenuRadioItem";
var ContextMenuRadioItem$1 = import_react.forwardRef((props, forwardedRef) => {
	const { __scopeContextMenu, ...radioItemProps } = props;
	const menuScope = useMenuScope(__scopeContextMenu);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RadioItem, {
		...menuScope,
		...radioItemProps,
		ref: forwardedRef
	});
});
ContextMenuRadioItem$1.displayName = RADIO_ITEM_NAME;
var INDICATOR_NAME = "ContextMenuItemIndicator";
var ContextMenuItemIndicator = import_react.forwardRef((props, forwardedRef) => {
	const { __scopeContextMenu, ...itemIndicatorProps } = props;
	const menuScope = useMenuScope(__scopeContextMenu);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ItemIndicator, {
		...menuScope,
		...itemIndicatorProps,
		ref: forwardedRef
	});
});
ContextMenuItemIndicator.displayName = INDICATOR_NAME;
var SEPARATOR_NAME = "ContextMenuSeparator";
var ContextMenuSeparator$1 = import_react.forwardRef((props, forwardedRef) => {
	const { __scopeContextMenu, ...separatorProps } = props;
	const menuScope = useMenuScope(__scopeContextMenu);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Separator, {
		...menuScope,
		...separatorProps,
		ref: forwardedRef
	});
});
ContextMenuSeparator$1.displayName = SEPARATOR_NAME;
var ARROW_NAME = "ContextMenuArrow";
var ContextMenuArrow = import_react.forwardRef((props, forwardedRef) => {
	const { __scopeContextMenu, ...arrowProps } = props;
	const menuScope = useMenuScope(__scopeContextMenu);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Arrow2, {
		...menuScope,
		...arrowProps,
		ref: forwardedRef
	});
});
ContextMenuArrow.displayName = ARROW_NAME;
var SUB_NAME = "ContextMenuSub";
var ContextMenuSub$1 = (props) => {
	const { __scopeContextMenu, children, onOpenChange, open: openProp, defaultOpen } = props;
	const menuScope = useMenuScope(__scopeContextMenu);
	const [open, setOpen] = useControllableState({
		prop: openProp,
		defaultProp: defaultOpen ?? false,
		onChange: onOpenChange,
		caller: SUB_NAME
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Sub, {
		...menuScope,
		open,
		onOpenChange: setOpen,
		children
	});
};
ContextMenuSub$1.displayName = SUB_NAME;
var SUB_TRIGGER_NAME = "ContextMenuSubTrigger";
var ContextMenuSubTrigger$1 = import_react.forwardRef((props, forwardedRef) => {
	const { __scopeContextMenu, ...triggerItemProps } = props;
	const menuScope = useMenuScope(__scopeContextMenu);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SubTrigger, {
		...menuScope,
		...triggerItemProps,
		ref: forwardedRef
	});
});
ContextMenuSubTrigger$1.displayName = SUB_TRIGGER_NAME;
var SUB_CONTENT_NAME = "ContextMenuSubContent";
var ContextMenuSubContent$1 = import_react.forwardRef((props, forwardedRef) => {
	const { __scopeContextMenu, ...subContentProps } = props;
	const menuScope = useMenuScope(__scopeContextMenu);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SubContent, {
		...menuScope,
		...subContentProps,
		ref: forwardedRef,
		style: {
			...props.style,
			"--radix-context-menu-content-transform-origin": "var(--radix-popper-transform-origin)",
			"--radix-context-menu-content-available-width": "var(--radix-popper-available-width)",
			"--radix-context-menu-content-available-height": "var(--radix-popper-available-height)",
			"--radix-context-menu-trigger-width": "var(--radix-popper-anchor-width)",
			"--radix-context-menu-trigger-height": "var(--radix-popper-anchor-height)"
		}
	});
});
ContextMenuSubContent$1.displayName = SUB_CONTENT_NAME;
function whenTouchOrPen(handler) {
	return (event) => event.pointerType !== "mouse" ? handler(event) : void 0;
}
var Root2 = ContextMenu$1;
var Trigger = ContextMenuTrigger$1;
var Portal2 = ContextMenuPortal;
var Content2 = ContextMenuContent$1;
var Label2 = ContextMenuLabel$1;
var Item2 = ContextMenuItem$1;
var RadioGroup2 = ContextMenuRadioGroup$1;
var RadioItem2 = ContextMenuRadioItem$1;
var ItemIndicator2 = ContextMenuItemIndicator;
var Separator2 = ContextMenuSeparator$1;
var Sub2 = ContextMenuSub$1;
var SubTrigger2 = ContextMenuSubTrigger$1;
var SubContent2 = ContextMenuSubContent$1;
function ContextMenu({ ...props }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Root2, {
		"data-slot": "context-menu",
		modal: false,
		...props
	});
}
function ContextMenuTrigger({ ...props }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trigger, {
		"data-slot": "context-menu-trigger",
		...props
	});
}
function ContextMenuSub({ ...props }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Sub2, {
		"data-slot": "context-menu-sub",
		...props
	});
}
function ContextMenuRadioGroup({ ...props }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RadioGroup2, {
		"data-slot": "context-menu-radio-group",
		...props
	});
}
function ContextMenuSubTrigger({ className, inset, children, ...props }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(SubTrigger2, {
		"data-slot": "context-menu-sub-trigger",
		"data-inset": inset,
		className: cn("flex cursor-default items-center gap-2 rounded-[7px] px-2 py-1 text-[12px] leading-5 font-[450] outline-hidden select-none focus:bg-black/8 dark:focus:bg-white/14 focus:text-accent-foreground data-[inset]:pl-8 data-[state=open]:bg-black/8 dark:data-[state=open]:bg-white/14 data-[state=open]:text-accent-foreground [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-3.5 [&_svg:not([class*='text-'])]:text-muted-foreground", className),
		...props,
		children: [children, /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronRight, { className: "ml-auto" })]
	});
}
function ContextMenuSubContent({ className, style, ...props }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Portal2, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SubContent2, {
		"data-slot": "context-menu-sub-content",
		className: cn("z-[70] min-w-[11rem] origin-(--radix-context-menu-content-transform-origin) overflow-hidden rounded-[11px] border border-black/14 bg-[rgba(255,255,255,0.10)] p-1 text-popover-foreground shadow-[0_16px_36px_rgba(0,0,0,0.24),inset_0_1px_0_rgba(255,255,255,0.14)] backdrop-blur-2xl dark:border-white/14 dark:bg-[rgba(0,0,0,0.12)] dark:shadow-[0_20px_44px_rgba(0,0,0,0.42),inset_0_1px_0_rgba(255,255,255,0.04)] data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=closed]:zoom-out-95 data-[state=open]:animate-in data-[state=open]:fade-in-0 data-[state=open]:zoom-in-95", className),
		style: {
			...style,
			WebkitAppRegion: "no-drag"
		},
		...props
	}) });
}
function ContextMenuContent({ className, style, ...props }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Portal2, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Content2, {
		"data-slot": "context-menu-content",
		className: cn("z-[70] max-h-(--radix-context-menu-content-available-height) min-w-[11rem] origin-(--radix-context-menu-content-transform-origin) overflow-x-hidden overflow-y-auto scrollbar-sleek rounded-[11px] border border-black/14 bg-[rgba(255,255,255,0.10)] p-1 text-popover-foreground shadow-[0_16px_36px_rgba(0,0,0,0.24),inset_0_1px_0_rgba(255,255,255,0.14)] backdrop-blur-2xl dark:border-white/14 dark:bg-[rgba(0,0,0,0.12)] dark:shadow-[0_20px_44px_rgba(0,0,0,0.42),inset_0_1px_0_rgba(255,255,255,0.04)] data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=closed]:zoom-out-95 data-[state=open]:animate-in data-[state=open]:fade-in-0 data-[state=open]:zoom-in-95", className),
		style: {
			...style,
			WebkitAppRegion: "no-drag"
		},
		...props
	}) });
}
function ContextMenuItem({ className, inset, variant = "default", ...props }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Item2, {
		"data-slot": "context-menu-item",
		"data-inset": inset,
		"data-variant": variant,
		className: cn("relative flex cursor-default items-center gap-2 rounded-[7px] px-2 py-1 text-[12px] leading-5 font-[450] outline-hidden select-none focus:bg-black/8 dark:focus:bg-white/14 focus:text-accent-foreground data-[disabled]:pointer-events-none data-[disabled]:opacity-50 data-[inset]:pl-8 data-[variant=destructive]:text-destructive data-[variant=destructive]:focus:bg-destructive/10 data-[variant=destructive]:focus:text-destructive dark:data-[variant=destructive]:focus:bg-destructive/20 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-3.5 [&_svg:not([class*='text-'])]:text-muted-foreground data-[variant=destructive]:*:[svg]:text-destructive!", className),
		...props
	});
}
function ContextMenuRadioItem({ className, children, ...props }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(RadioItem2, {
		"data-slot": "context-menu-radio-item",
		className: cn("relative flex cursor-default items-center gap-2 rounded-[7px] py-1 pr-2 pl-8 text-[12px] leading-5 font-[450] outline-hidden select-none focus:bg-black/8 dark:focus:bg-white/14 focus:text-accent-foreground data-[disabled]:pointer-events-none data-[disabled]:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-3.5", className),
		...props,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "pointer-events-none absolute left-2 flex size-3.5 items-center justify-center",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ItemIndicator2, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Circle, { className: "size-2 fill-current" }) })
		}), children]
	});
}
function ContextMenuLabel({ className, inset, ...props }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label2, {
		"data-slot": "context-menu-label",
		"data-inset": inset,
		className: cn("px-2 py-1 text-[11px] font-semibold text-muted-foreground data-[inset]:pl-8", className),
		...props
	});
}
function ContextMenuSeparator({ className, ...props }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Separator2, {
		"data-slot": "context-menu-separator",
		className: cn("my-1 h-px bg-border/70", className),
		...props
	});
}
function ContextMenuShortcut({ className, ...props }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		"data-slot": "context-menu-shortcut",
		className: cn("ml-auto shrink-0 whitespace-nowrap text-[11px] tracking-normal text-muted-foreground/85", className),
		...props
	});
}
export { ContextMenuRadioGroup as a, ContextMenuShortcut as c, ContextMenuSubTrigger as d, ContextMenuTrigger as f, ContextMenuLabel as i, ContextMenuSub as l, ContextMenuContent as n, ContextMenuRadioItem as o, ContextMenuItem as r, ContextMenuSeparator as s, ContextMenu as t, ContextMenuSubContent as u };
