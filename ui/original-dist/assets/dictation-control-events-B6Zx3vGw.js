const DICTATION_CONTROL_EVENT = "dictation:control";
function dispatchDictationControl(action) {
	document.dispatchEvent(new CustomEvent(DICTATION_CONTROL_EVENT, { detail: action }));
}
export { dispatchDictationControl as n, DICTATION_CONTROL_EVENT as t };
