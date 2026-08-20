import { C as createDefaultSharedCoreModule, S as createDefaultCoreModule, a as CynefinGrammarGeneratedModule, i as CommonValueConverter, o as EmptyFileSystem, t as AbstractMermaidTokenBuilder, u as MermaidGeneratedSharedModule, w as inject, x as __name } from "./chunk-KEIR6QF5-XwhODICK.js";
var _Class;
var CynefinTokenBuilder = (_Class = class extends AbstractMermaidTokenBuilder {
	constructor() {
		super(["cynefin-beta"]);
	}
}, __name(_Class, "CynefinTokenBuilder"), _Class);
var CynefinModule = { parser: {
	TokenBuilder: /* @__PURE__ */ __name(() => new CynefinTokenBuilder(), "TokenBuilder"),
	ValueConverter: /* @__PURE__ */ __name(() => new CommonValueConverter(), "ValueConverter")
} };
function createCynefinServices(context = EmptyFileSystem) {
	const shared = inject(createDefaultSharedCoreModule(context), MermaidGeneratedSharedModule);
	const Cynefin = inject(createDefaultCoreModule({ shared }), CynefinGrammarGeneratedModule, CynefinModule);
	shared.ServiceRegistry.register(Cynefin);
	return {
		shared,
		Cynefin
	};
}
__name(createCynefinServices, "createCynefinServices");
export { createCynefinServices as n, CynefinModule as t };
