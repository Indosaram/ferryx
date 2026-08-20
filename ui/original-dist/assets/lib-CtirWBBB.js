import { o as __toESM, r as __export, t as __commonJSMin } from "./chunk-Dhmk_5SA.js";
function ok() {}
function unreachable() {}
function stringifyPosition(value) {
	if (!value || typeof value !== "object") return "";
	if ("position" in value || "type" in value) return position(value.position);
	if ("start" in value || "end" in value) return position(value);
	if ("line" in value || "column" in value) return point$1(value);
	return "";
}
function point$1(point$2) {
	return index(point$2 && point$2.line) + ":" + index(point$2 && point$2.column);
}
function position(pos) {
	return point$1(pos && pos.start) + "-" + point$1(pos && pos.end);
}
function index(value) {
	return value && typeof value === "number" ? value : 1;
}
var VFileMessage = class extends Error {
	constructor(causeOrReason, optionsOrParentOrPlace, origin) {
		super();
		if (typeof optionsOrParentOrPlace === "string") {
			origin = optionsOrParentOrPlace;
			optionsOrParentOrPlace = void 0;
		}
		let reason = "";
		let options = {};
		let legacyCause = false;
		if (optionsOrParentOrPlace) if ("line" in optionsOrParentOrPlace && "column" in optionsOrParentOrPlace) options = { place: optionsOrParentOrPlace };
		else if ("start" in optionsOrParentOrPlace && "end" in optionsOrParentOrPlace) options = { place: optionsOrParentOrPlace };
		else if ("type" in optionsOrParentOrPlace) options = {
			ancestors: [optionsOrParentOrPlace],
			place: optionsOrParentOrPlace.position
		};
		else options = { ...optionsOrParentOrPlace };
		if (typeof causeOrReason === "string") reason = causeOrReason;
		else if (!options.cause && causeOrReason) {
			legacyCause = true;
			reason = causeOrReason.message;
			options.cause = causeOrReason;
		}
		if (!options.ruleId && !options.source && typeof origin === "string") {
			const index$1 = origin.indexOf(":");
			if (index$1 === -1) options.ruleId = origin;
			else {
				options.source = origin.slice(0, index$1);
				options.ruleId = origin.slice(index$1 + 1);
			}
		}
		if (!options.place && options.ancestors && options.ancestors) {
			const parent = options.ancestors[options.ancestors.length - 1];
			if (parent) options.place = parent.position;
		}
		const start = options.place && "start" in options.place ? options.place.start : options.place;
		this.ancestors = options.ancestors || void 0;
		this.cause = options.cause || void 0;
		this.column = start ? start.column : void 0;
		this.fatal = void 0;
		this.file = "";
		this.message = reason;
		this.line = start ? start.line : void 0;
		this.name = stringifyPosition(options.place) || "1:1";
		this.place = options.place || void 0;
		this.reason = this.message;
		this.ruleId = options.ruleId || void 0;
		this.source = options.source || void 0;
		this.stack = legacyCause && options.cause && typeof options.cause.stack === "string" ? options.cause.stack : "";
		this.actual = void 0;
		this.expected = void 0;
		this.note = void 0;
		this.url = void 0;
	}
};
VFileMessage.prototype.file = "";
VFileMessage.prototype.name = "";
VFileMessage.prototype.reason = "";
VFileMessage.prototype.message = "";
VFileMessage.prototype.stack = "";
VFileMessage.prototype.column = void 0;
VFileMessage.prototype.line = void 0;
VFileMessage.prototype.ancestors = void 0;
VFileMessage.prototype.cause = void 0;
VFileMessage.prototype.fatal = void 0;
VFileMessage.prototype.place = void 0;
VFileMessage.prototype.ruleId = void 0;
VFileMessage.prototype.source = void 0;
var emptyOptions$1 = {};
function toString(value, options) {
	const settings = options || emptyOptions$1;
	return one(value, typeof settings.includeImageAlt === "boolean" ? settings.includeImageAlt : true, typeof settings.includeHtml === "boolean" ? settings.includeHtml : true);
}
function one(value, includeImageAlt, includeHtml) {
	if (node(value)) {
		if ("value" in value) return value.type === "html" && !includeHtml ? "" : value.value;
		if (includeImageAlt && "alt" in value && value.alt) return value.alt;
		if ("children" in value) return all(value.children, includeImageAlt, includeHtml);
	}
	if (Array.isArray(value)) return all(value, includeImageAlt, includeHtml);
	return "";
}
function all(values, includeImageAlt, includeHtml) {
	const result = [];
	let index$1 = -1;
	while (++index$1 < values.length) result[index$1] = one(values[index$1], includeImageAlt, includeHtml);
	return result.join("");
}
function node(value) {
	return Boolean(value && typeof value === "object");
}
var element = document.createElement("i");
function decodeNamedCharacterReference(value) {
	const characterReference$1 = "&" + value + ";";
	element.innerHTML = characterReference$1;
	const character = element.textContent;
	if (character.charCodeAt(character.length - 1) === 59 && value !== "semi") return false;
	return character === characterReference$1 ? false : character;
}
function splice(list$2, start, remove, items) {
	const end = list$2.length;
	let chunkStart = 0;
	let parameters;
	if (start < 0) start = -start > end ? 0 : end + start;
	else start = start > end ? end : start;
	remove = remove > 0 ? remove : 0;
	if (items.length < 1e4) {
		parameters = Array.from(items);
		parameters.unshift(start, remove);
		list$2.splice(...parameters);
	} else {
		if (remove) list$2.splice(start, remove);
		while (chunkStart < items.length) {
			parameters = items.slice(chunkStart, chunkStart + 1e4);
			parameters.unshift(start, 0);
			list$2.splice(...parameters);
			chunkStart += 1e4;
			start += 1e4;
		}
	}
}
function push(list$2, items) {
	if (list$2.length > 0) {
		splice(list$2, list$2.length, 0, items);
		return list$2;
	}
	return items;
}
var hasOwnProperty = {}.hasOwnProperty;
function combineExtensions(extensions) {
	const all$1 = {};
	let index$1 = -1;
	while (++index$1 < extensions.length) syntaxExtension(all$1, extensions[index$1]);
	return all$1;
}
function syntaxExtension(all$1, extension$1) {
	let hook;
	for (hook in extension$1) {
		const left = (hasOwnProperty.call(all$1, hook) ? all$1[hook] : void 0) || (all$1[hook] = {});
		const right = extension$1[hook];
		let code$2;
		if (right) for (code$2 in right) {
			if (!hasOwnProperty.call(left, code$2)) left[code$2] = [];
			const value = right[code$2];
			constructs(left[code$2], Array.isArray(value) ? value : value ? [value] : []);
		}
	}
}
function constructs(existing, list$2) {
	let index$1 = -1;
	const before = [];
	while (++index$1 < list$2.length) (list$2[index$1].add === "after" ? existing : before).push(list$2[index$1]);
	splice(existing, 0, 0, before);
}
function decodeNumericCharacterReference(value, base) {
	const code$2 = Number.parseInt(value, base);
	if (code$2 < 9 || code$2 === 11 || code$2 > 13 && code$2 < 32 || code$2 > 126 && code$2 < 160 || code$2 > 55295 && code$2 < 57344 || code$2 > 64975 && code$2 < 65008 || (code$2 & 65535) === 65535 || (code$2 & 65535) === 65534 || code$2 > 1114111) return "�";
	return String.fromCodePoint(code$2);
}
function normalizeIdentifier(value) {
	return value.replace(/[\t\n\r ]+/g, " ").replace(/^ | $/g, "").toLowerCase().toUpperCase();
}
const asciiAlpha = regexCheck(/[A-Za-z]/);
const asciiAlphanumeric = regexCheck(/[\dA-Za-z]/);
const asciiAtext = regexCheck(/[#-'*+\--9=?A-Z^-~]/);
function asciiControl(code$2) {
	return code$2 !== null && (code$2 < 32 || code$2 === 127);
}
const asciiDigit = regexCheck(/\d/);
const asciiHexDigit = regexCheck(/[\dA-Fa-f]/);
const asciiPunctuation = regexCheck(/[!-/:-@[-`{-~]/);
function markdownLineEnding(code$2) {
	return code$2 !== null && code$2 < -2;
}
function markdownLineEndingOrSpace(code$2) {
	return code$2 !== null && (code$2 < 0 || code$2 === 32);
}
function markdownSpace(code$2) {
	return code$2 === -2 || code$2 === -1 || code$2 === 32;
}
const unicodePunctuation = regexCheck(/\p{P}|\p{S}/u);
const unicodeWhitespace = regexCheck(/\s/);
function regexCheck(regex) {
	return check;
	function check(code$2) {
		return code$2 !== null && code$2 > -1 && regex.test(String.fromCharCode(code$2));
	}
}
function factorySpace(effects, ok$2, type, max) {
	const limit = max ? max - 1 : Number.POSITIVE_INFINITY;
	let size = 0;
	return start;
	function start(code$2) {
		if (markdownSpace(code$2)) {
			effects.enter(type);
			return prefix(code$2);
		}
		return ok$2(code$2);
	}
	function prefix(code$2) {
		if (markdownSpace(code$2) && size++ < limit) {
			effects.consume(code$2);
			return prefix;
		}
		effects.exit(type);
		return ok$2(code$2);
	}
}
const content = { tokenize: initializeContent };
function initializeContent(effects) {
	const contentStart = effects.attempt(this.parser.constructs.contentInitial, afterContentStartConstruct, paragraphInitial);
	let previous$2;
	return contentStart;
	function afterContentStartConstruct(code$2) {
		if (code$2 === null) {
			effects.consume(code$2);
			return;
		}
		effects.enter("lineEnding");
		effects.consume(code$2);
		effects.exit("lineEnding");
		return factorySpace(effects, contentStart, "linePrefix");
	}
	function paragraphInitial(code$2) {
		effects.enter("paragraph");
		return lineStart(code$2);
	}
	function lineStart(code$2) {
		const token = effects.enter("chunkText", {
			contentType: "text",
			previous: previous$2
		});
		if (previous$2) previous$2.next = token;
		previous$2 = token;
		return data(code$2);
	}
	function data(code$2) {
		if (code$2 === null) {
			effects.exit("chunkText");
			effects.exit("paragraph");
			effects.consume(code$2);
			return;
		}
		if (markdownLineEnding(code$2)) {
			effects.consume(code$2);
			effects.exit("chunkText");
			return lineStart;
		}
		effects.consume(code$2);
		return data;
	}
}
const document$1 = { tokenize: initializeDocument };
var containerConstruct = { tokenize: tokenizeContainer };
function initializeDocument(effects) {
	const self = this;
	const stack = [];
	let continued = 0;
	let childFlow;
	let childToken;
	let lineStartOffset;
	return start;
	function start(code$2) {
		if (continued < stack.length) {
			const item = stack[continued];
			self.containerState = item[1];
			return effects.attempt(item[0].continuation, documentContinue, checkNewContainers)(code$2);
		}
		return checkNewContainers(code$2);
	}
	function documentContinue(code$2) {
		continued++;
		if (self.containerState._closeFlow) {
			self.containerState._closeFlow = void 0;
			if (childFlow) closeFlow();
			const indexBeforeExits = self.events.length;
			let indexBeforeFlow = indexBeforeExits;
			let point$2;
			while (indexBeforeFlow--) if (self.events[indexBeforeFlow][0] === "exit" && self.events[indexBeforeFlow][1].type === "chunkFlow") {
				point$2 = self.events[indexBeforeFlow][1].end;
				break;
			}
			exitContainers(continued);
			let index$1 = indexBeforeExits;
			while (index$1 < self.events.length) {
				self.events[index$1][1].end = { ...point$2 };
				index$1++;
			}
			splice(self.events, indexBeforeFlow + 1, 0, self.events.slice(indexBeforeExits));
			self.events.length = index$1;
			return checkNewContainers(code$2);
		}
		return start(code$2);
	}
	function checkNewContainers(code$2) {
		if (continued === stack.length) {
			if (!childFlow) return documentContinued(code$2);
			if (childFlow.currentConstruct && childFlow.currentConstruct.concrete) return flowStart(code$2);
			self.interrupt = Boolean(childFlow.currentConstruct && !childFlow._gfmTableDynamicInterruptHack);
		}
		self.containerState = {};
		return effects.check(containerConstruct, thereIsANewContainer, thereIsNoNewContainer)(code$2);
	}
	function thereIsANewContainer(code$2) {
		if (childFlow) closeFlow();
		exitContainers(continued);
		return documentContinued(code$2);
	}
	function thereIsNoNewContainer(code$2) {
		self.parser.lazy[self.now().line] = continued !== stack.length;
		lineStartOffset = self.now().offset;
		return flowStart(code$2);
	}
	function documentContinued(code$2) {
		self.containerState = {};
		return effects.attempt(containerConstruct, containerContinue, flowStart)(code$2);
	}
	function containerContinue(code$2) {
		continued++;
		stack.push([self.currentConstruct, self.containerState]);
		return documentContinued(code$2);
	}
	function flowStart(code$2) {
		if (code$2 === null) {
			if (childFlow) closeFlow();
			exitContainers(0);
			effects.consume(code$2);
			return;
		}
		childFlow = childFlow || self.parser.flow(self.now());
		effects.enter("chunkFlow", {
			_tokenizer: childFlow,
			contentType: "flow",
			previous: childToken
		});
		return flowContinue(code$2);
	}
	function flowContinue(code$2) {
		if (code$2 === null) {
			writeToChild(effects.exit("chunkFlow"), true);
			exitContainers(0);
			effects.consume(code$2);
			return;
		}
		if (markdownLineEnding(code$2)) {
			effects.consume(code$2);
			writeToChild(effects.exit("chunkFlow"));
			continued = 0;
			self.interrupt = void 0;
			return start;
		}
		effects.consume(code$2);
		return flowContinue;
	}
	function writeToChild(token, endOfFile) {
		const stream = self.sliceStream(token);
		if (endOfFile) stream.push(null);
		token.previous = childToken;
		if (childToken) childToken.next = token;
		childToken = token;
		childFlow.defineSkip(token.start);
		childFlow.write(stream);
		if (self.parser.lazy[token.start.line]) {
			let index$1 = childFlow.events.length;
			while (index$1--) if (childFlow.events[index$1][1].start.offset < lineStartOffset && (!childFlow.events[index$1][1].end || childFlow.events[index$1][1].end.offset > lineStartOffset)) return;
			const indexBeforeExits = self.events.length;
			let indexBeforeFlow = indexBeforeExits;
			let seen;
			let point$2;
			while (indexBeforeFlow--) if (self.events[indexBeforeFlow][0] === "exit" && self.events[indexBeforeFlow][1].type === "chunkFlow") {
				if (seen) {
					point$2 = self.events[indexBeforeFlow][1].end;
					break;
				}
				seen = true;
			}
			exitContainers(continued);
			index$1 = indexBeforeExits;
			while (index$1 < self.events.length) {
				self.events[index$1][1].end = { ...point$2 };
				index$1++;
			}
			splice(self.events, indexBeforeFlow + 1, 0, self.events.slice(indexBeforeExits));
			self.events.length = index$1;
		}
	}
	function exitContainers(size) {
		let index$1 = stack.length;
		while (index$1-- > size) {
			const entry = stack[index$1];
			self.containerState = entry[1];
			entry[0].exit.call(self, effects);
		}
		stack.length = size;
	}
	function closeFlow() {
		childFlow.write([null]);
		childToken = void 0;
		childFlow = void 0;
		self.containerState._closeFlow = void 0;
	}
}
function tokenizeContainer(effects, ok$2, nok) {
	return factorySpace(effects, effects.attempt(this.parser.constructs.document, ok$2, nok), "linePrefix", this.parser.constructs.disable.null.includes("codeIndented") ? void 0 : 4);
}
function classifyCharacter(code$2) {
	if (code$2 === null || markdownLineEndingOrSpace(code$2) || unicodeWhitespace(code$2)) return 1;
	if (unicodePunctuation(code$2)) return 2;
}
function resolveAll(constructs$1, events, context) {
	const called = [];
	let index$1 = -1;
	while (++index$1 < constructs$1.length) {
		const resolve = constructs$1[index$1].resolveAll;
		if (resolve && !called.includes(resolve)) {
			events = resolve(events, context);
			called.push(resolve);
		}
	}
	return events;
}
const attention = {
	name: "attention",
	resolveAll: resolveAllAttention,
	tokenize: tokenizeAttention
};
function resolveAllAttention(events, context) {
	let index$1 = -1;
	let open;
	let group;
	let text$4;
	let openingSequence;
	let closingSequence;
	let use;
	let nextEvents;
	let offset;
	while (++index$1 < events.length) if (events[index$1][0] === "enter" && events[index$1][1].type === "attentionSequence" && events[index$1][1]._close) {
		open = index$1;
		while (open--) if (events[open][0] === "exit" && events[open][1].type === "attentionSequence" && events[open][1]._open && context.sliceSerialize(events[open][1]).charCodeAt(0) === context.sliceSerialize(events[index$1][1]).charCodeAt(0)) {
			if ((events[open][1]._close || events[index$1][1]._open) && (events[index$1][1].end.offset - events[index$1][1].start.offset) % 3 && !((events[open][1].end.offset - events[open][1].start.offset + events[index$1][1].end.offset - events[index$1][1].start.offset) % 3)) continue;
			use = events[open][1].end.offset - events[open][1].start.offset > 1 && events[index$1][1].end.offset - events[index$1][1].start.offset > 1 ? 2 : 1;
			const start = { ...events[open][1].end };
			const end = { ...events[index$1][1].start };
			movePoint(start, -use);
			movePoint(end, use);
			openingSequence = {
				type: use > 1 ? "strongSequence" : "emphasisSequence",
				start,
				end: { ...events[open][1].end }
			};
			closingSequence = {
				type: use > 1 ? "strongSequence" : "emphasisSequence",
				start: { ...events[index$1][1].start },
				end
			};
			text$4 = {
				type: use > 1 ? "strongText" : "emphasisText",
				start: { ...events[open][1].end },
				end: { ...events[index$1][1].start }
			};
			group = {
				type: use > 1 ? "strong" : "emphasis",
				start: { ...openingSequence.start },
				end: { ...closingSequence.end }
			};
			events[open][1].end = { ...openingSequence.start };
			events[index$1][1].start = { ...closingSequence.end };
			nextEvents = [];
			if (events[open][1].end.offset - events[open][1].start.offset) nextEvents = push(nextEvents, [[
				"enter",
				events[open][1],
				context
			], [
				"exit",
				events[open][1],
				context
			]]);
			nextEvents = push(nextEvents, [
				[
					"enter",
					group,
					context
				],
				[
					"enter",
					openingSequence,
					context
				],
				[
					"exit",
					openingSequence,
					context
				],
				[
					"enter",
					text$4,
					context
				]
			]);
			nextEvents = push(nextEvents, resolveAll(context.parser.constructs.insideSpan.null, events.slice(open + 1, index$1), context));
			nextEvents = push(nextEvents, [
				[
					"exit",
					text$4,
					context
				],
				[
					"enter",
					closingSequence,
					context
				],
				[
					"exit",
					closingSequence,
					context
				],
				[
					"exit",
					group,
					context
				]
			]);
			if (events[index$1][1].end.offset - events[index$1][1].start.offset) {
				offset = 2;
				nextEvents = push(nextEvents, [[
					"enter",
					events[index$1][1],
					context
				], [
					"exit",
					events[index$1][1],
					context
				]]);
			} else offset = 0;
			splice(events, open - 1, index$1 - open + 3, nextEvents);
			index$1 = open + nextEvents.length - offset - 2;
			break;
		}
	}
	index$1 = -1;
	while (++index$1 < events.length) if (events[index$1][1].type === "attentionSequence") events[index$1][1].type = "data";
	return events;
}
function tokenizeAttention(effects, ok$2) {
	const attentionMarkers$1 = this.parser.constructs.attentionMarkers.null;
	const previous$2 = this.previous;
	const before = classifyCharacter(previous$2);
	let marker;
	return start;
	function start(code$2) {
		marker = code$2;
		effects.enter("attentionSequence");
		return inside(code$2);
	}
	function inside(code$2) {
		if (code$2 === marker) {
			effects.consume(code$2);
			return inside;
		}
		const token = effects.exit("attentionSequence");
		const after = classifyCharacter(code$2);
		const open = !after || after === 2 && before || attentionMarkers$1.includes(code$2);
		const close = !before || before === 2 && after || attentionMarkers$1.includes(previous$2);
		token._open = Boolean(marker === 42 ? open : open && (before || !close));
		token._close = Boolean(marker === 42 ? close : close && (after || !open));
		return ok$2(code$2);
	}
}
function movePoint(point$2, offset) {
	point$2.column += offset;
	point$2.offset += offset;
	point$2._bufferIndex += offset;
}
const autolink = {
	name: "autolink",
	tokenize: tokenizeAutolink
};
function tokenizeAutolink(effects, ok$2, nok) {
	let size = 0;
	return start;
	function start(code$2) {
		effects.enter("autolink");
		effects.enter("autolinkMarker");
		effects.consume(code$2);
		effects.exit("autolinkMarker");
		effects.enter("autolinkProtocol");
		return open;
	}
	function open(code$2) {
		if (asciiAlpha(code$2)) {
			effects.consume(code$2);
			return schemeOrEmailAtext;
		}
		if (code$2 === 64) return nok(code$2);
		return emailAtext(code$2);
	}
	function schemeOrEmailAtext(code$2) {
		if (code$2 === 43 || code$2 === 45 || code$2 === 46 || asciiAlphanumeric(code$2)) {
			size = 1;
			return schemeInsideOrEmailAtext(code$2);
		}
		return emailAtext(code$2);
	}
	function schemeInsideOrEmailAtext(code$2) {
		if (code$2 === 58) {
			effects.consume(code$2);
			size = 0;
			return urlInside;
		}
		if ((code$2 === 43 || code$2 === 45 || code$2 === 46 || asciiAlphanumeric(code$2)) && size++ < 32) {
			effects.consume(code$2);
			return schemeInsideOrEmailAtext;
		}
		size = 0;
		return emailAtext(code$2);
	}
	function urlInside(code$2) {
		if (code$2 === 62) {
			effects.exit("autolinkProtocol");
			effects.enter("autolinkMarker");
			effects.consume(code$2);
			effects.exit("autolinkMarker");
			effects.exit("autolink");
			return ok$2;
		}
		if (code$2 === null || code$2 === 32 || code$2 === 60 || asciiControl(code$2)) return nok(code$2);
		effects.consume(code$2);
		return urlInside;
	}
	function emailAtext(code$2) {
		if (code$2 === 64) {
			effects.consume(code$2);
			return emailAtSignOrDot;
		}
		if (asciiAtext(code$2)) {
			effects.consume(code$2);
			return emailAtext;
		}
		return nok(code$2);
	}
	function emailAtSignOrDot(code$2) {
		return asciiAlphanumeric(code$2) ? emailLabel(code$2) : nok(code$2);
	}
	function emailLabel(code$2) {
		if (code$2 === 46) {
			effects.consume(code$2);
			size = 0;
			return emailAtSignOrDot;
		}
		if (code$2 === 62) {
			effects.exit("autolinkProtocol").type = "autolinkEmail";
			effects.enter("autolinkMarker");
			effects.consume(code$2);
			effects.exit("autolinkMarker");
			effects.exit("autolink");
			return ok$2;
		}
		return emailValue(code$2);
	}
	function emailValue(code$2) {
		if ((code$2 === 45 || asciiAlphanumeric(code$2)) && size++ < 63) {
			const next = code$2 === 45 ? emailValue : emailLabel;
			effects.consume(code$2);
			return next;
		}
		return nok(code$2);
	}
}
const blankLine = {
	partial: true,
	tokenize: tokenizeBlankLine
};
function tokenizeBlankLine(effects, ok$2, nok) {
	return start;
	function start(code$2) {
		return markdownSpace(code$2) ? factorySpace(effects, after, "linePrefix")(code$2) : after(code$2);
	}
	function after(code$2) {
		return code$2 === null || markdownLineEnding(code$2) ? ok$2(code$2) : nok(code$2);
	}
}
const blockQuote = {
	continuation: { tokenize: tokenizeBlockQuoteContinuation },
	exit: exit$1,
	name: "blockQuote",
	tokenize: tokenizeBlockQuoteStart
};
function tokenizeBlockQuoteStart(effects, ok$2, nok) {
	const self = this;
	return start;
	function start(code$2) {
		if (code$2 === 62) {
			const state = self.containerState;
			if (!state.open) {
				effects.enter("blockQuote", { _container: true });
				state.open = true;
			}
			effects.enter("blockQuotePrefix");
			effects.enter("blockQuoteMarker");
			effects.consume(code$2);
			effects.exit("blockQuoteMarker");
			return after;
		}
		return nok(code$2);
	}
	function after(code$2) {
		if (markdownSpace(code$2)) {
			effects.enter("blockQuotePrefixWhitespace");
			effects.consume(code$2);
			effects.exit("blockQuotePrefixWhitespace");
			effects.exit("blockQuotePrefix");
			return ok$2;
		}
		effects.exit("blockQuotePrefix");
		return ok$2(code$2);
	}
}
function tokenizeBlockQuoteContinuation(effects, ok$2, nok) {
	const self = this;
	return contStart;
	function contStart(code$2) {
		if (markdownSpace(code$2)) return factorySpace(effects, contBefore, "linePrefix", self.parser.constructs.disable.null.includes("codeIndented") ? void 0 : 4)(code$2);
		return contBefore(code$2);
	}
	function contBefore(code$2) {
		return effects.attempt(blockQuote, ok$2, nok)(code$2);
	}
}
function exit$1(effects) {
	effects.exit("blockQuote");
}
const characterEscape = {
	name: "characterEscape",
	tokenize: tokenizeCharacterEscape
};
function tokenizeCharacterEscape(effects, ok$2, nok) {
	return start;
	function start(code$2) {
		effects.enter("characterEscape");
		effects.enter("escapeMarker");
		effects.consume(code$2);
		effects.exit("escapeMarker");
		return inside;
	}
	function inside(code$2) {
		if (asciiPunctuation(code$2)) {
			effects.enter("characterEscapeValue");
			effects.consume(code$2);
			effects.exit("characterEscapeValue");
			effects.exit("characterEscape");
			return ok$2;
		}
		return nok(code$2);
	}
}
const characterReference = {
	name: "characterReference",
	tokenize: tokenizeCharacterReference
};
function tokenizeCharacterReference(effects, ok$2, nok) {
	const self = this;
	let size = 0;
	let max;
	let test;
	return start;
	function start(code$2) {
		effects.enter("characterReference");
		effects.enter("characterReferenceMarker");
		effects.consume(code$2);
		effects.exit("characterReferenceMarker");
		return open;
	}
	function open(code$2) {
		if (code$2 === 35) {
			effects.enter("characterReferenceMarkerNumeric");
			effects.consume(code$2);
			effects.exit("characterReferenceMarkerNumeric");
			return numeric;
		}
		effects.enter("characterReferenceValue");
		max = 31;
		test = asciiAlphanumeric;
		return value(code$2);
	}
	function numeric(code$2) {
		if (code$2 === 88 || code$2 === 120) {
			effects.enter("characterReferenceMarkerHexadecimal");
			effects.consume(code$2);
			effects.exit("characterReferenceMarkerHexadecimal");
			effects.enter("characterReferenceValue");
			max = 6;
			test = asciiHexDigit;
			return value;
		}
		effects.enter("characterReferenceValue");
		max = 7;
		test = asciiDigit;
		return value(code$2);
	}
	function value(code$2) {
		if (code$2 === 59 && size) {
			const token = effects.exit("characterReferenceValue");
			if (test === asciiAlphanumeric && !decodeNamedCharacterReference(self.sliceSerialize(token))) return nok(code$2);
			effects.enter("characterReferenceMarker");
			effects.consume(code$2);
			effects.exit("characterReferenceMarker");
			effects.exit("characterReference");
			return ok$2;
		}
		if (test(code$2) && size++ < max) {
			effects.consume(code$2);
			return value;
		}
		return nok(code$2);
	}
}
var nonLazyContinuation = {
	partial: true,
	tokenize: tokenizeNonLazyContinuation
};
const codeFenced = {
	concrete: true,
	name: "codeFenced",
	tokenize: tokenizeCodeFenced
};
function tokenizeCodeFenced(effects, ok$2, nok) {
	const self = this;
	const closeStart = {
		partial: true,
		tokenize: tokenizeCloseStart
	};
	let initialPrefix = 0;
	let sizeOpen = 0;
	let marker;
	return start;
	function start(code$2) {
		return beforeSequenceOpen(code$2);
	}
	function beforeSequenceOpen(code$2) {
		const tail = self.events[self.events.length - 1];
		initialPrefix = tail && tail[1].type === "linePrefix" ? tail[2].sliceSerialize(tail[1], true).length : 0;
		marker = code$2;
		effects.enter("codeFenced");
		effects.enter("codeFencedFence");
		effects.enter("codeFencedFenceSequence");
		return sequenceOpen(code$2);
	}
	function sequenceOpen(code$2) {
		if (code$2 === marker) {
			sizeOpen++;
			effects.consume(code$2);
			return sequenceOpen;
		}
		if (sizeOpen < 3) return nok(code$2);
		effects.exit("codeFencedFenceSequence");
		return markdownSpace(code$2) ? factorySpace(effects, infoBefore, "whitespace")(code$2) : infoBefore(code$2);
	}
	function infoBefore(code$2) {
		if (code$2 === null || markdownLineEnding(code$2)) {
			effects.exit("codeFencedFence");
			return self.interrupt ? ok$2(code$2) : effects.check(nonLazyContinuation, atNonLazyBreak, after)(code$2);
		}
		effects.enter("codeFencedFenceInfo");
		effects.enter("chunkString", { contentType: "string" });
		return info(code$2);
	}
	function info(code$2) {
		if (code$2 === null || markdownLineEnding(code$2)) {
			effects.exit("chunkString");
			effects.exit("codeFencedFenceInfo");
			return infoBefore(code$2);
		}
		if (markdownSpace(code$2)) {
			effects.exit("chunkString");
			effects.exit("codeFencedFenceInfo");
			return factorySpace(effects, metaBefore, "whitespace")(code$2);
		}
		if (code$2 === 96 && code$2 === marker) return nok(code$2);
		effects.consume(code$2);
		return info;
	}
	function metaBefore(code$2) {
		if (code$2 === null || markdownLineEnding(code$2)) return infoBefore(code$2);
		effects.enter("codeFencedFenceMeta");
		effects.enter("chunkString", { contentType: "string" });
		return meta(code$2);
	}
	function meta(code$2) {
		if (code$2 === null || markdownLineEnding(code$2)) {
			effects.exit("chunkString");
			effects.exit("codeFencedFenceMeta");
			return infoBefore(code$2);
		}
		if (code$2 === 96 && code$2 === marker) return nok(code$2);
		effects.consume(code$2);
		return meta;
	}
	function atNonLazyBreak(code$2) {
		return effects.attempt(closeStart, after, contentBefore)(code$2);
	}
	function contentBefore(code$2) {
		effects.enter("lineEnding");
		effects.consume(code$2);
		effects.exit("lineEnding");
		return contentStart;
	}
	function contentStart(code$2) {
		return initialPrefix > 0 && markdownSpace(code$2) ? factorySpace(effects, beforeContentChunk, "linePrefix", initialPrefix + 1)(code$2) : beforeContentChunk(code$2);
	}
	function beforeContentChunk(code$2) {
		if (code$2 === null || markdownLineEnding(code$2)) return effects.check(nonLazyContinuation, atNonLazyBreak, after)(code$2);
		effects.enter("codeFlowValue");
		return contentChunk(code$2);
	}
	function contentChunk(code$2) {
		if (code$2 === null || markdownLineEnding(code$2)) {
			effects.exit("codeFlowValue");
			return beforeContentChunk(code$2);
		}
		effects.consume(code$2);
		return contentChunk;
	}
	function after(code$2) {
		effects.exit("codeFenced");
		return ok$2(code$2);
	}
	function tokenizeCloseStart(effects$1, ok$3, nok$1) {
		let size = 0;
		return startBefore;
		function startBefore(code$2) {
			effects$1.enter("lineEnding");
			effects$1.consume(code$2);
			effects$1.exit("lineEnding");
			return start$1;
		}
		function start$1(code$2) {
			effects$1.enter("codeFencedFence");
			return markdownSpace(code$2) ? factorySpace(effects$1, beforeSequenceClose, "linePrefix", self.parser.constructs.disable.null.includes("codeIndented") ? void 0 : 4)(code$2) : beforeSequenceClose(code$2);
		}
		function beforeSequenceClose(code$2) {
			if (code$2 === marker) {
				effects$1.enter("codeFencedFenceSequence");
				return sequenceClose(code$2);
			}
			return nok$1(code$2);
		}
		function sequenceClose(code$2) {
			if (code$2 === marker) {
				size++;
				effects$1.consume(code$2);
				return sequenceClose;
			}
			if (size >= sizeOpen) {
				effects$1.exit("codeFencedFenceSequence");
				return markdownSpace(code$2) ? factorySpace(effects$1, sequenceCloseAfter, "whitespace")(code$2) : sequenceCloseAfter(code$2);
			}
			return nok$1(code$2);
		}
		function sequenceCloseAfter(code$2) {
			if (code$2 === null || markdownLineEnding(code$2)) {
				effects$1.exit("codeFencedFence");
				return ok$3(code$2);
			}
			return nok$1(code$2);
		}
	}
}
function tokenizeNonLazyContinuation(effects, ok$2, nok) {
	const self = this;
	return start;
	function start(code$2) {
		if (code$2 === null) return nok(code$2);
		effects.enter("lineEnding");
		effects.consume(code$2);
		effects.exit("lineEnding");
		return lineStart;
	}
	function lineStart(code$2) {
		return self.parser.lazy[self.now().line] ? nok(code$2) : ok$2(code$2);
	}
}
const codeIndented = {
	name: "codeIndented",
	tokenize: tokenizeCodeIndented
};
var furtherStart = {
	partial: true,
	tokenize: tokenizeFurtherStart
};
function tokenizeCodeIndented(effects, ok$2, nok) {
	const self = this;
	return start;
	function start(code$2) {
		effects.enter("codeIndented");
		return factorySpace(effects, afterPrefix, "linePrefix", 5)(code$2);
	}
	function afterPrefix(code$2) {
		const tail = self.events[self.events.length - 1];
		return tail && tail[1].type === "linePrefix" && tail[2].sliceSerialize(tail[1], true).length >= 4 ? atBreak(code$2) : nok(code$2);
	}
	function atBreak(code$2) {
		if (code$2 === null) return after(code$2);
		if (markdownLineEnding(code$2)) return effects.attempt(furtherStart, atBreak, after)(code$2);
		effects.enter("codeFlowValue");
		return inside(code$2);
	}
	function inside(code$2) {
		if (code$2 === null || markdownLineEnding(code$2)) {
			effects.exit("codeFlowValue");
			return atBreak(code$2);
		}
		effects.consume(code$2);
		return inside;
	}
	function after(code$2) {
		effects.exit("codeIndented");
		return ok$2(code$2);
	}
}
function tokenizeFurtherStart(effects, ok$2, nok) {
	const self = this;
	return furtherStart$1;
	function furtherStart$1(code$2) {
		if (self.parser.lazy[self.now().line]) return nok(code$2);
		if (markdownLineEnding(code$2)) {
			effects.enter("lineEnding");
			effects.consume(code$2);
			effects.exit("lineEnding");
			return furtherStart$1;
		}
		return factorySpace(effects, afterPrefix, "linePrefix", 5)(code$2);
	}
	function afterPrefix(code$2) {
		const tail = self.events[self.events.length - 1];
		return tail && tail[1].type === "linePrefix" && tail[2].sliceSerialize(tail[1], true).length >= 4 ? ok$2(code$2) : markdownLineEnding(code$2) ? furtherStart$1(code$2) : nok(code$2);
	}
}
const codeText = {
	name: "codeText",
	previous: previous$1,
	resolve: resolveCodeText,
	tokenize: tokenizeCodeText
};
function resolveCodeText(events) {
	let tailExitIndex = events.length - 4;
	let headEnterIndex = 3;
	let index$1;
	let enter;
	if ((events[headEnterIndex][1].type === "lineEnding" || events[headEnterIndex][1].type === "space") && (events[tailExitIndex][1].type === "lineEnding" || events[tailExitIndex][1].type === "space")) {
		index$1 = headEnterIndex;
		while (++index$1 < tailExitIndex) if (events[index$1][1].type === "codeTextData") {
			events[headEnterIndex][1].type = "codeTextPadding";
			events[tailExitIndex][1].type = "codeTextPadding";
			headEnterIndex += 2;
			tailExitIndex -= 2;
			break;
		}
	}
	index$1 = headEnterIndex - 1;
	tailExitIndex++;
	while (++index$1 <= tailExitIndex) if (enter === void 0) {
		if (index$1 !== tailExitIndex && events[index$1][1].type !== "lineEnding") enter = index$1;
	} else if (index$1 === tailExitIndex || events[index$1][1].type === "lineEnding") {
		events[enter][1].type = "codeTextData";
		if (index$1 !== enter + 2) {
			events[enter][1].end = events[index$1 - 1][1].end;
			events.splice(enter + 2, index$1 - enter - 2);
			tailExitIndex -= index$1 - enter - 2;
			index$1 = enter + 2;
		}
		enter = void 0;
	}
	return events;
}
function previous$1(code$2) {
	return code$2 !== 96 || this.events[this.events.length - 1][1].type === "characterEscape";
}
function tokenizeCodeText(effects, ok$2, nok) {
	let sizeOpen = 0;
	let size;
	let token;
	return start;
	function start(code$2) {
		effects.enter("codeText");
		effects.enter("codeTextSequence");
		return sequenceOpen(code$2);
	}
	function sequenceOpen(code$2) {
		if (code$2 === 96) {
			effects.consume(code$2);
			sizeOpen++;
			return sequenceOpen;
		}
		effects.exit("codeTextSequence");
		return between(code$2);
	}
	function between(code$2) {
		if (code$2 === null) return nok(code$2);
		if (code$2 === 32) {
			effects.enter("space");
			effects.consume(code$2);
			effects.exit("space");
			return between;
		}
		if (code$2 === 96) {
			token = effects.enter("codeTextSequence");
			size = 0;
			return sequenceClose(code$2);
		}
		if (markdownLineEnding(code$2)) {
			effects.enter("lineEnding");
			effects.consume(code$2);
			effects.exit("lineEnding");
			return between;
		}
		effects.enter("codeTextData");
		return data(code$2);
	}
	function data(code$2) {
		if (code$2 === null || code$2 === 32 || code$2 === 96 || markdownLineEnding(code$2)) {
			effects.exit("codeTextData");
			return between(code$2);
		}
		effects.consume(code$2);
		return data;
	}
	function sequenceClose(code$2) {
		if (code$2 === 96) {
			effects.consume(code$2);
			size++;
			return sequenceClose;
		}
		if (size === sizeOpen) {
			effects.exit("codeTextSequence");
			effects.exit("codeText");
			return ok$2(code$2);
		}
		token.type = "codeTextData";
		return data(code$2);
	}
}
var SpliceBuffer = class {
	constructor(initial) {
		this.left = initial ? [...initial] : [];
		this.right = [];
	}
	get(index$1) {
		if (index$1 < 0 || index$1 >= this.left.length + this.right.length) throw new RangeError("Cannot access index `" + index$1 + "` in a splice buffer of size `" + (this.left.length + this.right.length) + "`");
		if (index$1 < this.left.length) return this.left[index$1];
		return this.right[this.right.length - index$1 + this.left.length - 1];
	}
	get length() {
		return this.left.length + this.right.length;
	}
	shift() {
		this.setCursor(0);
		return this.right.pop();
	}
	slice(start, end) {
		const stop = end === null || end === void 0 ? Number.POSITIVE_INFINITY : end;
		if (stop < this.left.length) return this.left.slice(start, stop);
		if (start > this.left.length) return this.right.slice(this.right.length - stop + this.left.length, this.right.length - start + this.left.length).reverse();
		return this.left.slice(start).concat(this.right.slice(this.right.length - stop + this.left.length).reverse());
	}
	splice(start, deleteCount, items) {
		const count = deleteCount || 0;
		this.setCursor(Math.trunc(start));
		const removed = this.right.splice(this.right.length - count, Number.POSITIVE_INFINITY);
		if (items) chunkedPush(this.left, items);
		return removed.reverse();
	}
	pop() {
		this.setCursor(Number.POSITIVE_INFINITY);
		return this.left.pop();
	}
	push(item) {
		this.setCursor(Number.POSITIVE_INFINITY);
		this.left.push(item);
	}
	pushMany(items) {
		this.setCursor(Number.POSITIVE_INFINITY);
		chunkedPush(this.left, items);
	}
	unshift(item) {
		this.setCursor(0);
		this.right.push(item);
	}
	unshiftMany(items) {
		this.setCursor(0);
		chunkedPush(this.right, items.reverse());
	}
	setCursor(n) {
		if (n === this.left.length || n > this.left.length && this.right.length === 0 || n < 0 && this.left.length === 0) return;
		if (n < this.left.length) {
			const removed = this.left.splice(n, Number.POSITIVE_INFINITY);
			chunkedPush(this.right, removed.reverse());
		} else {
			const removed = this.right.splice(this.left.length + this.right.length - n, Number.POSITIVE_INFINITY);
			chunkedPush(this.left, removed.reverse());
		}
	}
};
function chunkedPush(list$2, right) {
	let chunkStart = 0;
	if (right.length < 1e4) list$2.push(...right);
	else while (chunkStart < right.length) {
		list$2.push(...right.slice(chunkStart, chunkStart + 1e4));
		chunkStart += 1e4;
	}
}
function subtokenize(eventsArray) {
	const jumps = {};
	let index$1 = -1;
	let event;
	let lineIndex;
	let otherIndex;
	let otherEvent;
	let parameters;
	let subevents;
	let more;
	const events = new SpliceBuffer(eventsArray);
	while (++index$1 < events.length) {
		while (index$1 in jumps) index$1 = jumps[index$1];
		event = events.get(index$1);
		if (index$1 && event[1].type === "chunkFlow" && events.get(index$1 - 1)[1].type === "listItemPrefix") {
			subevents = event[1]._tokenizer.events;
			otherIndex = 0;
			if (otherIndex < subevents.length && subevents[otherIndex][1].type === "lineEndingBlank") otherIndex += 2;
			if (otherIndex < subevents.length && subevents[otherIndex][1].type === "content") while (++otherIndex < subevents.length) {
				if (subevents[otherIndex][1].type === "content") break;
				if (subevents[otherIndex][1].type === "chunkText") {
					subevents[otherIndex][1]._isInFirstContentOfListItem = true;
					otherIndex++;
				}
			}
		}
		if (event[0] === "enter") {
			if (event[1].contentType) {
				Object.assign(jumps, subcontent(events, index$1));
				index$1 = jumps[index$1];
				more = true;
			}
		} else if (event[1]._container) {
			otherIndex = index$1;
			lineIndex = void 0;
			while (otherIndex--) {
				otherEvent = events.get(otherIndex);
				if (otherEvent[1].type === "lineEnding" || otherEvent[1].type === "lineEndingBlank") {
					if (otherEvent[0] === "enter") {
						if (lineIndex) events.get(lineIndex)[1].type = "lineEndingBlank";
						otherEvent[1].type = "lineEnding";
						lineIndex = otherIndex;
					}
				} else if (otherEvent[1].type === "linePrefix" || otherEvent[1].type === "listItemIndent") {} else break;
			}
			if (lineIndex) {
				event[1].end = { ...events.get(lineIndex)[1].start };
				parameters = events.slice(lineIndex, index$1);
				parameters.unshift(event);
				events.splice(lineIndex, index$1 - lineIndex + 1, parameters);
			}
		}
	}
	splice(eventsArray, 0, Number.POSITIVE_INFINITY, events.slice(0));
	return !more;
}
function subcontent(events, eventIndex) {
	const token = events.get(eventIndex)[1];
	const context = events.get(eventIndex)[2];
	let startPosition = eventIndex - 1;
	const startPositions = [];
	let tokenizer = token._tokenizer;
	if (!tokenizer) {
		tokenizer = context.parser[token.contentType](token.start);
		if (token._contentTypeTextTrailing) tokenizer._contentTypeTextTrailing = true;
	}
	const childEvents = tokenizer.events;
	const jumps = [];
	const gaps = {};
	let stream;
	let previous$2;
	let index$1 = -1;
	let current = token;
	let adjust = 0;
	let start = 0;
	const breaks = [start];
	while (current) {
		while (events.get(++startPosition)[1] !== current);
		startPositions.push(startPosition);
		if (!current._tokenizer) {
			stream = context.sliceStream(current);
			if (!current.next) stream.push(null);
			if (previous$2) tokenizer.defineSkip(current.start);
			if (current._isInFirstContentOfListItem) tokenizer._gfmTasklistFirstContentOfListItem = true;
			tokenizer.write(stream);
			if (current._isInFirstContentOfListItem) tokenizer._gfmTasklistFirstContentOfListItem = void 0;
		}
		previous$2 = current;
		current = current.next;
	}
	current = token;
	while (++index$1 < childEvents.length) if (childEvents[index$1][0] === "exit" && childEvents[index$1 - 1][0] === "enter" && childEvents[index$1][1].type === childEvents[index$1 - 1][1].type && childEvents[index$1][1].start.line !== childEvents[index$1][1].end.line) {
		start = index$1 + 1;
		breaks.push(start);
		current._tokenizer = void 0;
		current.previous = void 0;
		current = current.next;
	}
	tokenizer.events = [];
	if (current) {
		current._tokenizer = void 0;
		current.previous = void 0;
	} else breaks.pop();
	index$1 = breaks.length;
	while (index$1--) {
		const slice = childEvents.slice(breaks[index$1], breaks[index$1 + 1]);
		const start$1 = startPositions.pop();
		jumps.push([start$1, start$1 + slice.length - 1]);
		events.splice(start$1, 2, slice);
	}
	jumps.reverse();
	index$1 = -1;
	while (++index$1 < jumps.length) {
		gaps[adjust + jumps[index$1][0]] = adjust + jumps[index$1][1];
		adjust += jumps[index$1][1] - jumps[index$1][0] - 1;
	}
	return gaps;
}
const content$1 = {
	resolve: resolveContent,
	tokenize: tokenizeContent
};
var continuationConstruct = {
	partial: true,
	tokenize: tokenizeContinuation
};
function resolveContent(events) {
	subtokenize(events);
	return events;
}
function tokenizeContent(effects, ok$2) {
	let previous$2;
	return chunkStart;
	function chunkStart(code$2) {
		effects.enter("content");
		previous$2 = effects.enter("chunkContent", { contentType: "content" });
		return chunkInside(code$2);
	}
	function chunkInside(code$2) {
		if (code$2 === null) return contentEnd(code$2);
		if (markdownLineEnding(code$2)) return effects.check(continuationConstruct, contentContinue, contentEnd)(code$2);
		effects.consume(code$2);
		return chunkInside;
	}
	function contentEnd(code$2) {
		effects.exit("chunkContent");
		effects.exit("content");
		return ok$2(code$2);
	}
	function contentContinue(code$2) {
		effects.consume(code$2);
		effects.exit("chunkContent");
		previous$2.next = effects.enter("chunkContent", {
			contentType: "content",
			previous: previous$2
		});
		previous$2 = previous$2.next;
		return chunkInside;
	}
}
function tokenizeContinuation(effects, ok$2, nok) {
	const self = this;
	return startLookahead;
	function startLookahead(code$2) {
		effects.exit("chunkContent");
		effects.enter("lineEnding");
		effects.consume(code$2);
		effects.exit("lineEnding");
		return factorySpace(effects, prefixed, "linePrefix");
	}
	function prefixed(code$2) {
		if (code$2 === null || markdownLineEnding(code$2)) return nok(code$2);
		const tail = self.events[self.events.length - 1];
		if (!self.parser.constructs.disable.null.includes("codeIndented") && tail && tail[1].type === "linePrefix" && tail[2].sliceSerialize(tail[1], true).length >= 4) return ok$2(code$2);
		return effects.interrupt(self.parser.constructs.flow, nok, ok$2)(code$2);
	}
}
function factoryDestination(effects, ok$2, nok, type, literalType, literalMarkerType, rawType, stringType, max) {
	const limit = max || Number.POSITIVE_INFINITY;
	let balance = 0;
	return start;
	function start(code$2) {
		if (code$2 === 60) {
			effects.enter(type);
			effects.enter(literalType);
			effects.enter(literalMarkerType);
			effects.consume(code$2);
			effects.exit(literalMarkerType);
			return enclosedBefore;
		}
		if (code$2 === null || code$2 === 32 || code$2 === 41 || asciiControl(code$2)) return nok(code$2);
		effects.enter(type);
		effects.enter(rawType);
		effects.enter(stringType);
		effects.enter("chunkString", { contentType: "string" });
		return raw(code$2);
	}
	function enclosedBefore(code$2) {
		if (code$2 === 62) {
			effects.enter(literalMarkerType);
			effects.consume(code$2);
			effects.exit(literalMarkerType);
			effects.exit(literalType);
			effects.exit(type);
			return ok$2;
		}
		effects.enter(stringType);
		effects.enter("chunkString", { contentType: "string" });
		return enclosed(code$2);
	}
	function enclosed(code$2) {
		if (code$2 === 62) {
			effects.exit("chunkString");
			effects.exit(stringType);
			return enclosedBefore(code$2);
		}
		if (code$2 === null || code$2 === 60 || markdownLineEnding(code$2)) return nok(code$2);
		effects.consume(code$2);
		return code$2 === 92 ? enclosedEscape : enclosed;
	}
	function enclosedEscape(code$2) {
		if (code$2 === 60 || code$2 === 62 || code$2 === 92) {
			effects.consume(code$2);
			return enclosed;
		}
		return enclosed(code$2);
	}
	function raw(code$2) {
		if (!balance && (code$2 === null || code$2 === 41 || markdownLineEndingOrSpace(code$2))) {
			effects.exit("chunkString");
			effects.exit(stringType);
			effects.exit(rawType);
			effects.exit(type);
			return ok$2(code$2);
		}
		if (balance < limit && code$2 === 40) {
			effects.consume(code$2);
			balance++;
			return raw;
		}
		if (code$2 === 41) {
			effects.consume(code$2);
			balance--;
			return raw;
		}
		if (code$2 === null || code$2 === 32 || code$2 === 40 || asciiControl(code$2)) return nok(code$2);
		effects.consume(code$2);
		return code$2 === 92 ? rawEscape : raw;
	}
	function rawEscape(code$2) {
		if (code$2 === 40 || code$2 === 41 || code$2 === 92) {
			effects.consume(code$2);
			return raw;
		}
		return raw(code$2);
	}
}
function factoryLabel(effects, ok$2, nok, type, markerType, stringType) {
	const self = this;
	let size = 0;
	let seen;
	return start;
	function start(code$2) {
		effects.enter(type);
		effects.enter(markerType);
		effects.consume(code$2);
		effects.exit(markerType);
		effects.enter(stringType);
		return atBreak;
	}
	function atBreak(code$2) {
		if (size > 999 || code$2 === null || code$2 === 91 || code$2 === 93 && !seen || code$2 === 94 && !size && "_hiddenFootnoteSupport" in self.parser.constructs) return nok(code$2);
		if (code$2 === 93) {
			effects.exit(stringType);
			effects.enter(markerType);
			effects.consume(code$2);
			effects.exit(markerType);
			effects.exit(type);
			return ok$2;
		}
		if (markdownLineEnding(code$2)) {
			effects.enter("lineEnding");
			effects.consume(code$2);
			effects.exit("lineEnding");
			return atBreak;
		}
		effects.enter("chunkString", { contentType: "string" });
		return labelInside(code$2);
	}
	function labelInside(code$2) {
		if (code$2 === null || code$2 === 91 || code$2 === 93 || markdownLineEnding(code$2) || size++ > 999) {
			effects.exit("chunkString");
			return atBreak(code$2);
		}
		effects.consume(code$2);
		if (!seen) seen = !markdownSpace(code$2);
		return code$2 === 92 ? labelEscape : labelInside;
	}
	function labelEscape(code$2) {
		if (code$2 === 91 || code$2 === 92 || code$2 === 93) {
			effects.consume(code$2);
			size++;
			return labelInside;
		}
		return labelInside(code$2);
	}
}
function factoryTitle(effects, ok$2, nok, type, markerType, stringType) {
	let marker;
	return start;
	function start(code$2) {
		if (code$2 === 34 || code$2 === 39 || code$2 === 40) {
			effects.enter(type);
			effects.enter(markerType);
			effects.consume(code$2);
			effects.exit(markerType);
			marker = code$2 === 40 ? 41 : code$2;
			return begin;
		}
		return nok(code$2);
	}
	function begin(code$2) {
		if (code$2 === marker) {
			effects.enter(markerType);
			effects.consume(code$2);
			effects.exit(markerType);
			effects.exit(type);
			return ok$2;
		}
		effects.enter(stringType);
		return atBreak(code$2);
	}
	function atBreak(code$2) {
		if (code$2 === marker) {
			effects.exit(stringType);
			return begin(marker);
		}
		if (code$2 === null) return nok(code$2);
		if (markdownLineEnding(code$2)) {
			effects.enter("lineEnding");
			effects.consume(code$2);
			effects.exit("lineEnding");
			return factorySpace(effects, atBreak, "linePrefix");
		}
		effects.enter("chunkString", { contentType: "string" });
		return inside(code$2);
	}
	function inside(code$2) {
		if (code$2 === marker || code$2 === null || markdownLineEnding(code$2)) {
			effects.exit("chunkString");
			return atBreak(code$2);
		}
		effects.consume(code$2);
		return code$2 === 92 ? escape : inside;
	}
	function escape(code$2) {
		if (code$2 === marker || code$2 === 92) {
			effects.consume(code$2);
			return inside;
		}
		return inside(code$2);
	}
}
function factoryWhitespace(effects, ok$2) {
	let seen;
	return start;
	function start(code$2) {
		if (markdownLineEnding(code$2)) {
			effects.enter("lineEnding");
			effects.consume(code$2);
			effects.exit("lineEnding");
			seen = true;
			return start;
		}
		if (markdownSpace(code$2)) return factorySpace(effects, start, seen ? "linePrefix" : "lineSuffix")(code$2);
		return ok$2(code$2);
	}
}
const definition$1 = {
	name: "definition",
	tokenize: tokenizeDefinition
};
var titleBefore = {
	partial: true,
	tokenize: tokenizeTitleBefore
};
function tokenizeDefinition(effects, ok$2, nok) {
	const self = this;
	let identifier;
	return start;
	function start(code$2) {
		effects.enter("definition");
		return before(code$2);
	}
	function before(code$2) {
		return factoryLabel.call(self, effects, labelAfter, nok, "definitionLabel", "definitionLabelMarker", "definitionLabelString")(code$2);
	}
	function labelAfter(code$2) {
		identifier = normalizeIdentifier(self.sliceSerialize(self.events[self.events.length - 1][1]).slice(1, -1));
		if (code$2 === 58) {
			effects.enter("definitionMarker");
			effects.consume(code$2);
			effects.exit("definitionMarker");
			return markerAfter;
		}
		return nok(code$2);
	}
	function markerAfter(code$2) {
		return markdownLineEndingOrSpace(code$2) ? factoryWhitespace(effects, destinationBefore)(code$2) : destinationBefore(code$2);
	}
	function destinationBefore(code$2) {
		return factoryDestination(effects, destinationAfter, nok, "definitionDestination", "definitionDestinationLiteral", "definitionDestinationLiteralMarker", "definitionDestinationRaw", "definitionDestinationString")(code$2);
	}
	function destinationAfter(code$2) {
		return effects.attempt(titleBefore, after, after)(code$2);
	}
	function after(code$2) {
		return markdownSpace(code$2) ? factorySpace(effects, afterWhitespace, "whitespace")(code$2) : afterWhitespace(code$2);
	}
	function afterWhitespace(code$2) {
		if (code$2 === null || markdownLineEnding(code$2)) {
			effects.exit("definition");
			self.parser.defined.push(identifier);
			return ok$2(code$2);
		}
		return nok(code$2);
	}
}
function tokenizeTitleBefore(effects, ok$2, nok) {
	return titleBefore$1;
	function titleBefore$1(code$2) {
		return markdownLineEndingOrSpace(code$2) ? factoryWhitespace(effects, beforeMarker)(code$2) : nok(code$2);
	}
	function beforeMarker(code$2) {
		return factoryTitle(effects, titleAfter, nok, "definitionTitle", "definitionTitleMarker", "definitionTitleString")(code$2);
	}
	function titleAfter(code$2) {
		return markdownSpace(code$2) ? factorySpace(effects, titleAfterOptionalWhitespace, "whitespace")(code$2) : titleAfterOptionalWhitespace(code$2);
	}
	function titleAfterOptionalWhitespace(code$2) {
		return code$2 === null || markdownLineEnding(code$2) ? ok$2(code$2) : nok(code$2);
	}
}
const hardBreakEscape = {
	name: "hardBreakEscape",
	tokenize: tokenizeHardBreakEscape
};
function tokenizeHardBreakEscape(effects, ok$2, nok) {
	return start;
	function start(code$2) {
		effects.enter("hardBreakEscape");
		effects.consume(code$2);
		return after;
	}
	function after(code$2) {
		if (markdownLineEnding(code$2)) {
			effects.exit("hardBreakEscape");
			return ok$2(code$2);
		}
		return nok(code$2);
	}
}
const headingAtx = {
	name: "headingAtx",
	resolve: resolveHeadingAtx,
	tokenize: tokenizeHeadingAtx
};
function resolveHeadingAtx(events, context) {
	let contentEnd = events.length - 2;
	let contentStart = 3;
	let content$2;
	let text$4;
	if (events[contentStart][1].type === "whitespace") contentStart += 2;
	if (contentEnd - 2 > contentStart && events[contentEnd][1].type === "whitespace") contentEnd -= 2;
	if (events[contentEnd][1].type === "atxHeadingSequence" && (contentStart === contentEnd - 1 || contentEnd - 4 > contentStart && events[contentEnd - 2][1].type === "whitespace")) contentEnd -= contentStart + 1 === contentEnd ? 2 : 4;
	if (contentEnd > contentStart) {
		content$2 = {
			type: "atxHeadingText",
			start: events[contentStart][1].start,
			end: events[contentEnd][1].end
		};
		text$4 = {
			type: "chunkText",
			start: events[contentStart][1].start,
			end: events[contentEnd][1].end,
			contentType: "text"
		};
		splice(events, contentStart, contentEnd - contentStart + 1, [
			[
				"enter",
				content$2,
				context
			],
			[
				"enter",
				text$4,
				context
			],
			[
				"exit",
				text$4,
				context
			],
			[
				"exit",
				content$2,
				context
			]
		]);
	}
	return events;
}
function tokenizeHeadingAtx(effects, ok$2, nok) {
	let size = 0;
	return start;
	function start(code$2) {
		effects.enter("atxHeading");
		return before(code$2);
	}
	function before(code$2) {
		effects.enter("atxHeadingSequence");
		return sequenceOpen(code$2);
	}
	function sequenceOpen(code$2) {
		if (code$2 === 35 && size++ < 6) {
			effects.consume(code$2);
			return sequenceOpen;
		}
		if (code$2 === null || markdownLineEndingOrSpace(code$2)) {
			effects.exit("atxHeadingSequence");
			return atBreak(code$2);
		}
		return nok(code$2);
	}
	function atBreak(code$2) {
		if (code$2 === 35) {
			effects.enter("atxHeadingSequence");
			return sequenceFurther(code$2);
		}
		if (code$2 === null || markdownLineEnding(code$2)) {
			effects.exit("atxHeading");
			return ok$2(code$2);
		}
		if (markdownSpace(code$2)) return factorySpace(effects, atBreak, "whitespace")(code$2);
		effects.enter("atxHeadingText");
		return data(code$2);
	}
	function sequenceFurther(code$2) {
		if (code$2 === 35) {
			effects.consume(code$2);
			return sequenceFurther;
		}
		effects.exit("atxHeadingSequence");
		return atBreak(code$2);
	}
	function data(code$2) {
		if (code$2 === null || code$2 === 35 || markdownLineEndingOrSpace(code$2)) {
			effects.exit("atxHeadingText");
			return atBreak(code$2);
		}
		effects.consume(code$2);
		return data;
	}
}
const htmlBlockNames = [
	"address",
	"article",
	"aside",
	"base",
	"basefont",
	"blockquote",
	"body",
	"caption",
	"center",
	"col",
	"colgroup",
	"dd",
	"details",
	"dialog",
	"dir",
	"div",
	"dl",
	"dt",
	"fieldset",
	"figcaption",
	"figure",
	"footer",
	"form",
	"frame",
	"frameset",
	"h1",
	"h2",
	"h3",
	"h4",
	"h5",
	"h6",
	"head",
	"header",
	"hr",
	"html",
	"iframe",
	"legend",
	"li",
	"link",
	"main",
	"menu",
	"menuitem",
	"nav",
	"noframes",
	"ol",
	"optgroup",
	"option",
	"p",
	"param",
	"search",
	"section",
	"summary",
	"table",
	"tbody",
	"td",
	"tfoot",
	"th",
	"thead",
	"title",
	"tr",
	"track",
	"ul"
];
const htmlRawNames = [
	"pre",
	"script",
	"style",
	"textarea"
];
const htmlFlow = {
	concrete: true,
	name: "htmlFlow",
	resolveTo: resolveToHtmlFlow,
	tokenize: tokenizeHtmlFlow
};
var blankLineBefore = {
	partial: true,
	tokenize: tokenizeBlankLineBefore
};
var nonLazyContinuationStart = {
	partial: true,
	tokenize: tokenizeNonLazyContinuationStart
};
function resolveToHtmlFlow(events) {
	let index$1 = events.length;
	while (index$1--) if (events[index$1][0] === "enter" && events[index$1][1].type === "htmlFlow") break;
	if (index$1 > 1 && events[index$1 - 2][1].type === "linePrefix") {
		events[index$1][1].start = events[index$1 - 2][1].start;
		events[index$1 + 1][1].start = events[index$1 - 2][1].start;
		events.splice(index$1 - 2, 2);
	}
	return events;
}
function tokenizeHtmlFlow(effects, ok$2, nok) {
	const self = this;
	let marker;
	let closingTag;
	let buffer;
	let index$1;
	let markerB;
	return start;
	function start(code$2) {
		return before(code$2);
	}
	function before(code$2) {
		effects.enter("htmlFlow");
		effects.enter("htmlFlowData");
		effects.consume(code$2);
		return open;
	}
	function open(code$2) {
		if (code$2 === 33) {
			effects.consume(code$2);
			return declarationOpen;
		}
		if (code$2 === 47) {
			effects.consume(code$2);
			closingTag = true;
			return tagCloseStart;
		}
		if (code$2 === 63) {
			effects.consume(code$2);
			marker = 3;
			return self.interrupt ? ok$2 : continuationDeclarationInside;
		}
		if (asciiAlpha(code$2)) {
			effects.consume(code$2);
			buffer = String.fromCharCode(code$2);
			return tagName;
		}
		return nok(code$2);
	}
	function declarationOpen(code$2) {
		if (code$2 === 45) {
			effects.consume(code$2);
			marker = 2;
			return commentOpenInside;
		}
		if (code$2 === 91) {
			effects.consume(code$2);
			marker = 5;
			index$1 = 0;
			return cdataOpenInside;
		}
		if (asciiAlpha(code$2)) {
			effects.consume(code$2);
			marker = 4;
			return self.interrupt ? ok$2 : continuationDeclarationInside;
		}
		return nok(code$2);
	}
	function commentOpenInside(code$2) {
		if (code$2 === 45) {
			effects.consume(code$2);
			return self.interrupt ? ok$2 : continuationDeclarationInside;
		}
		return nok(code$2);
	}
	function cdataOpenInside(code$2) {
		if (code$2 === "CDATA[".charCodeAt(index$1++)) {
			effects.consume(code$2);
			if (index$1 === 6) return self.interrupt ? ok$2 : continuation;
			return cdataOpenInside;
		}
		return nok(code$2);
	}
	function tagCloseStart(code$2) {
		if (asciiAlpha(code$2)) {
			effects.consume(code$2);
			buffer = String.fromCharCode(code$2);
			return tagName;
		}
		return nok(code$2);
	}
	function tagName(code$2) {
		if (code$2 === null || code$2 === 47 || code$2 === 62 || markdownLineEndingOrSpace(code$2)) {
			const slash = code$2 === 47;
			const name = buffer.toLowerCase();
			if (!slash && !closingTag && htmlRawNames.includes(name)) {
				marker = 1;
				return self.interrupt ? ok$2(code$2) : continuation(code$2);
			}
			if (htmlBlockNames.includes(buffer.toLowerCase())) {
				marker = 6;
				if (slash) {
					effects.consume(code$2);
					return basicSelfClosing;
				}
				return self.interrupt ? ok$2(code$2) : continuation(code$2);
			}
			marker = 7;
			return self.interrupt && !self.parser.lazy[self.now().line] ? nok(code$2) : closingTag ? completeClosingTagAfter(code$2) : completeAttributeNameBefore(code$2);
		}
		if (code$2 === 45 || asciiAlphanumeric(code$2)) {
			effects.consume(code$2);
			buffer += String.fromCharCode(code$2);
			return tagName;
		}
		return nok(code$2);
	}
	function basicSelfClosing(code$2) {
		if (code$2 === 62) {
			effects.consume(code$2);
			return self.interrupt ? ok$2 : continuation;
		}
		return nok(code$2);
	}
	function completeClosingTagAfter(code$2) {
		if (markdownSpace(code$2)) {
			effects.consume(code$2);
			return completeClosingTagAfter;
		}
		return completeEnd(code$2);
	}
	function completeAttributeNameBefore(code$2) {
		if (code$2 === 47) {
			effects.consume(code$2);
			return completeEnd;
		}
		if (code$2 === 58 || code$2 === 95 || asciiAlpha(code$2)) {
			effects.consume(code$2);
			return completeAttributeName;
		}
		if (markdownSpace(code$2)) {
			effects.consume(code$2);
			return completeAttributeNameBefore;
		}
		return completeEnd(code$2);
	}
	function completeAttributeName(code$2) {
		if (code$2 === 45 || code$2 === 46 || code$2 === 58 || code$2 === 95 || asciiAlphanumeric(code$2)) {
			effects.consume(code$2);
			return completeAttributeName;
		}
		return completeAttributeNameAfter(code$2);
	}
	function completeAttributeNameAfter(code$2) {
		if (code$2 === 61) {
			effects.consume(code$2);
			return completeAttributeValueBefore;
		}
		if (markdownSpace(code$2)) {
			effects.consume(code$2);
			return completeAttributeNameAfter;
		}
		return completeAttributeNameBefore(code$2);
	}
	function completeAttributeValueBefore(code$2) {
		if (code$2 === null || code$2 === 60 || code$2 === 61 || code$2 === 62 || code$2 === 96) return nok(code$2);
		if (code$2 === 34 || code$2 === 39) {
			effects.consume(code$2);
			markerB = code$2;
			return completeAttributeValueQuoted;
		}
		if (markdownSpace(code$2)) {
			effects.consume(code$2);
			return completeAttributeValueBefore;
		}
		return completeAttributeValueUnquoted(code$2);
	}
	function completeAttributeValueQuoted(code$2) {
		if (code$2 === markerB) {
			effects.consume(code$2);
			markerB = null;
			return completeAttributeValueQuotedAfter;
		}
		if (code$2 === null || markdownLineEnding(code$2)) return nok(code$2);
		effects.consume(code$2);
		return completeAttributeValueQuoted;
	}
	function completeAttributeValueUnquoted(code$2) {
		if (code$2 === null || code$2 === 34 || code$2 === 39 || code$2 === 47 || code$2 === 60 || code$2 === 61 || code$2 === 62 || code$2 === 96 || markdownLineEndingOrSpace(code$2)) return completeAttributeNameAfter(code$2);
		effects.consume(code$2);
		return completeAttributeValueUnquoted;
	}
	function completeAttributeValueQuotedAfter(code$2) {
		if (code$2 === 47 || code$2 === 62 || markdownSpace(code$2)) return completeAttributeNameBefore(code$2);
		return nok(code$2);
	}
	function completeEnd(code$2) {
		if (code$2 === 62) {
			effects.consume(code$2);
			return completeAfter;
		}
		return nok(code$2);
	}
	function completeAfter(code$2) {
		if (code$2 === null || markdownLineEnding(code$2)) return continuation(code$2);
		if (markdownSpace(code$2)) {
			effects.consume(code$2);
			return completeAfter;
		}
		return nok(code$2);
	}
	function continuation(code$2) {
		if (code$2 === 45 && marker === 2) {
			effects.consume(code$2);
			return continuationCommentInside;
		}
		if (code$2 === 60 && marker === 1) {
			effects.consume(code$2);
			return continuationRawTagOpen;
		}
		if (code$2 === 62 && marker === 4) {
			effects.consume(code$2);
			return continuationClose;
		}
		if (code$2 === 63 && marker === 3) {
			effects.consume(code$2);
			return continuationDeclarationInside;
		}
		if (code$2 === 93 && marker === 5) {
			effects.consume(code$2);
			return continuationCdataInside;
		}
		if (markdownLineEnding(code$2) && (marker === 6 || marker === 7)) {
			effects.exit("htmlFlowData");
			return effects.check(blankLineBefore, continuationAfter, continuationStart)(code$2);
		}
		if (code$2 === null || markdownLineEnding(code$2)) {
			effects.exit("htmlFlowData");
			return continuationStart(code$2);
		}
		effects.consume(code$2);
		return continuation;
	}
	function continuationStart(code$2) {
		return effects.check(nonLazyContinuationStart, continuationStartNonLazy, continuationAfter)(code$2);
	}
	function continuationStartNonLazy(code$2) {
		effects.enter("lineEnding");
		effects.consume(code$2);
		effects.exit("lineEnding");
		return continuationBefore;
	}
	function continuationBefore(code$2) {
		if (code$2 === null || markdownLineEnding(code$2)) return continuationStart(code$2);
		effects.enter("htmlFlowData");
		return continuation(code$2);
	}
	function continuationCommentInside(code$2) {
		if (code$2 === 45) {
			effects.consume(code$2);
			return continuationDeclarationInside;
		}
		return continuation(code$2);
	}
	function continuationRawTagOpen(code$2) {
		if (code$2 === 47) {
			effects.consume(code$2);
			buffer = "";
			return continuationRawEndTag;
		}
		return continuation(code$2);
	}
	function continuationRawEndTag(code$2) {
		if (code$2 === 62) {
			const name = buffer.toLowerCase();
			if (htmlRawNames.includes(name)) {
				effects.consume(code$2);
				return continuationClose;
			}
			return continuation(code$2);
		}
		if (asciiAlpha(code$2) && buffer.length < 8) {
			effects.consume(code$2);
			buffer += String.fromCharCode(code$2);
			return continuationRawEndTag;
		}
		return continuation(code$2);
	}
	function continuationCdataInside(code$2) {
		if (code$2 === 93) {
			effects.consume(code$2);
			return continuationDeclarationInside;
		}
		return continuation(code$2);
	}
	function continuationDeclarationInside(code$2) {
		if (code$2 === 62) {
			effects.consume(code$2);
			return continuationClose;
		}
		if (code$2 === 45 && marker === 2) {
			effects.consume(code$2);
			return continuationDeclarationInside;
		}
		return continuation(code$2);
	}
	function continuationClose(code$2) {
		if (code$2 === null || markdownLineEnding(code$2)) {
			effects.exit("htmlFlowData");
			return continuationAfter(code$2);
		}
		effects.consume(code$2);
		return continuationClose;
	}
	function continuationAfter(code$2) {
		effects.exit("htmlFlow");
		return ok$2(code$2);
	}
}
function tokenizeNonLazyContinuationStart(effects, ok$2, nok) {
	const self = this;
	return start;
	function start(code$2) {
		if (markdownLineEnding(code$2)) {
			effects.enter("lineEnding");
			effects.consume(code$2);
			effects.exit("lineEnding");
			return after;
		}
		return nok(code$2);
	}
	function after(code$2) {
		return self.parser.lazy[self.now().line] ? nok(code$2) : ok$2(code$2);
	}
}
function tokenizeBlankLineBefore(effects, ok$2, nok) {
	return start;
	function start(code$2) {
		effects.enter("lineEnding");
		effects.consume(code$2);
		effects.exit("lineEnding");
		return effects.attempt(blankLine, ok$2, nok);
	}
}
const htmlText = {
	name: "htmlText",
	tokenize: tokenizeHtmlText
};
function tokenizeHtmlText(effects, ok$2, nok) {
	const self = this;
	let marker;
	let index$1;
	let returnState;
	return start;
	function start(code$2) {
		effects.enter("htmlText");
		effects.enter("htmlTextData");
		effects.consume(code$2);
		return open;
	}
	function open(code$2) {
		if (code$2 === 33) {
			effects.consume(code$2);
			return declarationOpen;
		}
		if (code$2 === 47) {
			effects.consume(code$2);
			return tagCloseStart;
		}
		if (code$2 === 63) {
			effects.consume(code$2);
			return instruction;
		}
		if (asciiAlpha(code$2)) {
			effects.consume(code$2);
			return tagOpen;
		}
		return nok(code$2);
	}
	function declarationOpen(code$2) {
		if (code$2 === 45) {
			effects.consume(code$2);
			return commentOpenInside;
		}
		if (code$2 === 91) {
			effects.consume(code$2);
			index$1 = 0;
			return cdataOpenInside;
		}
		if (asciiAlpha(code$2)) {
			effects.consume(code$2);
			return declaration;
		}
		return nok(code$2);
	}
	function commentOpenInside(code$2) {
		if (code$2 === 45) {
			effects.consume(code$2);
			return commentEnd;
		}
		return nok(code$2);
	}
	function comment(code$2) {
		if (code$2 === null) return nok(code$2);
		if (code$2 === 45) {
			effects.consume(code$2);
			return commentClose;
		}
		if (markdownLineEnding(code$2)) {
			returnState = comment;
			return lineEndingBefore(code$2);
		}
		effects.consume(code$2);
		return comment;
	}
	function commentClose(code$2) {
		if (code$2 === 45) {
			effects.consume(code$2);
			return commentEnd;
		}
		return comment(code$2);
	}
	function commentEnd(code$2) {
		return code$2 === 62 ? end(code$2) : code$2 === 45 ? commentClose(code$2) : comment(code$2);
	}
	function cdataOpenInside(code$2) {
		if (code$2 === "CDATA[".charCodeAt(index$1++)) {
			effects.consume(code$2);
			return index$1 === 6 ? cdata : cdataOpenInside;
		}
		return nok(code$2);
	}
	function cdata(code$2) {
		if (code$2 === null) return nok(code$2);
		if (code$2 === 93) {
			effects.consume(code$2);
			return cdataClose;
		}
		if (markdownLineEnding(code$2)) {
			returnState = cdata;
			return lineEndingBefore(code$2);
		}
		effects.consume(code$2);
		return cdata;
	}
	function cdataClose(code$2) {
		if (code$2 === 93) {
			effects.consume(code$2);
			return cdataEnd;
		}
		return cdata(code$2);
	}
	function cdataEnd(code$2) {
		if (code$2 === 62) return end(code$2);
		if (code$2 === 93) {
			effects.consume(code$2);
			return cdataEnd;
		}
		return cdata(code$2);
	}
	function declaration(code$2) {
		if (code$2 === null || code$2 === 62) return end(code$2);
		if (markdownLineEnding(code$2)) {
			returnState = declaration;
			return lineEndingBefore(code$2);
		}
		effects.consume(code$2);
		return declaration;
	}
	function instruction(code$2) {
		if (code$2 === null) return nok(code$2);
		if (code$2 === 63) {
			effects.consume(code$2);
			return instructionClose;
		}
		if (markdownLineEnding(code$2)) {
			returnState = instruction;
			return lineEndingBefore(code$2);
		}
		effects.consume(code$2);
		return instruction;
	}
	function instructionClose(code$2) {
		return code$2 === 62 ? end(code$2) : instruction(code$2);
	}
	function tagCloseStart(code$2) {
		if (asciiAlpha(code$2)) {
			effects.consume(code$2);
			return tagClose;
		}
		return nok(code$2);
	}
	function tagClose(code$2) {
		if (code$2 === 45 || asciiAlphanumeric(code$2)) {
			effects.consume(code$2);
			return tagClose;
		}
		return tagCloseBetween(code$2);
	}
	function tagCloseBetween(code$2) {
		if (markdownLineEnding(code$2)) {
			returnState = tagCloseBetween;
			return lineEndingBefore(code$2);
		}
		if (markdownSpace(code$2)) {
			effects.consume(code$2);
			return tagCloseBetween;
		}
		return end(code$2);
	}
	function tagOpen(code$2) {
		if (code$2 === 45 || asciiAlphanumeric(code$2)) {
			effects.consume(code$2);
			return tagOpen;
		}
		if (code$2 === 47 || code$2 === 62 || markdownLineEndingOrSpace(code$2)) return tagOpenBetween(code$2);
		return nok(code$2);
	}
	function tagOpenBetween(code$2) {
		if (code$2 === 47) {
			effects.consume(code$2);
			return end;
		}
		if (code$2 === 58 || code$2 === 95 || asciiAlpha(code$2)) {
			effects.consume(code$2);
			return tagOpenAttributeName;
		}
		if (markdownLineEnding(code$2)) {
			returnState = tagOpenBetween;
			return lineEndingBefore(code$2);
		}
		if (markdownSpace(code$2)) {
			effects.consume(code$2);
			return tagOpenBetween;
		}
		return end(code$2);
	}
	function tagOpenAttributeName(code$2) {
		if (code$2 === 45 || code$2 === 46 || code$2 === 58 || code$2 === 95 || asciiAlphanumeric(code$2)) {
			effects.consume(code$2);
			return tagOpenAttributeName;
		}
		return tagOpenAttributeNameAfter(code$2);
	}
	function tagOpenAttributeNameAfter(code$2) {
		if (code$2 === 61) {
			effects.consume(code$2);
			return tagOpenAttributeValueBefore;
		}
		if (markdownLineEnding(code$2)) {
			returnState = tagOpenAttributeNameAfter;
			return lineEndingBefore(code$2);
		}
		if (markdownSpace(code$2)) {
			effects.consume(code$2);
			return tagOpenAttributeNameAfter;
		}
		return tagOpenBetween(code$2);
	}
	function tagOpenAttributeValueBefore(code$2) {
		if (code$2 === null || code$2 === 60 || code$2 === 61 || code$2 === 62 || code$2 === 96) return nok(code$2);
		if (code$2 === 34 || code$2 === 39) {
			effects.consume(code$2);
			marker = code$2;
			return tagOpenAttributeValueQuoted;
		}
		if (markdownLineEnding(code$2)) {
			returnState = tagOpenAttributeValueBefore;
			return lineEndingBefore(code$2);
		}
		if (markdownSpace(code$2)) {
			effects.consume(code$2);
			return tagOpenAttributeValueBefore;
		}
		effects.consume(code$2);
		return tagOpenAttributeValueUnquoted;
	}
	function tagOpenAttributeValueQuoted(code$2) {
		if (code$2 === marker) {
			effects.consume(code$2);
			marker = void 0;
			return tagOpenAttributeValueQuotedAfter;
		}
		if (code$2 === null) return nok(code$2);
		if (markdownLineEnding(code$2)) {
			returnState = tagOpenAttributeValueQuoted;
			return lineEndingBefore(code$2);
		}
		effects.consume(code$2);
		return tagOpenAttributeValueQuoted;
	}
	function tagOpenAttributeValueUnquoted(code$2) {
		if (code$2 === null || code$2 === 34 || code$2 === 39 || code$2 === 60 || code$2 === 61 || code$2 === 96) return nok(code$2);
		if (code$2 === 47 || code$2 === 62 || markdownLineEndingOrSpace(code$2)) return tagOpenBetween(code$2);
		effects.consume(code$2);
		return tagOpenAttributeValueUnquoted;
	}
	function tagOpenAttributeValueQuotedAfter(code$2) {
		if (code$2 === 47 || code$2 === 62 || markdownLineEndingOrSpace(code$2)) return tagOpenBetween(code$2);
		return nok(code$2);
	}
	function end(code$2) {
		if (code$2 === 62) {
			effects.consume(code$2);
			effects.exit("htmlTextData");
			effects.exit("htmlText");
			return ok$2;
		}
		return nok(code$2);
	}
	function lineEndingBefore(code$2) {
		effects.exit("htmlTextData");
		effects.enter("lineEnding");
		effects.consume(code$2);
		effects.exit("lineEnding");
		return lineEndingAfter;
	}
	function lineEndingAfter(code$2) {
		return markdownSpace(code$2) ? factorySpace(effects, lineEndingAfterPrefix, "linePrefix", self.parser.constructs.disable.null.includes("codeIndented") ? void 0 : 4)(code$2) : lineEndingAfterPrefix(code$2);
	}
	function lineEndingAfterPrefix(code$2) {
		effects.enter("htmlTextData");
		return returnState(code$2);
	}
}
const labelEnd = {
	name: "labelEnd",
	resolveAll: resolveAllLabelEnd,
	resolveTo: resolveToLabelEnd,
	tokenize: tokenizeLabelEnd
};
var resourceConstruct = { tokenize: tokenizeResource };
var referenceFullConstruct = { tokenize: tokenizeReferenceFull };
var referenceCollapsedConstruct = { tokenize: tokenizeReferenceCollapsed };
function resolveAllLabelEnd(events) {
	let index$1 = -1;
	const newEvents = [];
	while (++index$1 < events.length) {
		const token = events[index$1][1];
		newEvents.push(events[index$1]);
		if (token.type === "labelImage" || token.type === "labelLink" || token.type === "labelEnd") {
			const offset = token.type === "labelImage" ? 4 : 2;
			token.type = "data";
			index$1 += offset;
		}
	}
	if (events.length !== newEvents.length) splice(events, 0, events.length, newEvents);
	return events;
}
function resolveToLabelEnd(events, context) {
	let index$1 = events.length;
	let offset = 0;
	let token;
	let open;
	let close;
	let media;
	while (index$1--) {
		token = events[index$1][1];
		if (open) {
			if (token.type === "link" || token.type === "labelLink" && token._inactive) break;
			if (events[index$1][0] === "enter" && token.type === "labelLink") token._inactive = true;
		} else if (close) {
			if (events[index$1][0] === "enter" && (token.type === "labelImage" || token.type === "labelLink") && !token._balanced) {
				open = index$1;
				if (token.type !== "labelLink") {
					offset = 2;
					break;
				}
			}
		} else if (token.type === "labelEnd") close = index$1;
	}
	const group = {
		type: events[open][1].type === "labelLink" ? "link" : "image",
		start: { ...events[open][1].start },
		end: { ...events[events.length - 1][1].end }
	};
	const label = {
		type: "label",
		start: { ...events[open][1].start },
		end: { ...events[close][1].end }
	};
	const text$4 = {
		type: "labelText",
		start: { ...events[open + offset + 2][1].end },
		end: { ...events[close - 2][1].start }
	};
	media = [[
		"enter",
		group,
		context
	], [
		"enter",
		label,
		context
	]];
	media = push(media, events.slice(open + 1, open + offset + 3));
	media = push(media, [[
		"enter",
		text$4,
		context
	]]);
	media = push(media, resolveAll(context.parser.constructs.insideSpan.null, events.slice(open + offset + 4, close - 3), context));
	media = push(media, [
		[
			"exit",
			text$4,
			context
		],
		events[close - 2],
		events[close - 1],
		[
			"exit",
			label,
			context
		]
	]);
	media = push(media, events.slice(close + 1));
	media = push(media, [[
		"exit",
		group,
		context
	]]);
	splice(events, open, events.length, media);
	return events;
}
function tokenizeLabelEnd(effects, ok$2, nok) {
	const self = this;
	let index$1 = self.events.length;
	let labelStart;
	let defined;
	while (index$1--) if ((self.events[index$1][1].type === "labelImage" || self.events[index$1][1].type === "labelLink") && !self.events[index$1][1]._balanced) {
		labelStart = self.events[index$1][1];
		break;
	}
	return start;
	function start(code$2) {
		if (!labelStart) return nok(code$2);
		if (labelStart._inactive) return labelEndNok(code$2);
		defined = self.parser.defined.includes(normalizeIdentifier(self.sliceSerialize({
			start: labelStart.end,
			end: self.now()
		})));
		effects.enter("labelEnd");
		effects.enter("labelMarker");
		effects.consume(code$2);
		effects.exit("labelMarker");
		effects.exit("labelEnd");
		return after;
	}
	function after(code$2) {
		if (code$2 === 40) return effects.attempt(resourceConstruct, labelEndOk, defined ? labelEndOk : labelEndNok)(code$2);
		if (code$2 === 91) return effects.attempt(referenceFullConstruct, labelEndOk, defined ? referenceNotFull : labelEndNok)(code$2);
		return defined ? labelEndOk(code$2) : labelEndNok(code$2);
	}
	function referenceNotFull(code$2) {
		return effects.attempt(referenceCollapsedConstruct, labelEndOk, labelEndNok)(code$2);
	}
	function labelEndOk(code$2) {
		return ok$2(code$2);
	}
	function labelEndNok(code$2) {
		labelStart._balanced = true;
		return nok(code$2);
	}
}
function tokenizeResource(effects, ok$2, nok) {
	return resourceStart;
	function resourceStart(code$2) {
		effects.enter("resource");
		effects.enter("resourceMarker");
		effects.consume(code$2);
		effects.exit("resourceMarker");
		return resourceBefore;
	}
	function resourceBefore(code$2) {
		return markdownLineEndingOrSpace(code$2) ? factoryWhitespace(effects, resourceOpen)(code$2) : resourceOpen(code$2);
	}
	function resourceOpen(code$2) {
		if (code$2 === 41) return resourceEnd(code$2);
		return factoryDestination(effects, resourceDestinationAfter, resourceDestinationMissing, "resourceDestination", "resourceDestinationLiteral", "resourceDestinationLiteralMarker", "resourceDestinationRaw", "resourceDestinationString", 32)(code$2);
	}
	function resourceDestinationAfter(code$2) {
		return markdownLineEndingOrSpace(code$2) ? factoryWhitespace(effects, resourceBetween)(code$2) : resourceEnd(code$2);
	}
	function resourceDestinationMissing(code$2) {
		return nok(code$2);
	}
	function resourceBetween(code$2) {
		if (code$2 === 34 || code$2 === 39 || code$2 === 40) return factoryTitle(effects, resourceTitleAfter, nok, "resourceTitle", "resourceTitleMarker", "resourceTitleString")(code$2);
		return resourceEnd(code$2);
	}
	function resourceTitleAfter(code$2) {
		return markdownLineEndingOrSpace(code$2) ? factoryWhitespace(effects, resourceEnd)(code$2) : resourceEnd(code$2);
	}
	function resourceEnd(code$2) {
		if (code$2 === 41) {
			effects.enter("resourceMarker");
			effects.consume(code$2);
			effects.exit("resourceMarker");
			effects.exit("resource");
			return ok$2;
		}
		return nok(code$2);
	}
}
function tokenizeReferenceFull(effects, ok$2, nok) {
	const self = this;
	return referenceFull;
	function referenceFull(code$2) {
		return factoryLabel.call(self, effects, referenceFullAfter, referenceFullMissing, "reference", "referenceMarker", "referenceString")(code$2);
	}
	function referenceFullAfter(code$2) {
		return self.parser.defined.includes(normalizeIdentifier(self.sliceSerialize(self.events[self.events.length - 1][1]).slice(1, -1))) ? ok$2(code$2) : nok(code$2);
	}
	function referenceFullMissing(code$2) {
		return nok(code$2);
	}
}
function tokenizeReferenceCollapsed(effects, ok$2, nok) {
	return referenceCollapsedStart;
	function referenceCollapsedStart(code$2) {
		effects.enter("reference");
		effects.enter("referenceMarker");
		effects.consume(code$2);
		effects.exit("referenceMarker");
		return referenceCollapsedOpen;
	}
	function referenceCollapsedOpen(code$2) {
		if (code$2 === 93) {
			effects.enter("referenceMarker");
			effects.consume(code$2);
			effects.exit("referenceMarker");
			effects.exit("reference");
			return ok$2;
		}
		return nok(code$2);
	}
}
const labelStartImage = {
	name: "labelStartImage",
	resolveAll: labelEnd.resolveAll,
	tokenize: tokenizeLabelStartImage
};
function tokenizeLabelStartImage(effects, ok$2, nok) {
	const self = this;
	return start;
	function start(code$2) {
		effects.enter("labelImage");
		effects.enter("labelImageMarker");
		effects.consume(code$2);
		effects.exit("labelImageMarker");
		return open;
	}
	function open(code$2) {
		if (code$2 === 91) {
			effects.enter("labelMarker");
			effects.consume(code$2);
			effects.exit("labelMarker");
			effects.exit("labelImage");
			return after;
		}
		return nok(code$2);
	}
	function after(code$2) {
		/* c8 ignore next 3 */
		return code$2 === 94 && "_hiddenFootnoteSupport" in self.parser.constructs ? nok(code$2) : ok$2(code$2);
	}
}
const labelStartLink = {
	name: "labelStartLink",
	resolveAll: labelEnd.resolveAll,
	tokenize: tokenizeLabelStartLink
};
function tokenizeLabelStartLink(effects, ok$2, nok) {
	const self = this;
	return start;
	function start(code$2) {
		effects.enter("labelLink");
		effects.enter("labelMarker");
		effects.consume(code$2);
		effects.exit("labelMarker");
		effects.exit("labelLink");
		return after;
	}
	function after(code$2) {
		/* c8 ignore next 3 */
		return code$2 === 94 && "_hiddenFootnoteSupport" in self.parser.constructs ? nok(code$2) : ok$2(code$2);
	}
}
const lineEnding = {
	name: "lineEnding",
	tokenize: tokenizeLineEnding
};
function tokenizeLineEnding(effects, ok$2) {
	return start;
	function start(code$2) {
		effects.enter("lineEnding");
		effects.consume(code$2);
		effects.exit("lineEnding");
		return factorySpace(effects, ok$2, "linePrefix");
	}
}
const thematicBreak$1 = {
	name: "thematicBreak",
	tokenize: tokenizeThematicBreak
};
function tokenizeThematicBreak(effects, ok$2, nok) {
	let size = 0;
	let marker;
	return start;
	function start(code$2) {
		effects.enter("thematicBreak");
		return before(code$2);
	}
	function before(code$2) {
		marker = code$2;
		return atBreak(code$2);
	}
	function atBreak(code$2) {
		if (code$2 === marker) {
			effects.enter("thematicBreakSequence");
			return sequence(code$2);
		}
		if (size >= 3 && (code$2 === null || markdownLineEnding(code$2))) {
			effects.exit("thematicBreak");
			return ok$2(code$2);
		}
		return nok(code$2);
	}
	function sequence(code$2) {
		if (code$2 === marker) {
			effects.consume(code$2);
			size++;
			return sequence;
		}
		effects.exit("thematicBreakSequence");
		return markdownSpace(code$2) ? factorySpace(effects, atBreak, "whitespace")(code$2) : atBreak(code$2);
	}
}
const list$1 = {
	continuation: { tokenize: tokenizeListContinuation },
	exit: tokenizeListEnd,
	name: "list",
	tokenize: tokenizeListStart
};
var listItemPrefixWhitespaceConstruct = {
	partial: true,
	tokenize: tokenizeListItemPrefixWhitespace
};
var indentConstruct = {
	partial: true,
	tokenize: tokenizeIndent$1
};
function tokenizeListStart(effects, ok$2, nok) {
	const self = this;
	const tail = self.events[self.events.length - 1];
	let initialSize = tail && tail[1].type === "linePrefix" ? tail[2].sliceSerialize(tail[1], true).length : 0;
	let size = 0;
	return start;
	function start(code$2) {
		const kind = self.containerState.type || (code$2 === 42 || code$2 === 43 || code$2 === 45 ? "listUnordered" : "listOrdered");
		if (kind === "listUnordered" ? !self.containerState.marker || code$2 === self.containerState.marker : asciiDigit(code$2)) {
			if (!self.containerState.type) {
				self.containerState.type = kind;
				effects.enter(kind, { _container: true });
			}
			if (kind === "listUnordered") {
				effects.enter("listItemPrefix");
				return code$2 === 42 || code$2 === 45 ? effects.check(thematicBreak$1, nok, atMarker)(code$2) : atMarker(code$2);
			}
			if (!self.interrupt || code$2 === 49) {
				effects.enter("listItemPrefix");
				effects.enter("listItemValue");
				return inside(code$2);
			}
		}
		return nok(code$2);
	}
	function inside(code$2) {
		if (asciiDigit(code$2) && ++size < 10) {
			effects.consume(code$2);
			return inside;
		}
		if ((!self.interrupt || size < 2) && (self.containerState.marker ? code$2 === self.containerState.marker : code$2 === 41 || code$2 === 46)) {
			effects.exit("listItemValue");
			return atMarker(code$2);
		}
		return nok(code$2);
	}
	function atMarker(code$2) {
		effects.enter("listItemMarker");
		effects.consume(code$2);
		effects.exit("listItemMarker");
		self.containerState.marker = self.containerState.marker || code$2;
		return effects.check(blankLine, self.interrupt ? nok : onBlank, effects.attempt(listItemPrefixWhitespaceConstruct, endOfPrefix, otherPrefix));
	}
	function onBlank(code$2) {
		self.containerState.initialBlankLine = true;
		initialSize++;
		return endOfPrefix(code$2);
	}
	function otherPrefix(code$2) {
		if (markdownSpace(code$2)) {
			effects.enter("listItemPrefixWhitespace");
			effects.consume(code$2);
			effects.exit("listItemPrefixWhitespace");
			return endOfPrefix;
		}
		return nok(code$2);
	}
	function endOfPrefix(code$2) {
		self.containerState.size = initialSize + self.sliceSerialize(effects.exit("listItemPrefix"), true).length;
		return ok$2(code$2);
	}
}
function tokenizeListContinuation(effects, ok$2, nok) {
	const self = this;
	self.containerState._closeFlow = void 0;
	return effects.check(blankLine, onBlank, notBlank);
	function onBlank(code$2) {
		self.containerState.furtherBlankLines = self.containerState.furtherBlankLines || self.containerState.initialBlankLine;
		return factorySpace(effects, ok$2, "listItemIndent", self.containerState.size + 1)(code$2);
	}
	function notBlank(code$2) {
		if (self.containerState.furtherBlankLines || !markdownSpace(code$2)) {
			self.containerState.furtherBlankLines = void 0;
			self.containerState.initialBlankLine = void 0;
			return notInCurrentItem(code$2);
		}
		self.containerState.furtherBlankLines = void 0;
		self.containerState.initialBlankLine = void 0;
		return effects.attempt(indentConstruct, ok$2, notInCurrentItem)(code$2);
	}
	function notInCurrentItem(code$2) {
		self.containerState._closeFlow = true;
		self.interrupt = void 0;
		return factorySpace(effects, effects.attempt(list$1, ok$2, nok), "linePrefix", self.parser.constructs.disable.null.includes("codeIndented") ? void 0 : 4)(code$2);
	}
}
function tokenizeIndent$1(effects, ok$2, nok) {
	const self = this;
	return factorySpace(effects, afterPrefix, "listItemIndent", self.containerState.size + 1);
	function afterPrefix(code$2) {
		const tail = self.events[self.events.length - 1];
		return tail && tail[1].type === "listItemIndent" && tail[2].sliceSerialize(tail[1], true).length === self.containerState.size ? ok$2(code$2) : nok(code$2);
	}
}
function tokenizeListEnd(effects) {
	effects.exit(this.containerState.type);
}
function tokenizeListItemPrefixWhitespace(effects, ok$2, nok) {
	const self = this;
	return factorySpace(effects, afterPrefix, "listItemPrefixWhitespace", self.parser.constructs.disable.null.includes("codeIndented") ? void 0 : 5);
	function afterPrefix(code$2) {
		const tail = self.events[self.events.length - 1];
		return !markdownSpace(code$2) && tail && tail[1].type === "listItemPrefixWhitespace" ? ok$2(code$2) : nok(code$2);
	}
}
const setextUnderline = {
	name: "setextUnderline",
	resolveTo: resolveToSetextUnderline,
	tokenize: tokenizeSetextUnderline
};
function resolveToSetextUnderline(events, context) {
	let index$1 = events.length;
	let content$2;
	let text$4;
	let definition$2;
	while (index$1--) if (events[index$1][0] === "enter") {
		if (events[index$1][1].type === "content") {
			content$2 = index$1;
			break;
		}
		if (events[index$1][1].type === "paragraph") text$4 = index$1;
	} else {
		if (events[index$1][1].type === "content") events.splice(index$1, 1);
		if (!definition$2 && events[index$1][1].type === "definition") definition$2 = index$1;
	}
	const heading$1 = {
		type: "setextHeading",
		start: { ...events[content$2][1].start },
		end: { ...events[events.length - 1][1].end }
	};
	events[text$4][1].type = "setextHeadingText";
	if (definition$2) {
		events.splice(text$4, 0, [
			"enter",
			heading$1,
			context
		]);
		events.splice(definition$2 + 1, 0, [
			"exit",
			events[content$2][1],
			context
		]);
		events[content$2][1].end = { ...events[definition$2][1].end };
	} else events[content$2][1] = heading$1;
	events.push([
		"exit",
		heading$1,
		context
	]);
	return events;
}
function tokenizeSetextUnderline(effects, ok$2, nok) {
	const self = this;
	let marker;
	return start;
	function start(code$2) {
		let index$1 = self.events.length;
		let paragraph$1;
		while (index$1--) if (self.events[index$1][1].type !== "lineEnding" && self.events[index$1][1].type !== "linePrefix" && self.events[index$1][1].type !== "content") {
			paragraph$1 = self.events[index$1][1].type === "paragraph";
			break;
		}
		if (!self.parser.lazy[self.now().line] && (self.interrupt || paragraph$1)) {
			effects.enter("setextHeadingLine");
			marker = code$2;
			return before(code$2);
		}
		return nok(code$2);
	}
	function before(code$2) {
		effects.enter("setextHeadingLineSequence");
		return inside(code$2);
	}
	function inside(code$2) {
		if (code$2 === marker) {
			effects.consume(code$2);
			return inside;
		}
		effects.exit("setextHeadingLineSequence");
		return markdownSpace(code$2) ? factorySpace(effects, after, "lineSuffix")(code$2) : after(code$2);
	}
	function after(code$2) {
		if (code$2 === null || markdownLineEnding(code$2)) {
			effects.exit("setextHeadingLine");
			return ok$2(code$2);
		}
		return nok(code$2);
	}
}
const flow = { tokenize: initializeFlow };
function initializeFlow(effects) {
	const self = this;
	const initial = effects.attempt(blankLine, atBlankEnding, effects.attempt(this.parser.constructs.flowInitial, afterConstruct, factorySpace(effects, effects.attempt(this.parser.constructs.flow, afterConstruct, effects.attempt(content$1, afterConstruct)), "linePrefix")));
	return initial;
	function atBlankEnding(code$2) {
		if (code$2 === null) {
			effects.consume(code$2);
			return;
		}
		effects.enter("lineEndingBlank");
		effects.consume(code$2);
		effects.exit("lineEndingBlank");
		self.currentConstruct = void 0;
		return initial;
	}
	function afterConstruct(code$2) {
		if (code$2 === null) {
			effects.consume(code$2);
			return;
		}
		effects.enter("lineEnding");
		effects.consume(code$2);
		effects.exit("lineEnding");
		self.currentConstruct = void 0;
		return initial;
	}
}
const resolver = { resolveAll: createResolver() };
const string = initializeFactory("string");
const text$2 = initializeFactory("text");
function initializeFactory(field) {
	return {
		resolveAll: createResolver(field === "text" ? resolveAllLineSuffixes : void 0),
		tokenize: initializeText
	};
	function initializeText(effects) {
		const self = this;
		const constructs$1 = this.parser.constructs[field];
		const text$4 = effects.attempt(constructs$1, start, notText);
		return start;
		function start(code$2) {
			return atBreak(code$2) ? text$4(code$2) : notText(code$2);
		}
		function notText(code$2) {
			if (code$2 === null) {
				effects.consume(code$2);
				return;
			}
			effects.enter("data");
			effects.consume(code$2);
			return data;
		}
		function data(code$2) {
			if (atBreak(code$2)) {
				effects.exit("data");
				return text$4(code$2);
			}
			effects.consume(code$2);
			return data;
		}
		function atBreak(code$2) {
			if (code$2 === null) return true;
			const list$2 = constructs$1[code$2];
			let index$1 = -1;
			if (list$2) while (++index$1 < list$2.length) {
				const item = list$2[index$1];
				if (!item.previous || item.previous.call(self, self.previous)) return true;
			}
			return false;
		}
	}
}
function createResolver(extraResolver) {
	return resolveAllText;
	function resolveAllText(events, context) {
		let index$1 = -1;
		let enter;
		while (++index$1 <= events.length) if (enter === void 0) {
			if (events[index$1] && events[index$1][1].type === "data") {
				enter = index$1;
				index$1++;
			}
		} else if (!events[index$1] || events[index$1][1].type !== "data") {
			if (index$1 !== enter + 2) {
				events[enter][1].end = events[index$1 - 1][1].end;
				events.splice(enter + 2, index$1 - enter - 2);
				index$1 = enter + 2;
			}
			enter = void 0;
		}
		return extraResolver ? extraResolver(events, context) : events;
	}
}
function resolveAllLineSuffixes(events, context) {
	let eventIndex = 0;
	while (++eventIndex <= events.length) if ((eventIndex === events.length || events[eventIndex][1].type === "lineEnding") && events[eventIndex - 1][1].type === "data") {
		const data = events[eventIndex - 1][1];
		const chunks = context.sliceStream(data);
		let index$1 = chunks.length;
		let bufferIndex = -1;
		let size = 0;
		let tabs;
		while (index$1--) {
			const chunk = chunks[index$1];
			if (typeof chunk === "string") {
				bufferIndex = chunk.length;
				while (chunk.charCodeAt(bufferIndex - 1) === 32) {
					size++;
					bufferIndex--;
				}
				if (bufferIndex) break;
				bufferIndex = -1;
			} else if (chunk === -2) {
				tabs = true;
				size++;
			} else if (chunk === -1) {} else {
				index$1++;
				break;
			}
		}
		if (context._contentTypeTextTrailing && eventIndex === events.length) size = 0;
		if (size) {
			const token = {
				type: eventIndex === events.length || tabs || size < 2 ? "lineSuffix" : "hardBreakTrailing",
				start: {
					_bufferIndex: index$1 ? bufferIndex : data.start._bufferIndex + bufferIndex,
					_index: data.start._index + index$1,
					line: data.end.line,
					column: data.end.column - size,
					offset: data.end.offset - size
				},
				end: { ...data.end }
			};
			data.end = { ...token.start };
			if (data.start.offset === data.end.offset) Object.assign(data, token);
			else {
				events.splice(eventIndex, 0, [
					"enter",
					token,
					context
				], [
					"exit",
					token,
					context
				]);
				eventIndex += 2;
			}
		}
		eventIndex++;
	}
	return events;
}
var constructs_exports = /* @__PURE__ */ __export({
	attentionMarkers: () => attentionMarkers,
	contentInitial: () => contentInitial,
	disable: () => disable,
	document: () => document$2,
	flow: () => flow$1,
	flowInitial: () => flowInitial,
	insideSpan: () => insideSpan,
	string: () => string$1,
	text: () => text$3
}, 1);
const document$2 = {
	[42]: list$1,
	[43]: list$1,
	[45]: list$1,
	[48]: list$1,
	[49]: list$1,
	[50]: list$1,
	[51]: list$1,
	[52]: list$1,
	[53]: list$1,
	[54]: list$1,
	[55]: list$1,
	[56]: list$1,
	[57]: list$1,
	[62]: blockQuote
};
const contentInitial = { [91]: definition$1 };
const flowInitial = {
	[-2]: codeIndented,
	[-1]: codeIndented,
	[32]: codeIndented
};
const flow$1 = {
	[35]: headingAtx,
	[42]: thematicBreak$1,
	[45]: [setextUnderline, thematicBreak$1],
	[60]: htmlFlow,
	[61]: setextUnderline,
	[95]: thematicBreak$1,
	[96]: codeFenced,
	[126]: codeFenced
};
const string$1 = {
	[38]: characterReference,
	[92]: characterEscape
};
const text$3 = {
	[-5]: lineEnding,
	[-4]: lineEnding,
	[-3]: lineEnding,
	[33]: labelStartImage,
	[38]: characterReference,
	[42]: attention,
	[60]: [autolink, htmlText],
	[91]: labelStartLink,
	[92]: [hardBreakEscape, characterEscape],
	[93]: labelEnd,
	[95]: attention,
	[96]: codeText
};
const insideSpan = { null: [attention, resolver] };
const attentionMarkers = { null: [42, 95] };
const disable = { null: [] };
function createTokenizer(parser, initialize, from) {
	let point$2 = {
		_bufferIndex: -1,
		_index: 0,
		line: from && from.line || 1,
		column: from && from.column || 1,
		offset: from && from.offset || 0
	};
	const columnStart = {};
	const resolveAllConstructs = [];
	let chunks = [];
	let stack = [];
	const effects = {
		attempt: constructFactory(onsuccessfulconstruct),
		check: constructFactory(onsuccessfulcheck),
		consume,
		enter,
		exit: exit$2,
		interrupt: constructFactory(onsuccessfulcheck, { interrupt: true })
	};
	const context = {
		code: null,
		containerState: {},
		defineSkip,
		events: [],
		now,
		parser,
		previous: null,
		sliceSerialize,
		sliceStream,
		write
	};
	let state = initialize.tokenize.call(context, effects);
	if (initialize.resolveAll) resolveAllConstructs.push(initialize);
	return context;
	function write(slice) {
		chunks = push(chunks, slice);
		main();
		if (chunks[chunks.length - 1] !== null) return [];
		addResult(initialize, 0);
		context.events = resolveAll(resolveAllConstructs, context.events, context);
		return context.events;
	}
	function sliceSerialize(token, expandTabs) {
		return serializeChunks(sliceStream(token), expandTabs);
	}
	function sliceStream(token) {
		return sliceChunks(chunks, token);
	}
	function now() {
		const { _bufferIndex, _index, line, column, offset } = point$2;
		return {
			_bufferIndex,
			_index,
			line,
			column,
			offset
		};
	}
	function defineSkip(value) {
		columnStart[value.line] = value.column;
		accountForPotentialSkip();
	}
	function main() {
		let chunkIndex;
		while (point$2._index < chunks.length) {
			const chunk = chunks[point$2._index];
			if (typeof chunk === "string") {
				chunkIndex = point$2._index;
				if (point$2._bufferIndex < 0) point$2._bufferIndex = 0;
				while (point$2._index === chunkIndex && point$2._bufferIndex < chunk.length) go(chunk.charCodeAt(point$2._bufferIndex));
			} else go(chunk);
		}
	}
	function go(code$2) {
		state = state(code$2);
	}
	function consume(code$2) {
		if (markdownLineEnding(code$2)) {
			point$2.line++;
			point$2.column = 1;
			point$2.offset += code$2 === -3 ? 2 : 1;
			accountForPotentialSkip();
		} else if (code$2 !== -1) {
			point$2.column++;
			point$2.offset++;
		}
		if (point$2._bufferIndex < 0) point$2._index++;
		else {
			point$2._bufferIndex++;
			if (point$2._bufferIndex === chunks[point$2._index].length) {
				point$2._bufferIndex = -1;
				point$2._index++;
			}
		}
		context.previous = code$2;
	}
	function enter(type, fields) {
		const token = fields || {};
		token.type = type;
		token.start = now();
		context.events.push([
			"enter",
			token,
			context
		]);
		stack.push(token);
		return token;
	}
	function exit$2(type) {
		const token = stack.pop();
		token.end = now();
		context.events.push([
			"exit",
			token,
			context
		]);
		return token;
	}
	function onsuccessfulconstruct(construct, info) {
		addResult(construct, info.from);
	}
	function onsuccessfulcheck(_, info) {
		info.restore();
	}
	function constructFactory(onreturn, fields) {
		return hook;
		function hook(constructs$1, returnState, bogusState) {
			let listOfConstructs;
			let constructIndex;
			let currentConstruct;
			let info;
			return Array.isArray(constructs$1) ? handleListOfConstructs(constructs$1) : "tokenize" in constructs$1 ? handleListOfConstructs([constructs$1]) : handleMapOfConstructs(constructs$1);
			function handleMapOfConstructs(map$2) {
				return start;
				function start(code$2) {
					const left = code$2 !== null && map$2[code$2];
					const all$1 = code$2 !== null && map$2.null;
					return handleListOfConstructs([...Array.isArray(left) ? left : left ? [left] : [], ...Array.isArray(all$1) ? all$1 : all$1 ? [all$1] : []])(code$2);
				}
			}
			function handleListOfConstructs(list$2) {
				listOfConstructs = list$2;
				constructIndex = 0;
				if (list$2.length === 0) return bogusState;
				return handleConstruct(list$2[constructIndex]);
			}
			function handleConstruct(construct) {
				return start;
				function start(code$2) {
					info = store();
					currentConstruct = construct;
					if (!construct.partial) context.currentConstruct = construct;
					if (construct.name && context.parser.constructs.disable.null.includes(construct.name)) return nok(code$2);
					return construct.tokenize.call(fields ? Object.assign(Object.create(context), fields) : context, effects, ok$2, nok)(code$2);
				}
			}
			function ok$2(code$2) {
				onreturn(currentConstruct, info);
				return returnState;
			}
			function nok(code$2) {
				info.restore();
				if (++constructIndex < listOfConstructs.length) return handleConstruct(listOfConstructs[constructIndex]);
				return bogusState;
			}
		}
	}
	function addResult(construct, from$1) {
		if (construct.resolveAll && !resolveAllConstructs.includes(construct)) resolveAllConstructs.push(construct);
		if (construct.resolve) splice(context.events, from$1, context.events.length - from$1, construct.resolve(context.events.slice(from$1), context));
		if (construct.resolveTo) context.events = construct.resolveTo(context.events, context);
	}
	function store() {
		const startPoint = now();
		const startPrevious = context.previous;
		const startCurrentConstruct = context.currentConstruct;
		const startEventsIndex = context.events.length;
		const startStack = Array.from(stack);
		return {
			from: startEventsIndex,
			restore
		};
		function restore() {
			point$2 = startPoint;
			context.previous = startPrevious;
			context.currentConstruct = startCurrentConstruct;
			context.events.length = startEventsIndex;
			stack = startStack;
			accountForPotentialSkip();
		}
	}
	function accountForPotentialSkip() {
		if (point$2.line in columnStart && point$2.column < 2) {
			point$2.column = columnStart[point$2.line];
			point$2.offset += columnStart[point$2.line] - 1;
		}
	}
}
function sliceChunks(chunks, token) {
	const startIndex = token.start._index;
	const startBufferIndex = token.start._bufferIndex;
	const endIndex = token.end._index;
	const endBufferIndex = token.end._bufferIndex;
	let view;
	if (startIndex === endIndex) view = [chunks[startIndex].slice(startBufferIndex, endBufferIndex)];
	else {
		view = chunks.slice(startIndex, endIndex);
		if (startBufferIndex > -1) {
			const head = view[0];
			if (typeof head === "string") view[0] = head.slice(startBufferIndex);
			else view.shift();
		}
		if (endBufferIndex > 0) view.push(chunks[endIndex].slice(0, endBufferIndex));
	}
	return view;
}
function serializeChunks(chunks, expandTabs) {
	let index$1 = -1;
	const result = [];
	let atTab;
	while (++index$1 < chunks.length) {
		const chunk = chunks[index$1];
		let value;
		if (typeof chunk === "string") value = chunk;
		else switch (chunk) {
			case -5:
				value = "\r";
				break;
			case -4:
				value = "\n";
				break;
			case -3:
				value = "\r\n";
				break;
			case -2:
				value = expandTabs ? " " : "	";
				break;
			case -1:
				if (!expandTabs && atTab) continue;
				value = " ";
				break;
			default: value = String.fromCharCode(chunk);
		}
		atTab = chunk === -2;
		result.push(value);
	}
	return result.join("");
}
function parse(options) {
	const parser = {
		constructs: combineExtensions([constructs_exports, ...(options || {}).extensions || []]),
		content: create(content),
		defined: [],
		document: create(document$1),
		flow: create(flow),
		lazy: {},
		string: create(string),
		text: create(text$2)
	};
	return parser;
	function create(initial) {
		return creator;
		function creator(from) {
			return createTokenizer(parser, initial, from);
		}
	}
}
function postprocess(events) {
	while (!subtokenize(events));
	return events;
}
var search = /[\0\t\n\r]/g;
function preprocess() {
	let column = 1;
	let buffer = "";
	let start = true;
	let atCarriageReturn;
	return preprocessor;
	function preprocessor(value, encoding, end) {
		const chunks = [];
		let match;
		let next;
		let startPosition;
		let endPosition;
		let code$2;
		value = buffer + (typeof value === "string" ? value.toString() : new TextDecoder(encoding || void 0).decode(value));
		startPosition = 0;
		buffer = "";
		if (start) {
			if (value.charCodeAt(0) === 65279) startPosition++;
			start = void 0;
		}
		while (startPosition < value.length) {
			search.lastIndex = startPosition;
			match = search.exec(value);
			endPosition = match && match.index !== void 0 ? match.index : value.length;
			code$2 = value.charCodeAt(endPosition);
			if (!match) {
				buffer = value.slice(startPosition);
				break;
			}
			if (code$2 === 10 && startPosition === endPosition && atCarriageReturn) {
				chunks.push(-3);
				atCarriageReturn = void 0;
			} else {
				if (atCarriageReturn) {
					chunks.push(-5);
					atCarriageReturn = void 0;
				}
				if (startPosition < endPosition) {
					chunks.push(value.slice(startPosition, endPosition));
					column += endPosition - startPosition;
				}
				switch (code$2) {
					case 0:
						chunks.push(65533);
						column++;
						break;
					case 9:
						next = Math.ceil(column / 4) * 4;
						chunks.push(-2);
						while (column++ < next) chunks.push(-1);
						break;
					case 10:
						chunks.push(-4);
						column = 1;
						break;
					default:
						atCarriageReturn = true;
						column = 1;
				}
			}
			startPosition = endPosition + 1;
		}
		if (end) {
			if (atCarriageReturn) chunks.push(-5);
			if (buffer) chunks.push(buffer);
			chunks.push(null);
		}
		return chunks;
	}
}
var characterEscapeOrReference = /\\([!-/:-@[-`{-~])|&(#(?:\d{1,7}|x[\da-f]{1,6})|[\da-z]{1,31});/gi;
function decodeString(value) {
	return value.replace(characterEscapeOrReference, decode);
}
function decode($0, $1, $2) {
	if ($1) return $1;
	if ($2.charCodeAt(0) === 35) {
		const head = $2.charCodeAt(1);
		const hex = head === 120 || head === 88;
		return decodeNumericCharacterReference($2.slice(hex ? 2 : 1), hex ? 16 : 10);
	}
	return decodeNamedCharacterReference($2) || $0;
}
var own$1 = {}.hasOwnProperty;
function fromMarkdown(value, encoding, options) {
	if (encoding && typeof encoding === "object") {
		options = encoding;
		encoding = void 0;
	}
	return compiler(options)(postprocess(parse(options).document().write(preprocess()(value, encoding, true))));
}
function compiler(options) {
	const config = {
		transforms: [],
		canContainEols: [
			"emphasis",
			"fragment",
			"heading",
			"paragraph",
			"strong"
		],
		enter: {
			autolink: opener(link$1),
			autolinkProtocol: onenterdata,
			autolinkEmail: onenterdata,
			atxHeading: opener(heading$1),
			blockQuote: opener(blockQuote$1),
			characterEscape: onenterdata,
			characterReference: onenterdata,
			codeFenced: opener(codeFlow),
			codeFencedFenceInfo: buffer,
			codeFencedFenceMeta: buffer,
			codeIndented: opener(codeFlow, buffer),
			codeText: opener(codeText$1, buffer),
			codeTextData: onenterdata,
			data: onenterdata,
			codeFlowValue: onenterdata,
			definition: opener(definition$2),
			definitionDestinationString: buffer,
			definitionLabelString: buffer,
			definitionTitleString: buffer,
			emphasis: opener(emphasis$1),
			hardBreakEscape: opener(hardBreak$1),
			hardBreakTrailing: opener(hardBreak$1),
			htmlFlow: opener(html$1, buffer),
			htmlFlowData: onenterdata,
			htmlText: opener(html$1, buffer),
			htmlTextData: onenterdata,
			image: opener(image$1),
			label: buffer,
			link: opener(link$1),
			listItem: opener(listItem$1),
			listItemValue: onenterlistitemvalue,
			listOrdered: opener(list$2, onenterlistordered),
			listUnordered: opener(list$2),
			paragraph: opener(paragraph$1),
			reference: onenterreference,
			referenceString: buffer,
			resourceDestinationString: buffer,
			resourceTitleString: buffer,
			setextHeading: opener(heading$1),
			strong: opener(strong$1),
			thematicBreak: opener(thematicBreak$2)
		},
		exit: {
			atxHeading: closer(),
			atxHeadingSequence: onexitatxheadingsequence,
			autolink: closer(),
			autolinkEmail: onexitautolinkemail,
			autolinkProtocol: onexitautolinkprotocol,
			blockQuote: closer(),
			characterEscapeValue: onexitdata,
			characterReferenceMarkerHexadecimal: onexitcharacterreferencemarker,
			characterReferenceMarkerNumeric: onexitcharacterreferencemarker,
			characterReferenceValue: onexitcharacterreferencevalue,
			characterReference: onexitcharacterreference,
			codeFenced: closer(onexitcodefenced),
			codeFencedFence: onexitcodefencedfence,
			codeFencedFenceInfo: onexitcodefencedfenceinfo,
			codeFencedFenceMeta: onexitcodefencedfencemeta,
			codeFlowValue: onexitdata,
			codeIndented: closer(onexitcodeindented),
			codeText: closer(onexitcodetext),
			codeTextData: onexitdata,
			data: onexitdata,
			definition: closer(),
			definitionDestinationString: onexitdefinitiondestinationstring,
			definitionLabelString: onexitdefinitionlabelstring,
			definitionTitleString: onexitdefinitiontitlestring,
			emphasis: closer(),
			hardBreakEscape: closer(onexithardbreak),
			hardBreakTrailing: closer(onexithardbreak),
			htmlFlow: closer(onexithtmlflow),
			htmlFlowData: onexitdata,
			htmlText: closer(onexithtmltext),
			htmlTextData: onexitdata,
			image: closer(onexitimage),
			label: onexitlabel,
			labelText: onexitlabeltext,
			lineEnding: onexitlineending,
			link: closer(onexitlink),
			listItem: closer(),
			listOrdered: closer(),
			listUnordered: closer(),
			paragraph: closer(),
			referenceString: onexitreferencestring,
			resourceDestinationString: onexitresourcedestinationstring,
			resourceTitleString: onexitresourcetitlestring,
			resource: onexitresource,
			setextHeading: closer(onexitsetextheading),
			setextHeadingLineSequence: onexitsetextheadinglinesequence,
			setextHeadingText: onexitsetextheadingtext,
			strong: closer(),
			thematicBreak: closer()
		}
	};
	configure(config, (options || {}).mdastExtensions || []);
	const data = {};
	return compile;
	function compile(events) {
		let tree = {
			type: "root",
			children: []
		};
		const context = {
			stack: [tree],
			tokenStack: [],
			config,
			enter,
			exit: exit$2,
			buffer,
			resume,
			data
		};
		const listStack = [];
		let index$1 = -1;
		while (++index$1 < events.length) if (events[index$1][1].type === "listOrdered" || events[index$1][1].type === "listUnordered") if (events[index$1][0] === "enter") listStack.push(index$1);
		else index$1 = prepareList(events, listStack.pop(), index$1);
		index$1 = -1;
		while (++index$1 < events.length) {
			const handler = config[events[index$1][0]];
			if (own$1.call(handler, events[index$1][1].type)) handler[events[index$1][1].type].call(Object.assign({ sliceSerialize: events[index$1][2].sliceSerialize }, context), events[index$1][1]);
		}
		if (context.tokenStack.length > 0) {
			const tail = context.tokenStack[context.tokenStack.length - 1];
			(tail[1] || defaultOnError).call(context, void 0, tail[0]);
		}
		tree.position = {
			start: point(events.length > 0 ? events[0][1].start : {
				line: 1,
				column: 1,
				offset: 0
			}),
			end: point(events.length > 0 ? events[events.length - 2][1].end : {
				line: 1,
				column: 1,
				offset: 0
			})
		};
		index$1 = -1;
		while (++index$1 < config.transforms.length) tree = config.transforms[index$1](tree) || tree;
		return tree;
	}
	function prepareList(events, start, length) {
		let index$1 = start - 1;
		let containerBalance = -1;
		let listSpread = false;
		let listItem$2;
		let lineIndex;
		let firstBlankLineIndex;
		let atMarker;
		while (++index$1 <= length) {
			const event = events[index$1];
			switch (event[1].type) {
				case "listUnordered":
				case "listOrdered":
				case "blockQuote":
					if (event[0] === "enter") containerBalance++;
					else containerBalance--;
					atMarker = void 0;
					break;
				case "lineEndingBlank":
					if (event[0] === "enter") {
						if (listItem$2 && !atMarker && !containerBalance && !firstBlankLineIndex) firstBlankLineIndex = index$1;
						atMarker = void 0;
					}
					break;
				case "linePrefix":
				case "listItemValue":
				case "listItemMarker":
				case "listItemPrefix":
				case "listItemPrefixWhitespace": break;
				default: atMarker = void 0;
			}
			if (!containerBalance && event[0] === "enter" && event[1].type === "listItemPrefix" || containerBalance === -1 && event[0] === "exit" && (event[1].type === "listUnordered" || event[1].type === "listOrdered")) {
				if (listItem$2) {
					let tailIndex = index$1;
					lineIndex = void 0;
					while (tailIndex--) {
						const tailEvent = events[tailIndex];
						if (tailEvent[1].type === "lineEnding" || tailEvent[1].type === "lineEndingBlank") {
							if (tailEvent[0] === "exit") continue;
							if (lineIndex) {
								events[lineIndex][1].type = "lineEndingBlank";
								listSpread = true;
							}
							tailEvent[1].type = "lineEnding";
							lineIndex = tailIndex;
						} else if (tailEvent[1].type === "linePrefix" || tailEvent[1].type === "blockQuotePrefix" || tailEvent[1].type === "blockQuotePrefixWhitespace" || tailEvent[1].type === "blockQuoteMarker" || tailEvent[1].type === "listItemIndent") {} else break;
					}
					if (firstBlankLineIndex && (!lineIndex || firstBlankLineIndex < lineIndex)) listItem$2._spread = true;
					listItem$2.end = Object.assign({}, lineIndex ? events[lineIndex][1].start : event[1].end);
					events.splice(lineIndex || index$1, 0, [
						"exit",
						listItem$2,
						event[2]
					]);
					index$1++;
					length++;
				}
				if (event[1].type === "listItemPrefix") {
					const item = {
						type: "listItem",
						_spread: false,
						start: Object.assign({}, event[1].start),
						end: void 0
					};
					listItem$2 = item;
					events.splice(index$1, 0, [
						"enter",
						item,
						event[2]
					]);
					index$1++;
					length++;
					firstBlankLineIndex = void 0;
					atMarker = true;
				}
			}
		}
		events[start][1]._spread = listSpread;
		return length;
	}
	function opener(create, and) {
		return open;
		function open(token) {
			enter.call(this, create(token), token);
			if (and) and.call(this, token);
		}
	}
	function buffer() {
		this.stack.push({
			type: "fragment",
			children: []
		});
	}
	function enter(node$1, token, errorHandler) {
		this.stack[this.stack.length - 1].children.push(node$1);
		this.stack.push(node$1);
		this.tokenStack.push([token, errorHandler || void 0]);
		node$1.position = {
			start: point(token.start),
			end: void 0
		};
	}
	function closer(and) {
		return close;
		function close(token) {
			if (and) and.call(this, token);
			exit$2.call(this, token);
		}
	}
	function exit$2(token, onExitError) {
		const node$1 = this.stack.pop();
		const open = this.tokenStack.pop();
		if (!open) throw new Error("Cannot close `" + token.type + "` (" + stringifyPosition({
			start: token.start,
			end: token.end
		}) + "): it’s not open");
		else if (open[0].type !== token.type) if (onExitError) onExitError.call(this, token, open[0]);
		else (open[1] || defaultOnError).call(this, token, open[0]);
		node$1.position.end = point(token.end);
	}
	function resume() {
		return toString(this.stack.pop());
	}
	function onenterlistordered() {
		this.data.expectingFirstListItemValue = true;
	}
	function onenterlistitemvalue(token) {
		if (this.data.expectingFirstListItemValue) {
			const ancestor = this.stack[this.stack.length - 2];
			ancestor.start = Number.parseInt(this.sliceSerialize(token), 10);
			this.data.expectingFirstListItemValue = void 0;
		}
	}
	function onexitcodefencedfenceinfo() {
		const data$1 = this.resume();
		const node$1 = this.stack[this.stack.length - 1];
		node$1.lang = data$1;
	}
	function onexitcodefencedfencemeta() {
		const data$1 = this.resume();
		const node$1 = this.stack[this.stack.length - 1];
		node$1.meta = data$1;
	}
	function onexitcodefencedfence() {
		if (this.data.flowCodeInside) return;
		this.buffer();
		this.data.flowCodeInside = true;
	}
	function onexitcodefenced() {
		const data$1 = this.resume();
		const node$1 = this.stack[this.stack.length - 1];
		node$1.value = data$1.replace(/^(\r?\n|\r)|(\r?\n|\r)$/g, "");
		this.data.flowCodeInside = void 0;
	}
	function onexitcodeindented() {
		const data$1 = this.resume();
		const node$1 = this.stack[this.stack.length - 1];
		node$1.value = data$1.replace(/(\r?\n|\r)$/g, "");
	}
	function onexitdefinitionlabelstring(token) {
		const label = this.resume();
		const node$1 = this.stack[this.stack.length - 1];
		node$1.label = label;
		node$1.identifier = normalizeIdentifier(this.sliceSerialize(token)).toLowerCase();
	}
	function onexitdefinitiontitlestring() {
		const data$1 = this.resume();
		const node$1 = this.stack[this.stack.length - 1];
		node$1.title = data$1;
	}
	function onexitdefinitiondestinationstring() {
		const data$1 = this.resume();
		const node$1 = this.stack[this.stack.length - 1];
		node$1.url = data$1;
	}
	function onexitatxheadingsequence(token) {
		const node$1 = this.stack[this.stack.length - 1];
		if (!node$1.depth) node$1.depth = this.sliceSerialize(token).length;
	}
	function onexitsetextheadingtext() {
		this.data.setextHeadingSlurpLineEnding = true;
	}
	function onexitsetextheadinglinesequence(token) {
		const node$1 = this.stack[this.stack.length - 1];
		node$1.depth = this.sliceSerialize(token).codePointAt(0) === 61 ? 1 : 2;
	}
	function onexitsetextheading() {
		this.data.setextHeadingSlurpLineEnding = void 0;
	}
	function onenterdata(token) {
		const siblings = this.stack[this.stack.length - 1].children;
		let tail = siblings[siblings.length - 1];
		if (!tail || tail.type !== "text") {
			tail = text$4();
			tail.position = {
				start: point(token.start),
				end: void 0
			};
			siblings.push(tail);
		}
		this.stack.push(tail);
	}
	function onexitdata(token) {
		const tail = this.stack.pop();
		tail.value += this.sliceSerialize(token);
		tail.position.end = point(token.end);
	}
	function onexitlineending(token) {
		const context = this.stack[this.stack.length - 1];
		if (this.data.atHardBreak) {
			const tail = context.children[context.children.length - 1];
			tail.position.end = point(token.end);
			this.data.atHardBreak = void 0;
			return;
		}
		if (!this.data.setextHeadingSlurpLineEnding && config.canContainEols.includes(context.type)) {
			onenterdata.call(this, token);
			onexitdata.call(this, token);
		}
	}
	function onexithardbreak() {
		this.data.atHardBreak = true;
	}
	function onexithtmlflow() {
		const data$1 = this.resume();
		const node$1 = this.stack[this.stack.length - 1];
		node$1.value = data$1;
	}
	function onexithtmltext() {
		const data$1 = this.resume();
		const node$1 = this.stack[this.stack.length - 1];
		node$1.value = data$1;
	}
	function onexitcodetext() {
		const data$1 = this.resume();
		const node$1 = this.stack[this.stack.length - 1];
		node$1.value = data$1;
	}
	function onexitlink() {
		const node$1 = this.stack[this.stack.length - 1];
		if (this.data.inReference) {
			const referenceType = this.data.referenceType || "shortcut";
			node$1.type += "Reference";
			node$1.referenceType = referenceType;
			delete node$1.url;
			delete node$1.title;
		} else {
			delete node$1.identifier;
			delete node$1.label;
		}
		this.data.referenceType = void 0;
	}
	function onexitimage() {
		const node$1 = this.stack[this.stack.length - 1];
		if (this.data.inReference) {
			const referenceType = this.data.referenceType || "shortcut";
			node$1.type += "Reference";
			node$1.referenceType = referenceType;
			delete node$1.url;
			delete node$1.title;
		} else {
			delete node$1.identifier;
			delete node$1.label;
		}
		this.data.referenceType = void 0;
	}
	function onexitlabeltext(token) {
		const string$2 = this.sliceSerialize(token);
		const ancestor = this.stack[this.stack.length - 2];
		ancestor.label = decodeString(string$2);
		ancestor.identifier = normalizeIdentifier(string$2).toLowerCase();
	}
	function onexitlabel() {
		const fragment = this.stack[this.stack.length - 1];
		const value = this.resume();
		const node$1 = this.stack[this.stack.length - 1];
		this.data.inReference = true;
		if (node$1.type === "link") node$1.children = fragment.children;
		else node$1.alt = value;
	}
	function onexitresourcedestinationstring() {
		const data$1 = this.resume();
		const node$1 = this.stack[this.stack.length - 1];
		node$1.url = data$1;
	}
	function onexitresourcetitlestring() {
		const data$1 = this.resume();
		const node$1 = this.stack[this.stack.length - 1];
		node$1.title = data$1;
	}
	function onexitresource() {
		this.data.inReference = void 0;
	}
	function onenterreference() {
		this.data.referenceType = "collapsed";
	}
	function onexitreferencestring(token) {
		const label = this.resume();
		const node$1 = this.stack[this.stack.length - 1];
		node$1.label = label;
		node$1.identifier = normalizeIdentifier(this.sliceSerialize(token)).toLowerCase();
		this.data.referenceType = "full";
	}
	function onexitcharacterreferencemarker(token) {
		this.data.characterReferenceType = token.type;
	}
	function onexitcharacterreferencevalue(token) {
		const data$1 = this.sliceSerialize(token);
		const type = this.data.characterReferenceType;
		let value;
		if (type) {
			value = decodeNumericCharacterReference(data$1, type === "characterReferenceMarkerNumeric" ? 10 : 16);
			this.data.characterReferenceType = void 0;
		} else value = decodeNamedCharacterReference(data$1);
		const tail = this.stack[this.stack.length - 1];
		tail.value += value;
	}
	function onexitcharacterreference(token) {
		const tail = this.stack.pop();
		tail.position.end = point(token.end);
	}
	function onexitautolinkprotocol(token) {
		onexitdata.call(this, token);
		const node$1 = this.stack[this.stack.length - 1];
		node$1.url = this.sliceSerialize(token);
	}
	function onexitautolinkemail(token) {
		onexitdata.call(this, token);
		const node$1 = this.stack[this.stack.length - 1];
		node$1.url = "mailto:" + this.sliceSerialize(token);
	}
	function blockQuote$1() {
		return {
			type: "blockquote",
			children: []
		};
	}
	function codeFlow() {
		return {
			type: "code",
			lang: null,
			meta: null,
			value: ""
		};
	}
	function codeText$1() {
		return {
			type: "inlineCode",
			value: ""
		};
	}
	function definition$2() {
		return {
			type: "definition",
			identifier: "",
			label: null,
			title: null,
			url: ""
		};
	}
	function emphasis$1() {
		return {
			type: "emphasis",
			children: []
		};
	}
	function heading$1() {
		return {
			type: "heading",
			depth: 0,
			children: []
		};
	}
	function hardBreak$1() {
		return { type: "break" };
	}
	function html$1() {
		return {
			type: "html",
			value: ""
		};
	}
	function image$1() {
		return {
			type: "image",
			title: null,
			url: "",
			alt: null
		};
	}
	function link$1() {
		return {
			type: "link",
			title: null,
			url: "",
			children: []
		};
	}
	function list$2(token) {
		return {
			type: "list",
			ordered: token.type === "listOrdered",
			start: null,
			spread: token._spread,
			children: []
		};
	}
	function listItem$1(token) {
		return {
			type: "listItem",
			spread: token._spread,
			checked: null,
			children: []
		};
	}
	function paragraph$1() {
		return {
			type: "paragraph",
			children: []
		};
	}
	function strong$1() {
		return {
			type: "strong",
			children: []
		};
	}
	function text$4() {
		return {
			type: "text",
			value: ""
		};
	}
	function thematicBreak$2() {
		return { type: "thematicBreak" };
	}
}
function point(d) {
	return {
		line: d.line,
		column: d.column,
		offset: d.offset
	};
}
function configure(combined, extensions) {
	let index$1 = -1;
	while (++index$1 < extensions.length) {
		const value = extensions[index$1];
		if (Array.isArray(value)) configure(combined, value);
		else extension(combined, value);
	}
}
function extension(combined, extension$1) {
	let key;
	for (key in extension$1) if (own$1.call(extension$1, key)) switch (key) {
		case "canContainEols": {
			const right = extension$1[key];
			if (right) combined[key].push(...right);
			break;
		}
		case "transforms": {
			const right = extension$1[key];
			if (right) combined[key].push(...right);
			break;
		}
		case "enter":
		case "exit": {
			const right = extension$1[key];
			if (right) Object.assign(combined[key], right);
			break;
		}
	}
}
function defaultOnError(left, right) {
	if (left) throw new Error("Cannot close `" + left.type + "` (" + stringifyPosition({
		start: left.start,
		end: left.end
	}) + "): a different token (`" + right.type + "`, " + stringifyPosition({
		start: right.start,
		end: right.end
	}) + ") is open");
	else throw new Error("Cannot close document, a token (`" + right.type + "`, " + stringifyPosition({
		start: right.start,
		end: right.end
	}) + ") is still open");
}
function remarkParse(options) {
	const self = this;
	self.parser = parser;
	function parser(doc) {
		return fromMarkdown(doc, {
			...self.data("settings"),
			...options,
			extensions: self.data("micromarkExtensions") || [],
			mdastExtensions: self.data("fromMarkdownExtensions") || []
		});
	}
}
const convert = (function(test) {
	if (test === null || test === void 0) return ok$1;
	if (typeof test === "function") return castFactory(test);
	if (typeof test === "object") return Array.isArray(test) ? anyFactory(test) : propertiesFactory(test);
	if (typeof test === "string") return typeFactory(test);
	throw new Error("Expected function, string, or object as test");
});
function anyFactory(tests) {
	const checks = [];
	let index$1 = -1;
	while (++index$1 < tests.length) checks[index$1] = convert(tests[index$1]);
	return castFactory(any);
	function any(...parameters) {
		let index$2 = -1;
		while (++index$2 < checks.length) if (checks[index$2].apply(this, parameters)) return true;
		return false;
	}
}
function propertiesFactory(check) {
	const checkAsRecord = check;
	return castFactory(all$1);
	function all$1(node$1) {
		const nodeAsRecord = node$1;
		let key;
		for (key in check) if (nodeAsRecord[key] !== checkAsRecord[key]) return false;
		return true;
	}
}
function typeFactory(check) {
	return castFactory(type);
	function type(node$1) {
		return node$1 && node$1.type === check;
	}
}
function castFactory(testFunction) {
	return check;
	function check(value, index$1, parent) {
		return Boolean(looksLikeANode(value) && testFunction.call(this, value, typeof index$1 === "number" ? index$1 : void 0, parent || void 0));
	}
}
function ok$1() {
	return true;
}
function looksLikeANode(value) {
	return value !== null && typeof value === "object" && "type" in value;
}
function color(d) {
	return d;
}
var empty = [];
const SKIP = "skip";
function visitParents(tree, test, visitor, reverse) {
	let check;
	if (typeof test === "function" && typeof visitor !== "function") {
		reverse = visitor;
		visitor = test;
	} else check = test;
	const is = convert(check);
	const step = reverse ? -1 : 1;
	factory(tree, void 0, [])();
	function factory(node$1, index$1, parents) {
		const value = node$1 && typeof node$1 === "object" ? node$1 : {};
		if (typeof value.type === "string") {
			const name = typeof value.tagName === "string" ? value.tagName : typeof value.name === "string" ? value.name : void 0;
			Object.defineProperty(visit$1, "name", { value: "node (" + color(node$1.type + (name ? "<" + name + ">" : "")) + ")" });
		}
		return visit$1;
		function visit$1() {
			let result = empty;
			let subresult;
			let offset;
			let grandparents;
			if (!test || is(node$1, index$1, parents[parents.length - 1] || void 0)) {
				result = toResult(visitor(node$1, parents));
				if (result[0] === false) return result;
			}
			if ("children" in node$1 && node$1.children) {
				const nodeAsParent = node$1;
				if (nodeAsParent.children && result[0] !== "skip") {
					offset = (reverse ? nodeAsParent.children.length : -1) + step;
					grandparents = parents.concat(nodeAsParent);
					while (offset > -1 && offset < nodeAsParent.children.length) {
						const child = nodeAsParent.children[offset];
						subresult = factory(child, offset, grandparents)();
						if (subresult[0] === false) return subresult;
						offset = typeof subresult[1] === "number" ? subresult[1] : offset + step;
					}
				}
			}
			return result;
		}
	}
}
function toResult(value) {
	if (Array.isArray(value)) return value;
	if (typeof value === "number") return [true, value];
	return value === null || value === void 0 ? empty : [value];
}
function visit(tree, testOrVisitor, visitorOrReverse, maybeReverse) {
	let reverse;
	let test;
	let visitor;
	if (typeof testOrVisitor === "function" && typeof visitorOrReverse !== "function") {
		test = void 0;
		visitor = testOrVisitor;
		reverse = visitorOrReverse;
	} else {
		test = testOrVisitor;
		visitor = visitorOrReverse;
		reverse = maybeReverse;
	}
	visitParents(tree, test, overload, reverse);
	function overload(node$1, parents) {
		const parent = parents[parents.length - 1];
		const index$1 = parent ? parent.children.indexOf(node$1) : void 0;
		return visitor(node$1, index$1, parent);
	}
}
function bail(error) {
	if (error) throw error;
}
var require_extend = /* @__PURE__ */ __commonJSMin(((exports, module) => {
	var hasOwn = Object.prototype.hasOwnProperty;
	var toStr = Object.prototype.toString;
	var defineProperty = Object.defineProperty;
	var gOPD = Object.getOwnPropertyDescriptor;
	var isArray = function isArray$1(arr) {
		if (typeof Array.isArray === "function") return Array.isArray(arr);
		return toStr.call(arr) === "[object Array]";
	};
	var isPlainObject$1 = function isPlainObject$2(obj) {
		if (!obj || toStr.call(obj) !== "[object Object]") return false;
		var hasOwnConstructor = hasOwn.call(obj, "constructor");
		var hasIsPrototypeOf = obj.constructor && obj.constructor.prototype && hasOwn.call(obj.constructor.prototype, "isPrototypeOf");
		if (obj.constructor && !hasOwnConstructor && !hasIsPrototypeOf) return false;
		var key;
		for (key in obj);
		return typeof key === "undefined" || hasOwn.call(obj, key);
	};
	var setProperty = function setProperty$1(target, options) {
		if (defineProperty && options.name === "__proto__") defineProperty(target, options.name, {
			enumerable: true,
			configurable: true,
			value: options.newValue,
			writable: true
		});
		else target[options.name] = options.newValue;
	};
	var getProperty = function getProperty$1(obj, name) {
		if (name === "__proto__") {
			if (!hasOwn.call(obj, name)) return;
			else if (gOPD) return gOPD(obj, name).value;
		}
		return obj[name];
	};
	module.exports = function extend$1() {
		var options, name, src, copy, copyIsArray, clone;
		var target = arguments[0];
		var i = 1;
		var length = arguments.length;
		var deep = false;
		if (typeof target === "boolean") {
			deep = target;
			target = arguments[1] || {};
			i = 2;
		}
		if (target == null || typeof target !== "object" && typeof target !== "function") target = {};
		for (; i < length; ++i) {
			options = arguments[i];
			if (options != null) for (name in options) {
				src = getProperty(target, name);
				copy = getProperty(options, name);
				if (target !== copy) {
					if (deep && copy && (isPlainObject$1(copy) || (copyIsArray = isArray(copy)))) {
						if (copyIsArray) {
							copyIsArray = false;
							clone = src && isArray(src) ? src : [];
						} else clone = src && isPlainObject$1(src) ? src : {};
						setProperty(target, {
							name,
							newValue: extend$1(deep, clone, copy)
						});
					} else if (typeof copy !== "undefined") setProperty(target, {
						name,
						newValue: copy
					});
				}
			}
		}
		return target;
	};
}));
function isPlainObject(value) {
	if (typeof value !== "object" || value === null) return false;
	const prototype = Object.getPrototypeOf(value);
	return (prototype === null || prototype === Object.prototype || Object.getPrototypeOf(prototype) === null) && !(Symbol.toStringTag in value) && !(Symbol.iterator in value);
}
function trough() {
	const fns = [];
	const pipeline = {
		run,
		use
	};
	return pipeline;
	function run(...values) {
		let middlewareIndex = -1;
		const callback = values.pop();
		if (typeof callback !== "function") throw new TypeError("Expected function as last argument, not " + callback);
		next(null, ...values);
		function next(error, ...output) {
			const fn = fns[++middlewareIndex];
			let index$1 = -1;
			if (error) {
				callback(error);
				return;
			}
			while (++index$1 < values.length) if (output[index$1] === null || output[index$1] === void 0) output[index$1] = values[index$1];
			values = output;
			if (fn) wrap(fn, next)(...output);
			else callback(null, ...output);
		}
	}
	function use(middelware) {
		if (typeof middelware !== "function") throw new TypeError("Expected `middelware` to be a function, not " + middelware);
		fns.push(middelware);
		return pipeline;
	}
}
function wrap(middleware, callback) {
	let called;
	return wrapped;
	function wrapped(...parameters) {
		const fnExpectsCallback = middleware.length > parameters.length;
		let result;
		if (fnExpectsCallback) parameters.push(done);
		try {
			result = middleware.apply(this, parameters);
		} catch (error) {
			const exception = error;
			if (fnExpectsCallback && called) throw exception;
			return done(exception);
		}
		if (!fnExpectsCallback) if (result && result.then && typeof result.then === "function") result.then(then, done);
		else if (result instanceof Error) done(result);
		else then(result);
	}
	function done(error, ...output) {
		if (!called) {
			called = true;
			callback(error, ...output);
		}
	}
	function then(value) {
		done(null, value);
	}
}
const minpath = {
	basename,
	dirname,
	extname,
	join,
	sep: "/"
};
function basename(path$1, extname$1) {
	if (extname$1 !== void 0 && typeof extname$1 !== "string") throw new TypeError("\"ext\" argument must be a string");
	assertPath$1(path$1);
	let start = 0;
	let end = -1;
	let index$1 = path$1.length;
	let seenNonSlash;
	if (extname$1 === void 0 || extname$1.length === 0 || extname$1.length > path$1.length) {
		while (index$1--) if (path$1.codePointAt(index$1) === 47) {
			if (seenNonSlash) {
				start = index$1 + 1;
				break;
			}
		} else if (end < 0) {
			seenNonSlash = true;
			end = index$1 + 1;
		}
		return end < 0 ? "" : path$1.slice(start, end);
	}
	if (extname$1 === path$1) return "";
	let firstNonSlashEnd = -1;
	let extnameIndex = extname$1.length - 1;
	while (index$1--) if (path$1.codePointAt(index$1) === 47) {
		if (seenNonSlash) {
			start = index$1 + 1;
			break;
		}
	} else {
		if (firstNonSlashEnd < 0) {
			seenNonSlash = true;
			firstNonSlashEnd = index$1 + 1;
		}
		if (extnameIndex > -1) if (path$1.codePointAt(index$1) === extname$1.codePointAt(extnameIndex--)) {
			if (extnameIndex < 0) end = index$1;
		} else {
			extnameIndex = -1;
			end = firstNonSlashEnd;
		}
	}
	if (start === end) end = firstNonSlashEnd;
	else if (end < 0) end = path$1.length;
	return path$1.slice(start, end);
}
function dirname(path$1) {
	assertPath$1(path$1);
	if (path$1.length === 0) return ".";
	let end = -1;
	let index$1 = path$1.length;
	let unmatchedSlash;
	while (--index$1) if (path$1.codePointAt(index$1) === 47) {
		if (unmatchedSlash) {
			end = index$1;
			break;
		}
	} else if (!unmatchedSlash) unmatchedSlash = true;
	return end < 0 ? path$1.codePointAt(0) === 47 ? "/" : "." : end === 1 && path$1.codePointAt(0) === 47 ? "//" : path$1.slice(0, end);
}
function extname(path$1) {
	assertPath$1(path$1);
	let index$1 = path$1.length;
	let end = -1;
	let startPart = 0;
	let startDot = -1;
	let preDotState = 0;
	let unmatchedSlash;
	while (index$1--) {
		const code$2 = path$1.codePointAt(index$1);
		if (code$2 === 47) {
			if (unmatchedSlash) {
				startPart = index$1 + 1;
				break;
			}
			continue;
		}
		if (end < 0) {
			unmatchedSlash = true;
			end = index$1 + 1;
		}
		if (code$2 === 46) {
			if (startDot < 0) startDot = index$1;
			else if (preDotState !== 1) preDotState = 1;
		} else if (startDot > -1) preDotState = -1;
	}
	if (startDot < 0 || end < 0 || preDotState === 0 || preDotState === 1 && startDot === end - 1 && startDot === startPart + 1) return "";
	return path$1.slice(startDot, end);
}
function join(...segments) {
	let index$1 = -1;
	let joined;
	while (++index$1 < segments.length) {
		assertPath$1(segments[index$1]);
		if (segments[index$1]) joined = joined === void 0 ? segments[index$1] : joined + "/" + segments[index$1];
	}
	return joined === void 0 ? "." : normalize(joined);
}
function normalize(path$1) {
	assertPath$1(path$1);
	const absolute = path$1.codePointAt(0) === 47;
	let value = normalizeString(path$1, !absolute);
	if (value.length === 0 && !absolute) value = ".";
	if (value.length > 0 && path$1.codePointAt(path$1.length - 1) === 47) value += "/";
	return absolute ? "/" + value : value;
}
function normalizeString(path$1, allowAboveRoot) {
	let result = "";
	let lastSegmentLength = 0;
	let lastSlash = -1;
	let dots = 0;
	let index$1 = -1;
	let code$2;
	let lastSlashIndex;
	while (++index$1 <= path$1.length) {
		if (index$1 < path$1.length) code$2 = path$1.codePointAt(index$1);
		else if (code$2 === 47) break;
		else code$2 = 47;
		if (code$2 === 47) {
			if (lastSlash === index$1 - 1 || dots === 1) {} else if (lastSlash !== index$1 - 1 && dots === 2) {
				if (result.length < 2 || lastSegmentLength !== 2 || result.codePointAt(result.length - 1) !== 46 || result.codePointAt(result.length - 2) !== 46) {
					if (result.length > 2) {
						lastSlashIndex = result.lastIndexOf("/");
						if (lastSlashIndex !== result.length - 1) {
							if (lastSlashIndex < 0) {
								result = "";
								lastSegmentLength = 0;
							} else {
								result = result.slice(0, lastSlashIndex);
								lastSegmentLength = result.length - 1 - result.lastIndexOf("/");
							}
							lastSlash = index$1;
							dots = 0;
							continue;
						}
					} else if (result.length > 0) {
						result = "";
						lastSegmentLength = 0;
						lastSlash = index$1;
						dots = 0;
						continue;
					}
				}
				if (allowAboveRoot) {
					result = result.length > 0 ? result + "/.." : "..";
					lastSegmentLength = 2;
				}
			} else {
				if (result.length > 0) result += "/" + path$1.slice(lastSlash + 1, index$1);
				else result = path$1.slice(lastSlash + 1, index$1);
				lastSegmentLength = index$1 - lastSlash - 1;
			}
			lastSlash = index$1;
			dots = 0;
		} else if (code$2 === 46 && dots > -1) dots++;
		else dots = -1;
	}
	return result;
}
function assertPath$1(path$1) {
	if (typeof path$1 !== "string") throw new TypeError("Path must be a string. Received " + JSON.stringify(path$1));
}
const minproc = { cwd };
function cwd() {
	return "/";
}
function isUrl(fileUrlOrPath) {
	return Boolean(fileUrlOrPath !== null && typeof fileUrlOrPath === "object" && "href" in fileUrlOrPath && fileUrlOrPath.href && "protocol" in fileUrlOrPath && fileUrlOrPath.protocol && fileUrlOrPath.auth === void 0);
}
function urlToPath(path$1) {
	if (typeof path$1 === "string") path$1 = new URL(path$1);
	else if (!isUrl(path$1)) {
		const error = /* @__PURE__ */ new TypeError("The \"path\" argument must be of type string or an instance of URL. Received `" + path$1 + "`");
		error.code = "ERR_INVALID_ARG_TYPE";
		throw error;
	}
	if (path$1.protocol !== "file:") {
		const error = /* @__PURE__ */ new TypeError("The URL must be of scheme file");
		error.code = "ERR_INVALID_URL_SCHEME";
		throw error;
	}
	return getPathFromURLPosix(path$1);
}
function getPathFromURLPosix(url) {
	if (url.hostname !== "") {
		const error = /* @__PURE__ */ new TypeError("File URL host must be \"localhost\" or empty on darwin");
		error.code = "ERR_INVALID_FILE_URL_HOST";
		throw error;
	}
	const pathname = url.pathname;
	let index$1 = -1;
	while (++index$1 < pathname.length) if (pathname.codePointAt(index$1) === 37 && pathname.codePointAt(index$1 + 1) === 50) {
		const third = pathname.codePointAt(index$1 + 2);
		if (third === 70 || third === 102) {
			const error = /* @__PURE__ */ new TypeError("File URL path must not include encoded / characters");
			error.code = "ERR_INVALID_FILE_URL_PATH";
			throw error;
		}
	}
	return decodeURIComponent(pathname);
}
var order = [
	"history",
	"path",
	"basename",
	"stem",
	"extname",
	"dirname"
];
var VFile = class {
	constructor(value) {
		let options;
		if (!value) options = {};
		else if (isUrl(value)) options = { path: value };
		else if (typeof value === "string" || isUint8Array$1(value)) options = { value };
		else options = value;
		this.cwd = "cwd" in options ? "" : minproc.cwd();
		this.data = {};
		this.history = [];
		this.messages = [];
		this.value;
		this.map;
		this.result;
		this.stored;
		let index$1 = -1;
		while (++index$1 < order.length) {
			const field$1 = order[index$1];
			if (field$1 in options && options[field$1] !== void 0 && options[field$1] !== null) this[field$1] = field$1 === "history" ? [...options[field$1]] : options[field$1];
		}
		let field;
		for (field in options) if (!order.includes(field)) this[field] = options[field];
	}
	get basename() {
		return typeof this.path === "string" ? minpath.basename(this.path) : void 0;
	}
	set basename(basename$1) {
		assertNonEmpty(basename$1, "basename");
		assertPart(basename$1, "basename");
		this.path = minpath.join(this.dirname || "", basename$1);
	}
	get dirname() {
		return typeof this.path === "string" ? minpath.dirname(this.path) : void 0;
	}
	set dirname(dirname$1) {
		assertPath(this.basename, "dirname");
		this.path = minpath.join(dirname$1 || "", this.basename);
	}
	get extname() {
		return typeof this.path === "string" ? minpath.extname(this.path) : void 0;
	}
	set extname(extname$1) {
		assertPart(extname$1, "extname");
		assertPath(this.dirname, "extname");
		if (extname$1) {
			if (extname$1.codePointAt(0) !== 46) throw new Error("`extname` must start with `.`");
			if (extname$1.includes(".", 1)) throw new Error("`extname` cannot contain multiple dots");
		}
		this.path = minpath.join(this.dirname, this.stem + (extname$1 || ""));
	}
	get path() {
		return this.history[this.history.length - 1];
	}
	set path(path$1) {
		if (isUrl(path$1)) path$1 = urlToPath(path$1);
		assertNonEmpty(path$1, "path");
		if (this.path !== path$1) this.history.push(path$1);
	}
	get stem() {
		return typeof this.path === "string" ? minpath.basename(this.path, this.extname) : void 0;
	}
	set stem(stem) {
		assertNonEmpty(stem, "stem");
		assertPart(stem, "stem");
		this.path = minpath.join(this.dirname || "", stem + (this.extname || ""));
	}
	fail(causeOrReason, optionsOrParentOrPlace, origin) {
		const message = this.message(causeOrReason, optionsOrParentOrPlace, origin);
		message.fatal = true;
		throw message;
	}
	info(causeOrReason, optionsOrParentOrPlace, origin) {
		const message = this.message(causeOrReason, optionsOrParentOrPlace, origin);
		message.fatal = void 0;
		return message;
	}
	message(causeOrReason, optionsOrParentOrPlace, origin) {
		const message = new VFileMessage(causeOrReason, optionsOrParentOrPlace, origin);
		if (this.path) {
			message.name = this.path + ":" + message.name;
			message.file = this.path;
		}
		message.fatal = false;
		this.messages.push(message);
		return message;
	}
	toString(encoding) {
		if (this.value === void 0) return "";
		if (typeof this.value === "string") return this.value;
		return new TextDecoder(encoding || void 0).decode(this.value);
	}
};
function assertPart(part, name) {
	if (part && part.includes(minpath.sep)) throw new Error("`" + name + "` cannot be a path: did not expect `" + minpath.sep + "`");
}
function assertNonEmpty(part, name) {
	if (!part) throw new Error("`" + name + "` cannot be empty");
}
function assertPath(path$1, name) {
	if (!path$1) throw new Error("Setting `" + name + "` requires `path` to be set too");
}
function isUint8Array$1(value) {
	return Boolean(value && typeof value === "object" && "byteLength" in value && "byteOffset" in value);
}
const CallableInstance = (function(property) {
	const proto = this.constructor.prototype;
	const value = proto[property];
	const apply = function() {
		return value.apply(apply, arguments);
	};
	Object.setPrototypeOf(apply, proto);
	return apply;
});
var import_extend = /* @__PURE__ */ __toESM(require_extend(), 1);
var own = {}.hasOwnProperty;
const unified = new class Processor extends CallableInstance {
	constructor() {
		super("copy");
		this.Compiler = void 0;
		this.Parser = void 0;
		this.attachers = [];
		this.compiler = void 0;
		this.freezeIndex = -1;
		this.frozen = void 0;
		this.namespace = {};
		this.parser = void 0;
		this.transformers = trough();
	}
	copy() {
		const destination = new Processor();
		let index$1 = -1;
		while (++index$1 < this.attachers.length) {
			const attacher = this.attachers[index$1];
			destination.use(...attacher);
		}
		destination.data((0, import_extend.default)(true, {}, this.namespace));
		return destination;
	}
	data(key, value) {
		if (typeof key === "string") {
			if (arguments.length === 2) {
				assertUnfrozen("data", this.frozen);
				this.namespace[key] = value;
				return this;
			}
			return own.call(this.namespace, key) && this.namespace[key] || void 0;
		}
		if (key) {
			assertUnfrozen("data", this.frozen);
			this.namespace = key;
			return this;
		}
		return this.namespace;
	}
	freeze() {
		if (this.frozen) return this;
		const self = this;
		while (++this.freezeIndex < this.attachers.length) {
			const [attacher, ...options] = this.attachers[this.freezeIndex];
			if (options[0] === false) continue;
			if (options[0] === true) options[0] = void 0;
			const transformer = attacher.call(self, ...options);
			if (typeof transformer === "function") this.transformers.use(transformer);
		}
		this.frozen = true;
		this.freezeIndex = Number.POSITIVE_INFINITY;
		return this;
	}
	parse(file) {
		this.freeze();
		const realFile = vfile(file);
		const parser = this.parser || this.Parser;
		assertParser("parse", parser);
		return parser(String(realFile), realFile);
	}
	process(file, done) {
		const self = this;
		this.freeze();
		assertParser("process", this.parser || this.Parser);
		assertCompiler("process", this.compiler || this.Compiler);
		return done ? executor(void 0, done) : new Promise(executor);
		function executor(resolve, reject) {
			const realFile = vfile(file);
			const parseTree = self.parse(realFile);
			self.run(parseTree, realFile, function(error, tree, file$1) {
				if (error || !tree || !file$1) return realDone(error);
				const compileTree = tree;
				const compileResult = self.stringify(compileTree, file$1);
				if (looksLikeAValue(compileResult)) file$1.value = compileResult;
				else file$1.result = compileResult;
				realDone(error, file$1);
			});
			function realDone(error, file$1) {
				if (error || !file$1) reject(error);
				else if (resolve) resolve(file$1);
				else done(void 0, file$1);
			}
		}
	}
	processSync(file) {
		let complete = false;
		let result;
		this.freeze();
		assertParser("processSync", this.parser || this.Parser);
		assertCompiler("processSync", this.compiler || this.Compiler);
		this.process(file, realDone);
		assertDone("processSync", "process", complete);
		return result;
		function realDone(error, file$1) {
			complete = true;
			bail(error);
			result = file$1;
		}
	}
	run(tree, file, done) {
		assertNode(tree);
		this.freeze();
		const transformers = this.transformers;
		if (!done && typeof file === "function") {
			done = file;
			file = void 0;
		}
		return done ? executor(void 0, done) : new Promise(executor);
		function executor(resolve, reject) {
			const realFile = vfile(file);
			transformers.run(tree, realFile, realDone);
			function realDone(error, outputTree, file$1) {
				const resultingTree = outputTree || tree;
				if (error) reject(error);
				else if (resolve) resolve(resultingTree);
				else done(void 0, resultingTree, file$1);
			}
		}
	}
	runSync(tree, file) {
		let complete = false;
		let result;
		this.run(tree, file, realDone);
		assertDone("runSync", "run", complete);
		return result;
		function realDone(error, tree$1) {
			bail(error);
			result = tree$1;
			complete = true;
		}
	}
	stringify(tree, file) {
		this.freeze();
		const realFile = vfile(file);
		const compiler$1 = this.compiler || this.Compiler;
		assertCompiler("stringify", compiler$1);
		assertNode(tree);
		return compiler$1(tree, realFile);
	}
	use(value, ...parameters) {
		const attachers = this.attachers;
		const namespace = this.namespace;
		assertUnfrozen("use", this.frozen);
		if (value === null || value === void 0) {} else if (typeof value === "function") addPlugin(value, parameters);
		else if (typeof value === "object") if (Array.isArray(value)) addList(value);
		else addPreset(value);
		else throw new TypeError("Expected usable value, not `" + value + "`");
		return this;
		function add(value$1) {
			if (typeof value$1 === "function") addPlugin(value$1, []);
			else if (typeof value$1 === "object") if (Array.isArray(value$1)) {
				const [plugin, ...parameters$1] = value$1;
				addPlugin(plugin, parameters$1);
			} else addPreset(value$1);
			else throw new TypeError("Expected usable value, not `" + value$1 + "`");
		}
		function addPreset(result) {
			if (!("plugins" in result) && !("settings" in result)) throw new Error("Expected usable value but received an empty preset, which is probably a mistake: presets typically come with `plugins` and sometimes with `settings`, but this has neither");
			addList(result.plugins);
			if (result.settings) namespace.settings = (0, import_extend.default)(true, namespace.settings, result.settings);
		}
		function addList(plugins) {
			let index$1 = -1;
			if (plugins === null || plugins === void 0) {} else if (Array.isArray(plugins)) while (++index$1 < plugins.length) {
				const thing = plugins[index$1];
				add(thing);
			}
			else throw new TypeError("Expected a list of plugins, not `" + plugins + "`");
		}
		function addPlugin(plugin, parameters$1) {
			let index$1 = -1;
			let entryIndex = -1;
			while (++index$1 < attachers.length) if (attachers[index$1][0] === plugin) {
				entryIndex = index$1;
				break;
			}
			if (entryIndex === -1) attachers.push([plugin, ...parameters$1]);
			else if (parameters$1.length > 0) {
				let [primary, ...rest] = parameters$1;
				const currentPrimary = attachers[entryIndex][1];
				if (isPlainObject(currentPrimary) && isPlainObject(primary)) primary = (0, import_extend.default)(true, currentPrimary, primary);
				attachers[entryIndex] = [
					plugin,
					primary,
					...rest
				];
			}
		}
	}
}().freeze();
function assertParser(name, value) {
	if (typeof value !== "function") throw new TypeError("Cannot `" + name + "` without `parser`");
}
function assertCompiler(name, value) {
	if (typeof value !== "function") throw new TypeError("Cannot `" + name + "` without `compiler`");
}
function assertUnfrozen(name, frozen) {
	if (frozen) throw new Error("Cannot call `" + name + "` on a frozen processor.\nCreate a new processor first, by calling it: use `processor()` instead of `processor`.");
}
function assertNode(node$1) {
	if (!isPlainObject(node$1) || typeof node$1.type !== "string") throw new TypeError("Expected node, got `" + node$1 + "`");
}
function assertDone(name, asyncName, complete) {
	if (!complete) throw new Error("`" + name + "` finished async. Use `" + asyncName + "` instead");
}
function vfile(value) {
	return looksLikeAVFile(value) ? value : new VFile(value);
}
function looksLikeAVFile(value) {
	return Boolean(value && typeof value === "object" && "message" in value && "messages" in value);
}
function looksLikeAValue(value) {
	return typeof value === "string" || isUint8Array(value);
}
function isUint8Array(value) {
	return Boolean(value && typeof value === "object" && "byteLength" in value && "byteOffset" in value);
}
function ccount(value, character) {
	const source = String(value);
	if (typeof character !== "string") throw new TypeError("Expected character");
	let count = 0;
	let index$1 = source.indexOf(character);
	while (index$1 !== -1) {
		count++;
		index$1 = source.indexOf(character, index$1 + character.length);
	}
	return count;
}
function escapeStringRegexp(string$2) {
	if (typeof string$2 !== "string") throw new TypeError("Expected a string");
	return string$2.replace(/[|\\{}()[\]^$+*?.]/g, "\\$&").replace(/-/g, "\\x2d");
}
function findAndReplace(tree, list$2, options) {
	const ignored = convert((options || {}).ignore || []);
	const pairs = toPairs(list$2);
	let pairIndex = -1;
	while (++pairIndex < pairs.length) visitParents(tree, "text", visitor);
	function visitor(node$1, parents) {
		let index$1 = -1;
		let grandparent;
		while (++index$1 < parents.length) {
			const parent = parents[index$1];
			const siblings = grandparent ? grandparent.children : void 0;
			if (ignored(parent, siblings ? siblings.indexOf(parent) : void 0, grandparent)) return;
			grandparent = parent;
		}
		if (grandparent) return handler(node$1, parents);
	}
	function handler(node$1, parents) {
		const parent = parents[parents.length - 1];
		const find = pairs[pairIndex][0];
		const replace$1 = pairs[pairIndex][1];
		let start = 0;
		const index$1 = parent.children.indexOf(node$1);
		let change = false;
		let nodes = [];
		find.lastIndex = 0;
		let match = find.exec(node$1.value);
		while (match) {
			const position$1 = match.index;
			const matchObject = {
				index: match.index,
				input: match.input,
				stack: [...parents, node$1]
			};
			let value = replace$1(...match, matchObject);
			if (typeof value === "string") value = value.length > 0 ? {
				type: "text",
				value
			} : void 0;
			if (value === false) find.lastIndex = position$1 + 1;
			else {
				if (start !== position$1) nodes.push({
					type: "text",
					value: node$1.value.slice(start, position$1)
				});
				if (Array.isArray(value)) nodes.push(...value);
				else if (value) nodes.push(value);
				start = position$1 + match[0].length;
				change = true;
			}
			if (!find.global) break;
			match = find.exec(node$1.value);
		}
		if (change) {
			if (start < node$1.value.length) nodes.push({
				type: "text",
				value: node$1.value.slice(start)
			});
			parent.children.splice(index$1, 1, ...nodes);
		} else nodes = [node$1];
		return index$1 + nodes.length;
	}
}
function toPairs(tupleOrList) {
	const result = [];
	if (!Array.isArray(tupleOrList)) throw new TypeError("Expected find and replace tuple or list of tuples");
	const list$2 = !tupleOrList[0] || Array.isArray(tupleOrList[0]) ? tupleOrList : [tupleOrList];
	let index$1 = -1;
	while (++index$1 < list$2.length) {
		const tuple = list$2[index$1];
		result.push([toExpression(tuple[0]), toFunction(tuple[1])]);
	}
	return result;
}
function toExpression(find) {
	return typeof find === "string" ? new RegExp(escapeStringRegexp(find), "g") : find;
}
function toFunction(replace$1) {
	return typeof replace$1 === "function" ? replace$1 : function() {
		return replace$1;
	};
}
var inConstruct = "phrasing";
var notInConstruct = [
	"autolink",
	"link",
	"image",
	"label"
];
function gfmAutolinkLiteralFromMarkdown() {
	return {
		transforms: [transformGfmAutolinkLiterals],
		enter: {
			literalAutolink: enterLiteralAutolink,
			literalAutolinkEmail: enterLiteralAutolinkValue,
			literalAutolinkHttp: enterLiteralAutolinkValue,
			literalAutolinkWww: enterLiteralAutolinkValue
		},
		exit: {
			literalAutolink: exitLiteralAutolink,
			literalAutolinkEmail: exitLiteralAutolinkEmail,
			literalAutolinkHttp: exitLiteralAutolinkHttp,
			literalAutolinkWww: exitLiteralAutolinkWww
		}
	};
}
function gfmAutolinkLiteralToMarkdown() {
	return { unsafe: [
		{
			character: "@",
			before: "[+\\-.\\w]",
			after: "[\\-.\\w]",
			inConstruct,
			notInConstruct
		},
		{
			character: ".",
			before: "[Ww]",
			after: "[\\-.\\w]",
			inConstruct,
			notInConstruct
		},
		{
			character: ":",
			before: "[ps]",
			after: "\\/",
			inConstruct,
			notInConstruct
		}
	] };
}
function enterLiteralAutolink(token) {
	this.enter({
		type: "link",
		title: null,
		url: "",
		children: []
	}, token);
}
function enterLiteralAutolinkValue(token) {
	this.config.enter.autolinkProtocol.call(this, token);
}
function exitLiteralAutolinkHttp(token) {
	this.config.exit.autolinkProtocol.call(this, token);
}
function exitLiteralAutolinkWww(token) {
	this.config.exit.data.call(this, token);
	const node$1 = this.stack[this.stack.length - 1];
	node$1.type;
	node$1.url = "http://" + this.sliceSerialize(token);
}
function exitLiteralAutolinkEmail(token) {
	this.config.exit.autolinkEmail.call(this, token);
}
function exitLiteralAutolink(token) {
	this.exit(token);
}
function transformGfmAutolinkLiterals(tree) {
	findAndReplace(tree, [[/(https?:\/\/|www(?=\.))([-.\w]+)([^ \t\r\n]*)/gi, findUrl], [/(?<=^|\s|\p{P}|\p{S})([-.\w+]+)@([-\w]+(?:\.[-\w]+)+)/gu, findEmail]], { ignore: ["link", "linkReference"] });
}
function findUrl(_, protocol, domain$1, path$1, match) {
	let prefix = "";
	if (!previous(match)) return false;
	if (/^w/i.test(protocol)) {
		domain$1 = protocol + domain$1;
		protocol = "";
		prefix = "http://";
	}
	if (!isCorrectDomain(domain$1)) return false;
	const parts = splitUrl(domain$1 + path$1);
	if (!parts[0]) return false;
	const result = {
		type: "link",
		title: null,
		url: prefix + protocol + parts[0],
		children: [{
			type: "text",
			value: protocol + parts[0]
		}]
	};
	if (parts[1]) return [result, {
		type: "text",
		value: parts[1]
	}];
	return result;
}
function findEmail(_, atext, label, match) {
	if (!previous(match, true) || /[-\d_]$/.test(label)) return false;
	return {
		type: "link",
		title: null,
		url: "mailto:" + atext + "@" + label,
		children: [{
			type: "text",
			value: atext + "@" + label
		}]
	};
}
function isCorrectDomain(domain$1) {
	const parts = domain$1.split(".");
	if (parts.length < 2 || parts[parts.length - 1] && (/_/.test(parts[parts.length - 1]) || !/[a-zA-Z\d]/.test(parts[parts.length - 1])) || parts[parts.length - 2] && (/_/.test(parts[parts.length - 2]) || !/[a-zA-Z\d]/.test(parts[parts.length - 2]))) return false;
	return true;
}
function splitUrl(url) {
	const trailExec = /[!"&'),.:;<>?\]}]+$/.exec(url);
	if (!trailExec) return [url, void 0];
	url = url.slice(0, trailExec.index);
	let trail$1 = trailExec[0];
	let closingParenIndex = trail$1.indexOf(")");
	const openingParens = ccount(url, "(");
	let closingParens = ccount(url, ")");
	while (closingParenIndex !== -1 && openingParens > closingParens) {
		url += trail$1.slice(0, closingParenIndex + 1);
		trail$1 = trail$1.slice(closingParenIndex + 1);
		closingParenIndex = trail$1.indexOf(")");
		closingParens++;
	}
	return [url, trail$1];
}
function previous(match, email) {
	const code$2 = match.input.charCodeAt(match.index - 1);
	return (match.index === 0 || unicodeWhitespace(code$2) || unicodePunctuation(code$2)) && (!email || code$2 !== 47);
}
footnoteReference.peek = footnoteReferencePeek;
function enterFootnoteCallString() {
	this.buffer();
}
function enterFootnoteCall(token) {
	this.enter({
		type: "footnoteReference",
		identifier: "",
		label: ""
	}, token);
}
function enterFootnoteDefinitionLabelString() {
	this.buffer();
}
function enterFootnoteDefinition(token) {
	this.enter({
		type: "footnoteDefinition",
		identifier: "",
		label: "",
		children: []
	}, token);
}
function exitFootnoteCallString(token) {
	const label = this.resume();
	const node$1 = this.stack[this.stack.length - 1];
	node$1.type;
	node$1.identifier = normalizeIdentifier(this.sliceSerialize(token)).toLowerCase();
	node$1.label = label;
}
function exitFootnoteCall(token) {
	this.exit(token);
}
function exitFootnoteDefinitionLabelString(token) {
	const label = this.resume();
	const node$1 = this.stack[this.stack.length - 1];
	node$1.type;
	node$1.identifier = normalizeIdentifier(this.sliceSerialize(token)).toLowerCase();
	node$1.label = label;
}
function exitFootnoteDefinition(token) {
	this.exit(token);
}
function footnoteReferencePeek() {
	return "[";
}
function footnoteReference(node$1, _, state, info) {
	const tracker = state.createTracker(info);
	let value = tracker.move("[^");
	const exit$2 = state.enter("footnoteReference");
	const subexit = state.enter("reference");
	value += tracker.move(state.safe(state.associationId(node$1), {
		after: "]",
		before: value
	}));
	subexit();
	exit$2();
	value += tracker.move("]");
	return value;
}
function gfmFootnoteFromMarkdown() {
	return {
		enter: {
			gfmFootnoteCallString: enterFootnoteCallString,
			gfmFootnoteCall: enterFootnoteCall,
			gfmFootnoteDefinitionLabelString: enterFootnoteDefinitionLabelString,
			gfmFootnoteDefinition: enterFootnoteDefinition
		},
		exit: {
			gfmFootnoteCallString: exitFootnoteCallString,
			gfmFootnoteCall: exitFootnoteCall,
			gfmFootnoteDefinitionLabelString: exitFootnoteDefinitionLabelString,
			gfmFootnoteDefinition: exitFootnoteDefinition
		}
	};
}
function gfmFootnoteToMarkdown(options) {
	let firstLineBlank = false;
	if (options && options.firstLineBlank) firstLineBlank = true;
	return {
		handlers: {
			footnoteDefinition,
			footnoteReference
		},
		unsafe: [{
			character: "[",
			inConstruct: [
				"label",
				"phrasing",
				"reference"
			]
		}]
	};
	function footnoteDefinition(node$1, _, state, info) {
		const tracker = state.createTracker(info);
		let value = tracker.move("[^");
		const exit$2 = state.enter("footnoteDefinition");
		const subexit = state.enter("label");
		value += tracker.move(state.safe(state.associationId(node$1), {
			before: value,
			after: "]"
		}));
		subexit();
		value += tracker.move("]:");
		if (node$1.children && node$1.children.length > 0) {
			tracker.shift(4);
			value += tracker.move((firstLineBlank ? "\n" : " ") + state.indentLines(state.containerFlow(node$1, tracker.current()), firstLineBlank ? mapAll : mapExceptFirst));
		}
		exit$2();
		return value;
	}
}
function mapExceptFirst(line, index$1, blank) {
	return index$1 === 0 ? line : mapAll(line, index$1, blank);
}
function mapAll(line, index$1, blank) {
	return (blank ? "" : "    ") + line;
}
var constructsWithoutStrikethrough = [
	"autolink",
	"destinationLiteral",
	"destinationRaw",
	"reference",
	"titleQuote",
	"titleApostrophe"
];
handleDelete.peek = peekDelete;
function gfmStrikethroughFromMarkdown() {
	return {
		canContainEols: ["delete"],
		enter: { strikethrough: enterStrikethrough },
		exit: { strikethrough: exitStrikethrough }
	};
}
function gfmStrikethroughToMarkdown() {
	return {
		unsafe: [{
			character: "~",
			inConstruct: "phrasing",
			notInConstruct: constructsWithoutStrikethrough
		}],
		handlers: { delete: handleDelete }
	};
}
function enterStrikethrough(token) {
	this.enter({
		type: "delete",
		children: []
	}, token);
}
function exitStrikethrough(token) {
	this.exit(token);
}
function handleDelete(node$1, _, state, info) {
	const tracker = state.createTracker(info);
	const exit$2 = state.enter("strikethrough");
	let value = tracker.move("~~");
	value += state.containerPhrasing(node$1, {
		...tracker.current(),
		before: value,
		after: "~"
	});
	value += tracker.move("~~");
	exit$2();
	return value;
}
function peekDelete() {
	return "~";
}
function defaultStringLength(value) {
	return value.length;
}
function markdownTable(table, options) {
	const settings = options || {};
	const align = (settings.align || []).concat();
	const stringLength = settings.stringLength || defaultStringLength;
	const alignments = [];
	const cellMatrix = [];
	const sizeMatrix = [];
	const longestCellByColumn = [];
	let mostCellsPerRow = 0;
	let rowIndex = -1;
	while (++rowIndex < table.length) {
		const row$1 = [];
		const sizes$1 = [];
		let columnIndex$1 = -1;
		if (table[rowIndex].length > mostCellsPerRow) mostCellsPerRow = table[rowIndex].length;
		while (++columnIndex$1 < table[rowIndex].length) {
			const cell = serialize(table[rowIndex][columnIndex$1]);
			if (settings.alignDelimiters !== false) {
				const size = stringLength(cell);
				sizes$1[columnIndex$1] = size;
				if (longestCellByColumn[columnIndex$1] === void 0 || size > longestCellByColumn[columnIndex$1]) longestCellByColumn[columnIndex$1] = size;
			}
			row$1.push(cell);
		}
		cellMatrix[rowIndex] = row$1;
		sizeMatrix[rowIndex] = sizes$1;
	}
	let columnIndex = -1;
	if (typeof align === "object" && "length" in align) while (++columnIndex < mostCellsPerRow) alignments[columnIndex] = toAlignment(align[columnIndex]);
	else {
		const code$2 = toAlignment(align);
		while (++columnIndex < mostCellsPerRow) alignments[columnIndex] = code$2;
	}
	columnIndex = -1;
	const row = [];
	const sizes = [];
	while (++columnIndex < mostCellsPerRow) {
		const code$2 = alignments[columnIndex];
		let before = "";
		let after = "";
		if (code$2 === 99) {
			before = ":";
			after = ":";
		} else if (code$2 === 108) before = ":";
		else if (code$2 === 114) after = ":";
		let size = settings.alignDelimiters === false ? 1 : Math.max(1, longestCellByColumn[columnIndex] - before.length - after.length);
		const cell = before + "-".repeat(size) + after;
		if (settings.alignDelimiters !== false) {
			size = before.length + size + after.length;
			if (size > longestCellByColumn[columnIndex]) longestCellByColumn[columnIndex] = size;
			sizes[columnIndex] = size;
		}
		row[columnIndex] = cell;
	}
	cellMatrix.splice(1, 0, row);
	sizeMatrix.splice(1, 0, sizes);
	rowIndex = -1;
	const lines = [];
	while (++rowIndex < cellMatrix.length) {
		const row$1 = cellMatrix[rowIndex];
		const sizes$1 = sizeMatrix[rowIndex];
		columnIndex = -1;
		const line = [];
		while (++columnIndex < mostCellsPerRow) {
			const cell = row$1[columnIndex] || "";
			let before = "";
			let after = "";
			if (settings.alignDelimiters !== false) {
				const size = longestCellByColumn[columnIndex] - (sizes$1[columnIndex] || 0);
				const code$2 = alignments[columnIndex];
				if (code$2 === 114) before = " ".repeat(size);
				else if (code$2 === 99) if (size % 2) {
					before = " ".repeat(size / 2 + .5);
					after = " ".repeat(size / 2 - .5);
				} else {
					before = " ".repeat(size / 2);
					after = before;
				}
				else after = " ".repeat(size);
			}
			if (settings.delimiterStart !== false && !columnIndex) line.push("|");
			if (settings.padding !== false && !(settings.alignDelimiters === false && cell === "") && (settings.delimiterStart !== false || columnIndex)) line.push(" ");
			if (settings.alignDelimiters !== false) line.push(before);
			line.push(cell);
			if (settings.alignDelimiters !== false) line.push(after);
			if (settings.padding !== false) line.push(" ");
			if (settings.delimiterEnd !== false || columnIndex !== mostCellsPerRow - 1) line.push("|");
		}
		lines.push(settings.delimiterEnd === false ? line.join("").replace(/ +$/, "") : line.join(""));
	}
	return lines.join("\n");
}
function serialize(value) {
	return value === null || value === void 0 ? "" : String(value);
}
function toAlignment(value) {
	const code$2 = typeof value === "string" ? value.codePointAt(0) : 0;
	return code$2 === 67 || code$2 === 99 ? 99 : code$2 === 76 || code$2 === 108 ? 108 : code$2 === 82 || code$2 === 114 ? 114 : 0;
}
function blockquote(node$1, _, state, info) {
	const exit$2 = state.enter("blockquote");
	const tracker = state.createTracker(info);
	tracker.move("> ");
	tracker.shift(2);
	const value = state.indentLines(state.containerFlow(node$1, tracker.current()), map$1);
	exit$2();
	return value;
}
function map$1(line, _, blank) {
	return ">" + (blank ? "" : " ") + line;
}
function patternInScope(stack, pattern) {
	return listInScope(stack, pattern.inConstruct, true) && !listInScope(stack, pattern.notInConstruct, false);
}
function listInScope(stack, list$2, none) {
	if (typeof list$2 === "string") list$2 = [list$2];
	if (!list$2 || list$2.length === 0) return none;
	let index$1 = -1;
	while (++index$1 < list$2.length) if (stack.includes(list$2[index$1])) return true;
	return false;
}
function hardBreak(_, _1, state, info) {
	let index$1 = -1;
	while (++index$1 < state.unsafe.length) if (state.unsafe[index$1].character === "\n" && patternInScope(state.stack, state.unsafe[index$1])) return /[ \t]/.test(info.before) ? "" : " ";
	return "\\\n";
}
function longestStreak(value, substring) {
	const source = String(value);
	let index$1 = source.indexOf(substring);
	let expected = index$1;
	let count = 0;
	let max = 0;
	if (typeof substring !== "string") throw new TypeError("Expected substring");
	while (index$1 !== -1) {
		if (index$1 === expected) {
			if (++count > max) max = count;
		} else count = 1;
		expected = index$1 + substring.length;
		index$1 = source.indexOf(substring, expected);
	}
	return max;
}
function formatCodeAsIndented(node$1, state) {
	return Boolean(state.options.fences === false && node$1.value && !node$1.lang && /[^ \r\n]/.test(node$1.value) && !/^[\t ]*(?:[\r\n]|$)|(?:^|[\r\n])[\t ]*$/.test(node$1.value));
}
function checkFence(state) {
	const marker = state.options.fence || "`";
	if (marker !== "`" && marker !== "~") throw new Error("Cannot serialize code with `" + marker + "` for `options.fence`, expected `` ` `` or `~`");
	return marker;
}
function code$1(node$1, _, state, info) {
	const marker = checkFence(state);
	const raw = node$1.value || "";
	const suffix = marker === "`" ? "GraveAccent" : "Tilde";
	if (formatCodeAsIndented(node$1, state)) {
		const exit$3 = state.enter("codeIndented");
		const value$1 = state.indentLines(raw, map);
		exit$3();
		return value$1;
	}
	const tracker = state.createTracker(info);
	const sequence = marker.repeat(Math.max(longestStreak(raw, marker) + 1, 3));
	const exit$2 = state.enter("codeFenced");
	let value = tracker.move(sequence);
	if (node$1.lang) {
		const subexit = state.enter(`codeFencedLang${suffix}`);
		value += tracker.move(state.safe(node$1.lang, {
			before: value,
			after: " ",
			encode: ["`"],
			...tracker.current()
		}));
		subexit();
	}
	if (node$1.lang && node$1.meta) {
		const subexit = state.enter(`codeFencedMeta${suffix}`);
		value += tracker.move(" ");
		value += tracker.move(state.safe(node$1.meta, {
			before: value,
			after: "\n",
			encode: ["`"],
			...tracker.current()
		}));
		subexit();
	}
	value += tracker.move("\n");
	if (raw) value += tracker.move(raw + "\n");
	value += tracker.move(sequence);
	exit$2();
	return value;
}
function map(line, _, blank) {
	return (blank ? "" : "    ") + line;
}
function checkQuote(state) {
	const marker = state.options.quote || "\"";
	if (marker !== "\"" && marker !== "'") throw new Error("Cannot serialize title with `" + marker + "` for `options.quote`, expected `\"`, or `'`");
	return marker;
}
function definition(node$1, _, state, info) {
	const quote = checkQuote(state);
	const suffix = quote === "\"" ? "Quote" : "Apostrophe";
	const exit$2 = state.enter("definition");
	let subexit = state.enter("label");
	const tracker = state.createTracker(info);
	let value = tracker.move("[");
	value += tracker.move(state.safe(state.associationId(node$1), {
		before: value,
		after: "]",
		...tracker.current()
	}));
	value += tracker.move("]: ");
	subexit();
	if (!node$1.url || /[\0- \u007F]/.test(node$1.url)) {
		subexit = state.enter("destinationLiteral");
		value += tracker.move("<");
		value += tracker.move(state.safe(node$1.url, {
			before: value,
			after: ">",
			...tracker.current()
		}));
		value += tracker.move(">");
	} else {
		subexit = state.enter("destinationRaw");
		value += tracker.move(state.safe(node$1.url, {
			before: value,
			after: node$1.title ? " " : "\n",
			...tracker.current()
		}));
	}
	subexit();
	if (node$1.title) {
		subexit = state.enter(`title${suffix}`);
		value += tracker.move(" " + quote);
		value += tracker.move(state.safe(node$1.title, {
			before: value,
			after: quote,
			...tracker.current()
		}));
		value += tracker.move(quote);
		subexit();
	}
	exit$2();
	return value;
}
function checkEmphasis(state) {
	const marker = state.options.emphasis || "*";
	if (marker !== "*" && marker !== "_") throw new Error("Cannot serialize emphasis with `" + marker + "` for `options.emphasis`, expected `*`, or `_`");
	return marker;
}
function encodeCharacterReference(code$2) {
	return "&#x" + code$2.toString(16).toUpperCase() + ";";
}
function encodeInfo(outside, inside, marker) {
	const outsideKind = classifyCharacter(outside);
	const insideKind = classifyCharacter(inside);
	if (outsideKind === void 0) return insideKind === void 0 ? marker === "_" ? {
		inside: true,
		outside: true
	} : {
		inside: false,
		outside: false
	} : insideKind === 1 ? {
		inside: true,
		outside: true
	} : {
		inside: false,
		outside: true
	};
	if (outsideKind === 1) return insideKind === void 0 ? {
		inside: false,
		outside: false
	} : insideKind === 1 ? {
		inside: true,
		outside: true
	} : {
		inside: false,
		outside: false
	};
	return insideKind === void 0 ? {
		inside: false,
		outside: false
	} : insideKind === 1 ? {
		inside: true,
		outside: false
	} : {
		inside: false,
		outside: false
	};
}
emphasis.peek = emphasisPeek;
function emphasis(node$1, _, state, info) {
	const marker = checkEmphasis(state);
	const exit$2 = state.enter("emphasis");
	const tracker = state.createTracker(info);
	const before = tracker.move(marker);
	let between = tracker.move(state.containerPhrasing(node$1, {
		after: marker,
		before,
		...tracker.current()
	}));
	const betweenHead = between.charCodeAt(0);
	const open = encodeInfo(info.before.charCodeAt(info.before.length - 1), betweenHead, marker);
	if (open.inside) between = encodeCharacterReference(betweenHead) + between.slice(1);
	const betweenTail = between.charCodeAt(between.length - 1);
	const close = encodeInfo(info.after.charCodeAt(0), betweenTail, marker);
	if (close.inside) between = between.slice(0, -1) + encodeCharacterReference(betweenTail);
	const after = tracker.move(marker);
	exit$2();
	state.attentionEncodeSurroundingInfo = {
		after: close.outside,
		before: open.outside
	};
	return before + between + after;
}
function emphasisPeek(_, _1, state) {
	return state.options.emphasis || "*";
}
function formatHeadingAsSetext(node$1, state) {
	let literalWithBreak = false;
	visit(node$1, function(node$2) {
		if ("value" in node$2 && /\r?\n|\r/.test(node$2.value) || node$2.type === "break") {
			literalWithBreak = true;
			return false;
		}
	});
	return Boolean((!node$1.depth || node$1.depth < 3) && toString(node$1) && (state.options.setext || literalWithBreak));
}
function heading(node$1, _, state, info) {
	const rank = Math.max(Math.min(6, node$1.depth || 1), 1);
	const tracker = state.createTracker(info);
	if (formatHeadingAsSetext(node$1, state)) {
		const exit$3 = state.enter("headingSetext");
		const subexit$1 = state.enter("phrasing");
		const value$1 = state.containerPhrasing(node$1, {
			...tracker.current(),
			before: "\n",
			after: "\n"
		});
		subexit$1();
		exit$3();
		return value$1 + "\n" + (rank === 1 ? "=" : "-").repeat(value$1.length - (Math.max(value$1.lastIndexOf("\r"), value$1.lastIndexOf("\n")) + 1));
	}
	const sequence = "#".repeat(rank);
	const exit$2 = state.enter("headingAtx");
	const subexit = state.enter("phrasing");
	tracker.move(sequence + " ");
	let value = state.containerPhrasing(node$1, {
		before: "# ",
		after: "\n",
		...tracker.current()
	});
	if (/^[\t ]/.test(value)) value = encodeCharacterReference(value.charCodeAt(0)) + value.slice(1);
	value = value ? sequence + " " + value : sequence;
	if (state.options.closeAtx) value += " " + sequence;
	subexit();
	exit$2();
	return value;
}
html.peek = htmlPeek;
function html(node$1) {
	return node$1.value || "";
}
function htmlPeek() {
	return "<";
}
image.peek = imagePeek;
function image(node$1, _, state, info) {
	const quote = checkQuote(state);
	const suffix = quote === "\"" ? "Quote" : "Apostrophe";
	const exit$2 = state.enter("image");
	let subexit = state.enter("label");
	const tracker = state.createTracker(info);
	let value = tracker.move("![");
	value += tracker.move(state.safe(node$1.alt, {
		before: value,
		after: "]",
		...tracker.current()
	}));
	value += tracker.move("](");
	subexit();
	if (!node$1.url && node$1.title || /[\0- \u007F]/.test(node$1.url)) {
		subexit = state.enter("destinationLiteral");
		value += tracker.move("<");
		value += tracker.move(state.safe(node$1.url, {
			before: value,
			after: ">",
			...tracker.current()
		}));
		value += tracker.move(">");
	} else {
		subexit = state.enter("destinationRaw");
		value += tracker.move(state.safe(node$1.url, {
			before: value,
			after: node$1.title ? " " : ")",
			...tracker.current()
		}));
	}
	subexit();
	if (node$1.title) {
		subexit = state.enter(`title${suffix}`);
		value += tracker.move(" " + quote);
		value += tracker.move(state.safe(node$1.title, {
			before: value,
			after: quote,
			...tracker.current()
		}));
		value += tracker.move(quote);
		subexit();
	}
	value += tracker.move(")");
	exit$2();
	return value;
}
function imagePeek() {
	return "!";
}
imageReference.peek = imageReferencePeek;
function imageReference(node$1, _, state, info) {
	const type = node$1.referenceType;
	const exit$2 = state.enter("imageReference");
	let subexit = state.enter("label");
	const tracker = state.createTracker(info);
	let value = tracker.move("![");
	const alt = state.safe(node$1.alt, {
		before: value,
		after: "]",
		...tracker.current()
	});
	value += tracker.move(alt + "][");
	subexit();
	const stack = state.stack;
	state.stack = [];
	subexit = state.enter("reference");
	const reference = state.safe(state.associationId(node$1), {
		before: value,
		after: "]",
		...tracker.current()
	});
	subexit();
	state.stack = stack;
	exit$2();
	if (type === "full" || !alt || alt !== reference) value += tracker.move(reference + "]");
	else if (type === "shortcut") value = value.slice(0, -1);
	else value += tracker.move("]");
	return value;
}
function imageReferencePeek() {
	return "!";
}
inlineCode.peek = inlineCodePeek;
function inlineCode(node$1, _, state) {
	let value = node$1.value || "";
	let sequence = "`";
	let index$1 = -1;
	while ((/* @__PURE__ */ new RegExp("(^|[^`])" + sequence + "([^`]|$)")).test(value)) sequence += "`";
	if (/[^ \r\n]/.test(value) && (/^[ \r\n]/.test(value) && /[ \r\n]$/.test(value) || /^`|`$/.test(value))) value = " " + value + " ";
	while (++index$1 < state.unsafe.length) {
		const pattern = state.unsafe[index$1];
		const expression = state.compilePattern(pattern);
		let match;
		if (!pattern.atBreak) continue;
		while (match = expression.exec(value)) {
			let position$1 = match.index;
			if (value.charCodeAt(position$1) === 10 && value.charCodeAt(position$1 - 1) === 13) position$1--;
			value = value.slice(0, position$1) + " " + value.slice(match.index + 1);
		}
	}
	return sequence + value + sequence;
}
function inlineCodePeek() {
	return "`";
}
function formatLinkAsAutolink(node$1, state) {
	const raw = toString(node$1);
	return Boolean(!state.options.resourceLink && node$1.url && !node$1.title && node$1.children && node$1.children.length === 1 && node$1.children[0].type === "text" && (raw === node$1.url || "mailto:" + raw === node$1.url) && /^[a-z][a-z+.-]+:/i.test(node$1.url) && !/[\0- <>\u007F]/.test(node$1.url));
}
link.peek = linkPeek;
function link(node$1, _, state, info) {
	const quote = checkQuote(state);
	const suffix = quote === "\"" ? "Quote" : "Apostrophe";
	const tracker = state.createTracker(info);
	let exit$2;
	let subexit;
	if (formatLinkAsAutolink(node$1, state)) {
		const stack = state.stack;
		state.stack = [];
		exit$2 = state.enter("autolink");
		let value$1 = tracker.move("<");
		value$1 += tracker.move(state.containerPhrasing(node$1, {
			before: value$1,
			after: ">",
			...tracker.current()
		}));
		value$1 += tracker.move(">");
		exit$2();
		state.stack = stack;
		return value$1;
	}
	exit$2 = state.enter("link");
	subexit = state.enter("label");
	let value = tracker.move("[");
	value += tracker.move(state.containerPhrasing(node$1, {
		before: value,
		after: "](",
		...tracker.current()
	}));
	value += tracker.move("](");
	subexit();
	if (!node$1.url && node$1.title || /[\0- \u007F]/.test(node$1.url)) {
		subexit = state.enter("destinationLiteral");
		value += tracker.move("<");
		value += tracker.move(state.safe(node$1.url, {
			before: value,
			after: ">",
			...tracker.current()
		}));
		value += tracker.move(">");
	} else {
		subexit = state.enter("destinationRaw");
		value += tracker.move(state.safe(node$1.url, {
			before: value,
			after: node$1.title ? " " : ")",
			...tracker.current()
		}));
	}
	subexit();
	if (node$1.title) {
		subexit = state.enter(`title${suffix}`);
		value += tracker.move(" " + quote);
		value += tracker.move(state.safe(node$1.title, {
			before: value,
			after: quote,
			...tracker.current()
		}));
		value += tracker.move(quote);
		subexit();
	}
	value += tracker.move(")");
	exit$2();
	return value;
}
function linkPeek(node$1, _, state) {
	return formatLinkAsAutolink(node$1, state) ? "<" : "[";
}
linkReference.peek = linkReferencePeek;
function linkReference(node$1, _, state, info) {
	const type = node$1.referenceType;
	const exit$2 = state.enter("linkReference");
	let subexit = state.enter("label");
	const tracker = state.createTracker(info);
	let value = tracker.move("[");
	const text$4 = state.containerPhrasing(node$1, {
		before: value,
		after: "]",
		...tracker.current()
	});
	value += tracker.move(text$4 + "][");
	subexit();
	const stack = state.stack;
	state.stack = [];
	subexit = state.enter("reference");
	const reference = state.safe(state.associationId(node$1), {
		before: value,
		after: "]",
		...tracker.current()
	});
	subexit();
	state.stack = stack;
	exit$2();
	if (type === "full" || !text$4 || text$4 !== reference) value += tracker.move(reference + "]");
	else if (type === "shortcut") value = value.slice(0, -1);
	else value += tracker.move("]");
	return value;
}
function linkReferencePeek() {
	return "[";
}
function checkBullet(state) {
	const marker = state.options.bullet || "*";
	if (marker !== "*" && marker !== "+" && marker !== "-") throw new Error("Cannot serialize items with `" + marker + "` for `options.bullet`, expected `*`, `+`, or `-`");
	return marker;
}
function checkBulletOther(state) {
	const bullet = checkBullet(state);
	const bulletOther = state.options.bulletOther;
	if (!bulletOther) return bullet === "*" ? "-" : "*";
	if (bulletOther !== "*" && bulletOther !== "+" && bulletOther !== "-") throw new Error("Cannot serialize items with `" + bulletOther + "` for `options.bulletOther`, expected `*`, `+`, or `-`");
	if (bulletOther === bullet) throw new Error("Expected `bullet` (`" + bullet + "`) and `bulletOther` (`" + bulletOther + "`) to be different");
	return bulletOther;
}
function checkBulletOrdered(state) {
	const marker = state.options.bulletOrdered || ".";
	if (marker !== "." && marker !== ")") throw new Error("Cannot serialize items with `" + marker + "` for `options.bulletOrdered`, expected `.` or `)`");
	return marker;
}
function checkRule(state) {
	const marker = state.options.rule || "*";
	if (marker !== "*" && marker !== "-" && marker !== "_") throw new Error("Cannot serialize rules with `" + marker + "` for `options.rule`, expected `*`, `-`, or `_`");
	return marker;
}
function list(node$1, parent, state, info) {
	const exit$2 = state.enter("list");
	const bulletCurrent = state.bulletCurrent;
	let bullet = node$1.ordered ? checkBulletOrdered(state) : checkBullet(state);
	const bulletOther = node$1.ordered ? bullet === "." ? ")" : "." : checkBulletOther(state);
	let useDifferentMarker = parent && state.bulletLastUsed ? bullet === state.bulletLastUsed : false;
	if (!node$1.ordered) {
		const firstListItem = node$1.children ? node$1.children[0] : void 0;
		if ((bullet === "*" || bullet === "-") && firstListItem && (!firstListItem.children || !firstListItem.children[0]) && state.stack[state.stack.length - 1] === "list" && state.stack[state.stack.length - 2] === "listItem" && state.stack[state.stack.length - 3] === "list" && state.stack[state.stack.length - 4] === "listItem" && state.indexStack[state.indexStack.length - 1] === 0 && state.indexStack[state.indexStack.length - 2] === 0 && state.indexStack[state.indexStack.length - 3] === 0) useDifferentMarker = true;
		if (checkRule(state) === bullet && firstListItem) {
			let index$1 = -1;
			while (++index$1 < node$1.children.length) {
				const item = node$1.children[index$1];
				if (item && item.type === "listItem" && item.children && item.children[0] && item.children[0].type === "thematicBreak") {
					useDifferentMarker = true;
					break;
				}
			}
		}
	}
	if (useDifferentMarker) bullet = bulletOther;
	state.bulletCurrent = bullet;
	const value = state.containerFlow(node$1, info);
	state.bulletLastUsed = bullet;
	state.bulletCurrent = bulletCurrent;
	exit$2();
	return value;
}
function checkListItemIndent(state) {
	const style = state.options.listItemIndent || "one";
	if (style !== "tab" && style !== "one" && style !== "mixed") throw new Error("Cannot serialize items with `" + style + "` for `options.listItemIndent`, expected `tab`, `one`, or `mixed`");
	return style;
}
function listItem(node$1, parent, state, info) {
	const listItemIndent = checkListItemIndent(state);
	let bullet = state.bulletCurrent || checkBullet(state);
	if (parent && parent.type === "list" && parent.ordered) bullet = (typeof parent.start === "number" && parent.start > -1 ? parent.start : 1) + (state.options.incrementListMarker === false ? 0 : parent.children.indexOf(node$1)) + bullet;
	let size = bullet.length + 1;
	if (listItemIndent === "tab" || listItemIndent === "mixed" && (parent && parent.type === "list" && parent.spread || node$1.spread)) size = Math.ceil(size / 4) * 4;
	const tracker = state.createTracker(info);
	tracker.move(bullet + " ".repeat(size - bullet.length));
	tracker.shift(size);
	const exit$2 = state.enter("listItem");
	const value = state.indentLines(state.containerFlow(node$1, tracker.current()), map$2);
	exit$2();
	return value;
	function map$2(line, index$1, blank) {
		if (index$1) return (blank ? "" : " ".repeat(size)) + line;
		return (blank ? bullet : bullet + " ".repeat(size - bullet.length)) + line;
	}
}
function paragraph(node$1, _, state, info) {
	const exit$2 = state.enter("paragraph");
	const subexit = state.enter("phrasing");
	const value = state.containerPhrasing(node$1, info);
	subexit();
	exit$2();
	return value;
}
const phrasing = convert([
	"break",
	"delete",
	"emphasis",
	"footnote",
	"footnoteReference",
	"image",
	"imageReference",
	"inlineCode",
	"inlineMath",
	"link",
	"linkReference",
	"mdxJsxTextElement",
	"mdxTextExpression",
	"strong",
	"text",
	"textDirective"
]);
function root(node$1, _, state, info) {
	return (node$1.children.some(function(d) {
		return phrasing(d);
	}) ? state.containerPhrasing : state.containerFlow).call(state, node$1, info);
}
function checkStrong(state) {
	const marker = state.options.strong || "*";
	if (marker !== "*" && marker !== "_") throw new Error("Cannot serialize strong with `" + marker + "` for `options.strong`, expected `*`, or `_`");
	return marker;
}
strong.peek = strongPeek;
function strong(node$1, _, state, info) {
	const marker = checkStrong(state);
	const exit$2 = state.enter("strong");
	const tracker = state.createTracker(info);
	const before = tracker.move(marker + marker);
	let between = tracker.move(state.containerPhrasing(node$1, {
		after: marker,
		before,
		...tracker.current()
	}));
	const betweenHead = between.charCodeAt(0);
	const open = encodeInfo(info.before.charCodeAt(info.before.length - 1), betweenHead, marker);
	if (open.inside) between = encodeCharacterReference(betweenHead) + between.slice(1);
	const betweenTail = between.charCodeAt(between.length - 1);
	const close = encodeInfo(info.after.charCodeAt(0), betweenTail, marker);
	if (close.inside) between = between.slice(0, -1) + encodeCharacterReference(betweenTail);
	const after = tracker.move(marker + marker);
	exit$2();
	state.attentionEncodeSurroundingInfo = {
		after: close.outside,
		before: open.outside
	};
	return before + between + after;
}
function strongPeek(_, _1, state) {
	return state.options.strong || "*";
}
function text$1(node$1, _, state, info) {
	return state.safe(node$1.value, info);
}
function checkRuleRepetition(state) {
	const repetition = state.options.ruleRepetition || 3;
	if (repetition < 3) throw new Error("Cannot serialize rules with repetition `" + repetition + "` for `options.ruleRepetition`, expected `3` or more");
	return repetition;
}
function thematicBreak(_, _1, state) {
	const value = (checkRule(state) + (state.options.ruleSpaces ? " " : "")).repeat(checkRuleRepetition(state));
	return state.options.ruleSpaces ? value.slice(0, -1) : value;
}
const handle = {
	blockquote,
	break: hardBreak,
	code: code$1,
	definition,
	emphasis,
	hardBreak,
	heading,
	html,
	image,
	imageReference,
	inlineCode,
	link,
	linkReference,
	list,
	listItem,
	paragraph,
	root,
	strong,
	text: text$1,
	thematicBreak
};
function gfmTableFromMarkdown() {
	return {
		enter: {
			table: enterTable,
			tableData: enterCell,
			tableHeader: enterCell,
			tableRow: enterRow
		},
		exit: {
			codeText: exitCodeText,
			table: exitTable,
			tableData: exit,
			tableHeader: exit,
			tableRow: exit
		}
	};
}
function enterTable(token) {
	const align = token._align;
	this.enter({
		type: "table",
		align: align.map(function(d) {
			return d === "none" ? null : d;
		}),
		children: []
	}, token);
	this.data.inTable = true;
}
function exitTable(token) {
	this.exit(token);
	this.data.inTable = void 0;
}
function enterRow(token) {
	this.enter({
		type: "tableRow",
		children: []
	}, token);
}
function exit(token) {
	this.exit(token);
}
function enterCell(token) {
	this.enter({
		type: "tableCell",
		children: []
	}, token);
}
function exitCodeText(token) {
	let value = this.resume();
	if (this.data.inTable) value = value.replace(/\\([\\|])/g, replace);
	const node$1 = this.stack[this.stack.length - 1];
	node$1.type;
	node$1.value = value;
	this.exit(token);
}
function replace($0, $1) {
	return $1 === "|" ? $1 : $0;
}
function gfmTableToMarkdown(options) {
	const settings = options || {};
	const padding = settings.tableCellPadding;
	const alignDelimiters = settings.tablePipeAlign;
	const stringLength = settings.stringLength;
	const around = padding ? " " : "|";
	return {
		unsafe: [
			{
				character: "\r",
				inConstruct: "tableCell"
			},
			{
				character: "\n",
				inConstruct: "tableCell"
			},
			{
				atBreak: true,
				character: "|",
				after: "[	 :-]"
			},
			{
				character: "|",
				inConstruct: "tableCell"
			},
			{
				atBreak: true,
				character: ":",
				after: "-"
			},
			{
				atBreak: true,
				character: "-",
				after: "[:|-]"
			}
		],
		handlers: {
			inlineCode: inlineCodeWithTable,
			table: handleTable,
			tableCell: handleTableCell,
			tableRow: handleTableRow
		}
	};
	function handleTable(node$1, _, state, info) {
		return serializeData(handleTableAsData(node$1, state, info), node$1.align);
	}
	function handleTableRow(node$1, _, state, info) {
		const value = serializeData([handleTableRowAsData(node$1, state, info)]);
		return value.slice(0, value.indexOf("\n"));
	}
	function handleTableCell(node$1, _, state, info) {
		const exit$2 = state.enter("tableCell");
		const subexit = state.enter("phrasing");
		const value = state.containerPhrasing(node$1, {
			...info,
			before: around,
			after: around
		});
		subexit();
		exit$2();
		return value;
	}
	function serializeData(matrix, align) {
		return markdownTable(matrix, {
			align,
			alignDelimiters,
			padding,
			stringLength
		});
	}
	function handleTableAsData(node$1, state, info) {
		const children = node$1.children;
		let index$1 = -1;
		const result = [];
		const subexit = state.enter("table");
		while (++index$1 < children.length) result[index$1] = handleTableRowAsData(children[index$1], state, info);
		subexit();
		return result;
	}
	function handleTableRowAsData(node$1, state, info) {
		const children = node$1.children;
		let index$1 = -1;
		const result = [];
		const subexit = state.enter("tableRow");
		while (++index$1 < children.length) result[index$1] = handleTableCell(children[index$1], node$1, state, info);
		subexit();
		return result;
	}
	function inlineCodeWithTable(node$1, parent, state) {
		let value = handle.inlineCode(node$1, parent, state);
		if (state.stack.includes("tableCell")) value = value.replace(/\|/g, "\\$&");
		return value;
	}
}
function gfmTaskListItemFromMarkdown() {
	return { exit: {
		taskListCheckValueChecked: exitCheck,
		taskListCheckValueUnchecked: exitCheck,
		paragraph: exitParagraphWithTaskListItem
	} };
}
function gfmTaskListItemToMarkdown() {
	return {
		unsafe: [{
			atBreak: true,
			character: "-",
			after: "[:|-]"
		}],
		handlers: { listItem: listItemWithTaskListItem }
	};
}
function exitCheck(token) {
	const node$1 = this.stack[this.stack.length - 2];
	node$1.type;
	node$1.checked = token.type === "taskListCheckValueChecked";
}
function exitParagraphWithTaskListItem(token) {
	const parent = this.stack[this.stack.length - 2];
	if (parent && parent.type === "listItem" && typeof parent.checked === "boolean") {
		const node$1 = this.stack[this.stack.length - 1];
		node$1.type;
		const head = node$1.children[0];
		if (head && head.type === "text") {
			const siblings = parent.children;
			let index$1 = -1;
			let firstParaghraph;
			while (++index$1 < siblings.length) {
				const sibling = siblings[index$1];
				if (sibling.type === "paragraph") {
					firstParaghraph = sibling;
					break;
				}
			}
			if (firstParaghraph === node$1) {
				head.value = head.value.slice(1);
				if (head.value.length === 0) node$1.children.shift();
				else if (node$1.position && head.position && typeof head.position.start.offset === "number") {
					head.position.start.column++;
					head.position.start.offset++;
					node$1.position.start = Object.assign({}, head.position.start);
				}
			}
		}
	}
	this.exit(token);
}
function listItemWithTaskListItem(node$1, parent, state, info) {
	const head = node$1.children[0];
	const checkable = typeof node$1.checked === "boolean" && head && head.type === "paragraph";
	const checkbox = "[" + (node$1.checked ? "x" : " ") + "] ";
	const tracker = state.createTracker(info);
	if (checkable) tracker.move(checkbox);
	let value = handle.listItem(node$1, parent, state, {
		...info,
		...tracker.current()
	});
	if (checkable) value = value.replace(/^(?:[*+-]|\d+\.)([\r\n]| {1,3})/, check);
	return value;
	function check($0) {
		return $0 + checkbox;
	}
}
function gfmFromMarkdown() {
	return [
		gfmAutolinkLiteralFromMarkdown(),
		gfmFootnoteFromMarkdown(),
		gfmStrikethroughFromMarkdown(),
		gfmTableFromMarkdown(),
		gfmTaskListItemFromMarkdown()
	];
}
function gfmToMarkdown(options) {
	return { extensions: [
		gfmAutolinkLiteralToMarkdown(),
		gfmFootnoteToMarkdown(options),
		gfmStrikethroughToMarkdown(),
		gfmTableToMarkdown(options),
		gfmTaskListItemToMarkdown()
	] };
}
var wwwPrefix = {
	tokenize: tokenizeWwwPrefix,
	partial: true
};
var domain = {
	tokenize: tokenizeDomain,
	partial: true
};
var path = {
	tokenize: tokenizePath,
	partial: true
};
var trail = {
	tokenize: tokenizeTrail,
	partial: true
};
var emailDomainDotTrail = {
	tokenize: tokenizeEmailDomainDotTrail,
	partial: true
};
var wwwAutolink = {
	name: "wwwAutolink",
	tokenize: tokenizeWwwAutolink,
	previous: previousWww
};
var protocolAutolink = {
	name: "protocolAutolink",
	tokenize: tokenizeProtocolAutolink,
	previous: previousProtocol
};
var emailAutolink = {
	name: "emailAutolink",
	tokenize: tokenizeEmailAutolink,
	previous: previousEmail
};
var text = {};
function gfmAutolinkLiteral() {
	return { text };
}
var code = 48;
while (code < 123) {
	text[code] = emailAutolink;
	code++;
	if (code === 58) code = 65;
	else if (code === 91) code = 97;
}
text[43] = emailAutolink;
text[45] = emailAutolink;
text[46] = emailAutolink;
text[95] = emailAutolink;
text[72] = [emailAutolink, protocolAutolink];
text[104] = [emailAutolink, protocolAutolink];
text[87] = [emailAutolink, wwwAutolink];
text[119] = [emailAutolink, wwwAutolink];
function tokenizeEmailAutolink(effects, ok$2, nok) {
	const self = this;
	let dot;
	let data;
	return start;
	function start(code$2) {
		if (!gfmAtext(code$2) || !previousEmail.call(self, self.previous) || previousUnbalanced(self.events)) return nok(code$2);
		effects.enter("literalAutolink");
		effects.enter("literalAutolinkEmail");
		return atext(code$2);
	}
	function atext(code$2) {
		if (gfmAtext(code$2)) {
			effects.consume(code$2);
			return atext;
		}
		if (code$2 === 64) {
			effects.consume(code$2);
			return emailDomain;
		}
		return nok(code$2);
	}
	function emailDomain(code$2) {
		if (code$2 === 46) return effects.check(emailDomainDotTrail, emailDomainAfter, emailDomainDot)(code$2);
		if (code$2 === 45 || code$2 === 95 || asciiAlphanumeric(code$2)) {
			data = true;
			effects.consume(code$2);
			return emailDomain;
		}
		return emailDomainAfter(code$2);
	}
	function emailDomainDot(code$2) {
		effects.consume(code$2);
		dot = true;
		return emailDomain;
	}
	function emailDomainAfter(code$2) {
		if (data && dot && asciiAlpha(self.previous)) {
			effects.exit("literalAutolinkEmail");
			effects.exit("literalAutolink");
			return ok$2(code$2);
		}
		return nok(code$2);
	}
}
function tokenizeWwwAutolink(effects, ok$2, nok) {
	const self = this;
	return wwwStart;
	function wwwStart(code$2) {
		if (code$2 !== 87 && code$2 !== 119 || !previousWww.call(self, self.previous) || previousUnbalanced(self.events)) return nok(code$2);
		effects.enter("literalAutolink");
		effects.enter("literalAutolinkWww");
		return effects.check(wwwPrefix, effects.attempt(domain, effects.attempt(path, wwwAfter), nok), nok)(code$2);
	}
	function wwwAfter(code$2) {
		effects.exit("literalAutolinkWww");
		effects.exit("literalAutolink");
		return ok$2(code$2);
	}
}
function tokenizeProtocolAutolink(effects, ok$2, nok) {
	const self = this;
	let buffer = "";
	let seen = false;
	return protocolStart;
	function protocolStart(code$2) {
		if ((code$2 === 72 || code$2 === 104) && previousProtocol.call(self, self.previous) && !previousUnbalanced(self.events)) {
			effects.enter("literalAutolink");
			effects.enter("literalAutolinkHttp");
			buffer += String.fromCodePoint(code$2);
			effects.consume(code$2);
			return protocolPrefixInside;
		}
		return nok(code$2);
	}
	function protocolPrefixInside(code$2) {
		if (asciiAlpha(code$2) && buffer.length < 5) {
			buffer += String.fromCodePoint(code$2);
			effects.consume(code$2);
			return protocolPrefixInside;
		}
		if (code$2 === 58) {
			const protocol = buffer.toLowerCase();
			if (protocol === "http" || protocol === "https") {
				effects.consume(code$2);
				return protocolSlashesInside;
			}
		}
		return nok(code$2);
	}
	function protocolSlashesInside(code$2) {
		if (code$2 === 47) {
			effects.consume(code$2);
			if (seen) return afterProtocol;
			seen = true;
			return protocolSlashesInside;
		}
		return nok(code$2);
	}
	function afterProtocol(code$2) {
		return code$2 === null || asciiControl(code$2) || markdownLineEndingOrSpace(code$2) || unicodeWhitespace(code$2) || unicodePunctuation(code$2) ? nok(code$2) : effects.attempt(domain, effects.attempt(path, protocolAfter), nok)(code$2);
	}
	function protocolAfter(code$2) {
		effects.exit("literalAutolinkHttp");
		effects.exit("literalAutolink");
		return ok$2(code$2);
	}
}
function tokenizeWwwPrefix(effects, ok$2, nok) {
	let size = 0;
	return wwwPrefixInside;
	function wwwPrefixInside(code$2) {
		if ((code$2 === 87 || code$2 === 119) && size < 3) {
			size++;
			effects.consume(code$2);
			return wwwPrefixInside;
		}
		if (code$2 === 46 && size === 3) {
			effects.consume(code$2);
			return wwwPrefixAfter;
		}
		return nok(code$2);
	}
	function wwwPrefixAfter(code$2) {
		return code$2 === null ? nok(code$2) : ok$2(code$2);
	}
}
function tokenizeDomain(effects, ok$2, nok) {
	let underscoreInLastSegment;
	let underscoreInLastLastSegment;
	let seen;
	return domainInside;
	function domainInside(code$2) {
		if (code$2 === 46 || code$2 === 95) return effects.check(trail, domainAfter, domainAtPunctuation)(code$2);
		if (code$2 === null || markdownLineEndingOrSpace(code$2) || unicodeWhitespace(code$2) || code$2 !== 45 && unicodePunctuation(code$2)) return domainAfter(code$2);
		seen = true;
		effects.consume(code$2);
		return domainInside;
	}
	function domainAtPunctuation(code$2) {
		if (code$2 === 95) underscoreInLastSegment = true;
		else {
			underscoreInLastLastSegment = underscoreInLastSegment;
			underscoreInLastSegment = void 0;
		}
		effects.consume(code$2);
		return domainInside;
	}
	function domainAfter(code$2) {
		if (underscoreInLastLastSegment || underscoreInLastSegment || !seen) return nok(code$2);
		return ok$2(code$2);
	}
}
function tokenizePath(effects, ok$2) {
	let sizeOpen = 0;
	let sizeClose = 0;
	return pathInside;
	function pathInside(code$2) {
		if (code$2 === 40) {
			sizeOpen++;
			effects.consume(code$2);
			return pathInside;
		}
		if (code$2 === 41 && sizeClose < sizeOpen) return pathAtPunctuation(code$2);
		if (code$2 === 33 || code$2 === 34 || code$2 === 38 || code$2 === 39 || code$2 === 41 || code$2 === 42 || code$2 === 44 || code$2 === 46 || code$2 === 58 || code$2 === 59 || code$2 === 60 || code$2 === 63 || code$2 === 93 || code$2 === 95 || code$2 === 126) return effects.check(trail, ok$2, pathAtPunctuation)(code$2);
		if (code$2 === null || markdownLineEndingOrSpace(code$2) || unicodeWhitespace(code$2)) return ok$2(code$2);
		effects.consume(code$2);
		return pathInside;
	}
	function pathAtPunctuation(code$2) {
		if (code$2 === 41) sizeClose++;
		effects.consume(code$2);
		return pathInside;
	}
}
function tokenizeTrail(effects, ok$2, nok) {
	return trail$1;
	function trail$1(code$2) {
		if (code$2 === 33 || code$2 === 34 || code$2 === 39 || code$2 === 41 || code$2 === 42 || code$2 === 44 || code$2 === 46 || code$2 === 58 || code$2 === 59 || code$2 === 63 || code$2 === 95 || code$2 === 126) {
			effects.consume(code$2);
			return trail$1;
		}
		if (code$2 === 38) {
			effects.consume(code$2);
			return trailCharacterReferenceStart;
		}
		if (code$2 === 93) {
			effects.consume(code$2);
			return trailBracketAfter;
		}
		if (code$2 === 60 || code$2 === null || markdownLineEndingOrSpace(code$2) || unicodeWhitespace(code$2)) return ok$2(code$2);
		return nok(code$2);
	}
	function trailBracketAfter(code$2) {
		if (code$2 === null || code$2 === 40 || code$2 === 91 || markdownLineEndingOrSpace(code$2) || unicodeWhitespace(code$2)) return ok$2(code$2);
		return trail$1(code$2);
	}
	function trailCharacterReferenceStart(code$2) {
		return asciiAlpha(code$2) ? trailCharacterReferenceInside(code$2) : nok(code$2);
	}
	function trailCharacterReferenceInside(code$2) {
		if (code$2 === 59) {
			effects.consume(code$2);
			return trail$1;
		}
		if (asciiAlpha(code$2)) {
			effects.consume(code$2);
			return trailCharacterReferenceInside;
		}
		return nok(code$2);
	}
}
function tokenizeEmailDomainDotTrail(effects, ok$2, nok) {
	return start;
	function start(code$2) {
		effects.consume(code$2);
		return after;
	}
	function after(code$2) {
		return asciiAlphanumeric(code$2) ? nok(code$2) : ok$2(code$2);
	}
}
function previousWww(code$2) {
	return code$2 === null || code$2 === 40 || code$2 === 42 || code$2 === 95 || code$2 === 91 || code$2 === 93 || code$2 === 126 || markdownLineEndingOrSpace(code$2);
}
function previousProtocol(code$2) {
	return !asciiAlpha(code$2);
}
function previousEmail(code$2) {
	return !(code$2 === 47 || gfmAtext(code$2));
}
function gfmAtext(code$2) {
	return code$2 === 43 || code$2 === 45 || code$2 === 46 || code$2 === 95 || asciiAlphanumeric(code$2);
}
function previousUnbalanced(events) {
	let index$1 = events.length;
	let result = false;
	while (index$1--) {
		const token = events[index$1][1];
		if ((token.type === "labelLink" || token.type === "labelImage") && !token._balanced) {
			result = true;
			break;
		}
		if (token._gfmAutolinkLiteralWalkedInto) {
			result = false;
			break;
		}
	}
	if (events.length > 0 && !result) events[events.length - 1][1]._gfmAutolinkLiteralWalkedInto = true;
	return result;
}
var indent = {
	tokenize: tokenizeIndent,
	partial: true
};
function gfmFootnote() {
	return {
		document: { [91]: {
			name: "gfmFootnoteDefinition",
			tokenize: tokenizeDefinitionStart,
			continuation: { tokenize: tokenizeDefinitionContinuation },
			exit: gfmFootnoteDefinitionEnd
		} },
		text: {
			[91]: {
				name: "gfmFootnoteCall",
				tokenize: tokenizeGfmFootnoteCall
			},
			[93]: {
				name: "gfmPotentialFootnoteCall",
				add: "after",
				tokenize: tokenizePotentialGfmFootnoteCall,
				resolveTo: resolveToPotentialGfmFootnoteCall
			}
		}
	};
}
function tokenizePotentialGfmFootnoteCall(effects, ok$2, nok) {
	const self = this;
	let index$1 = self.events.length;
	const defined = self.parser.gfmFootnotes || (self.parser.gfmFootnotes = []);
	let labelStart;
	while (index$1--) {
		const token = self.events[index$1][1];
		if (token.type === "labelImage") {
			labelStart = token;
			break;
		}
		if (token.type === "gfmFootnoteCall" || token.type === "labelLink" || token.type === "label" || token.type === "image" || token.type === "link") break;
	}
	return start;
	function start(code$2) {
		if (!labelStart || !labelStart._balanced) return nok(code$2);
		const id = normalizeIdentifier(self.sliceSerialize({
			start: labelStart.end,
			end: self.now()
		}));
		if (id.codePointAt(0) !== 94 || !defined.includes(id.slice(1))) return nok(code$2);
		effects.enter("gfmFootnoteCallLabelMarker");
		effects.consume(code$2);
		effects.exit("gfmFootnoteCallLabelMarker");
		return ok$2(code$2);
	}
}
function resolveToPotentialGfmFootnoteCall(events, context) {
	let index$1 = events.length;
	while (index$1--) if (events[index$1][1].type === "labelImage" && events[index$1][0] === "enter") {
		events[index$1][1];
		break;
	}
	events[index$1 + 1][1].type = "data";
	events[index$1 + 3][1].type = "gfmFootnoteCallLabelMarker";
	const call = {
		type: "gfmFootnoteCall",
		start: Object.assign({}, events[index$1 + 3][1].start),
		end: Object.assign({}, events[events.length - 1][1].end)
	};
	const marker = {
		type: "gfmFootnoteCallMarker",
		start: Object.assign({}, events[index$1 + 3][1].end),
		end: Object.assign({}, events[index$1 + 3][1].end)
	};
	marker.end.column++;
	marker.end.offset++;
	marker.end._bufferIndex++;
	const string$2 = {
		type: "gfmFootnoteCallString",
		start: Object.assign({}, marker.end),
		end: Object.assign({}, events[events.length - 1][1].start)
	};
	const chunk = {
		type: "chunkString",
		contentType: "string",
		start: Object.assign({}, string$2.start),
		end: Object.assign({}, string$2.end)
	};
	const replacement = [
		events[index$1 + 1],
		events[index$1 + 2],
		[
			"enter",
			call,
			context
		],
		events[index$1 + 3],
		events[index$1 + 4],
		[
			"enter",
			marker,
			context
		],
		[
			"exit",
			marker,
			context
		],
		[
			"enter",
			string$2,
			context
		],
		[
			"enter",
			chunk,
			context
		],
		[
			"exit",
			chunk,
			context
		],
		[
			"exit",
			string$2,
			context
		],
		events[events.length - 2],
		events[events.length - 1],
		[
			"exit",
			call,
			context
		]
	];
	events.splice(index$1, events.length - index$1 + 1, ...replacement);
	return events;
}
function tokenizeGfmFootnoteCall(effects, ok$2, nok) {
	const self = this;
	const defined = self.parser.gfmFootnotes || (self.parser.gfmFootnotes = []);
	let size = 0;
	let data;
	return start;
	function start(code$2) {
		effects.enter("gfmFootnoteCall");
		effects.enter("gfmFootnoteCallLabelMarker");
		effects.consume(code$2);
		effects.exit("gfmFootnoteCallLabelMarker");
		return callStart;
	}
	function callStart(code$2) {
		if (code$2 !== 94) return nok(code$2);
		effects.enter("gfmFootnoteCallMarker");
		effects.consume(code$2);
		effects.exit("gfmFootnoteCallMarker");
		effects.enter("gfmFootnoteCallString");
		effects.enter("chunkString").contentType = "string";
		return callData;
	}
	function callData(code$2) {
		if (size > 999 || code$2 === 93 && !data || code$2 === null || code$2 === 91 || markdownLineEndingOrSpace(code$2)) return nok(code$2);
		if (code$2 === 93) {
			effects.exit("chunkString");
			const token = effects.exit("gfmFootnoteCallString");
			if (!defined.includes(normalizeIdentifier(self.sliceSerialize(token)))) return nok(code$2);
			effects.enter("gfmFootnoteCallLabelMarker");
			effects.consume(code$2);
			effects.exit("gfmFootnoteCallLabelMarker");
			effects.exit("gfmFootnoteCall");
			return ok$2;
		}
		if (!markdownLineEndingOrSpace(code$2)) data = true;
		size++;
		effects.consume(code$2);
		return code$2 === 92 ? callEscape : callData;
	}
	function callEscape(code$2) {
		if (code$2 === 91 || code$2 === 92 || code$2 === 93) {
			effects.consume(code$2);
			size++;
			return callData;
		}
		return callData(code$2);
	}
}
function tokenizeDefinitionStart(effects, ok$2, nok) {
	const self = this;
	const defined = self.parser.gfmFootnotes || (self.parser.gfmFootnotes = []);
	let identifier;
	let size = 0;
	let data;
	return start;
	function start(code$2) {
		effects.enter("gfmFootnoteDefinition")._container = true;
		effects.enter("gfmFootnoteDefinitionLabel");
		effects.enter("gfmFootnoteDefinitionLabelMarker");
		effects.consume(code$2);
		effects.exit("gfmFootnoteDefinitionLabelMarker");
		return labelAtMarker;
	}
	function labelAtMarker(code$2) {
		if (code$2 === 94) {
			effects.enter("gfmFootnoteDefinitionMarker");
			effects.consume(code$2);
			effects.exit("gfmFootnoteDefinitionMarker");
			effects.enter("gfmFootnoteDefinitionLabelString");
			effects.enter("chunkString").contentType = "string";
			return labelInside;
		}
		return nok(code$2);
	}
	function labelInside(code$2) {
		if (size > 999 || code$2 === 93 && !data || code$2 === null || code$2 === 91 || markdownLineEndingOrSpace(code$2)) return nok(code$2);
		if (code$2 === 93) {
			effects.exit("chunkString");
			const token = effects.exit("gfmFootnoteDefinitionLabelString");
			identifier = normalizeIdentifier(self.sliceSerialize(token));
			effects.enter("gfmFootnoteDefinitionLabelMarker");
			effects.consume(code$2);
			effects.exit("gfmFootnoteDefinitionLabelMarker");
			effects.exit("gfmFootnoteDefinitionLabel");
			return labelAfter;
		}
		if (!markdownLineEndingOrSpace(code$2)) data = true;
		size++;
		effects.consume(code$2);
		return code$2 === 92 ? labelEscape : labelInside;
	}
	function labelEscape(code$2) {
		if (code$2 === 91 || code$2 === 92 || code$2 === 93) {
			effects.consume(code$2);
			size++;
			return labelInside;
		}
		return labelInside(code$2);
	}
	function labelAfter(code$2) {
		if (code$2 === 58) {
			effects.enter("definitionMarker");
			effects.consume(code$2);
			effects.exit("definitionMarker");
			if (!defined.includes(identifier)) defined.push(identifier);
			return factorySpace(effects, whitespaceAfter, "gfmFootnoteDefinitionWhitespace");
		}
		return nok(code$2);
	}
	function whitespaceAfter(code$2) {
		return ok$2(code$2);
	}
}
function tokenizeDefinitionContinuation(effects, ok$2, nok) {
	return effects.check(blankLine, ok$2, effects.attempt(indent, ok$2, nok));
}
function gfmFootnoteDefinitionEnd(effects) {
	effects.exit("gfmFootnoteDefinition");
}
function tokenizeIndent(effects, ok$2, nok) {
	const self = this;
	return factorySpace(effects, afterPrefix, "gfmFootnoteDefinitionIndent", 5);
	function afterPrefix(code$2) {
		const tail = self.events[self.events.length - 1];
		return tail && tail[1].type === "gfmFootnoteDefinitionIndent" && tail[2].sliceSerialize(tail[1], true).length === 4 ? ok$2(code$2) : nok(code$2);
	}
}
function gfmStrikethrough(options) {
	let single = (options || {}).singleTilde;
	const tokenizer = {
		name: "strikethrough",
		tokenize: tokenizeStrikethrough,
		resolveAll: resolveAllStrikethrough
	};
	if (single === null || single === void 0) single = true;
	return {
		text: { [126]: tokenizer },
		insideSpan: { null: [tokenizer] },
		attentionMarkers: { null: [126] }
	};
	function resolveAllStrikethrough(events, context) {
		let index$1 = -1;
		while (++index$1 < events.length) if (events[index$1][0] === "enter" && events[index$1][1].type === "strikethroughSequenceTemporary" && events[index$1][1]._close) {
			let open = index$1;
			while (open--) if (events[open][0] === "exit" && events[open][1].type === "strikethroughSequenceTemporary" && events[open][1]._open && events[index$1][1].end.offset - events[index$1][1].start.offset === events[open][1].end.offset - events[open][1].start.offset) {
				events[index$1][1].type = "strikethroughSequence";
				events[open][1].type = "strikethroughSequence";
				const strikethrough = {
					type: "strikethrough",
					start: Object.assign({}, events[open][1].start),
					end: Object.assign({}, events[index$1][1].end)
				};
				const text$4 = {
					type: "strikethroughText",
					start: Object.assign({}, events[open][1].end),
					end: Object.assign({}, events[index$1][1].start)
				};
				const nextEvents = [
					[
						"enter",
						strikethrough,
						context
					],
					[
						"enter",
						events[open][1],
						context
					],
					[
						"exit",
						events[open][1],
						context
					],
					[
						"enter",
						text$4,
						context
					]
				];
				const insideSpan$1 = context.parser.constructs.insideSpan.null;
				if (insideSpan$1) splice(nextEvents, nextEvents.length, 0, resolveAll(insideSpan$1, events.slice(open + 1, index$1), context));
				splice(nextEvents, nextEvents.length, 0, [
					[
						"exit",
						text$4,
						context
					],
					[
						"enter",
						events[index$1][1],
						context
					],
					[
						"exit",
						events[index$1][1],
						context
					],
					[
						"exit",
						strikethrough,
						context
					]
				]);
				splice(events, open - 1, index$1 - open + 3, nextEvents);
				index$1 = open + nextEvents.length - 2;
				break;
			}
		}
		index$1 = -1;
		while (++index$1 < events.length) if (events[index$1][1].type === "strikethroughSequenceTemporary") events[index$1][1].type = "data";
		return events;
	}
	function tokenizeStrikethrough(effects, ok$2, nok) {
		const previous$2 = this.previous;
		const events = this.events;
		let size = 0;
		return start;
		function start(code$2) {
			if (previous$2 === 126 && events[events.length - 1][1].type !== "characterEscape") return nok(code$2);
			effects.enter("strikethroughSequenceTemporary");
			return more(code$2);
		}
		function more(code$2) {
			const before = classifyCharacter(previous$2);
			if (code$2 === 126) {
				if (size > 1) return nok(code$2);
				effects.consume(code$2);
				size++;
				return more;
			}
			if (size < 2 && !single) return nok(code$2);
			const token = effects.exit("strikethroughSequenceTemporary");
			const after = classifyCharacter(code$2);
			token._open = !after || after === 2 && Boolean(before);
			token._close = !before || before === 2 && Boolean(after);
			return ok$2(code$2);
		}
	}
}
var EditMap = class {
	constructor() {
		this.map = [];
	}
	add(index$1, remove, add) {
		addImplementation(this, index$1, remove, add);
	}
	consume(events) {
		this.map.sort(function(a, b) {
			return a[0] - b[0];
		});
		/* c8 ignore next 3 -- `resolve` is never called without tables, so without edits. */
		if (this.map.length === 0) return;
		let index$1 = this.map.length;
		const vecs = [];
		while (index$1 > 0) {
			index$1 -= 1;
			vecs.push(events.slice(this.map[index$1][0] + this.map[index$1][1]), this.map[index$1][2]);
			events.length = this.map[index$1][0];
		}
		vecs.push(events.slice());
		events.length = 0;
		let slice = vecs.pop();
		while (slice) {
			for (const element$1 of slice) events.push(element$1);
			slice = vecs.pop();
		}
		this.map.length = 0;
	}
};
function addImplementation(editMap, at, remove, add) {
	let index$1 = 0;
	/* c8 ignore next 3 -- `resolve` is never called without tables, so without edits. */
	if (remove === 0 && add.length === 0) return;
	while (index$1 < editMap.map.length) {
		if (editMap.map[index$1][0] === at) {
			editMap.map[index$1][1] += remove;
			editMap.map[index$1][2].push(...add);
			return;
		}
		index$1 += 1;
	}
	editMap.map.push([
		at,
		remove,
		add
	]);
}
function gfmTableAlign(events, index$1) {
	let inDelimiterRow = false;
	const align = [];
	while (index$1 < events.length) {
		const event = events[index$1];
		if (inDelimiterRow) {
			if (event[0] === "enter") {
				if (event[1].type === "tableContent") align.push(events[index$1 + 1][1].type === "tableDelimiterMarker" ? "left" : "none");
			} else if (event[1].type === "tableContent") {
				if (events[index$1 - 1][1].type === "tableDelimiterMarker") {
					const alignIndex = align.length - 1;
					align[alignIndex] = align[alignIndex] === "left" ? "center" : "right";
				}
			} else if (event[1].type === "tableDelimiterRow") break;
		} else if (event[0] === "enter" && event[1].type === "tableDelimiterRow") inDelimiterRow = true;
		index$1 += 1;
	}
	return align;
}
function gfmTable() {
	return { flow: { null: {
		name: "table",
		tokenize: tokenizeTable,
		resolveAll: resolveTable
	} } };
}
function tokenizeTable(effects, ok$2, nok) {
	const self = this;
	let size = 0;
	let sizeB = 0;
	let seen;
	return start;
	function start(code$2) {
		let index$1 = self.events.length - 1;
		while (index$1 > -1) {
			const type = self.events[index$1][1].type;
			if (type === "lineEnding" || type === "linePrefix") index$1--;
			else break;
		}
		const tail = index$1 > -1 ? self.events[index$1][1].type : null;
		const next = tail === "tableHead" || tail === "tableRow" ? bodyRowStart : headRowBefore;
		if (next === bodyRowStart && self.parser.lazy[self.now().line]) return nok(code$2);
		return next(code$2);
	}
	function headRowBefore(code$2) {
		effects.enter("tableHead");
		effects.enter("tableRow");
		return headRowStart(code$2);
	}
	function headRowStart(code$2) {
		if (code$2 === 124) return headRowBreak(code$2);
		seen = true;
		sizeB += 1;
		return headRowBreak(code$2);
	}
	function headRowBreak(code$2) {
		if (code$2 === null) return nok(code$2);
		if (markdownLineEnding(code$2)) {
			if (sizeB > 1) {
				sizeB = 0;
				self.interrupt = true;
				effects.exit("tableRow");
				effects.enter("lineEnding");
				effects.consume(code$2);
				effects.exit("lineEnding");
				return headDelimiterStart;
			}
			return nok(code$2);
		}
		if (markdownSpace(code$2)) return factorySpace(effects, headRowBreak, "whitespace")(code$2);
		sizeB += 1;
		if (seen) {
			seen = false;
			size += 1;
		}
		if (code$2 === 124) {
			effects.enter("tableCellDivider");
			effects.consume(code$2);
			effects.exit("tableCellDivider");
			seen = true;
			return headRowBreak;
		}
		effects.enter("data");
		return headRowData(code$2);
	}
	function headRowData(code$2) {
		if (code$2 === null || code$2 === 124 || markdownLineEndingOrSpace(code$2)) {
			effects.exit("data");
			return headRowBreak(code$2);
		}
		effects.consume(code$2);
		return code$2 === 92 ? headRowEscape : headRowData;
	}
	function headRowEscape(code$2) {
		if (code$2 === 92 || code$2 === 124) {
			effects.consume(code$2);
			return headRowData;
		}
		return headRowData(code$2);
	}
	function headDelimiterStart(code$2) {
		self.interrupt = false;
		if (self.parser.lazy[self.now().line]) return nok(code$2);
		effects.enter("tableDelimiterRow");
		seen = false;
		if (markdownSpace(code$2)) return factorySpace(effects, headDelimiterBefore, "linePrefix", self.parser.constructs.disable.null.includes("codeIndented") ? void 0 : 4)(code$2);
		return headDelimiterBefore(code$2);
	}
	function headDelimiterBefore(code$2) {
		if (code$2 === 45 || code$2 === 58) return headDelimiterValueBefore(code$2);
		if (code$2 === 124) {
			seen = true;
			effects.enter("tableCellDivider");
			effects.consume(code$2);
			effects.exit("tableCellDivider");
			return headDelimiterCellBefore;
		}
		return headDelimiterNok(code$2);
	}
	function headDelimiterCellBefore(code$2) {
		if (markdownSpace(code$2)) return factorySpace(effects, headDelimiterValueBefore, "whitespace")(code$2);
		return headDelimiterValueBefore(code$2);
	}
	function headDelimiterValueBefore(code$2) {
		if (code$2 === 58) {
			sizeB += 1;
			seen = true;
			effects.enter("tableDelimiterMarker");
			effects.consume(code$2);
			effects.exit("tableDelimiterMarker");
			return headDelimiterLeftAlignmentAfter;
		}
		if (code$2 === 45) {
			sizeB += 1;
			return headDelimiterLeftAlignmentAfter(code$2);
		}
		if (code$2 === null || markdownLineEnding(code$2)) return headDelimiterCellAfter(code$2);
		return headDelimiterNok(code$2);
	}
	function headDelimiterLeftAlignmentAfter(code$2) {
		if (code$2 === 45) {
			effects.enter("tableDelimiterFiller");
			return headDelimiterFiller(code$2);
		}
		return headDelimiterNok(code$2);
	}
	function headDelimiterFiller(code$2) {
		if (code$2 === 45) {
			effects.consume(code$2);
			return headDelimiterFiller;
		}
		if (code$2 === 58) {
			seen = true;
			effects.exit("tableDelimiterFiller");
			effects.enter("tableDelimiterMarker");
			effects.consume(code$2);
			effects.exit("tableDelimiterMarker");
			return headDelimiterRightAlignmentAfter;
		}
		effects.exit("tableDelimiterFiller");
		return headDelimiterRightAlignmentAfter(code$2);
	}
	function headDelimiterRightAlignmentAfter(code$2) {
		if (markdownSpace(code$2)) return factorySpace(effects, headDelimiterCellAfter, "whitespace")(code$2);
		return headDelimiterCellAfter(code$2);
	}
	function headDelimiterCellAfter(code$2) {
		if (code$2 === 124) return headDelimiterBefore(code$2);
		if (code$2 === null || markdownLineEnding(code$2)) {
			if (!seen || size !== sizeB) return headDelimiterNok(code$2);
			effects.exit("tableDelimiterRow");
			effects.exit("tableHead");
			return ok$2(code$2);
		}
		return headDelimiterNok(code$2);
	}
	function headDelimiterNok(code$2) {
		return nok(code$2);
	}
	function bodyRowStart(code$2) {
		effects.enter("tableRow");
		return bodyRowBreak(code$2);
	}
	function bodyRowBreak(code$2) {
		if (code$2 === 124) {
			effects.enter("tableCellDivider");
			effects.consume(code$2);
			effects.exit("tableCellDivider");
			return bodyRowBreak;
		}
		if (code$2 === null || markdownLineEnding(code$2)) {
			effects.exit("tableRow");
			return ok$2(code$2);
		}
		if (markdownSpace(code$2)) return factorySpace(effects, bodyRowBreak, "whitespace")(code$2);
		effects.enter("data");
		return bodyRowData(code$2);
	}
	function bodyRowData(code$2) {
		if (code$2 === null || code$2 === 124 || markdownLineEndingOrSpace(code$2)) {
			effects.exit("data");
			return bodyRowBreak(code$2);
		}
		effects.consume(code$2);
		return code$2 === 92 ? bodyRowEscape : bodyRowData;
	}
	function bodyRowEscape(code$2) {
		if (code$2 === 92 || code$2 === 124) {
			effects.consume(code$2);
			return bodyRowData;
		}
		return bodyRowData(code$2);
	}
}
function resolveTable(events, context) {
	let index$1 = -1;
	let inFirstCellAwaitingPipe = true;
	let rowKind = 0;
	let lastCell = [
		0,
		0,
		0,
		0
	];
	let cell = [
		0,
		0,
		0,
		0
	];
	let afterHeadAwaitingFirstBodyRow = false;
	let lastTableEnd = 0;
	let currentTable;
	let currentBody;
	let currentCell;
	const map$2 = new EditMap();
	while (++index$1 < events.length) {
		const event = events[index$1];
		const token = event[1];
		if (event[0] === "enter") {
			if (token.type === "tableHead") {
				afterHeadAwaitingFirstBodyRow = false;
				if (lastTableEnd !== 0) {
					flushTableEnd(map$2, context, lastTableEnd, currentTable, currentBody);
					currentBody = void 0;
					lastTableEnd = 0;
				}
				currentTable = {
					type: "table",
					start: Object.assign({}, token.start),
					end: Object.assign({}, token.end)
				};
				map$2.add(index$1, 0, [[
					"enter",
					currentTable,
					context
				]]);
			} else if (token.type === "tableRow" || token.type === "tableDelimiterRow") {
				inFirstCellAwaitingPipe = true;
				currentCell = void 0;
				lastCell = [
					0,
					0,
					0,
					0
				];
				cell = [
					0,
					index$1 + 1,
					0,
					0
				];
				if (afterHeadAwaitingFirstBodyRow) {
					afterHeadAwaitingFirstBodyRow = false;
					currentBody = {
						type: "tableBody",
						start: Object.assign({}, token.start),
						end: Object.assign({}, token.end)
					};
					map$2.add(index$1, 0, [[
						"enter",
						currentBody,
						context
					]]);
				}
				rowKind = token.type === "tableDelimiterRow" ? 2 : currentBody ? 3 : 1;
			} else if (rowKind && (token.type === "data" || token.type === "tableDelimiterMarker" || token.type === "tableDelimiterFiller")) {
				inFirstCellAwaitingPipe = false;
				if (cell[2] === 0) {
					if (lastCell[1] !== 0) {
						cell[0] = cell[1];
						currentCell = flushCell(map$2, context, lastCell, rowKind, void 0, currentCell);
						lastCell = [
							0,
							0,
							0,
							0
						];
					}
					cell[2] = index$1;
				}
			} else if (token.type === "tableCellDivider") if (inFirstCellAwaitingPipe) inFirstCellAwaitingPipe = false;
			else {
				if (lastCell[1] !== 0) {
					cell[0] = cell[1];
					currentCell = flushCell(map$2, context, lastCell, rowKind, void 0, currentCell);
				}
				lastCell = cell;
				cell = [
					lastCell[1],
					index$1,
					0,
					0
				];
			}
		} else if (token.type === "tableHead") {
			afterHeadAwaitingFirstBodyRow = true;
			lastTableEnd = index$1;
		} else if (token.type === "tableRow" || token.type === "tableDelimiterRow") {
			lastTableEnd = index$1;
			if (lastCell[1] !== 0) {
				cell[0] = cell[1];
				currentCell = flushCell(map$2, context, lastCell, rowKind, index$1, currentCell);
			} else if (cell[1] !== 0) currentCell = flushCell(map$2, context, cell, rowKind, index$1, currentCell);
			rowKind = 0;
		} else if (rowKind && (token.type === "data" || token.type === "tableDelimiterMarker" || token.type === "tableDelimiterFiller")) cell[3] = index$1;
	}
	if (lastTableEnd !== 0) flushTableEnd(map$2, context, lastTableEnd, currentTable, currentBody);
	map$2.consume(context.events);
	index$1 = -1;
	while (++index$1 < context.events.length) {
		const event = context.events[index$1];
		if (event[0] === "enter" && event[1].type === "table") event[1]._align = gfmTableAlign(context.events, index$1);
	}
	return events;
}
function flushCell(map$2, context, range, rowKind, rowEnd, previousCell) {
	const groupName = rowKind === 1 ? "tableHeader" : rowKind === 2 ? "tableDelimiter" : "tableData";
	const valueName = "tableContent";
	if (range[0] !== 0) {
		previousCell.end = Object.assign({}, getPoint(context.events, range[0]));
		map$2.add(range[0], 0, [[
			"exit",
			previousCell,
			context
		]]);
	}
	const now = getPoint(context.events, range[1]);
	previousCell = {
		type: groupName,
		start: Object.assign({}, now),
		end: Object.assign({}, now)
	};
	map$2.add(range[1], 0, [[
		"enter",
		previousCell,
		context
	]]);
	if (range[2] !== 0) {
		const relatedStart = getPoint(context.events, range[2]);
		const relatedEnd = getPoint(context.events, range[3]);
		const valueToken = {
			type: valueName,
			start: Object.assign({}, relatedStart),
			end: Object.assign({}, relatedEnd)
		};
		map$2.add(range[2], 0, [[
			"enter",
			valueToken,
			context
		]]);
		if (rowKind !== 2) {
			const start = context.events[range[2]];
			const end = context.events[range[3]];
			start[1].end = Object.assign({}, end[1].end);
			start[1].type = "chunkText";
			start[1].contentType = "text";
			if (range[3] > range[2] + 1) {
				const a = range[2] + 1;
				const b = range[3] - range[2] - 1;
				map$2.add(a, b, []);
			}
		}
		map$2.add(range[3] + 1, 0, [[
			"exit",
			valueToken,
			context
		]]);
	}
	if (rowEnd !== void 0) {
		previousCell.end = Object.assign({}, getPoint(context.events, rowEnd));
		map$2.add(rowEnd, 0, [[
			"exit",
			previousCell,
			context
		]]);
		previousCell = void 0;
	}
	return previousCell;
}
function flushTableEnd(map$2, context, index$1, table, tableBody) {
	const exits = [];
	const related = getPoint(context.events, index$1);
	if (tableBody) {
		tableBody.end = Object.assign({}, related);
		exits.push([
			"exit",
			tableBody,
			context
		]);
	}
	table.end = Object.assign({}, related);
	exits.push([
		"exit",
		table,
		context
	]);
	map$2.add(index$1 + 1, 0, exits);
}
function getPoint(events, index$1) {
	const event = events[index$1];
	const side = event[0] === "enter" ? "start" : "end";
	return event[1][side];
}
var tasklistCheck = {
	name: "tasklistCheck",
	tokenize: tokenizeTasklistCheck
};
function gfmTaskListItem() {
	return { text: { [91]: tasklistCheck } };
}
function tokenizeTasklistCheck(effects, ok$2, nok) {
	const self = this;
	return open;
	function open(code$2) {
		if (self.previous !== null || !self._gfmTasklistFirstContentOfListItem) return nok(code$2);
		effects.enter("taskListCheck");
		effects.enter("taskListCheckMarker");
		effects.consume(code$2);
		effects.exit("taskListCheckMarker");
		return inside;
	}
	function inside(code$2) {
		if (markdownLineEndingOrSpace(code$2)) {
			effects.enter("taskListCheckValueUnchecked");
			effects.consume(code$2);
			effects.exit("taskListCheckValueUnchecked");
			return close;
		}
		if (code$2 === 88 || code$2 === 120) {
			effects.enter("taskListCheckValueChecked");
			effects.consume(code$2);
			effects.exit("taskListCheckValueChecked");
			return close;
		}
		return nok(code$2);
	}
	function close(code$2) {
		if (code$2 === 93) {
			effects.enter("taskListCheckMarker");
			effects.consume(code$2);
			effects.exit("taskListCheckMarker");
			effects.exit("taskListCheck");
			return after;
		}
		return nok(code$2);
	}
	function after(code$2) {
		if (markdownLineEnding(code$2)) return ok$2(code$2);
		if (markdownSpace(code$2)) return effects.check({ tokenize: spaceThenNonSpace }, ok$2, nok)(code$2);
		return nok(code$2);
	}
}
function spaceThenNonSpace(effects, ok$2, nok) {
	return factorySpace(effects, after, "whitespace");
	function after(code$2) {
		return code$2 === null ? nok(code$2) : ok$2(code$2);
	}
}
function gfm(options) {
	return combineExtensions([
		gfmAutolinkLiteral(),
		gfmFootnote(),
		gfmStrikethrough(options),
		gfmTable(),
		gfmTaskListItem()
	]);
}
var emptyOptions = {};
function remarkGfm(options) {
	const self = this;
	const settings = options || emptyOptions;
	const data = self.data();
	const micromarkExtensions = data.micromarkExtensions || (data.micromarkExtensions = []);
	const fromMarkdownExtensions = data.fromMarkdownExtensions || (data.fromMarkdownExtensions = []);
	const toMarkdownExtensions = data.toMarkdownExtensions || (data.toMarkdownExtensions = []);
	micromarkExtensions.push(gfm(settings));
	fromMarkdownExtensions.push(gfmFromMarkdown());
	toMarkdownExtensions.push(gfmToMarkdown(settings));
}
export { ok as _, unified as a, SKIP as c, remarkParse as d, factorySpace as f, VFileMessage as g, markdownSpace as h, escapeStringRegexp as i, visitParents as l, markdownLineEnding as m, longestStreak as n, VFile as o, asciiAlphanumeric as p, findAndReplace as r, visit as s, remarkGfm as t, convert as u, unreachable as v };
