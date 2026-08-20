import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as Copy } from "./copy-jk2iqVkp.js";
import { t as require_react_dom } from "./react-dom-Da8MQai-.js";
import { t as addViewportSizeChangeListener } from "./viewport-size-change-listener-zuLoM3r3.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_react_dom = require_react_dom();
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var MENU_WIDTH = 144;
var MENU_HEIGHT = 36;
var MENU_MARGIN = 8;
function getSelectionTextInside(container) {
	const selection = window.getSelection();
	if (!selection || selection.rangeCount === 0) return "";
	const anchorNode = selection.anchorNode;
	const focusNode = selection.focusNode;
	if (!anchorNode || !focusNode) return "";
	if (!container.contains(anchorNode) || !container.contains(focusNode)) return "";
	return selection.toString().trim();
}
function SelectedTextCopyMenu({ children, className }) {
	const [menu, setMenu] = import_react.useState(null);
	import_react.useEffect(() => {
		if (!menu) return;
		const close = () => setMenu(null);
		const handleKeyDown = (event) => {
			if (event.key === "Escape") close();
		};
		window.addEventListener("pointerdown", close);
		window.addEventListener("keydown", handleKeyDown, true);
		window.addEventListener("scroll", close, true);
		const removeViewportListener = addViewportSizeChangeListener(close);
		return () => {
			window.removeEventListener("pointerdown", close);
			window.removeEventListener("keydown", handleKeyDown, true);
			window.removeEventListener("scroll", close, true);
			removeViewportListener();
		};
	}, [menu]);
	const handleContextMenu = import_react.useCallback((event) => {
		const selectedText = getSelectionTextInside(event.currentTarget);
		if (!selectedText) return;
		event.preventDefault();
		event.stopPropagation();
		event.nativeEvent.stopImmediatePropagation();
		setMenu({
			text: selectedText,
			x: Math.max(MENU_MARGIN, Math.min(event.clientX, window.innerWidth - MENU_WIDTH - MENU_MARGIN)),
			y: Math.max(MENU_MARGIN, Math.min(event.clientY, window.innerHeight - MENU_HEIGHT - MENU_MARGIN))
		});
	}, []);
	const handleCopy = import_react.useCallback(() => {
		if (!menu) return;
		window.api.ui.writeClipboardText(menu.text);
		setMenu(null);
	}, [menu]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className,
		onContextMenuCapture: handleContextMenu,
		children: [children, menu && (0, import_react_dom.createPortal)(/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "fixed z-[100] min-w-36 rounded-[11px] border border-black/14 bg-popover p-1 text-popover-foreground shadow-[0_16px_36px_rgba(0,0,0,0.24),inset_0_1px_0_rgba(255,255,255,0.14)] dark:border-white/14 dark:shadow-[0_20px_44px_rgba(0,0,0,0.42),inset_0_1px_0_rgba(255,255,255,0.04)]",
			style: {
				left: menu.x,
				top: menu.y
			},
			onPointerDown: (event) => event.stopPropagation(),
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
				type: "button",
				className: "flex w-full cursor-default items-center gap-2 rounded-[7px] px-2 py-1 text-left text-[12px] font-[450] leading-5 outline-hidden hover:bg-accent focus:bg-accent",
				onClick: handleCopy,
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Copy, { className: "size-3.5 text-muted-foreground" }), translate("auto.components.SelectedTextCopyMenu.9b40d7b018", "Copy")]
			})
		}), document.body)]
	});
}
export { SelectedTextCopyMenu as t };
