import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
const ConfirmationDialogContext = (0, import_react.createContext)(null);
function useConfirmationDialog() {
	const confirm = (0, import_react.useContext)(ConfirmationDialogContext);
	if (!confirm) throw new Error("useConfirmationDialog must be used inside ConfirmationDialogProvider");
	return confirm;
}
export { useConfirmationDialog as n, ConfirmationDialogContext as t };
