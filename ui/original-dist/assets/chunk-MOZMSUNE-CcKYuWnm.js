import { C as createDefaultSharedCoreModule, S as createDefaultCoreModule, n as AbstractMermaidValueConverter, o as EmptyFileSystem, r as ArchitectureGrammarGeneratedModule, t as AbstractMermaidTokenBuilder, u as MermaidGeneratedSharedModule, w as inject, x as __name } from "./chunk-KEIR6QF5-XwhODICK.js";
var _Class, _Class2;
var ArchitectureTokenBuilder = (_Class = class extends AbstractMermaidTokenBuilder {
	constructor() {
		super(["architecture"]);
	}
}, __name(_Class, "ArchitectureTokenBuilder"), _Class);
var ArchitectureValueConverter = (_Class2 = class extends AbstractMermaidValueConverter {
	runCustomConverter(rule, input, _cstNode) {
		if (rule.name === "ARCH_ICON") return input.replace(/[()]/g, "").trim();
		else if (rule.name === "ARCH_TEXT_ICON") return input.replace(/["()]/g, "");
		else if (rule.name === "ARCH_TITLE") {
			let result = input.replace(/^\[|]$/g, "").trim();
			if (result.startsWith("\"") && result.endsWith("\"") || result.startsWith("'") && result.endsWith("'")) {
				result = result.slice(1, -1);
				result = result.replace(/\\"/g, "\"").replace(/\\'/g, "'");
			}
			return result.trim();
		}
	}
}, __name(_Class2, "ArchitectureValueConverter"), _Class2);
var ArchitectureModule = { parser: {
	TokenBuilder: /* @__PURE__ */ __name(() => new ArchitectureTokenBuilder(), "TokenBuilder"),
	ValueConverter: /* @__PURE__ */ __name(() => new ArchitectureValueConverter(), "ValueConverter")
} };
function createArchitectureServices(context = EmptyFileSystem) {
	const shared = inject(createDefaultSharedCoreModule(context), MermaidGeneratedSharedModule);
	const Architecture = inject(createDefaultCoreModule({ shared }), ArchitectureGrammarGeneratedModule, ArchitectureModule);
	shared.ServiceRegistry.register(Architecture);
	return {
		shared,
		Architecture
	};
}
__name(createArchitectureServices, "createArchitectureServices");
export { createArchitectureServices as n, ArchitectureModule as t };
