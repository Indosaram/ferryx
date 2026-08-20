var CACHE_MAX_ENTRIES = 20;
function setWithLRU(map, key, value, maxEntries = CACHE_MAX_ENTRIES) {
	map.delete(key);
	map.set(key, value);
	if (map.size > maxEntries) {
		const first = map.keys().next();
		if (!first.done) map.delete(first.value);
	}
}
const scrollTopCache = /* @__PURE__ */ new Map();
const cursorPositionCache = /* @__PURE__ */ new Map();
const pdfViewPositionCache = /* @__PURE__ */ new Map();
const diffViewStateCache = /* @__PURE__ */ new Map();
export { setWithLRU as a, scrollTopCache as i, diffViewStateCache as n, pdfViewPositionCache as r, cursorPositionCache as t };
