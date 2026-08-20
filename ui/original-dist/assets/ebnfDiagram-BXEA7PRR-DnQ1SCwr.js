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
import { n as createRailroadEbnfServices } from "./chunk-U6XO7XAA-D3PEC3N4.js";
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
var langiumParser = createRailroadEbnfServices().RailroadEbnf.parser.LangiumParser;
var transformChoice = /* @__PURE__ */ __name((choice) => {
	const alternatives = choice.alternatives.map(transformSequence);
	if (alternatives.length === 1) return alternatives[0];
	return {
		type: "choice",
		alternatives
	};
}, "transformChoice");
var transformSequence = /* @__PURE__ */ __name((sequence) => {
	const elements = sequence.elements.map(transformTerm);
	if (elements.length === 1) return elements[0];
	return {
		type: "sequence",
		elements
	};
}, "transformSequence");
var transformPrimary = /* @__PURE__ */ __name((primary) => {
	switch (primary.$type) {
		case "EbnfTerminal": return {
			type: "terminal",
			value: primary.value
		};
		case "EbnfNonTerminal": return {
			type: "nonterminal",
			name: primary.name
		};
		case "EbnfSpecial": return {
			type: "special",
			text: primary.text
		};
		case "EbnfGroup": return transformChoice(primary.element);
		case "EbnfOptional": return {
			type: "optional",
			element: transformChoice(primary.element)
		};
		case "EbnfRepetition": return {
			type: "repetition",
			element: transformChoice(primary.element),
			min: 0,
			max: Infinity
		};
		default: throw new Error(`Unsupported EBNF primary node: ${primary.$type}`);
	}
}, "transformPrimary");
var transformPostfix = /* @__PURE__ */ __name((node, postfix) => {
	switch (postfix.$type) {
		case "EbnfOptionalPostfix": return {
			type: "optional",
			element: node
		};
		case "EbnfZeroOrMorePostfix": return {
			type: "repetition",
			element: node,
			min: 0,
			max: Infinity
		};
		case "EbnfOneOrMorePostfix": return {
			type: "repetition",
			element: node,
			min: 1,
			max: Infinity
		};
		case "EbnfExceptionPostfix": return {
			type: "sequence",
			elements: [
				node,
				{
					type: "terminal",
					value: "-"
				},
				transformPrimary(postfix.except)
			]
		};
		default: throw new Error(`Unsupported EBNF postfix node: ${postfix.$type}`);
	}
}, "transformPostfix");
var transformTerm = /* @__PURE__ */ __name((term) => {
	return term.postfixes.reduce((currentNode, postfix) => {
		return transformPostfix(currentNode, postfix);
	}, transformPrimary(term.base));
}, "transformTerm");
var transformRule = /* @__PURE__ */ __name((rule) => {
	return {
		name: rule.name,
		definition: transformChoice(rule.definition)
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
			log.debug("[EBNF Parser] Starting Langium parse");
			const result = langiumParser.parse(input);
			if (result.lexerErrors.length > 0 || result.parserErrors.length > 0) throw new MermaidParseError(result);
			const ast = result.value;
			log.debug("[EBNF Parser] Parsed rules:", ast.rules.length);
			populateDb(ast);
			log.debug("[EBNF Parser] Parse complete");
		}, "parse"),
		parser: { yy: db }
	},
	db,
	renderer,
	styles: getStyles
};
export { diagram };
