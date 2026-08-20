import "./purify.es-C_rn83UJ.js";
import "./chunk-KEIR6QF5-XwhODICK.js";
import "./chunk-MOZMSUNE-CcKYuWnm.js";
import "./chunk-OSBZ3O6U-CH3rGoG-.js";
import "./chunk-5JV3BV7I-1IAmjKDB.js";
import "./chunk-CYSBUYHQ-CB-FGyEu.js";
import "./chunk-BIQX33UG-CAkRtdoy.js";
import "./chunk-EMLP6XTP-BMukZhRX.js";
import "./chunk-YOTPTUD7-C7xEeX8u.js";
import "./chunk-QBLGF6JB-DnxjFx6i.js";
import "./chunk-5TONJI2A-SKlxK3bm.js";
import "./chunk-5HE753X5-BURdYK09.js";
import "./chunk-U6XO7XAA-D3PEC3N4.js";
import "./chunk-JG7HCLWE-B91zDApt.js";
import "./chunk-CQNSW5MT-8iQCDHSN.js";
import "./chunk-R7FJI6CG-gQOQs0xQ.js";
import "./chunk-5FCAYU7R-BfEra_5g.js";
import { n as __name } from "./chunk-Y2CYZVJY-DUyCWguD.js";
import { m as log } from "./src-BFzutHoD.js";
import { c as configureSvgSize } from "./chunk-I66GZJ75-7VFc6IgS.js";
import { t as selectSvgElement } from "./chunk-3NCLNEKW-Cn4Yk-Ir.js";
import { n as parse } from "./mermaid-parser.core-CWzMSSqh.js";
var parser = { parse: /* @__PURE__ */ __name(async (input) => {
	const ast = await parse("info", input);
	log.debug(ast);
}, "parse") };
var DEFAULT_INFO_DB = { version: "11.16.1" };
var diagram = {
	parser,
	db: { getVersion: /* @__PURE__ */ __name(() => DEFAULT_INFO_DB.version, "getVersion") },
	renderer: { draw: /* @__PURE__ */ __name((text, id, version) => {
		log.debug("rendering info diagram\n" + text);
		const svg = selectSvgElement(id);
		configureSvgSize(svg, 100, 400, true);
		svg.append("g").append("text").attr("x", 100).attr("y", 40).attr("class", "version").attr("font-size", 32).style("text-anchor", "middle").text(`v${version}`);
	}, "draw") }
};
export { diagram };
