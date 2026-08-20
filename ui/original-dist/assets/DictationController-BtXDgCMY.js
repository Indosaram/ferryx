import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as Button } from "./button-DszXJEV6.js";
import { t as Mic } from "./mic-BeATIGID.js";
import { t as Square } from "./square-P9Rbe3Cg.js";
import { cu as getUtf8ChunkEndIndex, no as yieldToEventLoop, t as useAppStore } from "./store-CgXrfmaH.js";
import { w as keybindingMatchesAction } from "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import { n as toast } from "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
import "./useMountedRef-1omUd-IV.js";
import { t as getShortcutPlatform } from "./shortcut-platform-BbPBGzth.js";
import { o as useShortcutKeyDetails } from "./useShortcutLabel-C-KRYtlB.js";
import { t as ShortcutKeyCombo } from "./ShortcutKeyCombo-Ch456Md0.js";
import { a as TEXT_CONTROL_PASTE_CHUNK_MAX_BYTES, n as measureTextControlPasteByteLengthWithYield, o as TEXT_CONTROL_PASTE_DIRECT_MAX_BYTES, r as pasteTextIntoTextControl, s as TEXT_CONTROL_PASTE_MAX_BYTES, t as measureTextControlPasteByteLength } from "./text-control-paste-PhBVbE2p.js";
import "./paste-payload-metadata-pr3nuODB.js";
import { n as dispatchDictationControl, t as DICTATION_CONTROL_EVENT } from "./dictation-control-events-B6Zx3vGw.js";
import { i as openMicrophoneCaptureStream } from "./microphone-devices-ipkzUAEd.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var MAX_BUFFERED_AUDIO_SECONDS = 30;
var MAX_BUFFERED_AUDIO_BYTES = 8 * 1024 * 1024;
function useAudioCapture() {
	const streamRef = (0, import_react.useRef)(null);
	const contextRef = (0, import_react.useRef)(null);
	const processorRef = (0, import_react.useRef)(null);
	const sourceRef = (0, import_react.useRef)(null);
	const isCapturingRef = (0, import_react.useRef)(false);
	const startRequestRef = (0, import_react.useRef)(0);
	const bufferAudioRef = (0, import_react.useRef)(false);
	const bufferedAudioGenerationRef = (0, import_react.useRef)(0);
	const bufferedAudioRef = (0, import_react.useRef)([]);
	const bufferedAudioBytesRef = (0, import_react.useRef)(0);
	const bufferedAudioSecondsRef = (0, import_react.useRef)(0);
	const capturedChunkCountRef = (0, import_react.useRef)(0);
	const sessionIdRef = (0, import_react.useRef)("desktop");
	const trackLostCleanupRef = (0, import_react.useRef)(null);
	const cleanupCaptureResources = (0, import_react.useCallback)(() => {
		trackLostCleanupRef.current?.();
		trackLostCleanupRef.current = null;
		processorRef.current?.disconnect();
		sourceRef.current?.disconnect();
		processorRef.current = null;
		sourceRef.current = null;
		if (contextRef.current?.state !== "closed") contextRef.current?.close();
		contextRef.current = null;
		streamRef.current?.getTracks().forEach((track) => track.stop());
		streamRef.current = null;
	}, []);
	const resetBufferedAudio = (0, import_react.useCallback)(() => {
		bufferedAudioGenerationRef.current += 1;
		bufferedAudioRef.current = [];
		bufferedAudioBytesRef.current = 0;
		bufferedAudioSecondsRef.current = 0;
	}, []);
	const removeOldestBufferedAudioChunk = (0, import_react.useCallback)(() => {
		const chunk = bufferedAudioRef.current.shift();
		if (!chunk) return;
		bufferedAudioBytesRef.current -= chunk.samples.byteLength;
		bufferedAudioSecondsRef.current -= chunk.samples.length / chunk.sampleRate;
	}, []);
	const appendBufferedAudioChunk = (0, import_react.useCallback)((chunk) => {
		bufferedAudioRef.current.push(chunk);
		bufferedAudioBytesRef.current += chunk.samples.byteLength;
		bufferedAudioSecondsRef.current += chunk.samples.length / chunk.sampleRate;
		while (bufferedAudioRef.current.length > 0 && (bufferedAudioBytesRef.current > MAX_BUFFERED_AUDIO_BYTES || bufferedAudioSecondsRef.current > MAX_BUFFERED_AUDIO_SECONDS)) removeOldestBufferedAudioChunk();
	}, [removeOldestBufferedAudioChunk]);
	const start = (0, import_react.useCallback)(async (options = {}) => {
		if (isCapturingRef.current) return;
		const startRequest = startRequestRef.current + 1;
		startRequestRef.current = startRequest;
		cleanupCaptureResources();
		sessionIdRef.current = options.sessionId ?? "desktop";
		bufferAudioRef.current = options.bufferAudio ?? false;
		resetBufferedAudio();
		capturedChunkCountRef.current = 0;
		const { stream, fellBackToDefaultMicrophone } = await openMicrophoneCaptureStream({
			preferredDeviceId: options.microphoneDeviceId,
			preferredDeviceLabel: options.microphoneDeviceLabel,
			getUserMedia: (constraints) => navigator.mediaDevices.getUserMedia(constraints),
			enumerateDevices: navigator.mediaDevices?.enumerateDevices ? () => navigator.mediaDevices.enumerateDevices() : void 0
		});
		if (startRequestRef.current !== startRequest) {
			stream.getTracks().forEach((track) => track.stop());
			return;
		}
		streamRef.current = stream;
		let context = null;
		let source = null;
		let processor = null;
		try {
			context = new AudioContext();
			contextRef.current = context;
			if (context.state === "suspended") await context.resume();
			if (startRequestRef.current !== startRequest || streamRef.current !== stream) {
				if (contextRef.current === context) contextRef.current = null;
				if (context.state !== "closed") context.close();
				if (streamRef.current === stream) streamRef.current = null;
				stream.getTracks().forEach((track) => track.stop());
				return;
			}
			source = context.createMediaStreamSource(stream);
			processor = context.createScriptProcessor(4096, 1, 1);
			const actualRate = context.sampleRate;
			processor.onaudioprocess = (e) => {
				if (!isCapturingRef.current || startRequestRef.current !== startRequest || processorRef.current !== processor) return;
				const samples = new Float32Array(e.inputBuffer.getChannelData(0));
				capturedChunkCountRef.current += 1;
				if (bufferAudioRef.current) {
					appendBufferedAudioChunk({
						samples,
						sampleRate: actualRate,
						sessionId: sessionIdRef.current
					});
					return;
				}
				window.api.speech.feedAudio(samples, actualRate, sessionIdRef.current).catch(() => void 0);
			};
			source.connect(processor);
			processor.connect(context.destination);
			processorRef.current = processor;
			sourceRef.current = source;
			isCapturingRef.current = true;
			const onCaptureLost = options.onCaptureLost;
			const audioTrack = stream.getAudioTracks()[0];
			if (onCaptureLost && audioTrack) {
				const handleTrackEnded = () => {
					if (startRequestRef.current !== startRequest || !isCapturingRef.current) return;
					onCaptureLost();
				};
				audioTrack.addEventListener("ended", handleTrackEnded);
				trackLostCleanupRef.current = () => {
					audioTrack.removeEventListener("ended", handleTrackEnded);
				};
			}
			return { fellBackToDefaultMicrophone };
		} catch (err) {
			processor?.disconnect();
			source?.disconnect();
			if (processorRef.current === processor) processorRef.current = null;
			if (sourceRef.current === source) sourceRef.current = null;
			if (contextRef.current === context) contextRef.current = null;
			if (context && context.state !== "closed") context.close();
			stream.getTracks().forEach((track) => track.stop());
			if (streamRef.current === stream) streamRef.current = null;
			if (startRequestRef.current === startRequest) {
				bufferAudioRef.current = false;
				resetBufferedAudio();
			}
			if (startRequestRef.current !== startRequest) return;
			throw err;
		}
	}, [
		appendBufferedAudioChunk,
		cleanupCaptureResources,
		resetBufferedAudio
	]);
	const flushBufferedAudio = (0, import_react.useCallback)(async () => {
		const flushGeneration = bufferedAudioGenerationRef.current;
		try {
			while (bufferedAudioGenerationRef.current === flushGeneration && bufferedAudioRef.current.length > 0) {
				const chunk = bufferedAudioRef.current[0];
				if (!chunk) break;
				removeOldestBufferedAudioChunk();
				await window.api.speech.feedAudio(chunk.samples, chunk.sampleRate, chunk.sessionId);
			}
		} finally {
			if (bufferedAudioGenerationRef.current === flushGeneration) {
				bufferAudioRef.current = false;
				resetBufferedAudio();
			}
		}
	}, [removeOldestBufferedAudioChunk, resetBufferedAudio]);
	const discardBufferedAudio = (0, import_react.useCallback)(() => {
		bufferAudioRef.current = false;
		resetBufferedAudio();
	}, [resetBufferedAudio]);
	const getCapturedChunkCount = (0, import_react.useCallback)(() => capturedChunkCountRef.current, []);
	return {
		start,
		stop: (0, import_react.useCallback)((options = {}) => {
			startRequestRef.current += 1;
			isCapturingRef.current = false;
			bufferAudioRef.current = false;
			if (!options.preserveBufferedAudio) resetBufferedAudio();
			cleanupCaptureResources();
		}, [cleanupCaptureResources, resetBufferedAudio]),
		flushBufferedAudio,
		discardBufferedAudio,
		getCapturedChunkCount,
		isCapturingRef
	};
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function DictationIndicator() {
	const dictationState = useAppStore((s) => s.dictationState);
	const partialTranscript = useAppStore((s) => s.partialTranscript);
	const isHoldMode = useAppStore((s) => s.settings?.voice?.dictationMode === "hold");
	const shortcut = useShortcutKeyDetails("voice.dictation");
	if (dictationState !== "listening" && dictationState !== "starting" && dictationState !== "stopping") return null;
	const label = dictationState === "starting" ? "Starting..." : dictationState === "stopping" ? "Processing..." : partialTranscript || "Listening...";
	const canStop = dictationState !== "stopping";
	const showShortcut = !isHoldMode && shortcut.keys.length > 0;
	const stopLabel = translate("auto.components.dictation.DictationIndicator.335e1bc6cb", "Stop dictation");
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "fixed bottom-12 left-1/2 z-50 flex max-w-[min(36rem,calc(100vw-3rem))] -translate-x-1/2 items-center gap-2 rounded-lg bg-foreground/90 px-3 py-1.5 text-background text-sm shadow-lg",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Mic, { className: `size-4 shrink-0 ${dictationState === "listening" ? "animate-pulse" : ""}` }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "min-w-0 truncate",
				children: label
			}),
			canStop ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				"aria-hidden": true,
				className: "h-3.5 w-px shrink-0 bg-background/25"
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
				asChild: true,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					type: "button",
					variant: "ghost",
					size: "icon-xs",
					"aria-label": stopLabel,
					className: "shrink-0 text-background/55 hover:bg-background/15 hover:text-background/85",
					onMouseDown: (event) => event.preventDefault(),
					onClick: () => dispatchDictationControl("stop"),
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Square, { className: "size-3 fill-current" })
				})
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(TooltipContent, {
				side: "top",
				sideOffset: 6,
				className: "flex items-center gap-1.5",
				children: [stopLabel, showShortcut ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ShortcutKeyCombo, {
					keys: shortcut.keys,
					doubleTap: shortcut.doubleTap,
					className: "gap-0.5",
					keyCapClassName: "min-w-0 border-background/20 bg-background/10 px-1 py-0 text-[10px] text-background shadow-none",
					separatorClassName: "text-[10px] text-background/70"
				}) : null]
			})] })] }) : null
		]
	});
}
function captureInsertionTarget() {
	const activeElement = document.activeElement;
	if (!activeElement) return null;
	if (activeElement.classList.contains("xterm-helper-textarea")) {
		const paneElement = activeElement.closest(".pane[data-pane-id]");
		const tabElement = activeElement.closest("[data-terminal-tab-id]");
		const paneId = Number(paneElement?.dataset.paneId);
		const tabId = tabElement?.dataset.terminalTabId;
		if (tabId && Number.isFinite(paneId)) return {
			kind: "terminal",
			tabId,
			paneId
		};
		return null;
	}
	if (activeElement instanceof HTMLInputElement || activeElement instanceof HTMLTextAreaElement) return {
		kind: "text",
		element: activeElement
	};
	if (activeElement instanceof HTMLElement && activeElement.isContentEditable) return {
		kind: "contentEditable",
		element: activeElement
	};
	return null;
}
function insertText(text, target) {
	if (target.kind === "terminal") {
		document.dispatchEvent(new CustomEvent("dictation:insertText", { detail: {
			text,
			tabId: target.tabId,
			paneId: target.paneId
		} }));
		return;
	}
	if (target.kind === "text") {
		const element = target.element;
		if (!element.isConnected) return;
		pasteTextIntoTextControl(element, text, {
			source: "programmatic",
			inputType: "insertText",
			canContinue: (candidate) => candidate.ownerDocument.activeElement === candidate
		}).catch(() => {});
		return;
	}
	if (target.kind === "contentEditable") insertTextIntoContentEditableTarget(target.element, text).catch(() => {});
}
function findClosestEditorElement(element) {
	return element.closest(".ProseMirror, [contenteditable=\"true\"]");
}
async function insertTextIntoContentEditableTarget(element, text) {
	const directByteLength = measureTextControlPasteByteLength(text, { stopAfterBytes: TEXT_CONTROL_PASTE_DIRECT_MAX_BYTES });
	if (directByteLength.byteLength === 0) return;
	if (!isContentEditableDictationTargetCurrent(element)) return;
	const editorElement = findClosestEditorElement(element) ?? element;
	if (!directByteLength.exceededLimit) {
		insertContentEditableDictationChunk(element, editorElement, text);
		return;
	}
	if ((await measureTextControlPasteByteLengthWithYield(text, { stopAfterBytes: 16777216 })).exceededLimit) return;
	let textIndex = 0;
	while (textIndex < text.length) {
		if (!isContentEditableDictationTargetCurrent(element)) return;
		const nextIndex = getUtf8ChunkEndIndex(text, textIndex, TEXT_CONTROL_PASTE_CHUNK_MAX_BYTES);
		if (!insertContentEditableDictationChunk(element, editorElement, text.slice(textIndex, nextIndex))) return;
		textIndex = nextIndex;
		if (textIndex < text.length) await yieldToEventLoop();
	}
}
function insertContentEditableDictationChunk(element, editorElement, text) {
	const beforeInput = new InputEvent("beforeinput", {
		bubbles: true,
		cancelable: true,
		inputType: "insertText",
		data: text
	});
	if (!editorElement.dispatchEvent(beforeInput)) return false;
	if (element.ownerDocument.execCommand?.("insertText", false, text) === true) return true;
	const selection = element.ownerDocument.getSelection();
	if (selection && selection.rangeCount > 0) {
		const range = selection.getRangeAt(0);
		range.deleteContents();
		const textNode = element.ownerDocument.createTextNode(text);
		range.insertNode(textNode);
		range.setStartAfter(textNode);
		range.collapse(true);
		selection.removeAllRanges();
		selection.addRange(range);
	}
	editorElement.dispatchEvent(new InputEvent("input", {
		bubbles: true,
		inputType: "insertText",
		data: text
	}));
	return true;
}
function isContentEditableDictationTargetCurrent(element) {
	return element.isConnected && element.contains(element.ownerDocument.activeElement);
}
var WORD_BOUNDARY_CHAR_RE = /^[\p{L}\p{N}]$/u;
var CJK_BOUNDARY_CHAR_RE = /^[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]$/u;
var NO_SPACE_BEFORE_CHAR_RE = /^[,.;:!?%。，、！？；：）)\]}]$/u;
var NO_SPACE_AFTER_CHAR_RE = /^[([{（《「『]$/u;
var SPACE_AFTER_CHAR_RE = /^[,.;:!?%]$/u;
function getFirstNonWhitespaceChar(text) {
	return Array.from(text.trimStart())[0] ?? "";
}
function getLastNonWhitespaceChar(text) {
	return Array.from(text.trimEnd()).at(-1) ?? "";
}
function shouldInsertSpaceBetweenFinalSegments(previousText, nextText) {
	if (!previousText || !nextText || /\s$/.test(previousText) || /^\s/.test(nextText)) return false;
	const previousChar = getLastNonWhitespaceChar(previousText);
	const nextChar = getFirstNonWhitespaceChar(nextText);
	if (!previousChar || !nextChar) return false;
	if (CJK_BOUNDARY_CHAR_RE.test(previousChar) || CJK_BOUNDARY_CHAR_RE.test(nextChar) || NO_SPACE_BEFORE_CHAR_RE.test(nextChar) || NO_SPACE_AFTER_CHAR_RE.test(previousChar)) return false;
	return (WORD_BOUNDARY_CHAR_RE.test(previousChar) || SPACE_AFTER_CHAR_RE.test(previousChar)) && WORD_BOUNDARY_CHAR_RE.test(nextChar);
}
function formatFinalTranscriptSegment(text, previousInsertedText) {
	if (shouldInsertSpaceBetweenFinalSegments(previousInsertedText, text)) return ` ${text}`;
	return text;
}
var STOPPED_SESSION_WAIT_MS = 1e3;
var MAX_EARLY_STOPPED_SESSION_IDS = 16;
function recordStoppedSession(sessionId, stoppedSessionIdsRef, stoppedResolversRef) {
	const resolver = stoppedResolversRef.current.get(sessionId);
	if (resolver) {
		stoppedResolversRef.current.delete(sessionId);
		resolver();
		return;
	}
	stoppedSessionIdsRef.current.delete(sessionId);
	stoppedSessionIdsRef.current.add(sessionId);
	while (stoppedSessionIdsRef.current.size > MAX_EARLY_STOPPED_SESSION_IDS) {
		const oldest = stoppedSessionIdsRef.current.values().next().value;
		if (!oldest) break;
		stoppedSessionIdsRef.current.delete(oldest);
	}
}
function waitForStoppedSession(sessionId, stoppedSessionIdsRef, stoppedResolversRef) {
	if (stoppedSessionIdsRef.current.delete(sessionId)) return Promise.resolve();
	return new Promise((resolve) => {
		const timeoutId = window.setTimeout(() => {
			stoppedResolversRef.current.delete(sessionId);
			resolve();
		}, STOPPED_SESSION_WAIT_MS);
		stoppedResolversRef.current.set(sessionId, () => {
			window.clearTimeout(timeoutId);
			resolve();
		});
	});
}
function openVoiceSettings() {
	useAppStore.getState().openSettingsTarget({
		pane: "voice",
		repoId: null
	});
	useAppStore.getState().openSettingsPage();
}
function showDictationStartErrorToast(message) {
	if (message.includes("Permission") || message.includes("NotAllowed")) toast.error(translate("auto.components.dictation.DictationController.2d5b9fabf9", "Microphone access denied. Grant access in system settings, then restart Orca."));
	else if (message.includes("not ready")) toast("Speech model not ready. Download it in Settings > Voice.");
	else if (message.includes("Unknown model")) toast("Selected model is no longer available. Please choose another in Settings > Voice.", { action: {
		label: translate("auto.components.dictation.DictationController.bb7f599ee7", "Open Settings"),
		onClick: openVoiceSettings
	} });
	else toast.error(translate("auto.components.dictation.DictationController.55127a3706", "Dictation failed: {{value0}}", { value0: message }));
}
var MODIFIER_KEYS_BY_NAME = {
	Alt: "alt",
	AltGraph: "alt",
	Control: "control",
	Ctrl: "control",
	Meta: "meta",
	OS: "meta",
	Shift: "shift"
};
var UNRELIABLE_KEY_VALUES = new Set([
	"",
	"Dead",
	"Unidentified"
]);
var UNRELIABLE_CODE_VALUES = new Set(["", "Unidentified"]);
function normalizeReleasedKey(key) {
	return key.length === 1 ? key.toLowerCase() : key;
}
function getReleasedModifier(event) {
	const byKey = MODIFIER_KEYS_BY_NAME[event.key];
	if (byKey) return byKey;
	if (event.code.startsWith("Alt")) return "alt";
	if (event.code.startsWith("Control")) return "control";
	if (event.code.startsWith("Meta")) return "meta";
	if (event.code.startsWith("Shift")) return "shift";
	return null;
}
function getReleasedPrimaryKey(event) {
	if (getReleasedModifier(event)) return null;
	const key = normalizeReleasedKey(event.key);
	return UNRELIABLE_KEY_VALUES.has(key) ? null : key;
}
function getReleasedPrimaryCode(event) {
	if (getReleasedModifier(event) || UNRELIABLE_CODE_VALUES.has(event.code)) return null;
	return event.code;
}
function createHoldDictationReleaseMatcher(event) {
	const primaryKey = getReleasedPrimaryKey(event);
	const primaryCode = getReleasedPrimaryCode(event);
	const heldModifiers = {
		alt: event.altKey,
		control: event.ctrlKey,
		meta: event.metaKey,
		shift: event.shiftKey
	};
	return (releaseEvent) => {
		const releasedModifier = getReleasedModifier(releaseEvent);
		if (releasedModifier) return heldModifiers[releasedModifier];
		const releasePrimaryCode = getReleasedPrimaryCode(releaseEvent);
		if (primaryCode !== null && releasePrimaryCode !== null) return releasePrimaryCode === primaryCode;
		return primaryKey !== null && getReleasedPrimaryKey(releaseEvent) === primaryKey;
	};
}
function useHoldDictationGesture({ dictationStateRef, holdGestureActiveRef, insertionTargetRef, intentionalTargetCancellationRef, keybindings, settings, startDictation, stopDictation }) {
	const releaseMatcherRef = (0, import_react.useRef)(null);
	(0, import_react.useEffect)(() => {
		if ((settings?.voice?.dictationMode ?? "toggle") !== "hold") return;
		const handleKeyDown = (e) => {
			if (keybindingMatchesAction("voice.dictation", e, getShortcutPlatform(), keybindings)) {
				if (!settings?.voice?.enabled || !settings.voice.sttModel) return;
				e.preventDefault();
				e.stopPropagation();
				holdGestureActiveRef.current = true;
				releaseMatcherRef.current = createHoldDictationReleaseMatcher(e);
				if (dictationStateRef.current === "idle") startDictation();
			}
		};
		const handleKeyUp = (e) => {
			if (!holdGestureActiveRef.current) return;
			if (!keybindingMatchesAction("voice.dictation", e, getShortcutPlatform(), keybindings) && releaseMatcherRef.current?.(e) !== true) return;
			releaseMatcherRef.current = null;
			if (dictationStateRef.current === "idle" || dictationStateRef.current === "stopping") {
				holdGestureActiveRef.current = false;
				return;
			}
			holdGestureActiveRef.current = false;
			stopDictation();
		};
		const handleBlur = () => {
			if (!holdGestureActiveRef.current) return;
			holdGestureActiveRef.current = false;
			releaseMatcherRef.current = null;
			if (dictationStateRef.current !== "idle" && dictationStateRef.current !== "stopping") {
				insertionTargetRef.current = null;
				intentionalTargetCancellationRef.current = true;
				stopDictation();
			}
		};
		const handleVisibilityChange = () => {
			if (document.visibilityState !== "visible") handleBlur();
		};
		window.addEventListener("keydown", handleKeyDown, true);
		window.addEventListener("keyup", handleKeyUp, true);
		window.addEventListener("blur", handleBlur);
		document.addEventListener("visibilitychange", handleVisibilityChange);
		return () => {
			handleBlur();
			window.removeEventListener("keydown", handleKeyDown, true);
			window.removeEventListener("keyup", handleKeyUp, true);
			window.removeEventListener("blur", handleBlur);
			document.removeEventListener("visibilitychange", handleVisibilityChange);
		};
	}, [
		settings?.voice?.dictationMode,
		settings?.voice?.enabled,
		settings?.voice?.sttModel,
		keybindings,
		startDictation,
		stopDictation,
		dictationStateRef,
		holdGestureActiveRef,
		insertionTargetRef,
		intentionalTargetCancellationRef
	]);
}
function DictationController() {
	const dictationState = useAppStore((s) => s.dictationState);
	const setDictationState = useAppStore((s) => s.setDictationState);
	const setPartialTranscript = useAppStore((s) => s.setPartialTranscript);
	const recordFeatureInteraction = useAppStore((s) => s.recordFeatureInteraction);
	const settings = useAppStore((s) => s.settings);
	const keybindings = useAppStore((s) => s.keybindings);
	const { start: startCapture, stop: stopCapture, flushBufferedAudio, discardBufferedAudio, getCapturedChunkCount } = useAudioCapture();
	const dictationStateRef = (0, import_react.useRef)(dictationState);
	dictationStateRef.current = dictationState;
	const dictationRunRef = (0, import_react.useRef)(0);
	const holdGestureActiveRef = (0, import_react.useRef)(false);
	const insertionTargetRef = (0, import_react.useRef)(null);
	const activeSessionIdRef = (0, import_react.useRef)(null);
	const stoppedSessionIdsRef = (0, import_react.useRef)(/* @__PURE__ */ new Set());
	const stoppedResolversRef = (0, import_react.useRef)(/* @__PURE__ */ new Map());
	const stopRequestedDuringStartRef = (0, import_react.useRef)(false);
	const finalTranscriptReceivedRef = (0, import_react.useRef)(false);
	const erroredSessionIdsRef = (0, import_react.useRef)(/* @__PURE__ */ new Set());
	const intentionalTargetCancellationRef = (0, import_react.useRef)(false);
	const insertedFinalTranscriptRef = (0, import_react.useRef)("");
	const micFallbackNotifiedForRef = (0, import_react.useRef)(null);
	const stopDictationRef = (0, import_react.useRef)(null);
	const drainStoppedSession = (0, import_react.useCallback)((sessionId) => {
		waitForStoppedSession(sessionId, stoppedSessionIdsRef, stoppedResolversRef);
	}, []);
	const finishDictationSession = (0, import_react.useCallback)(async (sessionId) => {
		dictationStateRef.current = "stopping";
		setDictationState("stopping");
		stopCapture();
		try {
			await window.api.speech.stopDictation(sessionId);
		} catch {}
		await waitForStoppedSession(sessionId, stoppedSessionIdsRef, stoppedResolversRef);
		if (!erroredSessionIdsRef.current.delete(sessionId) && !finalTranscriptReceivedRef.current && getCapturedChunkCount() > 0) toast.message(translate("auto.components.dictation.DictationController.5d2c3e7ae3", "No speech detected."));
		insertionTargetRef.current = null;
		finalTranscriptReceivedRef.current = false;
		insertedFinalTranscriptRef.current = "";
		intentionalTargetCancellationRef.current = false;
		stopRequestedDuringStartRef.current = false;
		if (activeSessionIdRef.current === sessionId) activeSessionIdRef.current = null;
		dictationStateRef.current = "idle";
		setDictationState("idle");
		setPartialTranscript("");
	}, [
		setDictationState,
		setPartialTranscript,
		stopCapture,
		getCapturedChunkCount
	]);
	const startDictation = (0, import_react.useCallback)(async () => {
		if (dictationStateRef.current !== "idle") return;
		const modelId = settings?.voice?.sttModel;
		if (!modelId) {
			toast("No speech model selected. Download one in Settings > Voice.", { action: {
				label: translate("auto.components.dictation.DictationController.bb7f599ee7", "Open Settings"),
				onClick: () => {
					useAppStore.getState().openSettingsTarget({
						pane: "voice",
						repoId: null
					});
					useAppStore.getState().openSettingsPage();
				}
			} });
			return;
		}
		if (!settings?.voice?.enabled) {
			toast("Voice dictation is disabled. Enable it in Settings > Voice.");
			return;
		}
		const runId = dictationRunRef.current + 1;
		const sessionId = String(runId);
		dictationRunRef.current = runId;
		activeSessionIdRef.current = sessionId;
		insertionTargetRef.current = captureInsertionTarget();
		stopRequestedDuringStartRef.current = false;
		finalTranscriptReceivedRef.current = false;
		erroredSessionIdsRef.current.clear();
		insertedFinalTranscriptRef.current = "";
		intentionalTargetCancellationRef.current = false;
		dictationStateRef.current = "starting";
		setDictationState("starting");
		let captureStarted = false;
		try {
			const preferredMicrophoneDeviceId = settings?.voice?.microphoneDeviceId ?? null;
			const captureResult = await startCapture({
				bufferAudio: true,
				sessionId,
				microphoneDeviceId: preferredMicrophoneDeviceId,
				microphoneDeviceLabel: settings?.voice?.microphoneDeviceLabel ?? null,
				onCaptureLost: () => {
					if (dictationRunRef.current !== runId) return;
					toast.message(translate("auto.components.dictation.DictationController.micDisconnected", "Microphone disconnected. Dictation stopped."));
					stopDictationRef.current?.();
				}
			});
			captureStarted = true;
			if (captureResult?.fellBackToDefaultMicrophone) {
				if (!stopRequestedDuringStartRef.current && micFallbackNotifiedForRef.current !== preferredMicrophoneDeviceId) {
					micFallbackNotifiedForRef.current = preferredMicrophoneDeviceId;
					toast.message(translate("auto.components.dictation.DictationController.micFallback", "Selected microphone unavailable. Using system default."));
				}
			} else micFallbackNotifiedForRef.current = null;
			if (stopRequestedDuringStartRef.current) stopCapture({ preserveBufferedAudio: true });
			if (dictationRunRef.current !== runId) {
				discardBufferedAudio();
				stopCapture();
				insertionTargetRef.current = null;
				return;
			}
			await window.api.speech.startDictation(modelId, void 0, sessionId);
			if (dictationRunRef.current !== runId) {
				discardBufferedAudio();
				insertionTargetRef.current = null;
				stopCapture();
				await window.api.speech.stopDictation(sessionId).catch(() => void 0);
				drainStoppedSession(sessionId);
				return;
			}
			await flushBufferedAudio();
			if (dictationRunRef.current !== runId) {
				discardBufferedAudio();
				insertionTargetRef.current = null;
				stopCapture();
				await window.api.speech.stopDictation(sessionId).catch(() => void 0);
				drainStoppedSession(sessionId);
				return;
			}
			if (stopRequestedDuringStartRef.current) {
				await finishDictationSession(sessionId);
				return;
			}
			dictationStateRef.current = "listening";
			setDictationState("listening");
			recordFeatureInteraction("voice-dictation");
		} catch (err) {
			if (dictationRunRef.current !== runId) return;
			await window.api.speech.stopDictation(sessionId).catch(() => void 0);
			drainStoppedSession(sessionId);
			if (captureStarted) stopCapture();
			discardBufferedAudio();
			const message = String(err);
			insertionTargetRef.current = null;
			intentionalTargetCancellationRef.current = false;
			stopRequestedDuringStartRef.current = false;
			finalTranscriptReceivedRef.current = false;
			erroredSessionIdsRef.current.clear();
			insertedFinalTranscriptRef.current = "";
			activeSessionIdRef.current = null;
			setPartialTranscript("");
			if (message.includes("dictation_canceled")) {
				dictationStateRef.current = "idle";
				setDictationState("idle");
				return;
			}
			dictationStateRef.current = "error";
			setDictationState("error");
			showDictationStartErrorToast(message);
			dictationStateRef.current = "idle";
			setDictationState("idle");
		}
	}, [
		settings,
		setDictationState,
		startCapture,
		flushBufferedAudio,
		discardBufferedAudio,
		stopCapture,
		finishDictationSession,
		drainStoppedSession,
		setPartialTranscript,
		recordFeatureInteraction
	]);
	const stopDictation = (0, import_react.useCallback)(async () => {
		if (dictationStateRef.current === "starting") {
			stopRequestedDuringStartRef.current = true;
			dictationStateRef.current = "stopping";
			setDictationState("stopping");
			stopCapture({ preserveBufferedAudio: true });
			return;
		}
		if (dictationStateRef.current !== "listening") return;
		const sessionId = activeSessionIdRef.current;
		if (!sessionId) return;
		await finishDictationSession(sessionId);
	}, [
		finishDictationSession,
		setDictationState,
		stopCapture
	]);
	stopDictationRef.current = () => void stopDictation();
	(0, import_react.useEffect)(() => {
		if ((settings?.voice?.dictationMode ?? "toggle") !== "toggle") return;
		const handleKeyDown = () => {
			if (!settings?.voice?.enabled || !settings.voice.sttModel || dictationStateRef.current === "stopping") return;
			if (dictationStateRef.current === "listening" || dictationStateRef.current === "starting") stopDictation();
			else startDictation();
		};
		return window.api.ui.onDictationKeyDown(handleKeyDown);
	}, [
		settings?.voice?.dictationMode,
		settings?.voice?.enabled,
		settings?.voice?.sttModel,
		startDictation,
		stopDictation
	]);
	(0, import_react.useEffect)(() => {
		const canDictate = () => Boolean(settings?.voice?.enabled && settings.voice.sttModel);
		const handleControl = (event) => {
			if (!canDictate() || dictationStateRef.current === "stopping") return;
			const action = event.detail;
			if (action === "start") {
				if (dictationStateRef.current === "idle") startDictation();
				return;
			}
			if (action === "stop") {
				if (dictationStateRef.current === "listening" || dictationStateRef.current === "starting") stopDictation();
				return;
			}
			if (dictationStateRef.current === "listening" || dictationStateRef.current === "starting") stopDictation();
			else startDictation();
		};
		document.addEventListener(DICTATION_CONTROL_EVENT, handleControl);
		return () => document.removeEventListener(DICTATION_CONTROL_EVENT, handleControl);
	}, [
		settings?.voice?.enabled,
		settings?.voice?.sttModel,
		startDictation,
		stopDictation
	]);
	useHoldDictationGesture({
		dictationStateRef,
		holdGestureActiveRef,
		insertionTargetRef,
		intentionalTargetCancellationRef,
		keybindings,
		settings,
		startDictation,
		stopDictation
	});
	(0, import_react.useEffect)(() => {
		const cleanupPartial = window.api.speech.onPartialTranscript((data) => {
			if (data.sessionId !== activeSessionIdRef.current) return;
			setPartialTranscript(data.text);
		});
		const cleanupFinal = window.api.speech.onFinalTranscript((data) => {
			if (data.sessionId !== activeSessionIdRef.current || !data.text) return;
			setPartialTranscript("");
			finalTranscriptReceivedRef.current = true;
			const target = insertionTargetRef.current;
			if (target) {
				const textToInsert = formatFinalTranscriptSegment(data.text, insertedFinalTranscriptRef.current);
				insertText(textToInsert, target);
				insertedFinalTranscriptRef.current += textToInsert;
			} else if (!intentionalTargetCancellationRef.current) toast.message(translate("auto.components.dictation.DictationController.7afff43472", "Dictation finished, but no text field was focused."));
		});
		const cleanupStopped = window.api.speech.onStopped((data) => {
			recordStoppedSession(data.sessionId, stoppedSessionIdsRef, stoppedResolversRef);
		});
		const cleanupError = window.api.speech.onError((data) => {
			if (data.sessionId !== activeSessionIdRef.current) return;
			const sessionId = data.sessionId;
			erroredSessionIdsRef.current.add(sessionId);
			dictationRunRef.current += 1;
			activeSessionIdRef.current = null;
			toast.error(translate("auto.components.dictation.DictationController.de136f1199", "Speech error: {{value0}}", { value0: data.error }));
			dictationStateRef.current = "stopping";
			setDictationState("stopping");
			stopCapture();
			discardBufferedAudio();
			(async () => {
				await window.api.speech.stopDictation(sessionId).catch(() => void 0);
				await waitForStoppedSession(sessionId, stoppedSessionIdsRef, stoppedResolversRef);
				insertionTargetRef.current = null;
				intentionalTargetCancellationRef.current = false;
				stopRequestedDuringStartRef.current = false;
				finalTranscriptReceivedRef.current = false;
				insertedFinalTranscriptRef.current = "";
				dictationStateRef.current = "idle";
				setDictationState("idle");
				setPartialTranscript("");
			})();
		});
		return () => {
			cleanupPartial();
			cleanupFinal();
			cleanupStopped();
			cleanupError();
		};
	}, [
		setPartialTranscript,
		setDictationState,
		stopCapture,
		discardBufferedAudio
	]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DictationIndicator, {});
}
export { DictationController };
