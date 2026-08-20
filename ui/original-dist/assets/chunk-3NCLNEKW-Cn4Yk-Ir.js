import { n as __name } from "./chunk-Y2CYZVJY-DUyCWguD.js";
import { p as select_default } from "./src-BFzutHoD.js";
import { x as getConfig2 } from "./chunk-I66GZJ75-7VFc6IgS.js";
var selectSvgElement = /* @__PURE__ */ __name((id) => {
	const { securityLevel } = getConfig2();
	let root = select_default("body");
	if (securityLevel === "sandbox") root = select_default((select_default(`#i${id}`).node()?.contentDocument ?? document).body);
	return root.select(`#${id}`);
}, "selectSvgElement");
export { selectSvgElement as t };
