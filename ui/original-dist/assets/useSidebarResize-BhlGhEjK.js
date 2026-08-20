import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
function clampSidebarResizeWidth(width, minWidth, maxWidth) {
	return Math.min(maxWidth, Math.max(minWidth, width));
}
function getRenderedSidebarWidthCssValue(isOpen, width, renderedExtraWidth) {
	return isOpen ? `${width + renderedExtraWidth}px` : "0px";
}
function getNextSidebarResizeWidth({ clientX, startX, startWidth, deltaSign, minWidth, maxWidth }) {
	return clampSidebarResizeWidth(startWidth + (clientX - startX) * deltaSign, minWidth, maxWidth);
}
function useSidebarResize({ isOpen, width, minWidth, maxWidth, deltaSign, renderedExtraWidth = 0, setWidth, onDraftWidthChange }) {
	const containerRef = (0, import_react.useRef)(null);
	const isResizingRef = (0, import_react.useRef)(false);
	const startXRef = (0, import_react.useRef)(0);
	const startWidthRef = (0, import_react.useRef)(width);
	const draftWidthRef = (0, import_react.useRef)(width);
	const frameRef = (0, import_react.useRef)(null);
	const overlayRef = (0, import_react.useRef)(null);
	const [isResizing, setIsResizing] = (0, import_react.useState)(false);
	const removeDragOverlay = (0, import_react.useCallback)(() => {
		const overlay = overlayRef.current;
		if (overlay && overlay.parentNode) overlay.parentNode.removeChild(overlay);
		overlayRef.current = null;
	}, []);
	const resetDocumentStyles = (0, import_react.useCallback)(() => {
		document.body.style.cursor = "";
		document.body.style.userSelect = "";
		removeDragOverlay();
	}, [removeDragOverlay]);
	const applyRenderedWidth = (0, import_react.useCallback)((nextWidth) => {
		const container = containerRef.current;
		if (!container) return;
		container.style.width = getRenderedSidebarWidthCssValue(isOpen, nextWidth, renderedExtraWidth);
	}, [isOpen, renderedExtraWidth]);
	(0, import_react.useLayoutEffect)(() => {
		if (isResizingRef.current) return;
		draftWidthRef.current = width;
		applyRenderedWidth(width);
		onDraftWidthChange?.(width);
	}, [
		applyRenderedWidth,
		onDraftWidthChange,
		width
	]);
	const stopResize = (0, import_react.useCallback)(() => {
		if (!isResizingRef.current) return;
		isResizingRef.current = false;
		setIsResizing(false);
		if (frameRef.current !== null) {
			cancelAnimationFrame(frameRef.current);
			frameRef.current = null;
		}
		resetDocumentStyles();
		const finalWidth = draftWidthRef.current;
		applyRenderedWidth(finalWidth);
		onDraftWidthChange?.(finalWidth);
		if (finalWidth !== width) setWidth(finalWidth);
	}, [
		applyRenderedWidth,
		onDraftWidthChange,
		resetDocumentStyles,
		setWidth,
		width
	]);
	const handleMouseMove = (0, import_react.useCallback)((event) => {
		if (!isResizingRef.current) return;
		const nextWidth = getNextSidebarResizeWidth({
			clientX: event.clientX,
			startX: startXRef.current,
			startWidth: startWidthRef.current,
			deltaSign,
			minWidth,
			maxWidth
		});
		if (nextWidth === draftWidthRef.current) return;
		draftWidthRef.current = nextWidth;
		if (frameRef.current !== null) return;
		frameRef.current = window.requestAnimationFrame(() => {
			frameRef.current = null;
			applyRenderedWidth(draftWidthRef.current);
			onDraftWidthChange?.(draftWidthRef.current);
		});
	}, [
		applyRenderedWidth,
		deltaSign,
		maxWidth,
		minWidth,
		onDraftWidthChange
	]);
	(0, import_react.useEffect)(() => {
		window.addEventListener("mousemove", handleMouseMove);
		window.addEventListener("mouseup", stopResize);
		window.addEventListener("blur", stopResize);
		return () => {
			window.removeEventListener("mousemove", handleMouseMove);
			window.removeEventListener("mouseup", stopResize);
			window.removeEventListener("blur", stopResize);
			if (frameRef.current !== null) {
				cancelAnimationFrame(frameRef.current);
				frameRef.current = null;
			}
			isResizingRef.current = false;
			resetDocumentStyles();
		};
	}, [
		handleMouseMove,
		resetDocumentStyles,
		stopResize
	]);
	return {
		containerRef,
		isResizing,
		onResizeStart: (0, import_react.useCallback)((event) => {
			event.preventDefault();
			isResizingRef.current = true;
			setIsResizing(true);
			startXRef.current = event.clientX;
			startWidthRef.current = width;
			draftWidthRef.current = width;
			onDraftWidthChange?.(width);
			document.body.style.cursor = "col-resize";
			document.body.style.userSelect = "none";
			if (!overlayRef.current) {
				const overlay = document.createElement("div");
				overlay.style.position = "fixed";
				overlay.style.inset = "0";
				overlay.style.zIndex = "2147483647";
				overlay.style.cursor = "col-resize";
				overlay.style.background = "transparent";
				document.body.appendChild(overlay);
				overlayRef.current = overlay;
			}
		}, [onDraftWidthChange, width])
	};
}
export { useSidebarResize as t };
