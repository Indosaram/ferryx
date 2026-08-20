import { C as createDefaultSharedCoreModule, S as createDefaultCoreModule, g as RailroadGrammarGeneratedModule, n as AbstractMermaidValueConverter, o as EmptyFileSystem, t as AbstractMermaidTokenBuilder, u as MermaidGeneratedSharedModule, w as inject, x as __name } from "./chunk-KEIR6QF5-XwhODICK.js";
var _Class, _Class2;
var RailroadTokenBuilder = (_Class = class extends AbstractMermaidTokenBuilder {
	constructor() {
		super(["railroad-beta"]);
	}
}, __name(_Class, "RailroadTokenBuilder"), _Class);
var decodeEscapedString = /* @__PURE__ */ __name((input) => {
	const content = input.slice(1, -1);
	let value = "";
	for (let index = 0; index < content.length; index++) {
		const character = content[index];
		if (character === "\\" && index + 1 < content.length) {
			index++;
			const escaped = content[index];
			switch (escaped) {
				case "n":
					value += "\n";
					break;
				case "r":
					value += "\r";
					break;
				case "t":
					value += "	";
					break;
				default: value += escaped;
			}
			continue;
		}
		value += character;
	}
	return value;
}, "decodeEscapedString");
var RailroadValueConverter = (_Class2 = class extends AbstractMermaidValueConverter {
	runConverter(rule, input, cstNode) {
		const value = super.runConverter(rule, input, cstNode);
		if (rule.name === "TITLE" && typeof value === "string") {
			const trimmedValue = value.trim();
			if (trimmedValue.startsWith("\"") && trimmedValue.endsWith("\"") || trimmedValue.startsWith("'") && trimmedValue.endsWith("'")) return decodeEscapedString(trimmedValue);
		}
		return value;
	}
	runCustomConverter(rule, input, _cstNode) {
		if (rule.name === "RR_STRING") return decodeEscapedString(input);
	}
}, __name(_Class2, "RailroadValueConverter"), _Class2);
var RailroadModule = { parser: {
	TokenBuilder: /* @__PURE__ */ __name(() => new RailroadTokenBuilder(), "TokenBuilder"),
	ValueConverter: /* @__PURE__ */ __name(() => new RailroadValueConverter(), "ValueConverter")
} };
function createRailroadServices(context = EmptyFileSystem) {
	const shared = inject(createDefaultSharedCoreModule(context), MermaidGeneratedSharedModule);
	const Railroad = inject(createDefaultCoreModule({ shared }), RailroadGrammarGeneratedModule, RailroadModule);
	shared.ServiceRegistry.register(Railroad);
	return {
		shared,
		Railroad
	};
}
__name(createRailroadServices, "createRailroadServices");
export { createRailroadServices as n, RailroadModule as t };
