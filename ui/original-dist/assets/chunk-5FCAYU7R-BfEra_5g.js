import { C as createDefaultSharedCoreModule, S as createDefaultCoreModule, b as WardleyGrammarGeneratedModule, n as AbstractMermaidValueConverter, o as EmptyFileSystem, u as MermaidGeneratedSharedModule, w as inject, x as __name } from "./chunk-KEIR6QF5-XwhODICK.js";
var _Class;
var WardleyValueConverter = (_Class = class extends AbstractMermaidValueConverter {
	runCustomConverter(rule, input, _cstNode) {
		switch (rule.name.toUpperCase()) {
			case "LINK_LABEL": return input.substring(1).trim();
			default: return;
		}
	}
}, __name(_Class, "WardleyValueConverter"), _Class);
var WardleyModule = { parser: { ValueConverter: /* @__PURE__ */ __name(() => new WardleyValueConverter(), "ValueConverter") } };
function createWardleyServices(context = EmptyFileSystem) {
	const shared = inject(createDefaultSharedCoreModule(context), MermaidGeneratedSharedModule);
	const Wardley = inject(createDefaultCoreModule({ shared }), WardleyGrammarGeneratedModule, WardleyModule);
	shared.ServiceRegistry.register(Wardley);
	return {
		shared,
		Wardley
	};
}
__name(createWardleyServices, "createWardleyServices");
export { createWardleyServices as n, WardleyModule as t };
