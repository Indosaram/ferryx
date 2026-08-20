const STATUS_BAR_CONTEXT_MENU_EXEMPT_ATTR = "data-status-bar-context-menu-exempt";
const STATUS_BAR_CONTEXT_MENU_EXEMPT_SELECTOR = `[${STATUS_BAR_CONTEXT_MENU_EXEMPT_ATTR}]`;
const STATUS_BAR_CONTEXT_MENU_EXEMPT_PROPS = { [STATUS_BAR_CONTEXT_MENU_EXEMPT_ATTR]: "" };
var FLOATING_TERMINAL_TOGGLE_SELECTOR = "[data-floating-terminal-toggle]";
function hasClosest(target) {
	return typeof target?.closest === "function";
}
function closestTargetFromEventTarget(target) {
	if (hasClosest(target)) return target;
	const parentElement = target?.parentElement;
	if (hasClosest(parentElement)) return parentElement;
	const parentNode = target?.parentNode;
	return hasClosest(parentNode) ? parentNode : null;
}
function shouldOpenStatusBarContextMenu(target) {
	const closestTarget = closestTargetFromEventTarget(target);
	if (!closestTarget) return true;
	return closestTarget.closest(FLOATING_TERMINAL_TOGGLE_SELECTOR) === null && closestTarget.closest(STATUS_BAR_CONTEXT_MENU_EXEMPT_SELECTOR) === null;
}
export { shouldOpenStatusBarContextMenu as n, STATUS_BAR_CONTEXT_MENU_EXEMPT_PROPS as t };
