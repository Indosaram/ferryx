import { n as __name } from "./chunk-Y2CYZVJY-DUyCWguD.js";
import { p as select_default } from "./src-BFzutHoD.js";
var getDiagramElement = /* @__PURE__ */ __name((id, securityLevel) => {
	let sandboxElement;
	if (securityLevel === "sandbox") sandboxElement = select_default("#i" + id);
	return (securityLevel === "sandbox" ? select_default(sandboxElement.nodes()[0].contentDocument.body) : select_default("body")).select(`[id="${id}"]`);
}, "getDiagramElement");
export { getDiagramElement as t };
