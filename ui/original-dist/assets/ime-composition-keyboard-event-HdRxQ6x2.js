import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
function isImeOwnedKeyboardEvent(event) {
	const candidate = event;
	return candidate.isComposing === true || candidate.keyCode === 229 || candidate.nativeEvent?.isComposing === true || candidate.nativeEvent?.keyCode === 229;
}
function useImeEnterGestureOwnership() {
	const stateRef = (0, import_react.useRef)({
		composing: false,
		pendingEnter: null
	});
	return (0, import_react.useMemo)(() => {
		const reset = () => {
			stateRef.current = {
				composing: false,
				pendingEnter: null
			};
		};
		const isPlainEnter = (event) => event.key === "Enter" && event.keyCode === 13 && !event.shiftKey;
		const hasChordModifier = (event) => Boolean(event.altKey || event.ctrlKey || event.metaKey);
		return {
			isComposing: () => stateRef.current.composing,
			ownsKeyDown: (event) => {
				if ((event.nativeEvent.isComposing || stateRef.current.composing) && (isPlainEnter(event) || event.key === "Enter" && event.keyCode === 229 || event.key === "Process" && event.keyCode === 229)) {
					stateRef.current.pendingEnter = {};
					return true;
				}
				if (stateRef.current.pendingEnter && isPlainEnter(event) && !event.nativeEvent.isComposing) {
					stateRef.current.pendingEnter = null;
					if (hasChordModifier(event)) return false;
					event.preventDefault();
					return true;
				}
				return false;
			},
			onKeyUp: (event) => {
				if (event.key === "Process" && event.keyCode === 229) {
					stateRef.current.pendingEnter = null;
					return;
				}
				const pendingEnter = stateRef.current.pendingEnter;
				if (pendingEnter) requestAnimationFrame(() => {
					if (stateRef.current.pendingEnter === pendingEnter) stateRef.current.pendingEnter = null;
				});
			},
			reset,
			setComposing: (active) => {
				stateRef.current.composing = active;
			}
		};
	}, []);
}
function isImeCompositionKeyDown(event) {
	return isImeOwnedKeyboardEvent(event);
}
export { useImeEnterGestureOwnership as n, isImeCompositionKeyDown as t };
