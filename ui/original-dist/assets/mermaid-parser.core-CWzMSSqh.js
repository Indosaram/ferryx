const __vite__mapDeps=(i,m=__vite__mapDeps,d=(m.f||(m.f=["./info-DKCQHKI2-BLDIKpnb.js","./chunk-BIQX33UG-CAkRtdoy.js","./chunk-KEIR6QF5-XwhODICK.js","./defineProperty-BAtR-r70.js","./chunk-Dhmk_5SA.js","./packet-7NZHBO7P-CjkN4DOo.js","./chunk-EMLP6XTP-BMukZhRX.js","./pie-RZYD4A2V-Pp7SMXNe.js","./chunk-YOTPTUD7-C7xEeX8u.js","./treeView-QDETBFTQ-CJy2ZBwA.js","./chunk-CQNSW5MT-8iQCDHSN.js","./architecture-TIHT7OUA-C3XJQWbq.js","./chunk-MOZMSUNE-CcKYuWnm.js","./gitGraph-TEB2WS4Q-G-dHRMGD.js","./chunk-CYSBUYHQ-CB-FGyEu.js","./eventmodeling-45OFAUF4-DUkCgMO5.js","./chunk-5JV3BV7I-1IAmjKDB.js","./radar-I7S5WNFK-D6Khr1bh.js","./chunk-QBLGF6JB-DnxjFx6i.js","./railroad-3IZDKUUU-PxKSiRtM.js","./chunk-5TONJI2A-SKlxK3bm.js","./railroad-ebnf-EBAXGLYW-BjrjiGYC.js","./chunk-U6XO7XAA-D3PEC3N4.js","./railroad-abnf-AHOZXSZD-B7crEqJF.js","./chunk-5HE753X5-BURdYK09.js","./railroad-peg-LSFZ7HO6-Dlw5nrLc.js","./chunk-JG7HCLWE-B91zDApt.js","./treemap-6X3UGDF4-D54h8T-g.js","./chunk-R7FJI6CG-gQOQs0xQ.js","./wardley-OPB4EBWU-C2arOcxA.js","./chunk-5FCAYU7R-BfEra_5g.js","./cynefin-VYW2F7L2-BWyXSQ30.js","./chunk-OSBZ3O6U-CH3rGoG-.js"])))=>i.map(i=>d[i]);
import { t as __vitePreload } from "./preload-helper-Cgw39-ka.js";
import { x as __name } from "./chunk-KEIR6QF5-XwhODICK.js";
var _Class;
var parsers = {};
var initializers = {
	info: /* @__PURE__ */ __name(async () => {
		const { createInfoServices: createInfoServices2 } = await __vitePreload(async () => {
			const { createInfoServices: createInfoServices2$1 } = await import("./info-DKCQHKI2-BLDIKpnb.js");
			return { createInfoServices: createInfoServices2$1 };
		}, __vite__mapDeps([0,1,2,3,4]), import.meta.url);
		parsers.info = createInfoServices2().Info.parser.LangiumParser;
	}, "info"),
	packet: /* @__PURE__ */ __name(async () => {
		const { createPacketServices: createPacketServices2 } = await __vitePreload(async () => {
			const { createPacketServices: createPacketServices2$1 } = await import("./packet-7NZHBO7P-CjkN4DOo.js");
			return { createPacketServices: createPacketServices2$1 };
		}, __vite__mapDeps([5,6,2,3,4]), import.meta.url);
		parsers.packet = createPacketServices2().Packet.parser.LangiumParser;
	}, "packet"),
	pie: /* @__PURE__ */ __name(async () => {
		const { createPieServices: createPieServices2 } = await __vitePreload(async () => {
			const { createPieServices: createPieServices2$1 } = await import("./pie-RZYD4A2V-Pp7SMXNe.js");
			return { createPieServices: createPieServices2$1 };
		}, __vite__mapDeps([7,2,3,4,8]), import.meta.url);
		parsers.pie = createPieServices2().Pie.parser.LangiumParser;
	}, "pie"),
	treeView: /* @__PURE__ */ __name(async () => {
		const { createTreeViewServices: createTreeViewServices2 } = await __vitePreload(async () => {
			const { createTreeViewServices: createTreeViewServices2$1 } = await import("./treeView-QDETBFTQ-CJy2ZBwA.js");
			return { createTreeViewServices: createTreeViewServices2$1 };
		}, __vite__mapDeps([9,10,2,3,4]), import.meta.url);
		parsers.treeView = createTreeViewServices2().TreeView.parser.LangiumParser;
	}, "treeView"),
	architecture: /* @__PURE__ */ __name(async () => {
		const { createArchitectureServices: createArchitectureServices2 } = await __vitePreload(async () => {
			const { createArchitectureServices: createArchitectureServices2$1 } = await import("./architecture-TIHT7OUA-C3XJQWbq.js");
			return { createArchitectureServices: createArchitectureServices2$1 };
		}, __vite__mapDeps([11,2,3,4,12]), import.meta.url);
		parsers.architecture = createArchitectureServices2().Architecture.parser.LangiumParser;
	}, "architecture"),
	gitGraph: /* @__PURE__ */ __name(async () => {
		const { createGitGraphServices: createGitGraphServices2 } = await __vitePreload(async () => {
			const { createGitGraphServices: createGitGraphServices2$1 } = await import("./gitGraph-TEB2WS4Q-G-dHRMGD.js");
			return { createGitGraphServices: createGitGraphServices2$1 };
		}, __vite__mapDeps([13,14,2,3,4]), import.meta.url);
		parsers.gitGraph = createGitGraphServices2().GitGraph.parser.LangiumParser;
	}, "gitGraph"),
	eventmodeling: /* @__PURE__ */ __name(async () => {
		const { createEventModelingServices: createEventModelingServices2 } = await __vitePreload(async () => {
			const { createEventModelingServices: createEventModelingServices2$1 } = await import("./eventmodeling-45OFAUF4-DUkCgMO5.js");
			return { createEventModelingServices: createEventModelingServices2$1 };
		}, __vite__mapDeps([15,16,2,3,4]), import.meta.url);
		parsers.eventmodeling = createEventModelingServices2().EventModel.parser.LangiumParser;
	}, "eventmodeling"),
	radar: /* @__PURE__ */ __name(async () => {
		const { createRadarServices: createRadarServices2 } = await __vitePreload(async () => {
			const { createRadarServices: createRadarServices2$1 } = await import("./radar-I7S5WNFK-D6Khr1bh.js");
			return { createRadarServices: createRadarServices2$1 };
		}, __vite__mapDeps([17,2,3,4,18]), import.meta.url);
		parsers.radar = createRadarServices2().Radar.parser.LangiumParser;
	}, "radar"),
	railroad: /* @__PURE__ */ __name(async () => {
		const { createRailroadServices: createRailroadServices2 } = await __vitePreload(async () => {
			const { createRailroadServices: createRailroadServices2$1 } = await import("./railroad-3IZDKUUU-PxKSiRtM.js");
			return { createRailroadServices: createRailroadServices2$1 };
		}, __vite__mapDeps([19,20,2,3,4]), import.meta.url);
		parsers.railroad = createRailroadServices2().Railroad.parser.LangiumParser;
	}, "railroad"),
	railroadEbnf: /* @__PURE__ */ __name(async () => {
		const { createRailroadEbnfServices: createRailroadEbnfServices2 } = await __vitePreload(async () => {
			const { createRailroadEbnfServices: createRailroadEbnfServices2$1 } = await import("./railroad-ebnf-EBAXGLYW-BjrjiGYC.js");
			return { createRailroadEbnfServices: createRailroadEbnfServices2$1 };
		}, __vite__mapDeps([21,2,3,4,22]), import.meta.url);
		parsers.railroadEbnf = createRailroadEbnfServices2().RailroadEbnf.parser.LangiumParser;
	}, "railroadEbnf"),
	railroadAbnf: /* @__PURE__ */ __name(async () => {
		const { createRailroadAbnfServices: createRailroadAbnfServices2 } = await __vitePreload(async () => {
			const { createRailroadAbnfServices: createRailroadAbnfServices2$1 } = await import("./railroad-abnf-AHOZXSZD-B7crEqJF.js");
			return { createRailroadAbnfServices: createRailroadAbnfServices2$1 };
		}, __vite__mapDeps([23,24,2,3,4]), import.meta.url);
		parsers.railroadAbnf = createRailroadAbnfServices2().RailroadAbnf.parser.LangiumParser;
	}, "railroadAbnf"),
	railroadPeg: /* @__PURE__ */ __name(async () => {
		const { createRailroadPegServices: createRailroadPegServices2 } = await __vitePreload(async () => {
			const { createRailroadPegServices: createRailroadPegServices2$1 } = await import("./railroad-peg-LSFZ7HO6-Dlw5nrLc.js");
			return { createRailroadPegServices: createRailroadPegServices2$1 };
		}, __vite__mapDeps([25,26,2,3,4]), import.meta.url);
		parsers.railroadPeg = createRailroadPegServices2().RailroadPeg.parser.LangiumParser;
	}, "railroadPeg"),
	treemap: /* @__PURE__ */ __name(async () => {
		const { createTreemapServices: createTreemapServices2 } = await __vitePreload(async () => {
			const { createTreemapServices: createTreemapServices2$1 } = await import("./treemap-6X3UGDF4-D54h8T-g.js");
			return { createTreemapServices: createTreemapServices2$1 };
		}, __vite__mapDeps([27,2,3,4,28]), import.meta.url);
		parsers.treemap = createTreemapServices2().Treemap.parser.LangiumParser;
	}, "treemap"),
	wardley: /* @__PURE__ */ __name(async () => {
		const { createWardleyServices: createWardleyServices2 } = await __vitePreload(async () => {
			const { createWardleyServices: createWardleyServices2$1 } = await import("./wardley-OPB4EBWU-C2arOcxA.js");
			return { createWardleyServices: createWardleyServices2$1 };
		}, __vite__mapDeps([29,30,2,3,4]), import.meta.url);
		parsers.wardley = createWardleyServices2().Wardley.parser.LangiumParser;
	}, "wardley"),
	cynefin: /* @__PURE__ */ __name(async () => {
		const { createCynefinServices: createCynefinServices2 } = await __vitePreload(async () => {
			const { createCynefinServices: createCynefinServices2$1 } = await import("./cynefin-VYW2F7L2-BWyXSQ30.js");
			return { createCynefinServices: createCynefinServices2$1 };
		}, __vite__mapDeps([31,2,3,4,32]), import.meta.url);
		parsers.cynefin = createCynefinServices2().Cynefin.parser.LangiumParser;
	}, "cynefin")
};
async function parse(diagramType, text) {
	const initializer = initializers[diagramType];
	if (!initializer) throw new Error(`Unknown diagram type: ${diagramType}`);
	if (!parsers[diagramType]) await initializer();
	const result = parsers[diagramType].parse(text);
	if (result.lexerErrors.length > 0 || result.parserErrors.length > 0) throw new MermaidParseError(result);
	return result.value;
}
__name(parse, "parse");
var MermaidParseError = (_Class = class extends Error {
	constructor(result) {
		const lexerErrors = result.lexerErrors.map((err) => {
			return `Lexer error on line ${err.line !== void 0 && !isNaN(err.line) ? err.line : "?"}, column ${err.column !== void 0 && !isNaN(err.column) ? err.column : "?"}: ${err.message}`;
		}).join("\n");
		const parserErrors = result.parserErrors.map((err) => {
			return `Parse error on line ${err.token.startLine !== void 0 && !isNaN(err.token.startLine) ? err.token.startLine : "?"}, column ${err.token.startColumn !== void 0 && !isNaN(err.token.startColumn) ? err.token.startColumn : "?"}: ${err.message}`;
		}).join("\n");
		super(`Parsing failed: ${lexerErrors} ${parserErrors}`);
		this.result = result;
	}
}, __name(_Class, "MermaidParseError"), _Class);
export { parse as n, MermaidParseError as t };
