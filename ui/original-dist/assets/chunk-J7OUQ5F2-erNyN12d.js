const __vite__mapDeps=(i,m=__vite__mapDeps,d=(m.f||(m.f=["./dagre-VZM6K2ZE-BveKGrcr.js","./dist-D77b-jAv.js","./chunk-Dhmk_5SA.js","./chunk-4I5QYGJK-DWpuFaIR.js","./src-BFzutHoD.js","./chunk-Y2CYZVJY-DUyCWguD.js","./chunk-I66GZJ75-7VFc6IgS.js","./preload-helper-Cgw39-ka.js","./purify.es-C_rn83UJ.js","./chunk-NSK5VX7P-s-SUMTDp.js","./dagre-CQcq6Hr9.js","./graphlib-DH3qXvB3.js","./map-DNLvs7Lr.js","./chunk-RYQCIY6F-D7QjPh_9.js","./chunk-WRU74C26-DkG7okAh.js","./defineProperty-BAtR-r70.js","./chunk-7BUUIJ7U-5tpNhKKt.js","./chunk-7Z6QIM7H-iH_4N9Gq.js","./line-k9Db7Zfd.js","./path-qJns2Yva.js","./array-Bg23kMkO.js","./chunk-QR6OTTB3-2iLDSHdi.js","./chunk-UBXNYLIW-DgrN8r-y.js","./chunk-W5SLKNZC-Ccz5azL6.js","./rough.esm-BLKBhDjp.js","./swimlanes-SLNWSIFB-CecdxcUZ.js","./cose-bilkent-JH36ORCC-oz8nd_jj.js","./cytoscape.esm-FVPqsWpQ.js"])))=>i.map(i=>d[i]);
import { t as __vitePreload } from "./preload-helper-Cgw39-ka.js";
import { n as __name } from "./chunk-Y2CYZVJY-DUyCWguD.js";
import { m as log } from "./src-BFzutHoD.js";
import { b as getConfig, s as common_default } from "./chunk-I66GZJ75-7VFc6IgS.js";
import { f as interpolateToCurve } from "./chunk-NSK5VX7P-s-SUMTDp.js";
import { a as insertNode, i as insertCluster, s as labelHelper } from "./chunk-QR6OTTB3-2iLDSHdi.js";
import { a as markers_default, i as insertEdgeLabel, o as positionEdgeLabel, r as insertEdge } from "./chunk-7Z6QIM7H-iH_4N9Gq.js";
var internalHelpers = {
	common: common_default,
	getConfig,
	insertCluster,
	insertEdge,
	insertEdgeLabel,
	insertMarkers: markers_default,
	insertNode,
	interpolateToCurve,
	labelHelper,
	log,
	positionEdgeLabel
};
var layoutAlgorithms = {};
var registerLayoutLoaders = /* @__PURE__ */ __name((loaders) => {
	for (const loader of loaders) layoutAlgorithms[loader.name] = loader;
}, "registerLayoutLoaders");
(/* @__PURE__ */ __name(() => {
	registerLayoutLoaders([
		{
			name: "dagre",
			loader: /* @__PURE__ */ __name(async () => await __vitePreload(() => import("./dagre-VZM6K2ZE-BveKGrcr.js"), __vite__mapDeps([0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24]), import.meta.url), "loader")
		},
		{
			name: "swimlane",
			loader: /* @__PURE__ */ __name(async () => await __vitePreload(() => import("./swimlanes-SLNWSIFB-CecdxcUZ.js"), __vite__mapDeps([25,7,1,2,3,4,5,6,8,9,11,13,12,14,15,16,17,18,19,20,21,22,23,24]), import.meta.url), "loader")
		},
		...[{
			name: "cose-bilkent",
			loader: /* @__PURE__ */ __name(async () => await __vitePreload(() => import("./cose-bilkent-JH36ORCC-oz8nd_jj.js"), __vite__mapDeps([26,27,4,5,2]), import.meta.url), "loader")
		}]
	]);
}, "registerDefaultLayoutLoaders"))();
var render = /* @__PURE__ */ __name(async (data4Layout, svg, positions) => {
	if (!(data4Layout.layoutAlgorithm in layoutAlgorithms)) throw new Error(`Unknown layout algorithm: ${data4Layout.layoutAlgorithm}`);
	if (data4Layout.diagramId) for (const node of data4Layout.nodes) {
		const originalDomId = node.domId || node.id;
		node.domId = `${data4Layout.diagramId}-${originalDomId}`;
	}
	const layoutDefinition = layoutAlgorithms[data4Layout.layoutAlgorithm];
	const layoutRenderer = await layoutDefinition.loader();
	const { theme, themeVariables } = data4Layout.config;
	const { useGradient, gradientStart, gradientStop } = themeVariables;
	const svgId = svg.attr("id");
	svg.append("defs").append("filter").attr("id", `${svgId}-drop-shadow`).attr("height", "130%").attr("width", "130%").append("feDropShadow").attr("dx", "4").attr("dy", "4").attr("stdDeviation", 0).attr("flood-opacity", "0.06").attr("flood-color", `${theme?.includes("dark") ? "#FFFFFF" : "#000000"}`);
	svg.append("defs").append("filter").attr("id", `${svgId}-drop-shadow-small`).attr("height", "150%").attr("width", "150%").append("feDropShadow").attr("dx", "2").attr("dy", "2").attr("stdDeviation", 0).attr("flood-opacity", "0.06").attr("flood-color", `${theme?.includes("dark") ? "#FFFFFF" : "#000000"}`);
	if (useGradient) {
		const gradient = svg.append("linearGradient").attr("id", svg.attr("id") + "-gradient").attr("gradientUnits", "objectBoundingBox").attr("x1", "0%").attr("y1", "0%").attr("x2", "100%").attr("y2", "0%");
		gradient.append("svg:stop").attr("offset", "0%").attr("stop-color", gradientStart).attr("stop-opacity", 1);
		gradient.append("svg:stop").attr("offset", "100%").attr("stop-color", gradientStop).attr("stop-opacity", 1);
	}
	return layoutRenderer.render(data4Layout, svg, internalHelpers, { algorithm: layoutDefinition.algorithm }, positions);
}, "render");
var getRegisteredLayoutAlgorithm = /* @__PURE__ */ __name((algorithm = "", { fallback = "dagre" } = {}) => {
	if (algorithm in layoutAlgorithms) return algorithm;
	if (fallback in layoutAlgorithms) {
		log.warn(`Layout algorithm ${algorithm} is not registered. Using ${fallback} as fallback.`);
		return fallback;
	}
	throw new Error(`Both layout algorithms ${algorithm} and ${fallback} are not registered.`);
}, "getRegisteredLayoutAlgorithm");
export { registerLayoutLoaders as n, render as r, getRegisteredLayoutAlgorithm as t };
