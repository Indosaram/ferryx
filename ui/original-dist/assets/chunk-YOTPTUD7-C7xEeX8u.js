import { C as createDefaultSharedCoreModule, S as createDefaultCoreModule, f as PieGrammarGeneratedModule, n as AbstractMermaidValueConverter, o as EmptyFileSystem, t as AbstractMermaidTokenBuilder, u as MermaidGeneratedSharedModule, w as inject, x as __name } from "./chunk-KEIR6QF5-XwhODICK.js";
var _Class, _Class2;
var PieTokenBuilder = (_Class = class extends AbstractMermaidTokenBuilder {
	constructor() {
		super(["pie", "showData"]);
	}
}, __name(_Class, "PieTokenBuilder"), _Class);
var PieValueConverter = (_Class2 = class extends AbstractMermaidValueConverter {
	runCustomConverter(rule, input, _cstNode) {
		if (rule.name !== "PIE_SECTION_LABEL") return;
		return input.replace(/"/g, "").trim();
	}
}, __name(_Class2, "PieValueConverter"), _Class2);
var PieModule = { parser: {
	TokenBuilder: /* @__PURE__ */ __name(() => new PieTokenBuilder(), "TokenBuilder"),
	ValueConverter: /* @__PURE__ */ __name(() => new PieValueConverter(), "ValueConverter")
} };
function createPieServices(context = EmptyFileSystem) {
	const shared = inject(createDefaultSharedCoreModule(context), MermaidGeneratedSharedModule);
	const Pie = inject(createDefaultCoreModule({ shared }), PieGrammarGeneratedModule, PieModule);
	shared.ServiceRegistry.register(Pie);
	return {
		shared,
		Pie
	};
}
__name(createPieServices, "createPieServices");
export { createPieServices as n, PieModule as t };
