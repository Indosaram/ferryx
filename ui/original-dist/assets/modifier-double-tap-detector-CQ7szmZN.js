import { n as init_defineProperty, t as _defineProperty } from "./defineProperty-BAtR-r70.js";
init_defineProperty();
var DOUBLE_TAP_WINDOW_MS = 300;
var MODIFIER_BY_CODE = {
	ShiftLeft: "Shift",
	ShiftRight: "Shift",
	ControlLeft: "Ctrl",
	ControlRight: "Ctrl",
	AltLeft: "Alt",
	AltRight: "Alt",
	MetaLeft: "Cmd",
	MetaRight: "Cmd"
};
var MODIFIER_BY_KEY = {
	Shift: "Shift",
	Control: "Ctrl",
	Alt: "Alt",
	Meta: "Cmd"
};
function modifierFromKeyEvent(code, key) {
	if (code && MODIFIER_BY_CODE[code]) return MODIFIER_BY_CODE[code];
	return key ? MODIFIER_BY_KEY[key] ?? null : null;
}
function otherModifierHeld(event, modifier) {
	if (modifier !== "Shift" && event.shift) return true;
	if (modifier !== "Ctrl" && event.control) return true;
	if (modifier !== "Alt" && event.alt) return true;
	if (modifier !== "Cmd" && event.meta) return true;
	return false;
}
function toModifierDoubleTapEvent(event) {
	const modifier = modifierFromKeyEvent(event.code, event.key);
	return {
		type: event.type,
		modifier,
		isModifierOnly: modifier !== null && !otherModifierHeld(event, modifier),
		isAutoRepeat: Boolean(event.isAutoRepeat)
	};
}
var ModifierDoubleTapDetector = class {
	constructor() {
		_defineProperty(this, "state", { phase: "idle" });
	}
	process(event, timestampMs) {
		if (event.modifier === null || !event.isModifierOnly) {
			this.state = { phase: "idle" };
			return null;
		}
		if (event.type === "keyUp") {
			this.onModifierUp(event.modifier, timestampMs);
			return null;
		}
		return this.onModifierDown(event.modifier, event.isAutoRepeat, timestampMs);
	}
	reset() {
		this.state = { phase: "idle" };
	}
	onModifierDown(modifier, isAutoRepeat, timestampMs) {
		if (this.state.phase === "armed" && this.state.modifier === modifier && !isAutoRepeat && timestampMs <= this.state.deadlineMs) {
			this.state = { phase: "idle" };
			return { modifier };
		}
		if (isAutoRepeat) {
			this.state = { phase: "idle" };
			return null;
		}
		this.state = {
			phase: "down1",
			modifier
		};
		return null;
	}
	onModifierUp(modifier, timestampMs) {
		if (this.state.phase === "down1" && this.state.modifier === modifier) {
			this.state = {
				phase: "armed",
				modifier,
				deadlineMs: timestampMs + DOUBLE_TAP_WINDOW_MS
			};
			return;
		}
		if (this.state.phase === "armed" && this.state.modifier === modifier) this.state = { phase: "idle" };
	}
};
export { modifierFromKeyEvent as n, toModifierDoubleTapEvent as r, ModifierDoubleTapDetector as t };
