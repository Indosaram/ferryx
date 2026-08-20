import { du as readUtf8CodePointAt, no as yieldToEventLoop, su as getUtf8ByteLengthForCodePoint } from "./store-CgXrfmaH.js";
function measurePastePayloadMetadata(text, options = {}) {
	if (!text) return createEmptyPastePayloadMetadata();
	const stopAfterBytes = options.stopAfterBytes;
	let byteLength = 0;
	let hasControlSequences = false;
	let lineCount = 1;
	let previousWasCarriageReturn = false;
	for (let index = 0; index < text.length; index += 1) {
		const codePoint = readUtf8CodePointAt(text, index);
		byteLength += getUtf8ByteLengthForCodePoint(codePoint);
		hasControlSequences || (hasControlSequences = isPasteControlSequenceCodePoint(codePoint));
		if (codePoint === 13) {
			lineCount += 1;
			previousWasCarriageReturn = true;
		} else {
			if (codePoint === 10 && !previousWasCarriageReturn) lineCount += 1;
			previousWasCarriageReturn = false;
		}
		if (Number.isFinite(stopAfterBytes) && byteLength > (stopAfterBytes ?? 0)) return {
			byteLength,
			exceededLimit: true,
			hasControlSequences,
			lineCount
		};
		if (codePoint > 65535) index += 1;
	}
	return {
		byteLength,
		exceededLimit: false,
		hasControlSequences,
		lineCount
	};
}
async function measurePastePayloadMetadataWithYield(text, options = {}) {
	if (!text) return createEmptyPastePayloadMetadata();
	const stopAfterBytes = options.stopAfterBytes;
	const yieldAfterCodeUnits = Math.max(1, options.yieldAfterCodeUnits ?? 262144);
	const yieldBetweenBatches = options.yieldToEventLoop ?? yieldToEventLoop;
	let nextYieldAt = yieldAfterCodeUnits;
	let byteLength = 0;
	let hasControlSequences = false;
	let lineCount = 1;
	let previousWasCarriageReturn = false;
	for (let index = 0; index < text.length; index += 1) {
		const codePoint = readUtf8CodePointAt(text, index);
		byteLength += getUtf8ByteLengthForCodePoint(codePoint);
		hasControlSequences || (hasControlSequences = isPasteControlSequenceCodePoint(codePoint));
		if (codePoint === 13) {
			lineCount += 1;
			previousWasCarriageReturn = true;
		} else {
			if (codePoint === 10 && !previousWasCarriageReturn) lineCount += 1;
			previousWasCarriageReturn = false;
		}
		if (Number.isFinite(stopAfterBytes) && byteLength > (stopAfterBytes ?? 0)) return {
			byteLength,
			exceededLimit: true,
			hasControlSequences,
			lineCount
		};
		if (codePoint > 65535) index += 1;
		if (index >= nextYieldAt) {
			await yieldBetweenBatches();
			nextYieldAt = index + yieldAfterCodeUnits;
		}
	}
	return {
		byteLength,
		exceededLimit: false,
		hasControlSequences,
		lineCount
	};
}
function createEmptyPastePayloadMetadata() {
	return {
		byteLength: 0,
		exceededLimit: false,
		hasControlSequences: false,
		lineCount: 0
	};
}
function isPasteControlSequenceCodePoint(codePoint) {
	return codePoint <= 8 || codePoint === 11 || codePoint === 12 || codePoint >= 14 && codePoint <= 31 || codePoint === 127;
}
export { measurePastePayloadMetadataWithYield as n, measurePastePayloadMetadata as t };
