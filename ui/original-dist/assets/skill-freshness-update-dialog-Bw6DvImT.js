var pendingOpen = false;
var listeners = /* @__PURE__ */ new Set();
function requestSkillFreshnessUpdateDialog() {
	pendingOpen = true;
	for (const listener of listeners) listener();
}
function consumeSkillFreshnessUpdateDialogRequest() {
	const requested = pendingOpen;
	pendingOpen = false;
	if (requested) for (const listener of listeners) listener();
	return requested;
}
function getSkillFreshnessUpdateDialogRequest() {
	return pendingOpen;
}
function subscribeSkillFreshnessUpdateDialog(listener) {
	listeners.add(listener);
	return () => listeners.delete(listener);
}
export { subscribeSkillFreshnessUpdateDialog as i, getSkillFreshnessUpdateDialogRequest as n, requestSkillFreshnessUpdateDialog as r, consumeSkillFreshnessUpdateDialogRequest as t };
