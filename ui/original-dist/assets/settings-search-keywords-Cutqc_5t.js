import { a as translate, r as i18n } from "./jsx-runtime-Cv_nyRjc.js";
function translateSearchKeyword(key, fallback, options) {
	if (options?.englishOnly || i18n.language === "en") return uniqueKeywords([fallback, ...options?.aliases ?? []]);
	return uniqueKeywords([
		translate(key, fallback),
		fallback,
		...options?.aliases ?? []
	]);
}
function searchKeywords(terms) {
	return uniqueKeywords(terms.flatMap((term) => {
		if (typeof term === "string") return [term];
		return translateSearchKeyword(term.key, term.fallback, term);
	}));
}
function uniqueKeywords(values) {
	const seen = /* @__PURE__ */ new Set();
	const result = [];
	for (const value of values) {
		const trimmed = value.trim();
		if (!trimmed || seen.has(trimmed)) continue;
		seen.add(trimmed);
		result.push(trimmed);
	}
	return result;
}
export { translateSearchKeyword as n, uniqueKeywords as r, searchKeywords as t };
