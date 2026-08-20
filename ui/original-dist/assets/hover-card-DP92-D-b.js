import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn, s as useComposedRefs } from "./button-DszXJEV6.js";
import { t as Primitive } from "./dist-DW1EJH6e.js";
import { t as createContextScope } from "./dist-BvH-oDES.js";
import { r as composeEventHandlers, t as useControllableState } from "./dist-CUdeCwrc.js";
import { n as DismissableLayer, o as Presence, t as Portal } from "./dist-DGfr86jh.js";
import { a as createPopperScope, i as Root2$1, n as Arrow, r as Content, t as Anchor } from "./dist-BsNIAh1s.js";
var import_react = /* @__PURE__ */ __toESM(require_react(), 1);
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime(), 1);
var originalBodyUserSelect;
var HOVERCARD_NAME = "HoverCard";
var [createHoverCardContext, createHoverCardScope] = createContextScope(HOVERCARD_NAME, [createPopperScope]);
var usePopperScope = createPopperScope();
var [HoverCardProvider, useHoverCardContext] = createHoverCardContext(HOVERCARD_NAME);
var HoverCard$1 = (props) => {
	const { __scopeHoverCard, children, open: openProp, defaultOpen, onOpenChange, openDelay = 700, closeDelay = 300 } = props;
	const popperScope = usePopperScope(__scopeHoverCard);
	const openTimerRef = import_react.useRef(0);
	const closeTimerRef = import_react.useRef(0);
	const hasSelectionRef = import_react.useRef(false);
	const isPointerDownOnContentRef = import_react.useRef(false);
	const [open, setOpen] = useControllableState({
		prop: openProp,
		defaultProp: defaultOpen ?? false,
		onChange: onOpenChange,
		caller: HOVERCARD_NAME
	});
	const handleOpen = import_react.useCallback(() => {
		clearTimeout(closeTimerRef.current);
		openTimerRef.current = window.setTimeout(() => setOpen(true), openDelay);
	}, [openDelay, setOpen]);
	const handleClose = import_react.useCallback(() => {
		clearTimeout(openTimerRef.current);
		if (!hasSelectionRef.current && !isPointerDownOnContentRef.current) closeTimerRef.current = window.setTimeout(() => setOpen(false), closeDelay);
	}, [closeDelay, setOpen]);
	const handleDismiss = import_react.useCallback(() => setOpen(false), [setOpen]);
	import_react.useEffect(() => {
		return () => {
			clearTimeout(openTimerRef.current);
			clearTimeout(closeTimerRef.current);
		};
	}, []);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(HoverCardProvider, {
		scope: __scopeHoverCard,
		open,
		onOpenChange: setOpen,
		onOpen: handleOpen,
		onClose: handleClose,
		onDismiss: handleDismiss,
		hasSelectionRef,
		isPointerDownOnContentRef,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Root2$1, {
			...popperScope,
			children
		})
	});
};
HoverCard$1.displayName = HOVERCARD_NAME;
var TRIGGER_NAME = "HoverCardTrigger";
var HoverCardTrigger$1 = import_react.forwardRef((props, forwardedRef) => {
	const { __scopeHoverCard, ...triggerProps } = props;
	const context = useHoverCardContext(TRIGGER_NAME, __scopeHoverCard);
	const popperScope = usePopperScope(__scopeHoverCard);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Anchor, {
		asChild: true,
		...popperScope,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Primitive.a, {
			"data-state": context.open ? "open" : "closed",
			...triggerProps,
			ref: forwardedRef,
			onPointerEnter: composeEventHandlers(props.onPointerEnter, excludeTouch(context.onOpen)),
			onPointerLeave: composeEventHandlers(props.onPointerLeave, excludeTouch(context.onClose)),
			onFocus: composeEventHandlers(props.onFocus, context.onOpen),
			onBlur: composeEventHandlers(props.onBlur, context.onClose),
			onTouchStart: composeEventHandlers(props.onTouchStart, (event) => event.preventDefault())
		})
	});
});
HoverCardTrigger$1.displayName = TRIGGER_NAME;
var PORTAL_NAME = "HoverCardPortal";
var [PortalProvider, usePortalContext] = createHoverCardContext(PORTAL_NAME, { forceMount: void 0 });
var HoverCardPortal = (props) => {
	const { __scopeHoverCard, forceMount, children, container } = props;
	const context = useHoverCardContext(PORTAL_NAME, __scopeHoverCard);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PortalProvider, {
		scope: __scopeHoverCard,
		forceMount,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Presence, {
			present: forceMount || context.open,
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Portal, {
				asChild: true,
				container,
				children
			})
		})
	});
};
HoverCardPortal.displayName = PORTAL_NAME;
var CONTENT_NAME = "HoverCardContent";
var HoverCardContent$1 = import_react.forwardRef((props, forwardedRef) => {
	const portalContext = usePortalContext(CONTENT_NAME, props.__scopeHoverCard);
	const { forceMount = portalContext.forceMount, ...contentProps } = props;
	const context = useHoverCardContext(CONTENT_NAME, props.__scopeHoverCard);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Presence, {
		present: forceMount || context.open,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(HoverCardContentImpl, {
			"data-state": context.open ? "open" : "closed",
			...contentProps,
			onPointerEnter: composeEventHandlers(props.onPointerEnter, excludeTouch(context.onOpen)),
			onPointerLeave: composeEventHandlers(props.onPointerLeave, excludeTouch(context.onClose)),
			ref: forwardedRef
		})
	});
});
HoverCardContent$1.displayName = CONTENT_NAME;
var HoverCardContentImpl = import_react.forwardRef((props, forwardedRef) => {
	const { __scopeHoverCard, onEscapeKeyDown, onPointerDownOutside, onFocusOutside, onInteractOutside, ...contentProps } = props;
	const context = useHoverCardContext(CONTENT_NAME, __scopeHoverCard);
	const popperScope = usePopperScope(__scopeHoverCard);
	const ref = import_react.useRef(null);
	const composedRefs = useComposedRefs(forwardedRef, ref);
	const [containSelection, setContainSelection] = import_react.useState(false);
	import_react.useEffect(() => {
		if (containSelection) {
			const body = document.body;
			originalBodyUserSelect = body.style.userSelect || body.style.webkitUserSelect;
			body.style.userSelect = "none";
			body.style.webkitUserSelect = "none";
			return () => {
				body.style.userSelect = originalBodyUserSelect;
				body.style.webkitUserSelect = originalBodyUserSelect;
			};
		}
	}, [containSelection]);
	import_react.useEffect(() => {
		if (ref.current) {
			const handlePointerUp = () => {
				setContainSelection(false);
				context.isPointerDownOnContentRef.current = false;
				setTimeout(() => {
					if (document.getSelection()?.toString() !== "") context.hasSelectionRef.current = true;
				});
			};
			document.addEventListener("pointerup", handlePointerUp);
			return () => {
				document.removeEventListener("pointerup", handlePointerUp);
				context.hasSelectionRef.current = false;
				context.isPointerDownOnContentRef.current = false;
			};
		}
	}, [context.isPointerDownOnContentRef, context.hasSelectionRef]);
	import_react.useEffect(() => {
		if (ref.current) getTabbableNodes(ref.current).forEach((tabbable) => tabbable.setAttribute("tabindex", "-1"));
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DismissableLayer, {
		asChild: true,
		disableOutsidePointerEvents: false,
		onInteractOutside,
		onEscapeKeyDown,
		onPointerDownOutside,
		onFocusOutside: composeEventHandlers(onFocusOutside, (event) => {
			event.preventDefault();
		}),
		onDismiss: context.onDismiss,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Content, {
			...popperScope,
			...contentProps,
			onPointerDown: composeEventHandlers(contentProps.onPointerDown, (event) => {
				if (event.currentTarget.contains(event.target)) setContainSelection(true);
				context.hasSelectionRef.current = false;
				context.isPointerDownOnContentRef.current = true;
			}),
			ref: composedRefs,
			style: {
				...contentProps.style,
				userSelect: containSelection ? "text" : void 0,
				WebkitUserSelect: containSelection ? "text" : void 0,
				"--radix-hover-card-content-transform-origin": "var(--radix-popper-transform-origin)",
				"--radix-hover-card-content-available-width": "var(--radix-popper-available-width)",
				"--radix-hover-card-content-available-height": "var(--radix-popper-available-height)",
				"--radix-hover-card-trigger-width": "var(--radix-popper-anchor-width)",
				"--radix-hover-card-trigger-height": "var(--radix-popper-anchor-height)"
			}
		})
	});
});
var ARROW_NAME = "HoverCardArrow";
var HoverCardArrow = import_react.forwardRef((props, forwardedRef) => {
	const { __scopeHoverCard, ...arrowProps } = props;
	const popperScope = usePopperScope(__scopeHoverCard);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Arrow, {
		...popperScope,
		...arrowProps,
		ref: forwardedRef
	});
});
HoverCardArrow.displayName = ARROW_NAME;
function excludeTouch(eventHandler) {
	return (event) => event.pointerType === "touch" ? void 0 : eventHandler();
}
function getTabbableNodes(container) {
	const nodes = [];
	const walker = document.createTreeWalker(container, NodeFilter.SHOW_ELEMENT, { acceptNode: (node) => {
		return node.tabIndex >= 0 ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_SKIP;
	} });
	while (walker.nextNode()) nodes.push(walker.currentNode);
	return nodes;
}
var Root2 = HoverCard$1;
var Trigger = HoverCardTrigger$1;
var Portal$1 = HoverCardPortal;
var Content2 = HoverCardContent$1;
function HoverCard({ ...props }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Root2, {
		"data-slot": "hover-card",
		...props
	});
}
function HoverCardTrigger({ ...props }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trigger, {
		"data-slot": "hover-card-trigger",
		...props
	});
}
function HoverCardContent({ className, align = "center", sideOffset = 4, ...props }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Portal$1, {
		"data-slot": "hover-card-portal",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Content2, {
			"data-slot": "hover-card-content",
			align,
			sideOffset,
			className: cn("z-50 w-64 origin-(--radix-hover-card-content-transform-origin) rounded-md border border-black/14 bg-[rgba(255,255,255,0.82)] p-4 text-popover-foreground shadow-[0_16px_36px_rgba(0,0,0,0.24),inset_0_1px_0_rgba(255,255,255,0.14)] backdrop-blur-2xl outline-hidden dark:border-white/14 dark:bg-[rgba(0,0,0,0.72)] dark:shadow-[0_20px_44px_rgba(0,0,0,0.42),inset_0_1px_0_rgba(255,255,255,0.04)] data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=closed]:zoom-out-95 data-[state=open]:animate-in data-[state=open]:fade-in-0 data-[state=open]:zoom-in-95", className),
			...props
		})
	});
}
export { HoverCardContent as n, HoverCardTrigger as r, HoverCard as t };
