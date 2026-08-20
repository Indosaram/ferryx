import { cu as getUtf8ChunkEndIndex, no as yieldToEventLoop } from "./store-CgXrfmaH.js";
import { n as measurePastePayloadMetadataWithYield, t as measurePastePayloadMetadata } from "./paste-payload-metadata-pr3nuODB.js";
function createRedactedTextControlPasteDiagnostic({ byteLength, chunksWritten, durationMs, hasControlSequences, lineCount, mode, reason, source, status }) {
	return [
		"text-control paste",
		`status=${status}`,
		mode ? `mode=${mode}` : null,
		"target=text-control",
		`source=${source}`,
		`bytes=${byteLength}`,
		`lines=${lineCount ?? "unknown"}`,
		`chunks=${chunksWritten}`,
		`durationMs=${Math.max(0, Math.round(durationMs))}`,
		`controls=${hasControlSequences ?? "unknown"}`,
		reason ? `reason=${reason}` : null,
		"content=redacted"
	].filter((part) => typeof part === "string").join(" ");
}
function getTextControlPasteSource(source) {
	return source ?? "programmatic";
}
function createTextControlPastedResult({ byteLength, chunksWritten, durationMs, hasControlSequences, lineCount, mode, source }) {
	return {
		status: "pasted",
		mode,
		byteLength,
		chunksWritten,
		durationMs: Math.max(0, Math.round(durationMs)),
		redactedDiagnostic: createRedactedTextControlPasteDiagnostic({
			byteLength,
			chunksWritten,
			durationMs,
			hasControlSequences,
			lineCount,
			mode,
			source,
			status: "pasted"
		})
	};
}
function createTextControlRejectedResult(reason, metadata, source, durationMs) {
	const diagnosticMetadata = typeof metadata === "number" ? { byteLength: metadata } : metadata;
	return {
		status: "rejected",
		reason,
		byteLength: diagnosticMetadata.byteLength,
		chunksWritten: 0,
		durationMs: Math.max(0, Math.round(durationMs)),
		redactedDiagnostic: createRedactedTextControlPasteDiagnostic({
			byteLength: diagnosticMetadata.byteLength,
			chunksWritten: 0,
			durationMs,
			hasControlSequences: diagnosticMetadata.hasControlSequences,
			lineCount: diagnosticMetadata.lineCount,
			reason,
			source,
			status: "rejected"
		})
	};
}
function createTextControlCancelledResult({ byteLength, chunksWritten, durationMs, hasControlSequences, lineCount, source }) {
	return {
		status: "cancelled",
		reason: "target-unavailable",
		byteLength,
		chunksWritten,
		durationMs: Math.max(0, Math.round(durationMs)),
		redactedDiagnostic: createRedactedTextControlPasteDiagnostic({
			byteLength,
			chunksWritten,
			durationMs,
			hasControlSequences,
			lineCount,
			reason: "target-unavailable",
			source,
			status: "cancelled"
		})
	};
}
const TEXT_CONTROL_PASTE_DIRECT_MAX_BYTES = 64 * 1024;
const TEXT_CONTROL_PASTE_CHUNK_MAX_BYTES = 16 * 1024;
const TEXT_CONTROL_PASTE_MAX_BYTES = 16 * 1024 * 1024;
function measureTextControlPasteByteLength(text, options = {}) {
	const { byteLength, exceededLimit } = measurePastePayloadMetadata(text, options);
	return {
		byteLength,
		exceededLimit
	};
}
async function measureTextControlPasteByteLengthWithYield(text, options) {
	const { byteLength, exceededLimit } = await measurePastePayloadMetadataWithYield(text, {
		stopAfterBytes: options.stopAfterBytes,
		yieldAfterCodeUnits: options.yieldAfterCodeUnits ?? 65536,
		yieldToEventLoop: options.yieldToEventLoop
	});
	return {
		byteLength,
		exceededLimit
	};
}
async function measureTextControlPasteForExecution(text, options) {
	const maxBytes = options.maxBytes ?? 16777216;
	if (options.measuredByteLength !== void 0) return {
		byteLength: options.measuredByteLength,
		exceededLimit: options.measuredByteLength > maxBytes
	};
	const directMaxBytes = options.directMaxBytes ?? 65536;
	const preflightLimit = Math.min(directMaxBytes, maxBytes);
	const preflightMeasurement = measurePastePayloadMetadata(text, { stopAfterBytes: preflightLimit });
	if (!preflightMeasurement.exceededLimit || preflightLimit === maxBytes) return preflightMeasurement;
	return measurePastePayloadMetadataWithYield(text, {
		stopAfterBytes: maxBytes,
		yieldAfterCodeUnits: options.measureYieldAfterCodeUnits,
		yieldToEventLoop: options.yieldToEventLoop
	});
}
function shouldHandleTextControlPaste(text, options = {}) {
	if (!text) return false;
	const maxBytes = options.maxBytes ?? 16777216;
	const measurement = options.measuredByteLength === void 0 ? measureTextControlPasteByteLength(text, { stopAfterBytes: maxBytes }) : {
		byteLength: options.measuredByteLength,
		exceededLimit: options.measuredByteLength > maxBytes
	};
	const directMaxBytes = options.directMaxBytes ?? 65536;
	return measurement.exceededLimit || measurement.byteLength > directMaxBytes;
}
function dispatchTextControlInputEvent(target, data, inputType) {
	const event = typeof InputEvent === "function" ? new InputEvent("input", {
		bubbles: true,
		cancelable: false,
		data,
		inputType
	}) : new Event("input", {
		bubbles: true,
		cancelable: false
	});
	target.dispatchEvent(event);
}
function isTextControlPasteTargetAvailable(target, canContinue) {
	return target.isConnected && !target.disabled && !target.readOnly && (canContinue?.(target) ?? true);
}
function getSelectionRange(target) {
	const start = target.selectionStart ?? target.value.length;
	const end = target.selectionEnd ?? start;
	return {
		start: Math.min(start, end),
		end: Math.max(start, end)
	};
}
function defaultNow() {
	return globalThis.performance?.now?.() ?? Date.now();
}
async function pasteTextIntoTextControl(target, text, options = {}) {
	const source = getTextControlPasteSource(options.source);
	const now = options.now ?? defaultNow;
	const startedAtMs = now();
	const getDurationMs = () => Math.max(0, now() - startedAtMs);
	const maxBytes = options.maxBytes ?? 16777216;
	const directMaxBytes = options.directMaxBytes ?? 65536;
	const pastePayloadMeasurement = await measureTextControlPasteForExecution(text, {
		directMaxBytes,
		maxBytes,
		measuredByteLength: options.measuredByteLength,
		measureYieldAfterCodeUnits: options.measureYieldAfterCodeUnits,
		yieldToEventLoop: options.yieldToEventLoop
	});
	const { byteLength } = pastePayloadMeasurement;
	if (byteLength === 0) return createTextControlRejectedResult("empty", pastePayloadMeasurement, source, getDurationMs());
	const chunkMaxBytes = options.chunkMaxBytes ?? 16384;
	const continuePaste = options.canContinue;
	const inputType = options.inputType ?? "insertFromPaste";
	if (pastePayloadMeasurement.exceededLimit) return createTextControlRejectedResult("too-large", pastePayloadMeasurement, source, getDurationMs());
	if (!isTextControlPasteTargetAvailable(target, continuePaste)) return createTextControlRejectedResult("target-unavailable", pastePayloadMeasurement, source, getDurationMs());
	try {
		target.focus();
		if (byteLength <= directMaxBytes) {
			const { start: start$1, end: end$1 } = getSelectionRange(target);
			target.setRangeText(text, start$1, end$1, "end");
			dispatchTextControlInputEvent(target, text, inputType);
			return createTextControlPastedResult({
				byteLength,
				chunksWritten: 1,
				hasControlSequences: pastePayloadMeasurement.hasControlSequences,
				lineCount: pastePayloadMeasurement.lineCount,
				mode: "direct",
				source,
				durationMs: getDurationMs()
			});
		}
		const { start, end } = getSelectionRange(target);
		if (start !== end) target.setRangeText("", start, end, "end");
		let chunksWritten = 0;
		let textIndex = 0;
		while (textIndex < text.length) {
			if (!isTextControlPasteTargetAvailable(target, continuePaste)) {
				if (chunksWritten > 0 && target.isConnected) dispatchTextControlInputEvent(target, null, inputType);
				return createTextControlCancelledResult({
					byteLength,
					chunksWritten,
					hasControlSequences: pastePayloadMeasurement.hasControlSequences,
					lineCount: pastePayloadMeasurement.lineCount,
					source,
					durationMs: getDurationMs()
				});
			}
			const nextIndex = getUtf8ChunkEndIndex(text, textIndex, chunkMaxBytes);
			const chunk = text.slice(textIndex, nextIndex);
			const caret = target.selectionStart ?? target.value.length;
			target.setRangeText(chunk, caret, caret, "end");
			textIndex = nextIndex;
			chunksWritten += 1;
			if (textIndex < text.length) await (options.yieldToEventLoop ?? yieldToEventLoop)();
		}
		dispatchTextControlInputEvent(target, null, inputType);
		return createTextControlPastedResult({
			byteLength,
			chunksWritten,
			hasControlSequences: pastePayloadMeasurement.hasControlSequences,
			lineCount: pastePayloadMeasurement.lineCount,
			mode: "chunked",
			source,
			durationMs: getDurationMs()
		});
	} catch {
		return createTextControlRejectedResult("target-unavailable", pastePayloadMeasurement, source, getDurationMs());
	}
}
export { TEXT_CONTROL_PASTE_CHUNK_MAX_BYTES as a, createTextControlRejectedResult as c, shouldHandleTextControlPaste as i, measureTextControlPasteByteLengthWithYield as n, TEXT_CONTROL_PASTE_DIRECT_MAX_BYTES as o, pasteTextIntoTextControl as r, TEXT_CONTROL_PASTE_MAX_BYTES as s, measureTextControlPasteByteLength as t };
