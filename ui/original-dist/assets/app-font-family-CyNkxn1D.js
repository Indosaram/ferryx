import { Om as DEFAULT_APP_FONT_FAMILY } from "./store-CgXrfmaH.js";
var APP_FONT_FALLBACKS = [
	DEFAULT_APP_FONT_FAMILY,
	"-apple-system",
	"BlinkMacSystemFont",
	"Segoe UI",
	"sans-serif"
];
var CSS_FONT_KEYWORDS = new Set([
	"serif",
	"sans-serif",
	"monospace",
	"cursive",
	"fantasy",
	"system-ui",
	"blinkmacsystemfont"
]);
function quoteFontFamily(fontFamily) {
	if (fontFamily.startsWith("-") || CSS_FONT_KEYWORDS.has(fontFamily.toLowerCase())) return fontFamily;
	return JSON.stringify(fontFamily);
}
function buildAppFontFamily(fontFamily) {
	const trimmed = fontFamily?.trim() || "Geist";
	const lowerTrimmed = trimmed.toLowerCase();
	return [trimmed, ...APP_FONT_FALLBACKS.filter((fallback) => fallback.toLowerCase() !== lowerTrimmed)].map(quoteFontFamily).join(", ");
}
export { buildAppFontFamily as t };
