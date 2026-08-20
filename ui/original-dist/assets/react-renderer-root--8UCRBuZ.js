import { t as require_client } from "./client-XKKXQWGM.js";
var import_client = require_client();
function getOrCreateRendererRoot(container, hotData) {
	const existingRoot = hotData?.orcaRendererRoot;
	if (existingRoot) return existingRoot;
	const root = (0, import_client.createRoot)(container);
	if (hotData) hotData.orcaRendererRoot = root;
	return root;
}
export { getOrCreateRendererRoot as t };
