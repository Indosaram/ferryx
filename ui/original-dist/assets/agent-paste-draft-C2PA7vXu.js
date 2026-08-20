import { Bo as subscribeToRuntimeTerminalData, Oo as ensurePtyDispatcher, du as readUtf8CodePointAt, es as ptyDataSidecars, ku as getSettingsForWorktreeRuntimeOwner, no as yieldToEventLoop, su as getUtf8ByteLengthForCodePoint, t as useAppStore } from "./store-CgXrfmaH.js";
import { dt as TUI_AGENT_CONFIG, i as classifyTitleActivity, y as isShellProcess } from "./agent-status-3vUKbY6l.js";
import { c as isRemoteRuntimePtyId, n as isExpectedAgentProcess, s as inspectRuntimeTerminalProcess, u as sendRuntimePtyInputVerified } from "./agent-process-recognition-BB0O3DaN.js";
import { a as normalizeTerminalPasteLineEndings, l as sanitizeTerminalPasteText, n as BRACKETED_PASTE_END, r as BRACKETED_PASTE_START, t as runTerminalPtyInputTransaction, u as wrapTerminalBracketedPasteText } from "./terminal-pty-input-transaction-2UskR-Bm.js";
function agentDeliversDraftViaNativePrefill(agent, forcePaste) {
	if (forcePaste) return false;
	const agentConfig = agent ? TUI_AGENT_CONFIG[agent] : null;
	return Boolean(agentConfig?.draftPromptFlag || agentConfig?.draftPromptEnvVar);
}
var DEFAULT_TIMEOUT_MS = 5e3;
var POLL_INTERVAL_MS = 120;
function resolvePrimaryPtyId(tabId) {
	return useAppStore.getState().ptyIdsByTabId[tabId]?.[0] ?? null;
}
function titleSuggestsReady(tabId) {
	const state = useAppStore.getState();
	const paneTitles = state.runtimePaneTitlesByTabId[tabId];
	const titles = [];
	if (paneTitles) {
		for (const title of Object.values(paneTitles)) if (title) titles.push(title);
	}
	if (titles.length === 0) for (const tabs of Object.values(state.tabsByWorktree)) {
		const tab = tabs.find((t) => t.id === tabId);
		if (tab?.title) {
			titles.push(tab.title);
			break;
		}
	}
	return titles.some((title) => classifyTitleActivity(title) === "idle");
}
async function waitForAgentReady(tabId, expectedProcess, opts) {
	const timeoutMs = opts?.timeoutMs ?? DEFAULT_TIMEOUT_MS;
	const deadline = Date.now() + timeoutMs;
	let attempt = 0;
	while (Date.now() < deadline) {
		if (attempt > 0) await new Promise((resolve) => window.setTimeout(resolve, POLL_INTERVAL_MS));
		attempt += 1;
		if (titleSuggestsReady(tabId)) return {
			ready: true,
			reason: "title-idle"
		};
		const ptyId = resolvePrimaryPtyId(tabId);
		if (!ptyId) continue;
		try {
			const process = await inspectRuntimeTerminalProcess(useAppStore.getState().settings, ptyId);
			const foreground = process.foregroundProcess?.toLowerCase() ?? "";
			if (isExpectedAgentProcess(foreground, expectedProcess)) return {
				ready: true,
				reason: "foreground-match"
			};
			if (attempt >= 4 && !isShellProcess(foreground)) {
				if (process.hasChildProcesses) return {
					ready: true,
					reason: "child-process"
				};
			}
		} catch {}
	}
	return {
		ready: false,
		reason: "timeout"
	};
}
const AGENT_DRAFT_PASTE_CHUNK_MAX_BYTES = 16 * 1024;
var AGENT_DRAFT_PASTE_PREFLIGHT_YIELD_CODE_UNITS = 256 * 1024;
var AGENT_DRAFT_PASTE_ESCAPE_CODE_POINT = 27;
var AGENT_DRAFT_PASTE_INERT_ESCAPE_CODE_POINT = 9243;
var AGENT_DRAFT_PASTE_INERT_ESCAPE = "␛";
async function sendAgentDraftPasteContent(settings, ptyId, content, writePty) {
	return await runTerminalPtyInputTransaction(ptyId, () => sendAgentDraftPasteContentNow(settings, ptyId, content, writePty));
}
async function sendAgentDraftPasteContentNow(settings, ptyId, content, writePty) {
	if (content.length > 16777216) return false;
	const terminalContent = normalizeTerminalPasteLineEndings(content);
	if (!measureSanitizedUtf8ByteLength(terminalContent, { stopAfterBytes: 65536 }).exceededLimit) return await writeAgentDraftPtyInput(settings, ptyId, wrapTerminalBracketedPasteText(terminalContent), writePty);
	if (await isSanitizedDraftPasteOverLimit(terminalContent, 16777216)) return false;
	let bracketedPasteOpen = false;
	for (const chunk of iterateAgentDraftPasteContentChunks(terminalContent)) {
		let accepted = false;
		try {
			accepted = await writeAgentDraftPtyInput(settings, ptyId, chunk, writePty);
		} catch {
			if (bracketedPasteOpen && chunk !== BRACKETED_PASTE_END) await closeAgentDraftBracketedPaste(settings, ptyId, writePty);
			return false;
		}
		if (!accepted) {
			if (bracketedPasteOpen && chunk !== BRACKETED_PASTE_END) await closeAgentDraftBracketedPaste(settings, ptyId, writePty);
			return false;
		}
		if (chunk === BRACKETED_PASTE_START) bracketedPasteOpen = true;
		else if (chunk === BRACKETED_PASTE_END) bracketedPasteOpen = false;
	}
	return true;
}
function* iterateAgentDraftPasteContentChunks(content, maxChunkBytes = AGENT_DRAFT_PASTE_CHUNK_MAX_BYTES) {
	const safeMaxChunkBytes = Math.max(4, maxChunkBytes);
	yield BRACKETED_PASTE_START;
	const terminalContent = normalizeTerminalPasteLineEndings(content);
	let chunk = "";
	let chunkBytes = 0;
	for (let index = 0; index < terminalContent.length; index += 1) {
		const codePoint = readUtf8CodePointAt(terminalContent, index);
		const codeUnitLength = codePoint > 65535 ? 2 : 1;
		const sanitizedEscape = codePoint === AGENT_DRAFT_PASTE_ESCAPE_CODE_POINT;
		const sanitized = sanitizedEscape ? AGENT_DRAFT_PASTE_INERT_ESCAPE : terminalContent.slice(index, index + codeUnitLength);
		const characterBytes = getUtf8ByteLengthForCodePoint(sanitizedEscape ? AGENT_DRAFT_PASTE_INERT_ESCAPE_CODE_POINT : codePoint);
		if (chunk && chunkBytes + characterBytes > safeMaxChunkBytes) {
			yield chunk;
			chunk = sanitized;
			chunkBytes = characterBytes;
			continue;
		}
		chunk += sanitized;
		chunkBytes += characterBytes;
		if (codeUnitLength === 2) index += 1;
	}
	if (chunk) yield chunk;
	yield BRACKETED_PASTE_END;
}
function measureSanitizedUtf8ByteLength(content, options = {}) {
	let byteLength = 0;
	const stopAfterBytes = options.stopAfterBytes;
	for (let index = 0; index < content.length; index += 1) {
		const codePoint = readUtf8CodePointAt(content, index);
		byteLength += getSanitizedUtf8ByteLengthForCodePoint(codePoint);
		if (Number.isFinite(stopAfterBytes) && byteLength > (stopAfterBytes ?? 0)) return {
			byteLength,
			exceededLimit: true
		};
		if (codePoint > 65535) index += 1;
	}
	return {
		byteLength,
		exceededLimit: false
	};
}
async function isSanitizedDraftPasteOverLimit(content, maxBytes) {
	let byteLength = 0;
	let nextYieldAt = AGENT_DRAFT_PASTE_PREFLIGHT_YIELD_CODE_UNITS;
	for (let index = 0; index < content.length; index += 1) {
		const codePoint = readUtf8CodePointAt(content, index);
		byteLength += getSanitizedUtf8ByteLengthForCodePoint(codePoint);
		if (byteLength > maxBytes) return true;
		if (codePoint > 65535) index += 1;
		if (index >= nextYieldAt) {
			await yieldToEventLoop();
			nextYieldAt = index + AGENT_DRAFT_PASTE_PREFLIGHT_YIELD_CODE_UNITS;
		}
	}
	return false;
}
function getSanitizedUtf8ByteLengthForCodePoint(codePoint) {
	return getUtf8ByteLengthForCodePoint(codePoint === AGENT_DRAFT_PASTE_ESCAPE_CODE_POINT ? AGENT_DRAFT_PASTE_INERT_ESCAPE_CODE_POINT : codePoint);
}
async function writeAgentDraftPtyInput(settings, ptyId, data, writePty) {
	return writePty ? await writePty(data) : await sendRuntimePtyInputVerified(settings, ptyId, data);
}
async function closeAgentDraftBracketedPaste(settings, ptyId, writePty) {
	try {
		await writeAgentDraftPtyInput(settings, ptyId, BRACKETED_PASTE_END, writePty);
	} catch {}
}
var ptyDeliveryInterestRefCounts = /* @__PURE__ */ new Map();
function sendPtyDeliveryInterest(ptyId, interested) {
	globalThis.window?.api?.pty?.setPtyDeliveryInterest?.(ptyId, interested);
}
function acquirePtyDeliveryInterest(ptyId) {
	const next = (ptyDeliveryInterestRefCounts.get(ptyId) ?? 0) + 1;
	ptyDeliveryInterestRefCounts.set(ptyId, next);
	if (next === 1) sendPtyDeliveryInterest(ptyId, true);
	let released = false;
	return () => {
		if (released) return;
		released = true;
		const current = ptyDeliveryInterestRefCounts.get(ptyId) ?? 0;
		if (current <= 1) {
			ptyDeliveryInterestRefCounts.delete(ptyId);
			sendPtyDeliveryInterest(ptyId, false);
		} else ptyDeliveryInterestRefCounts.set(ptyId, current - 1);
	};
}
function subscribeToPtyData(ptyId, watcher) {
	ensurePtyDispatcher();
	const releaseDeliveryInterest = acquirePtyDeliveryInterest(ptyId);
	let set = ptyDataSidecars.get(ptyId);
	if (!set) {
		set = /* @__PURE__ */ new Set();
		ptyDataSidecars.set(ptyId, set);
	}
	set.add(watcher);
	return () => {
		releaseDeliveryInterest();
		const current = ptyDataSidecars.get(ptyId);
		if (!current) return;
		current.delete(watcher);
		if (current.size === 0) ptyDataSidecars.delete(ptyId);
	};
}
var DECSET_BRACKETED_PASTE = "\x1B[?2004h";
var CODEX_COMPOSER_PROMPT = "›";
var DECTCEM_SHOW_CURSOR = "\x1B[?25h";
var GROK_COMPOSER_PROMPT = "❯";
var DECSET_ALT_SCREEN = "\x1B[?1049h";
var DECRST_ALT_SCREEN = "\x1B[?1049l";
var DRAFT_PASTE_READY_SIGNALS = {
	"codex-composer-prompt": {
		markerAnchor: DECSET_BRACKETED_PASTE,
		markerAnchorEnd: null,
		marker: CODEX_COMPOSER_PROMPT,
		quietAnchor: null
	},
	"render-cursor-after-bracketed-paste": {
		markerAnchor: DECSET_BRACKETED_PASTE,
		markerAnchorEnd: null,
		marker: DECTCEM_SHOW_CURSOR,
		quietAnchor: null
	},
	"grok-composer-prompt": {
		markerAnchor: DECSET_ALT_SCREEN,
		markerAnchorEnd: DECRST_ALT_SCREEN,
		marker: GROK_COMPOSER_PROMPT,
		quietAnchor: DECSET_BRACKETED_PASTE
	},
	"render-quiet-after-bracketed-paste": {
		markerAnchor: null,
		markerAnchorEnd: null,
		marker: null,
		quietAnchor: DECSET_BRACKETED_PASTE
	}
};
var ANCHOR_CARRY_CHARS = 7;
function createDraftPasteReadyScanner(readySignal) {
	let recent = "";
	let postAnchorRecent = "";
	let anchorCarry = "";
	let sawMarkerAnchor = false;
	let sawQuietAnchor = false;
	const { markerAnchor, markerAnchorEnd, marker: signalMarker, quietAnchor } = DRAFT_PASTE_READY_SIGNALS[readySignal];
	const scanRevocableAnchorSegments = (window$1, anchor, end) => {
		let cursor = 0;
		while (cursor < window$1.length) {
			if (!sawMarkerAnchor) {
				const enterIndex = window$1.indexOf(anchor, cursor);
				if (enterIndex === -1) return false;
				sawMarkerAnchor = true;
				postAnchorRecent = "";
				cursor = enterIndex + anchor.length;
				continue;
			}
			const leaveIndex = window$1.indexOf(end, cursor);
			const segment = leaveIndex === -1 ? window$1.slice(cursor) : window$1.slice(cursor, leaveIndex);
			if ((postAnchorRecent + segment).includes(signalMarker ?? "")) return true;
			if (leaveIndex === -1) {
				postAnchorRecent = (postAnchorRecent + segment).slice(-512);
				return false;
			}
			sawMarkerAnchor = false;
			postAnchorRecent = "";
			cursor = leaveIndex + end.length;
		}
		return false;
	};
	return { observe(data) {
		const combined = recent + data;
		recent = combined.slice(-512);
		if (!sawQuietAnchor && quietAnchor !== null && combined.includes(quietAnchor)) sawQuietAnchor = true;
		if (signalMarker !== null && markerAnchor !== null) if (markerAnchorEnd !== null) {
			const window$1 = anchorCarry + data;
			anchorCarry = window$1.slice(-ANCHOR_CARRY_CHARS);
			if (scanRevocableAnchorSegments(window$1, markerAnchor, markerAnchorEnd)) return {
				ready: true,
				armQuietTimer: false
			};
		} else if (!sawMarkerAnchor) {
			const anchorIndex = combined.indexOf(markerAnchor);
			if (anchorIndex !== -1) {
				sawMarkerAnchor = true;
				const postAnchorChunk = combined.slice(anchorIndex + markerAnchor.length);
				if (postAnchorChunk.includes(signalMarker)) return {
					ready: true,
					armQuietTimer: false
				};
				postAnchorRecent = postAnchorChunk.slice(-512);
			}
		} else {
			if (data.includes(signalMarker) || (postAnchorRecent + data).includes(signalMarker)) return {
				ready: true,
				armQuietTimer: false
			};
			postAnchorRecent = (postAnchorRecent + data).slice(-512);
		}
		return {
			ready: false,
			armQuietTimer: sawQuietAnchor
		};
	} };
}
var BRACKETED_PASTE_QUIET_MS = 1500;
function waitForAgentDraftInputReady(ptyId, timeoutMs, readySignal, settings) {
	return new Promise((resolve) => {
		let settled = false;
		const scanner = createDraftPasteReadyScanner(readySignal);
		let quietTimer = null;
		let hardTimer = null;
		let unsubscribe = null;
		const finish = (value) => {
			if (settled) return;
			settled = true;
			if (hardTimer !== null) window.clearTimeout(hardTimer);
			if (quietTimer !== null) window.clearTimeout(quietTimer);
			unsubscribe?.();
			resolve(value);
		};
		const armQuietTimer = () => {
			if (quietTimer !== null) window.clearTimeout(quietTimer);
			quietTimer = window.setTimeout(() => finish(true), BRACKETED_PASTE_QUIET_MS);
		};
		const observeData = (data) => {
			const { ready, armQuietTimer: shouldArm } = scanner.observe(data);
			if (ready) {
				finish(true);
				return;
			}
			if (shouldArm) armQuietTimer();
		};
		if (isRemoteRuntimePtyId(ptyId)) subscribeToRuntimeTerminalData(settings, ptyId, `desktop:paste-ready:${ptyId}`, observeData).then((remoteUnsubscribe) => {
			if (settled) {
				remoteUnsubscribe();
				return;
			}
			unsubscribe = remoteUnsubscribe;
		}).catch(() => finish(false));
		else unsubscribe = subscribeToPtyData(ptyId, observeData);
		if (!settled) hardTimer = window.setTimeout(() => finish(false), timeoutMs);
	});
}
const BRACKETED_PASTE_BEGIN = BRACKETED_PASTE_START;
const POST_PASTE_SUBMIT_DELAY_MS = 50;
function sanitizeBracketedPasteContent(content) {
	return sanitizeTerminalPasteText(content);
}
var READINESS_TIMEOUT_MS = 8e3;
function getSettingsForAgentTabRuntimeOwner(tabId) {
	const store = useAppStore.getState();
	for (const [worktreeId, tabs] of Object.entries(store.tabsByWorktree ?? {})) if (tabs?.some((tab) => tab.id === tabId)) return getSettingsForWorktreeRuntimeOwner(store, worktreeId);
	return store.settings;
}
async function pasteDraftWhenAgentReady(args) {
	const { tabId, content, agent, submit, forcePaste, timeoutMs, onTimeout } = args;
	const agentConfig = agent ? TUI_AGENT_CONFIG[agent] : null;
	if (agentDeliversDraftViaNativePrefill(agent, forcePaste)) return false;
	const budget = timeoutMs ?? READINESS_TIMEOUT_MS;
	const readySignal = agentConfig?.draftPasteReadySignal ?? "render-quiet-after-bracketed-paste";
	const ptyId = await waitForPtyId(tabId, budget);
	if (!ptyId) {
		onTimeout?.();
		return false;
	}
	const settings = getSettingsForAgentTabRuntimeOwner(tabId);
	if (!await waitForAgentDraftInputReady(ptyId, budget, readySignal, settings)) {
		if (!(agentConfig ? await waitForAgentReady(tabId, agentConfig.expectedProcess, { timeoutMs: 1e3 }) : { ready: false }).ready) {
			onTimeout?.();
			return false;
		}
	}
	return await sendBracketedPasteToAgent({
		settings,
		ptyId,
		content,
		submit: submit === true
	});
}
async function pasteDraftToAgentPtyWhenReady(args) {
	const { tabId, ptyId, content, agent, submit, forcePaste, timeoutMs, onTimeout } = args;
	const agentConfig = agent ? TUI_AGENT_CONFIG[agent] : null;
	if (agentDeliversDraftViaNativePrefill(agent, forcePaste)) return false;
	const budget = timeoutMs ?? READINESS_TIMEOUT_MS;
	const settings = getSettingsForAgentTabRuntimeOwner(tabId);
	if (!await waitForAgentDraftInputReady(ptyId, budget, agentConfig?.draftPasteReadySignal ?? "render-quiet-after-bracketed-paste", settings)) {
		if (!(agentConfig ? await waitForExpectedAgentOnPty(ptyId, agentConfig.expectedProcess, 1e3, settings) : false)) {
			onTimeout?.();
			return false;
		}
	}
	return await sendBracketedPasteToAgent({
		settings,
		ptyId,
		content,
		submit: submit === true
	});
}
async function submitPromptToAgentPty(args) {
	return await sendBracketedPasteToAgent({
		settings: getSettingsForAgentTabRuntimeOwner(args.tabId),
		ptyId: args.ptyId,
		content: args.content,
		submit: true
	});
}
async function sendBracketedPasteToAgent(args) {
	const { settings = useAppStore.getState().settings, ptyId, content, submit } = args;
	try {
		return await runTerminalPtyInputTransaction(ptyId, async () => {
			const pasted = await sendAgentDraftPasteContentNow(settings, ptyId, content);
			if (!pasted || !submit) return pasted;
			await new Promise((resolve) => window.setTimeout(resolve, 50));
			return await sendRuntimePtyInputVerified(settings, ptyId, "\r");
		});
	} catch {
		return false;
	}
}
async function waitForPtyId(tabId, timeoutMs) {
	const deadline = Date.now() + timeoutMs;
	while (Date.now() < deadline) {
		const ptyId = useAppStore.getState().ptyIdsByTabId[tabId]?.[0];
		if (ptyId) return ptyId;
		await new Promise((resolve) => window.setTimeout(resolve, 50));
	}
	return null;
}
async function waitForExpectedAgentOnPty(ptyId, expectedProcess, timeoutMs, settings) {
	const deadline = Date.now() + timeoutMs;
	while (Date.now() < deadline) {
		try {
			const process = await withDeadline(inspectRuntimeTerminalProcess(settings, ptyId), Math.max(0, deadline - Date.now()));
			if (!process) return false;
			if (isExpectedAgentProcess(process.foregroundProcess?.toLowerCase() ?? "", expectedProcess)) return true;
		} catch {}
		const delayMs = Math.min(120, Math.max(0, deadline - Date.now()));
		if (delayMs > 0) await new Promise((resolve) => window.setTimeout(resolve, delayMs));
	}
	return false;
}
function withDeadline(promise, timeoutMs) {
	if (timeoutMs <= 0) return Promise.resolve(null);
	return new Promise((resolve, reject) => {
		const timer = window.setTimeout(() => resolve(null), timeoutMs);
		promise.then((value) => {
			window.clearTimeout(timer);
			resolve(value);
		}, (error) => {
			window.clearTimeout(timer);
			reject(error);
		});
	});
}
export { pasteDraftWhenAgentReady as a, createDraftPasteReadyScanner as c, agentDeliversDraftViaNativePrefill as d, pasteDraftToAgentPtyWhenReady as i, subscribeToPtyData as l, POST_PASTE_SUBMIT_DELAY_MS as n, sanitizeBracketedPasteContent as o, getSettingsForAgentTabRuntimeOwner as r, submitPromptToAgentPty as s, BRACKETED_PASTE_BEGIN as t, sendAgentDraftPasteContent as u };
