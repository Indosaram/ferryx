import { $a as isClipboardTextByteLengthOverLimit } from "./store-CgXrfmaH.js";
const FIND_QUERY_MAX_BYTES = 2 * 1024;
function isFindQueryTooLarge(query, maxBytes = FIND_QUERY_MAX_BYTES) {
	return isClipboardTextByteLengthOverLimit(query, maxBytes);
}
function getFindRequestQuery(query) {
	return isFindQueryTooLarge(query) ? null : query;
}
export { isFindQueryTooLarge as n, getFindRequestQuery as t };
