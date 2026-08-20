import { C as createDefaultSharedCoreModule, S as createDefaultCoreModule, i as CommonValueConverter, o as EmptyFileSystem, p as RadarGrammarGeneratedModule, t as AbstractMermaidTokenBuilder, u as MermaidGeneratedSharedModule, w as inject, x as __name } from "./chunk-KEIR6QF5-XwhODICK.js";
var _Class;
var RadarTokenBuilder = (_Class = class extends AbstractMermaidTokenBuilder {
	constructor() {
		super(["radar-beta"]);
	}
}, __name(_Class, "RadarTokenBuilder"), _Class);
var RadarModule = { parser: {
	TokenBuilder: /* @__PURE__ */ __name(() => new RadarTokenBuilder(), "TokenBuilder"),
	ValueConverter: /* @__PURE__ */ __name(() => new CommonValueConverter(), "ValueConverter")
} };
function createRadarServices(context = EmptyFileSystem) {
	const shared = inject(createDefaultSharedCoreModule(context), MermaidGeneratedSharedModule);
	const Radar = inject(createDefaultCoreModule({ shared }), RadarGrammarGeneratedModule, RadarModule);
	shared.ServiceRegistry.register(Radar);
	return {
		shared,
		Radar
	};
}
__name(createRadarServices, "createRadarServices");
export { createRadarServices as n, RadarModule as t };
