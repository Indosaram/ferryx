import { C as createDefaultSharedCoreModule, S as createDefaultCoreModule, i as CommonValueConverter, l as InfoGrammarGeneratedModule, o as EmptyFileSystem, t as AbstractMermaidTokenBuilder, u as MermaidGeneratedSharedModule, w as inject, x as __name } from "./chunk-KEIR6QF5-XwhODICK.js";
var _Class;
var InfoTokenBuilder = (_Class = class extends AbstractMermaidTokenBuilder {
	constructor() {
		super(["info", "showInfo"]);
	}
}, __name(_Class, "InfoTokenBuilder"), _Class);
var InfoModule = { parser: {
	TokenBuilder: /* @__PURE__ */ __name(() => new InfoTokenBuilder(), "TokenBuilder"),
	ValueConverter: /* @__PURE__ */ __name(() => new CommonValueConverter(), "ValueConverter")
} };
function createInfoServices(context = EmptyFileSystem) {
	const shared = inject(createDefaultSharedCoreModule(context), MermaidGeneratedSharedModule);
	const Info = inject(createDefaultCoreModule({ shared }), InfoGrammarGeneratedModule, InfoModule);
	shared.ServiceRegistry.register(Info);
	return {
		shared,
		Info
	};
}
__name(createInfoServices, "createInfoServices");
export { createInfoServices as n, InfoModule as t };
