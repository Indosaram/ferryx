const __vite__mapDeps=(i,m=__vite__mapDeps,d=(m.f||(m.f=["./mermaid.core-Ktcf33az.js","./preload-helper-Cgw39-ka.js","./dist-D77b-jAv.js","./chunk-Dhmk_5SA.js","./chunk-4I5QYGJK-DWpuFaIR.js","./src-BFzutHoD.js","./chunk-Y2CYZVJY-DUyCWguD.js","./chunk-I66GZJ75-7VFc6IgS.js","./purify.es-C_rn83UJ.js","./chunk-NSK5VX7P-s-SUMTDp.js","./chunk-WRU74C26-DkG7okAh.js","./defineProperty-BAtR-r70.js","./chunk-3NCLNEKW-Cn4Yk-Ir.js","./chunk-7BUUIJ7U-5tpNhKKt.js","./chunk-7Z6QIM7H-iH_4N9Gq.js","./line-k9Db7Zfd.js","./path-qJns2Yva.js","./array-Bg23kMkO.js","./chunk-QR6OTTB3-2iLDSHdi.js","./chunk-UBXNYLIW-DgrN8r-y.js","./chunk-W5SLKNZC-Ccz5azL6.js","./rough.esm-BLKBhDjp.js","./chunk-J7OUQ5F2-erNyN12d.js","./chunk-ZIRB5QZD-BkUKHWca.js"])))=>i.map(i=>d[i]);
import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as __vitePreload } from "./preload-helper-Cgw39-ka.js";
import { t as purify } from "./purify.es-C_rn83UJ.js";
function getMermaidConfig(isDark, htmlLabels = false) {
	return {
		startOnLoad: false,
		securityLevel: "strict",
		suppressErrorRendering: true,
		theme: isDark ? "dark" : "default",
		htmlLabels
	};
}
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var mermaidModulePromise = null;
function loadMermaid() {
	if (!mermaidModulePromise) mermaidModulePromise = __vitePreload(() => import("./mermaid.core-Ktcf33az.js"), __vite__mapDeps([0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23]), import.meta.url).then((mod) => mod.default);
	return mermaidModulePromise;
}
var renderQueue = Promise.resolve();
function enqueueRender(fn) {
	renderQueue = renderQueue.then(fn, fn).then(() => {
		renderQueue = Promise.resolve();
	});
}
function MermaidBlock({ content, isDark, htmlLabels = false }) {
	const id = (0, import_react.useId)().replace(/:/g, "_");
	const containerRef = (0, import_react.useRef)(null);
	const [error, setError] = (0, import_react.useState)(null);
	(0, import_react.useEffect)(() => {
		let cancelled = false;
		const render = async () => {
			try {
				const mermaid = await loadMermaid();
				if (cancelled) return;
				mermaid.initialize(getMermaidConfig(isDark, htmlLabels));
				const { svg } = await mermaid.render(`mermaid-${id}`, content);
				if (!cancelled && containerRef.current) {
					containerRef.current.innerHTML = purify.sanitize(svg, { USE_PROFILES: { svg: true } });
					setError(null);
				}
			} catch (err) {
				if (!cancelled) {
					setError(err instanceof Error ? err.message : "Invalid mermaid syntax");
					document.getElementById(`d${`mermaid-${id}`}`)?.remove();
				}
			}
		};
		enqueueRender(render);
		return () => {
			cancelled = true;
		};
	}, [
		content,
		htmlLabels,
		isDark,
		id
	]);
	if (error) return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "mermaid-block",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "mermaid-error",
			children: [
				translate("auto.components.editor.MermaidBlock.dcc132e691", "Diagram error:"),
				" ",
				error
			]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("pre", { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("code", { children: content }) })]
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "mermaid-block",
		ref: containerRef
	});
}
export { MermaidBlock as t };
