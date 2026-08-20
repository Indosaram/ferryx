import { C as createDefaultSharedCoreModule, S as createDefaultCoreModule, m as RailroadAbnfGrammarGeneratedModule, n as AbstractMermaidValueConverter, o as EmptyFileSystem, t as AbstractMermaidTokenBuilder, u as MermaidGeneratedSharedModule, w as inject, x as __name } from "./chunk-KEIR6QF5-XwhODICK.js";
var _Class, _Class2;
var RailroadAbnfTokenBuilder = (_Class = class extends AbstractMermaidTokenBuilder {
	constructor() {
		super(["railroad-abnf-beta"]);
	}
}, __name(_Class, "RailroadAbnfTokenBuilder"), _Class);
var RailroadAbnfValueConverter = (_Class2 = class extends AbstractMermaidValueConverter {
	runConverter(rule, input, cstNode) {
		const value = super.runConverter(rule, input, cstNode);
		if (rule.name === "TITLE" && typeof value === "string") {
			const trimmedValue = value.trim();
			if (trimmedValue.startsWith("\"") && trimmedValue.endsWith("\"") || trimmedValue.startsWith("'") && trimmedValue.endsWith("'")) return trimmedValue.slice(1, -1);
		}
		return value;
	}
	runCustomConverter(rule, input, _cstNode) {
		if (rule.name === "ABNF_STRING") return input.slice(1, -1);
	}
}, __name(_Class2, "RailroadAbnfValueConverter"), _Class2);
var RailroadAbnfModule = { parser: {
	TokenBuilder: /* @__PURE__ */ __name(() => new RailroadAbnfTokenBuilder(), "TokenBuilder"),
	ValueConverter: /* @__PURE__ */ __name(() => new RailroadAbnfValueConverter(), "ValueConverter")
} };
function createRailroadAbnfServices(context = EmptyFileSystem) {
	const shared = inject(createDefaultSharedCoreModule(context), MermaidGeneratedSharedModule);
	const RailroadAbnf = inject(createDefaultCoreModule({ shared }), RailroadAbnfGrammarGeneratedModule, RailroadAbnfModule);
	shared.ServiceRegistry.register(RailroadAbnf);
	return {
		shared,
		RailroadAbnf
	};
}
__name(createRailroadAbnfServices, "createRailroadAbnfServices");
export { createRailroadAbnfServices as n, RailroadAbnfModule as t };
