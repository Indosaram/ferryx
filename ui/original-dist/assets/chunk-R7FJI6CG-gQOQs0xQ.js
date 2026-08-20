import { C as createDefaultSharedCoreModule, S as createDefaultCoreModule, n as AbstractMermaidValueConverter, o as EmptyFileSystem, t as AbstractMermaidTokenBuilder, u as MermaidGeneratedSharedModule, w as inject, x as __name, y as TreemapGrammarGeneratedModule } from "./chunk-KEIR6QF5-XwhODICK.js";
var _Class, _Class2, _Class3;
var TreemapTokenBuilder = (_Class = class extends AbstractMermaidTokenBuilder {
	constructor() {
		super(["treemap"]);
	}
}, __name(_Class, "TreemapTokenBuilder"), _Class);
var classDefRegex = /classDef\s+([A-Z_a-z]\w+)(?:\s+([^\n\r;]*))?;?/;
var TreemapValueConverter = (_Class2 = class extends AbstractMermaidValueConverter {
	runCustomConverter(rule, input, _cstNode) {
		if (rule.name === "NUMBER2") return parseFloat(input.replace(/,/g, ""));
		else if (rule.name === "SEPARATOR") return input.substring(1, input.length - 1);
		else if (rule.name === "STRING2") return input.substring(1, input.length - 1);
		else if (rule.name === "INDENTATION") return input.length;
		else if (rule.name === "ClassDef") {
			if (typeof input !== "string") return input;
			const match = classDefRegex.exec(input);
			if (match) return {
				$type: "ClassDefStatement",
				className: match[1],
				styleText: match[2] || void 0
			};
		}
	}
}, __name(_Class2, "TreemapValueConverter"), _Class2);
function registerValidationChecks(services) {
	const validator = services.validation.TreemapValidator;
	const registry = services.validation.ValidationRegistry;
	if (registry) {
		const checks = { Treemap: validator.checkSingleRoot.bind(validator) };
		registry.register(checks, validator);
	}
}
__name(registerValidationChecks, "registerValidationChecks");
var TreemapValidator = (_Class3 = class {
	checkSingleRoot(doc, accept) {
		let rootNodeIndentation;
		for (const row of doc.TreemapRows) {
			if (!row.item) continue;
			if (rootNodeIndentation === void 0 && row.indent === void 0) rootNodeIndentation = 0;
			else if (row.indent === void 0) accept("error", "Multiple root nodes are not allowed in a treemap.", {
				node: row,
				property: "item"
			});
			else if (rootNodeIndentation !== void 0 && rootNodeIndentation >= parseInt(row.indent, 10)) accept("error", "Multiple root nodes are not allowed in a treemap.", {
				node: row,
				property: "item"
			});
		}
	}
}, __name(_Class3, "TreemapValidator"), _Class3);
var TreemapModule = {
	parser: {
		TokenBuilder: /* @__PURE__ */ __name(() => new TreemapTokenBuilder(), "TokenBuilder"),
		ValueConverter: /* @__PURE__ */ __name(() => new TreemapValueConverter(), "ValueConverter")
	},
	validation: { TreemapValidator: /* @__PURE__ */ __name(() => new TreemapValidator(), "TreemapValidator") }
};
function createTreemapServices(context = EmptyFileSystem) {
	const shared = inject(createDefaultSharedCoreModule(context), MermaidGeneratedSharedModule);
	const Treemap = inject(createDefaultCoreModule({ shared }), TreemapGrammarGeneratedModule, TreemapModule);
	shared.ServiceRegistry.register(Treemap);
	registerValidationChecks(Treemap);
	return {
		shared,
		Treemap
	};
}
__name(createTreemapServices, "createTreemapServices");
export { createTreemapServices as n, TreemapModule as t };
