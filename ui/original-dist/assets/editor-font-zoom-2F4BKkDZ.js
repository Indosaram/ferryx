var EDITOR_FONT_ZOOM_MIN = -6;
var EDITOR_FONT_ZOOM_MAX = 18;
var EDITOR_FONT_ZOOM_STEP = 1;
function clampEditorFontZoomLevel(level) {
	return Math.max(EDITOR_FONT_ZOOM_MIN, Math.min(EDITOR_FONT_ZOOM_MAX, level));
}
function nextEditorFontZoomLevel(current, direction) {
	if (direction === "reset") return 0;
	if (direction === "in") return clampEditorFontZoomLevel(current + EDITOR_FONT_ZOOM_STEP);
	return clampEditorFontZoomLevel(current - EDITOR_FONT_ZOOM_STEP);
}
function computeEditorFontSize(baseFontSize, zoomLevel) {
	return Math.max(8, Math.min(32, baseFontSize + zoomLevel));
}
function computeDiffEditorFontSize(baseFontSize, zoomLevel) {
	return computeEditorFontSize(baseFontSize - .5, zoomLevel);
}
function resolveEditorFontFamily(settings) {
	return settings?.editorFontFamily?.trim() || settings?.terminalFontFamily || "monospace";
}
function resolveEditorFontFamilyOrInherit(settings) {
	return settings?.editorFontFamily?.trim() || settings?.terminalFontFamily || void 0;
}
export { resolveEditorFontFamilyOrInherit as a, resolveEditorFontFamily as i, computeEditorFontSize as n, nextEditorFontZoomLevel as r, computeDiffEditorFontSize as t };
