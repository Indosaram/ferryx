import { du as readUtf8CodePointAt, su as getUtf8ByteLengthForCodePoint } from "./store-CgXrfmaH.js";
const COMMENT_BODY_NONBLANK_SCAN_MAX_BYTES = 64 * 1024;
function getCommentBodyPresence(body, maxScanBytes = COMMENT_BODY_NONBLANK_SCAN_MAX_BYTES) {
	let scannedBytes = 0;
	for (let index = 0; index < body.length; index += 1) {
		const codePoint = readUtf8CodePointAt(body, index);
		const codeUnitLength = codePoint > 65535 ? 2 : 1;
		scannedBytes += getUtf8ByteLengthForCodePoint(codePoint);
		if (scannedBytes > maxScanBytes) return "too-large-leading-whitespace";
		if (/\S/u.test(body.slice(index, index + codeUnitLength))) return "present";
		if (codeUnitLength === 2) index += 1;
	}
	return "empty";
}
function hasBoundedCommentBodyText(body) {
	return getCommentBodyPresence(body) === "present";
}
function getCommentBodySubmitState(body) {
	const presence = getCommentBodyPresence(body);
	if (presence === "empty") return { status: "empty" };
	if (presence === "too-large-leading-whitespace") return { status: "too-large-leading-whitespace" };
	return {
		status: "ready",
		body: body.trim()
	};
}
export { hasBoundedCommentBodyText as n, getCommentBodySubmitState as t };
