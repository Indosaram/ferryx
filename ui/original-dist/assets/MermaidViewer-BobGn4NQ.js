import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import "./button-DszXJEV6.js";
import { t as useAppStore } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./useMountedRef-1omUd-IV.js";
import "./purify.es-C_rn83UJ.js";
import { t as MermaidBlock } from "./MermaidBlock-gW3wAx0A.js";
import { a as setWithLRU, i as scrollTopCache } from "./scroll-cache-B8ebRfkp.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function MermaidViewer({ content, filePath }) {
	const rootRef = (0, import_react.useRef)(null);
	const settings = useAppStore((s) => s.settings);
	const isDark = settings?.theme === "dark" || settings?.theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches;
	const scrollCacheKey = `${filePath}:mermaid-diagram`;
	(0, import_react.useLayoutEffect)(() => {
		const container = rootRef.current;
		if (!container) return;
		let throttleTimer = null;
		const onScroll = () => {
			if (throttleTimer !== null) clearTimeout(throttleTimer);
			throttleTimer = setTimeout(() => {
				setWithLRU(scrollTopCache, scrollCacheKey, container.scrollTop);
				throttleTimer = null;
			}, 150);
		};
		container.addEventListener("scroll", onScroll, { passive: true });
		return () => {
			if (container.scrollHeight > container.clientHeight || container.scrollTop > 0) setWithLRU(scrollTopCache, scrollCacheKey, container.scrollTop);
			if (throttleTimer !== null) clearTimeout(throttleTimer);
			container.removeEventListener("scroll", onScroll);
		};
	}, [scrollCacheKey]);
	(0, import_react.useLayoutEffect)(() => {
		const container = rootRef.current;
		const targetScrollTop = scrollTopCache.get(scrollCacheKey);
		if (!container || targetScrollTop === void 0) return;
		let frameId = 0;
		let attempts = 0;
		const tryRestore = () => {
			const maxScrollTop = Math.max(0, container.scrollHeight - container.clientHeight);
			container.scrollTop = Math.min(targetScrollTop, maxScrollTop);
			if (Math.abs(container.scrollTop - targetScrollTop) <= 1 || maxScrollTop >= targetScrollTop) return;
			attempts += 1;
			if (attempts < 30) frameId = window.requestAnimationFrame(tryRestore);
		};
		tryRestore();
		return () => window.cancelAnimationFrame(frameId);
	}, [scrollCacheKey, content]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		ref: rootRef,
		className: "mermaid-viewer h-full min-h-0 overflow-auto scrollbar-editor",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "mermaid-viewer-canvas",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MermaidBlock, {
				content: content.trim(),
				isDark,
				htmlLabels: false
			})
		})
	});
}
export { MermaidViewer as default };
