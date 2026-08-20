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
import { n as createRailroadServices } from "./chunk-5TONJI2A-SKlxK3bm.js";
import "./chunk-5HE753X5-BURdYK09.js";
import "./chunk-U6XO7XAA-D3PEC3N4.js";
import "./chunk-JG7HCLWE-B91zDApt.js";
import "./chunk-CQNSW5MT-8iQCDHSN.js";
import "./chunk-R7FJI6CG-gQOQs0xQ.js";
import "./chunk-5FCAYU7R-BfEra_5g.js";
import { n as __name } from "./chunk-Y2CYZVJY-DUyCWguD.js";
import { m as log } from "./src-BFzutHoD.js";
import "./chunk-I66GZJ75-7VFc6IgS.js";
import "./chunk-3NCLNEKW-Cn4Yk-Ir.js";
import { n as getStyles, r as renderer, t as db } from "./chunk-6Q2QTUOP-Boy6TtvD.js";
import { t as populateCommonDb } from "./chunk-JWPE2WC7-Cm_QrJjD.js";
import { t as MermaidParseError } from "./mermaid-parser.core-CWzMSSqh.js";
var langiumParser = createRailroadServices().Railroad.parser.LangiumParser;
var transformExpression = /* @__PURE__ */ __name((expr) => {
	switch (expr.$type) {
		case "RailroadTerminalExpr": return {
			type: "terminal",
			value: expr.value
		};
		case "RailroadNonTerminalExpr": return {
			type: "nonterminal",
			name: expr.name
		};
		case "RailroadSpecialExpr": return {
			type: "special",
			text: expr.text
		};
		case "RailroadSequenceExpr": {
			const elements = expr.elements.map(transformExpression);
			return elements.length === 1 ? elements[0] : {
				type: "sequence",
				elements
			};
		}
		case "RailroadChoiceExpr": {
			const alternatives = expr.alternatives.map(transformExpression);
			return alternatives.length === 1 ? alternatives[0] : {
				type: "choice",
				alternatives
			};
		}
		case "RailroadOptionalExpr": return {
			type: "optional",
			element: transformExpression(expr.element)
		};
		case "RailroadOneOrMoreExpr": return {
			type: "repetition",
			element: transformExpression(expr.element),
			min: 1,
			max: Infinity
		};
		case "RailroadZeroOrMoreExpr": return {
			type: "repetition",
			element: transformExpression(expr.element),
			min: 0,
			max: Infinity
		};
		default: throw new Error(`Unsupported railroad expression: ${expr.$type}`);
	}
}, "transformExpression");
var transformRule = /* @__PURE__ */ __name((rule) => {
	return {
		name: rule.name,
		definition: transformExpression(rule.definition)
	};
}, "transformRule");
var populateDb = /* @__PURE__ */ __name((ast) => {
	populateCommonDb(ast, db);
	if (ast.title) db.setTitle(ast.title);
	ast.rules.map((rule) => db.addRule(transformRule(rule)));
}, "populateDb");
var diagram = {
	parser: {
		parse: /* @__PURE__ */ __name((input) => {
			db.clear();
			log.debug("[Railroad Parser] Starting Langium parse");
			const result = langiumParser.parse(input);
			if (result.lexerErrors.length > 0 || result.parserErrors.length > 0) throw new MermaidParseError(result);
			const ast = result.value;
			log.debug("[Railroad Parser] Parsed rules:", ast.rules.length);
			populateDb(ast);
			log.debug("[Railroad Parser] Parse complete");
		}, "parse"),
		parser: { yy: db }
	},
	db,
	renderer,
	styles: getStyles
};
export { diagram };
