import { C as createDefaultSharedCoreModule, S as createDefaultCoreModule, n as AbstractMermaidValueConverter, o as EmptyFileSystem, t as AbstractMermaidTokenBuilder, u as MermaidGeneratedSharedModule, v as TreeViewGrammarGeneratedModule, w as inject, x as __name } from "./chunk-KEIR6QF5-XwhODICK.js";
var _Class, _Class2;
var TreeViewValueConverter = (_Class = class extends AbstractMermaidValueConverter {
	runCustomConverter(rule, input, _cstNode) {
		if (rule.name === "INDENTATION") return input?.length || 0;
		if (rule.name === "QUOTED_NAME") return input.substring(1, input.length - 1);
		if (rule.name === "BARE_NAME") return input.replace(/[\t ]+$/, "");
		if (rule.name === "CLASS_ANNOTATION") return input.trim().substring(3).trim();
		if (rule.name === "ICON_ANNOTATION") {
			const trimmed = input.trim();
			return trimmed.substring(5, trimmed.length - 1);
		}
		if (rule.name === "DESC_ANNOTATION") return input.trim().substring(2).trim();
	}
}, __name(_Class, "TreeViewValueConverter"), _Class);
var TreeViewTokenBuilder = (_Class2 = class extends AbstractMermaidTokenBuilder {
	constructor() {
		super(["treeView-beta"]);
	}
}, __name(_Class2, "TreeViewTokenBuilder"), _Class2);
var TreeViewModule = { parser: {
	TokenBuilder: /* @__PURE__ */ __name(() => new TreeViewTokenBuilder(), "TokenBuilder"),
	ValueConverter: /* @__PURE__ */ __name(() => new TreeViewValueConverter(), "ValueConverter")
} };
function createTreeViewServices(context = EmptyFileSystem) {
	const shared = inject(createDefaultSharedCoreModule(context), MermaidGeneratedSharedModule);
	const TreeView = inject(createDefaultCoreModule({ shared }), TreeViewGrammarGeneratedModule, TreeViewModule);
	shared.ServiceRegistry.register(TreeView);
	return {
		shared,
		TreeView
	};
}
__name(createTreeViewServices, "createTreeViewServices");
export { createTreeViewServices as n, TreeViewModule as t };
