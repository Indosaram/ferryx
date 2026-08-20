var interruptedBracketedPasteTerminals = /* @__PURE__ */ new WeakSet();
var bracketedPasteModeOutputTail = /* @__PURE__ */ new WeakMap();
var ESCAPE = "\x1B";
const BRACKETED_PASTE_START = `${ESCAPE}[200~`;
const BRACKETED_PASTE_END = `${ESCAPE}[201~`;
var BRACKETED_PASTE_MODE_SEQUENCE_RE = /^\[\?(?:\d+;)*2004(?:;\d+)*[hl]/;
var BRACKETED_PASTE_MODE_TAIL_MAX = 128;
var BRACKETED_PASTE_MODE_SEQUENCE_SCAN_MAX = BRACKETED_PASTE_MODE_TAIL_MAX;
var LINE_BREAK_RE = /[\r\n]/;
function hasBracketedPasteModeSequence(data) {
	let escapeIndex = data.indexOf(ESCAPE);
	while (escapeIndex !== -1) {
		const sequenceStart = escapeIndex + 1;
		if (data.charCodeAt(sequenceStart) === 91 && BRACKETED_PASTE_MODE_SEQUENCE_RE.test(data.slice(sequenceStart, sequenceStart + BRACKETED_PASTE_MODE_SEQUENCE_SCAN_MAX))) return true;
		escapeIndex = data.indexOf(ESCAPE, escapeIndex + 1);
	}
	return false;
}
function sanitizeBracketedPasteText(text) {
	let escapeIndex = text.indexOf(ESCAPE);
	if (escapeIndex === -1) return text;
	let sanitized = "";
	let start = 0;
	while (escapeIndex !== -1) {
		sanitized += `${text.slice(start, escapeIndex)}\u241b`;
		start = escapeIndex + 1;
		escapeIndex = text.indexOf(ESCAPE, start);
	}
	return sanitized + text.slice(start);
}
function sanitizeTerminalPasteText(text) {
	return sanitizeBracketedPasteText(text);
}
function normalizeTerminalPasteLineEndings(text) {
	return text.replace(/\r?\n/g, "\r");
}
function wrapTerminalBracketedPasteText(text) {
	return `${BRACKETED_PASTE_START}${sanitizeBracketedPasteText(normalizeTerminalPasteLineEndings(text))}${BRACKETED_PASTE_END}`;
}
function forceBracketedPaste(terminal, text) {
	terminal.input(wrapTerminalBracketedPasteText(text));
}
function markTerminalBracketedPasteInterrupted(terminal) {
	if (terminal.modes.bracketedPasteMode) interruptedBracketedPasteTerminals.add(terminal);
}
function observeTerminalBracketedPasteModeOutput(terminal, data) {
	if (!interruptedBracketedPasteTerminals.has(terminal)) {
		bracketedPasteModeOutputTail.delete(terminal);
		return;
	}
	const combined = (bracketedPasteModeOutputTail.get(terminal) ?? "") + data;
	bracketedPasteModeOutputTail.set(terminal, combined.slice(-BRACKETED_PASTE_MODE_TAIL_MAX));
	if (hasBracketedPasteModeSequence(combined)) {
		interruptedBracketedPasteTerminals.delete(terminal);
		bracketedPasteModeOutputTail.delete(terminal);
	}
}
function pasteTerminalText(terminal, text, options) {
	if (options?.forceBracketedPaste) {
		forceBracketedPaste(terminal, text);
		return;
	}
	if (!interruptedBracketedPasteTerminals.has(terminal)) {
		terminal.paste(text);
		return;
	}
	if (!terminal.modes.bracketedPasteMode) {
		interruptedBracketedPasteTerminals.delete(terminal);
		bracketedPasteModeOutputTail.delete(terminal);
		terminal.paste(text);
		return;
	}
	if (LINE_BREAK_RE.test(text)) {
		terminal.paste(text);
		return;
	}
	const previousIgnoreBracketedPasteMode = terminal.options.ignoreBracketedPasteMode;
	terminal.options.ignoreBracketedPasteMode = true;
	try {
		terminal.paste(sanitizeTerminalPasteText(text));
	} finally {
		terminal.options.ignoreBracketedPasteMode = previousIgnoreBracketedPasteMode;
	}
}
var transactionTails = /* @__PURE__ */ new Map();
async function runTerminalPtyInputTransaction(ptyId, operation) {
	if (!ptyId) return await operation();
	const previous = transactionTails.get(ptyId);
	let release;
	const current = new Promise((resolve) => {
		release = resolve;
	});
	transactionTails.set(ptyId, current);
	if (previous) await previous;
	try {
		return await operation();
	} finally {
		release();
		if (transactionTails.get(ptyId) === current) transactionTails.delete(ptyId);
	}
}
export { normalizeTerminalPasteLineEndings as a, sanitizeBracketedPasteText as c, markTerminalBracketedPasteInterrupted as i, sanitizeTerminalPasteText as l, BRACKETED_PASTE_END as n, observeTerminalBracketedPasteModeOutput as o, BRACKETED_PASTE_START as r, pasteTerminalText as s, runTerminalPtyInputTransaction as t, wrapTerminalBracketedPasteText as u };
