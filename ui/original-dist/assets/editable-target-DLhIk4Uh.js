function isEditableTarget(target) {
	if (!(target instanceof HTMLElement)) return false;
	if (target.classList.contains("xterm-helper-textarea")) return false;
	if (target.isContentEditable) return true;
	return target.closest("input, textarea, select, [contenteditable=\"\"], [contenteditable=\"true\"]") !== null;
}
export { isEditableTarget as t };
