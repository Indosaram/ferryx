import { o as __toESM, t as __commonJSMin } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as require_shim, t as useTranslation } from "./useTranslation-DX5IRIhk.js";
import { n as init_defineProperty, t as _defineProperty } from "./defineProperty-BAtR-r70.js";
import { t as Check } from "./check-Lb2n4tDb.js";
import { t as Copy } from "./copy-jk2iqVkp.js";
import { $t as resolveMarkdownLinkTarget, Qt as projectMarkdownHrefForClipboard, t as useAppStore } from "./store-CgXrfmaH.js";
import { t as require_react_dom } from "./react-dom-Da8MQai-.js";
import { t as MermaidBlock } from "./MermaidBlock-gW3wAx0A.js";
import { c as katex } from "./katex-BBbt5qcv.js";
import { h as core_default, i as onImageCacheInvalidated, m as grammars, p as createLowlight, r as loadLocalImageSrc } from "./useLocalImageSrc-DrVdAMx8.js";
import { l as resolveMarkdownDocLink, n as formatMarkdownDocLink, o as parseMarkdownDocLink, r as formatMarkdownDocLinkBody, t as createMarkdownDocumentIndex } from "./markdown-doc-links-D1db8u5w.js";
function OrderedMap(content) {
	this.content = content;
}
OrderedMap.prototype = {
	constructor: OrderedMap,
	find: function(key) {
		for (var i$1 = 0; i$1 < this.content.length; i$1 += 2) if (this.content[i$1] === key) return i$1;
		return -1;
	},
	get: function(key) {
		var found$1 = this.find(key);
		return found$1 == -1 ? void 0 : this.content[found$1 + 1];
	},
	update: function(key, value, newKey) {
		var self = newKey && newKey != key ? this.remove(newKey) : this;
		var found$1 = self.find(key), content = self.content.slice();
		if (found$1 == -1) content.push(newKey || key, value);
		else {
			content[found$1 + 1] = value;
			if (newKey) content[found$1] = newKey;
		}
		return new OrderedMap(content);
	},
	remove: function(key) {
		var found$1 = this.find(key);
		if (found$1 == -1) return this;
		var content = this.content.slice();
		content.splice(found$1, 2);
		return new OrderedMap(content);
	},
	addToStart: function(key, value) {
		return new OrderedMap([key, value].concat(this.remove(key).content));
	},
	addToEnd: function(key, value) {
		var content = this.remove(key).content.slice();
		content.push(key, value);
		return new OrderedMap(content);
	},
	addBefore: function(place, key, value) {
		var without = this.remove(key), content = without.content.slice();
		var found$1 = without.find(place);
		content.splice(found$1 == -1 ? content.length : found$1, 0, key, value);
		return new OrderedMap(content);
	},
	forEach: function(f) {
		for (var i$1 = 0; i$1 < this.content.length; i$1 += 2) f(this.content[i$1], this.content[i$1 + 1]);
	},
	prepend: function(map) {
		map = OrderedMap.from(map);
		if (!map.size) return this;
		return new OrderedMap(map.content.concat(this.subtract(map).content));
	},
	append: function(map) {
		map = OrderedMap.from(map);
		if (!map.size) return this;
		return new OrderedMap(this.subtract(map).content.concat(map.content));
	},
	subtract: function(map) {
		var result = this;
		map = OrderedMap.from(map);
		for (var i$1 = 0; i$1 < map.content.length; i$1 += 2) result = result.remove(map.content[i$1]);
		return result;
	},
	toObject: function() {
		var result = {};
		this.forEach(function(key, value) {
			result[key] = value;
		});
		return result;
	},
	get size() {
		return this.content.length >> 1;
	}
};
OrderedMap.from = function(value) {
	if (value instanceof OrderedMap) return value;
	var content = [];
	if (value) for (var prop in value) content.push(prop, value[prop]);
	return new OrderedMap(content);
};
var dist_default$1 = OrderedMap;
function findDiffStart(a, b$1, pos) {
	for (let i$1 = 0;; i$1++) {
		if (i$1 == a.childCount || i$1 == b$1.childCount) return a.childCount == b$1.childCount ? null : pos;
		let childA = a.child(i$1), childB = b$1.child(i$1);
		if (childA == childB) {
			pos += childA.nodeSize;
			continue;
		}
		if (!childA.sameMarkup(childB)) return pos;
		if (childA.isText && childA.text != childB.text) {
			for (let j$1 = 0; childA.text[j$1] == childB.text[j$1]; j$1++) pos++;
			return pos;
		}
		if (childA.content.size || childB.content.size) {
			let inner = findDiffStart(childA.content, childB.content, pos + 1);
			if (inner != null) return inner;
		}
		pos += childA.nodeSize;
	}
}
function findDiffEnd(a, b$1, posA, posB) {
	for (let iA = a.childCount, iB = b$1.childCount;;) {
		if (iA == 0 || iB == 0) return iA == iB ? null : {
			a: posA,
			b: posB
		};
		let childA = a.child(--iA), childB = b$1.child(--iB), size = childA.nodeSize;
		if (childA == childB) {
			posA -= size;
			posB -= size;
			continue;
		}
		if (!childA.sameMarkup(childB)) return {
			a: posA,
			b: posB
		};
		if (childA.isText && childA.text != childB.text) {
			let same = 0, minSize = Math.min(childA.text.length, childB.text.length);
			while (same < minSize && childA.text[childA.text.length - same - 1] == childB.text[childB.text.length - same - 1]) {
				same++;
				posA--;
				posB--;
			}
			return {
				a: posA,
				b: posB
			};
		}
		if (childA.content.size || childB.content.size) {
			let inner = findDiffEnd(childA.content, childB.content, posA - 1, posB - 1);
			if (inner) return inner;
		}
		posA -= size;
		posB -= size;
	}
}
var Fragment = class Fragment {
	constructor(content, size) {
		this.content = content;
		this.size = size || 0;
		if (size == null) for (let i$1 = 0; i$1 < content.length; i$1++) this.size += content[i$1].nodeSize;
	}
	nodesBetween(from, to, f, nodeStart = 0, parent) {
		for (let i$1 = 0, pos = 0; pos < to; i$1++) {
			let child = this.content[i$1], end = pos + child.nodeSize;
			if (end > from && f(child, nodeStart + pos, parent || null, i$1) !== false && child.content.size) {
				let start = pos + 1;
				child.nodesBetween(Math.max(0, from - start), Math.min(child.content.size, to - start), f, nodeStart + start);
			}
			pos = end;
		}
	}
	descendants(f) {
		this.nodesBetween(0, this.size, f);
	}
	textBetween(from, to, blockSeparator, leafText) {
		let text = "", first$1 = true;
		this.nodesBetween(from, to, (node, pos) => {
			let nodeText = node.isText ? node.text.slice(Math.max(from, pos) - pos, to - pos) : !node.isLeaf ? "" : leafText ? typeof leafText === "function" ? leafText(node) : leafText : node.type.spec.leafText ? node.type.spec.leafText(node) : "";
			if (node.isBlock && (node.isLeaf && nodeText || node.isTextblock) && blockSeparator) if (first$1) first$1 = false;
			else text += blockSeparator;
			text += nodeText;
		}, 0);
		return text;
	}
	append(other) {
		if (!other.size) return this;
		if (!this.size) return other;
		let last = this.lastChild, first$1 = other.firstChild, content = this.content.slice(), i$1 = 0;
		if (last.isText && last.sameMarkup(first$1)) {
			content[content.length - 1] = last.withText(last.text + first$1.text);
			i$1 = 1;
		}
		for (; i$1 < other.content.length; i$1++) content.push(other.content[i$1]);
		return new Fragment(content, this.size + other.size);
	}
	cut(from, to = this.size) {
		if (from == 0 && to == this.size) return this;
		let result = [], size = 0;
		if (to > from) for (let i$1 = 0, pos = 0; pos < to; i$1++) {
			let child = this.content[i$1], end = pos + child.nodeSize;
			if (end > from) {
				if (pos < from || end > to) if (child.isText) child = child.cut(Math.max(0, from - pos), Math.min(child.text.length, to - pos));
				else child = child.cut(Math.max(0, from - pos - 1), Math.min(child.content.size, to - pos - 1));
				result.push(child);
				size += child.nodeSize;
			}
			pos = end;
		}
		return new Fragment(result, size);
	}
	cutByIndex(from, to) {
		if (from == to) return Fragment.empty;
		if (from == 0 && to == this.content.length) return this;
		return new Fragment(this.content.slice(from, to));
	}
	replaceChild(index, node) {
		let current = this.content[index];
		if (current == node) return this;
		let copy$1 = this.content.slice();
		let size = this.size + node.nodeSize - current.nodeSize;
		copy$1[index] = node;
		return new Fragment(copy$1, size);
	}
	addToStart(node) {
		return new Fragment([node].concat(this.content), this.size + node.nodeSize);
	}
	addToEnd(node) {
		return new Fragment(this.content.concat(node), this.size + node.nodeSize);
	}
	eq(other) {
		if (this.content.length != other.content.length) return false;
		for (let i$1 = 0; i$1 < this.content.length; i$1++) if (!this.content[i$1].eq(other.content[i$1])) return false;
		return true;
	}
	get firstChild() {
		return this.content.length ? this.content[0] : null;
	}
	get lastChild() {
		return this.content.length ? this.content[this.content.length - 1] : null;
	}
	get childCount() {
		return this.content.length;
	}
	child(index) {
		let found$1 = this.content[index];
		if (!found$1) throw new RangeError("Index " + index + " out of range for " + this);
		return found$1;
	}
	maybeChild(index) {
		return this.content[index] || null;
	}
	forEach(f) {
		for (let i$1 = 0, p = 0; i$1 < this.content.length; i$1++) {
			let child = this.content[i$1];
			f(child, p, i$1);
			p += child.nodeSize;
		}
	}
	findDiffStart(other, pos = 0) {
		return findDiffStart(this, other, pos);
	}
	findDiffEnd(other, pos = this.size, otherPos = other.size) {
		return findDiffEnd(this, other, pos, otherPos);
	}
	findIndex(pos) {
		if (pos == 0) return retIndex(0, pos);
		if (pos == this.size) return retIndex(this.content.length, pos);
		if (pos > this.size || pos < 0) throw new RangeError(`Position ${pos} outside of fragment (${this})`);
		for (let i$1 = 0, curPos = 0;; i$1++) {
			let cur = this.child(i$1), end = curPos + cur.nodeSize;
			if (end >= pos) {
				if (end == pos) return retIndex(i$1 + 1, end);
				return retIndex(i$1, curPos);
			}
			curPos = end;
		}
	}
	toString() {
		return "<" + this.toStringInner() + ">";
	}
	toStringInner() {
		return this.content.join(", ");
	}
	toJSON() {
		return this.content.length ? this.content.map((n) => n.toJSON()) : null;
	}
	static fromJSON(schema, value) {
		if (!value) return Fragment.empty;
		if (!Array.isArray(value)) throw new RangeError("Invalid input for Fragment.fromJSON");
		return new Fragment(value.map(schema.nodeFromJSON));
	}
	static fromArray(array) {
		if (!array.length) return Fragment.empty;
		let joined, size = 0;
		for (let i$1 = 0; i$1 < array.length; i$1++) {
			let node = array[i$1];
			size += node.nodeSize;
			if (i$1 && node.isText && array[i$1 - 1].sameMarkup(node)) {
				if (!joined) joined = array.slice(0, i$1);
				joined[joined.length - 1] = node.withText(joined[joined.length - 1].text + node.text);
			} else if (joined) joined.push(node);
		}
		return new Fragment(joined || array, size);
	}
	static from(nodes) {
		if (!nodes) return Fragment.empty;
		if (nodes instanceof Fragment) return nodes;
		if (Array.isArray(nodes)) return this.fromArray(nodes);
		if (nodes.attrs) return new Fragment([nodes], nodes.nodeSize);
		throw new RangeError("Can not convert " + nodes + " to a Fragment" + (nodes.nodesBetween ? " (looks like multiple versions of prosemirror-model were loaded)" : ""));
	}
};
Fragment.empty = new Fragment([], 0);
var found = {
	index: 0,
	offset: 0
};
function retIndex(index, offset) {
	found.index = index;
	found.offset = offset;
	return found;
}
function compareDeep(a, b$1) {
	if (a === b$1) return true;
	if (!(a && typeof a == "object") || !(b$1 && typeof b$1 == "object")) return false;
	let array = Array.isArray(a);
	if (Array.isArray(b$1) != array) return false;
	if (array) {
		if (a.length != b$1.length) return false;
		for (let i$1 = 0; i$1 < a.length; i$1++) if (!compareDeep(a[i$1], b$1[i$1])) return false;
	} else {
		for (let p in a) if (!(p in b$1) || !compareDeep(a[p], b$1[p])) return false;
		for (let p in b$1) if (!(p in a)) return false;
	}
	return true;
}
var Mark$1 = class Mark$1 {
	constructor(type, attrs) {
		this.type = type;
		this.attrs = attrs;
	}
	addToSet(set) {
		let copy$1, placed = false;
		for (let i$1 = 0; i$1 < set.length; i$1++) {
			let other = set[i$1];
			if (this.eq(other)) return set;
			if (this.type.excludes(other.type)) {
				if (!copy$1) copy$1 = set.slice(0, i$1);
			} else if (other.type.excludes(this.type)) return set;
			else {
				if (!placed && other.type.rank > this.type.rank) {
					if (!copy$1) copy$1 = set.slice(0, i$1);
					copy$1.push(this);
					placed = true;
				}
				if (copy$1) copy$1.push(other);
			}
		}
		if (!copy$1) copy$1 = set.slice();
		if (!placed) copy$1.push(this);
		return copy$1;
	}
	removeFromSet(set) {
		for (let i$1 = 0; i$1 < set.length; i$1++) if (this.eq(set[i$1])) return set.slice(0, i$1).concat(set.slice(i$1 + 1));
		return set;
	}
	isInSet(set) {
		for (let i$1 = 0; i$1 < set.length; i$1++) if (this.eq(set[i$1])) return true;
		return false;
	}
	eq(other) {
		return this == other || this.type == other.type && compareDeep(this.attrs, other.attrs);
	}
	toJSON() {
		let obj = { type: this.type.name };
		for (let _$1 in this.attrs) {
			obj.attrs = this.attrs;
			break;
		}
		return obj;
	}
	static fromJSON(schema, json) {
		if (!json) throw new RangeError("Invalid input for Mark.fromJSON");
		let type = schema.marks[json.type];
		if (!type) throw new RangeError(`There is no mark type ${json.type} in this schema`);
		let mark = type.create(json.attrs);
		type.checkAttrs(mark.attrs);
		return mark;
	}
	static sameSet(a, b$1) {
		if (a == b$1) return true;
		if (a.length != b$1.length) return false;
		for (let i$1 = 0; i$1 < a.length; i$1++) if (!a[i$1].eq(b$1[i$1])) return false;
		return true;
	}
	static setFrom(marks) {
		if (!marks || Array.isArray(marks) && marks.length == 0) return Mark$1.none;
		if (marks instanceof Mark$1) return [marks];
		let copy$1 = marks.slice();
		copy$1.sort((a, b$1) => a.type.rank - b$1.type.rank);
		return copy$1;
	}
};
Mark$1.none = [];
var ReplaceError = class extends Error {};
var Slice = class Slice {
	constructor(content, openStart, openEnd) {
		this.content = content;
		this.openStart = openStart;
		this.openEnd = openEnd;
	}
	get size() {
		return this.content.size - this.openStart - this.openEnd;
	}
	insertAt(pos, fragment) {
		let content = insertInto(this.content, pos + this.openStart, fragment);
		return content && new Slice(content, this.openStart, this.openEnd);
	}
	removeBetween(from, to) {
		return new Slice(removeRange(this.content, from + this.openStart, to + this.openStart), this.openStart, this.openEnd);
	}
	eq(other) {
		return this.content.eq(other.content) && this.openStart == other.openStart && this.openEnd == other.openEnd;
	}
	toString() {
		return this.content + "(" + this.openStart + "," + this.openEnd + ")";
	}
	toJSON() {
		if (!this.content.size) return null;
		let json = { content: this.content.toJSON() };
		if (this.openStart > 0) json.openStart = this.openStart;
		if (this.openEnd > 0) json.openEnd = this.openEnd;
		return json;
	}
	static fromJSON(schema, json) {
		if (!json) return Slice.empty;
		let openStart = json.openStart || 0, openEnd = json.openEnd || 0;
		if (typeof openStart != "number" || typeof openEnd != "number") throw new RangeError("Invalid input for Slice.fromJSON");
		return new Slice(Fragment.fromJSON(schema, json.content), openStart, openEnd);
	}
	static maxOpen(fragment, openIsolating = true) {
		let openStart = 0, openEnd = 0;
		for (let n = fragment.firstChild; n && !n.isLeaf && (openIsolating || !n.type.spec.isolating); n = n.firstChild) openStart++;
		for (let n = fragment.lastChild; n && !n.isLeaf && (openIsolating || !n.type.spec.isolating); n = n.lastChild) openEnd++;
		return new Slice(fragment, openStart, openEnd);
	}
};
Slice.empty = new Slice(Fragment.empty, 0, 0);
function removeRange(content, from, to) {
	let { index, offset } = content.findIndex(from), child = content.maybeChild(index);
	let { index: indexTo, offset: offsetTo } = content.findIndex(to);
	if (offset == from || child.isText) {
		if (offsetTo != to && !content.child(indexTo).isText) throw new RangeError("Removing non-flat range");
		return content.cut(0, from).append(content.cut(to));
	}
	if (index != indexTo) throw new RangeError("Removing non-flat range");
	return content.replaceChild(index, child.copy(removeRange(child.content, from - offset - 1, to - offset - 1)));
}
function insertInto(content, dist, insert, parent) {
	let { index, offset } = content.findIndex(dist), child = content.maybeChild(index);
	if (offset == dist || child.isText) {
		if (parent && !parent.canReplace(index, index, insert)) return null;
		return content.cut(0, dist).append(insert).append(content.cut(dist));
	}
	let inner = insertInto(child.content, dist - offset - 1, insert, child);
	return inner && content.replaceChild(index, child.copy(inner));
}
function replace($from, $to, slice) {
	if (slice.openStart > $from.depth) throw new ReplaceError("Inserted content deeper than insertion position");
	if ($from.depth - slice.openStart != $to.depth - slice.openEnd) throw new ReplaceError("Inconsistent open depths");
	return replaceOuter($from, $to, slice, 0);
}
function replaceOuter($from, $to, slice, depth) {
	let index = $from.index(depth), node = $from.node(depth);
	if (index == $to.index(depth) && depth < $from.depth - slice.openStart) {
		let inner = replaceOuter($from, $to, slice, depth + 1);
		return node.copy(node.content.replaceChild(index, inner));
	} else if (!slice.content.size) return close(node, replaceTwoWay($from, $to, depth));
	else if (!slice.openStart && !slice.openEnd && $from.depth == depth && $to.depth == depth) {
		let parent = $from.parent, content = parent.content;
		return close(parent, content.cut(0, $from.parentOffset).append(slice.content).append(content.cut($to.parentOffset)));
	} else {
		let { start, end } = prepareSliceForReplace(slice, $from);
		return close(node, replaceThreeWay($from, start, end, $to, depth));
	}
}
function checkJoin(main, sub) {
	if (!sub.type.compatibleContent(main.type)) throw new ReplaceError("Cannot join " + sub.type.name + " onto " + main.type.name);
}
function joinable$1($before, $after, depth) {
	let node = $before.node(depth);
	checkJoin(node, $after.node(depth));
	return node;
}
function addNode(child, target) {
	let last = target.length - 1;
	if (last >= 0 && child.isText && child.sameMarkup(target[last])) target[last] = child.withText(target[last].text + child.text);
	else target.push(child);
}
function addRange($start, $end, depth, target) {
	let node = ($end || $start).node(depth);
	let startIndex = 0, endIndex = $end ? $end.index(depth) : node.childCount;
	if ($start) {
		startIndex = $start.index(depth);
		if ($start.depth > depth) startIndex++;
		else if ($start.textOffset) {
			addNode($start.nodeAfter, target);
			startIndex++;
		}
	}
	for (let i$1 = startIndex; i$1 < endIndex; i$1++) addNode(node.child(i$1), target);
	if ($end && $end.depth == depth && $end.textOffset) addNode($end.nodeBefore, target);
}
function close(node, content) {
	node.type.checkContent(content);
	return node.copy(content);
}
function replaceThreeWay($from, $start, $end, $to, depth) {
	let openStart = $from.depth > depth && joinable$1($from, $start, depth + 1);
	let openEnd = $to.depth > depth && joinable$1($end, $to, depth + 1);
	let content = [];
	addRange(null, $from, depth, content);
	if (openStart && openEnd && $start.index(depth) == $end.index(depth)) {
		checkJoin(openStart, openEnd);
		addNode(close(openStart, replaceThreeWay($from, $start, $end, $to, depth + 1)), content);
	} else {
		if (openStart) addNode(close(openStart, replaceTwoWay($from, $start, depth + 1)), content);
		addRange($start, $end, depth, content);
		if (openEnd) addNode(close(openEnd, replaceTwoWay($end, $to, depth + 1)), content);
	}
	addRange($to, null, depth, content);
	return new Fragment(content);
}
function replaceTwoWay($from, $to, depth) {
	let content = [];
	addRange(null, $from, depth, content);
	if ($from.depth > depth) addNode(close(joinable$1($from, $to, depth + 1), replaceTwoWay($from, $to, depth + 1)), content);
	addRange($to, null, depth, content);
	return new Fragment(content);
}
function prepareSliceForReplace(slice, $along) {
	let extra = $along.depth - slice.openStart;
	let node = $along.node(extra).copy(slice.content);
	for (let i$1 = extra - 1; i$1 >= 0; i$1--) node = $along.node(i$1).copy(Fragment.from(node));
	return {
		start: node.resolveNoCache(slice.openStart + extra),
		end: node.resolveNoCache(node.content.size - slice.openEnd - extra)
	};
}
var ResolvedPos = class ResolvedPos {
	constructor(pos, path, parentOffset) {
		this.pos = pos;
		this.path = path;
		this.parentOffset = parentOffset;
		this.depth = path.length / 3 - 1;
	}
	resolveDepth(val) {
		if (val == null) return this.depth;
		if (val < 0) return this.depth + val;
		return val;
	}
	get parent() {
		return this.node(this.depth);
	}
	get doc() {
		return this.node(0);
	}
	node(depth) {
		return this.path[this.resolveDepth(depth) * 3];
	}
	index(depth) {
		return this.path[this.resolveDepth(depth) * 3 + 1];
	}
	indexAfter(depth) {
		depth = this.resolveDepth(depth);
		return this.index(depth) + (depth == this.depth && !this.textOffset ? 0 : 1);
	}
	start(depth) {
		depth = this.resolveDepth(depth);
		return depth == 0 ? 0 : this.path[depth * 3 - 1] + 1;
	}
	end(depth) {
		depth = this.resolveDepth(depth);
		return this.start(depth) + this.node(depth).content.size;
	}
	before(depth) {
		depth = this.resolveDepth(depth);
		if (!depth) throw new RangeError("There is no position before the top-level node");
		return depth == this.depth + 1 ? this.pos : this.path[depth * 3 - 1];
	}
	after(depth) {
		depth = this.resolveDepth(depth);
		if (!depth) throw new RangeError("There is no position after the top-level node");
		return depth == this.depth + 1 ? this.pos : this.path[depth * 3 - 1] + this.path[depth * 3].nodeSize;
	}
	get textOffset() {
		return this.pos - this.path[this.path.length - 1];
	}
	get nodeAfter() {
		let parent = this.parent, index = this.index(this.depth);
		if (index == parent.childCount) return null;
		let dOff = this.pos - this.path[this.path.length - 1], child = parent.child(index);
		return dOff ? parent.child(index).cut(dOff) : child;
	}
	get nodeBefore() {
		let index = this.index(this.depth);
		let dOff = this.pos - this.path[this.path.length - 1];
		if (dOff) return this.parent.child(index).cut(0, dOff);
		return index == 0 ? null : this.parent.child(index - 1);
	}
	posAtIndex(index, depth) {
		depth = this.resolveDepth(depth);
		let node = this.path[depth * 3], pos = depth == 0 ? 0 : this.path[depth * 3 - 1] + 1;
		for (let i$1 = 0; i$1 < index; i$1++) pos += node.child(i$1).nodeSize;
		return pos;
	}
	marks() {
		let parent = this.parent, index = this.index();
		if (parent.content.size == 0) return Mark$1.none;
		if (this.textOffset) return parent.child(index).marks;
		let main = parent.maybeChild(index - 1), other = parent.maybeChild(index);
		if (!main) {
			let tmp = main;
			main = other;
			other = tmp;
		}
		let marks = main.marks;
		for (var i$1 = 0; i$1 < marks.length; i$1++) if (marks[i$1].type.spec.inclusive === false && (!other || !marks[i$1].isInSet(other.marks))) marks = marks[i$1--].removeFromSet(marks);
		return marks;
	}
	marksAcross($end) {
		let after = this.parent.maybeChild(this.index());
		if (!after || !after.isInline) return null;
		let marks = after.marks, next = $end.parent.maybeChild($end.index());
		for (var i$1 = 0; i$1 < marks.length; i$1++) if (marks[i$1].type.spec.inclusive === false && (!next || !marks[i$1].isInSet(next.marks))) marks = marks[i$1--].removeFromSet(marks);
		return marks;
	}
	sharedDepth(pos) {
		for (let depth = this.depth; depth > 0; depth--) if (this.start(depth) <= pos && this.end(depth) >= pos) return depth;
		return 0;
	}
	blockRange(other = this, pred) {
		if (other.pos < this.pos) return other.blockRange(this);
		for (let d = this.depth - (this.parent.inlineContent || this.pos == other.pos ? 1 : 0); d >= 0; d--) if (other.pos <= this.end(d) && (!pred || pred(this.node(d)))) return new NodeRange(this, other, d);
		return null;
	}
	sameParent(other) {
		return this.pos - this.parentOffset == other.pos - other.parentOffset;
	}
	max(other) {
		return other.pos > this.pos ? other : this;
	}
	min(other) {
		return other.pos < this.pos ? other : this;
	}
	toString() {
		let str = "";
		for (let i$1 = 1; i$1 <= this.depth; i$1++) str += (str ? "/" : "") + this.node(i$1).type.name + "_" + this.index(i$1 - 1);
		return str + ":" + this.parentOffset;
	}
	static resolve(doc$2, pos) {
		if (!(pos >= 0 && pos <= doc$2.content.size)) throw new RangeError("Position " + pos + " out of range");
		let path = [];
		let start = 0, parentOffset = pos;
		for (let node = doc$2;;) {
			let { index, offset } = node.content.findIndex(parentOffset);
			let rem = parentOffset - offset;
			path.push(node, index, start + offset);
			if (!rem) break;
			node = node.child(index);
			if (node.isText) break;
			parentOffset = rem - 1;
			start += offset + 1;
		}
		return new ResolvedPos(pos, path, parentOffset);
	}
	static resolveCached(doc$2, pos) {
		let cache = resolveCache.get(doc$2);
		if (cache) for (let i$1 = 0; i$1 < cache.elts.length; i$1++) {
			let elt = cache.elts[i$1];
			if (elt.pos == pos) return elt;
		}
		else resolveCache.set(doc$2, cache = new ResolveCache());
		let result = cache.elts[cache.i] = ResolvedPos.resolve(doc$2, pos);
		cache.i = (cache.i + 1) % resolveCacheSize;
		return result;
	}
};
var ResolveCache = class {
	constructor() {
		this.elts = [];
		this.i = 0;
	}
};
var resolveCacheSize = 12, resolveCache = /* @__PURE__ */ new WeakMap();
var NodeRange = class {
	constructor($from, $to, depth) {
		this.$from = $from;
		this.$to = $to;
		this.depth = depth;
	}
	get start() {
		return this.$from.before(this.depth + 1);
	}
	get end() {
		return this.$to.after(this.depth + 1);
	}
	get parent() {
		return this.$from.node(this.depth);
	}
	get startIndex() {
		return this.$from.index(this.depth);
	}
	get endIndex() {
		return this.$to.indexAfter(this.depth);
	}
};
var emptyAttrs = Object.create(null);
var Node = class Node {
	constructor(type, attrs, content, marks = Mark$1.none) {
		this.type = type;
		this.attrs = attrs;
		this.marks = marks;
		this.content = content || Fragment.empty;
	}
	get children() {
		return this.content.content;
	}
	get nodeSize() {
		return this.isLeaf ? 1 : 2 + this.content.size;
	}
	get childCount() {
		return this.content.childCount;
	}
	child(index) {
		return this.content.child(index);
	}
	maybeChild(index) {
		return this.content.maybeChild(index);
	}
	forEach(f) {
		this.content.forEach(f);
	}
	nodesBetween(from, to, f, startPos = 0) {
		this.content.nodesBetween(from, to, f, startPos, this);
	}
	descendants(f) {
		this.nodesBetween(0, this.content.size, f);
	}
	get textContent() {
		return this.isLeaf && this.type.spec.leafText ? this.type.spec.leafText(this) : this.textBetween(0, this.content.size, "");
	}
	textBetween(from, to, blockSeparator, leafText) {
		return this.content.textBetween(from, to, blockSeparator, leafText);
	}
	get firstChild() {
		return this.content.firstChild;
	}
	get lastChild() {
		return this.content.lastChild;
	}
	eq(other) {
		return this == other || this.sameMarkup(other) && this.content.eq(other.content);
	}
	sameMarkup(other) {
		return this.hasMarkup(other.type, other.attrs, other.marks);
	}
	hasMarkup(type, attrs, marks) {
		return this.type == type && compareDeep(this.attrs, attrs || type.defaultAttrs || emptyAttrs) && Mark$1.sameSet(this.marks, marks || Mark$1.none);
	}
	copy(content = null) {
		if (content == this.content) return this;
		return new Node(this.type, this.attrs, content, this.marks);
	}
	mark(marks) {
		return marks == this.marks ? this : new Node(this.type, this.attrs, this.content, marks);
	}
	cut(from, to = this.content.size) {
		if (from == 0 && to == this.content.size) return this;
		return this.copy(this.content.cut(from, to));
	}
	slice(from, to = this.content.size, includeParents = false) {
		if (from == to) return Slice.empty;
		let $from = this.resolve(from), $to = this.resolve(to);
		let depth = includeParents ? 0 : $from.sharedDepth(to);
		let start = $from.start(depth);
		return new Slice($from.node(depth).content.cut($from.pos - start, $to.pos - start), $from.depth - depth, $to.depth - depth);
	}
	replace(from, to, slice) {
		return replace(this.resolve(from), this.resolve(to), slice);
	}
	nodeAt(pos) {
		for (let node = this;;) {
			let { index, offset } = node.content.findIndex(pos);
			node = node.maybeChild(index);
			if (!node) return null;
			if (offset == pos || node.isText) return node;
			pos -= offset + 1;
		}
	}
	childAfter(pos) {
		let { index, offset } = this.content.findIndex(pos);
		return {
			node: this.content.maybeChild(index),
			index,
			offset
		};
	}
	childBefore(pos) {
		if (pos == 0) return {
			node: null,
			index: 0,
			offset: 0
		};
		let { index, offset } = this.content.findIndex(pos);
		if (offset < pos) return {
			node: this.content.child(index),
			index,
			offset
		};
		let node = this.content.child(index - 1);
		return {
			node,
			index: index - 1,
			offset: offset - node.nodeSize
		};
	}
	resolve(pos) {
		return ResolvedPos.resolveCached(this, pos);
	}
	resolveNoCache(pos) {
		return ResolvedPos.resolve(this, pos);
	}
	rangeHasMark(from, to, type) {
		let found$1 = false;
		if (to > from) this.nodesBetween(from, to, (node) => {
			if (type.isInSet(node.marks)) found$1 = true;
			return !found$1;
		});
		return found$1;
	}
	get isBlock() {
		return this.type.isBlock;
	}
	get isTextblock() {
		return this.type.isTextblock;
	}
	get inlineContent() {
		return this.type.inlineContent;
	}
	get isInline() {
		return this.type.isInline;
	}
	get isText() {
		return this.type.isText;
	}
	get isLeaf() {
		return this.type.isLeaf;
	}
	get isAtom() {
		return this.type.isAtom;
	}
	toString() {
		if (this.type.spec.toDebugString) return this.type.spec.toDebugString(this);
		let name = this.type.name;
		if (this.content.size) name += "(" + this.content.toStringInner() + ")";
		return wrapMarks(this.marks, name);
	}
	contentMatchAt(index) {
		let match = this.type.contentMatch.matchFragment(this.content, 0, index);
		if (!match) throw new Error("Called contentMatchAt on a node with invalid content");
		return match;
	}
	canReplace(from, to, replacement = Fragment.empty, start = 0, end = replacement.childCount) {
		let one = this.contentMatchAt(from).matchFragment(replacement, start, end);
		let two = one && one.matchFragment(this.content, to);
		if (!two || !two.validEnd) return false;
		for (let i$1 = start; i$1 < end; i$1++) if (!this.type.allowsMarks(replacement.child(i$1).marks)) return false;
		return true;
	}
	canReplaceWith(from, to, type, marks) {
		if (marks && !this.type.allowsMarks(marks)) return false;
		let start = this.contentMatchAt(from).matchType(type);
		let end = start && start.matchFragment(this.content, to);
		return end ? end.validEnd : false;
	}
	canAppend(other) {
		if (other.content.size) return this.canReplace(this.childCount, this.childCount, other.content);
		else return this.type.compatibleContent(other.type);
	}
	check() {
		this.type.checkContent(this.content);
		this.type.checkAttrs(this.attrs);
		let copy$1 = Mark$1.none;
		for (let i$1 = 0; i$1 < this.marks.length; i$1++) {
			let mark = this.marks[i$1];
			mark.type.checkAttrs(mark.attrs);
			copy$1 = mark.addToSet(copy$1);
		}
		if (!Mark$1.sameSet(copy$1, this.marks)) throw new RangeError(`Invalid collection of marks for node ${this.type.name}: ${this.marks.map((m$1) => m$1.type.name)}`);
		this.content.forEach((node) => node.check());
	}
	toJSON() {
		let obj = { type: this.type.name };
		for (let _$1 in this.attrs) {
			obj.attrs = this.attrs;
			break;
		}
		if (this.content.size) obj.content = this.content.toJSON();
		if (this.marks.length) obj.marks = this.marks.map((n) => n.toJSON());
		return obj;
	}
	static fromJSON(schema, json) {
		if (!json) throw new RangeError("Invalid input for Node.fromJSON");
		let marks = void 0;
		if (json.marks) {
			if (!Array.isArray(json.marks)) throw new RangeError("Invalid mark data for Node.fromJSON");
			marks = json.marks.map(schema.markFromJSON);
		}
		if (json.type == "text") {
			if (typeof json.text != "string") throw new RangeError("Invalid text node in JSON");
			return schema.text(json.text, marks);
		}
		let content = Fragment.fromJSON(schema, json.content);
		let node = schema.nodeType(json.type).create(json.attrs, content, marks);
		node.type.checkAttrs(node.attrs);
		return node;
	}
};
Node.prototype.text = void 0;
var TextNode = class TextNode extends Node {
	constructor(type, attrs, content, marks) {
		super(type, attrs, null, marks);
		if (!content) throw new RangeError("Empty text nodes are not allowed");
		this.text = content;
	}
	toString() {
		if (this.type.spec.toDebugString) return this.type.spec.toDebugString(this);
		return wrapMarks(this.marks, JSON.stringify(this.text));
	}
	get textContent() {
		return this.text;
	}
	textBetween(from, to) {
		return this.text.slice(from, to);
	}
	get nodeSize() {
		return this.text.length;
	}
	mark(marks) {
		return marks == this.marks ? this : new TextNode(this.type, this.attrs, this.text, marks);
	}
	withText(text) {
		if (text == this.text) return this;
		return new TextNode(this.type, this.attrs, text, this.marks);
	}
	cut(from = 0, to = this.text.length) {
		if (from == 0 && to == this.text.length) return this;
		return this.withText(this.text.slice(from, to));
	}
	eq(other) {
		return this.sameMarkup(other) && this.text == other.text;
	}
	toJSON() {
		let base$1 = super.toJSON();
		base$1.text = this.text;
		return base$1;
	}
};
function wrapMarks(marks, str) {
	for (let i$1 = marks.length - 1; i$1 >= 0; i$1--) str = marks[i$1].type.name + "(" + str + ")";
	return str;
}
var ContentMatch = class ContentMatch {
	constructor(validEnd) {
		this.validEnd = validEnd;
		this.next = [];
		this.wrapCache = [];
	}
	static parse(string, nodeTypes) {
		let stream = new TokenStream(string, nodeTypes);
		if (stream.next == null) return ContentMatch.empty;
		let expr = parseExpr(stream);
		if (stream.next) stream.err("Unexpected trailing text");
		let match = dfa(nfa(expr));
		checkForDeadEnds(match, stream);
		return match;
	}
	matchType(type) {
		for (let i$1 = 0; i$1 < this.next.length; i$1++) if (this.next[i$1].type == type) return this.next[i$1].next;
		return null;
	}
	matchFragment(frag, start = 0, end = frag.childCount) {
		let cur = this;
		for (let i$1 = start; cur && i$1 < end; i$1++) cur = cur.matchType(frag.child(i$1).type);
		return cur;
	}
	get inlineContent() {
		return this.next.length != 0 && this.next[0].type.isInline;
	}
	get defaultType() {
		for (let i$1 = 0; i$1 < this.next.length; i$1++) {
			let { type } = this.next[i$1];
			if (!(type.isText || type.hasRequiredAttrs())) return type;
		}
		return null;
	}
	compatible(other) {
		for (let i$1 = 0; i$1 < this.next.length; i$1++) for (let j$1 = 0; j$1 < other.next.length; j$1++) if (this.next[i$1].type == other.next[j$1].type) return true;
		return false;
	}
	fillBefore(after, toEnd = false, startIndex = 0) {
		let seen = [this];
		function search(match, types) {
			let finished = match.matchFragment(after, startIndex);
			if (finished && (!toEnd || finished.validEnd)) return Fragment.from(types.map((tp) => tp.createAndFill()));
			for (let i$1 = 0; i$1 < match.next.length; i$1++) {
				let { type, next } = match.next[i$1];
				if (!(type.isText || type.hasRequiredAttrs()) && seen.indexOf(next) == -1) {
					seen.push(next);
					let found$1 = search(next, types.concat(type));
					if (found$1) return found$1;
				}
			}
			return null;
		}
		return search(this, []);
	}
	findWrapping(target) {
		for (let i$1 = 0; i$1 < this.wrapCache.length; i$1 += 2) if (this.wrapCache[i$1] == target) return this.wrapCache[i$1 + 1];
		let computed = this.computeWrapping(target);
		this.wrapCache.push(target, computed);
		return computed;
	}
	computeWrapping(target) {
		let seen = Object.create(null), active = [{
			match: this,
			type: null,
			via: null
		}];
		while (active.length) {
			let current = active.shift(), match = current.match;
			if (match.matchType(target)) {
				let result = [];
				for (let obj = current; obj.type; obj = obj.via) result.push(obj.type);
				return result.reverse();
			}
			for (let i$1 = 0; i$1 < match.next.length; i$1++) {
				let { type, next } = match.next[i$1];
				if (!type.isLeaf && !type.hasRequiredAttrs() && !(type.name in seen) && (!current.type || next.validEnd)) {
					active.push({
						match: type.contentMatch,
						type,
						via: current
					});
					seen[type.name] = true;
				}
			}
		}
		return null;
	}
	get edgeCount() {
		return this.next.length;
	}
	edge(n) {
		if (n >= this.next.length) throw new RangeError(`There's no ${n}th edge in this content match`);
		return this.next[n];
	}
	toString() {
		let seen = [];
		function scan(m$1) {
			seen.push(m$1);
			for (let i$1 = 0; i$1 < m$1.next.length; i$1++) if (seen.indexOf(m$1.next[i$1].next) == -1) scan(m$1.next[i$1].next);
		}
		scan(this);
		return seen.map((m$1, i$1) => {
			let out = i$1 + (m$1.validEnd ? "*" : " ") + " ";
			for (let i$2 = 0; i$2 < m$1.next.length; i$2++) out += (i$2 ? ", " : "") + m$1.next[i$2].type.name + "->" + seen.indexOf(m$1.next[i$2].next);
			return out;
		}).join("\n");
	}
};
ContentMatch.empty = new ContentMatch(true);
var TokenStream = class {
	constructor(string, nodeTypes) {
		this.string = string;
		this.nodeTypes = nodeTypes;
		this.inline = null;
		this.pos = 0;
		this.tokens = string.split(/\s*(?=\b|\W|$)/);
		if (this.tokens[this.tokens.length - 1] == "") this.tokens.pop();
		if (this.tokens[0] == "") this.tokens.shift();
	}
	get next() {
		return this.tokens[this.pos];
	}
	eat(tok) {
		return this.next == tok && (this.pos++ || true);
	}
	err(str) {
		throw new SyntaxError(str + " (in content expression '" + this.string + "')");
	}
};
function parseExpr(stream) {
	let exprs = [];
	do
		exprs.push(parseExprSeq(stream));
	while (stream.eat("|"));
	return exprs.length == 1 ? exprs[0] : {
		type: "choice",
		exprs
	};
}
function parseExprSeq(stream) {
	let exprs = [];
	do
		exprs.push(parseExprSubscript(stream));
	while (stream.next && stream.next != ")" && stream.next != "|");
	return exprs.length == 1 ? exprs[0] : {
		type: "seq",
		exprs
	};
}
function parseExprSubscript(stream) {
	let expr = parseExprAtom(stream);
	for (;;) if (stream.eat("+")) expr = {
		type: "plus",
		expr
	};
	else if (stream.eat("*")) expr = {
		type: "star",
		expr
	};
	else if (stream.eat("?")) expr = {
		type: "opt",
		expr
	};
	else if (stream.eat("{")) expr = parseExprRange(stream, expr);
	else break;
	return expr;
}
function parseNum(stream) {
	if (/\D/.test(stream.next)) stream.err("Expected number, got '" + stream.next + "'");
	let result = Number(stream.next);
	stream.pos++;
	return result;
}
function parseExprRange(stream, expr) {
	let min = parseNum(stream), max = min;
	if (stream.eat(",")) if (stream.next != "}") max = parseNum(stream);
	else max = -1;
	if (!stream.eat("}")) stream.err("Unclosed braced range");
	return {
		type: "range",
		min,
		max,
		expr
	};
}
function resolveName(stream, name) {
	let types = stream.nodeTypes, type = types[name];
	if (type) return [type];
	let result = [];
	for (let typeName in types) {
		let type$1 = types[typeName];
		if (type$1.isInGroup(name)) result.push(type$1);
	}
	if (result.length == 0) stream.err("No node type or group '" + name + "' found");
	return result;
}
function parseExprAtom(stream) {
	if (stream.eat("(")) {
		let expr = parseExpr(stream);
		if (!stream.eat(")")) stream.err("Missing closing paren");
		return expr;
	} else if (!/\W/.test(stream.next)) {
		let exprs = resolveName(stream, stream.next).map((type) => {
			if (stream.inline == null) stream.inline = type.isInline;
			else if (stream.inline != type.isInline) stream.err("Mixing inline and block content");
			return {
				type: "name",
				value: type
			};
		});
		stream.pos++;
		return exprs.length == 1 ? exprs[0] : {
			type: "choice",
			exprs
		};
	} else stream.err("Unexpected token '" + stream.next + "'");
}
function nfa(expr) {
	let nfa$1 = [[]];
	connect(compile(expr, 0), node());
	return nfa$1;
	function node() {
		return nfa$1.push([]) - 1;
	}
	function edge(from, to, term) {
		let edge$1 = {
			term,
			to
		};
		nfa$1[from].push(edge$1);
		return edge$1;
	}
	function connect(edges, to) {
		edges.forEach((edge$1) => edge$1.to = to);
	}
	function compile(expr$1, from) {
		if (expr$1.type == "choice") return expr$1.exprs.reduce((out, expr$2) => out.concat(compile(expr$2, from)), []);
		else if (expr$1.type == "seq") for (let i$1 = 0;; i$1++) {
			let next = compile(expr$1.exprs[i$1], from);
			if (i$1 == expr$1.exprs.length - 1) return next;
			connect(next, from = node());
		}
		else if (expr$1.type == "star") {
			let loop = node();
			edge(from, loop);
			connect(compile(expr$1.expr, loop), loop);
			return [edge(loop)];
		} else if (expr$1.type == "plus") {
			let loop = node();
			connect(compile(expr$1.expr, from), loop);
			connect(compile(expr$1.expr, loop), loop);
			return [edge(loop)];
		} else if (expr$1.type == "opt") return [edge(from)].concat(compile(expr$1.expr, from));
		else if (expr$1.type == "range") {
			let cur = from;
			for (let i$1 = 0; i$1 < expr$1.min; i$1++) {
				let next = node();
				connect(compile(expr$1.expr, cur), next);
				cur = next;
			}
			if (expr$1.max == -1) connect(compile(expr$1.expr, cur), cur);
			else for (let i$1 = expr$1.min; i$1 < expr$1.max; i$1++) {
				let next = node();
				edge(cur, next);
				connect(compile(expr$1.expr, cur), next);
				cur = next;
			}
			return [edge(cur)];
		} else if (expr$1.type == "name") return [edge(from, void 0, expr$1.value)];
		else throw new Error("Unknown expr type");
	}
}
function cmp(a, b$1) {
	return b$1 - a;
}
function nullFrom(nfa$1, node) {
	let result = [];
	scan(node);
	return result.sort(cmp);
	function scan(node$1) {
		let edges = nfa$1[node$1];
		if (edges.length == 1 && !edges[0].term) return scan(edges[0].to);
		result.push(node$1);
		for (let i$1 = 0; i$1 < edges.length; i$1++) {
			let { term, to } = edges[i$1];
			if (!term && result.indexOf(to) == -1) scan(to);
		}
	}
}
function dfa(nfa$1) {
	let labeled = Object.create(null);
	return explore(nullFrom(nfa$1, 0));
	function explore(states) {
		let out = [];
		states.forEach((node) => {
			nfa$1[node].forEach(({ term, to }) => {
				if (!term) return;
				let set;
				for (let i$1 = 0; i$1 < out.length; i$1++) if (out[i$1][0] == term) set = out[i$1][1];
				nullFrom(nfa$1, to).forEach((node$1) => {
					if (!set) out.push([term, set = []]);
					if (set.indexOf(node$1) == -1) set.push(node$1);
				});
			});
		});
		let state = labeled[states.join(",")] = new ContentMatch(states.indexOf(nfa$1.length - 1) > -1);
		for (let i$1 = 0; i$1 < out.length; i$1++) {
			let states$1 = out[i$1][1].sort(cmp);
			state.next.push({
				type: out[i$1][0],
				next: labeled[states$1.join(",")] || explore(states$1)
			});
		}
		return state;
	}
}
function checkForDeadEnds(match, stream) {
	for (let i$1 = 0, work = [match]; i$1 < work.length; i$1++) {
		let state = work[i$1], dead = !state.validEnd, nodes = [];
		for (let j$1 = 0; j$1 < state.next.length; j$1++) {
			let { type, next } = state.next[j$1];
			nodes.push(type.name);
			if (dead && !(type.isText || type.hasRequiredAttrs())) dead = false;
			if (work.indexOf(next) == -1) work.push(next);
		}
		if (dead) stream.err("Only non-generatable nodes (" + nodes.join(", ") + ") in a required position (see https://prosemirror.net/docs/guide/#generatable)");
	}
}
function defaultAttrs(attrs) {
	let defaults$1 = Object.create(null);
	for (let attrName in attrs) {
		let attr = attrs[attrName];
		if (!attr.hasDefault) return null;
		defaults$1[attrName] = attr.default;
	}
	return defaults$1;
}
function computeAttrs(attrs, value) {
	let built = Object.create(null);
	for (let name in attrs) {
		let given = value && value[name];
		if (given === void 0) {
			let attr = attrs[name];
			if (attr.hasDefault) given = attr.default;
			else throw new RangeError("No value supplied for attribute " + name);
		}
		built[name] = given;
	}
	return built;
}
function checkAttrs(attrs, values, type, name) {
	for (let name$1 in values) if (!(name$1 in attrs)) throw new RangeError(`Unsupported attribute ${name$1} for ${type} of type ${name$1}`);
	for (let name$1 in attrs) {
		let attr = attrs[name$1];
		if (attr.validate) attr.validate(values[name$1]);
	}
}
function initAttrs(typeName, attrs) {
	let result = Object.create(null);
	if (attrs) for (let name in attrs) result[name] = new Attribute(typeName, name, attrs[name]);
	return result;
}
var NodeType$1 = class NodeType$1 {
	constructor(name, schema, spec) {
		this.name = name;
		this.schema = schema;
		this.spec = spec;
		this.markSet = null;
		this.groups = spec.group ? spec.group.split(" ") : [];
		this.attrs = initAttrs(name, spec.attrs);
		this.defaultAttrs = defaultAttrs(this.attrs);
		this.contentMatch = null;
		this.inlineContent = null;
		this.isBlock = !(spec.inline || name == "text");
		this.isText = name == "text";
	}
	get isInline() {
		return !this.isBlock;
	}
	get isTextblock() {
		return this.isBlock && this.inlineContent;
	}
	get isLeaf() {
		return this.contentMatch == ContentMatch.empty;
	}
	get isAtom() {
		return this.isLeaf || !!this.spec.atom;
	}
	isInGroup(group) {
		return this.groups.indexOf(group) > -1;
	}
	get whitespace() {
		return this.spec.whitespace || (this.spec.code ? "pre" : "normal");
	}
	hasRequiredAttrs() {
		for (let n in this.attrs) if (this.attrs[n].isRequired) return true;
		return false;
	}
	compatibleContent(other) {
		return this == other || this.contentMatch.compatible(other.contentMatch);
	}
	computeAttrs(attrs) {
		if (!attrs && this.defaultAttrs) return this.defaultAttrs;
		else return computeAttrs(this.attrs, attrs);
	}
	create(attrs = null, content, marks) {
		if (this.isText) throw new Error("NodeType.create can't construct text nodes");
		return new Node(this, this.computeAttrs(attrs), Fragment.from(content), Mark$1.setFrom(marks));
	}
	createChecked(attrs = null, content, marks) {
		content = Fragment.from(content);
		this.checkContent(content);
		return new Node(this, this.computeAttrs(attrs), content, Mark$1.setFrom(marks));
	}
	createAndFill(attrs = null, content, marks) {
		attrs = this.computeAttrs(attrs);
		content = Fragment.from(content);
		if (content.size) {
			let before = this.contentMatch.fillBefore(content);
			if (!before) return null;
			content = before.append(content);
		}
		let matched = this.contentMatch.matchFragment(content);
		let after = matched && matched.fillBefore(Fragment.empty, true);
		if (!after) return null;
		return new Node(this, attrs, content.append(after), Mark$1.setFrom(marks));
	}
	validContent(content) {
		let result = this.contentMatch.matchFragment(content);
		if (!result || !result.validEnd) return false;
		for (let i$1 = 0; i$1 < content.childCount; i$1++) if (!this.allowsMarks(content.child(i$1).marks)) return false;
		return true;
	}
	checkContent(content) {
		if (!this.validContent(content)) throw new RangeError(`Invalid content for node ${this.name}: ${content.toString().slice(0, 50)}`);
	}
	checkAttrs(attrs) {
		checkAttrs(this.attrs, attrs, "node", this.name);
	}
	allowsMarkType(markType) {
		return this.markSet == null || this.markSet.indexOf(markType) > -1;
	}
	allowsMarks(marks) {
		if (this.markSet == null) return true;
		for (let i$1 = 0; i$1 < marks.length; i$1++) if (!this.allowsMarkType(marks[i$1].type)) return false;
		return true;
	}
	allowedMarks(marks) {
		if (this.markSet == null) return marks;
		let copy$1;
		for (let i$1 = 0; i$1 < marks.length; i$1++) if (!this.allowsMarkType(marks[i$1].type)) {
			if (!copy$1) copy$1 = marks.slice(0, i$1);
		} else if (copy$1) copy$1.push(marks[i$1]);
		return !copy$1 ? marks : copy$1.length ? copy$1 : Mark$1.none;
	}
	static compile(nodes, schema) {
		let result = Object.create(null);
		nodes.forEach((name, spec) => result[name] = new NodeType$1(name, schema, spec));
		let topType = schema.spec.topNode || "doc";
		if (!result[topType]) throw new RangeError("Schema is missing its top node type ('" + topType + "')");
		if (!result.text) throw new RangeError("Every schema needs a 'text' type");
		for (let _$1 in result.text.attrs) throw new RangeError("The text node type should not have attributes");
		return result;
	}
};
function validateType(typeName, attrName, type) {
	let types = type.split("|");
	return (value) => {
		let name = value === null ? "null" : typeof value;
		if (types.indexOf(name) < 0) throw new RangeError(`Expected value of type ${types} for attribute ${attrName} on type ${typeName}, got ${name}`);
	};
}
var Attribute = class {
	constructor(typeName, attrName, options) {
		this.hasDefault = Object.prototype.hasOwnProperty.call(options, "default");
		this.default = options.default;
		this.validate = typeof options.validate == "string" ? validateType(typeName, attrName, options.validate) : options.validate;
	}
	get isRequired() {
		return !this.hasDefault;
	}
};
var MarkType = class MarkType {
	constructor(name, rank, schema, spec) {
		this.name = name;
		this.rank = rank;
		this.schema = schema;
		this.spec = spec;
		this.attrs = initAttrs(name, spec.attrs);
		this.excluded = null;
		let defaults$1 = defaultAttrs(this.attrs);
		this.instance = defaults$1 ? new Mark$1(this, defaults$1) : null;
	}
	create(attrs = null) {
		if (!attrs && this.instance) return this.instance;
		return new Mark$1(this, computeAttrs(this.attrs, attrs));
	}
	static compile(marks, schema) {
		let result = Object.create(null), rank = 0;
		marks.forEach((name, spec) => result[name] = new MarkType(name, rank++, schema, spec));
		return result;
	}
	removeFromSet(set) {
		for (var i$1 = 0; i$1 < set.length; i$1++) if (set[i$1].type == this) {
			set = set.slice(0, i$1).concat(set.slice(i$1 + 1));
			i$1--;
		}
		return set;
	}
	isInSet(set) {
		for (let i$1 = 0; i$1 < set.length; i$1++) if (set[i$1].type == this) return set[i$1];
	}
	checkAttrs(attrs) {
		checkAttrs(this.attrs, attrs, "mark", this.name);
	}
	excludes(other) {
		return this.excluded.indexOf(other) > -1;
	}
};
var Schema = class {
	constructor(spec) {
		this.linebreakReplacement = null;
		this.cached = Object.create(null);
		let instanceSpec = this.spec = {};
		for (let prop in spec) instanceSpec[prop] = spec[prop];
		instanceSpec.nodes = dist_default$1.from(spec.nodes), instanceSpec.marks = dist_default$1.from(spec.marks || {}), this.nodes = NodeType$1.compile(this.spec.nodes, this);
		this.marks = MarkType.compile(this.spec.marks, this);
		let contentExprCache = Object.create(null);
		for (let prop in this.nodes) {
			if (prop in this.marks) throw new RangeError(prop + " can not be both a node and a mark");
			let type = this.nodes[prop], contentExpr = type.spec.content || "", markExpr = type.spec.marks;
			type.contentMatch = contentExprCache[contentExpr] || (contentExprCache[contentExpr] = ContentMatch.parse(contentExpr, this.nodes));
			type.inlineContent = type.contentMatch.inlineContent;
			if (type.spec.linebreakReplacement) {
				if (this.linebreakReplacement) throw new RangeError("Multiple linebreak nodes defined");
				if (!type.isInline || !type.isLeaf) throw new RangeError("Linebreak replacement nodes must be inline leaf nodes");
				this.linebreakReplacement = type;
			}
			type.markSet = markExpr == "_" ? null : markExpr ? gatherMarks(this, markExpr.split(" ")) : markExpr == "" || !type.inlineContent ? [] : null;
		}
		for (let prop in this.marks) {
			let type = this.marks[prop], excl = type.spec.excludes;
			type.excluded = excl == null ? [type] : excl == "" ? [] : gatherMarks(this, excl.split(" "));
		}
		this.nodeFromJSON = (json) => Node.fromJSON(this, json);
		this.markFromJSON = (json) => Mark$1.fromJSON(this, json);
		this.topNodeType = this.nodes[this.spec.topNode || "doc"];
		this.cached.wrappings = Object.create(null);
	}
	node(type, attrs = null, content, marks) {
		if (typeof type == "string") type = this.nodeType(type);
		else if (!(type instanceof NodeType$1)) throw new RangeError("Invalid node type: " + type);
		else if (type.schema != this) throw new RangeError("Node type from different schema used (" + type.name + ")");
		return type.createChecked(attrs, content, marks);
	}
	text(text, marks) {
		let type = this.nodes.text;
		return new TextNode(type, type.defaultAttrs, text, Mark$1.setFrom(marks));
	}
	mark(type, attrs) {
		if (typeof type == "string") type = this.marks[type];
		return type.create(attrs);
	}
	nodeType(name) {
		let found$1 = this.nodes[name];
		if (!found$1) throw new RangeError("Unknown node type: " + name);
		return found$1;
	}
};
function gatherMarks(schema, marks) {
	let found$1 = [];
	for (let i$1 = 0; i$1 < marks.length; i$1++) {
		let name = marks[i$1], mark = schema.marks[name], ok = mark;
		if (mark) found$1.push(mark);
		else for (let prop in schema.marks) {
			let mark$1 = schema.marks[prop];
			if (name == "_" || mark$1.spec.group && mark$1.spec.group.split(" ").indexOf(name) > -1) found$1.push(ok = mark$1);
		}
		if (!ok) throw new SyntaxError("Unknown mark type: '" + marks[i$1] + "'");
	}
	return found$1;
}
function isTagRule(rule) {
	return rule.tag != null;
}
function isStyleRule(rule) {
	return rule.style != null;
}
var DOMParser = class DOMParser {
	constructor(schema, rules) {
		this.schema = schema;
		this.rules = rules;
		this.tags = [];
		this.styles = [];
		let matchedStyles = this.matchedStyles = [];
		rules.forEach((rule) => {
			if (isTagRule(rule)) this.tags.push(rule);
			else if (isStyleRule(rule)) {
				let prop = /[^=]*/.exec(rule.style)[0];
				if (matchedStyles.indexOf(prop) < 0) matchedStyles.push(prop);
				this.styles.push(rule);
			}
		});
		this.normalizeLists = !this.tags.some((r) => {
			if (!/^(ul|ol)\b/.test(r.tag) || !r.node) return false;
			let node = schema.nodes[r.node];
			return node.contentMatch.matchType(node);
		});
	}
	parse(dom, options = {}) {
		let context = new ParseContext(this, options, false);
		context.addAll(dom, Mark$1.none, options.from, options.to);
		return context.finish();
	}
	parseSlice(dom, options = {}) {
		let context = new ParseContext(this, options, true);
		context.addAll(dom, Mark$1.none, options.from, options.to);
		return Slice.maxOpen(context.finish());
	}
	matchTag(dom, context, after) {
		for (let i$1 = after ? this.tags.indexOf(after) + 1 : 0; i$1 < this.tags.length; i$1++) {
			let rule = this.tags[i$1];
			if (matches(dom, rule.tag) && (rule.namespace === void 0 || dom.namespaceURI == rule.namespace) && (!rule.context || context.matchesContext(rule.context))) {
				if (rule.getAttrs) {
					let result = rule.getAttrs(dom);
					if (result === false) continue;
					rule.attrs = result || void 0;
				}
				return rule;
			}
		}
	}
	matchStyle(prop, value, context, after) {
		for (let i$1 = after ? this.styles.indexOf(after) + 1 : 0; i$1 < this.styles.length; i$1++) {
			let rule = this.styles[i$1], style$1 = rule.style;
			if (style$1.indexOf(prop) != 0 || rule.context && !context.matchesContext(rule.context) || style$1.length > prop.length && (style$1.charCodeAt(prop.length) != 61 || style$1.slice(prop.length + 1) != value)) continue;
			if (rule.getAttrs) {
				let result = rule.getAttrs(value);
				if (result === false) continue;
				rule.attrs = result || void 0;
			}
			return rule;
		}
	}
	static schemaRules(schema) {
		let result = [];
		function insert(rule) {
			let priority = rule.priority == null ? 50 : rule.priority, i$1 = 0;
			for (; i$1 < result.length; i$1++) {
				let next = result[i$1];
				if ((next.priority == null ? 50 : next.priority) < priority) break;
			}
			result.splice(i$1, 0, rule);
		}
		for (let name in schema.marks) {
			let rules = schema.marks[name].spec.parseDOM;
			if (rules) rules.forEach((rule) => {
				insert(rule = copy(rule));
				if (!(rule.mark || rule.ignore || rule.clearMark)) rule.mark = name;
			});
		}
		for (let name in schema.nodes) {
			let rules = schema.nodes[name].spec.parseDOM;
			if (rules) rules.forEach((rule) => {
				insert(rule = copy(rule));
				if (!(rule.node || rule.ignore || rule.mark)) rule.node = name;
			});
		}
		return result;
	}
	static fromSchema(schema) {
		return schema.cached.domParser || (schema.cached.domParser = new DOMParser(schema, DOMParser.schemaRules(schema)));
	}
};
var blockTags = {
	address: true,
	article: true,
	aside: true,
	blockquote: true,
	canvas: true,
	dd: true,
	div: true,
	dl: true,
	fieldset: true,
	figcaption: true,
	figure: true,
	footer: true,
	form: true,
	h1: true,
	h2: true,
	h3: true,
	h4: true,
	h5: true,
	h6: true,
	header: true,
	hgroup: true,
	hr: true,
	li: true,
	noscript: true,
	ol: true,
	output: true,
	p: true,
	pre: true,
	section: true,
	table: true,
	tfoot: true,
	ul: true
};
var ignoreTags = {
	head: true,
	noscript: true,
	object: true,
	script: true,
	style: true,
	title: true
};
var listTags = {
	ol: true,
	ul: true
};
var OPT_PRESERVE_WS = 1, OPT_PRESERVE_WS_FULL = 2, OPT_OPEN_LEFT = 4;
function wsOptionsFor(type, preserveWhitespace, base$1) {
	if (preserveWhitespace != null) return (preserveWhitespace ? OPT_PRESERVE_WS : 0) | (preserveWhitespace === "full" ? OPT_PRESERVE_WS_FULL : 0);
	return type && type.whitespace == "pre" ? OPT_PRESERVE_WS | OPT_PRESERVE_WS_FULL : base$1 & ~OPT_OPEN_LEFT;
}
var NodeContext = class {
	constructor(type, attrs, marks, solid, match, options) {
		this.type = type;
		this.attrs = attrs;
		this.marks = marks;
		this.solid = solid;
		this.options = options;
		this.content = [];
		this.activeMarks = Mark$1.none;
		this.match = match || (options & OPT_OPEN_LEFT ? null : type.contentMatch);
	}
	findWrapping(node) {
		if (!this.match) {
			if (!this.type) return [];
			let fill = this.type.contentMatch.fillBefore(Fragment.from(node));
			if (fill) this.match = this.type.contentMatch.matchFragment(fill);
			else {
				let start = this.type.contentMatch, wrap$1;
				if (wrap$1 = start.findWrapping(node.type)) {
					this.match = start;
					return wrap$1;
				} else return null;
			}
		}
		return this.match.findWrapping(node.type);
	}
	finish(openEnd) {
		if (!(this.options & OPT_PRESERVE_WS)) {
			let last = this.content[this.content.length - 1], m$1;
			if (last && last.isText && (m$1 = /[ \t\r\n\u000c]+$/.exec(last.text))) {
				let text = last;
				if (last.text.length == m$1[0].length) this.content.pop();
				else this.content[this.content.length - 1] = text.withText(text.text.slice(0, text.text.length - m$1[0].length));
			}
		}
		let content = Fragment.from(this.content);
		if (!openEnd && this.match) content = content.append(this.match.fillBefore(Fragment.empty, true));
		return this.type ? this.type.create(this.attrs, content, this.marks) : content;
	}
	inlineContext(node) {
		if (this.type) return this.type.inlineContent;
		if (this.content.length) return this.content[0].isInline;
		return node.parentNode && !blockTags.hasOwnProperty(node.parentNode.nodeName.toLowerCase());
	}
};
var ParseContext = class {
	constructor(parser, options, isOpen) {
		this.parser = parser;
		this.options = options;
		this.isOpen = isOpen;
		this.open = 0;
		this.localPreserveWS = false;
		let topNode = options.topNode, topContext;
		let topOptions = wsOptionsFor(null, options.preserveWhitespace, 0) | (isOpen ? OPT_OPEN_LEFT : 0);
		if (topNode) topContext = new NodeContext(topNode.type, topNode.attrs, Mark$1.none, true, options.topMatch || topNode.type.contentMatch, topOptions);
		else if (isOpen) topContext = new NodeContext(null, null, Mark$1.none, true, null, topOptions);
		else topContext = new NodeContext(parser.schema.topNodeType, null, Mark$1.none, true, null, topOptions);
		this.nodes = [topContext];
		this.find = options.findPositions;
		this.needsBlock = false;
	}
	get top() {
		return this.nodes[this.open];
	}
	addDOM(dom, marks) {
		if (dom.nodeType == 3) this.addTextNode(dom, marks);
		else if (dom.nodeType == 1) this.addElement(dom, marks);
	}
	addTextNode(dom, marks) {
		let value = dom.nodeValue;
		let top = this.top, preserveWS = top.options & OPT_PRESERVE_WS_FULL ? "full" : this.localPreserveWS || (top.options & OPT_PRESERVE_WS) > 0;
		let { schema } = this.parser;
		if (preserveWS === "full" || top.inlineContext(dom) || /[^ \t\r\n\u000c]/.test(value)) {
			if (!preserveWS) {
				value = value.replace(/[ \t\r\n\u000c]+/g, " ");
				if (/^[ \t\r\n\u000c]/.test(value) && this.open == this.nodes.length - 1) {
					let nodeBefore = top.content[top.content.length - 1];
					let domNodeBefore = dom.previousSibling;
					if (!nodeBefore || domNodeBefore && domNodeBefore.nodeName == "BR" || nodeBefore.isText && /[ \t\r\n\u000c]$/.test(nodeBefore.text)) value = value.slice(1);
				}
			} else if (preserveWS === "full") value = value.replace(/\r\n?/g, "\n");
			else if (schema.linebreakReplacement && /[\r\n]/.test(value) && this.top.findWrapping(schema.linebreakReplacement.create())) {
				let lines = value.split(/\r?\n|\r/);
				for (let i$1 = 0; i$1 < lines.length; i$1++) {
					if (i$1) this.insertNode(schema.linebreakReplacement.create(), marks, true);
					if (lines[i$1]) this.insertNode(schema.text(lines[i$1]), marks, !/\S/.test(lines[i$1]));
				}
				value = "";
			} else value = value.replace(/\r?\n|\r/g, " ");
			if (value) this.insertNode(schema.text(value), marks, !/\S/.test(value));
			this.findInText(dom);
		} else this.findInside(dom);
	}
	addElement(dom, marks, matchAfter) {
		let outerWS = this.localPreserveWS, top = this.top;
		if (dom.tagName == "PRE" || /pre/.test(dom.style && dom.style.whiteSpace)) this.localPreserveWS = true;
		let name = dom.nodeName.toLowerCase(), ruleID;
		if (listTags.hasOwnProperty(name) && this.parser.normalizeLists) normalizeList(dom);
		let rule = this.options.ruleFromNode && this.options.ruleFromNode(dom) || (ruleID = this.parser.matchTag(dom, this, matchAfter));
		out: if (rule ? rule.ignore : ignoreTags.hasOwnProperty(name)) {
			this.findInside(dom);
			this.ignoreFallback(dom, marks);
		} else if (!rule || rule.skip || rule.closeParent) {
			if (rule && rule.closeParent) this.open = Math.max(0, this.open - 1);
			else if (rule && rule.skip.nodeType) dom = rule.skip;
			let sync, oldNeedsBlock = this.needsBlock;
			if (blockTags.hasOwnProperty(name)) {
				if (top.content.length && top.content[0].isInline && this.open) {
					this.open--;
					top = this.top;
				}
				sync = true;
				if (!top.type) this.needsBlock = true;
			} else if (!dom.firstChild) {
				this.leafFallback(dom, marks);
				break out;
			}
			let innerMarks = rule && rule.skip ? marks : this.readStyles(dom, marks);
			if (innerMarks) this.addAll(dom, innerMarks);
			if (sync) this.sync(top);
			this.needsBlock = oldNeedsBlock;
		} else {
			let innerMarks = this.readStyles(dom, marks);
			if (innerMarks) this.addElementByRule(dom, rule, innerMarks, rule.consuming === false ? ruleID : void 0);
		}
		this.localPreserveWS = outerWS;
	}
	leafFallback(dom, marks) {
		if (dom.nodeName == "BR" && this.top.type && this.top.type.inlineContent) this.addTextNode(dom.ownerDocument.createTextNode("\n"), marks);
	}
	ignoreFallback(dom, marks) {
		if (dom.nodeName == "BR" && (!this.top.type || !this.top.type.inlineContent)) this.findPlace(this.parser.schema.text("-"), marks, true);
	}
	readStyles(dom, marks) {
		let styles = dom.style;
		if (styles && styles.length) for (let i$1 = 0; i$1 < this.parser.matchedStyles.length; i$1++) {
			let name = this.parser.matchedStyles[i$1], value = styles.getPropertyValue(name);
			if (value) for (let after = void 0;;) {
				let rule = this.parser.matchStyle(name, value, this, after);
				if (!rule) break;
				if (rule.ignore) return null;
				if (rule.clearMark) marks = marks.filter((m$1) => !rule.clearMark(m$1));
				else marks = marks.concat(this.parser.schema.marks[rule.mark].create(rule.attrs));
				if (rule.consuming === false) after = rule;
				else break;
			}
		}
		return marks;
	}
	addElementByRule(dom, rule, marks, continueAfter) {
		let sync, nodeType;
		if (rule.node) {
			nodeType = this.parser.schema.nodes[rule.node];
			if (!nodeType.isLeaf) {
				let inner = this.enter(nodeType, rule.attrs || null, marks, rule.preserveWhitespace);
				if (inner) {
					sync = true;
					marks = inner;
				}
			} else if (!this.insertNode(nodeType.create(rule.attrs), marks, dom.nodeName == "BR")) this.leafFallback(dom, marks);
		} else {
			let markType = this.parser.schema.marks[rule.mark];
			marks = marks.concat(markType.create(rule.attrs));
		}
		let startIn = this.top;
		if (nodeType && nodeType.isLeaf) this.findInside(dom);
		else if (continueAfter) this.addElement(dom, marks, continueAfter);
		else if (rule.getContent) {
			this.findInside(dom);
			rule.getContent(dom, this.parser.schema).forEach((node) => this.insertNode(node, marks, false));
		} else {
			let contentDOM = dom;
			if (typeof rule.contentElement == "string") contentDOM = dom.querySelector(rule.contentElement);
			else if (typeof rule.contentElement == "function") contentDOM = rule.contentElement(dom);
			else if (rule.contentElement) contentDOM = rule.contentElement;
			this.findAround(dom, contentDOM, true);
			this.addAll(contentDOM, marks);
			this.findAround(dom, contentDOM, false);
		}
		if (sync && this.sync(startIn)) this.open--;
	}
	addAll(parent, marks, startIndex, endIndex) {
		let index = startIndex || 0;
		for (let dom = startIndex ? parent.childNodes[startIndex] : parent.firstChild, end = endIndex == null ? null : parent.childNodes[endIndex]; dom != end; dom = dom.nextSibling, ++index) {
			this.findAtPoint(parent, index);
			this.addDOM(dom, marks);
		}
		this.findAtPoint(parent, index);
	}
	findPlace(node, marks, cautious) {
		let route, sync;
		for (let depth = this.open, penalty = 0; depth >= 0; depth--) {
			let cx = this.nodes[depth];
			let found$1 = cx.findWrapping(node);
			if (found$1 && (!route || route.length > found$1.length + penalty)) {
				route = found$1;
				sync = cx;
				if (!found$1.length) break;
			}
			if (cx.solid) {
				if (cautious) break;
				penalty += 2;
			}
		}
		if (!route) return null;
		this.sync(sync);
		for (let i$1 = 0; i$1 < route.length; i$1++) marks = this.enterInner(route[i$1], null, marks, false);
		return marks;
	}
	insertNode(node, marks, cautious) {
		if (node.isInline && this.needsBlock && !this.top.type) {
			let block = this.textblockFromContext();
			if (block) marks = this.enterInner(block, null, marks);
		}
		let innerMarks = this.findPlace(node, marks, cautious);
		if (innerMarks) {
			this.closeExtra();
			let top = this.top;
			if (top.match) top.match = top.match.matchType(node.type);
			let nodeMarks = Mark$1.none;
			for (let m$1 of innerMarks.concat(node.marks)) if (top.type ? top.type.allowsMarkType(m$1.type) : markMayApply(m$1.type, node.type)) nodeMarks = m$1.addToSet(nodeMarks);
			top.content.push(node.mark(nodeMarks));
			return true;
		}
		return false;
	}
	enter(type, attrs, marks, preserveWS) {
		let innerMarks = this.findPlace(type.create(attrs), marks, false);
		if (innerMarks) innerMarks = this.enterInner(type, attrs, marks, true, preserveWS);
		return innerMarks;
	}
	enterInner(type, attrs, marks, solid = false, preserveWS) {
		this.closeExtra();
		let top = this.top;
		top.match = top.match && top.match.matchType(type);
		let options = wsOptionsFor(type, preserveWS, top.options);
		if (top.options & OPT_OPEN_LEFT && top.content.length == 0) options |= OPT_OPEN_LEFT;
		let applyMarks = Mark$1.none;
		marks = marks.filter((m$1) => {
			if (top.type ? top.type.allowsMarkType(m$1.type) : markMayApply(m$1.type, type)) {
				applyMarks = m$1.addToSet(applyMarks);
				return false;
			}
			return true;
		});
		this.nodes.push(new NodeContext(type, attrs, applyMarks, solid, null, options));
		this.open++;
		return marks;
	}
	closeExtra(openEnd = false) {
		let i$1 = this.nodes.length - 1;
		if (i$1 > this.open) {
			for (; i$1 > this.open; i$1--) this.nodes[i$1 - 1].content.push(this.nodes[i$1].finish(openEnd));
			this.nodes.length = this.open + 1;
		}
	}
	finish() {
		this.open = 0;
		this.closeExtra(this.isOpen);
		return this.nodes[0].finish(!!(this.isOpen || this.options.topOpen));
	}
	sync(to) {
		for (let i$1 = this.open; i$1 >= 0; i$1--) if (this.nodes[i$1] == to) {
			this.open = i$1;
			return true;
		} else if (this.localPreserveWS) this.nodes[i$1].options |= OPT_PRESERVE_WS;
		return false;
	}
	get currentPos() {
		this.closeExtra();
		let pos = 0;
		for (let i$1 = this.open; i$1 >= 0; i$1--) {
			let content = this.nodes[i$1].content;
			for (let j$1 = content.length - 1; j$1 >= 0; j$1--) pos += content[j$1].nodeSize;
			if (i$1) pos++;
		}
		return pos;
	}
	findAtPoint(parent, offset) {
		if (this.find) {
			for (let i$1 = 0; i$1 < this.find.length; i$1++) if (this.find[i$1].node == parent && this.find[i$1].offset == offset) this.find[i$1].pos = this.currentPos;
		}
	}
	findInside(parent) {
		if (this.find) {
			for (let i$1 = 0; i$1 < this.find.length; i$1++) if (this.find[i$1].pos == null && parent.nodeType == 1 && parent.contains(this.find[i$1].node)) this.find[i$1].pos = this.currentPos;
		}
	}
	findAround(parent, content, before) {
		if (parent != content && this.find) {
			for (let i$1 = 0; i$1 < this.find.length; i$1++) if (this.find[i$1].pos == null && parent.nodeType == 1 && parent.contains(this.find[i$1].node)) {
				if (content.compareDocumentPosition(this.find[i$1].node) & (before ? 2 : 4)) this.find[i$1].pos = this.currentPos;
			}
		}
	}
	findInText(textNode) {
		if (this.find) {
			for (let i$1 = 0; i$1 < this.find.length; i$1++) if (this.find[i$1].node == textNode) this.find[i$1].pos = this.currentPos - (textNode.nodeValue.length - this.find[i$1].offset);
		}
	}
	matchesContext(context) {
		if (context.indexOf("|") > -1) return context.split(/\s*\|\s*/).some(this.matchesContext, this);
		let parts = context.split("/");
		let option = this.options.context;
		let useRoot = !this.isOpen && (!option || option.parent.type == this.nodes[0].type);
		let minDepth = -(option ? option.depth + 1 : 0) + (useRoot ? 0 : 1);
		let match = (i$1, depth) => {
			for (; i$1 >= 0; i$1--) {
				let part = parts[i$1];
				if (part == "") {
					if (i$1 == parts.length - 1 || i$1 == 0) continue;
					for (; depth >= minDepth; depth--) if (match(i$1 - 1, depth)) return true;
					return false;
				} else {
					let next = depth > 0 || depth == 0 && useRoot ? this.nodes[depth].type : option && depth >= minDepth ? option.node(depth - minDepth).type : null;
					if (!next || next.name != part && !next.isInGroup(part)) return false;
					depth--;
				}
			}
			return true;
		};
		return match(parts.length - 1, this.open);
	}
	textblockFromContext() {
		let $context = this.options.context;
		if ($context) for (let d = $context.depth; d >= 0; d--) {
			let deflt = $context.node(d).contentMatchAt($context.indexAfter(d)).defaultType;
			if (deflt && deflt.isTextblock && deflt.defaultAttrs) return deflt;
		}
		for (let name in this.parser.schema.nodes) {
			let type = this.parser.schema.nodes[name];
			if (type.isTextblock && type.defaultAttrs) return type;
		}
	}
};
function normalizeList(dom) {
	for (let child = dom.firstChild, prevItem = null; child; child = child.nextSibling) {
		let name = child.nodeType == 1 ? child.nodeName.toLowerCase() : null;
		if (name && listTags.hasOwnProperty(name) && prevItem) {
			prevItem.appendChild(child);
			child = prevItem;
		} else if (name == "li") prevItem = child;
		else if (name) prevItem = null;
	}
}
function matches(dom, selector) {
	return (dom.matches || dom.msMatchesSelector || dom.webkitMatchesSelector || dom.mozMatchesSelector).call(dom, selector);
}
function copy(obj) {
	let copy$1 = {};
	for (let prop in obj) copy$1[prop] = obj[prop];
	return copy$1;
}
function markMayApply(markType, nodeType) {
	let nodes = nodeType.schema.nodes;
	for (let name in nodes) {
		let parent = nodes[name];
		if (!parent.allowsMarkType(markType)) continue;
		let seen = [], scan = (match) => {
			seen.push(match);
			for (let i$1 = 0; i$1 < match.edgeCount; i$1++) {
				let { type, next } = match.edge(i$1);
				if (type == nodeType) return true;
				if (seen.indexOf(next) < 0 && scan(next)) return true;
			}
		};
		if (scan(parent.contentMatch)) return true;
	}
}
var DOMSerializer = class DOMSerializer {
	constructor(nodes, marks) {
		this.nodes = nodes;
		this.marks = marks;
	}
	serializeFragment(fragment, options = {}, target) {
		if (!target) target = doc$1(options).createDocumentFragment();
		let top = target, active = [];
		fragment.forEach((node) => {
			if (active.length || node.marks.length) {
				let keep = 0, rendered = 0;
				while (keep < active.length && rendered < node.marks.length) {
					let next = node.marks[rendered];
					if (!this.marks[next.type.name]) {
						rendered++;
						continue;
					}
					if (!next.eq(active[keep][0]) || next.type.spec.spanning === false) break;
					keep++;
					rendered++;
				}
				while (keep < active.length) top = active.pop()[1];
				while (rendered < node.marks.length) {
					let add = node.marks[rendered++];
					let markDOM = this.serializeMark(add, node.isInline, options);
					if (markDOM) {
						active.push([add, top]);
						top.appendChild(markDOM.dom);
						top = markDOM.contentDOM || markDOM.dom;
					}
				}
			}
			top.appendChild(this.serializeNodeInner(node, options));
		});
		return target;
	}
	serializeNodeInner(node, options) {
		let { dom, contentDOM } = renderSpec(doc$1(options), this.nodes[node.type.name](node), null, node.attrs);
		if (contentDOM) {
			if (node.isLeaf) throw new RangeError("Content hole not allowed in a leaf node spec");
			this.serializeFragment(node.content, options, contentDOM);
		}
		return dom;
	}
	serializeNode(node, options = {}) {
		let dom = this.serializeNodeInner(node, options);
		for (let i$1 = node.marks.length - 1; i$1 >= 0; i$1--) {
			let wrap$1 = this.serializeMark(node.marks[i$1], node.isInline, options);
			if (wrap$1) {
				(wrap$1.contentDOM || wrap$1.dom).appendChild(dom);
				dom = wrap$1.dom;
			}
		}
		return dom;
	}
	serializeMark(mark, inline, options = {}) {
		let toDOM = this.marks[mark.type.name];
		return toDOM && renderSpec(doc$1(options), toDOM(mark, inline), null, mark.attrs);
	}
	static renderSpec(doc$2, structure, xmlNS = null, blockArraysIn) {
		return renderSpec(doc$2, structure, xmlNS, blockArraysIn);
	}
	static fromSchema(schema) {
		return schema.cached.domSerializer || (schema.cached.domSerializer = new DOMSerializer(this.nodesFromSchema(schema), this.marksFromSchema(schema)));
	}
	static nodesFromSchema(schema) {
		let result = gatherToDOM(schema.nodes);
		if (!result.text) result.text = (node) => node.text;
		return result;
	}
	static marksFromSchema(schema) {
		return gatherToDOM(schema.marks);
	}
};
function gatherToDOM(obj) {
	let result = {};
	for (let name in obj) {
		let toDOM = obj[name].spec.toDOM;
		if (toDOM) result[name] = toDOM;
	}
	return result;
}
function doc$1(options) {
	return options.document || window.document;
}
var suspiciousAttributeCache = /* @__PURE__ */ new WeakMap();
function suspiciousAttributes(attrs) {
	let value = suspiciousAttributeCache.get(attrs);
	if (value === void 0) suspiciousAttributeCache.set(attrs, value = suspiciousAttributesInner(attrs));
	return value;
}
function suspiciousAttributesInner(attrs) {
	let result = null;
	function scan(value) {
		if (value && typeof value == "object") if (Array.isArray(value)) if (typeof value[0] == "string") {
			if (!result) result = [];
			result.push(value);
		} else for (let i$1 = 0; i$1 < value.length; i$1++) scan(value[i$1]);
		else for (let prop in value) scan(value[prop]);
	}
	scan(attrs);
	return result;
}
function renderSpec(doc$2, structure, xmlNS, blockArraysIn) {
	if (typeof structure == "string") return { dom: doc$2.createTextNode(structure) };
	if (structure.nodeType != null) return { dom: structure };
	if (structure.dom && structure.dom.nodeType != null) return structure;
	let tagName = structure[0], suspicious;
	if (typeof tagName != "string") throw new RangeError("Invalid array passed to renderSpec");
	if (blockArraysIn && (suspicious = suspiciousAttributes(blockArraysIn)) && suspicious.indexOf(structure) > -1) throw new RangeError("Using an array from an attribute object as a DOM spec. This may be an attempted cross site scripting attack.");
	let space = tagName.indexOf(" ");
	if (space > 0) {
		xmlNS = tagName.slice(0, space);
		tagName = tagName.slice(space + 1);
	}
	let contentDOM;
	let dom = xmlNS ? doc$2.createElementNS(xmlNS, tagName) : doc$2.createElement(tagName);
	let attrs = structure[1], start = 1;
	if (attrs && typeof attrs == "object" && attrs.nodeType == null && !Array.isArray(attrs)) {
		start = 2;
		for (let name in attrs) if (attrs[name] != null) {
			let space$1 = name.indexOf(" ");
			if (space$1 > 0) dom.setAttributeNS(name.slice(0, space$1), name.slice(space$1 + 1), attrs[name]);
			else if (name == "style" && dom.style) dom.style.cssText = attrs[name];
			else dom.setAttribute(name, attrs[name]);
		}
	}
	for (let i$1 = start; i$1 < structure.length; i$1++) {
		let child = structure[i$1];
		if (child === 0) {
			if (i$1 < structure.length - 1 || i$1 > start) throw new RangeError("Content hole must be the only child of its parent node");
			return {
				dom,
				contentDOM: dom
			};
		} else {
			let { dom: inner, contentDOM: innerContent } = renderSpec(doc$2, child, xmlNS, blockArraysIn);
			dom.appendChild(inner);
			if (innerContent) {
				if (contentDOM) throw new RangeError("Multiple content holes");
				contentDOM = innerContent;
			}
		}
	}
	return {
		dom,
		contentDOM
	};
}
var lower16 = 65535;
var factor16 = Math.pow(2, 16);
function makeRecover(index, offset) {
	return index + offset * factor16;
}
function recoverIndex(value) {
	return value & lower16;
}
function recoverOffset(value) {
	return (value - (value & lower16)) / factor16;
}
var DEL_BEFORE = 1, DEL_AFTER = 2, DEL_ACROSS = 4, DEL_SIDE = 8;
var MapResult = class {
	constructor(pos, delInfo, recover) {
		this.pos = pos;
		this.delInfo = delInfo;
		this.recover = recover;
	}
	get deleted() {
		return (this.delInfo & DEL_SIDE) > 0;
	}
	get deletedBefore() {
		return (this.delInfo & (DEL_BEFORE | DEL_ACROSS)) > 0;
	}
	get deletedAfter() {
		return (this.delInfo & (DEL_AFTER | DEL_ACROSS)) > 0;
	}
	get deletedAcross() {
		return (this.delInfo & DEL_ACROSS) > 0;
	}
};
var StepMap = class StepMap {
	constructor(ranges, inverted = false) {
		this.ranges = ranges;
		this.inverted = inverted;
		if (!ranges.length && StepMap.empty) return StepMap.empty;
	}
	recover(value) {
		let diff = 0, index = recoverIndex(value);
		if (!this.inverted) for (let i$1 = 0; i$1 < index; i$1++) diff += this.ranges[i$1 * 3 + 2] - this.ranges[i$1 * 3 + 1];
		return this.ranges[index * 3] + diff + recoverOffset(value);
	}
	mapResult(pos, assoc = 1) {
		return this._map(pos, assoc, false);
	}
	map(pos, assoc = 1) {
		return this._map(pos, assoc, true);
	}
	_map(pos, assoc, simple) {
		let diff = 0, oldIndex = this.inverted ? 2 : 1, newIndex = this.inverted ? 1 : 2;
		for (let i$1 = 0; i$1 < this.ranges.length; i$1 += 3) {
			let start = this.ranges[i$1] - (this.inverted ? diff : 0);
			if (start > pos) break;
			let oldSize = this.ranges[i$1 + oldIndex], newSize = this.ranges[i$1 + newIndex], end = start + oldSize;
			if (pos <= end) {
				let side = !oldSize ? assoc : pos == start ? -1 : pos == end ? 1 : assoc;
				let result = start + diff + (side < 0 ? 0 : newSize);
				if (simple) return result;
				let recover = pos == (assoc < 0 ? start : end) ? null : makeRecover(i$1 / 3, pos - start);
				let del$1 = pos == start ? DEL_AFTER : pos == end ? DEL_BEFORE : DEL_ACROSS;
				if (assoc < 0 ? pos != start : pos != end) del$1 |= DEL_SIDE;
				return new MapResult(result, del$1, recover);
			}
			diff += newSize - oldSize;
		}
		return simple ? pos + diff : new MapResult(pos + diff, 0, null);
	}
	touches(pos, recover) {
		let diff = 0, index = recoverIndex(recover);
		let oldIndex = this.inverted ? 2 : 1, newIndex = this.inverted ? 1 : 2;
		for (let i$1 = 0; i$1 < this.ranges.length; i$1 += 3) {
			let start = this.ranges[i$1] - (this.inverted ? diff : 0);
			if (start > pos) break;
			let oldSize = this.ranges[i$1 + oldIndex];
			if (pos <= start + oldSize && i$1 == index * 3) return true;
			diff += this.ranges[i$1 + newIndex] - oldSize;
		}
		return false;
	}
	forEach(f) {
		let oldIndex = this.inverted ? 2 : 1, newIndex = this.inverted ? 1 : 2;
		for (let i$1 = 0, diff = 0; i$1 < this.ranges.length; i$1 += 3) {
			let start = this.ranges[i$1], oldStart = start - (this.inverted ? diff : 0), newStart = start + (this.inverted ? 0 : diff);
			let oldSize = this.ranges[i$1 + oldIndex], newSize = this.ranges[i$1 + newIndex];
			f(oldStart, oldStart + oldSize, newStart, newStart + newSize);
			diff += newSize - oldSize;
		}
	}
	invert() {
		return new StepMap(this.ranges, !this.inverted);
	}
	toString() {
		return (this.inverted ? "-" : "") + JSON.stringify(this.ranges);
	}
	static offset(n) {
		return n == 0 ? StepMap.empty : new StepMap(n < 0 ? [
			0,
			-n,
			0
		] : [
			0,
			0,
			n
		]);
	}
};
StepMap.empty = new StepMap([]);
var Mapping = class Mapping {
	constructor(maps, mirror, from = 0, to = maps ? maps.length : 0) {
		this.mirror = mirror;
		this.from = from;
		this.to = to;
		this._maps = maps || [];
		this.ownData = !(maps || mirror);
	}
	get maps() {
		return this._maps;
	}
	slice(from = 0, to = this.maps.length) {
		return new Mapping(this._maps, this.mirror, from, to);
	}
	appendMap(map, mirrors) {
		if (!this.ownData) {
			this._maps = this._maps.slice();
			this.mirror = this.mirror && this.mirror.slice();
			this.ownData = true;
		}
		this.to = this._maps.push(map);
		if (mirrors != null) this.setMirror(this._maps.length - 1, mirrors);
	}
	appendMapping(mapping) {
		for (let i$1 = 0, startSize = this._maps.length; i$1 < mapping._maps.length; i$1++) {
			let mirr = mapping.getMirror(i$1);
			this.appendMap(mapping._maps[i$1], mirr != null && mirr < i$1 ? startSize + mirr : void 0);
		}
	}
	getMirror(n) {
		if (this.mirror) {
			for (let i$1 = 0; i$1 < this.mirror.length; i$1++) if (this.mirror[i$1] == n) return this.mirror[i$1 + (i$1 % 2 ? -1 : 1)];
		}
	}
	setMirror(n, m$1) {
		if (!this.mirror) this.mirror = [];
		this.mirror.push(n, m$1);
	}
	appendMappingInverted(mapping) {
		for (let i$1 = mapping.maps.length - 1, totalSize = this._maps.length + mapping._maps.length; i$1 >= 0; i$1--) {
			let mirr = mapping.getMirror(i$1);
			this.appendMap(mapping._maps[i$1].invert(), mirr != null && mirr > i$1 ? totalSize - mirr - 1 : void 0);
		}
	}
	invert() {
		let inverse = new Mapping();
		inverse.appendMappingInverted(this);
		return inverse;
	}
	map(pos, assoc = 1) {
		if (this.mirror) return this._map(pos, assoc, true);
		for (let i$1 = this.from; i$1 < this.to; i$1++) pos = this._maps[i$1].map(pos, assoc);
		return pos;
	}
	mapResult(pos, assoc = 1) {
		return this._map(pos, assoc, false);
	}
	_map(pos, assoc, simple) {
		let delInfo = 0;
		for (let i$1 = this.from; i$1 < this.to; i$1++) {
			let result = this._maps[i$1].mapResult(pos, assoc);
			if (result.recover != null) {
				let corr = this.getMirror(i$1);
				if (corr != null && corr > i$1 && corr < this.to) {
					i$1 = corr;
					pos = this._maps[corr].recover(result.recover);
					continue;
				}
			}
			delInfo |= result.delInfo;
			pos = result.pos;
		}
		return simple ? pos : new MapResult(pos, delInfo, null);
	}
};
var stepsByID = Object.create(null);
var Step = class {
	getMap() {
		return StepMap.empty;
	}
	merge(other) {
		return null;
	}
	static fromJSON(schema, json) {
		if (!json || !json.stepType) throw new RangeError("Invalid input for Step.fromJSON");
		let type = stepsByID[json.stepType];
		if (!type) throw new RangeError(`No step type ${json.stepType} defined`);
		return type.fromJSON(schema, json);
	}
	static jsonID(id, stepClass) {
		if (id in stepsByID) throw new RangeError("Duplicate use of step JSON ID " + id);
		stepsByID[id] = stepClass;
		stepClass.prototype.jsonID = id;
		return stepClass;
	}
};
var StepResult = class StepResult {
	constructor(doc$2, failed) {
		this.doc = doc$2;
		this.failed = failed;
	}
	static ok(doc$2) {
		return new StepResult(doc$2, null);
	}
	static fail(message) {
		return new StepResult(null, message);
	}
	static fromReplace(doc$2, from, to, slice) {
		try {
			return StepResult.ok(doc$2.replace(from, to, slice));
		} catch (e) {
			if (e instanceof ReplaceError) return StepResult.fail(e.message);
			throw e;
		}
	}
};
function mapFragment(fragment, f, parent) {
	let mapped = [];
	for (let i$1 = 0; i$1 < fragment.childCount; i$1++) {
		let child = fragment.child(i$1);
		if (child.content.size) child = child.copy(mapFragment(child.content, f, child));
		if (child.isInline) child = f(child, parent, i$1);
		mapped.push(child);
	}
	return Fragment.fromArray(mapped);
}
var AddMarkStep = class AddMarkStep extends Step {
	constructor(from, to, mark) {
		super();
		this.from = from;
		this.to = to;
		this.mark = mark;
	}
	apply(doc$2) {
		let oldSlice = doc$2.slice(this.from, this.to), $from = doc$2.resolve(this.from);
		let parent = $from.node($from.sharedDepth(this.to));
		let slice = new Slice(mapFragment(oldSlice.content, (node, parent$1) => {
			if (!node.isAtom || !parent$1.type.allowsMarkType(this.mark.type)) return node;
			return node.mark(this.mark.addToSet(node.marks));
		}, parent), oldSlice.openStart, oldSlice.openEnd);
		return StepResult.fromReplace(doc$2, this.from, this.to, slice);
	}
	invert() {
		return new RemoveMarkStep(this.from, this.to, this.mark);
	}
	map(mapping) {
		let from = mapping.mapResult(this.from, 1), to = mapping.mapResult(this.to, -1);
		if (from.deleted && to.deleted || from.pos >= to.pos) return null;
		return new AddMarkStep(from.pos, to.pos, this.mark);
	}
	merge(other) {
		if (other instanceof AddMarkStep && other.mark.eq(this.mark) && this.from <= other.to && this.to >= other.from) return new AddMarkStep(Math.min(this.from, other.from), Math.max(this.to, other.to), this.mark);
		return null;
	}
	toJSON() {
		return {
			stepType: "addMark",
			mark: this.mark.toJSON(),
			from: this.from,
			to: this.to
		};
	}
	static fromJSON(schema, json) {
		if (typeof json.from != "number" || typeof json.to != "number") throw new RangeError("Invalid input for AddMarkStep.fromJSON");
		return new AddMarkStep(json.from, json.to, schema.markFromJSON(json.mark));
	}
};
Step.jsonID("addMark", AddMarkStep);
var RemoveMarkStep = class RemoveMarkStep extends Step {
	constructor(from, to, mark) {
		super();
		this.from = from;
		this.to = to;
		this.mark = mark;
	}
	apply(doc$2) {
		let oldSlice = doc$2.slice(this.from, this.to);
		let slice = new Slice(mapFragment(oldSlice.content, (node) => {
			return node.mark(this.mark.removeFromSet(node.marks));
		}, doc$2), oldSlice.openStart, oldSlice.openEnd);
		return StepResult.fromReplace(doc$2, this.from, this.to, slice);
	}
	invert() {
		return new AddMarkStep(this.from, this.to, this.mark);
	}
	map(mapping) {
		let from = mapping.mapResult(this.from, 1), to = mapping.mapResult(this.to, -1);
		if (from.deleted && to.deleted || from.pos >= to.pos) return null;
		return new RemoveMarkStep(from.pos, to.pos, this.mark);
	}
	merge(other) {
		if (other instanceof RemoveMarkStep && other.mark.eq(this.mark) && this.from <= other.to && this.to >= other.from) return new RemoveMarkStep(Math.min(this.from, other.from), Math.max(this.to, other.to), this.mark);
		return null;
	}
	toJSON() {
		return {
			stepType: "removeMark",
			mark: this.mark.toJSON(),
			from: this.from,
			to: this.to
		};
	}
	static fromJSON(schema, json) {
		if (typeof json.from != "number" || typeof json.to != "number") throw new RangeError("Invalid input for RemoveMarkStep.fromJSON");
		return new RemoveMarkStep(json.from, json.to, schema.markFromJSON(json.mark));
	}
};
Step.jsonID("removeMark", RemoveMarkStep);
var AddNodeMarkStep = class AddNodeMarkStep extends Step {
	constructor(pos, mark) {
		super();
		this.pos = pos;
		this.mark = mark;
	}
	apply(doc$2) {
		let node = doc$2.nodeAt(this.pos);
		if (!node) return StepResult.fail("No node at mark step's position");
		let updated = node.type.create(node.attrs, null, this.mark.addToSet(node.marks));
		return StepResult.fromReplace(doc$2, this.pos, this.pos + 1, new Slice(Fragment.from(updated), 0, node.isLeaf ? 0 : 1));
	}
	invert(doc$2) {
		let node = doc$2.nodeAt(this.pos);
		if (node) {
			let newSet = this.mark.addToSet(node.marks);
			if (newSet.length == node.marks.length) {
				for (let i$1 = 0; i$1 < node.marks.length; i$1++) if (!node.marks[i$1].isInSet(newSet)) return new AddNodeMarkStep(this.pos, node.marks[i$1]);
				return new AddNodeMarkStep(this.pos, this.mark);
			}
		}
		return new RemoveNodeMarkStep(this.pos, this.mark);
	}
	map(mapping) {
		let pos = mapping.mapResult(this.pos, 1);
		return pos.deletedAfter ? null : new AddNodeMarkStep(pos.pos, this.mark);
	}
	toJSON() {
		return {
			stepType: "addNodeMark",
			pos: this.pos,
			mark: this.mark.toJSON()
		};
	}
	static fromJSON(schema, json) {
		if (typeof json.pos != "number") throw new RangeError("Invalid input for AddNodeMarkStep.fromJSON");
		return new AddNodeMarkStep(json.pos, schema.markFromJSON(json.mark));
	}
};
Step.jsonID("addNodeMark", AddNodeMarkStep);
var RemoveNodeMarkStep = class RemoveNodeMarkStep extends Step {
	constructor(pos, mark) {
		super();
		this.pos = pos;
		this.mark = mark;
	}
	apply(doc$2) {
		let node = doc$2.nodeAt(this.pos);
		if (!node) return StepResult.fail("No node at mark step's position");
		let updated = node.type.create(node.attrs, null, this.mark.removeFromSet(node.marks));
		return StepResult.fromReplace(doc$2, this.pos, this.pos + 1, new Slice(Fragment.from(updated), 0, node.isLeaf ? 0 : 1));
	}
	invert(doc$2) {
		let node = doc$2.nodeAt(this.pos);
		if (!node || !this.mark.isInSet(node.marks)) return this;
		return new AddNodeMarkStep(this.pos, this.mark);
	}
	map(mapping) {
		let pos = mapping.mapResult(this.pos, 1);
		return pos.deletedAfter ? null : new RemoveNodeMarkStep(pos.pos, this.mark);
	}
	toJSON() {
		return {
			stepType: "removeNodeMark",
			pos: this.pos,
			mark: this.mark.toJSON()
		};
	}
	static fromJSON(schema, json) {
		if (typeof json.pos != "number") throw new RangeError("Invalid input for RemoveNodeMarkStep.fromJSON");
		return new RemoveNodeMarkStep(json.pos, schema.markFromJSON(json.mark));
	}
};
Step.jsonID("removeNodeMark", RemoveNodeMarkStep);
var ReplaceStep = class ReplaceStep extends Step {
	constructor(from, to, slice, structure = false) {
		super();
		this.from = from;
		this.to = to;
		this.slice = slice;
		this.structure = structure;
	}
	apply(doc$2) {
		if (this.structure && contentBetween(doc$2, this.from, this.to)) return StepResult.fail("Structure replace would overwrite content");
		return StepResult.fromReplace(doc$2, this.from, this.to, this.slice);
	}
	getMap() {
		return new StepMap([
			this.from,
			this.to - this.from,
			this.slice.size
		]);
	}
	invert(doc$2) {
		return new ReplaceStep(this.from, this.from + this.slice.size, doc$2.slice(this.from, this.to));
	}
	map(mapping) {
		let to = mapping.mapResult(this.to, -1);
		let from = this.from == this.to && ReplaceStep.MAP_BIAS < 0 ? to : mapping.mapResult(this.from, 1);
		if (from.deletedAcross && to.deletedAcross) return null;
		return new ReplaceStep(from.pos, Math.max(from.pos, to.pos), this.slice, this.structure);
	}
	merge(other) {
		if (!(other instanceof ReplaceStep) || other.structure || this.structure) return null;
		if (this.from + this.slice.size == other.from && !this.slice.openEnd && !other.slice.openStart) {
			let slice = this.slice.size + other.slice.size == 0 ? Slice.empty : new Slice(this.slice.content.append(other.slice.content), this.slice.openStart, other.slice.openEnd);
			return new ReplaceStep(this.from, this.to + (other.to - other.from), slice, this.structure);
		} else if (other.to == this.from && !this.slice.openStart && !other.slice.openEnd) {
			let slice = this.slice.size + other.slice.size == 0 ? Slice.empty : new Slice(other.slice.content.append(this.slice.content), other.slice.openStart, this.slice.openEnd);
			return new ReplaceStep(other.from, this.to, slice, this.structure);
		} else return null;
	}
	toJSON() {
		let json = {
			stepType: "replace",
			from: this.from,
			to: this.to
		};
		if (this.slice.size) json.slice = this.slice.toJSON();
		if (this.structure) json.structure = true;
		return json;
	}
	static fromJSON(schema, json) {
		if (typeof json.from != "number" || typeof json.to != "number") throw new RangeError("Invalid input for ReplaceStep.fromJSON");
		return new ReplaceStep(json.from, json.to, Slice.fromJSON(schema, json.slice), !!json.structure);
	}
};
ReplaceStep.MAP_BIAS = 1;
Step.jsonID("replace", ReplaceStep);
var ReplaceAroundStep = class ReplaceAroundStep extends Step {
	constructor(from, to, gapFrom, gapTo, slice, insert, structure = false) {
		super();
		this.from = from;
		this.to = to;
		this.gapFrom = gapFrom;
		this.gapTo = gapTo;
		this.slice = slice;
		this.insert = insert;
		this.structure = structure;
	}
	apply(doc$2) {
		if (this.structure && (contentBetween(doc$2, this.from, this.gapFrom) || contentBetween(doc$2, this.gapTo, this.to))) return StepResult.fail("Structure gap-replace would overwrite content");
		let gap = doc$2.slice(this.gapFrom, this.gapTo);
		if (gap.openStart || gap.openEnd) return StepResult.fail("Gap is not a flat range");
		let inserted = this.slice.insertAt(this.insert, gap.content);
		if (!inserted) return StepResult.fail("Content does not fit in gap");
		return StepResult.fromReplace(doc$2, this.from, this.to, inserted);
	}
	getMap() {
		return new StepMap([
			this.from,
			this.gapFrom - this.from,
			this.insert,
			this.gapTo,
			this.to - this.gapTo,
			this.slice.size - this.insert
		]);
	}
	invert(doc$2) {
		let gap = this.gapTo - this.gapFrom;
		return new ReplaceAroundStep(this.from, this.from + this.slice.size + gap, this.from + this.insert, this.from + this.insert + gap, doc$2.slice(this.from, this.to).removeBetween(this.gapFrom - this.from, this.gapTo - this.from), this.gapFrom - this.from, this.structure);
	}
	map(mapping) {
		let from = mapping.mapResult(this.from, 1), to = mapping.mapResult(this.to, -1);
		let gapFrom = this.from == this.gapFrom ? from.pos : mapping.map(this.gapFrom, -1);
		let gapTo = this.to == this.gapTo ? to.pos : mapping.map(this.gapTo, 1);
		if (from.deletedAcross && to.deletedAcross || gapFrom < from.pos || gapTo > to.pos) return null;
		return new ReplaceAroundStep(from.pos, to.pos, gapFrom, gapTo, this.slice, this.insert, this.structure);
	}
	toJSON() {
		let json = {
			stepType: "replaceAround",
			from: this.from,
			to: this.to,
			gapFrom: this.gapFrom,
			gapTo: this.gapTo,
			insert: this.insert
		};
		if (this.slice.size) json.slice = this.slice.toJSON();
		if (this.structure) json.structure = true;
		return json;
	}
	static fromJSON(schema, json) {
		if (typeof json.from != "number" || typeof json.to != "number" || typeof json.gapFrom != "number" || typeof json.gapTo != "number" || typeof json.insert != "number") throw new RangeError("Invalid input for ReplaceAroundStep.fromJSON");
		return new ReplaceAroundStep(json.from, json.to, json.gapFrom, json.gapTo, Slice.fromJSON(schema, json.slice), json.insert, !!json.structure);
	}
};
Step.jsonID("replaceAround", ReplaceAroundStep);
function contentBetween(doc$2, from, to) {
	let $from = doc$2.resolve(from), dist = to - from, depth = $from.depth;
	while (dist > 0 && depth > 0 && $from.indexAfter(depth) == $from.node(depth).childCount) {
		depth--;
		dist--;
	}
	if (dist > 0) {
		let next = $from.node(depth).maybeChild($from.indexAfter(depth));
		while (dist > 0) {
			if (!next || next.isLeaf) return true;
			next = next.firstChild;
			dist--;
		}
	}
	return false;
}
function addMark(tr$1, from, to, mark) {
	let removed = [], added = [];
	let removing, adding;
	tr$1.doc.nodesBetween(from, to, (node, pos, parent) => {
		if (!node.isInline) return;
		let marks = node.marks;
		if (!mark.isInSet(marks) && parent.type.allowsMarkType(mark.type)) {
			let start = Math.max(pos, from), end = Math.min(pos + node.nodeSize, to);
			let newSet = mark.addToSet(marks);
			for (let i$1 = 0; i$1 < marks.length; i$1++) if (!marks[i$1].isInSet(newSet)) if (removing && removing.to == start && removing.mark.eq(marks[i$1])) removing.to = end;
			else removed.push(removing = new RemoveMarkStep(start, end, marks[i$1]));
			if (adding && adding.to == start) adding.to = end;
			else added.push(adding = new AddMarkStep(start, end, mark));
		}
	});
	removed.forEach((s) => tr$1.step(s));
	added.forEach((s) => tr$1.step(s));
}
function removeMark(tr$1, from, to, mark) {
	let matched = [], step$1 = 0;
	tr$1.doc.nodesBetween(from, to, (node, pos) => {
		if (!node.isInline) return;
		step$1++;
		let toRemove = null;
		if (mark instanceof MarkType) {
			let set = node.marks, found$1;
			while (found$1 = mark.isInSet(set)) {
				(toRemove || (toRemove = [])).push(found$1);
				set = found$1.removeFromSet(set);
			}
		} else if (mark) {
			if (mark.isInSet(node.marks)) toRemove = [mark];
		} else toRemove = node.marks;
		if (toRemove && toRemove.length) {
			let end = Math.min(pos + node.nodeSize, to);
			for (let i$1 = 0; i$1 < toRemove.length; i$1++) {
				let style$1 = toRemove[i$1], found$1;
				for (let j$1 = 0; j$1 < matched.length; j$1++) {
					let m$1 = matched[j$1];
					if (m$1.step == step$1 - 1 && style$1.eq(matched[j$1].style)) found$1 = m$1;
				}
				if (found$1) {
					found$1.to = end;
					found$1.step = step$1;
				} else matched.push({
					style: style$1,
					from: Math.max(pos, from),
					to: end,
					step: step$1
				});
			}
		}
	});
	matched.forEach((m$1) => tr$1.step(new RemoveMarkStep(m$1.from, m$1.to, m$1.style)));
}
function clearIncompatible(tr$1, pos, parentType, match = parentType.contentMatch, clearNewlines = true) {
	let node = tr$1.doc.nodeAt(pos);
	let replSteps = [], cur = pos + 1;
	for (let i$1 = 0; i$1 < node.childCount; i$1++) {
		let child = node.child(i$1), end = cur + child.nodeSize;
		let allowed = match.matchType(child.type);
		if (!allowed) replSteps.push(new ReplaceStep(cur, end, Slice.empty));
		else {
			match = allowed;
			for (let j$1 = 0; j$1 < child.marks.length; j$1++) if (!parentType.allowsMarkType(child.marks[j$1].type)) tr$1.step(new RemoveMarkStep(cur, end, child.marks[j$1]));
			if (clearNewlines && child.isText && parentType.whitespace != "pre") {
				let m$1, newline = /\r?\n|\r/g, slice;
				while (m$1 = newline.exec(child.text)) {
					if (!slice) slice = new Slice(Fragment.from(parentType.schema.text(" ", parentType.allowedMarks(child.marks))), 0, 0);
					replSteps.push(new ReplaceStep(cur + m$1.index, cur + m$1.index + m$1[0].length, slice));
				}
			}
		}
		cur = end;
	}
	if (!match.validEnd) {
		let fill = match.fillBefore(Fragment.empty, true);
		tr$1.replace(cur, cur, new Slice(fill, 0, 0));
	}
	for (let i$1 = replSteps.length - 1; i$1 >= 0; i$1--) tr$1.step(replSteps[i$1]);
}
function canCut(node, start, end) {
	return (start == 0 || node.canReplace(start, node.childCount)) && (end == node.childCount || node.canReplace(0, end));
}
function liftTarget(range) {
	let content = range.parent.content.cutByIndex(range.startIndex, range.endIndex);
	for (let depth = range.depth, contentBefore = 0, contentAfter = 0;; --depth) {
		let node = range.$from.node(depth);
		let index = range.$from.index(depth) + contentBefore, endIndex = range.$to.indexAfter(depth) - contentAfter;
		if (depth < range.depth && node.canReplace(index, endIndex, content)) return depth;
		if (depth == 0 || node.type.spec.isolating || !canCut(node, index, endIndex)) break;
		if (index) contentBefore = 1;
		if (endIndex < node.childCount) contentAfter = 1;
	}
	return null;
}
function lift$2(tr$1, range, target) {
	let { $from, $to, depth } = range;
	let gapStart = $from.before(depth + 1), gapEnd = $to.after(depth + 1);
	let start = gapStart, end = gapEnd;
	let before = Fragment.empty, openStart = 0;
	for (let d = depth, splitting = false; d > target; d--) if (splitting || $from.index(d) > 0) {
		splitting = true;
		before = Fragment.from($from.node(d).copy(before));
		openStart++;
	} else start--;
	let after = Fragment.empty, openEnd = 0;
	for (let d = depth, splitting = false; d > target; d--) if (splitting || $to.after(d + 1) < $to.end(d)) {
		splitting = true;
		after = Fragment.from($to.node(d).copy(after));
		openEnd++;
	} else end++;
	tr$1.step(new ReplaceAroundStep(start, end, gapStart, gapEnd, new Slice(before.append(after), openStart, openEnd), before.size - openStart, true));
}
function findWrapping(range, nodeType, attrs = null, innerRange = range) {
	let around = findWrappingOutside(range, nodeType);
	let inner = around && findWrappingInside(innerRange, nodeType);
	if (!inner) return null;
	return around.map(withAttrs).concat({
		type: nodeType,
		attrs
	}).concat(inner.map(withAttrs));
}
function withAttrs(type) {
	return {
		type,
		attrs: null
	};
}
function findWrappingOutside(range, type) {
	let { parent, startIndex, endIndex } = range;
	let around = parent.contentMatchAt(startIndex).findWrapping(type);
	if (!around) return null;
	let outer = around.length ? around[0] : type;
	return parent.canReplaceWith(startIndex, endIndex, outer) ? around : null;
}
function findWrappingInside(range, type) {
	let { parent, startIndex, endIndex } = range;
	let inner = parent.child(startIndex);
	let inside = type.contentMatch.findWrapping(inner.type);
	if (!inside) return null;
	let innerMatch = (inside.length ? inside[inside.length - 1] : type).contentMatch;
	for (let i$1 = startIndex; innerMatch && i$1 < endIndex; i$1++) innerMatch = innerMatch.matchType(parent.child(i$1).type);
	if (!innerMatch || !innerMatch.validEnd) return null;
	return inside;
}
function wrap(tr$1, range, wrappers) {
	let content = Fragment.empty;
	for (let i$1 = wrappers.length - 1; i$1 >= 0; i$1--) {
		if (content.size) {
			let match = wrappers[i$1].type.contentMatch.matchFragment(content);
			if (!match || !match.validEnd) throw new RangeError("Wrapper type given to Transform.wrap does not form valid content of its parent wrapper");
		}
		content = Fragment.from(wrappers[i$1].type.create(wrappers[i$1].attrs, content));
	}
	let start = range.start, end = range.end;
	tr$1.step(new ReplaceAroundStep(start, end, start, end, new Slice(content, 0, 0), wrappers.length, true));
}
function setBlockType$1(tr$1, from, to, type, attrs) {
	if (!type.isTextblock) throw new RangeError("Type given to setBlockType should be a textblock");
	let mapFrom = tr$1.steps.length;
	tr$1.doc.nodesBetween(from, to, (node, pos) => {
		let attrsHere = typeof attrs == "function" ? attrs(node) : attrs;
		if (node.isTextblock && !node.hasMarkup(type, attrsHere) && canChangeType(tr$1.doc, tr$1.mapping.slice(mapFrom).map(pos), type)) {
			let convertNewlines = null;
			if (type.schema.linebreakReplacement) {
				let pre = type.whitespace == "pre", supportLinebreak = !!type.contentMatch.matchType(type.schema.linebreakReplacement);
				if (pre && !supportLinebreak) convertNewlines = false;
				else if (!pre && supportLinebreak) convertNewlines = true;
			}
			if (convertNewlines === false) replaceLinebreaks(tr$1, node, pos, mapFrom);
			clearIncompatible(tr$1, tr$1.mapping.slice(mapFrom).map(pos, 1), type, void 0, convertNewlines === null);
			let mapping = tr$1.mapping.slice(mapFrom);
			let startM = mapping.map(pos, 1), endM = mapping.map(pos + node.nodeSize, 1);
			tr$1.step(new ReplaceAroundStep(startM, endM, startM + 1, endM - 1, new Slice(Fragment.from(type.create(attrsHere, null, node.marks)), 0, 0), 1, true));
			if (convertNewlines === true) replaceNewlines(tr$1, node, pos, mapFrom);
			return false;
		}
	});
}
function replaceNewlines(tr$1, node, pos, mapFrom) {
	node.forEach((child, offset) => {
		if (child.isText) {
			let m$1, newline = /\r?\n|\r/g;
			while (m$1 = newline.exec(child.text)) {
				let start = tr$1.mapping.slice(mapFrom).map(pos + 1 + offset + m$1.index);
				tr$1.replaceWith(start, start + 1, node.type.schema.linebreakReplacement.create());
			}
		}
	});
}
function replaceLinebreaks(tr$1, node, pos, mapFrom) {
	node.forEach((child, offset) => {
		if (child.type == child.type.schema.linebreakReplacement) {
			let start = tr$1.mapping.slice(mapFrom).map(pos + 1 + offset);
			tr$1.replaceWith(start, start + 1, node.type.schema.text("\n"));
		}
	});
}
function canChangeType(doc$2, pos, type) {
	let $pos = doc$2.resolve(pos), index = $pos.index();
	return $pos.parent.canReplaceWith(index, index + 1, type);
}
function setNodeMarkup(tr$1, pos, type, attrs, marks) {
	let node = tr$1.doc.nodeAt(pos);
	if (!node) throw new RangeError("No node at given position");
	if (!type) type = node.type;
	let newNode = type.create(attrs, null, marks || node.marks);
	if (node.isLeaf) return tr$1.replaceWith(pos, pos + node.nodeSize, newNode);
	if (!type.validContent(node.content)) throw new RangeError("Invalid content for node type " + type.name);
	tr$1.step(new ReplaceAroundStep(pos, pos + node.nodeSize, pos + 1, pos + node.nodeSize - 1, new Slice(Fragment.from(newNode), 0, 0), 1, true));
}
function canSplit(doc$2, pos, depth = 1, typesAfter) {
	let $pos = doc$2.resolve(pos), base$1 = $pos.depth - depth;
	let innerType = typesAfter && typesAfter[typesAfter.length - 1] || $pos.parent;
	if (base$1 < 0 || $pos.parent.type.spec.isolating || !$pos.parent.canReplace($pos.index(), $pos.parent.childCount) || !innerType.type.validContent($pos.parent.content.cutByIndex($pos.index(), $pos.parent.childCount))) return false;
	for (let d = $pos.depth - 1, i$1 = depth - 2; d > base$1; d--, i$1--) {
		let node = $pos.node(d), index$1 = $pos.index(d);
		if (node.type.spec.isolating) return false;
		let rest = node.content.cutByIndex(index$1, node.childCount);
		let overrideChild = typesAfter && typesAfter[i$1 + 1];
		if (overrideChild) rest = rest.replaceChild(0, overrideChild.type.create(overrideChild.attrs));
		let after = typesAfter && typesAfter[i$1] || node;
		if (!node.canReplace(index$1 + 1, node.childCount) || !after.type.validContent(rest)) return false;
	}
	let index = $pos.indexAfter(base$1);
	let baseType = typesAfter && typesAfter[0];
	return $pos.node(base$1).canReplaceWith(index, index, baseType ? baseType.type : $pos.node(base$1 + 1).type);
}
function split(tr$1, pos, depth = 1, typesAfter) {
	let $pos = tr$1.doc.resolve(pos), before = Fragment.empty, after = Fragment.empty;
	for (let d = $pos.depth, e = $pos.depth - depth, i$1 = depth - 1; d > e; d--, i$1--) {
		before = Fragment.from($pos.node(d).copy(before));
		let typeAfter = typesAfter && typesAfter[i$1];
		after = Fragment.from(typeAfter ? typeAfter.type.create(typeAfter.attrs, after) : $pos.node(d).copy(after));
	}
	tr$1.step(new ReplaceStep(pos, pos, new Slice(before.append(after), depth, depth), true));
}
function canJoin(doc$2, pos) {
	let $pos = doc$2.resolve(pos), index = $pos.index();
	return joinable($pos.nodeBefore, $pos.nodeAfter) && $pos.parent.canReplace(index, index + 1);
}
function canAppendWithSubstitutedLinebreaks(a, b$1) {
	if (!b$1.content.size) a.type.compatibleContent(b$1.type);
	let match = a.contentMatchAt(a.childCount);
	let { linebreakReplacement } = a.type.schema;
	for (let i$1 = 0; i$1 < b$1.childCount; i$1++) {
		let child = b$1.child(i$1);
		let type = child.type == linebreakReplacement ? a.type.schema.nodes.text : child.type;
		match = match.matchType(type);
		if (!match) return false;
		if (!a.type.allowsMarks(child.marks)) return false;
	}
	return match.validEnd;
}
function joinable(a, b$1) {
	return !!(a && b$1 && !a.isLeaf && canAppendWithSubstitutedLinebreaks(a, b$1));
}
function joinPoint(doc$2, pos, dir = -1) {
	let $pos = doc$2.resolve(pos);
	for (let d = $pos.depth;; d--) {
		let before, after, index = $pos.index(d);
		if (d == $pos.depth) {
			before = $pos.nodeBefore;
			after = $pos.nodeAfter;
		} else if (dir > 0) {
			before = $pos.node(d + 1);
			index++;
			after = $pos.node(d).maybeChild(index);
		} else {
			before = $pos.node(d).maybeChild(index - 1);
			after = $pos.node(d + 1);
		}
		if (before && !before.isTextblock && joinable(before, after) && $pos.node(d).canReplace(index, index + 1)) return pos;
		if (d == 0) break;
		pos = dir < 0 ? $pos.before(d) : $pos.after(d);
	}
}
function join(tr$1, pos, depth) {
	let convertNewlines = null;
	let { linebreakReplacement } = tr$1.doc.type.schema;
	let $before = tr$1.doc.resolve(pos - depth), beforeType = $before.node().type;
	if (linebreakReplacement && beforeType.inlineContent) {
		let pre = beforeType.whitespace == "pre";
		let supportLinebreak = !!beforeType.contentMatch.matchType(linebreakReplacement);
		if (pre && !supportLinebreak) convertNewlines = false;
		else if (!pre && supportLinebreak) convertNewlines = true;
	}
	let mapFrom = tr$1.steps.length;
	if (convertNewlines === false) {
		let $after = tr$1.doc.resolve(pos + depth);
		replaceLinebreaks(tr$1, $after.node(), $after.before(), mapFrom);
	}
	if (beforeType.inlineContent) clearIncompatible(tr$1, pos + depth - 1, beforeType, $before.node().contentMatchAt($before.index()), convertNewlines == null);
	let mapping = tr$1.mapping.slice(mapFrom), start = mapping.map(pos - depth);
	tr$1.step(new ReplaceStep(start, mapping.map(pos + depth, -1), Slice.empty, true));
	if (convertNewlines === true) {
		let $full = tr$1.doc.resolve(start);
		replaceNewlines(tr$1, $full.node(), $full.before(), tr$1.steps.length);
	}
	return tr$1;
}
function insertPoint(doc$2, pos, nodeType) {
	let $pos = doc$2.resolve(pos);
	if ($pos.parent.canReplaceWith($pos.index(), $pos.index(), nodeType)) return pos;
	if ($pos.parentOffset == 0) for (let d = $pos.depth - 1; d >= 0; d--) {
		let index = $pos.index(d);
		if ($pos.node(d).canReplaceWith(index, index, nodeType)) return $pos.before(d + 1);
		if (index > 0) return null;
	}
	if ($pos.parentOffset == $pos.parent.content.size) for (let d = $pos.depth - 1; d >= 0; d--) {
		let index = $pos.indexAfter(d);
		if ($pos.node(d).canReplaceWith(index, index, nodeType)) return $pos.after(d + 1);
		if (index < $pos.node(d).childCount) return null;
	}
	return null;
}
function dropPoint(doc$2, pos, slice) {
	let $pos = doc$2.resolve(pos);
	if (!slice.content.size) return pos;
	let content = slice.content;
	for (let i$1 = 0; i$1 < slice.openStart; i$1++) content = content.firstChild.content;
	for (let pass = 1; pass <= (slice.openStart == 0 && slice.size ? 2 : 1); pass++) for (let d = $pos.depth; d >= 0; d--) {
		let bias = d == $pos.depth ? 0 : $pos.pos <= ($pos.start(d + 1) + $pos.end(d + 1)) / 2 ? -1 : 1;
		let insertPos = $pos.index(d) + (bias > 0 ? 1 : 0);
		let parent = $pos.node(d), fits = false;
		if (pass == 1) fits = parent.canReplace(insertPos, insertPos, content);
		else {
			let wrapping = parent.contentMatchAt(insertPos).findWrapping(content.firstChild.type);
			fits = wrapping && parent.canReplaceWith(insertPos, insertPos, wrapping[0]);
		}
		if (fits) return bias == 0 ? $pos.pos : bias < 0 ? $pos.before(d + 1) : $pos.after(d + 1);
	}
	return null;
}
function replaceStep(doc$2, from, to = from, slice = Slice.empty) {
	if (from == to && !slice.size) return null;
	let $from = doc$2.resolve(from), $to = doc$2.resolve(to);
	if (fitsTrivially($from, $to, slice)) return new ReplaceStep(from, to, slice);
	return new Fitter($from, $to, slice).fit();
}
function fitsTrivially($from, $to, slice) {
	return !slice.openStart && !slice.openEnd && $from.start() == $to.start() && $from.parent.canReplace($from.index(), $to.index(), slice.content);
}
var Fitter = class {
	constructor($from, $to, unplaced) {
		this.$from = $from;
		this.$to = $to;
		this.unplaced = unplaced;
		this.frontier = [];
		this.placed = Fragment.empty;
		for (let i$1 = 0; i$1 <= $from.depth; i$1++) {
			let node = $from.node(i$1);
			this.frontier.push({
				type: node.type,
				match: node.contentMatchAt($from.indexAfter(i$1))
			});
		}
		for (let i$1 = $from.depth; i$1 > 0; i$1--) this.placed = Fragment.from($from.node(i$1).copy(this.placed));
	}
	get depth() {
		return this.frontier.length - 1;
	}
	fit() {
		while (this.unplaced.size) {
			let fit = this.findFittable();
			if (fit) this.placeNodes(fit);
			else this.openMore() || this.dropNode();
		}
		let moveInline = this.mustMoveInline(), placedSize = this.placed.size - this.depth - this.$from.depth;
		let $from = this.$from, $to = this.close(moveInline < 0 ? this.$to : $from.doc.resolve(moveInline));
		if (!$to) return null;
		let content = this.placed, openStart = $from.depth, openEnd = $to.depth;
		while (openStart && openEnd && content.childCount == 1) {
			content = content.firstChild.content;
			openStart--;
			openEnd--;
		}
		let slice = new Slice(content, openStart, openEnd);
		if (moveInline > -1) return new ReplaceAroundStep($from.pos, moveInline, this.$to.pos, this.$to.end(), slice, placedSize);
		if (slice.size || $from.pos != this.$to.pos) return new ReplaceStep($from.pos, $to.pos, slice);
		return null;
	}
	findFittable() {
		let startDepth = this.unplaced.openStart;
		for (let cur = this.unplaced.content, d = 0, openEnd = this.unplaced.openEnd; d < startDepth; d++) {
			let node = cur.firstChild;
			if (cur.childCount > 1) openEnd = 0;
			if (node.type.spec.isolating && openEnd <= d) {
				startDepth = d;
				break;
			}
			cur = node.content;
		}
		for (let pass = 1; pass <= 2; pass++) for (let sliceDepth = pass == 1 ? startDepth : this.unplaced.openStart; sliceDepth >= 0; sliceDepth--) {
			let fragment, parent = null;
			if (sliceDepth) {
				parent = contentAt(this.unplaced.content, sliceDepth - 1).firstChild;
				fragment = parent.content;
			} else fragment = this.unplaced.content;
			let first$1 = fragment.firstChild;
			for (let frontierDepth = this.depth; frontierDepth >= 0; frontierDepth--) {
				let { type, match } = this.frontier[frontierDepth], wrap$1, inject = null;
				if (pass == 1 && (first$1 ? match.matchType(first$1.type) || (inject = match.fillBefore(Fragment.from(first$1), false)) : parent && type.compatibleContent(parent.type))) return {
					sliceDepth,
					frontierDepth,
					parent,
					inject
				};
				else if (pass == 2 && first$1 && (wrap$1 = match.findWrapping(first$1.type))) return {
					sliceDepth,
					frontierDepth,
					parent,
					wrap: wrap$1
				};
				if (parent && match.matchType(parent.type)) break;
			}
		}
	}
	openMore() {
		let { content, openStart, openEnd } = this.unplaced;
		let inner = contentAt(content, openStart);
		if (!inner.childCount || inner.firstChild.isLeaf) return false;
		this.unplaced = new Slice(content, openStart + 1, Math.max(openEnd, inner.size + openStart >= content.size - openEnd ? openStart + 1 : 0));
		return true;
	}
	dropNode() {
		let { content, openStart, openEnd } = this.unplaced;
		let inner = contentAt(content, openStart);
		if (inner.childCount <= 1 && openStart > 0) {
			let openAtEnd = content.size - openStart <= openStart + inner.size;
			this.unplaced = new Slice(dropFromFragment(content, openStart - 1, 1), openStart - 1, openAtEnd ? openStart - 1 : openEnd);
		} else this.unplaced = new Slice(dropFromFragment(content, openStart, 1), openStart, openEnd);
	}
	placeNodes({ sliceDepth, frontierDepth, parent, inject, wrap: wrap$1 }) {
		while (this.depth > frontierDepth) this.closeFrontierNode();
		if (wrap$1) for (let i$1 = 0; i$1 < wrap$1.length; i$1++) this.openFrontierNode(wrap$1[i$1]);
		let slice = this.unplaced, fragment = parent ? parent.content : slice.content;
		let openStart = slice.openStart - sliceDepth;
		let taken = 0, add = [];
		let { match, type } = this.frontier[frontierDepth];
		if (inject) {
			for (let i$1 = 0; i$1 < inject.childCount; i$1++) add.push(inject.child(i$1));
			match = match.matchFragment(inject);
		}
		let openEndCount = fragment.size + sliceDepth - (slice.content.size - slice.openEnd);
		while (taken < fragment.childCount) {
			let next = fragment.child(taken), matches$1 = match.matchType(next.type);
			if (!matches$1) break;
			taken++;
			if (taken > 1 || openStart == 0 || next.content.size) {
				match = matches$1;
				add.push(closeNodeStart(next.mark(type.allowedMarks(next.marks)), taken == 1 ? openStart : 0, taken == fragment.childCount ? openEndCount : -1));
			}
		}
		let toEnd = taken == fragment.childCount;
		if (!toEnd) openEndCount = -1;
		this.placed = addToFragment(this.placed, frontierDepth, Fragment.from(add));
		this.frontier[frontierDepth].match = match;
		if (toEnd && openEndCount < 0 && parent && parent.type == this.frontier[this.depth].type && this.frontier.length > 1) this.closeFrontierNode();
		for (let i$1 = 0, cur = fragment; i$1 < openEndCount; i$1++) {
			let node = cur.lastChild;
			this.frontier.push({
				type: node.type,
				match: node.contentMatchAt(node.childCount)
			});
			cur = node.content;
		}
		this.unplaced = !toEnd ? new Slice(dropFromFragment(slice.content, sliceDepth, taken), slice.openStart, slice.openEnd) : sliceDepth == 0 ? Slice.empty : new Slice(dropFromFragment(slice.content, sliceDepth - 1, 1), sliceDepth - 1, openEndCount < 0 ? slice.openEnd : sliceDepth - 1);
	}
	mustMoveInline() {
		if (!this.$to.parent.isTextblock) return -1;
		let top = this.frontier[this.depth], level;
		if (!top.type.isTextblock || !contentAfterFits(this.$to, this.$to.depth, top.type, top.match, false) || this.$to.depth == this.depth && (level = this.findCloseLevel(this.$to)) && level.depth == this.depth) return -1;
		let { depth } = this.$to, after = this.$to.after(depth);
		while (depth > 1 && after == this.$to.end(--depth)) ++after;
		return after;
	}
	findCloseLevel($to) {
		scan: for (let i$1 = Math.min(this.depth, $to.depth); i$1 >= 0; i$1--) {
			let { match, type } = this.frontier[i$1];
			let dropInner = i$1 < $to.depth && $to.end(i$1 + 1) == $to.pos + ($to.depth - (i$1 + 1));
			let fit = contentAfterFits($to, i$1, type, match, dropInner);
			if (!fit) continue;
			for (let d = i$1 - 1; d >= 0; d--) {
				let { match: match$1, type: type$1 } = this.frontier[d];
				let matches$1 = contentAfterFits($to, d, type$1, match$1, true);
				if (!matches$1 || matches$1.childCount) continue scan;
			}
			return {
				depth: i$1,
				fit,
				move: dropInner ? $to.doc.resolve($to.after(i$1 + 1)) : $to
			};
		}
	}
	close($to) {
		let close$1 = this.findCloseLevel($to);
		if (!close$1) return null;
		while (this.depth > close$1.depth) this.closeFrontierNode();
		if (close$1.fit.childCount) this.placed = addToFragment(this.placed, close$1.depth, close$1.fit);
		$to = close$1.move;
		for (let d = close$1.depth + 1; d <= $to.depth; d++) {
			let node = $to.node(d), add = node.type.contentMatch.fillBefore(node.content, true, $to.index(d));
			this.openFrontierNode(node.type, node.attrs, add);
		}
		return $to;
	}
	openFrontierNode(type, attrs = null, content) {
		let top = this.frontier[this.depth];
		top.match = top.match.matchType(type);
		this.placed = addToFragment(this.placed, this.depth, Fragment.from(type.create(attrs, content)));
		this.frontier.push({
			type,
			match: type.contentMatch
		});
	}
	closeFrontierNode() {
		let add = this.frontier.pop().match.fillBefore(Fragment.empty, true);
		if (add.childCount) this.placed = addToFragment(this.placed, this.frontier.length, add);
	}
};
function dropFromFragment(fragment, depth, count) {
	if (depth == 0) return fragment.cutByIndex(count, fragment.childCount);
	return fragment.replaceChild(0, fragment.firstChild.copy(dropFromFragment(fragment.firstChild.content, depth - 1, count)));
}
function addToFragment(fragment, depth, content) {
	if (depth == 0) return fragment.append(content);
	return fragment.replaceChild(fragment.childCount - 1, fragment.lastChild.copy(addToFragment(fragment.lastChild.content, depth - 1, content)));
}
function contentAt(fragment, depth) {
	for (let i$1 = 0; i$1 < depth; i$1++) fragment = fragment.firstChild.content;
	return fragment;
}
function closeNodeStart(node, openStart, openEnd) {
	if (openStart <= 0) return node;
	let frag = node.content;
	if (openStart > 1) frag = frag.replaceChild(0, closeNodeStart(frag.firstChild, openStart - 1, frag.childCount == 1 ? openEnd - 1 : 0));
	if (openStart > 0) {
		frag = node.type.contentMatch.fillBefore(frag).append(frag);
		if (openEnd <= 0) frag = frag.append(node.type.contentMatch.matchFragment(frag).fillBefore(Fragment.empty, true));
	}
	return node.copy(frag);
}
function contentAfterFits($to, depth, type, match, open) {
	let node = $to.node(depth), index = open ? $to.indexAfter(depth) : $to.index(depth);
	if (index == node.childCount && !type.compatibleContent(node.type)) return null;
	let fit = match.fillBefore(node.content, true, index);
	return fit && !invalidMarks(type, node.content, index) ? fit : null;
}
function invalidMarks(type, fragment, start) {
	for (let i$1 = start; i$1 < fragment.childCount; i$1++) if (!type.allowsMarks(fragment.child(i$1).marks)) return true;
	return false;
}
function definesContent(type) {
	return type.spec.defining || type.spec.definingForContent;
}
function replaceRange(tr$1, from, to, slice) {
	if (!slice.size) return tr$1.deleteRange(from, to);
	let $from = tr$1.doc.resolve(from), $to = tr$1.doc.resolve(to);
	if (fitsTrivially($from, $to, slice)) return tr$1.step(new ReplaceStep(from, to, slice));
	let targetDepths = coveredDepths($from, $to);
	if (targetDepths[targetDepths.length - 1] == 0) targetDepths.pop();
	let preferredTarget = -($from.depth + 1);
	targetDepths.unshift(preferredTarget);
	for (let d = $from.depth, pos = $from.pos - 1; d > 0; d--, pos--) {
		let spec = $from.node(d).type.spec;
		if (spec.defining || spec.definingAsContext || spec.isolating) break;
		if (targetDepths.indexOf(d) > -1) preferredTarget = d;
		else if ($from.before(d) == pos) targetDepths.splice(1, 0, -d);
	}
	let preferredTargetIndex = targetDepths.indexOf(preferredTarget);
	let leftNodes = [], preferredDepth = slice.openStart;
	for (let content = slice.content, i$1 = 0;; i$1++) {
		let node = content.firstChild;
		leftNodes.push(node);
		if (i$1 == slice.openStart) break;
		content = node.content;
	}
	for (let d = preferredDepth - 1; d >= 0; d--) {
		let leftNode = leftNodes[d], def = definesContent(leftNode.type);
		if (def && !leftNode.sameMarkup($from.node(Math.abs(preferredTarget) - 1))) preferredDepth = d;
		else if (def || !leftNode.type.isTextblock) break;
	}
	for (let j$1 = slice.openStart; j$1 >= 0; j$1--) {
		let openDepth = (j$1 + preferredDepth + 1) % (slice.openStart + 1);
		let insert = leftNodes[openDepth];
		if (!insert) continue;
		for (let i$1 = 0; i$1 < targetDepths.length; i$1++) {
			let targetDepth = targetDepths[(i$1 + preferredTargetIndex) % targetDepths.length], expand = true;
			if (targetDepth < 0) {
				expand = false;
				targetDepth = -targetDepth;
			}
			let parent = $from.node(targetDepth - 1), index = $from.index(targetDepth - 1);
			if (parent.canReplaceWith(index, index, insert.type, insert.marks)) return tr$1.replace($from.before(targetDepth), expand ? $to.after(targetDepth) : to, new Slice(closeFragment(slice.content, 0, slice.openStart, openDepth), openDepth, slice.openEnd));
		}
	}
	let startSteps = tr$1.steps.length;
	for (let i$1 = targetDepths.length - 1; i$1 >= 0; i$1--) {
		tr$1.replace(from, to, slice);
		if (tr$1.steps.length > startSteps) break;
		let depth = targetDepths[i$1];
		if (depth < 0) continue;
		from = $from.before(depth);
		to = $to.after(depth);
	}
}
function closeFragment(fragment, depth, oldOpen, newOpen, parent) {
	if (depth < oldOpen) {
		let first$1 = fragment.firstChild;
		fragment = fragment.replaceChild(0, first$1.copy(closeFragment(first$1.content, depth + 1, oldOpen, newOpen, first$1)));
	}
	if (depth > newOpen) {
		let match = parent.contentMatchAt(0);
		let start = match.fillBefore(fragment).append(fragment);
		fragment = start.append(match.matchFragment(start).fillBefore(Fragment.empty, true));
	}
	return fragment;
}
function replaceRangeWith(tr$1, from, to, node) {
	if (!node.isInline && from == to && tr$1.doc.resolve(from).parent.content.size) {
		let point = insertPoint(tr$1.doc, from, node.type);
		if (point != null) from = to = point;
	}
	tr$1.replaceRange(from, to, new Slice(Fragment.from(node), 0, 0));
}
function deleteRange$1(tr$1, from, to) {
	let $from = tr$1.doc.resolve(from), $to = tr$1.doc.resolve(to);
	if ($from.parent.isTextblock && $to.parent.isTextblock && $from.start() != $to.start() && $from.parentOffset == 0 && $to.parentOffset == 0) {
		let shared = $from.sharedDepth(to), isolated = false;
		for (let d = $from.depth; d > shared; d--) if ($from.node(d).type.spec.isolating) isolated = true;
		for (let d = $to.depth; d > shared; d--) if ($to.node(d).type.spec.isolating) isolated = true;
		if (!isolated) {
			for (let d = $from.depth; d > 0 && from == $from.start(d); d--) from = $from.before(d);
			for (let d = $to.depth; d > 0 && to == $to.start(d); d--) to = $to.before(d);
			$from = tr$1.doc.resolve(from);
			$to = tr$1.doc.resolve(to);
		}
	}
	let covered = coveredDepths($from, $to);
	for (let i$1 = 0; i$1 < covered.length; i$1++) {
		let depth = covered[i$1], last = i$1 == covered.length - 1;
		if (last && depth == 0 || $from.node(depth).type.contentMatch.validEnd) return tr$1.delete($from.start(depth), $to.end(depth));
		if (depth > 0 && (last || $from.node(depth - 1).canReplace($from.index(depth - 1), $to.indexAfter(depth - 1)))) return tr$1.delete($from.before(depth), $to.after(depth));
	}
	for (let d = 1; d <= $from.depth && d <= $to.depth; d++) if (from - $from.start(d) == $from.depth - d && to > $from.end(d) && $to.end(d) - to != $to.depth - d && $from.start(d - 1) == $to.start(d - 1) && $from.node(d - 1).canReplace($from.index(d - 1), $to.index(d - 1))) return tr$1.delete($from.before(d), to);
	tr$1.delete(from, to);
}
function coveredDepths($from, $to) {
	let result = [], minDepth = Math.min($from.depth, $to.depth);
	for (let d = minDepth; d >= 0; d--) {
		let start = $from.start(d);
		if (start < $from.pos - ($from.depth - d) || $to.end(d) > $to.pos + ($to.depth - d) || $from.node(d).type.spec.isolating || $to.node(d).type.spec.isolating) break;
		if (start == $to.start(d) || d == $from.depth && d == $to.depth && $from.parent.inlineContent && $to.parent.inlineContent && d && $to.start(d - 1) == start - 1) result.push(d);
	}
	return result;
}
var AttrStep = class AttrStep extends Step {
	constructor(pos, attr, value) {
		super();
		this.pos = pos;
		this.attr = attr;
		this.value = value;
	}
	apply(doc$2) {
		let node = doc$2.nodeAt(this.pos);
		if (!node) return StepResult.fail("No node at attribute step's position");
		let attrs = Object.create(null);
		for (let name in node.attrs) attrs[name] = node.attrs[name];
		attrs[this.attr] = this.value;
		let updated = node.type.create(attrs, null, node.marks);
		return StepResult.fromReplace(doc$2, this.pos, this.pos + 1, new Slice(Fragment.from(updated), 0, node.isLeaf ? 0 : 1));
	}
	getMap() {
		return StepMap.empty;
	}
	invert(doc$2) {
		return new AttrStep(this.pos, this.attr, doc$2.nodeAt(this.pos).attrs[this.attr]);
	}
	map(mapping) {
		let pos = mapping.mapResult(this.pos, 1);
		return pos.deletedAfter ? null : new AttrStep(pos.pos, this.attr, this.value);
	}
	toJSON() {
		return {
			stepType: "attr",
			pos: this.pos,
			attr: this.attr,
			value: this.value
		};
	}
	static fromJSON(schema, json) {
		if (typeof json.pos != "number" || typeof json.attr != "string") throw new RangeError("Invalid input for AttrStep.fromJSON");
		return new AttrStep(json.pos, json.attr, json.value);
	}
};
Step.jsonID("attr", AttrStep);
var DocAttrStep = class DocAttrStep extends Step {
	constructor(attr, value) {
		super();
		this.attr = attr;
		this.value = value;
	}
	apply(doc$2) {
		let attrs = Object.create(null);
		for (let name in doc$2.attrs) attrs[name] = doc$2.attrs[name];
		attrs[this.attr] = this.value;
		let updated = doc$2.type.create(attrs, doc$2.content, doc$2.marks);
		return StepResult.ok(updated);
	}
	getMap() {
		return StepMap.empty;
	}
	invert(doc$2) {
		return new DocAttrStep(this.attr, doc$2.attrs[this.attr]);
	}
	map(mapping) {
		return this;
	}
	toJSON() {
		return {
			stepType: "docAttr",
			attr: this.attr,
			value: this.value
		};
	}
	static fromJSON(schema, json) {
		if (typeof json.attr != "string") throw new RangeError("Invalid input for DocAttrStep.fromJSON");
		return new DocAttrStep(json.attr, json.value);
	}
};
Step.jsonID("docAttr", DocAttrStep);
var TransformError = class extends Error {};
TransformError = function TransformError$1(message) {
	let err = Error.call(this, message);
	err.__proto__ = TransformError$1.prototype;
	return err;
};
TransformError.prototype = Object.create(Error.prototype);
TransformError.prototype.constructor = TransformError;
TransformError.prototype.name = "TransformError";
var Transform = class {
	constructor(doc$2) {
		this.doc = doc$2;
		this.steps = [];
		this.docs = [];
		this.mapping = new Mapping();
	}
	get before() {
		return this.docs.length ? this.docs[0] : this.doc;
	}
	step(step$1) {
		let result = this.maybeStep(step$1);
		if (result.failed) throw new TransformError(result.failed);
		return this;
	}
	maybeStep(step$1) {
		let result = step$1.apply(this.doc);
		if (!result.failed) this.addStep(step$1, result.doc);
		return result;
	}
	get docChanged() {
		return this.steps.length > 0;
	}
	changedRange() {
		let from = 1e9, to = -1e9;
		for (let i$1 = 0; i$1 < this.mapping.maps.length; i$1++) {
			let map = this.mapping.maps[i$1];
			if (i$1) {
				from = map.map(from, 1);
				to = map.map(to, -1);
			}
			map.forEach((_f, _t, fromB, toB) => {
				from = Math.min(from, fromB);
				to = Math.max(to, toB);
			});
		}
		return from == 1e9 ? null : {
			from,
			to
		};
	}
	addStep(step$1, doc$2) {
		this.docs.push(this.doc);
		this.steps.push(step$1);
		this.mapping.appendMap(step$1.getMap());
		this.doc = doc$2;
	}
	replace(from, to = from, slice = Slice.empty) {
		let step$1 = replaceStep(this.doc, from, to, slice);
		if (step$1) this.step(step$1);
		return this;
	}
	replaceWith(from, to, content) {
		return this.replace(from, to, new Slice(Fragment.from(content), 0, 0));
	}
	delete(from, to) {
		return this.replace(from, to, Slice.empty);
	}
	insert(pos, content) {
		return this.replaceWith(pos, pos, content);
	}
	replaceRange(from, to, slice) {
		replaceRange(this, from, to, slice);
		return this;
	}
	replaceRangeWith(from, to, node) {
		replaceRangeWith(this, from, to, node);
		return this;
	}
	deleteRange(from, to) {
		deleteRange$1(this, from, to);
		return this;
	}
	lift(range, target) {
		lift$2(this, range, target);
		return this;
	}
	join(pos, depth = 1) {
		join(this, pos, depth);
		return this;
	}
	wrap(range, wrappers) {
		wrap(this, range, wrappers);
		return this;
	}
	setBlockType(from, to = from, type, attrs = null) {
		setBlockType$1(this, from, to, type, attrs);
		return this;
	}
	setNodeMarkup(pos, type, attrs = null, marks) {
		setNodeMarkup(this, pos, type, attrs, marks);
		return this;
	}
	setNodeAttribute(pos, attr, value) {
		this.step(new AttrStep(pos, attr, value));
		return this;
	}
	setDocAttribute(attr, value) {
		this.step(new DocAttrStep(attr, value));
		return this;
	}
	addNodeMark(pos, mark) {
		this.step(new AddNodeMarkStep(pos, mark));
		return this;
	}
	removeNodeMark(pos, mark) {
		let node = this.doc.nodeAt(pos);
		if (!node) throw new RangeError("No node at position " + pos);
		if (mark instanceof Mark$1) {
			if (mark.isInSet(node.marks)) this.step(new RemoveNodeMarkStep(pos, mark));
		} else {
			let set = node.marks, found$1, steps = [];
			while (found$1 = mark.isInSet(set)) {
				steps.push(new RemoveNodeMarkStep(pos, found$1));
				set = found$1.removeFromSet(set);
			}
			for (let i$1 = steps.length - 1; i$1 >= 0; i$1--) this.step(steps[i$1]);
		}
		return this;
	}
	split(pos, depth = 1, typesAfter) {
		split(this, pos, depth, typesAfter);
		return this;
	}
	addMark(from, to, mark) {
		addMark(this, from, to, mark);
		return this;
	}
	removeMark(from, to, mark) {
		removeMark(this, from, to, mark);
		return this;
	}
	clearIncompatible(pos, parentType, match) {
		clearIncompatible(this, pos, parentType, match);
		return this;
	}
};
var classesById = Object.create(null);
var Selection = class {
	constructor($anchor, $head, ranges) {
		this.$anchor = $anchor;
		this.$head = $head;
		this.ranges = ranges || [new SelectionRange($anchor.min($head), $anchor.max($head))];
	}
	get anchor() {
		return this.$anchor.pos;
	}
	get head() {
		return this.$head.pos;
	}
	get from() {
		return this.$from.pos;
	}
	get to() {
		return this.$to.pos;
	}
	get $from() {
		return this.ranges[0].$from;
	}
	get $to() {
		return this.ranges[0].$to;
	}
	get empty() {
		let ranges = this.ranges;
		for (let i$1 = 0; i$1 < ranges.length; i$1++) if (ranges[i$1].$from.pos != ranges[i$1].$to.pos) return false;
		return true;
	}
	content() {
		return this.$from.doc.slice(this.from, this.to, true);
	}
	replace(tr$1, content = Slice.empty) {
		let lastNode = content.content.lastChild, lastParent = null;
		for (let i$1 = 0; i$1 < content.openEnd; i$1++) {
			lastParent = lastNode;
			lastNode = lastNode.lastChild;
		}
		let mapFrom = tr$1.steps.length, ranges = this.ranges;
		for (let i$1 = 0; i$1 < ranges.length; i$1++) {
			let { $from, $to } = ranges[i$1], mapping = tr$1.mapping.slice(mapFrom);
			tr$1.replaceRange(mapping.map($from.pos), mapping.map($to.pos), i$1 ? Slice.empty : content);
			if (i$1 == 0) selectionToInsertionEnd$1(tr$1, mapFrom, (lastNode ? lastNode.isInline : lastParent && lastParent.isTextblock) ? -1 : 1);
		}
	}
	replaceWith(tr$1, node) {
		let mapFrom = tr$1.steps.length, ranges = this.ranges;
		for (let i$1 = 0; i$1 < ranges.length; i$1++) {
			let { $from, $to } = ranges[i$1], mapping = tr$1.mapping.slice(mapFrom);
			let from = mapping.map($from.pos), to = mapping.map($to.pos);
			if (i$1) tr$1.deleteRange(from, to);
			else {
				tr$1.replaceRangeWith(from, to, node);
				selectionToInsertionEnd$1(tr$1, mapFrom, node.isInline ? -1 : 1);
			}
		}
	}
	static findFrom($pos, dir, textOnly = false) {
		let inner = $pos.parent.inlineContent ? new TextSelection($pos) : findSelectionIn($pos.node(0), $pos.parent, $pos.pos, $pos.index(), dir, textOnly);
		if (inner) return inner;
		for (let depth = $pos.depth - 1; depth >= 0; depth--) {
			let found$1 = dir < 0 ? findSelectionIn($pos.node(0), $pos.node(depth), $pos.before(depth + 1), $pos.index(depth), dir, textOnly) : findSelectionIn($pos.node(0), $pos.node(depth), $pos.after(depth + 1), $pos.index(depth) + 1, dir, textOnly);
			if (found$1) return found$1;
		}
		return null;
	}
	static near($pos, bias = 1) {
		return this.findFrom($pos, bias) || this.findFrom($pos, -bias) || new AllSelection($pos.node(0));
	}
	static atStart(doc$2) {
		return findSelectionIn(doc$2, doc$2, 0, 0, 1) || new AllSelection(doc$2);
	}
	static atEnd(doc$2) {
		return findSelectionIn(doc$2, doc$2, doc$2.content.size, doc$2.childCount, -1) || new AllSelection(doc$2);
	}
	static fromJSON(doc$2, json) {
		if (!json || !json.type) throw new RangeError("Invalid input for Selection.fromJSON");
		let cls = classesById[json.type];
		if (!cls) throw new RangeError(`No selection type ${json.type} defined`);
		return cls.fromJSON(doc$2, json);
	}
	static jsonID(id, selectionClass) {
		if (id in classesById) throw new RangeError("Duplicate use of selection JSON ID " + id);
		classesById[id] = selectionClass;
		selectionClass.prototype.jsonID = id;
		return selectionClass;
	}
	getBookmark() {
		return TextSelection.between(this.$anchor, this.$head).getBookmark();
	}
};
Selection.prototype.visible = true;
var SelectionRange = class {
	constructor($from, $to) {
		this.$from = $from;
		this.$to = $to;
	}
};
var warnedAboutTextSelection = false;
function checkTextSelection($pos) {
	if (!warnedAboutTextSelection && !$pos.parent.inlineContent) {
		warnedAboutTextSelection = true;
		console["warn"]("TextSelection endpoint not pointing into a node with inline content (" + $pos.parent.type.name + ")");
	}
}
var TextSelection = class TextSelection extends Selection {
	constructor($anchor, $head = $anchor) {
		checkTextSelection($anchor);
		checkTextSelection($head);
		super($anchor, $head);
	}
	get $cursor() {
		return this.$anchor.pos == this.$head.pos ? this.$head : null;
	}
	map(doc$2, mapping) {
		let $head = doc$2.resolve(mapping.map(this.head));
		if (!$head.parent.inlineContent) return Selection.near($head);
		let $anchor = doc$2.resolve(mapping.map(this.anchor));
		return new TextSelection($anchor.parent.inlineContent ? $anchor : $head, $head);
	}
	replace(tr$1, content = Slice.empty) {
		super.replace(tr$1, content);
		if (content == Slice.empty) {
			let marks = this.$from.marksAcross(this.$to);
			if (marks) tr$1.ensureMarks(marks);
		}
	}
	eq(other) {
		return other instanceof TextSelection && other.anchor == this.anchor && other.head == this.head;
	}
	getBookmark() {
		return new TextBookmark(this.anchor, this.head);
	}
	toJSON() {
		return {
			type: "text",
			anchor: this.anchor,
			head: this.head
		};
	}
	static fromJSON(doc$2, json) {
		if (typeof json.anchor != "number" || typeof json.head != "number") throw new RangeError("Invalid input for TextSelection.fromJSON");
		return new TextSelection(doc$2.resolve(json.anchor), doc$2.resolve(json.head));
	}
	static create(doc$2, anchor, head = anchor) {
		let $anchor = doc$2.resolve(anchor);
		return new this($anchor, head == anchor ? $anchor : doc$2.resolve(head));
	}
	static between($anchor, $head, bias) {
		let dPos = $anchor.pos - $head.pos;
		if (!bias || dPos) bias = dPos >= 0 ? 1 : -1;
		if (!$head.parent.inlineContent) {
			let found$1 = Selection.findFrom($head, bias, true) || Selection.findFrom($head, -bias, true);
			if (found$1) $head = found$1.$head;
			else return Selection.near($head, bias);
		}
		if (!$anchor.parent.inlineContent) if (dPos == 0) $anchor = $head;
		else {
			$anchor = (Selection.findFrom($anchor, -bias, true) || Selection.findFrom($anchor, bias, true)).$anchor;
			if ($anchor.pos < $head.pos != dPos < 0) $anchor = $head;
		}
		return new TextSelection($anchor, $head);
	}
};
Selection.jsonID("text", TextSelection);
var TextBookmark = class TextBookmark {
	constructor(anchor, head) {
		this.anchor = anchor;
		this.head = head;
	}
	map(mapping) {
		return new TextBookmark(mapping.map(this.anchor), mapping.map(this.head));
	}
	resolve(doc$2) {
		return TextSelection.between(doc$2.resolve(this.anchor), doc$2.resolve(this.head));
	}
};
var NodeSelection = class NodeSelection extends Selection {
	constructor($pos) {
		let node = $pos.nodeAfter;
		let $end = $pos.node(0).resolve($pos.pos + node.nodeSize);
		super($pos, $end);
		this.node = node;
	}
	map(doc$2, mapping) {
		let { deleted, pos } = mapping.mapResult(this.anchor);
		let $pos = doc$2.resolve(pos);
		if (deleted) return Selection.near($pos);
		return new NodeSelection($pos);
	}
	content() {
		return new Slice(Fragment.from(this.node), 0, 0);
	}
	eq(other) {
		return other instanceof NodeSelection && other.anchor == this.anchor;
	}
	toJSON() {
		return {
			type: "node",
			anchor: this.anchor
		};
	}
	getBookmark() {
		return new NodeBookmark(this.anchor);
	}
	static fromJSON(doc$2, json) {
		if (typeof json.anchor != "number") throw new RangeError("Invalid input for NodeSelection.fromJSON");
		return new NodeSelection(doc$2.resolve(json.anchor));
	}
	static create(doc$2, from) {
		return new NodeSelection(doc$2.resolve(from));
	}
	static isSelectable(node) {
		return !node.isText && node.type.spec.selectable !== false;
	}
};
NodeSelection.prototype.visible = false;
Selection.jsonID("node", NodeSelection);
var NodeBookmark = class NodeBookmark {
	constructor(anchor) {
		this.anchor = anchor;
	}
	map(mapping) {
		let { deleted, pos } = mapping.mapResult(this.anchor);
		return deleted ? new TextBookmark(pos, pos) : new NodeBookmark(pos);
	}
	resolve(doc$2) {
		let $pos = doc$2.resolve(this.anchor), node = $pos.nodeAfter;
		if (node && NodeSelection.isSelectable(node)) return new NodeSelection($pos);
		return Selection.near($pos);
	}
};
var AllSelection = class AllSelection extends Selection {
	constructor(doc$2) {
		super(doc$2.resolve(0), doc$2.resolve(doc$2.content.size));
	}
	replace(tr$1, content = Slice.empty) {
		if (content == Slice.empty) {
			tr$1.delete(0, tr$1.doc.content.size);
			let sel = Selection.atStart(tr$1.doc);
			if (!sel.eq(tr$1.selection)) tr$1.setSelection(sel);
		} else super.replace(tr$1, content);
	}
	toJSON() {
		return { type: "all" };
	}
	static fromJSON(doc$2) {
		return new AllSelection(doc$2);
	}
	map(doc$2) {
		return new AllSelection(doc$2);
	}
	eq(other) {
		return other instanceof AllSelection;
	}
	getBookmark() {
		return AllBookmark;
	}
};
Selection.jsonID("all", AllSelection);
var AllBookmark = {
	map() {
		return this;
	},
	resolve(doc$2) {
		return new AllSelection(doc$2);
	}
};
function findSelectionIn(doc$2, node, pos, index, dir, text = false) {
	if (node.inlineContent) return TextSelection.create(doc$2, pos);
	for (let i$1 = index - (dir > 0 ? 0 : 1); dir > 0 ? i$1 < node.childCount : i$1 >= 0; i$1 += dir) {
		let child = node.child(i$1);
		if (!child.isAtom) {
			let inner = findSelectionIn(doc$2, child, pos + dir, dir < 0 ? child.childCount : 0, dir, text);
			if (inner) return inner;
		} else if (!text && NodeSelection.isSelectable(child)) return NodeSelection.create(doc$2, pos - (dir < 0 ? child.nodeSize : 0));
		pos += child.nodeSize * dir;
	}
	return null;
}
function selectionToInsertionEnd$1(tr$1, startLen, bias) {
	let last = tr$1.steps.length - 1;
	if (last < startLen) return;
	let step$1 = tr$1.steps[last];
	if (!(step$1 instanceof ReplaceStep || step$1 instanceof ReplaceAroundStep)) return;
	let map = tr$1.mapping.maps[last], end;
	map.forEach((_from, _to, _newFrom, newTo) => {
		if (end == null) end = newTo;
	});
	tr$1.setSelection(Selection.near(tr$1.doc.resolve(end), bias));
}
var UPDATED_SEL = 1, UPDATED_MARKS = 2, UPDATED_SCROLL = 4;
var Transaction = class extends Transform {
	constructor(state) {
		super(state.doc);
		this.curSelectionFor = 0;
		this.updated = 0;
		this.meta = Object.create(null);
		this.time = Date.now();
		this.curSelection = state.selection;
		this.storedMarks = state.storedMarks;
	}
	get selection() {
		if (this.curSelectionFor < this.steps.length) {
			this.curSelection = this.curSelection.map(this.doc, this.mapping.slice(this.curSelectionFor));
			this.curSelectionFor = this.steps.length;
		}
		return this.curSelection;
	}
	setSelection(selection) {
		if (selection.$from.doc != this.doc) throw new RangeError("Selection passed to setSelection must point at the current document");
		this.curSelection = selection;
		this.curSelectionFor = this.steps.length;
		this.updated = (this.updated | UPDATED_SEL) & ~UPDATED_MARKS;
		this.storedMarks = null;
		return this;
	}
	get selectionSet() {
		return (this.updated & UPDATED_SEL) > 0;
	}
	setStoredMarks(marks) {
		this.storedMarks = marks;
		this.updated |= UPDATED_MARKS;
		return this;
	}
	ensureMarks(marks) {
		if (!Mark$1.sameSet(this.storedMarks || this.selection.$from.marks(), marks)) this.setStoredMarks(marks);
		return this;
	}
	addStoredMark(mark) {
		return this.ensureMarks(mark.addToSet(this.storedMarks || this.selection.$head.marks()));
	}
	removeStoredMark(mark) {
		return this.ensureMarks(mark.removeFromSet(this.storedMarks || this.selection.$head.marks()));
	}
	get storedMarksSet() {
		return (this.updated & UPDATED_MARKS) > 0;
	}
	addStep(step$1, doc$2) {
		super.addStep(step$1, doc$2);
		this.updated = this.updated & ~UPDATED_MARKS;
		this.storedMarks = null;
	}
	setTime(time) {
		this.time = time;
		return this;
	}
	replaceSelection(slice) {
		this.selection.replace(this, slice);
		return this;
	}
	replaceSelectionWith(node, inheritMarks = true) {
		let selection = this.selection;
		if (inheritMarks) node = node.mark(this.storedMarks || (selection.empty ? selection.$from.marks() : selection.$from.marksAcross(selection.$to) || Mark$1.none));
		selection.replaceWith(this, node);
		return this;
	}
	deleteSelection() {
		this.selection.replace(this);
		return this;
	}
	insertText(text, from, to) {
		let schema = this.doc.type.schema;
		if (from == null) {
			if (!text) return this.deleteSelection();
			return this.replaceSelectionWith(schema.text(text), true);
		} else {
			if (to == null) to = from;
			if (!text) return this.deleteRange(from, to);
			let marks = this.storedMarks;
			if (!marks) {
				let $from = this.doc.resolve(from);
				marks = to == from ? $from.marks() : $from.marksAcross(this.doc.resolve(to));
			}
			this.replaceRangeWith(from, to, schema.text(text, marks));
			if (!this.selection.empty && this.selection.to == from + text.length) this.setSelection(Selection.near(this.selection.$to));
			return this;
		}
	}
	setMeta(key, value) {
		this.meta[typeof key == "string" ? key : key.key] = value;
		return this;
	}
	getMeta(key) {
		return this.meta[typeof key == "string" ? key : key.key];
	}
	get isGeneric() {
		for (let _$1 in this.meta) return false;
		return true;
	}
	scrollIntoView() {
		this.updated |= UPDATED_SCROLL;
		return this;
	}
	get scrolledIntoView() {
		return (this.updated & UPDATED_SCROLL) > 0;
	}
};
function bind(f, self) {
	return !self || !f ? f : f.bind(self);
}
var FieldDesc = class {
	constructor(name, desc, self) {
		this.name = name;
		this.init = bind(desc.init, self);
		this.apply = bind(desc.apply, self);
	}
};
var baseFields = [
	new FieldDesc("doc", {
		init(config) {
			return config.doc || config.schema.topNodeType.createAndFill();
		},
		apply(tr$1) {
			return tr$1.doc;
		}
	}),
	new FieldDesc("selection", {
		init(config, instance) {
			return config.selection || Selection.atStart(instance.doc);
		},
		apply(tr$1) {
			return tr$1.selection;
		}
	}),
	new FieldDesc("storedMarks", {
		init(config) {
			return config.storedMarks || null;
		},
		apply(tr$1, _marks, _old, state) {
			return state.selection.$cursor ? tr$1.storedMarks : null;
		}
	}),
	new FieldDesc("scrollToSelection", {
		init() {
			return 0;
		},
		apply(tr$1, prev) {
			return tr$1.scrolledIntoView ? prev + 1 : prev;
		}
	})
];
var Configuration = class {
	constructor(schema, plugins) {
		this.schema = schema;
		this.plugins = [];
		this.pluginsByKey = Object.create(null);
		this.fields = baseFields.slice();
		if (plugins) plugins.forEach((plugin) => {
			if (this.pluginsByKey[plugin.key]) throw new RangeError("Adding different instances of a keyed plugin (" + plugin.key + ")");
			this.plugins.push(plugin);
			this.pluginsByKey[plugin.key] = plugin;
			if (plugin.spec.state) this.fields.push(new FieldDesc(plugin.key, plugin.spec.state, plugin));
		});
	}
};
var EditorState = class EditorState {
	constructor(config) {
		this.config = config;
	}
	get schema() {
		return this.config.schema;
	}
	get plugins() {
		return this.config.plugins;
	}
	apply(tr$1) {
		return this.applyTransaction(tr$1).state;
	}
	filterTransaction(tr$1, ignore = -1) {
		for (let i$1 = 0; i$1 < this.config.plugins.length; i$1++) if (i$1 != ignore) {
			let plugin = this.config.plugins[i$1];
			if (plugin.spec.filterTransaction && !plugin.spec.filterTransaction.call(plugin, tr$1, this)) return false;
		}
		return true;
	}
	applyTransaction(rootTr) {
		if (!this.filterTransaction(rootTr)) return {
			state: this,
			transactions: []
		};
		let trs = [rootTr], newState = this.applyInner(rootTr), seen = null;
		for (;;) {
			let haveNew = false;
			for (let i$1 = 0; i$1 < this.config.plugins.length; i$1++) {
				let plugin = this.config.plugins[i$1];
				if (plugin.spec.appendTransaction) {
					let n = seen ? seen[i$1].n : 0, oldState = seen ? seen[i$1].state : this;
					let tr$1 = n < trs.length && plugin.spec.appendTransaction.call(plugin, n ? trs.slice(n) : trs, oldState, newState);
					if (tr$1 && newState.filterTransaction(tr$1, i$1)) {
						tr$1.setMeta("appendedTransaction", rootTr);
						if (!seen) {
							seen = [];
							for (let j$1 = 0; j$1 < this.config.plugins.length; j$1++) seen.push(j$1 < i$1 ? {
								state: newState,
								n: trs.length
							} : {
								state: this,
								n: 0
							});
						}
						trs.push(tr$1);
						newState = newState.applyInner(tr$1);
						haveNew = true;
					}
					if (seen) seen[i$1] = {
						state: newState,
						n: trs.length
					};
				}
			}
			if (!haveNew) return {
				state: newState,
				transactions: trs
			};
		}
	}
	applyInner(tr$1) {
		if (!tr$1.before.eq(this.doc)) throw new RangeError("Applying a mismatched transaction");
		let newInstance = new EditorState(this.config), fields = this.config.fields;
		for (let i$1 = 0; i$1 < fields.length; i$1++) {
			let field = fields[i$1];
			newInstance[field.name] = field.apply(tr$1, this[field.name], this, newInstance);
		}
		return newInstance;
	}
	get tr() {
		return new Transaction(this);
	}
	static create(config) {
		let $config = new Configuration(config.doc ? config.doc.type.schema : config.schema, config.plugins);
		let instance = new EditorState($config);
		for (let i$1 = 0; i$1 < $config.fields.length; i$1++) instance[$config.fields[i$1].name] = $config.fields[i$1].init(config, instance);
		return instance;
	}
	reconfigure(config) {
		let $config = new Configuration(this.schema, config.plugins);
		let fields = $config.fields, instance = new EditorState($config);
		for (let i$1 = 0; i$1 < fields.length; i$1++) {
			let name = fields[i$1].name;
			instance[name] = this.hasOwnProperty(name) ? this[name] : fields[i$1].init(config, instance);
		}
		return instance;
	}
	toJSON(pluginFields) {
		let result = {
			doc: this.doc.toJSON(),
			selection: this.selection.toJSON()
		};
		if (this.storedMarks) result.storedMarks = this.storedMarks.map((m$1) => m$1.toJSON());
		if (pluginFields && typeof pluginFields == "object") for (let prop in pluginFields) {
			if (prop == "doc" || prop == "selection") throw new RangeError("The JSON fields `doc` and `selection` are reserved");
			let plugin = pluginFields[prop], state = plugin.spec.state;
			if (state && state.toJSON) result[prop] = state.toJSON.call(plugin, this[plugin.key]);
		}
		return result;
	}
	static fromJSON(config, json, pluginFields) {
		if (!json) throw new RangeError("Invalid input for EditorState.fromJSON");
		if (!config.schema) throw new RangeError("Required config field 'schema' missing");
		let $config = new Configuration(config.schema, config.plugins);
		let instance = new EditorState($config);
		$config.fields.forEach((field) => {
			if (field.name == "doc") instance.doc = Node.fromJSON(config.schema, json.doc);
			else if (field.name == "selection") instance.selection = Selection.fromJSON(instance.doc, json.selection);
			else if (field.name == "storedMarks") {
				if (json.storedMarks) instance.storedMarks = json.storedMarks.map(config.schema.markFromJSON);
			} else {
				if (pluginFields) for (let prop in pluginFields) {
					let plugin = pluginFields[prop], state = plugin.spec.state;
					if (plugin.key == field.name && state && state.fromJSON && Object.prototype.hasOwnProperty.call(json, prop)) {
						instance[field.name] = state.fromJSON.call(plugin, config, json[prop], instance);
						return;
					}
				}
				instance[field.name] = field.init(config, instance);
			}
		});
		return instance;
	}
};
function bindProps(obj, self, target) {
	for (let prop in obj) {
		let val = obj[prop];
		if (val instanceof Function) val = val.bind(self);
		else if (prop == "handleDOMEvents") val = bindProps(val, self, {});
		target[prop] = val;
	}
	return target;
}
var Plugin = class {
	constructor(spec) {
		this.spec = spec;
		this.props = {};
		if (spec.props) bindProps(spec.props, this, this.props);
		this.key = spec.key ? spec.key.key : createKey("plugin");
	}
	getState(state) {
		return state[this.key];
	}
};
var keys$1 = Object.create(null);
function createKey(name) {
	if (name in keys$1) return name + "$" + ++keys$1[name];
	keys$1[name] = 0;
	return name + "$";
}
var PluginKey = class {
	constructor(name = "key") {
		this.key = createKey(name);
	}
	get(state) {
		return state.config.pluginsByKey[this.key];
	}
	getState(state) {
		return state[this.key];
	}
};
var deleteSelection = (state, dispatch) => {
	if (state.selection.empty) return false;
	if (dispatch) dispatch(state.tr.deleteSelection().scrollIntoView());
	return true;
};
function atBlockStart(state, view) {
	let { $cursor } = state.selection;
	if (!$cursor || (view ? !view.endOfTextblock("backward", state) : $cursor.parentOffset > 0)) return null;
	return $cursor;
}
var joinBackward = (state, dispatch, view) => {
	let $cursor = atBlockStart(state, view);
	if (!$cursor) return false;
	let $cut = findCutBefore($cursor);
	if (!$cut) {
		let range = $cursor.blockRange(), target = range && liftTarget(range);
		if (target == null) return false;
		if (dispatch) dispatch(state.tr.lift(range, target).scrollIntoView());
		return true;
	}
	let before = $cut.nodeBefore;
	if (deleteBarrier(state, $cut, dispatch, -1)) return true;
	if ($cursor.parent.content.size == 0 && (textblockAt(before, "end") || NodeSelection.isSelectable(before))) for (let depth = $cursor.depth;; depth--) {
		let delStep = replaceStep(state.doc, $cursor.before(depth), $cursor.after(depth), Slice.empty);
		if (delStep && delStep.slice.size < delStep.to - delStep.from) {
			if (dispatch) {
				let tr$1 = state.tr.step(delStep);
				tr$1.setSelection(textblockAt(before, "end") ? Selection.findFrom(tr$1.doc.resolve(tr$1.mapping.map($cut.pos, -1)), -1) : NodeSelection.create(tr$1.doc, $cut.pos - before.nodeSize));
				dispatch(tr$1.scrollIntoView());
			}
			return true;
		}
		if (depth == 1 || $cursor.node(depth - 1).childCount > 1) break;
	}
	if (before.isAtom && $cut.depth == $cursor.depth - 1) {
		if (dispatch) dispatch(state.tr.delete($cut.pos - before.nodeSize, $cut.pos).scrollIntoView());
		return true;
	}
	return false;
};
var joinTextblockBackward = (state, dispatch, view) => {
	let $cursor = atBlockStart(state, view);
	if (!$cursor) return false;
	let $cut = findCutBefore($cursor);
	return $cut ? joinTextblocksAround(state, $cut, dispatch) : false;
};
var joinTextblockForward = (state, dispatch, view) => {
	let $cursor = atBlockEnd(state, view);
	if (!$cursor) return false;
	let $cut = findCutAfter($cursor);
	return $cut ? joinTextblocksAround(state, $cut, dispatch) : false;
};
function joinTextblocksAround(state, $cut, dispatch) {
	let beforeText = $cut.nodeBefore, beforePos = $cut.pos - 1;
	for (; !beforeText.isTextblock; beforePos--) {
		if (beforeText.type.spec.isolating) return false;
		let child = beforeText.lastChild;
		if (!child) return false;
		beforeText = child;
	}
	let afterText = $cut.nodeAfter, afterPos = $cut.pos + 1;
	for (; !afterText.isTextblock; afterPos++) {
		if (afterText.type.spec.isolating) return false;
		let child = afterText.firstChild;
		if (!child) return false;
		afterText = child;
	}
	let step$1 = replaceStep(state.doc, beforePos, afterPos, Slice.empty);
	if (!step$1 || step$1.from != beforePos || step$1 instanceof ReplaceStep && step$1.slice.size >= afterPos - beforePos) return false;
	if (dispatch) {
		let tr$1 = state.tr.step(step$1);
		tr$1.setSelection(TextSelection.create(tr$1.doc, beforePos));
		dispatch(tr$1.scrollIntoView());
	}
	return true;
}
function textblockAt(node, side, only = false) {
	for (let scan = node; scan; scan = side == "start" ? scan.firstChild : scan.lastChild) {
		if (scan.isTextblock) return true;
		if (only && scan.childCount != 1) return false;
	}
	return false;
}
var selectNodeBackward = (state, dispatch, view) => {
	let { $head, empty: empty$1 } = state.selection, $cut = $head;
	if (!empty$1) return false;
	if ($head.parent.isTextblock) {
		if (view ? !view.endOfTextblock("backward", state) : $head.parentOffset > 0) return false;
		$cut = findCutBefore($head);
	}
	let node = $cut && $cut.nodeBefore;
	if (!node || !NodeSelection.isSelectable(node)) return false;
	if (dispatch) dispatch(state.tr.setSelection(NodeSelection.create(state.doc, $cut.pos - node.nodeSize)).scrollIntoView());
	return true;
};
function findCutBefore($pos) {
	if (!$pos.parent.type.spec.isolating) for (let i$1 = $pos.depth - 1; i$1 >= 0; i$1--) {
		if ($pos.index(i$1) > 0) return $pos.doc.resolve($pos.before(i$1 + 1));
		if ($pos.node(i$1).type.spec.isolating) break;
	}
	return null;
}
function atBlockEnd(state, view) {
	let { $cursor } = state.selection;
	if (!$cursor || (view ? !view.endOfTextblock("forward", state) : $cursor.parentOffset < $cursor.parent.content.size)) return null;
	return $cursor;
}
var joinForward = (state, dispatch, view) => {
	let $cursor = atBlockEnd(state, view);
	if (!$cursor) return false;
	let $cut = findCutAfter($cursor);
	if (!$cut) return false;
	let after = $cut.nodeAfter;
	if (deleteBarrier(state, $cut, dispatch, 1)) return true;
	if ($cursor.parent.content.size == 0 && (textblockAt(after, "start") || NodeSelection.isSelectable(after))) {
		let delStep = replaceStep(state.doc, $cursor.before(), $cursor.after(), Slice.empty);
		if (delStep && delStep.slice.size < delStep.to - delStep.from) {
			if (dispatch) {
				let tr$1 = state.tr.step(delStep);
				tr$1.setSelection(textblockAt(after, "start") ? Selection.findFrom(tr$1.doc.resolve(tr$1.mapping.map($cut.pos)), 1) : NodeSelection.create(tr$1.doc, tr$1.mapping.map($cut.pos)));
				dispatch(tr$1.scrollIntoView());
			}
			return true;
		}
	}
	if (after.isAtom && $cut.depth == $cursor.depth - 1) {
		if (dispatch) dispatch(state.tr.delete($cut.pos, $cut.pos + after.nodeSize).scrollIntoView());
		return true;
	}
	return false;
};
var selectNodeForward = (state, dispatch, view) => {
	let { $head, empty: empty$1 } = state.selection, $cut = $head;
	if (!empty$1) return false;
	if ($head.parent.isTextblock) {
		if (view ? !view.endOfTextblock("forward", state) : $head.parentOffset < $head.parent.content.size) return false;
		$cut = findCutAfter($head);
	}
	let node = $cut && $cut.nodeAfter;
	if (!node || !NodeSelection.isSelectable(node)) return false;
	if (dispatch) dispatch(state.tr.setSelection(NodeSelection.create(state.doc, $cut.pos)).scrollIntoView());
	return true;
};
function findCutAfter($pos) {
	if (!$pos.parent.type.spec.isolating) for (let i$1 = $pos.depth - 1; i$1 >= 0; i$1--) {
		let parent = $pos.node(i$1);
		if ($pos.index(i$1) + 1 < parent.childCount) return $pos.doc.resolve($pos.after(i$1 + 1));
		if (parent.type.spec.isolating) break;
	}
	return null;
}
var joinUp = (state, dispatch) => {
	let sel = state.selection, nodeSel = sel instanceof NodeSelection, point;
	if (nodeSel) {
		if (sel.node.isTextblock || !canJoin(state.doc, sel.from)) return false;
		point = sel.from;
	} else {
		point = joinPoint(state.doc, sel.from, -1);
		if (point == null) return false;
	}
	if (dispatch) {
		let tr$1 = state.tr.join(point);
		if (nodeSel) tr$1.setSelection(NodeSelection.create(tr$1.doc, point - state.doc.resolve(point).nodeBefore.nodeSize));
		dispatch(tr$1.scrollIntoView());
	}
	return true;
};
var joinDown = (state, dispatch) => {
	let sel = state.selection, point;
	if (sel instanceof NodeSelection) {
		if (sel.node.isTextblock || !canJoin(state.doc, sel.to)) return false;
		point = sel.to;
	} else {
		point = joinPoint(state.doc, sel.to, 1);
		if (point == null) return false;
	}
	if (dispatch) dispatch(state.tr.join(point).scrollIntoView());
	return true;
};
var lift = (state, dispatch) => {
	let { $from, $to } = state.selection;
	let range = $from.blockRange($to), target = range && liftTarget(range);
	if (target == null) return false;
	if (dispatch) dispatch(state.tr.lift(range, target).scrollIntoView());
	return true;
};
var newlineInCode = (state, dispatch) => {
	let { $head, $anchor } = state.selection;
	if (!$head.parent.type.spec.code || !$head.sameParent($anchor)) return false;
	if (dispatch) dispatch(state.tr.insertText("\n").scrollIntoView());
	return true;
};
function defaultBlockAt$1(match) {
	for (let i$1 = 0; i$1 < match.edgeCount; i$1++) {
		let { type } = match.edge(i$1);
		if (type.isTextblock && !type.hasRequiredAttrs()) return type;
	}
	return null;
}
var exitCode = (state, dispatch) => {
	let { $head, $anchor } = state.selection;
	if (!$head.parent.type.spec.code || !$head.sameParent($anchor)) return false;
	let above = $head.node(-1), after = $head.indexAfter(-1), type = defaultBlockAt$1(above.contentMatchAt(after));
	if (!type || !above.canReplaceWith(after, after, type)) return false;
	if (dispatch) {
		let pos = $head.after(), tr$1 = state.tr.replaceWith(pos, pos, type.createAndFill());
		tr$1.setSelection(Selection.near(tr$1.doc.resolve(pos), 1));
		dispatch(tr$1.scrollIntoView());
	}
	return true;
};
var createParagraphNear = (state, dispatch) => {
	let sel = state.selection, { $from, $to } = sel;
	if (sel instanceof AllSelection || $from.parent.inlineContent || $to.parent.inlineContent) return false;
	let type = defaultBlockAt$1($to.parent.contentMatchAt($to.indexAfter()));
	if (!type || !type.isTextblock) return false;
	if (dispatch) {
		let side = (!$from.parentOffset && $to.index() < $to.parent.childCount ? $from : $to).pos;
		let tr$1 = state.tr.insert(side, type.createAndFill());
		tr$1.setSelection(TextSelection.create(tr$1.doc, side + 1));
		dispatch(tr$1.scrollIntoView());
	}
	return true;
};
var liftEmptyBlock = (state, dispatch) => {
	let { $cursor } = state.selection;
	if (!$cursor || $cursor.parent.content.size) return false;
	if ($cursor.depth > 1 && $cursor.after() != $cursor.end(-1)) {
		let before = $cursor.before();
		if (canSplit(state.doc, before)) {
			if (dispatch) dispatch(state.tr.split(before).scrollIntoView());
			return true;
		}
	}
	let range = $cursor.blockRange(), target = range && liftTarget(range);
	if (target == null) return false;
	if (dispatch) dispatch(state.tr.lift(range, target).scrollIntoView());
	return true;
};
function splitBlockAs(splitNode) {
	return (state, dispatch) => {
		let { $from, $to } = state.selection;
		if (state.selection instanceof NodeSelection && state.selection.node.isBlock) {
			if (!$from.parentOffset || !canSplit(state.doc, $from.pos)) return false;
			if (dispatch) dispatch(state.tr.split($from.pos).scrollIntoView());
			return true;
		}
		if (!$from.depth) return false;
		let types = [];
		let splitDepth, deflt, atEnd = false, atStart = false;
		for (let d = $from.depth;; d--) if ($from.node(d).isBlock) {
			atEnd = $from.end(d) == $from.pos + ($from.depth - d);
			atStart = $from.start(d) == $from.pos - ($from.depth - d);
			deflt = defaultBlockAt$1($from.node(d - 1).contentMatchAt($from.indexAfter(d - 1)));
			let splitType = splitNode && splitNode($to.parent, atEnd, $from);
			types.unshift(splitType || (atEnd && deflt ? { type: deflt } : null));
			splitDepth = d;
			break;
		} else {
			if (d == 1) return false;
			types.unshift(null);
		}
		let tr$1 = state.tr;
		if (state.selection instanceof TextSelection || state.selection instanceof AllSelection) tr$1.deleteSelection();
		let splitPos = tr$1.mapping.map($from.pos);
		let can = canSplit(tr$1.doc, splitPos, types.length, types);
		if (!can) {
			types[0] = deflt ? { type: deflt } : null;
			can = canSplit(tr$1.doc, splitPos, types.length, types);
		}
		if (!can) return false;
		tr$1.split(splitPos, types.length, types);
		if (!atEnd && atStart && $from.node(splitDepth).type != deflt) {
			let first$1 = tr$1.mapping.map($from.before(splitDepth)), $first = tr$1.doc.resolve(first$1);
			if (deflt && $from.node(splitDepth - 1).canReplaceWith($first.index(), $first.index() + 1, deflt)) tr$1.setNodeMarkup(tr$1.mapping.map($from.before(splitDepth)), deflt);
		}
		if (dispatch) dispatch(tr$1.scrollIntoView());
		return true;
	};
}
var splitBlock$1 = splitBlockAs();
var selectParentNode = (state, dispatch) => {
	let { $from, to } = state.selection, pos;
	let same = $from.sharedDepth(to);
	if (same == 0) return false;
	pos = $from.before(same);
	if (dispatch) dispatch(state.tr.setSelection(NodeSelection.create(state.doc, pos)));
	return true;
};
var selectAll$1 = (state, dispatch) => {
	if (dispatch) dispatch(state.tr.setSelection(new AllSelection(state.doc)));
	return true;
};
function joinMaybeClear(state, $pos, dispatch) {
	let before = $pos.nodeBefore, after = $pos.nodeAfter, index = $pos.index();
	if (!before || !after || !before.type.compatibleContent(after.type)) return false;
	if (!before.content.size && $pos.parent.canReplace(index - 1, index)) {
		if (dispatch) dispatch(state.tr.delete($pos.pos - before.nodeSize, $pos.pos).scrollIntoView());
		return true;
	}
	if (!$pos.parent.canReplace(index, index + 1) || !(after.isTextblock || canJoin(state.doc, $pos.pos))) return false;
	if (dispatch) dispatch(state.tr.join($pos.pos).scrollIntoView());
	return true;
}
function deleteBarrier(state, $cut, dispatch, dir) {
	let before = $cut.nodeBefore, after = $cut.nodeAfter, conn, match;
	let isolated = before.type.spec.isolating || after.type.spec.isolating;
	if (!isolated && joinMaybeClear(state, $cut, dispatch)) return true;
	let canDelAfter = !isolated && $cut.parent.canReplace($cut.index(), $cut.index() + 1);
	if (canDelAfter && (conn = (match = before.contentMatchAt(before.childCount)).findWrapping(after.type)) && match.matchType(conn[0] || after.type).validEnd) {
		if (dispatch) {
			let end = $cut.pos + after.nodeSize, wrap$1 = Fragment.empty;
			for (let i$1 = conn.length - 1; i$1 >= 0; i$1--) wrap$1 = Fragment.from(conn[i$1].create(null, wrap$1));
			wrap$1 = Fragment.from(before.copy(wrap$1));
			let tr$1 = state.tr.step(new ReplaceAroundStep($cut.pos - 1, end, $cut.pos, end, new Slice(wrap$1, 1, 0), conn.length, true));
			let $joinAt = tr$1.doc.resolve(end + 2 * conn.length);
			if ($joinAt.nodeAfter && $joinAt.nodeAfter.type == before.type && canJoin(tr$1.doc, $joinAt.pos)) tr$1.join($joinAt.pos);
			dispatch(tr$1.scrollIntoView());
		}
		return true;
	}
	let selAfter = after.type.spec.isolating || dir > 0 && isolated ? null : Selection.findFrom($cut, 1);
	let range = selAfter && selAfter.$from.blockRange(selAfter.$to), target = range && liftTarget(range);
	if (target != null && target >= $cut.depth) {
		if (dispatch) dispatch(state.tr.lift(range, target).scrollIntoView());
		return true;
	}
	if (canDelAfter && textblockAt(after, "start", true) && textblockAt(before, "end")) {
		let at = before, wrap$1 = [];
		for (;;) {
			wrap$1.push(at);
			if (at.isTextblock) break;
			at = at.lastChild;
		}
		let afterText = after, afterDepth = 1;
		for (; !afterText.isTextblock; afterText = afterText.firstChild) afterDepth++;
		if (at.canReplace(at.childCount, at.childCount, afterText.content)) {
			if (dispatch) {
				let end = Fragment.empty;
				for (let i$1 = wrap$1.length - 1; i$1 >= 0; i$1--) end = Fragment.from(wrap$1[i$1].copy(end));
				dispatch(state.tr.step(new ReplaceAroundStep($cut.pos - wrap$1.length, $cut.pos + after.nodeSize, $cut.pos + afterDepth, $cut.pos + after.nodeSize - afterDepth, new Slice(end, wrap$1.length, 0), 0, true)).scrollIntoView());
			}
			return true;
		}
	}
	return false;
}
function selectTextblockSide(side) {
	return function(state, dispatch) {
		let sel = state.selection, $pos = side < 0 ? sel.$from : sel.$to;
		let depth = $pos.depth;
		while ($pos.node(depth).isInline) {
			if (!depth) return false;
			depth--;
		}
		if (!$pos.node(depth).isTextblock) return false;
		if (dispatch) dispatch(state.tr.setSelection(TextSelection.create(state.doc, side < 0 ? $pos.start(depth) : $pos.end(depth))));
		return true;
	};
}
var selectTextblockStart = selectTextblockSide(-1);
var selectTextblockEnd = selectTextblockSide(1);
function wrapIn(nodeType, attrs = null) {
	return function(state, dispatch) {
		let { $from, $to } = state.selection;
		let range = $from.blockRange($to), wrapping = range && findWrapping(range, nodeType, attrs);
		if (!wrapping) return false;
		if (dispatch) dispatch(state.tr.wrap(range, wrapping).scrollIntoView());
		return true;
	};
}
function setBlockType(nodeType, attrs = null) {
	return function(state, dispatch) {
		let applicable = false;
		for (let i$1 = 0; i$1 < state.selection.ranges.length && !applicable; i$1++) {
			let { $from: { pos: from }, $to: { pos: to } } = state.selection.ranges[i$1];
			state.doc.nodesBetween(from, to, (node, pos) => {
				if (applicable) return false;
				if (!node.isTextblock || node.hasMarkup(nodeType, attrs)) return;
				if (node.type == nodeType) applicable = true;
				else {
					let $pos = state.doc.resolve(pos), index = $pos.index();
					applicable = $pos.parent.canReplaceWith(index, index + 1, nodeType);
				}
			});
		}
		if (!applicable) return false;
		if (dispatch) {
			let tr$1 = state.tr;
			for (let i$1 = 0; i$1 < state.selection.ranges.length; i$1++) {
				let { $from: { pos: from }, $to: { pos: to } } = state.selection.ranges[i$1];
				tr$1.setBlockType(from, to, nodeType, attrs);
			}
			dispatch(tr$1.scrollIntoView());
		}
		return true;
	};
}
function chainCommands(...commands) {
	return function(state, dispatch, view) {
		for (let i$1 = 0; i$1 < commands.length; i$1++) if (commands[i$1](state, dispatch, view)) return true;
		return false;
	};
}
var backspace = chainCommands(deleteSelection, joinBackward, selectNodeBackward);
var del = chainCommands(deleteSelection, joinForward, selectNodeForward);
var pcBaseKeymap = {
	"Enter": chainCommands(newlineInCode, createParagraphNear, liftEmptyBlock, splitBlock$1),
	"Mod-Enter": exitCode,
	"Backspace": backspace,
	"Mod-Backspace": backspace,
	"Shift-Backspace": backspace,
	"Delete": del,
	"Mod-Delete": del,
	"Mod-a": selectAll$1
};
var macBaseKeymap = {
	"Ctrl-h": pcBaseKeymap["Backspace"],
	"Alt-Backspace": pcBaseKeymap["Mod-Backspace"],
	"Ctrl-d": pcBaseKeymap["Delete"],
	"Ctrl-Alt-Backspace": pcBaseKeymap["Mod-Delete"],
	"Alt-Delete": pcBaseKeymap["Mod-Delete"],
	"Alt-d": pcBaseKeymap["Mod-Delete"],
	"Ctrl-a": selectTextblockStart,
	"Ctrl-e": selectTextblockEnd
};
for (let key in pcBaseKeymap) macBaseKeymap[key] = pcBaseKeymap[key];
typeof navigator != "undefined" ? /Mac|iP(hone|[oa]d)/.test(navigator.platform) : typeof os != "undefined" && os.platform && os.platform();
function wrapInList(listType, attrs = null) {
	return function(state, dispatch) {
		let { $from, $to } = state.selection;
		let range = $from.blockRange($to);
		if (!range) return false;
		let tr$1 = dispatch ? state.tr : null;
		if (!wrapRangeInList(tr$1, range, listType, attrs)) return false;
		if (dispatch) dispatch(tr$1.scrollIntoView());
		return true;
	};
}
function wrapRangeInList(tr$1, range, listType, attrs = null) {
	let doJoin = false, outerRange = range, doc$2 = range.$from.doc;
	if (range.depth >= 2 && range.$from.node(range.depth - 1).type.compatibleContent(listType) && range.startIndex == 0) {
		if (range.$from.index(range.depth - 1) == 0) return false;
		let $insert = doc$2.resolve(range.start - 2);
		outerRange = new NodeRange($insert, $insert, range.depth);
		if (range.endIndex < range.parent.childCount) range = new NodeRange(range.$from, doc$2.resolve(range.$to.end(range.depth)), range.depth);
		doJoin = true;
	}
	let wrap$1 = findWrapping(outerRange, listType, attrs, range);
	if (!wrap$1) return false;
	if (tr$1) doWrapInList(tr$1, range, wrap$1, doJoin, listType);
	return true;
}
function doWrapInList(tr$1, range, wrappers, joinBefore, listType) {
	let content = Fragment.empty;
	for (let i$1 = wrappers.length - 1; i$1 >= 0; i$1--) content = Fragment.from(wrappers[i$1].type.create(wrappers[i$1].attrs, content));
	tr$1.step(new ReplaceAroundStep(range.start - (joinBefore ? 2 : 0), range.end, range.start, range.end, new Slice(content, 0, 0), wrappers.length, true));
	let found$1 = 0;
	for (let i$1 = 0; i$1 < wrappers.length; i$1++) if (wrappers[i$1].type == listType) found$1 = i$1 + 1;
	let splitDepth = wrappers.length - found$1;
	let splitPos = range.start + wrappers.length - (joinBefore ? 2 : 0), parent = range.parent;
	for (let i$1 = range.startIndex, e = range.endIndex, first$1 = true; i$1 < e; i$1++, first$1 = false) {
		if (!first$1 && canSplit(tr$1.doc, splitPos, splitDepth)) {
			tr$1.split(splitPos, splitDepth);
			splitPos += 2 * splitDepth;
		}
		splitPos += parent.child(i$1).nodeSize;
	}
	return tr$1;
}
function liftListItem(itemType) {
	return function(state, dispatch) {
		let { $from, $to } = state.selection;
		let range = $from.blockRange($to, (node) => node.childCount > 0 && node.firstChild.type == itemType);
		if (!range) return false;
		if (!dispatch) return true;
		if ($from.node(range.depth - 1).type == itemType) return liftToOuterList(state, dispatch, itemType, range);
		else return liftOutOfList(state, dispatch, range);
	};
}
function liftToOuterList(state, dispatch, itemType, range) {
	let tr$1 = state.tr, end = range.end, endOfList = range.$to.end(range.depth);
	if (end < endOfList) {
		tr$1.step(new ReplaceAroundStep(end - 1, endOfList, end, endOfList, new Slice(Fragment.from(itemType.create(null, range.parent.copy())), 1, 0), 1, true));
		range = new NodeRange(tr$1.doc.resolve(range.$from.pos), tr$1.doc.resolve(endOfList), range.depth);
	}
	const target = liftTarget(range);
	if (target == null) return false;
	tr$1.lift(range, target);
	let $after = tr$1.doc.resolve(tr$1.mapping.map(end, -1) - 1);
	if (canJoin(tr$1.doc, $after.pos) && $after.nodeBefore.type == $after.nodeAfter.type) tr$1.join($after.pos);
	dispatch(tr$1.scrollIntoView());
	return true;
}
function liftOutOfList(state, dispatch, range) {
	let tr$1 = state.tr, list = range.parent;
	for (let pos = range.end, i$1 = range.endIndex - 1, e = range.startIndex; i$1 > e; i$1--) {
		pos -= list.child(i$1).nodeSize;
		tr$1.delete(pos - 1, pos + 1);
	}
	let $start = tr$1.doc.resolve(range.start), item = $start.nodeAfter;
	if (tr$1.mapping.map(range.end) != range.start + $start.nodeAfter.nodeSize) return false;
	let atStart = range.startIndex == 0, atEnd = range.endIndex == list.childCount;
	let parent = $start.node(-1), indexBefore = $start.index(-1);
	if (!parent.canReplace(indexBefore + (atStart ? 0 : 1), indexBefore + 1, item.content.append(atEnd ? Fragment.empty : Fragment.from(list)))) return false;
	let start = $start.pos, end = start + item.nodeSize;
	tr$1.step(new ReplaceAroundStep(start - (atStart ? 1 : 0), end + (atEnd ? 1 : 0), start + 1, end - 1, new Slice((atStart ? Fragment.empty : Fragment.from(list.copy(Fragment.empty))).append(atEnd ? Fragment.empty : Fragment.from(list.copy(Fragment.empty))), atStart ? 0 : 1, atEnd ? 0 : 1), atStart ? 0 : 1));
	dispatch(tr$1.scrollIntoView());
	return true;
}
function sinkListItem(itemType) {
	return function(state, dispatch) {
		let { $from, $to } = state.selection;
		let range = $from.blockRange($to, (node) => node.childCount > 0 && node.firstChild.type == itemType);
		if (!range) return false;
		let startIndex = range.startIndex;
		if (startIndex == 0) return false;
		let parent = range.parent, nodeBefore = parent.child(startIndex - 1);
		if (nodeBefore.type != itemType) return false;
		if (dispatch) {
			let nestedBefore = nodeBefore.lastChild && nodeBefore.lastChild.type == parent.type;
			let inner = Fragment.from(nestedBefore ? itemType.create() : null);
			let slice = new Slice(Fragment.from(itemType.create(null, Fragment.from(parent.type.create(null, inner)))), nestedBefore ? 3 : 1, 0);
			let before = range.start, after = range.end;
			dispatch(state.tr.step(new ReplaceAroundStep(before - (nestedBefore ? 3 : 1), after, before, after, slice, 1, true)).scrollIntoView());
		}
		return true;
	};
}
var domIndex = function(node) {
	for (var index = 0;; index++) {
		node = node.previousSibling;
		if (!node) return index;
	}
};
var parentNode = function(node) {
	let parent = node.assignedSlot || node.parentNode;
	return parent && parent.nodeType == 11 ? parent.host : parent;
};
var reusedRange = null;
var textRange = function(node, from, to) {
	let range = reusedRange || (reusedRange = document.createRange());
	range.setEnd(node, to == null ? node.nodeValue.length : to);
	range.setStart(node, from || 0);
	return range;
};
var clearReusedRange = function() {
	reusedRange = null;
};
var isEquivalentPosition = function(node, off, targetNode, targetOff) {
	return targetNode && (scanFor(node, off, targetNode, targetOff, -1) || scanFor(node, off, targetNode, targetOff, 1));
};
var atomElements = /^(img|br|input|textarea|hr)$/i;
function scanFor(node, off, targetNode, targetOff, dir) {
	var _a;
	for (;;) {
		if (node == targetNode && off == targetOff) return true;
		if (off == (dir < 0 ? 0 : nodeSize(node))) {
			let parent = node.parentNode;
			if (!parent || parent.nodeType != 1 || hasBlockDesc(node) || atomElements.test(node.nodeName) || node.contentEditable == "false") return false;
			off = domIndex(node) + (dir < 0 ? 0 : 1);
			node = parent;
		} else if (node.nodeType == 1) {
			let child = node.childNodes[off + (dir < 0 ? -1 : 0)];
			if (child.nodeType == 1 && child.contentEditable == "false") if ((_a = child.pmViewDesc) === null || _a === void 0 ? void 0 : _a.ignoreForSelection) off += dir;
			else return false;
			else {
				node = child;
				off = dir < 0 ? nodeSize(node) : 0;
			}
		} else return false;
	}
}
function nodeSize(node) {
	return node.nodeType == 3 ? node.nodeValue.length : node.childNodes.length;
}
function textNodeBefore$1(node, offset) {
	for (;;) {
		if (node.nodeType == 3 && offset) return node;
		if (node.nodeType == 1 && offset > 0) {
			if (node.contentEditable == "false") return null;
			node = node.childNodes[offset - 1];
			offset = nodeSize(node);
		} else if (node.parentNode && !hasBlockDesc(node)) {
			offset = domIndex(node);
			node = node.parentNode;
		} else return null;
	}
}
function textNodeAfter$1(node, offset) {
	for (;;) {
		if (node.nodeType == 3 && offset < node.nodeValue.length) return node;
		if (node.nodeType == 1 && offset < node.childNodes.length) {
			if (node.contentEditable == "false") return null;
			node = node.childNodes[offset];
			offset = 0;
		} else if (node.parentNode && !hasBlockDesc(node)) {
			offset = domIndex(node) + 1;
			node = node.parentNode;
		} else return null;
	}
}
function isOnEdge(node, offset, parent) {
	for (let atStart = offset == 0, atEnd = offset == nodeSize(node); atStart || atEnd;) {
		if (node == parent) return true;
		let index = domIndex(node);
		node = node.parentNode;
		if (!node) return false;
		atStart = atStart && index == 0;
		atEnd = atEnd && index == nodeSize(node);
	}
}
function hasBlockDesc(dom) {
	let desc;
	for (let cur = dom; cur; cur = cur.parentNode) if (desc = cur.pmViewDesc) break;
	return desc && desc.node && desc.node.isBlock && (desc.dom == dom || desc.contentDOM == dom);
}
var selectionCollapsed = function(domSel) {
	return domSel.focusNode && isEquivalentPosition(domSel.focusNode, domSel.focusOffset, domSel.anchorNode, domSel.anchorOffset);
};
function keyEvent(keyCode, key) {
	let event = document.createEvent("Event");
	event.initEvent("keydown", true, true);
	event.keyCode = keyCode;
	event.key = event.code = key;
	return event;
}
function deepActiveElement(doc$2) {
	let elt = doc$2.activeElement;
	while (elt && elt.shadowRoot) elt = elt.shadowRoot.activeElement;
	return elt;
}
function caretFromPoint(doc$2, x$1, y$1) {
	if (doc$2.caretPositionFromPoint) try {
		let pos = doc$2.caretPositionFromPoint(x$1, y$1);
		if (pos) return {
			node: pos.offsetNode,
			offset: Math.min(nodeSize(pos.offsetNode), pos.offset)
		};
	} catch (_$1) {}
	if (doc$2.caretRangeFromPoint) {
		let range = doc$2.caretRangeFromPoint(x$1, y$1);
		if (range) return {
			node: range.startContainer,
			offset: Math.min(nodeSize(range.startContainer), range.startOffset)
		};
	}
}
var nav = typeof navigator != "undefined" ? navigator : null;
var doc = typeof document != "undefined" ? document : null;
var agent = nav && nav.userAgent || "";
var ie_edge = /Edge\/(\d+)/.exec(agent);
var ie_upto10 = /MSIE \d/.exec(agent);
var ie_11up = /Trident\/(?:[7-9]|\d{2,})\..*rv:(\d+)/.exec(agent);
var ie$2 = !!(ie_upto10 || ie_11up || ie_edge);
var ie_version = ie_upto10 ? document.documentMode : ie_11up ? +ie_11up[1] : ie_edge ? +ie_edge[1] : 0;
var gecko = !ie$2 && /gecko\/(\d+)/i.test(agent);
gecko && +(/Firefox\/(\d+)/.exec(agent) || [0, 0])[1];
var _chrome = !ie$2 && /Chrome\/(\d+)/.exec(agent);
var chrome = !!_chrome;
var chrome_version = _chrome ? +_chrome[1] : 0;
var safari = !ie$2 && !!nav && /Apple Computer/.test(nav.vendor);
var ios = safari && (/Mobile\/\w+/.test(agent) || !!nav && nav.maxTouchPoints > 2);
var mac$2 = ios || (nav ? /Mac/.test(nav.platform) : false);
var windows$1 = nav ? /Win/.test(nav.platform) : false;
var android = /Android \d/.test(agent);
var webkit = !!doc && "webkitFontSmoothing" in doc.documentElement.style;
var webkit_version = webkit ? +(/\bAppleWebKit\/(\d+)/.exec(navigator.userAgent) || [0, 0])[1] : 0;
function windowRect(doc$2) {
	let vp = doc$2.defaultView && doc$2.defaultView.visualViewport;
	if (vp) return {
		left: 0,
		right: vp.width,
		top: 0,
		bottom: vp.height
	};
	return {
		left: 0,
		right: doc$2.documentElement.clientWidth,
		top: 0,
		bottom: doc$2.documentElement.clientHeight
	};
}
function getSide(value, side) {
	return typeof value == "number" ? value : value[side];
}
function clientRect(node) {
	let rect = node.getBoundingClientRect();
	let scaleX = rect.width / node.offsetWidth || 1;
	let scaleY = rect.height / node.offsetHeight || 1;
	return {
		left: rect.left,
		right: rect.left + node.clientWidth * scaleX,
		top: rect.top,
		bottom: rect.top + node.clientHeight * scaleY
	};
}
function scrollRectIntoView(view, rect, startDOM) {
	let scrollThreshold = view.someProp("scrollThreshold") || 0, scrollMargin = view.someProp("scrollMargin") || 5;
	let doc$2 = view.dom.ownerDocument;
	for (let parent = startDOM || view.dom;;) {
		if (!parent) break;
		if (parent.nodeType != 1) {
			parent = parentNode(parent);
			continue;
		}
		let elt = parent;
		let atTop = elt == doc$2.body;
		let bounding = atTop ? windowRect(doc$2) : clientRect(elt);
		let moveX = 0, moveY = 0;
		if (rect.top < bounding.top + getSide(scrollThreshold, "top")) moveY = -(bounding.top - rect.top + getSide(scrollMargin, "top"));
		else if (rect.bottom > bounding.bottom - getSide(scrollThreshold, "bottom")) moveY = rect.bottom - rect.top > bounding.bottom - bounding.top ? rect.top + getSide(scrollMargin, "top") - bounding.top : rect.bottom - bounding.bottom + getSide(scrollMargin, "bottom");
		if (rect.left < bounding.left + getSide(scrollThreshold, "left")) moveX = -(bounding.left - rect.left + getSide(scrollMargin, "left"));
		else if (rect.right > bounding.right - getSide(scrollThreshold, "right")) moveX = rect.right - bounding.right + getSide(scrollMargin, "right");
		if (moveX || moveY) if (atTop) doc$2.defaultView.scrollBy(moveX, moveY);
		else {
			let startX = elt.scrollLeft, startY = elt.scrollTop;
			if (moveY) elt.scrollTop += moveY;
			if (moveX) elt.scrollLeft += moveX;
			let dX = elt.scrollLeft - startX, dY = elt.scrollTop - startY;
			rect = {
				left: rect.left - dX,
				top: rect.top - dY,
				right: rect.right - dX,
				bottom: rect.bottom - dY
			};
		}
		let pos = atTop ? "fixed" : getComputedStyle(parent).position;
		if (/^(fixed|sticky)$/.test(pos)) break;
		parent = pos == "absolute" ? parent.offsetParent : parentNode(parent);
	}
}
function storeScrollPos(view) {
	let rect = view.dom.getBoundingClientRect(), startY = Math.max(0, rect.top);
	let refDOM, refTop;
	for (let x$1 = (rect.left + rect.right) / 2, y$1 = startY + 1; y$1 < Math.min(innerHeight, rect.bottom); y$1 += 5) {
		let dom = view.root.elementFromPoint(x$1, y$1);
		if (!dom || dom == view.dom || !view.dom.contains(dom)) continue;
		let localRect = dom.getBoundingClientRect();
		if (localRect.top >= startY - 20) {
			refDOM = dom;
			refTop = localRect.top;
			break;
		}
	}
	return {
		refDOM,
		refTop,
		stack: scrollStack(view.dom)
	};
}
function scrollStack(dom) {
	let stack = [], doc$2 = dom.ownerDocument;
	for (let cur = dom; cur; cur = parentNode(cur)) {
		stack.push({
			dom: cur,
			top: cur.scrollTop,
			left: cur.scrollLeft
		});
		if (dom == doc$2) break;
	}
	return stack;
}
function resetScrollPos({ refDOM, refTop, stack }) {
	let newRefTop = refDOM ? refDOM.getBoundingClientRect().top : 0;
	restoreScrollStack(stack, newRefTop == 0 ? 0 : newRefTop - refTop);
}
function restoreScrollStack(stack, dTop) {
	for (let i$1 = 0; i$1 < stack.length; i$1++) {
		let { dom, top, left } = stack[i$1];
		if (dom.scrollTop != top + dTop) dom.scrollTop = top + dTop;
		if (dom.scrollLeft != left) dom.scrollLeft = left;
	}
}
var preventScrollSupported = null;
function focusPreventScroll(dom) {
	if (dom.setActive) return dom.setActive();
	if (preventScrollSupported) return dom.focus(preventScrollSupported);
	let stored = scrollStack(dom);
	dom.focus(preventScrollSupported == null ? { get preventScroll() {
		preventScrollSupported = { preventScroll: true };
		return true;
	} } : void 0);
	if (!preventScrollSupported) {
		preventScrollSupported = false;
		restoreScrollStack(stored, 0);
	}
}
function findOffsetInNode(node, coords) {
	let closest, dxClosest = 2e8, coordsClosest, offset = 0;
	let rowBot = coords.top, rowTop = coords.top;
	let firstBelow, coordsBelow;
	for (let child = node.firstChild, childIndex = 0; child; child = child.nextSibling, childIndex++) {
		let rects;
		if (child.nodeType == 1) rects = child.getClientRects();
		else if (child.nodeType == 3) rects = textRange(child).getClientRects();
		else continue;
		for (let i$1 = 0; i$1 < rects.length; i$1++) {
			let rect = rects[i$1];
			if (rect.top <= rowBot && rect.bottom >= rowTop) {
				rowBot = Math.max(rect.bottom, rowBot);
				rowTop = Math.min(rect.top, rowTop);
				let dx = rect.left > coords.left ? rect.left - coords.left : rect.right < coords.left ? coords.left - rect.right : 0;
				if (dx < dxClosest) {
					closest = child;
					dxClosest = dx;
					coordsClosest = dx && closest.nodeType == 3 ? {
						left: rect.right < coords.left ? rect.right : rect.left,
						top: coords.top
					} : coords;
					if (child.nodeType == 1 && dx) offset = childIndex + (coords.left >= (rect.left + rect.right) / 2 ? 1 : 0);
					continue;
				}
			} else if (rect.top > coords.top && !firstBelow && rect.left <= coords.left && rect.right >= coords.left) {
				firstBelow = child;
				coordsBelow = {
					left: Math.max(rect.left, Math.min(rect.right, coords.left)),
					top: rect.top
				};
			}
			if (!closest && (coords.left >= rect.right && coords.top >= rect.top || coords.left >= rect.left && coords.top >= rect.bottom)) offset = childIndex + 1;
		}
	}
	if (!closest && firstBelow) {
		closest = firstBelow;
		coordsClosest = coordsBelow;
		dxClosest = 0;
	}
	if (closest && closest.nodeType == 3) return findOffsetInText(closest, coordsClosest);
	if (!closest || dxClosest && closest.nodeType == 1) return {
		node,
		offset
	};
	return findOffsetInNode(closest, coordsClosest);
}
function findOffsetInText(node, coords) {
	let len = node.nodeValue.length;
	let range = document.createRange(), result;
	for (let i$1 = 0; i$1 < len; i$1++) {
		range.setEnd(node, i$1 + 1);
		range.setStart(node, i$1);
		let rect = singleRect(range, 1);
		if (rect.top == rect.bottom) continue;
		if (inRect(coords, rect)) {
			result = {
				node,
				offset: i$1 + (coords.left >= (rect.left + rect.right) / 2 ? 1 : 0)
			};
			break;
		}
	}
	range.detach();
	return result || {
		node,
		offset: 0
	};
}
function inRect(coords, rect) {
	return coords.left >= rect.left - 1 && coords.left <= rect.right + 1 && coords.top >= rect.top - 1 && coords.top <= rect.bottom + 1;
}
function targetKludge(dom, coords) {
	let parent = dom.parentNode;
	if (parent && /^li$/i.test(parent.nodeName) && coords.left < dom.getBoundingClientRect().left) return parent;
	return dom;
}
function posFromElement(view, elt, coords) {
	let { node, offset } = findOffsetInNode(elt, coords), bias = -1;
	if (node.nodeType == 1 && !node.firstChild) {
		let rect = node.getBoundingClientRect();
		bias = rect.left != rect.right && coords.left > (rect.left + rect.right) / 2 ? 1 : -1;
	}
	return view.docView.posFromDOM(node, offset, bias);
}
function posFromCaret(view, node, offset, coords) {
	let outsideBlock = -1;
	for (let cur = node, sawBlock = false;;) {
		if (cur == view.dom) break;
		let desc = view.docView.nearestDesc(cur, true), rect;
		if (!desc) return null;
		if (desc.dom.nodeType == 1 && (desc.node.isBlock && desc.parent || !desc.contentDOM) && ((rect = desc.dom.getBoundingClientRect()).width || rect.height)) {
			if (desc.node.isBlock && desc.parent && !/^T(R|BODY|HEAD|FOOT)$/.test(desc.dom.nodeName)) {
				if (!sawBlock && rect.left > coords.left || rect.top > coords.top) outsideBlock = desc.posBefore;
				else if (!sawBlock && rect.right < coords.left || rect.bottom < coords.top) outsideBlock = desc.posAfter;
				sawBlock = true;
			}
			if (!desc.contentDOM && outsideBlock < 0 && !desc.node.isText) return (desc.node.isBlock ? coords.top < (rect.top + rect.bottom) / 2 : coords.left < (rect.left + rect.right) / 2) ? desc.posBefore : desc.posAfter;
		}
		cur = desc.dom.parentNode;
	}
	return outsideBlock > -1 ? outsideBlock : view.docView.posFromDOM(node, offset, -1);
}
function elementFromPoint(element, coords, box) {
	let len = element.childNodes.length;
	if (len && box.top < box.bottom) for (let startI = Math.max(0, Math.min(len - 1, Math.floor(len * (coords.top - box.top) / (box.bottom - box.top)) - 2)), i$1 = startI;;) {
		let child = element.childNodes[i$1];
		if (child.nodeType == 1) {
			let rects = child.getClientRects();
			for (let j$1 = 0; j$1 < rects.length; j$1++) {
				let rect = rects[j$1];
				if (inRect(coords, rect)) return elementFromPoint(child, coords, rect);
			}
		}
		if ((i$1 = (i$1 + 1) % len) == startI) break;
	}
	return element;
}
function posAtCoords(view, coords) {
	let doc$2 = view.dom.ownerDocument, node, offset = 0;
	let caret = caretFromPoint(doc$2, coords.left, coords.top);
	if (caret) ({node, offset} = caret);
	let elt = (view.root.elementFromPoint ? view.root : doc$2).elementFromPoint(coords.left, coords.top);
	let pos;
	if (!elt || !view.dom.contains(elt.nodeType != 1 ? elt.parentNode : elt)) {
		let box = view.dom.getBoundingClientRect();
		if (!inRect(coords, box)) return null;
		elt = elementFromPoint(view.dom, coords, box);
		if (!elt) return null;
	}
	if (safari) {
		for (let p = elt; node && p; p = parentNode(p)) if (p.draggable) node = void 0;
	}
	elt = targetKludge(elt, coords);
	if (node) {
		if (gecko && node.nodeType == 1) {
			offset = Math.min(offset, node.childNodes.length);
			if (offset < node.childNodes.length) {
				let next = node.childNodes[offset], box;
				if (next.nodeName == "IMG" && (box = next.getBoundingClientRect()).right <= coords.left && box.bottom > coords.top) offset++;
			}
		}
		let prev;
		if (webkit && offset && node.nodeType == 1 && (prev = node.childNodes[offset - 1]).nodeType == 1 && prev.contentEditable == "false" && prev.getBoundingClientRect().top >= coords.top) offset--;
		if (node == view.dom && offset == node.childNodes.length - 1 && node.lastChild.nodeType == 1 && coords.top > node.lastChild.getBoundingClientRect().bottom) pos = view.state.doc.content.size;
		else if (offset == 0 || node.nodeType != 1 || node.childNodes[offset - 1].nodeName != "BR") pos = posFromCaret(view, node, offset, coords);
	}
	if (pos == null) pos = posFromElement(view, elt, coords);
	let desc = view.docView.nearestDesc(elt, true);
	return {
		pos,
		inside: desc ? desc.posAtStart - desc.border : -1
	};
}
function nonZero(rect) {
	return rect.top < rect.bottom || rect.left < rect.right;
}
function singleRect(target, bias) {
	let rects = target.getClientRects();
	if (rects.length) {
		let first$1 = rects[bias < 0 ? 0 : rects.length - 1];
		if (nonZero(first$1)) return first$1;
	}
	return Array.prototype.find.call(rects, nonZero) || target.getBoundingClientRect();
}
var BIDI = /[\u0590-\u05f4\u0600-\u06ff\u0700-\u08ac]/;
function coordsAtPos(view, pos, side) {
	let { node, offset, atom } = view.docView.domFromPos(pos, side < 0 ? -1 : 1);
	let supportEmptyRange = webkit || gecko;
	if (node.nodeType == 3) if (supportEmptyRange && (BIDI.test(node.nodeValue) || (side < 0 ? !offset : offset == node.nodeValue.length))) {
		let rect = singleRect(textRange(node, offset, offset), side);
		if (gecko && offset && /\s/.test(node.nodeValue[offset - 1]) && offset < node.nodeValue.length) {
			let rectBefore = singleRect(textRange(node, offset - 1, offset - 1), -1);
			if (rectBefore.top == rect.top) {
				let rectAfter = singleRect(textRange(node, offset, offset + 1), -1);
				if (rectAfter.top != rect.top) return flattenV(rectAfter, rectAfter.left < rectBefore.left);
			}
		}
		return rect;
	} else {
		let from = offset, to = offset, takeSide = side < 0 ? 1 : -1;
		if (side < 0 && !offset) {
			to++;
			takeSide = -1;
		} else if (side >= 0 && offset == node.nodeValue.length) {
			from--;
			takeSide = 1;
		} else if (side < 0) from--;
		else to++;
		return flattenV(singleRect(textRange(node, from, to), takeSide), takeSide < 0);
	}
	if (!view.state.doc.resolve(pos - (atom || 0)).parent.inlineContent) {
		if (atom == null && offset && (side < 0 || offset == nodeSize(node))) {
			let before = node.childNodes[offset - 1];
			if (before.nodeType == 1) return flattenH(before.getBoundingClientRect(), false);
		}
		if (atom == null && offset < nodeSize(node)) {
			let after = node.childNodes[offset];
			if (after.nodeType == 1) return flattenH(after.getBoundingClientRect(), true);
		}
		return flattenH(node.getBoundingClientRect(), side >= 0);
	}
	if (atom == null && offset && (side < 0 || offset == nodeSize(node))) {
		let before = node.childNodes[offset - 1];
		let target = before.nodeType == 3 ? textRange(before, nodeSize(before) - (supportEmptyRange ? 0 : 1)) : before.nodeType == 1 && (before.nodeName != "BR" || !before.nextSibling) ? before : null;
		if (target) return flattenV(singleRect(target, 1), false);
	}
	if (atom == null && offset < nodeSize(node)) {
		let after = node.childNodes[offset];
		while (after.pmViewDesc && after.pmViewDesc.ignoreForCoords) after = after.nextSibling;
		let target = !after ? null : after.nodeType == 3 ? textRange(after, 0, supportEmptyRange ? 0 : 1) : after.nodeType == 1 ? after : null;
		if (target) return flattenV(singleRect(target, -1), true);
	}
	return flattenV(singleRect(node.nodeType == 3 ? textRange(node) : node, -side), side >= 0);
}
function flattenV(rect, left) {
	if (rect.width == 0) return rect;
	let x$1 = left ? rect.left : rect.right;
	return {
		top: rect.top,
		bottom: rect.bottom,
		left: x$1,
		right: x$1
	};
}
function flattenH(rect, top) {
	if (rect.height == 0) return rect;
	let y$1 = top ? rect.top : rect.bottom;
	return {
		top: y$1,
		bottom: y$1,
		left: rect.left,
		right: rect.right
	};
}
function withFlushedState(view, state, f) {
	let viewState = view.state, active = view.root.activeElement;
	if (viewState != state) view.updateState(state);
	if (active != view.dom) view.focus();
	try {
		return f();
	} finally {
		if (viewState != state) view.updateState(viewState);
		if (active != view.dom && active) active.focus();
	}
}
function endOfTextblockVertical(view, state, dir) {
	let sel = state.selection;
	let $pos = dir == "up" ? sel.$from : sel.$to;
	return withFlushedState(view, state, () => {
		let { node: dom } = view.docView.domFromPos($pos.pos, dir == "up" ? -1 : 1);
		for (;;) {
			let nearest = view.docView.nearestDesc(dom, true);
			if (!nearest) break;
			if (nearest.node.isBlock) {
				dom = nearest.contentDOM || nearest.dom;
				break;
			}
			dom = nearest.dom.parentNode;
		}
		let coords = coordsAtPos(view, $pos.pos, 1);
		for (let child = dom.firstChild; child; child = child.nextSibling) {
			let boxes;
			if (child.nodeType == 1) boxes = child.getClientRects();
			else if (child.nodeType == 3) boxes = textRange(child, 0, child.nodeValue.length).getClientRects();
			else continue;
			for (let i$1 = 0; i$1 < boxes.length; i$1++) {
				let box = boxes[i$1];
				if (box.bottom > box.top + 1 && (dir == "up" ? coords.top - box.top > (box.bottom - coords.top) * 2 : box.bottom - coords.bottom > (coords.bottom - box.top) * 2)) return false;
			}
		}
		return true;
	});
}
var maybeRTL = /[\u0590-\u08ac]/;
function endOfTextblockHorizontal(view, state, dir) {
	let { $head } = state.selection;
	if (!$head.parent.isTextblock) return false;
	let offset = $head.parentOffset, atStart = !offset, atEnd = offset == $head.parent.content.size;
	let sel = view.domSelection();
	if (!sel) return $head.pos == $head.start() || $head.pos == $head.end();
	if (!maybeRTL.test($head.parent.textContent) || !sel.modify) return dir == "left" || dir == "backward" ? atStart : atEnd;
	return withFlushedState(view, state, () => {
		let { focusNode: oldNode, focusOffset: oldOff, anchorNode, anchorOffset } = view.domSelectionRange();
		let oldBidiLevel = sel.caretBidiLevel;
		sel.modify("move", dir, "character");
		let parentDOM = $head.depth ? view.docView.domAfterPos($head.before()) : view.dom;
		let { focusNode: newNode, focusOffset: newOff } = view.domSelectionRange();
		let result = newNode && !parentDOM.contains(newNode.nodeType == 1 ? newNode : newNode.parentNode) || oldNode == newNode && oldOff == newOff;
		try {
			sel.collapse(anchorNode, anchorOffset);
			if (oldNode && (oldNode != anchorNode || oldOff != anchorOffset) && sel.extend) sel.extend(oldNode, oldOff);
		} catch (_$1) {}
		if (oldBidiLevel != null) sel.caretBidiLevel = oldBidiLevel;
		return result;
	});
}
var cachedState = null;
var cachedDir = null;
var cachedResult = false;
function endOfTextblock(view, state, dir) {
	if (cachedState == state && cachedDir == dir) return cachedResult;
	cachedState = state;
	cachedDir = dir;
	return cachedResult = dir == "up" || dir == "down" ? endOfTextblockVertical(view, state, dir) : endOfTextblockHorizontal(view, state, dir);
}
var NOT_DIRTY = 0, CHILD_DIRTY = 1, CONTENT_DIRTY = 2, NODE_DIRTY = 3;
var ViewDesc = class {
	constructor(parent, children, dom, contentDOM) {
		this.parent = parent;
		this.children = children;
		this.dom = dom;
		this.contentDOM = contentDOM;
		this.dirty = NOT_DIRTY;
		dom.pmViewDesc = this;
	}
	matchesWidget(widget) {
		return false;
	}
	matchesMark(mark) {
		return false;
	}
	matchesNode(node, outerDeco, innerDeco) {
		return false;
	}
	matchesHack(nodeName) {
		return false;
	}
	parseRule() {
		return null;
	}
	stopEvent(event) {
		return false;
	}
	get size() {
		let size = 0;
		for (let i$1 = 0; i$1 < this.children.length; i$1++) size += this.children[i$1].size;
		return size;
	}
	get border() {
		return 0;
	}
	destroy() {
		this.parent = void 0;
		if (this.dom.pmViewDesc == this) this.dom.pmViewDesc = void 0;
		for (let i$1 = 0; i$1 < this.children.length; i$1++) this.children[i$1].destroy();
	}
	posBeforeChild(child) {
		for (let i$1 = 0, pos = this.posAtStart;; i$1++) {
			let cur = this.children[i$1];
			if (cur == child) return pos;
			pos += cur.size;
		}
	}
	get posBefore() {
		return this.parent.posBeforeChild(this);
	}
	get posAtStart() {
		return this.parent ? this.parent.posBeforeChild(this) + this.border : 0;
	}
	get posAfter() {
		return this.posBefore + this.size;
	}
	get posAtEnd() {
		return this.posAtStart + this.size - 2 * this.border;
	}
	localPosFromDOM(dom, offset, bias) {
		if (this.contentDOM && this.contentDOM.contains(dom.nodeType == 1 ? dom : dom.parentNode)) if (bias < 0) {
			let domBefore, desc;
			if (dom == this.contentDOM) domBefore = dom.childNodes[offset - 1];
			else {
				while (dom.parentNode != this.contentDOM) dom = dom.parentNode;
				domBefore = dom.previousSibling;
			}
			while (domBefore && !((desc = domBefore.pmViewDesc) && desc.parent == this)) domBefore = domBefore.previousSibling;
			return domBefore ? this.posBeforeChild(desc) + desc.size : this.posAtStart;
		} else {
			let domAfter, desc;
			if (dom == this.contentDOM) domAfter = dom.childNodes[offset];
			else {
				while (dom.parentNode != this.contentDOM) dom = dom.parentNode;
				domAfter = dom.nextSibling;
			}
			while (domAfter && !((desc = domAfter.pmViewDesc) && desc.parent == this)) domAfter = domAfter.nextSibling;
			return domAfter ? this.posBeforeChild(desc) : this.posAtEnd;
		}
		let atEnd;
		if (dom == this.dom && this.contentDOM) atEnd = offset > domIndex(this.contentDOM);
		else if (this.contentDOM && this.contentDOM != this.dom && this.dom.contains(this.contentDOM)) atEnd = dom.compareDocumentPosition(this.contentDOM) & 2;
		else if (this.dom.firstChild) {
			if (offset == 0) for (let search = dom;; search = search.parentNode) {
				if (search == this.dom) {
					atEnd = false;
					break;
				}
				if (search.previousSibling) break;
			}
			if (atEnd == null && offset == dom.childNodes.length) for (let search = dom;; search = search.parentNode) {
				if (search == this.dom) {
					atEnd = true;
					break;
				}
				if (search.nextSibling) break;
			}
		}
		return (atEnd == null ? bias > 0 : atEnd) ? this.posAtEnd : this.posAtStart;
	}
	nearestDesc(dom, onlyNodes = false) {
		for (let first$1 = true, cur = dom; cur; cur = cur.parentNode) {
			let desc = this.getDesc(cur), nodeDOM;
			if (desc && (!onlyNodes || desc.node)) if (first$1 && (nodeDOM = desc.nodeDOM) && !(nodeDOM.nodeType == 1 ? nodeDOM.contains(dom.nodeType == 1 ? dom : dom.parentNode) : nodeDOM == dom)) first$1 = false;
			else return desc;
		}
	}
	getDesc(dom) {
		let desc = dom.pmViewDesc;
		for (let cur = desc; cur; cur = cur.parent) if (cur == this) return desc;
	}
	posFromDOM(dom, offset, bias) {
		for (let scan = dom; scan; scan = scan.parentNode) {
			let desc = this.getDesc(scan);
			if (desc) return desc.localPosFromDOM(dom, offset, bias);
		}
		return -1;
	}
	descAt(pos) {
		for (let i$1 = 0, offset = 0; i$1 < this.children.length; i$1++) {
			let child = this.children[i$1], end = offset + child.size;
			if (offset == pos && end != offset) {
				while (!child.border && child.children.length) for (let i$2 = 0; i$2 < child.children.length; i$2++) {
					let inner = child.children[i$2];
					if (inner.size) {
						child = inner;
						break;
					}
				}
				return child;
			}
			if (pos < end) return child.descAt(pos - offset - child.border);
			offset = end;
		}
	}
	domFromPos(pos, side) {
		if (!this.contentDOM) return {
			node: this.dom,
			offset: 0,
			atom: pos + 1
		};
		let i$1 = 0, offset = 0;
		for (let curPos = 0; i$1 < this.children.length; i$1++) {
			let child = this.children[i$1], end = curPos + child.size;
			if (end > pos || child instanceof TrailingHackViewDesc) {
				offset = pos - curPos;
				break;
			}
			curPos = end;
		}
		if (offset) return this.children[i$1].domFromPos(offset - this.children[i$1].border, side);
		for (let prev; i$1 && !(prev = this.children[i$1 - 1]).size && prev instanceof WidgetViewDesc && prev.side >= 0; i$1--);
		if (side <= 0) {
			let prev, enter$1 = true;
			for (;; i$1--, enter$1 = false) {
				prev = i$1 ? this.children[i$1 - 1] : null;
				if (!prev || prev.dom.parentNode == this.contentDOM) break;
			}
			if (prev && side && enter$1 && !prev.border && !prev.domAtom) return prev.domFromPos(prev.size, side);
			return {
				node: this.contentDOM,
				offset: prev ? domIndex(prev.dom) + 1 : 0
			};
		} else {
			let next, enter$1 = true;
			for (;; i$1++, enter$1 = false) {
				next = i$1 < this.children.length ? this.children[i$1] : null;
				if (!next || next.dom.parentNode == this.contentDOM) break;
			}
			if (next && enter$1 && !next.border && !next.domAtom) return next.domFromPos(0, side);
			return {
				node: this.contentDOM,
				offset: next ? domIndex(next.dom) : this.contentDOM.childNodes.length
			};
		}
	}
	parseRange(from, to, base$1 = 0) {
		if (this.children.length == 0) return {
			node: this.contentDOM,
			from,
			to,
			fromOffset: 0,
			toOffset: this.contentDOM.childNodes.length
		};
		let fromOffset = -1, toOffset = -1;
		for (let offset = base$1, i$1 = 0;; i$1++) {
			let child = this.children[i$1], end = offset + child.size;
			if (fromOffset == -1 && from <= end) {
				let childBase = offset + child.border;
				if (from >= childBase && to <= end - child.border && child.node && child.contentDOM && this.contentDOM.contains(child.contentDOM)) return child.parseRange(from, to, childBase);
				from = offset;
				for (let j$1 = i$1; j$1 > 0; j$1--) {
					let prev = this.children[j$1 - 1];
					if (prev.size && prev.dom.parentNode == this.contentDOM && !prev.emptyChildAt(1)) {
						fromOffset = domIndex(prev.dom) + 1;
						break;
					}
					from -= prev.size;
				}
				if (fromOffset == -1) fromOffset = 0;
			}
			if (fromOffset > -1 && (end > to || i$1 == this.children.length - 1)) {
				to = end;
				for (let j$1 = i$1 + 1; j$1 < this.children.length; j$1++) {
					let next = this.children[j$1];
					if (next.size && next.dom.parentNode == this.contentDOM && !next.emptyChildAt(-1)) {
						toOffset = domIndex(next.dom);
						break;
					}
					to += next.size;
				}
				if (toOffset == -1) toOffset = this.contentDOM.childNodes.length;
				break;
			}
			offset = end;
		}
		return {
			node: this.contentDOM,
			from,
			to,
			fromOffset,
			toOffset
		};
	}
	emptyChildAt(side) {
		if (this.border || !this.contentDOM || !this.children.length) return false;
		let child = this.children[side < 0 ? 0 : this.children.length - 1];
		return child.size == 0 || child.emptyChildAt(side);
	}
	domAfterPos(pos) {
		let { node, offset } = this.domFromPos(pos, 0);
		if (node.nodeType != 1 || offset == node.childNodes.length) throw new RangeError("No node after pos " + pos);
		return node.childNodes[offset];
	}
	setSelection(anchor, head, view, force = false) {
		let from = Math.min(anchor, head), to = Math.max(anchor, head);
		for (let i$1 = 0, offset = 0; i$1 < this.children.length; i$1++) {
			let child = this.children[i$1], end = offset + child.size;
			if (from > offset && to < end) return child.setSelection(anchor - offset - child.border, head - offset - child.border, view, force);
			offset = end;
		}
		let anchorDOM = this.domFromPos(anchor, anchor ? -1 : 1);
		let headDOM = head == anchor ? anchorDOM : this.domFromPos(head, head ? -1 : 1);
		let domSel = view.root.getSelection();
		let selRange = view.domSelectionRange();
		let brKludge = false;
		if ((gecko || safari) && anchor == head) {
			let { node, offset } = anchorDOM;
			if (node.nodeType == 3) {
				brKludge = !!(offset && node.nodeValue[offset - 1] == "\n");
				if (brKludge && offset == node.nodeValue.length) for (let scan = node, after; scan; scan = scan.parentNode) {
					if (after = scan.nextSibling) {
						if (after.nodeName == "BR") anchorDOM = headDOM = {
							node: after.parentNode,
							offset: domIndex(after) + 1
						};
						break;
					}
					let desc = scan.pmViewDesc;
					if (desc && desc.node && desc.node.isBlock) break;
				}
			} else {
				let prev = node.childNodes[offset - 1];
				brKludge = prev && (prev.nodeName == "BR" || prev.contentEditable == "false");
			}
		}
		if (gecko && selRange.focusNode && selRange.focusNode != headDOM.node && selRange.focusNode.nodeType == 1) {
			let after = selRange.focusNode.childNodes[selRange.focusOffset];
			if (after && after.contentEditable == "false") force = true;
		}
		if (!(force || brKludge && safari) && isEquivalentPosition(anchorDOM.node, anchorDOM.offset, selRange.anchorNode, selRange.anchorOffset) && isEquivalentPosition(headDOM.node, headDOM.offset, selRange.focusNode, selRange.focusOffset)) return;
		let domSelExtended = false;
		if ((domSel.extend || anchor == head) && !(brKludge && gecko)) {
			domSel.collapse(anchorDOM.node, anchorDOM.offset);
			try {
				if (anchor != head) domSel.extend(headDOM.node, headDOM.offset);
				domSelExtended = true;
			} catch (_$1) {}
		}
		if (!domSelExtended) {
			if (anchor > head) {
				let tmp = anchorDOM;
				anchorDOM = headDOM;
				headDOM = tmp;
			}
			let range = document.createRange();
			range.setEnd(headDOM.node, headDOM.offset);
			range.setStart(anchorDOM.node, anchorDOM.offset);
			domSel.removeAllRanges();
			domSel.addRange(range);
		}
	}
	ignoreMutation(mutation) {
		return !this.contentDOM && mutation.type != "selection";
	}
	get contentLost() {
		return this.contentDOM && this.contentDOM != this.dom && !this.dom.contains(this.contentDOM);
	}
	markDirty(from, to) {
		for (let offset = 0, i$1 = 0; i$1 < this.children.length; i$1++) {
			let child = this.children[i$1], end = offset + child.size;
			if (offset == end ? from <= end && to >= offset : from < end && to > offset) {
				let startInside = offset + child.border, endInside = end - child.border;
				if (from >= startInside && to <= endInside) {
					this.dirty = from == offset || to == end ? CONTENT_DIRTY : CHILD_DIRTY;
					if (from == startInside && to == endInside && (child.contentLost || child.dom.parentNode != this.contentDOM)) child.dirty = NODE_DIRTY;
					else child.markDirty(from - startInside, to - startInside);
					return;
				} else child.dirty = child.dom == child.contentDOM && child.dom.parentNode == this.contentDOM && !child.children.length ? CONTENT_DIRTY : NODE_DIRTY;
			}
			offset = end;
		}
		this.dirty = CONTENT_DIRTY;
	}
	markParentsDirty() {
		let level = 1;
		for (let node = this.parent; node; node = node.parent, level++) {
			let dirty = level == 1 ? CONTENT_DIRTY : CHILD_DIRTY;
			if (node.dirty < dirty) node.dirty = dirty;
		}
	}
	get domAtom() {
		return false;
	}
	get ignoreForCoords() {
		return false;
	}
	get ignoreForSelection() {
		return false;
	}
	isText(text) {
		return false;
	}
};
var WidgetViewDesc = class extends ViewDesc {
	constructor(parent, widget, view, pos) {
		let self, dom = widget.type.toDOM;
		if (typeof dom == "function") dom = dom(view, () => {
			if (!self) return pos;
			if (self.parent) return self.parent.posBeforeChild(self);
		});
		if (!widget.type.spec.raw) {
			if (dom.nodeType != 1) {
				let wrap$1 = document.createElement("span");
				wrap$1.appendChild(dom);
				dom = wrap$1;
			}
			dom.contentEditable = "false";
			dom.classList.add("ProseMirror-widget");
		}
		super(parent, [], dom, null);
		this.widget = widget;
		this.widget = widget;
		self = this;
	}
	matchesWidget(widget) {
		return this.dirty == NOT_DIRTY && widget.type.eq(this.widget.type);
	}
	parseRule() {
		return { ignore: true };
	}
	stopEvent(event) {
		let stop = this.widget.spec.stopEvent;
		return stop ? stop(event) : false;
	}
	ignoreMutation(mutation) {
		return mutation.type != "selection" || this.widget.spec.ignoreSelection;
	}
	destroy() {
		this.widget.type.destroy(this.dom);
		super.destroy();
	}
	get domAtom() {
		return true;
	}
	get ignoreForSelection() {
		return !!this.widget.type.spec.relaxedSide;
	}
	get side() {
		return this.widget.type.side;
	}
};
var CompositionViewDesc = class extends ViewDesc {
	constructor(parent, dom, textDOM, text) {
		super(parent, [], dom, null);
		this.textDOM = textDOM;
		this.text = text;
	}
	get size() {
		return this.text.length;
	}
	localPosFromDOM(dom, offset) {
		if (dom != this.textDOM) return this.posAtStart + (offset ? this.size : 0);
		return this.posAtStart + offset;
	}
	domFromPos(pos) {
		return {
			node: this.textDOM,
			offset: pos
		};
	}
	ignoreMutation(mut) {
		return mut.type === "characterData" && mut.target.nodeValue == mut.oldValue;
	}
};
var MarkViewDesc = class MarkViewDesc extends ViewDesc {
	constructor(parent, mark, dom, contentDOM, spec) {
		super(parent, [], dom, contentDOM);
		this.mark = mark;
		this.spec = spec;
	}
	static create(parent, mark, inline, view) {
		let custom = view.nodeViews[mark.type.name];
		let spec = custom && custom(mark, view, inline);
		if (!spec || !spec.dom) spec = DOMSerializer.renderSpec(document, mark.type.spec.toDOM(mark, inline), null, mark.attrs);
		return new MarkViewDesc(parent, mark, spec.dom, spec.contentDOM || spec.dom, spec);
	}
	parseRule() {
		if (this.dirty & NODE_DIRTY || this.mark.type.spec.reparseInView) return null;
		return {
			mark: this.mark.type.name,
			attrs: this.mark.attrs,
			contentElement: this.contentDOM
		};
	}
	matchesMark(mark) {
		return this.dirty != NODE_DIRTY && this.mark.eq(mark);
	}
	markDirty(from, to) {
		super.markDirty(from, to);
		if (this.dirty != NOT_DIRTY) {
			let parent = this.parent;
			while (!parent.node) parent = parent.parent;
			if (parent.dirty < this.dirty) parent.dirty = this.dirty;
			this.dirty = NOT_DIRTY;
		}
	}
	slice(from, to, view) {
		let copy$1 = MarkViewDesc.create(this.parent, this.mark, true, view);
		let nodes = this.children, size = this.size;
		if (to < size) nodes = replaceNodes(nodes, to, size, view);
		if (from > 0) nodes = replaceNodes(nodes, 0, from, view);
		for (let i$1 = 0; i$1 < nodes.length; i$1++) nodes[i$1].parent = copy$1;
		copy$1.children = nodes;
		return copy$1;
	}
	ignoreMutation(mutation) {
		return this.spec.ignoreMutation ? this.spec.ignoreMutation(mutation) : super.ignoreMutation(mutation);
	}
	destroy() {
		if (this.spec.destroy) this.spec.destroy();
		super.destroy();
	}
};
var NodeViewDesc = class NodeViewDesc extends ViewDesc {
	constructor(parent, node, outerDeco, innerDeco, dom, contentDOM, nodeDOM, view, pos) {
		super(parent, [], dom, contentDOM);
		this.node = node;
		this.outerDeco = outerDeco;
		this.innerDeco = innerDeco;
		this.nodeDOM = nodeDOM;
	}
	static create(parent, node, outerDeco, innerDeco, view, pos) {
		let custom = view.nodeViews[node.type.name], descObj;
		let spec = custom && custom(node, view, () => {
			if (!descObj) return pos;
			if (descObj.parent) return descObj.parent.posBeforeChild(descObj);
		}, outerDeco, innerDeco);
		let dom = spec && spec.dom, contentDOM = spec && spec.contentDOM;
		if (node.isText) {
			if (!dom) dom = document.createTextNode(node.text);
			else if (dom.nodeType != 3) throw new RangeError("Text must be rendered as a DOM text node");
		} else if (!dom) {
			let spec$1 = DOMSerializer.renderSpec(document, node.type.spec.toDOM(node), null, node.attrs);
			({dom, contentDOM} = spec$1);
		}
		if (!contentDOM && !node.isText && dom.nodeName != "BR") {
			if (!dom.hasAttribute("contenteditable")) dom.contentEditable = "false";
			if (node.type.spec.draggable) dom.draggable = true;
		}
		let nodeDOM = dom;
		dom = applyOuterDeco(dom, outerDeco, node);
		if (spec) return descObj = new CustomNodeViewDesc(parent, node, outerDeco, innerDeco, dom, contentDOM || null, nodeDOM, spec, view, pos + 1);
		else if (node.isText) return new TextViewDesc(parent, node, outerDeco, innerDeco, dom, nodeDOM, view);
		else return new NodeViewDesc(parent, node, outerDeco, innerDeco, dom, contentDOM || null, nodeDOM, view, pos + 1);
	}
	parseRule() {
		if (this.node.type.spec.reparseInView) return null;
		let rule = {
			node: this.node.type.name,
			attrs: this.node.attrs
		};
		if (this.node.type.whitespace == "pre") rule.preserveWhitespace = "full";
		if (!this.contentDOM) rule.getContent = () => this.node.content;
		else if (!this.contentLost) rule.contentElement = this.contentDOM;
		else {
			for (let i$1 = this.children.length - 1; i$1 >= 0; i$1--) {
				let child = this.children[i$1];
				if (this.dom.contains(child.dom.parentNode)) {
					rule.contentElement = child.dom.parentNode;
					break;
				}
			}
			if (!rule.contentElement) rule.getContent = () => Fragment.empty;
		}
		return rule;
	}
	matchesNode(node, outerDeco, innerDeco) {
		return this.dirty == NOT_DIRTY && node.eq(this.node) && sameOuterDeco(outerDeco, this.outerDeco) && innerDeco.eq(this.innerDeco);
	}
	get size() {
		return this.node.nodeSize;
	}
	get border() {
		return this.node.isLeaf ? 0 : 1;
	}
	updateChildren(view, pos) {
		let inline = this.node.inlineContent, off = pos;
		let composition = view.composing ? this.localCompositionInfo(view, pos) : null;
		let localComposition = composition && composition.pos > -1 ? composition : null;
		let compositionInChild = composition && composition.pos < 0;
		let updater = new ViewTreeUpdater(this, localComposition && localComposition.node, view);
		iterDeco(this.node, this.innerDeco, (widget, i$1, insideNode) => {
			if (widget.spec.marks) updater.syncToMarks(widget.spec.marks, inline, view, i$1);
			else if (widget.type.side >= 0 && !insideNode) updater.syncToMarks(i$1 == this.node.childCount ? Mark$1.none : this.node.child(i$1).marks, inline, view, i$1);
			updater.placeWidget(widget, view, off);
		}, (child, outerDeco, innerDeco, i$1) => {
			updater.syncToMarks(child.marks, inline, view, i$1);
			let compIndex;
			if (updater.findNodeMatch(child, outerDeco, innerDeco, i$1));
			else if (compositionInChild && view.state.selection.from > off && view.state.selection.to < off + child.nodeSize && (compIndex = updater.findIndexWithChild(composition.node)) > -1 && updater.updateNodeAt(child, outerDeco, innerDeco, compIndex, view));
			else if (updater.updateNextNode(child, outerDeco, innerDeco, view, i$1, off));
			else updater.addNode(child, outerDeco, innerDeco, view, off);
			off += child.nodeSize;
		});
		updater.syncToMarks([], inline, view, 0);
		if (this.node.isTextblock) updater.addTextblockHacks();
		updater.destroyRest();
		if (updater.changed || this.dirty == CONTENT_DIRTY) {
			if (localComposition) this.protectLocalComposition(view, localComposition);
			renderDescs(this.contentDOM, this.children, view);
			if (ios) iosHacks(this.dom);
		}
	}
	localCompositionInfo(view, pos) {
		let { from, to } = view.state.selection;
		if (!(view.state.selection instanceof TextSelection) || from < pos || to > pos + this.node.content.size) return null;
		let textNode = view.input.compositionNode;
		if (!textNode || !this.dom.contains(textNode.parentNode)) return null;
		if (this.node.inlineContent) {
			let text = textNode.nodeValue;
			let textPos = findTextInFragment(this.node.content, text, from - pos, to - pos);
			return textPos < 0 ? null : {
				node: textNode,
				pos: textPos,
				text
			};
		} else return {
			node: textNode,
			pos: -1,
			text: ""
		};
	}
	protectLocalComposition(view, { node, pos, text }) {
		if (this.getDesc(node)) return;
		let topNode = node;
		for (;; topNode = topNode.parentNode) {
			if (topNode.parentNode == this.contentDOM) break;
			while (topNode.previousSibling) topNode.parentNode.removeChild(topNode.previousSibling);
			while (topNode.nextSibling) topNode.parentNode.removeChild(topNode.nextSibling);
			if (topNode.pmViewDesc) topNode.pmViewDesc = void 0;
		}
		let desc = new CompositionViewDesc(this, topNode, node, text);
		view.input.compositionNodes.push(desc);
		this.children = replaceNodes(this.children, pos, pos + text.length, view, desc);
	}
	update(node, outerDeco, innerDeco, view) {
		if (this.dirty == NODE_DIRTY || !node.sameMarkup(this.node)) return false;
		this.updateInner(node, outerDeco, innerDeco, view);
		return true;
	}
	updateInner(node, outerDeco, innerDeco, view) {
		this.updateOuterDeco(outerDeco);
		this.node = node;
		this.innerDeco = innerDeco;
		if (this.contentDOM) this.updateChildren(view, this.posAtStart);
		this.dirty = NOT_DIRTY;
	}
	updateOuterDeco(outerDeco) {
		if (sameOuterDeco(outerDeco, this.outerDeco)) return;
		let needsWrap = this.nodeDOM.nodeType != 1;
		let oldDOM = this.dom;
		this.dom = patchOuterDeco(this.dom, this.nodeDOM, computeOuterDeco(this.outerDeco, this.node, needsWrap), computeOuterDeco(outerDeco, this.node, needsWrap));
		if (this.dom != oldDOM) {
			oldDOM.pmViewDesc = void 0;
			this.dom.pmViewDesc = this;
		}
		this.outerDeco = outerDeco;
	}
	selectNode() {
		if (this.nodeDOM.nodeType == 1) {
			this.nodeDOM.classList.add("ProseMirror-selectednode");
			if (this.contentDOM || !this.node.type.spec.draggable) this.nodeDOM.draggable = true;
		}
	}
	deselectNode() {
		if (this.nodeDOM.nodeType == 1) {
			this.nodeDOM.classList.remove("ProseMirror-selectednode");
			if (this.contentDOM || !this.node.type.spec.draggable) this.nodeDOM.removeAttribute("draggable");
		}
	}
	get domAtom() {
		return this.node.isAtom;
	}
};
function docViewDesc(doc$2, outerDeco, innerDeco, dom, view) {
	applyOuterDeco(dom, outerDeco, doc$2);
	let docView = new NodeViewDesc(void 0, doc$2, outerDeco, innerDeco, dom, dom, dom, view, 0);
	if (docView.contentDOM) docView.updateChildren(view, 0);
	return docView;
}
var TextViewDesc = class TextViewDesc extends NodeViewDesc {
	constructor(parent, node, outerDeco, innerDeco, dom, nodeDOM, view) {
		super(parent, node, outerDeco, innerDeco, dom, null, nodeDOM, view, 0);
	}
	parseRule() {
		let skip = this.nodeDOM.parentNode;
		while (skip && skip != this.dom && !skip.pmIsDeco) skip = skip.parentNode;
		return { skip: skip || true };
	}
	update(node, outerDeco, innerDeco, view) {
		if (this.dirty == NODE_DIRTY || this.dirty != NOT_DIRTY && !this.inParent() || !node.sameMarkup(this.node)) return false;
		this.updateOuterDeco(outerDeco);
		if ((this.dirty != NOT_DIRTY || node.text != this.node.text) && node.text != this.nodeDOM.nodeValue) {
			this.nodeDOM.nodeValue = node.text;
			if (view.trackWrites == this.nodeDOM) view.trackWrites = null;
		}
		this.node = node;
		this.dirty = NOT_DIRTY;
		return true;
	}
	inParent() {
		let parentDOM = this.parent.contentDOM;
		for (let n = this.nodeDOM; n; n = n.parentNode) if (n == parentDOM) return true;
		return false;
	}
	domFromPos(pos) {
		return {
			node: this.nodeDOM,
			offset: pos
		};
	}
	localPosFromDOM(dom, offset, bias) {
		if (dom == this.nodeDOM) return this.posAtStart + Math.min(offset, this.node.text.length);
		return super.localPosFromDOM(dom, offset, bias);
	}
	ignoreMutation(mutation) {
		return mutation.type != "characterData" && mutation.type != "selection";
	}
	slice(from, to, view) {
		let node = this.node.cut(from, to), dom = document.createTextNode(node.text);
		return new TextViewDesc(this.parent, node, this.outerDeco, this.innerDeco, dom, dom, view);
	}
	markDirty(from, to) {
		super.markDirty(from, to);
		if (this.dom != this.nodeDOM && (from == 0 || to == this.nodeDOM.nodeValue.length)) this.dirty = NODE_DIRTY;
	}
	get domAtom() {
		return false;
	}
	isText(text) {
		return this.node.text == text;
	}
};
var TrailingHackViewDesc = class extends ViewDesc {
	parseRule() {
		return { ignore: true };
	}
	matchesHack(nodeName) {
		return this.dirty == NOT_DIRTY && this.dom.nodeName == nodeName;
	}
	get domAtom() {
		return true;
	}
	get ignoreForCoords() {
		return this.dom.nodeName == "IMG";
	}
};
var CustomNodeViewDesc = class extends NodeViewDesc {
	constructor(parent, node, outerDeco, innerDeco, dom, contentDOM, nodeDOM, spec, view, pos) {
		super(parent, node, outerDeco, innerDeco, dom, contentDOM, nodeDOM, view, pos);
		this.spec = spec;
	}
	update(node, outerDeco, innerDeco, view) {
		if (this.dirty == NODE_DIRTY) return false;
		if (this.spec.update && (this.node.type == node.type || this.spec.multiType)) {
			let result = this.spec.update(node, outerDeco, innerDeco);
			if (result) this.updateInner(node, outerDeco, innerDeco, view);
			return result;
		} else if (!this.contentDOM && !node.isLeaf) return false;
		else return super.update(node, outerDeco, innerDeco, view);
	}
	selectNode() {
		this.spec.selectNode ? this.spec.selectNode() : super.selectNode();
	}
	deselectNode() {
		this.spec.deselectNode ? this.spec.deselectNode() : super.deselectNode();
	}
	setSelection(anchor, head, view, force) {
		this.spec.setSelection ? this.spec.setSelection(anchor, head, view.root) : super.setSelection(anchor, head, view, force);
	}
	destroy() {
		if (this.spec.destroy) this.spec.destroy();
		super.destroy();
	}
	stopEvent(event) {
		return this.spec.stopEvent ? this.spec.stopEvent(event) : false;
	}
	ignoreMutation(mutation) {
		return this.spec.ignoreMutation ? this.spec.ignoreMutation(mutation) : super.ignoreMutation(mutation);
	}
};
function renderDescs(parentDOM, descs, view) {
	let dom = parentDOM.firstChild, written = false;
	for (let i$1 = 0; i$1 < descs.length; i$1++) {
		let desc = descs[i$1], childDOM = desc.dom;
		if (childDOM.parentNode == parentDOM) {
			while (childDOM != dom) {
				dom = rm(dom);
				written = true;
			}
			dom = dom.nextSibling;
		} else {
			written = true;
			parentDOM.insertBefore(childDOM, dom);
		}
		if (desc instanceof MarkViewDesc) {
			let pos = dom ? dom.previousSibling : parentDOM.lastChild;
			renderDescs(desc.contentDOM, desc.children, view);
			dom = pos ? pos.nextSibling : parentDOM.firstChild;
		}
	}
	while (dom) {
		dom = rm(dom);
		written = true;
	}
	if (written && view.trackWrites == parentDOM) view.trackWrites = null;
}
var OuterDecoLevel = function(nodeName) {
	if (nodeName) this.nodeName = nodeName;
};
OuterDecoLevel.prototype = Object.create(null);
var noDeco = [new OuterDecoLevel()];
function computeOuterDeco(outerDeco, node, needsWrap) {
	if (outerDeco.length == 0) return noDeco;
	let top = needsWrap ? noDeco[0] : new OuterDecoLevel(), result = [top];
	for (let i$1 = 0; i$1 < outerDeco.length; i$1++) {
		let attrs = outerDeco[i$1].type.attrs;
		if (!attrs) continue;
		if (attrs.nodeName) result.push(top = new OuterDecoLevel(attrs.nodeName));
		for (let name in attrs) {
			let val = attrs[name];
			if (val == null) continue;
			if (needsWrap && result.length == 1) result.push(top = new OuterDecoLevel(node.isInline ? "span" : "div"));
			if (name == "class") top.class = (top.class ? top.class + " " : "") + val;
			else if (name == "style") top.style = (top.style ? top.style + ";" : "") + val;
			else if (name != "nodeName") top[name] = val;
		}
	}
	return result;
}
function patchOuterDeco(outerDOM, nodeDOM, prevComputed, curComputed) {
	if (prevComputed == noDeco && curComputed == noDeco) return nodeDOM;
	let curDOM = nodeDOM;
	for (let i$1 = 0; i$1 < curComputed.length; i$1++) {
		let deco = curComputed[i$1], prev = prevComputed[i$1];
		if (i$1) {
			let parent;
			if (prev && prev.nodeName == deco.nodeName && curDOM != outerDOM && (parent = curDOM.parentNode) && parent.nodeName.toLowerCase() == deco.nodeName) curDOM = parent;
			else {
				parent = document.createElement(deco.nodeName);
				parent.pmIsDeco = true;
				parent.appendChild(curDOM);
				prev = noDeco[0];
				curDOM = parent;
			}
		}
		patchAttributes(curDOM, prev || noDeco[0], deco);
	}
	return curDOM;
}
function patchAttributes(dom, prev, cur) {
	for (let name in prev) if (name != "class" && name != "style" && name != "nodeName" && !(name in cur)) dom.removeAttribute(name);
	for (let name in cur) if (name != "class" && name != "style" && name != "nodeName" && cur[name] != prev[name]) dom.setAttribute(name, cur[name]);
	if (prev.class != cur.class) {
		let prevList = prev.class ? prev.class.split(" ").filter(Boolean) : [];
		let curList = cur.class ? cur.class.split(" ").filter(Boolean) : [];
		for (let i$1 = 0; i$1 < prevList.length; i$1++) if (curList.indexOf(prevList[i$1]) == -1) dom.classList.remove(prevList[i$1]);
		for (let i$1 = 0; i$1 < curList.length; i$1++) if (prevList.indexOf(curList[i$1]) == -1) dom.classList.add(curList[i$1]);
		if (dom.classList.length == 0) dom.removeAttribute("class");
	}
	if (prev.style != cur.style) {
		if (prev.style) {
			let prop = /\s*([\w\-\xa1-\uffff]+)\s*:(?:"(?:\\.|[^"])*"|'(?:\\.|[^'])*'|\(.*?\)|[^;])*/g, m$1;
			while (m$1 = prop.exec(prev.style)) dom.style.removeProperty(m$1[1]);
		}
		if (cur.style) dom.style.cssText += cur.style;
	}
}
function applyOuterDeco(dom, deco, node) {
	return patchOuterDeco(dom, dom, noDeco, computeOuterDeco(deco, node, dom.nodeType != 1));
}
function sameOuterDeco(a, b$1) {
	if (a.length != b$1.length) return false;
	for (let i$1 = 0; i$1 < a.length; i$1++) if (!a[i$1].type.eq(b$1[i$1].type)) return false;
	return true;
}
function rm(dom) {
	let next = dom.nextSibling;
	dom.parentNode.removeChild(dom);
	return next;
}
var ViewTreeUpdater = class {
	constructor(top, lock, view) {
		this.lock = lock;
		this.view = view;
		this.index = 0;
		this.stack = [];
		this.changed = false;
		this.top = top;
		this.preMatch = preMatch(top.node.content, top);
	}
	destroyBetween(start, end) {
		if (start == end) return;
		for (let i$1 = start; i$1 < end; i$1++) this.top.children[i$1].destroy();
		this.top.children.splice(start, end - start);
		this.changed = true;
	}
	destroyRest() {
		this.destroyBetween(this.index, this.top.children.length);
	}
	syncToMarks(marks, inline, view, parentIndex) {
		let keep = 0, depth = this.stack.length >> 1;
		let maxKeep = Math.min(depth, marks.length);
		while (keep < maxKeep && (keep == depth - 1 ? this.top : this.stack[keep + 1 << 1]).matchesMark(marks[keep]) && marks[keep].type.spec.spanning !== false) keep++;
		while (keep < depth) {
			this.destroyRest();
			this.top.dirty = NOT_DIRTY;
			this.index = this.stack.pop();
			this.top = this.stack.pop();
			depth--;
		}
		while (depth < marks.length) {
			this.stack.push(this.top, this.index + 1);
			let found$1 = -1, scanTo = this.top.children.length;
			if (parentIndex < this.preMatch.index) scanTo = Math.min(this.index + 3, scanTo);
			for (let i$1 = this.index; i$1 < scanTo; i$1++) {
				let next = this.top.children[i$1];
				if (next.matchesMark(marks[depth]) && !this.isLocked(next.dom)) {
					found$1 = i$1;
					break;
				}
			}
			if (found$1 > -1) {
				if (found$1 > this.index) {
					this.changed = true;
					this.destroyBetween(this.index, found$1);
				}
				this.top = this.top.children[this.index];
			} else {
				let markDesc = MarkViewDesc.create(this.top, marks[depth], inline, view);
				this.top.children.splice(this.index, 0, markDesc);
				this.top = markDesc;
				this.changed = true;
			}
			this.index = 0;
			depth++;
		}
	}
	findNodeMatch(node, outerDeco, innerDeco, index) {
		let found$1 = -1, targetDesc;
		if (index >= this.preMatch.index && (targetDesc = this.preMatch.matches[index - this.preMatch.index]).parent == this.top && targetDesc.matchesNode(node, outerDeco, innerDeco)) found$1 = this.top.children.indexOf(targetDesc, this.index);
		else for (let i$1 = this.index, e = Math.min(this.top.children.length, i$1 + 5); i$1 < e; i$1++) {
			let child = this.top.children[i$1];
			if (child.matchesNode(node, outerDeco, innerDeco) && !this.preMatch.matched.has(child)) {
				found$1 = i$1;
				break;
			}
		}
		if (found$1 < 0) return false;
		this.destroyBetween(this.index, found$1);
		this.index++;
		return true;
	}
	updateNodeAt(node, outerDeco, innerDeco, index, view) {
		let child = this.top.children[index];
		if (child.dirty == NODE_DIRTY && child.dom == child.contentDOM) child.dirty = CONTENT_DIRTY;
		if (!child.update(node, outerDeco, innerDeco, view)) return false;
		this.destroyBetween(this.index, index);
		this.index++;
		return true;
	}
	findIndexWithChild(domNode) {
		for (;;) {
			let parent = domNode.parentNode;
			if (!parent) return -1;
			if (parent == this.top.contentDOM) {
				let desc = domNode.pmViewDesc;
				if (desc) {
					for (let i$1 = this.index; i$1 < this.top.children.length; i$1++) if (this.top.children[i$1] == desc) return i$1;
				}
				return -1;
			}
			domNode = parent;
		}
	}
	updateNextNode(node, outerDeco, innerDeco, view, index, pos) {
		for (let i$1 = this.index; i$1 < this.top.children.length; i$1++) {
			let next = this.top.children[i$1];
			if (next instanceof NodeViewDesc) {
				let preMatch$1 = this.preMatch.matched.get(next);
				if (preMatch$1 != null && preMatch$1 != index) return false;
				let nextDOM = next.dom, updated;
				let locked = this.isLocked(nextDOM) && !(node.isText && next.node && next.node.isText && next.nodeDOM.nodeValue == node.text && next.dirty != NODE_DIRTY && sameOuterDeco(outerDeco, next.outerDeco));
				if (!locked && next.update(node, outerDeco, innerDeco, view)) {
					this.destroyBetween(this.index, i$1);
					if (next.dom != nextDOM) this.changed = true;
					this.index++;
					return true;
				} else if (!locked && (updated = this.recreateWrapper(next, node, outerDeco, innerDeco, view, pos))) {
					this.destroyBetween(this.index, i$1);
					this.top.children[this.index] = updated;
					if (updated.contentDOM) {
						updated.dirty = CONTENT_DIRTY;
						updated.updateChildren(view, pos + 1);
						updated.dirty = NOT_DIRTY;
					}
					this.changed = true;
					this.index++;
					return true;
				}
				break;
			}
		}
		return false;
	}
	recreateWrapper(next, node, outerDeco, innerDeco, view, pos) {
		if (next.dirty || node.isAtom || !next.children.length || !next.node.content.eq(node.content) || !sameOuterDeco(outerDeco, next.outerDeco) || !innerDeco.eq(next.innerDeco)) return null;
		let wrapper = NodeViewDesc.create(this.top, node, outerDeco, innerDeco, view, pos);
		if (wrapper.contentDOM) {
			wrapper.children = next.children;
			next.children = [];
			for (let ch of wrapper.children) ch.parent = wrapper;
		}
		next.destroy();
		return wrapper;
	}
	addNode(node, outerDeco, innerDeco, view, pos) {
		let desc = NodeViewDesc.create(this.top, node, outerDeco, innerDeco, view, pos);
		if (desc.contentDOM) desc.updateChildren(view, pos + 1);
		this.top.children.splice(this.index++, 0, desc);
		this.changed = true;
	}
	placeWidget(widget, view, pos) {
		let next = this.index < this.top.children.length ? this.top.children[this.index] : null;
		if (next && next.matchesWidget(widget) && (widget == next.widget || !next.widget.type.toDOM.parentNode)) this.index++;
		else {
			let desc = new WidgetViewDesc(this.top, widget, view, pos);
			this.top.children.splice(this.index++, 0, desc);
			this.changed = true;
		}
	}
	addTextblockHacks() {
		let lastChild = this.top.children[this.index - 1], parent = this.top;
		while (lastChild instanceof MarkViewDesc) {
			parent = lastChild;
			lastChild = parent.children[parent.children.length - 1];
		}
		if (!lastChild || !(lastChild instanceof TextViewDesc) || /\n$/.test(lastChild.node.text) || this.view.requiresGeckoHackNode && /\s$/.test(lastChild.node.text)) {
			if ((safari || chrome) && lastChild && lastChild.dom.contentEditable == "false") this.addHackNode("IMG", parent);
			this.addHackNode("BR", this.top);
		}
	}
	addHackNode(nodeName, parent) {
		if (parent == this.top && this.index < parent.children.length && parent.children[this.index].matchesHack(nodeName)) this.index++;
		else {
			let dom = document.createElement(nodeName);
			if (nodeName == "IMG") {
				dom.className = "ProseMirror-separator";
				dom.alt = "";
			}
			if (nodeName == "BR") dom.className = "ProseMirror-trailingBreak";
			let hack = new TrailingHackViewDesc(this.top, [], dom, null);
			if (parent != this.top) parent.children.push(hack);
			else parent.children.splice(this.index++, 0, hack);
			this.changed = true;
		}
	}
	isLocked(node) {
		return this.lock && (node == this.lock || node.nodeType == 1 && node.contains(this.lock.parentNode));
	}
};
function preMatch(frag, parentDesc) {
	let curDesc = parentDesc, descI = curDesc.children.length;
	let fI = frag.childCount, matched = /* @__PURE__ */ new Map(), matches$1 = [];
	outer: while (fI > 0) {
		let desc;
		for (;;) if (descI) {
			let next = curDesc.children[descI - 1];
			if (next instanceof MarkViewDesc) {
				curDesc = next;
				descI = next.children.length;
			} else {
				desc = next;
				descI--;
				break;
			}
		} else if (curDesc == parentDesc) break outer;
		else {
			descI = curDesc.parent.children.indexOf(curDesc);
			curDesc = curDesc.parent;
		}
		let node = desc.node;
		if (!node) continue;
		if (node != frag.child(fI - 1)) break;
		--fI;
		matched.set(desc, fI);
		matches$1.push(desc);
	}
	return {
		index: fI,
		matched,
		matches: matches$1.reverse()
	};
}
function compareSide(a, b$1) {
	return a.type.side - b$1.type.side;
}
function iterDeco(parent, deco, onWidget, onNode) {
	let locals = deco.locals(parent), offset = 0;
	if (locals.length == 0) {
		for (let i$1 = 0; i$1 < parent.childCount; i$1++) {
			let child = parent.child(i$1);
			onNode(child, locals, deco.forChild(offset, child), i$1);
			offset += child.nodeSize;
		}
		return;
	}
	let decoIndex = 0, active = [], restNode = null;
	for (let parentIndex = 0;;) {
		let widget, widgets;
		while (decoIndex < locals.length && locals[decoIndex].to == offset) {
			let next = locals[decoIndex++];
			if (next.widget) if (!widget) widget = next;
			else (widgets || (widgets = [widget])).push(next);
		}
		if (widget) if (widgets) {
			widgets.sort(compareSide);
			for (let i$1 = 0; i$1 < widgets.length; i$1++) onWidget(widgets[i$1], parentIndex, !!restNode);
		} else onWidget(widget, parentIndex, !!restNode);
		let child, index;
		if (restNode) {
			index = -1;
			child = restNode;
			restNode = null;
		} else if (parentIndex < parent.childCount) {
			index = parentIndex;
			child = parent.child(parentIndex++);
		} else break;
		for (let i$1 = 0; i$1 < active.length; i$1++) if (active[i$1].to <= offset) active.splice(i$1--, 1);
		while (decoIndex < locals.length && locals[decoIndex].from <= offset && locals[decoIndex].to > offset) active.push(locals[decoIndex++]);
		let end = offset + child.nodeSize;
		if (child.isText) {
			let cutAt = end;
			if (decoIndex < locals.length && locals[decoIndex].from < cutAt) cutAt = locals[decoIndex].from;
			for (let i$1 = 0; i$1 < active.length; i$1++) if (active[i$1].to < cutAt) cutAt = active[i$1].to;
			if (cutAt < end) {
				restNode = child.cut(cutAt - offset);
				child = child.cut(0, cutAt - offset);
				end = cutAt;
				index = -1;
			}
		} else while (decoIndex < locals.length && locals[decoIndex].to < end) decoIndex++;
		let outerDeco = child.isInline && !child.isLeaf ? active.filter((d) => !d.inline) : active.slice();
		onNode(child, outerDeco, deco.forChild(offset, child), index);
		offset = end;
	}
}
function iosHacks(dom) {
	if (dom.nodeName == "UL" || dom.nodeName == "OL") {
		let oldCSS = dom.style.cssText;
		dom.style.cssText = oldCSS + "; list-style: square !important";
		window.getComputedStyle(dom).listStyle;
		dom.style.cssText = oldCSS;
	}
}
function findTextInFragment(frag, text, from, to) {
	for (let i$1 = 0, pos = 0; i$1 < frag.childCount && pos <= to;) {
		let child = frag.child(i$1++), childStart = pos;
		pos += child.nodeSize;
		if (!child.isText) continue;
		let str = child.text;
		while (i$1 < frag.childCount) {
			let next = frag.child(i$1++);
			pos += next.nodeSize;
			if (!next.isText) break;
			str += next.text;
		}
		if (pos >= from) {
			if (pos >= to && str.slice(to - text.length - childStart, to - childStart) == text) return to - text.length;
			let found$1 = childStart < to ? str.lastIndexOf(text, to - childStart - 1) : -1;
			if (found$1 >= 0 && found$1 + text.length + childStart >= from) return childStart + found$1;
			if (from == to && str.length >= to + text.length - childStart && str.slice(to - childStart, to - childStart + text.length) == text) return to;
		}
	}
	return -1;
}
function replaceNodes(nodes, from, to, view, replacement) {
	let result = [];
	for (let i$1 = 0, off = 0; i$1 < nodes.length; i$1++) {
		let child = nodes[i$1], start = off, end = off += child.size;
		if (start >= to || end <= from) result.push(child);
		else {
			if (start < from) result.push(child.slice(0, from - start, view));
			if (replacement) {
				result.push(replacement);
				replacement = void 0;
			}
			if (end > to) result.push(child.slice(to - start, child.size, view));
		}
	}
	return result;
}
function selectionFromDOM(view, origin = null) {
	let domSel = view.domSelectionRange(), doc$2 = view.state.doc;
	if (!domSel.focusNode) return null;
	let nearestDesc = view.docView.nearestDesc(domSel.focusNode), inWidget = nearestDesc && nearestDesc.size == 0;
	let head = view.docView.posFromDOM(domSel.focusNode, domSel.focusOffset, 1);
	if (head < 0) return null;
	let $head = doc$2.resolve(head), anchor, selection;
	if (selectionCollapsed(domSel)) {
		anchor = head;
		while (nearestDesc && !nearestDesc.node) nearestDesc = nearestDesc.parent;
		let nearestDescNode = nearestDesc.node;
		if (nearestDesc && nearestDescNode.isAtom && NodeSelection.isSelectable(nearestDescNode) && nearestDesc.parent && !(nearestDescNode.isInline && isOnEdge(domSel.focusNode, domSel.focusOffset, nearestDesc.dom))) {
			let pos = nearestDesc.posBefore;
			selection = new NodeSelection(head == pos ? $head : doc$2.resolve(pos));
		}
	} else {
		if (domSel instanceof view.dom.ownerDocument.defaultView.Selection && domSel.rangeCount > 1) {
			let min = head, max = head;
			for (let i$1 = 0; i$1 < domSel.rangeCount; i$1++) {
				let range = domSel.getRangeAt(i$1);
				min = Math.min(min, view.docView.posFromDOM(range.startContainer, range.startOffset, 1));
				max = Math.max(max, view.docView.posFromDOM(range.endContainer, range.endOffset, -1));
			}
			if (min < 0) return null;
			[anchor, head] = max == view.state.selection.anchor ? [max, min] : [min, max];
			$head = doc$2.resolve(head);
		} else anchor = view.docView.posFromDOM(domSel.anchorNode, domSel.anchorOffset, 1);
		if (anchor < 0) return null;
	}
	let $anchor = doc$2.resolve(anchor);
	if (!selection) {
		let bias = origin == "pointer" || view.state.selection.head < $head.pos && !inWidget ? 1 : -1;
		selection = selectionBetween(view, $anchor, $head, bias);
	}
	return selection;
}
function editorOwnsSelection(view) {
	return view.editable ? view.hasFocus() : hasSelection(view) && document.activeElement && document.activeElement.contains(view.dom);
}
function selectionToDOM(view, force = false) {
	let sel = view.state.selection;
	syncNodeSelection(view, sel);
	if (!editorOwnsSelection(view)) return;
	if (!force && view.input.mouseDown && view.input.mouseDown.allowDefault && chrome) {
		let domSel = view.domSelectionRange(), curSel = view.domObserver.currentSelection;
		if (domSel.anchorNode && curSel.anchorNode && isEquivalentPosition(domSel.anchorNode, domSel.anchorOffset, curSel.anchorNode, curSel.anchorOffset)) {
			view.input.mouseDown.delayedSelectionSync = true;
			view.domObserver.setCurSelection();
			return;
		}
	}
	view.domObserver.disconnectSelection();
	if (view.cursorWrapper) selectCursorWrapper(view);
	else {
		let { anchor, head } = sel, resetEditableFrom, resetEditableTo;
		if (brokenSelectBetweenUneditable && !(sel instanceof TextSelection)) {
			if (!sel.$from.parent.inlineContent) resetEditableFrom = temporarilyEditableNear(view, sel.from);
			if (!sel.empty && !sel.$from.parent.inlineContent) resetEditableTo = temporarilyEditableNear(view, sel.to);
		}
		view.docView.setSelection(anchor, head, view, force);
		if (brokenSelectBetweenUneditable) {
			if (resetEditableFrom) resetEditable(resetEditableFrom);
			if (resetEditableTo) resetEditable(resetEditableTo);
		}
		if (sel.visible) view.dom.classList.remove("ProseMirror-hideselection");
		else {
			view.dom.classList.add("ProseMirror-hideselection");
			if ("onselectionchange" in document) removeClassOnSelectionChange(view);
		}
	}
	view.domObserver.setCurSelection();
	view.domObserver.connectSelection();
}
var brokenSelectBetweenUneditable = safari || chrome && chrome_version < 63;
function temporarilyEditableNear(view, pos) {
	let { node, offset } = view.docView.domFromPos(pos, 0);
	let after = offset < node.childNodes.length ? node.childNodes[offset] : null;
	let before = offset ? node.childNodes[offset - 1] : null;
	if (safari && after && after.contentEditable == "false") return setEditable(after);
	if ((!after || after.contentEditable == "false") && (!before || before.contentEditable == "false")) {
		if (after) return setEditable(after);
		else if (before) return setEditable(before);
	}
}
function setEditable(element) {
	element.contentEditable = "true";
	if (safari && element.draggable) {
		element.draggable = false;
		element.wasDraggable = true;
	}
	return element;
}
function resetEditable(element) {
	element.contentEditable = "false";
	if (element.wasDraggable) {
		element.draggable = true;
		element.wasDraggable = null;
	}
}
function removeClassOnSelectionChange(view) {
	let doc$2 = view.dom.ownerDocument;
	doc$2.removeEventListener("selectionchange", view.input.hideSelectionGuard);
	let domSel = view.domSelectionRange();
	let node = domSel.anchorNode, offset = domSel.anchorOffset;
	doc$2.addEventListener("selectionchange", view.input.hideSelectionGuard = () => {
		if (domSel.anchorNode != node || domSel.anchorOffset != offset) {
			doc$2.removeEventListener("selectionchange", view.input.hideSelectionGuard);
			setTimeout(() => {
				if (!editorOwnsSelection(view) || view.state.selection.visible) view.dom.classList.remove("ProseMirror-hideselection");
			}, 20);
		}
	});
}
function selectCursorWrapper(view) {
	let domSel = view.domSelection();
	if (!domSel) return;
	let node = view.cursorWrapper.dom, img = node.nodeName == "IMG";
	if (img) domSel.collapse(node.parentNode, domIndex(node) + 1);
	else domSel.collapse(node, 0);
	if (!img && !view.state.selection.visible && ie$2 && ie_version <= 11) {
		node.disabled = true;
		node.disabled = false;
	}
}
function syncNodeSelection(view, sel) {
	if (sel instanceof NodeSelection) {
		let desc = view.docView.descAt(sel.from);
		if (desc != view.lastSelectedViewDesc) {
			clearNodeSelection(view);
			if (desc) desc.selectNode();
			view.lastSelectedViewDesc = desc;
		}
	} else clearNodeSelection(view);
}
function clearNodeSelection(view) {
	if (view.lastSelectedViewDesc) {
		if (view.lastSelectedViewDesc.parent) view.lastSelectedViewDesc.deselectNode();
		view.lastSelectedViewDesc = void 0;
	}
}
function selectionBetween(view, $anchor, $head, bias) {
	return view.someProp("createSelectionBetween", (f) => f(view, $anchor, $head)) || TextSelection.between($anchor, $head, bias);
}
function hasFocusAndSelection(view) {
	if (view.editable && !view.hasFocus()) return false;
	return hasSelection(view);
}
function hasSelection(view) {
	let sel = view.domSelectionRange();
	if (!sel.anchorNode) return false;
	try {
		return view.dom.contains(sel.anchorNode.nodeType == 3 ? sel.anchorNode.parentNode : sel.anchorNode) && (view.editable || view.dom.contains(sel.focusNode.nodeType == 3 ? sel.focusNode.parentNode : sel.focusNode));
	} catch (_$1) {
		return false;
	}
}
function anchorInRightPlace(view) {
	let anchorDOM = view.docView.domFromPos(view.state.selection.anchor, 0);
	let domSel = view.domSelectionRange();
	return isEquivalentPosition(anchorDOM.node, anchorDOM.offset, domSel.anchorNode, domSel.anchorOffset);
}
function moveSelectionBlock(state, dir) {
	let { $anchor, $head } = state.selection;
	let $side = dir > 0 ? $anchor.max($head) : $anchor.min($head);
	let $start = !$side.parent.inlineContent ? $side : $side.depth ? state.doc.resolve(dir > 0 ? $side.after() : $side.before()) : null;
	return $start && Selection.findFrom($start, dir);
}
function apply(view, sel) {
	view.dispatch(view.state.tr.setSelection(sel).scrollIntoView());
	return true;
}
function selectHorizontally(view, dir, mods) {
	let sel = view.state.selection;
	if (sel instanceof TextSelection) {
		if (mods.indexOf("s") > -1) {
			let { $head } = sel, node = $head.textOffset ? null : dir < 0 ? $head.nodeBefore : $head.nodeAfter;
			if (!node || node.isText || !node.isLeaf) return false;
			let $newHead = view.state.doc.resolve($head.pos + node.nodeSize * (dir < 0 ? -1 : 1));
			return apply(view, new TextSelection(sel.$anchor, $newHead));
		} else if (!sel.empty) return false;
		else if (view.endOfTextblock(dir > 0 ? "forward" : "backward")) {
			let next = moveSelectionBlock(view.state, dir);
			if (next && next instanceof NodeSelection) return apply(view, next);
			return false;
		} else if (!(mac$2 && mods.indexOf("m") > -1)) {
			let $head = sel.$head, node = $head.textOffset ? null : dir < 0 ? $head.nodeBefore : $head.nodeAfter, desc;
			if (!node || node.isText) return false;
			let nodePos = dir < 0 ? $head.pos - node.nodeSize : $head.pos;
			if (!(node.isAtom || (desc = view.docView.descAt(nodePos)) && !desc.contentDOM)) return false;
			if (NodeSelection.isSelectable(node)) return apply(view, new NodeSelection(dir < 0 ? view.state.doc.resolve($head.pos - node.nodeSize) : $head));
			else if (webkit) return apply(view, new TextSelection(view.state.doc.resolve(dir < 0 ? nodePos : nodePos + node.nodeSize)));
			else return false;
		}
	} else if (sel instanceof NodeSelection && sel.node.isInline) return apply(view, new TextSelection(dir > 0 ? sel.$to : sel.$from));
	else {
		let next = moveSelectionBlock(view.state, dir);
		if (next) return apply(view, next);
		return false;
	}
}
function nodeLen(node) {
	return node.nodeType == 3 ? node.nodeValue.length : node.childNodes.length;
}
function isIgnorable(dom, dir) {
	let desc = dom.pmViewDesc;
	return desc && desc.size == 0 && (dir < 0 || dom.nextSibling || dom.nodeName != "BR");
}
function skipIgnoredNodes(view, dir) {
	return dir < 0 ? skipIgnoredNodesBefore(view) : skipIgnoredNodesAfter(view);
}
function skipIgnoredNodesBefore(view) {
	let sel = view.domSelectionRange();
	let node = sel.focusNode, offset = sel.focusOffset;
	if (!node) return;
	let moveNode, moveOffset, force = false;
	if (gecko && node.nodeType == 1 && offset < nodeLen(node) && isIgnorable(node.childNodes[offset], -1)) force = true;
	for (;;) if (offset > 0) if (node.nodeType != 1) break;
	else {
		let before = node.childNodes[offset - 1];
		if (isIgnorable(before, -1)) {
			moveNode = node;
			moveOffset = --offset;
		} else if (before.nodeType == 3) {
			node = before;
			offset = node.nodeValue.length;
		} else break;
	}
	else if (isBlockNode(node)) break;
	else {
		let prev = node.previousSibling;
		while (prev && isIgnorable(prev, -1)) {
			moveNode = node.parentNode;
			moveOffset = domIndex(prev);
			prev = prev.previousSibling;
		}
		if (!prev) {
			node = node.parentNode;
			if (node == view.dom) break;
			offset = 0;
		} else {
			node = prev;
			offset = nodeLen(node);
		}
	}
	if (force) setSelFocus(view, node, offset);
	else if (moveNode) setSelFocus(view, moveNode, moveOffset);
}
function skipIgnoredNodesAfter(view) {
	let sel = view.domSelectionRange();
	let node = sel.focusNode, offset = sel.focusOffset;
	if (!node) return;
	let len = nodeLen(node);
	let moveNode, moveOffset;
	for (;;) if (offset < len) {
		if (node.nodeType != 1) break;
		let after = node.childNodes[offset];
		if (isIgnorable(after, 1)) {
			moveNode = node;
			moveOffset = ++offset;
		} else break;
	} else if (isBlockNode(node)) break;
	else {
		let next = node.nextSibling;
		while (next && isIgnorable(next, 1)) {
			moveNode = next.parentNode;
			moveOffset = domIndex(next) + 1;
			next = next.nextSibling;
		}
		if (!next) {
			node = node.parentNode;
			if (node == view.dom) break;
			offset = len = 0;
		} else {
			node = next;
			offset = 0;
			len = nodeLen(node);
		}
	}
	if (moveNode) setSelFocus(view, moveNode, moveOffset);
}
function isBlockNode(dom) {
	let desc = dom.pmViewDesc;
	return desc && desc.node && desc.node.isBlock;
}
function textNodeAfter(node, offset) {
	while (node && offset == node.childNodes.length && !hasBlockDesc(node)) {
		offset = domIndex(node) + 1;
		node = node.parentNode;
	}
	while (node && offset < node.childNodes.length) {
		let next = node.childNodes[offset];
		if (next.nodeType == 3) return next;
		if (next.nodeType == 1 && next.contentEditable == "false") break;
		node = next;
		offset = 0;
	}
}
function textNodeBefore(node, offset) {
	while (node && !offset && !hasBlockDesc(node)) {
		offset = domIndex(node);
		node = node.parentNode;
	}
	while (node && offset) {
		let next = node.childNodes[offset - 1];
		if (next.nodeType == 3) return next;
		if (next.nodeType == 1 && next.contentEditable == "false") break;
		node = next;
		offset = node.childNodes.length;
	}
}
function setSelFocus(view, node, offset) {
	if (node.nodeType != 3) {
		let before, after;
		if (after = textNodeAfter(node, offset)) {
			node = after;
			offset = 0;
		} else if (before = textNodeBefore(node, offset)) {
			node = before;
			offset = before.nodeValue.length;
		}
	}
	let sel = view.domSelection();
	if (!sel) return;
	if (selectionCollapsed(sel)) {
		let range = document.createRange();
		range.setEnd(node, offset);
		range.setStart(node, offset);
		sel.removeAllRanges();
		sel.addRange(range);
	} else if (sel.extend) sel.extend(node, offset);
	view.domObserver.setCurSelection();
	let { state } = view;
	setTimeout(() => {
		if (view.state == state) selectionToDOM(view);
	}, 50);
}
function findDirection(view, pos) {
	let $pos = view.state.doc.resolve(pos);
	if (!(chrome || windows$1) && $pos.parent.inlineContent) {
		let coords = view.coordsAtPos(pos);
		if (pos > $pos.start()) {
			let before = view.coordsAtPos(pos - 1);
			let mid = (before.top + before.bottom) / 2;
			if (mid > coords.top && mid < coords.bottom && Math.abs(before.left - coords.left) > 1) return before.left < coords.left ? "ltr" : "rtl";
		}
		if (pos < $pos.end()) {
			let after = view.coordsAtPos(pos + 1);
			let mid = (after.top + after.bottom) / 2;
			if (mid > coords.top && mid < coords.bottom && Math.abs(after.left - coords.left) > 1) return after.left > coords.left ? "ltr" : "rtl";
		}
	}
	return getComputedStyle(view.dom).direction == "rtl" ? "rtl" : "ltr";
}
function selectVertically(view, dir, mods) {
	let sel = view.state.selection;
	if (sel instanceof TextSelection && !sel.empty || mods.indexOf("s") > -1) return false;
	if (mac$2 && mods.indexOf("m") > -1) return false;
	let { $from, $to } = sel;
	if (!$from.parent.inlineContent || view.endOfTextblock(dir < 0 ? "up" : "down")) {
		let next = moveSelectionBlock(view.state, dir);
		if (next && next instanceof NodeSelection) return apply(view, next);
	}
	if (!$from.parent.inlineContent) {
		let side = dir < 0 ? $from : $to;
		let beyond = sel instanceof AllSelection ? Selection.near(side, dir) : Selection.findFrom(side, dir);
		return beyond ? apply(view, beyond) : false;
	}
	return false;
}
function stopNativeHorizontalDelete(view, dir) {
	if (!(view.state.selection instanceof TextSelection)) return true;
	let { $head, $anchor, empty: empty$1 } = view.state.selection;
	if (!$head.sameParent($anchor)) return true;
	if (!empty$1) return false;
	if (view.endOfTextblock(dir > 0 ? "forward" : "backward")) return true;
	let nextNode = !$head.textOffset && (dir < 0 ? $head.nodeBefore : $head.nodeAfter);
	if (nextNode && !nextNode.isText) {
		let tr$1 = view.state.tr;
		if (dir < 0) tr$1.delete($head.pos - nextNode.nodeSize, $head.pos);
		else tr$1.delete($head.pos, $head.pos + nextNode.nodeSize);
		view.dispatch(tr$1);
		return true;
	}
	return false;
}
function switchEditable(view, node, state) {
	view.domObserver.stop();
	node.contentEditable = state;
	view.domObserver.start();
}
function safariDownArrowBug(view) {
	if (!safari || view.state.selection.$head.parentOffset > 0) return false;
	let { focusNode, focusOffset } = view.domSelectionRange();
	if (focusNode && focusNode.nodeType == 1 && focusOffset == 0 && focusNode.firstChild && focusNode.firstChild.contentEditable == "false") {
		let child = focusNode.firstChild;
		switchEditable(view, child, "true");
		setTimeout(() => switchEditable(view, child, "false"), 20);
	}
	return false;
}
function getMods(event) {
	let result = "";
	if (event.ctrlKey) result += "c";
	if (event.metaKey) result += "m";
	if (event.altKey) result += "a";
	if (event.shiftKey) result += "s";
	return result;
}
function captureKeyDown(view, event) {
	let code$1 = event.keyCode, mods = getMods(event);
	if (code$1 == 8 || mac$2 && code$1 == 72 && mods == "c") return stopNativeHorizontalDelete(view, -1) || skipIgnoredNodes(view, -1);
	else if (code$1 == 46 && !event.shiftKey || mac$2 && code$1 == 68 && mods == "c") return stopNativeHorizontalDelete(view, 1) || skipIgnoredNodes(view, 1);
	else if (code$1 == 13 || code$1 == 27) return true;
	else if (code$1 == 37 || mac$2 && code$1 == 66 && mods == "c") {
		let dir = code$1 == 37 ? findDirection(view, view.state.selection.from) == "ltr" ? -1 : 1 : -1;
		return selectHorizontally(view, dir, mods) || skipIgnoredNodes(view, dir);
	} else if (code$1 == 39 || mac$2 && code$1 == 70 && mods == "c") {
		let dir = code$1 == 39 ? findDirection(view, view.state.selection.from) == "ltr" ? 1 : -1 : 1;
		return selectHorizontally(view, dir, mods) || skipIgnoredNodes(view, dir);
	} else if (code$1 == 38 || mac$2 && code$1 == 80 && mods == "c") return selectVertically(view, -1, mods) || skipIgnoredNodes(view, -1);
	else if (code$1 == 40 || mac$2 && code$1 == 78 && mods == "c") return safariDownArrowBug(view) || selectVertically(view, 1, mods) || skipIgnoredNodes(view, 1);
	else if (mods == (mac$2 ? "m" : "c") && (code$1 == 66 || code$1 == 73 || code$1 == 89 || code$1 == 90)) return true;
	return false;
}
function serializeForClipboard(view, slice) {
	view.someProp("transformCopied", (f) => {
		slice = f(slice, view);
	});
	let context = [], { content, openStart, openEnd } = slice;
	while (openStart > 1 && openEnd > 1 && content.childCount == 1 && content.firstChild.childCount == 1) {
		openStart--;
		openEnd--;
		let node = content.firstChild;
		context.push(node.type.name, node.attrs != node.type.defaultAttrs ? node.attrs : null);
		content = node.content;
	}
	let serializer = view.someProp("clipboardSerializer") || DOMSerializer.fromSchema(view.state.schema);
	let doc$2 = detachedDoc(), wrap$1 = doc$2.createElement("div");
	wrap$1.appendChild(serializer.serializeFragment(content, { document: doc$2 }));
	let firstChild = wrap$1.firstChild, needsWrap, wrappers = 0;
	while (firstChild && firstChild.nodeType == 1 && (needsWrap = wrapMap[firstChild.nodeName.toLowerCase()])) {
		for (let i$1 = needsWrap.length - 1; i$1 >= 0; i$1--) {
			let wrapper = doc$2.createElement(needsWrap[i$1]);
			while (wrap$1.firstChild) wrapper.appendChild(wrap$1.firstChild);
			wrap$1.appendChild(wrapper);
			wrappers++;
		}
		firstChild = wrap$1.firstChild;
	}
	if (firstChild && firstChild.nodeType == 1) firstChild.setAttribute("data-pm-slice", `${openStart} ${openEnd}${wrappers ? ` -${wrappers}` : ""} ${JSON.stringify(context)}`);
	return {
		dom: wrap$1,
		text: view.someProp("clipboardTextSerializer", (f) => f(slice, view)) || slice.content.textBetween(0, slice.content.size, "\n\n"),
		slice
	};
}
function parseFromClipboard(view, text, html, plainText, $context) {
	let inCode = $context.parent.type.spec.code;
	let dom, slice;
	if (!html && !text) return null;
	let asText = !!text && (plainText || inCode || !html);
	if (asText) {
		view.someProp("transformPastedText", (f) => {
			text = f(text, inCode || plainText, view);
		});
		if (inCode) {
			slice = new Slice(Fragment.from(view.state.schema.text(text.replace(/\r\n?/g, "\n"))), 0, 0);
			view.someProp("transformPasted", (f) => {
				slice = f(slice, view, true);
			});
			return slice;
		}
		let parsed = view.someProp("clipboardTextParser", (f) => f(text, $context, plainText, view));
		if (parsed) slice = parsed;
		else {
			let marks = $context.marks();
			let { schema } = view.state, serializer = DOMSerializer.fromSchema(schema);
			dom = document.createElement("div");
			text.split(/(?:\r\n?|\n)+/).forEach((block) => {
				let p = dom.appendChild(document.createElement("p"));
				if (block) p.appendChild(serializer.serializeNode(schema.text(block, marks)));
			});
		}
	} else {
		view.someProp("transformPastedHTML", (f) => {
			html = f(html, view);
		});
		dom = readHTML(html);
		if (webkit) restoreReplacedSpaces(dom);
	}
	let contextNode = dom && dom.querySelector("[data-pm-slice]");
	let sliceData = contextNode && /^(\d+) (\d+)(?: -(\d+))? (.*)/.exec(contextNode.getAttribute("data-pm-slice") || "");
	if (sliceData && sliceData[3]) for (let i$1 = +sliceData[3]; i$1 > 0; i$1--) {
		let child = dom.firstChild;
		while (child && child.nodeType != 1) child = child.nextSibling;
		if (!child) break;
		dom = child;
	}
	if (!slice) slice = (view.someProp("clipboardParser") || view.someProp("domParser") || DOMParser.fromSchema(view.state.schema)).parseSlice(dom, {
		preserveWhitespace: !!(asText || sliceData),
		context: $context,
		ruleFromNode(dom$1) {
			if (dom$1.nodeName == "BR" && !dom$1.nextSibling && dom$1.parentNode && !inlineParents.test(dom$1.parentNode.nodeName)) return { ignore: true };
			return null;
		}
	});
	if (sliceData) slice = addContext(closeSlice(slice, +sliceData[1], +sliceData[2]), sliceData[4]);
	else {
		slice = Slice.maxOpen(normalizeSiblings(slice.content, $context), true);
		if (slice.openStart || slice.openEnd) {
			let openStart = 0, openEnd = 0;
			for (let node = slice.content.firstChild; openStart < slice.openStart && !node.type.spec.isolating; openStart++, node = node.firstChild);
			for (let node = slice.content.lastChild; openEnd < slice.openEnd && !node.type.spec.isolating; openEnd++, node = node.lastChild);
			slice = closeSlice(slice, openStart, openEnd);
		}
	}
	view.someProp("transformPasted", (f) => {
		slice = f(slice, view, asText);
	});
	return slice;
}
var inlineParents = /^(a|abbr|acronym|b|cite|code|del|em|i|ins|kbd|label|output|q|ruby|s|samp|span|strong|sub|sup|time|u|tt|var)$/i;
function normalizeSiblings(fragment, $context) {
	if (fragment.childCount < 2) return fragment;
	for (let d = $context.depth; d >= 0; d--) {
		let match = $context.node(d).contentMatchAt($context.index(d));
		let lastWrap, result = [];
		fragment.forEach((node) => {
			if (!result) return;
			let wrap$1 = match.findWrapping(node.type), inLast;
			if (!wrap$1) return result = null;
			if (inLast = result.length && lastWrap.length && addToSibling(wrap$1, lastWrap, node, result[result.length - 1], 0)) result[result.length - 1] = inLast;
			else {
				if (result.length) result[result.length - 1] = closeRight(result[result.length - 1], lastWrap.length);
				let wrapped = withWrappers(node, wrap$1);
				result.push(wrapped);
				match = match.matchType(wrapped.type);
				lastWrap = wrap$1;
			}
		});
		if (result) return Fragment.from(result);
	}
	return fragment;
}
function withWrappers(node, wrap$1, from = 0) {
	for (let i$1 = wrap$1.length - 1; i$1 >= from; i$1--) node = wrap$1[i$1].create(null, Fragment.from(node));
	return node;
}
function addToSibling(wrap$1, lastWrap, node, sibling, depth) {
	if (depth < wrap$1.length && depth < lastWrap.length && wrap$1[depth] == lastWrap[depth]) {
		let inner = addToSibling(wrap$1, lastWrap, node, sibling.lastChild, depth + 1);
		if (inner) return sibling.copy(sibling.content.replaceChild(sibling.childCount - 1, inner));
		if (sibling.contentMatchAt(sibling.childCount).matchType(depth == wrap$1.length - 1 ? node.type : wrap$1[depth + 1])) return sibling.copy(sibling.content.append(Fragment.from(withWrappers(node, wrap$1, depth + 1))));
	}
}
function closeRight(node, depth) {
	if (depth == 0) return node;
	let fragment = node.content.replaceChild(node.childCount - 1, closeRight(node.lastChild, depth - 1));
	let fill = node.contentMatchAt(node.childCount).fillBefore(Fragment.empty, true);
	return node.copy(fragment.append(fill));
}
function closeRange(fragment, side, from, to, depth, openEnd) {
	let node = side < 0 ? fragment.firstChild : fragment.lastChild, inner = node.content;
	if (fragment.childCount > 1) openEnd = 0;
	if (depth < to - 1) inner = closeRange(inner, side, from, to, depth + 1, openEnd);
	if (depth >= from) inner = side < 0 ? node.contentMatchAt(0).fillBefore(inner, openEnd <= depth).append(inner) : inner.append(node.contentMatchAt(node.childCount).fillBefore(Fragment.empty, true));
	return fragment.replaceChild(side < 0 ? 0 : fragment.childCount - 1, node.copy(inner));
}
function closeSlice(slice, openStart, openEnd) {
	if (openStart < slice.openStart) slice = new Slice(closeRange(slice.content, -1, openStart, slice.openStart, 0, slice.openEnd), openStart, slice.openEnd);
	if (openEnd < slice.openEnd) slice = new Slice(closeRange(slice.content, 1, openEnd, slice.openEnd, 0, 0), slice.openStart, openEnd);
	return slice;
}
var wrapMap = {
	thead: ["table"],
	tbody: ["table"],
	tfoot: ["table"],
	caption: ["table"],
	colgroup: ["table"],
	col: ["table", "colgroup"],
	tr: ["table", "tbody"],
	td: [
		"table",
		"tbody",
		"tr"
	],
	th: [
		"table",
		"tbody",
		"tr"
	]
};
var _detachedDoc = null;
function detachedDoc() {
	return _detachedDoc || (_detachedDoc = document.implementation.createHTMLDocument("title"));
}
var _policy = null;
function maybeWrapTrusted(html) {
	let trustedTypes = window.trustedTypes;
	if (!trustedTypes) return html;
	if (!_policy) _policy = trustedTypes.defaultPolicy || trustedTypes.createPolicy("ProseMirrorClipboard", { createHTML: (s) => s });
	return _policy.createHTML(html);
}
function readHTML(html) {
	let metas = /^(\s*<meta [^>]*>)*/.exec(html);
	if (metas) html = html.slice(metas[0].length);
	let elt = detachedDoc().createElement("div");
	let firstTag = /<([a-z][^>\s]+)/i.exec(html), wrap$1;
	if (wrap$1 = firstTag && wrapMap[firstTag[1].toLowerCase()]) html = wrap$1.map((n) => "<" + n + ">").join("") + html + wrap$1.map((n) => "</" + n + ">").reverse().join("");
	elt.innerHTML = maybeWrapTrusted(html);
	if (wrap$1) for (let i$1 = 0; i$1 < wrap$1.length; i$1++) elt = elt.querySelector(wrap$1[i$1]) || elt;
	return elt;
}
function restoreReplacedSpaces(dom) {
	let nodes = dom.querySelectorAll(chrome ? "span:not([class]):not([style])" : "span.Apple-converted-space");
	for (let i$1 = 0; i$1 < nodes.length; i$1++) {
		let node = nodes[i$1];
		if (node.childNodes.length == 1 && node.textContent == "\xA0" && node.parentNode) node.parentNode.replaceChild(dom.ownerDocument.createTextNode(" "), node);
	}
}
function addContext(slice, context) {
	if (!slice.size) return slice;
	let schema = slice.content.firstChild.type.schema, array;
	try {
		array = JSON.parse(context);
	} catch (e) {
		return slice;
	}
	let { content, openStart, openEnd } = slice;
	for (let i$1 = array.length - 2; i$1 >= 0; i$1 -= 2) {
		let type = schema.nodes[array[i$1]];
		if (!type || type.hasRequiredAttrs()) break;
		content = Fragment.from(type.create(array[i$1 + 1], content));
		openStart++;
		openEnd++;
	}
	return new Slice(content, openStart, openEnd);
}
var handlers = {};
var editHandlers = {};
var passiveHandlers = {
	touchstart: true,
	touchmove: true
};
var InputState = class {
	constructor() {
		this.shiftKey = false;
		this.mouseDown = null;
		this.lastKeyCode = null;
		this.lastKeyCodeTime = 0;
		this.lastClick = {
			time: 0,
			x: 0,
			y: 0,
			type: "",
			button: 0
		};
		this.lastSelectionOrigin = null;
		this.lastSelectionTime = 0;
		this.lastIOSEnter = 0;
		this.lastIOSEnterFallbackTimeout = -1;
		this.lastFocus = 0;
		this.lastTouch = 0;
		this.lastChromeDelete = 0;
		this.composing = false;
		this.compositionNode = null;
		this.composingTimeout = -1;
		this.compositionNodes = [];
		this.compositionEndedAt = -2e8;
		this.compositionID = 1;
		this.badSafariComposition = false;
		this.compositionPendingChanges = 0;
		this.domChangeCount = 0;
		this.eventHandlers = Object.create(null);
		this.hideSelectionGuard = null;
	}
};
function initInput(view) {
	for (let event in handlers) {
		let handler = handlers[event];
		view.dom.addEventListener(event, view.input.eventHandlers[event] = (event$1) => {
			if (eventBelongsToView(view, event$1) && !runCustomHandler(view, event$1) && (view.editable || !(event$1.type in editHandlers))) handler(view, event$1);
		}, passiveHandlers[event] ? { passive: true } : void 0);
	}
	if (safari) view.dom.addEventListener("input", () => null);
	ensureListeners(view);
}
function setSelectionOrigin(view, origin) {
	view.input.lastSelectionOrigin = origin;
	view.input.lastSelectionTime = Date.now();
}
function destroyInput(view) {
	view.domObserver.stop();
	for (let type in view.input.eventHandlers) view.dom.removeEventListener(type, view.input.eventHandlers[type]);
	clearTimeout(view.input.composingTimeout);
	clearTimeout(view.input.lastIOSEnterFallbackTimeout);
}
function ensureListeners(view) {
	view.someProp("handleDOMEvents", (currentHandlers) => {
		for (let type in currentHandlers) if (!view.input.eventHandlers[type]) view.dom.addEventListener(type, view.input.eventHandlers[type] = (event) => runCustomHandler(view, event));
	});
}
function runCustomHandler(view, event) {
	return view.someProp("handleDOMEvents", (handlers$1) => {
		let handler = handlers$1[event.type];
		return handler ? handler(view, event) || event.defaultPrevented : false;
	});
}
function eventBelongsToView(view, event) {
	if (!event.bubbles) return true;
	if (event.defaultPrevented) return false;
	for (let node = event.target; node != view.dom; node = node.parentNode) if (!node || node.nodeType == 11 || node.pmViewDesc && node.pmViewDesc.stopEvent(event)) return false;
	return true;
}
function dispatchEvent(view, event) {
	if (!runCustomHandler(view, event) && handlers[event.type] && (view.editable || !(event.type in editHandlers))) handlers[event.type](view, event);
}
editHandlers.keydown = (view, _event) => {
	let event = _event;
	view.input.shiftKey = event.keyCode == 16 || event.shiftKey;
	if (inOrNearComposition(view, event)) return;
	view.input.lastKeyCode = event.keyCode;
	view.input.lastKeyCodeTime = Date.now();
	if (android && chrome && event.keyCode == 13) return;
	if (event.keyCode != 229) view.domObserver.forceFlush();
	if (ios && event.keyCode == 13 && !event.ctrlKey && !event.altKey && !event.metaKey) {
		let now = Date.now();
		view.input.lastIOSEnter = now;
		view.input.lastIOSEnterFallbackTimeout = setTimeout(() => {
			if (view.input.lastIOSEnter == now) {
				view.someProp("handleKeyDown", (f) => f(view, keyEvent(13, "Enter")));
				view.input.lastIOSEnter = 0;
			}
		}, 200);
	} else if (view.someProp("handleKeyDown", (f) => f(view, event)) || captureKeyDown(view, event)) event.preventDefault();
	else setSelectionOrigin(view, "key");
};
editHandlers.keyup = (view, event) => {
	if (event.keyCode == 16) view.input.shiftKey = false;
};
editHandlers.keypress = (view, _event) => {
	let event = _event;
	if (inOrNearComposition(view, event) || !event.charCode || event.ctrlKey && !event.altKey || mac$2 && event.metaKey) return;
	if (view.someProp("handleKeyPress", (f) => f(view, event))) {
		event.preventDefault();
		return;
	}
	let sel = view.state.selection;
	if (!(sel instanceof TextSelection) || !sel.$from.sameParent(sel.$to)) {
		let text = String.fromCharCode(event.charCode);
		let deflt = () => view.state.tr.insertText(text).scrollIntoView();
		if (!/[\r\n]/.test(text) && !view.someProp("handleTextInput", (f) => f(view, sel.$from.pos, sel.$to.pos, text, deflt))) view.dispatch(deflt());
		event.preventDefault();
	}
};
function eventCoords(event) {
	return {
		left: event.clientX,
		top: event.clientY
	};
}
function isNear(event, click) {
	let dx = click.x - event.clientX, dy = click.y - event.clientY;
	return dx * dx + dy * dy < 100;
}
function runHandlerOnContext(view, propName, pos, inside, event) {
	if (inside == -1) return false;
	let $pos = view.state.doc.resolve(inside);
	for (let i$1 = $pos.depth + 1; i$1 > 0; i$1--) if (view.someProp(propName, (f) => i$1 > $pos.depth ? f(view, pos, $pos.nodeAfter, $pos.before(i$1), event, true) : f(view, pos, $pos.node(i$1), $pos.before(i$1), event, false))) return true;
	return false;
}
function updateSelection(view, selection, origin) {
	if (!view.focused) view.focus();
	if (view.state.selection.eq(selection)) return;
	let tr$1 = view.state.tr.setSelection(selection);
	if (origin == "pointer") tr$1.setMeta("pointer", true);
	view.dispatch(tr$1);
}
function selectClickedLeaf(view, inside) {
	if (inside == -1) return false;
	let $pos = view.state.doc.resolve(inside), node = $pos.nodeAfter;
	if (node && node.isAtom && NodeSelection.isSelectable(node)) {
		updateSelection(view, new NodeSelection($pos), "pointer");
		return true;
	}
	return false;
}
function selectClickedNode(view, inside) {
	if (inside == -1) return false;
	let sel = view.state.selection, selectedNode, selectAt;
	if (sel instanceof NodeSelection) selectedNode = sel.node;
	let $pos = view.state.doc.resolve(inside);
	for (let i$1 = $pos.depth + 1; i$1 > 0; i$1--) {
		let node = i$1 > $pos.depth ? $pos.nodeAfter : $pos.node(i$1);
		if (NodeSelection.isSelectable(node)) {
			if (selectedNode && sel.$from.depth > 0 && i$1 >= sel.$from.depth && $pos.before(sel.$from.depth + 1) == sel.$from.pos) selectAt = $pos.before(sel.$from.depth);
			else selectAt = $pos.before(i$1);
			break;
		}
	}
	if (selectAt != null) {
		updateSelection(view, NodeSelection.create(view.state.doc, selectAt), "pointer");
		return true;
	} else return false;
}
function handleSingleClick(view, pos, inside, event, selectNode) {
	return runHandlerOnContext(view, "handleClickOn", pos, inside, event) || view.someProp("handleClick", (f) => f(view, pos, event)) || (selectNode ? selectClickedNode(view, inside) : selectClickedLeaf(view, inside));
}
function handleDoubleClick(view, pos, inside, event) {
	return runHandlerOnContext(view, "handleDoubleClickOn", pos, inside, event) || view.someProp("handleDoubleClick", (f) => f(view, pos, event));
}
function handleTripleClick$1(view, pos, inside, event) {
	return runHandlerOnContext(view, "handleTripleClickOn", pos, inside, event) || view.someProp("handleTripleClick", (f) => f(view, pos, event)) || defaultTripleClick(view, inside, event);
}
function defaultTripleClick(view, inside, event) {
	if (event.button != 0) return false;
	let doc$2 = view.state.doc;
	if (inside == -1) {
		if (doc$2.inlineContent) {
			updateSelection(view, TextSelection.create(doc$2, 0, doc$2.content.size), "pointer");
			return true;
		}
		return false;
	}
	let $pos = doc$2.resolve(inside);
	for (let i$1 = $pos.depth + 1; i$1 > 0; i$1--) {
		let node = i$1 > $pos.depth ? $pos.nodeAfter : $pos.node(i$1);
		let nodePos = $pos.before(i$1);
		if (node.inlineContent) updateSelection(view, TextSelection.create(doc$2, nodePos + 1, nodePos + 1 + node.content.size), "pointer");
		else if (NodeSelection.isSelectable(node)) updateSelection(view, NodeSelection.create(doc$2, nodePos), "pointer");
		else continue;
		return true;
	}
}
function forceDOMFlush(view) {
	return endComposition(view);
}
var selectNodeModifier = mac$2 ? "metaKey" : "ctrlKey";
handlers.mousedown = (view, _event) => {
	let event = _event;
	view.input.shiftKey = event.shiftKey;
	let flushed = forceDOMFlush(view);
	let now = Date.now(), type = "singleClick";
	if (now - view.input.lastClick.time < 500 && isNear(event, view.input.lastClick) && !event[selectNodeModifier] && view.input.lastClick.button == event.button) {
		if (view.input.lastClick.type == "singleClick") type = "doubleClick";
		else if (view.input.lastClick.type == "doubleClick") type = "tripleClick";
	}
	view.input.lastClick = {
		time: now,
		x: event.clientX,
		y: event.clientY,
		type,
		button: event.button
	};
	let pos = view.posAtCoords(eventCoords(event));
	if (!pos) return;
	if (type == "singleClick") {
		if (view.input.mouseDown) view.input.mouseDown.done();
		view.input.mouseDown = new MouseDown(view, pos, event, !!flushed);
	} else if ((type == "doubleClick" ? handleDoubleClick : handleTripleClick$1)(view, pos.pos, pos.inside, event)) event.preventDefault();
	else setSelectionOrigin(view, "pointer");
};
var MouseDown = class {
	constructor(view, pos, event, flushed) {
		this.view = view;
		this.pos = pos;
		this.event = event;
		this.flushed = flushed;
		this.delayedSelectionSync = false;
		this.mightDrag = null;
		this.startDoc = view.state.doc;
		this.selectNode = !!event[selectNodeModifier];
		this.allowDefault = event.shiftKey;
		let targetNode, targetPos;
		if (pos.inside > -1) {
			targetNode = view.state.doc.nodeAt(pos.inside);
			targetPos = pos.inside;
		} else {
			let $pos = view.state.doc.resolve(pos.pos);
			targetNode = $pos.parent;
			targetPos = $pos.depth ? $pos.before() : 0;
		}
		const target = flushed ? null : event.target;
		const targetDesc = target ? view.docView.nearestDesc(target, true) : null;
		this.target = targetDesc && targetDesc.nodeDOM.nodeType == 1 ? targetDesc.nodeDOM : null;
		let { selection } = view.state;
		if (event.button == 0 && (targetNode.type.spec.draggable && targetNode.type.spec.selectable !== false || selection instanceof NodeSelection && selection.from <= targetPos && selection.to > targetPos)) this.mightDrag = {
			node: targetNode,
			pos: targetPos,
			addAttr: !!(this.target && !this.target.draggable),
			setUneditable: !!(this.target && gecko && !this.target.hasAttribute("contentEditable"))
		};
		if (this.target && this.mightDrag && (this.mightDrag.addAttr || this.mightDrag.setUneditable)) {
			this.view.domObserver.stop();
			if (this.mightDrag.addAttr) this.target.draggable = true;
			if (this.mightDrag.setUneditable) setTimeout(() => {
				if (this.view.input.mouseDown == this) this.target.setAttribute("contentEditable", "false");
			}, 20);
			this.view.domObserver.start();
		}
		view.root.addEventListener("mouseup", this.up = this.up.bind(this));
		view.root.addEventListener("mousemove", this.move = this.move.bind(this));
		setSelectionOrigin(view, "pointer");
	}
	done() {
		this.view.root.removeEventListener("mouseup", this.up);
		this.view.root.removeEventListener("mousemove", this.move);
		if (this.mightDrag && this.target) {
			this.view.domObserver.stop();
			if (this.mightDrag.addAttr) this.target.removeAttribute("draggable");
			if (this.mightDrag.setUneditable) this.target.removeAttribute("contentEditable");
			this.view.domObserver.start();
		}
		if (this.delayedSelectionSync) setTimeout(() => selectionToDOM(this.view));
		this.view.input.mouseDown = null;
	}
	up(event) {
		this.done();
		if (!this.view.dom.contains(event.target)) return;
		let pos = this.pos;
		if (this.view.state.doc != this.startDoc) pos = this.view.posAtCoords(eventCoords(event));
		this.updateAllowDefault(event);
		if (this.allowDefault || !pos) setSelectionOrigin(this.view, "pointer");
		else if (handleSingleClick(this.view, pos.pos, pos.inside, event, this.selectNode)) event.preventDefault();
		else if (event.button == 0 && (this.flushed || safari && this.mightDrag && !this.mightDrag.node.isAtom || chrome && !this.view.state.selection.visible && Math.min(Math.abs(pos.pos - this.view.state.selection.from), Math.abs(pos.pos - this.view.state.selection.to)) <= 2)) {
			updateSelection(this.view, Selection.near(this.view.state.doc.resolve(pos.pos)), "pointer");
			event.preventDefault();
		} else setSelectionOrigin(this.view, "pointer");
	}
	move(event) {
		this.updateAllowDefault(event);
		setSelectionOrigin(this.view, "pointer");
		if (event.buttons == 0) this.done();
	}
	updateAllowDefault(event) {
		if (!this.allowDefault && (Math.abs(this.event.x - event.clientX) > 4 || Math.abs(this.event.y - event.clientY) > 4)) this.allowDefault = true;
	}
};
handlers.touchstart = (view) => {
	view.input.lastTouch = Date.now();
	forceDOMFlush(view);
	setSelectionOrigin(view, "pointer");
};
handlers.touchmove = (view) => {
	view.input.lastTouch = Date.now();
	setSelectionOrigin(view, "pointer");
};
handlers.contextmenu = (view) => forceDOMFlush(view);
function inOrNearComposition(view, event) {
	if (view.composing) return true;
	if (safari && Math.abs(event.timeStamp - view.input.compositionEndedAt) < 500) {
		view.input.compositionEndedAt = -2e8;
		return true;
	}
	return false;
}
var timeoutComposition = android ? 5e3 : -1;
editHandlers.compositionstart = editHandlers.compositionupdate = (view) => {
	if (!view.composing) {
		view.domObserver.flush();
		let { state } = view, $pos = state.selection.$to;
		if (state.selection instanceof TextSelection && (state.storedMarks || !$pos.textOffset && $pos.parentOffset && $pos.nodeBefore.marks.some((m$1) => m$1.type.spec.inclusive === false) || chrome && windows$1 && selectionBeforeUneditable(view))) {
			view.markCursor = view.state.storedMarks || $pos.marks();
			endComposition(view, true);
			view.markCursor = null;
		} else {
			endComposition(view, !state.selection.empty);
			if (gecko && state.selection.empty && $pos.parentOffset && !$pos.textOffset && $pos.nodeBefore.marks.length) {
				let sel = view.domSelectionRange();
				for (let node = sel.focusNode, offset = sel.focusOffset; node && node.nodeType == 1 && offset != 0;) {
					let before = offset < 0 ? node.lastChild : node.childNodes[offset - 1];
					if (!before) break;
					if (before.nodeType == 3) {
						let sel$1 = view.domSelection();
						if (sel$1) sel$1.collapse(before, before.nodeValue.length);
						break;
					} else {
						node = before;
						offset = -1;
					}
				}
			}
		}
		view.input.composing = true;
	}
	scheduleComposeEnd(view, timeoutComposition);
};
function selectionBeforeUneditable(view) {
	let { focusNode, focusOffset } = view.domSelectionRange();
	if (!focusNode || focusNode.nodeType != 1 || focusOffset >= focusNode.childNodes.length) return false;
	let next = focusNode.childNodes[focusOffset];
	return next.nodeType == 1 && next.contentEditable == "false";
}
editHandlers.compositionend = (view, event) => {
	if (view.composing) {
		view.input.composing = false;
		view.input.compositionEndedAt = event.timeStamp;
		view.input.compositionPendingChanges = view.domObserver.pendingRecords().length ? view.input.compositionID : 0;
		view.input.compositionNode = null;
		if (view.input.badSafariComposition) view.domObserver.forceFlush();
		else if (view.input.compositionPendingChanges) Promise.resolve().then(() => view.domObserver.flush());
		view.input.compositionID++;
		scheduleComposeEnd(view, 20);
	}
};
function scheduleComposeEnd(view, delay) {
	clearTimeout(view.input.composingTimeout);
	if (delay > -1) view.input.composingTimeout = setTimeout(() => endComposition(view), delay);
}
function clearComposition(view) {
	if (view.composing) {
		view.input.composing = false;
		view.input.compositionEndedAt = timestampFromCustomEvent();
	}
	while (view.input.compositionNodes.length > 0) view.input.compositionNodes.pop().markParentsDirty();
}
function findCompositionNode(view) {
	let sel = view.domSelectionRange();
	if (!sel.focusNode) return null;
	let textBefore = textNodeBefore$1(sel.focusNode, sel.focusOffset);
	let textAfter = textNodeAfter$1(sel.focusNode, sel.focusOffset);
	if (textBefore && textAfter && textBefore != textAfter) {
		let descAfter = textAfter.pmViewDesc, lastChanged = view.domObserver.lastChangedTextNode;
		if (textBefore == lastChanged || textAfter == lastChanged) return lastChanged;
		if (!descAfter || !descAfter.isText(textAfter.nodeValue)) return textAfter;
		else if (view.input.compositionNode == textAfter) {
			let descBefore = textBefore.pmViewDesc;
			if (!(!descBefore || !descBefore.isText(textBefore.nodeValue))) return textAfter;
		}
	}
	return textBefore || textAfter;
}
function timestampFromCustomEvent() {
	let event = document.createEvent("Event");
	event.initEvent("event", true, true);
	return event.timeStamp;
}
function endComposition(view, restarting = false) {
	if (android && view.domObserver.flushingSoon >= 0) return;
	view.domObserver.forceFlush();
	clearComposition(view);
	if (restarting || view.docView && view.docView.dirty) {
		let sel = selectionFromDOM(view), cur = view.state.selection;
		if (sel && !sel.eq(cur)) view.dispatch(view.state.tr.setSelection(sel));
		else if ((view.markCursor || restarting) && !cur.$from.node(cur.$from.sharedDepth(cur.to)).inlineContent) view.dispatch(view.state.tr.deleteSelection());
		else view.updateState(view.state);
		return true;
	}
	return false;
}
function captureCopy(view, dom) {
	if (!view.dom.parentNode) return;
	let wrap$1 = view.dom.parentNode.appendChild(document.createElement("div"));
	wrap$1.appendChild(dom);
	wrap$1.style.cssText = "position: fixed; left: -10000px; top: 10px";
	let sel = getSelection(), range = document.createRange();
	range.selectNodeContents(dom);
	view.dom.blur();
	sel.removeAllRanges();
	sel.addRange(range);
	setTimeout(() => {
		if (wrap$1.parentNode) wrap$1.parentNode.removeChild(wrap$1);
		view.focus();
	}, 50);
}
var brokenClipboardAPI = ie$2 && ie_version < 15 || ios && webkit_version < 604;
handlers.copy = editHandlers.cut = (view, _event) => {
	let event = _event;
	let sel = view.state.selection, cut$1 = event.type == "cut";
	if (sel.empty) return;
	let data = brokenClipboardAPI ? null : event.clipboardData;
	let { dom, text } = serializeForClipboard(view, sel.content());
	if (data) {
		event.preventDefault();
		data.clearData();
		data.setData("text/html", dom.innerHTML);
		data.setData("text/plain", text);
	} else captureCopy(view, dom);
	if (cut$1) view.dispatch(view.state.tr.deleteSelection().scrollIntoView().setMeta("uiEvent", "cut"));
};
function sliceSingleNode(slice) {
	return slice.openStart == 0 && slice.openEnd == 0 && slice.content.childCount == 1 ? slice.content.firstChild : null;
}
function capturePaste(view, event) {
	if (!view.dom.parentNode) return;
	let plainText = view.input.shiftKey || view.state.selection.$from.parent.type.spec.code;
	let target = view.dom.parentNode.appendChild(document.createElement(plainText ? "textarea" : "div"));
	if (!plainText) target.contentEditable = "true";
	target.style.cssText = "position: fixed; left: -10000px; top: 10px";
	target.focus();
	let plain = view.input.shiftKey && view.input.lastKeyCode != 45;
	setTimeout(() => {
		view.focus();
		if (target.parentNode) target.parentNode.removeChild(target);
		if (plainText) doPaste(view, target.value, null, plain, event);
		else doPaste(view, target.textContent, target.innerHTML, plain, event);
	}, 50);
}
function doPaste(view, text, html, preferPlain, event) {
	let slice = parseFromClipboard(view, text, html, preferPlain, view.state.selection.$from);
	if (view.someProp("handlePaste", (f) => f(view, event, slice || Slice.empty))) return true;
	if (!slice) return false;
	let singleNode = sliceSingleNode(slice);
	let tr$1 = singleNode ? view.state.tr.replaceSelectionWith(singleNode, preferPlain) : view.state.tr.replaceSelection(slice);
	view.dispatch(tr$1.scrollIntoView().setMeta("paste", true).setMeta("uiEvent", "paste"));
	return true;
}
function getText$1(clipboardData) {
	let text = clipboardData.getData("text/plain") || clipboardData.getData("Text");
	if (text) return text;
	let uris = clipboardData.getData("text/uri-list");
	return uris ? uris.replace(/\r?\n/g, " ") : "";
}
editHandlers.paste = (view, _event) => {
	let event = _event;
	if (view.composing && !android) return;
	let data = brokenClipboardAPI ? null : event.clipboardData;
	let plain = view.input.shiftKey && view.input.lastKeyCode != 45;
	if (data && doPaste(view, getText$1(data), data.getData("text/html"), plain, event)) event.preventDefault();
	else capturePaste(view, event);
};
var Dragging = class {
	constructor(slice, move, node) {
		this.slice = slice;
		this.move = move;
		this.node = node;
	}
};
var dragCopyModifier = mac$2 ? "altKey" : "ctrlKey";
function dragMoves(view, event) {
	let copy$1;
	view.someProp("dragCopies", (test) => {
		copy$1 = copy$1 || test(event);
	});
	return copy$1 != null ? !copy$1 : !event[dragCopyModifier];
}
handlers.dragstart = (view, _event) => {
	let event = _event;
	let mouseDown = view.input.mouseDown;
	if (mouseDown) mouseDown.done();
	if (!event.dataTransfer) return;
	let sel = view.state.selection;
	let pos = sel.empty ? null : view.posAtCoords(eventCoords(event));
	let node;
	if (pos && pos.pos >= sel.from && pos.pos <= (sel instanceof NodeSelection ? sel.to - 1 : sel.to));
	else if (mouseDown && mouseDown.mightDrag) node = NodeSelection.create(view.state.doc, mouseDown.mightDrag.pos);
	else if (event.target && event.target.nodeType == 1) {
		let desc = view.docView.nearestDesc(event.target, true);
		if (desc && desc.node.type.spec.draggable && desc != view.docView) node = NodeSelection.create(view.state.doc, desc.posBefore);
	}
	let { dom, text, slice } = serializeForClipboard(view, (node || view.state.selection).content());
	if (!event.dataTransfer.files.length || !chrome || chrome_version > 120) event.dataTransfer.clearData();
	event.dataTransfer.setData(brokenClipboardAPI ? "Text" : "text/html", dom.innerHTML);
	event.dataTransfer.effectAllowed = "copyMove";
	if (!brokenClipboardAPI) event.dataTransfer.setData("text/plain", text);
	view.dragging = new Dragging(slice, dragMoves(view, event), node);
};
handlers.dragend = (view) => {
	let dragging = view.dragging;
	window.setTimeout(() => {
		if (view.dragging == dragging) view.dragging = null;
	}, 50);
};
editHandlers.dragover = editHandlers.dragenter = (_$1, e) => e.preventDefault();
editHandlers.drop = (view, event) => {
	try {
		handleDrop(view, event, view.dragging);
	} finally {
		view.dragging = null;
	}
};
function handleDrop(view, event, dragging) {
	if (!event.dataTransfer) return;
	let eventPos = view.posAtCoords(eventCoords(event));
	if (!eventPos) return;
	let $mouse = view.state.doc.resolve(eventPos.pos);
	let slice = dragging && dragging.slice;
	if (slice) view.someProp("transformPasted", (f) => {
		slice = f(slice, view, false);
	});
	else slice = parseFromClipboard(view, getText$1(event.dataTransfer), brokenClipboardAPI ? null : event.dataTransfer.getData("text/html"), false, $mouse);
	let move = !!(dragging && dragMoves(view, event));
	if (view.someProp("handleDrop", (f) => f(view, event, slice || Slice.empty, move))) {
		event.preventDefault();
		return;
	}
	if (!slice) return;
	event.preventDefault();
	let insertPos = slice ? dropPoint(view.state.doc, $mouse.pos, slice) : $mouse.pos;
	if (insertPos == null) insertPos = $mouse.pos;
	let tr$1 = view.state.tr;
	if (move) {
		let { node } = dragging;
		if (node) node.replace(tr$1);
		else tr$1.deleteSelection();
	}
	let pos = tr$1.mapping.map(insertPos);
	let isNode = slice.openStart == 0 && slice.openEnd == 0 && slice.content.childCount == 1;
	let beforeInsert = tr$1.doc;
	if (isNode) tr$1.replaceRangeWith(pos, pos, slice.content.firstChild);
	else tr$1.replaceRange(pos, pos, slice);
	if (tr$1.doc.eq(beforeInsert)) return;
	let $pos = tr$1.doc.resolve(pos);
	if (isNode && NodeSelection.isSelectable(slice.content.firstChild) && $pos.nodeAfter && $pos.nodeAfter.sameMarkup(slice.content.firstChild)) tr$1.setSelection(new NodeSelection($pos));
	else {
		let end = tr$1.mapping.map(insertPos);
		tr$1.mapping.maps[tr$1.mapping.maps.length - 1].forEach((_from, _to, _newFrom, newTo) => end = newTo);
		tr$1.setSelection(selectionBetween(view, $pos, tr$1.doc.resolve(end)));
	}
	view.focus();
	view.dispatch(tr$1.setMeta("uiEvent", "drop"));
}
handlers.focus = (view) => {
	view.input.lastFocus = Date.now();
	if (!view.focused) {
		view.domObserver.stop();
		view.dom.classList.add("ProseMirror-focused");
		view.domObserver.start();
		view.focused = true;
		setTimeout(() => {
			if (view.docView && view.hasFocus() && !view.domObserver.currentSelection.eq(view.domSelectionRange())) selectionToDOM(view);
		}, 20);
	}
};
handlers.blur = (view, _event) => {
	let event = _event;
	if (view.focused) {
		view.domObserver.stop();
		view.dom.classList.remove("ProseMirror-focused");
		view.domObserver.start();
		if (event.relatedTarget && view.dom.contains(event.relatedTarget)) view.domObserver.currentSelection.clear();
		view.focused = false;
	}
};
handlers.beforeinput = (view, _event) => {
	if (chrome && android && _event.inputType == "deleteContentBackward") {
		view.domObserver.flushSoon();
		let { domChangeCount } = view.input;
		setTimeout(() => {
			if (view.input.domChangeCount != domChangeCount) return;
			view.dom.blur();
			view.focus();
			if (view.someProp("handleKeyDown", (f) => f(view, keyEvent(8, "Backspace")))) return;
			let { $cursor } = view.state.selection;
			if ($cursor && $cursor.pos > 0) view.dispatch(view.state.tr.delete($cursor.pos - 1, $cursor.pos).scrollIntoView());
		}, 50);
	}
};
for (let prop in editHandlers) handlers[prop] = editHandlers[prop];
function compareObjs(a, b$1) {
	if (a == b$1) return true;
	for (let p in a) if (a[p] !== b$1[p]) return false;
	for (let p in b$1) if (!(p in a)) return false;
	return true;
}
var WidgetType = class WidgetType {
	constructor(toDOM, spec) {
		this.toDOM = toDOM;
		this.spec = spec || noSpec;
		this.side = this.spec.side || 0;
	}
	map(mapping, span, offset, oldOffset) {
		let { pos, deleted } = mapping.mapResult(span.from + oldOffset, this.side < 0 ? -1 : 1);
		return deleted ? null : new Decoration(pos - offset, pos - offset, this);
	}
	valid() {
		return true;
	}
	eq(other) {
		return this == other || other instanceof WidgetType && (this.spec.key && this.spec.key == other.spec.key || this.toDOM == other.toDOM && compareObjs(this.spec, other.spec));
	}
	destroy(node) {
		if (this.spec.destroy) this.spec.destroy(node);
	}
};
var InlineType = class InlineType {
	constructor(attrs, spec) {
		this.attrs = attrs;
		this.spec = spec || noSpec;
	}
	map(mapping, span, offset, oldOffset) {
		let from = mapping.map(span.from + oldOffset, this.spec.inclusiveStart ? -1 : 1) - offset;
		let to = mapping.map(span.to + oldOffset, this.spec.inclusiveEnd ? 1 : -1) - offset;
		return from >= to ? null : new Decoration(from, to, this);
	}
	valid(_$1, span) {
		return span.from < span.to;
	}
	eq(other) {
		return this == other || other instanceof InlineType && compareObjs(this.attrs, other.attrs) && compareObjs(this.spec, other.spec);
	}
	static is(span) {
		return span.type instanceof InlineType;
	}
	destroy() {}
};
var NodeType = class NodeType {
	constructor(attrs, spec) {
		this.attrs = attrs;
		this.spec = spec || noSpec;
	}
	map(mapping, span, offset, oldOffset) {
		let from = mapping.mapResult(span.from + oldOffset, 1);
		if (from.deleted) return null;
		let to = mapping.mapResult(span.to + oldOffset, -1);
		if (to.deleted || to.pos <= from.pos) return null;
		return new Decoration(from.pos - offset, to.pos - offset, this);
	}
	valid(node, span) {
		let { index, offset } = node.content.findIndex(span.from), child;
		return offset == span.from && !(child = node.child(index)).isText && offset + child.nodeSize == span.to;
	}
	eq(other) {
		return this == other || other instanceof NodeType && compareObjs(this.attrs, other.attrs) && compareObjs(this.spec, other.spec);
	}
	destroy() {}
};
var Decoration = class Decoration {
	constructor(from, to, type) {
		this.from = from;
		this.to = to;
		this.type = type;
	}
	copy(from, to) {
		return new Decoration(from, to, this.type);
	}
	eq(other, offset = 0) {
		return this.type.eq(other.type) && this.from + offset == other.from && this.to + offset == other.to;
	}
	map(mapping, offset, oldOffset) {
		return this.type.map(mapping, this, offset, oldOffset);
	}
	static widget(pos, toDOM, spec) {
		return new Decoration(pos, pos, new WidgetType(toDOM, spec));
	}
	static inline(from, to, attrs, spec) {
		return new Decoration(from, to, new InlineType(attrs, spec));
	}
	static node(from, to, attrs, spec) {
		return new Decoration(from, to, new NodeType(attrs, spec));
	}
	get spec() {
		return this.type.spec;
	}
	get inline() {
		return this.type instanceof InlineType;
	}
	get widget() {
		return this.type instanceof WidgetType;
	}
};
var none = [], noSpec = {};
var DecorationSet = class DecorationSet {
	constructor(local, children) {
		this.local = local.length ? local : none;
		this.children = children.length ? children : none;
	}
	static create(doc$2, decorations) {
		return decorations.length ? buildTree(decorations, doc$2, 0, noSpec) : empty;
	}
	find(start, end, predicate) {
		let result = [];
		this.findInner(start == null ? 0 : start, end == null ? 1e9 : end, result, 0, predicate);
		return result;
	}
	findInner(start, end, result, offset, predicate) {
		for (let i$1 = 0; i$1 < this.local.length; i$1++) {
			let span = this.local[i$1];
			if (span.from <= end && span.to >= start && (!predicate || predicate(span.spec))) result.push(span.copy(span.from + offset, span.to + offset));
		}
		for (let i$1 = 0; i$1 < this.children.length; i$1 += 3) if (this.children[i$1] < end && this.children[i$1 + 1] > start) {
			let childOff = this.children[i$1] + 1;
			this.children[i$1 + 2].findInner(start - childOff, end - childOff, result, offset + childOff, predicate);
		}
	}
	map(mapping, doc$2, options) {
		if (this == empty || mapping.maps.length == 0) return this;
		return this.mapInner(mapping, doc$2, 0, 0, options || noSpec);
	}
	mapInner(mapping, node, offset, oldOffset, options) {
		let newLocal;
		for (let i$1 = 0; i$1 < this.local.length; i$1++) {
			let mapped = this.local[i$1].map(mapping, offset, oldOffset);
			if (mapped && mapped.type.valid(node, mapped)) (newLocal || (newLocal = [])).push(mapped);
			else if (options.onRemove) options.onRemove(this.local[i$1].spec);
		}
		if (this.children.length) return mapChildren(this.children, newLocal || [], mapping, node, offset, oldOffset, options);
		else return newLocal ? new DecorationSet(newLocal.sort(byPos), none) : empty;
	}
	add(doc$2, decorations) {
		if (!decorations.length) return this;
		if (this == empty) return DecorationSet.create(doc$2, decorations);
		return this.addInner(doc$2, decorations, 0);
	}
	addInner(doc$2, decorations, offset) {
		let children, childIndex = 0;
		doc$2.forEach((childNode, childOffset) => {
			let baseOffset = childOffset + offset, found$1;
			if (!(found$1 = takeSpansForNode(decorations, childNode, baseOffset))) return;
			if (!children) children = this.children.slice();
			while (childIndex < children.length && children[childIndex] < childOffset) childIndex += 3;
			if (children[childIndex] == childOffset) children[childIndex + 2] = children[childIndex + 2].addInner(childNode, found$1, baseOffset + 1);
			else children.splice(childIndex, 0, childOffset, childOffset + childNode.nodeSize, buildTree(found$1, childNode, baseOffset + 1, noSpec));
			childIndex += 3;
		});
		let local = moveSpans(childIndex ? withoutNulls(decorations) : decorations, -offset);
		for (let i$1 = 0; i$1 < local.length; i$1++) if (!local[i$1].type.valid(doc$2, local[i$1])) local.splice(i$1--, 1);
		return new DecorationSet(local.length ? this.local.concat(local).sort(byPos) : this.local, children || this.children);
	}
	remove(decorations) {
		if (decorations.length == 0 || this == empty) return this;
		return this.removeInner(decorations, 0);
	}
	removeInner(decorations, offset) {
		let children = this.children, local = this.local;
		for (let i$1 = 0; i$1 < children.length; i$1 += 3) {
			let found$1;
			let from = children[i$1] + offset, to = children[i$1 + 1] + offset;
			for (let j$1 = 0, span; j$1 < decorations.length; j$1++) if (span = decorations[j$1]) {
				if (span.from > from && span.to < to) {
					decorations[j$1] = null;
					(found$1 || (found$1 = [])).push(span);
				}
			}
			if (!found$1) continue;
			if (children == this.children) children = this.children.slice();
			let removed = children[i$1 + 2].removeInner(found$1, from + 1);
			if (removed != empty) children[i$1 + 2] = removed;
			else {
				children.splice(i$1, 3);
				i$1 -= 3;
			}
		}
		if (local.length) {
			for (let i$1 = 0, span; i$1 < decorations.length; i$1++) if (span = decorations[i$1]) {
				for (let j$1 = 0; j$1 < local.length; j$1++) if (local[j$1].eq(span, offset)) {
					if (local == this.local) local = this.local.slice();
					local.splice(j$1--, 1);
				}
			}
		}
		if (children == this.children && local == this.local) return this;
		return local.length || children.length ? new DecorationSet(local, children) : empty;
	}
	forChild(offset, node) {
		if (this == empty) return this;
		if (node.isLeaf) return DecorationSet.empty;
		let child, local;
		for (let i$1 = 0; i$1 < this.children.length; i$1 += 3) if (this.children[i$1] >= offset) {
			if (this.children[i$1] == offset) child = this.children[i$1 + 2];
			break;
		}
		let start = offset + 1, end = start + node.content.size;
		for (let i$1 = 0; i$1 < this.local.length; i$1++) {
			let dec = this.local[i$1];
			if (dec.from < end && dec.to > start && dec.type instanceof InlineType) {
				let from = Math.max(start, dec.from) - start, to = Math.min(end, dec.to) - start;
				if (from < to) (local || (local = [])).push(dec.copy(from, to));
			}
		}
		if (local) {
			let localSet = new DecorationSet(local.sort(byPos), none);
			return child ? new DecorationGroup([localSet, child]) : localSet;
		}
		return child || empty;
	}
	eq(other) {
		if (this == other) return true;
		if (!(other instanceof DecorationSet) || this.local.length != other.local.length || this.children.length != other.children.length) return false;
		for (let i$1 = 0; i$1 < this.local.length; i$1++) if (!this.local[i$1].eq(other.local[i$1])) return false;
		for (let i$1 = 0; i$1 < this.children.length; i$1 += 3) if (this.children[i$1] != other.children[i$1] || this.children[i$1 + 1] != other.children[i$1 + 1] || !this.children[i$1 + 2].eq(other.children[i$1 + 2])) return false;
		return true;
	}
	locals(node) {
		return removeOverlap(this.localsInner(node));
	}
	localsInner(node) {
		if (this == empty) return none;
		if (node.inlineContent || !this.local.some(InlineType.is)) return this.local;
		let result = [];
		for (let i$1 = 0; i$1 < this.local.length; i$1++) if (!(this.local[i$1].type instanceof InlineType)) result.push(this.local[i$1]);
		return result;
	}
	forEachSet(f) {
		f(this);
	}
};
DecorationSet.empty = new DecorationSet([], []);
DecorationSet.removeOverlap = removeOverlap;
var empty = DecorationSet.empty;
var DecorationGroup = class DecorationGroup {
	constructor(members) {
		this.members = members;
	}
	map(mapping, doc$2) {
		const mappedDecos = this.members.map((member) => member.map(mapping, doc$2, noSpec));
		return DecorationGroup.from(mappedDecos);
	}
	forChild(offset, child) {
		if (child.isLeaf) return DecorationSet.empty;
		let found$1 = [];
		for (let i$1 = 0; i$1 < this.members.length; i$1++) {
			let result = this.members[i$1].forChild(offset, child);
			if (result == empty) continue;
			if (result instanceof DecorationGroup) found$1 = found$1.concat(result.members);
			else found$1.push(result);
		}
		return DecorationGroup.from(found$1);
	}
	eq(other) {
		if (!(other instanceof DecorationGroup) || other.members.length != this.members.length) return false;
		for (let i$1 = 0; i$1 < this.members.length; i$1++) if (!this.members[i$1].eq(other.members[i$1])) return false;
		return true;
	}
	locals(node) {
		let result, sorted = true;
		for (let i$1 = 0; i$1 < this.members.length; i$1++) {
			let locals = this.members[i$1].localsInner(node);
			if (!locals.length) continue;
			if (!result) result = locals;
			else {
				if (sorted) {
					result = result.slice();
					sorted = false;
				}
				for (let j$1 = 0; j$1 < locals.length; j$1++) result.push(locals[j$1]);
			}
		}
		return result ? removeOverlap(sorted ? result : result.sort(byPos)) : none;
	}
	static from(members) {
		switch (members.length) {
			case 0: return empty;
			case 1: return members[0];
			default: return new DecorationGroup(members.every((m$1) => m$1 instanceof DecorationSet) ? members : members.reduce((r, m$1) => r.concat(m$1 instanceof DecorationSet ? m$1 : m$1.members), []));
		}
	}
	forEachSet(f) {
		for (let i$1 = 0; i$1 < this.members.length; i$1++) this.members[i$1].forEachSet(f);
	}
};
function mapChildren(oldChildren, newLocal, mapping, node, offset, oldOffset, options) {
	let children = oldChildren.slice();
	for (let i$1 = 0, baseOffset = oldOffset; i$1 < mapping.maps.length; i$1++) {
		let moved = 0;
		mapping.maps[i$1].forEach((oldStart, oldEnd, newStart, newEnd) => {
			let dSize = newEnd - newStart - (oldEnd - oldStart);
			for (let i$2 = 0; i$2 < children.length; i$2 += 3) {
				let end = children[i$2 + 1];
				if (end < 0 || oldStart > end + baseOffset - moved) continue;
				let start = children[i$2] + baseOffset - moved;
				if (oldEnd >= start) children[i$2 + 1] = oldStart <= start ? -2 : -1;
				else if (oldStart >= baseOffset && dSize) {
					children[i$2] += dSize;
					children[i$2 + 1] += dSize;
				}
			}
			moved += dSize;
		});
		baseOffset = mapping.maps[i$1].map(baseOffset, -1);
	}
	let mustRebuild = false;
	for (let i$1 = 0; i$1 < children.length; i$1 += 3) if (children[i$1 + 1] < 0) {
		if (children[i$1 + 1] == -2) {
			mustRebuild = true;
			children[i$1 + 1] = -1;
			continue;
		}
		let from = mapping.map(oldChildren[i$1] + oldOffset), fromLocal = from - offset;
		if (fromLocal < 0 || fromLocal >= node.content.size) {
			mustRebuild = true;
			continue;
		}
		let toLocal = mapping.map(oldChildren[i$1 + 1] + oldOffset, -1) - offset;
		let { index, offset: childOffset } = node.content.findIndex(fromLocal);
		let childNode = node.maybeChild(index);
		if (childNode && childOffset == fromLocal && childOffset + childNode.nodeSize == toLocal) {
			let mapped = children[i$1 + 2].mapInner(mapping, childNode, from + 1, oldChildren[i$1] + oldOffset + 1, options);
			if (mapped != empty) {
				children[i$1] = fromLocal;
				children[i$1 + 1] = toLocal;
				children[i$1 + 2] = mapped;
			} else {
				children[i$1 + 1] = -2;
				mustRebuild = true;
			}
		} else mustRebuild = true;
	}
	if (mustRebuild) {
		let built = buildTree(mapAndGatherRemainingDecorations(children, oldChildren, newLocal, mapping, offset, oldOffset, options), node, 0, options);
		newLocal = built.local;
		for (let i$1 = 0; i$1 < children.length; i$1 += 3) if (children[i$1 + 1] < 0) {
			children.splice(i$1, 3);
			i$1 -= 3;
		}
		for (let i$1 = 0, j$1 = 0; i$1 < built.children.length; i$1 += 3) {
			let from = built.children[i$1];
			while (j$1 < children.length && children[j$1] < from) j$1 += 3;
			children.splice(j$1, 0, built.children[i$1], built.children[i$1 + 1], built.children[i$1 + 2]);
		}
	}
	return new DecorationSet(newLocal.sort(byPos), children);
}
function moveSpans(spans, offset) {
	if (!offset || !spans.length) return spans;
	let result = [];
	for (let i$1 = 0; i$1 < spans.length; i$1++) {
		let span = spans[i$1];
		result.push(new Decoration(span.from + offset, span.to + offset, span.type));
	}
	return result;
}
function mapAndGatherRemainingDecorations(children, oldChildren, decorations, mapping, offset, oldOffset, options) {
	function gather(set, oldOffset$1) {
		for (let i$1 = 0; i$1 < set.local.length; i$1++) {
			let mapped = set.local[i$1].map(mapping, offset, oldOffset$1);
			if (mapped) decorations.push(mapped);
			else if (options.onRemove) options.onRemove(set.local[i$1].spec);
		}
		for (let i$1 = 0; i$1 < set.children.length; i$1 += 3) gather(set.children[i$1 + 2], set.children[i$1] + oldOffset$1 + 1);
	}
	for (let i$1 = 0; i$1 < children.length; i$1 += 3) if (children[i$1 + 1] == -1) gather(children[i$1 + 2], oldChildren[i$1] + oldOffset + 1);
	return decorations;
}
function takeSpansForNode(spans, node, offset) {
	if (node.isLeaf) return null;
	let end = offset + node.nodeSize, found$1 = null;
	for (let i$1 = 0, span; i$1 < spans.length; i$1++) if ((span = spans[i$1]) && span.from > offset && span.to < end) {
		(found$1 || (found$1 = [])).push(span);
		spans[i$1] = null;
	}
	return found$1;
}
function withoutNulls(array) {
	let result = [];
	for (let i$1 = 0; i$1 < array.length; i$1++) if (array[i$1] != null) result.push(array[i$1]);
	return result;
}
function buildTree(spans, node, offset, options) {
	let children = [], hasNulls = false;
	node.forEach((childNode, localStart) => {
		let found$1 = takeSpansForNode(spans, childNode, localStart + offset);
		if (found$1) {
			hasNulls = true;
			let subtree = buildTree(found$1, childNode, offset + localStart + 1, options);
			if (subtree != empty) children.push(localStart, localStart + childNode.nodeSize, subtree);
		}
	});
	let locals = moveSpans(hasNulls ? withoutNulls(spans) : spans, -offset).sort(byPos);
	for (let i$1 = 0; i$1 < locals.length; i$1++) if (!locals[i$1].type.valid(node, locals[i$1])) {
		if (options.onRemove) options.onRemove(locals[i$1].spec);
		locals.splice(i$1--, 1);
	}
	return locals.length || children.length ? new DecorationSet(locals, children) : empty;
}
function byPos(a, b$1) {
	return a.from - b$1.from || a.to - b$1.to;
}
function removeOverlap(spans) {
	let working = spans;
	for (let i$1 = 0; i$1 < working.length - 1; i$1++) {
		let span = working[i$1];
		if (span.from != span.to) for (let j$1 = i$1 + 1; j$1 < working.length; j$1++) {
			let next = working[j$1];
			if (next.from == span.from) {
				if (next.to != span.to) {
					if (working == spans) working = spans.slice();
					working[j$1] = next.copy(next.from, span.to);
					insertAhead(working, j$1 + 1, next.copy(span.to, next.to));
				}
				continue;
			} else {
				if (next.from < span.to) {
					if (working == spans) working = spans.slice();
					working[i$1] = span.copy(span.from, next.from);
					insertAhead(working, j$1, span.copy(next.from, span.to));
				}
				break;
			}
		}
	}
	return working;
}
function insertAhead(array, i$1, deco) {
	while (i$1 < array.length && byPos(deco, array[i$1]) > 0) i$1++;
	array.splice(i$1, 0, deco);
}
function viewDecorations(view) {
	let found$1 = [];
	view.someProp("decorations", (f) => {
		let result = f(view.state);
		if (result && result != empty) found$1.push(result);
	});
	if (view.cursorWrapper) found$1.push(DecorationSet.create(view.state.doc, [view.cursorWrapper.deco]));
	return DecorationGroup.from(found$1);
}
var observeOptions = {
	childList: true,
	characterData: true,
	characterDataOldValue: true,
	attributes: true,
	attributeOldValue: true,
	subtree: true
};
var useCharData = ie$2 && ie_version <= 11;
var SelectionState = class {
	constructor() {
		this.anchorNode = null;
		this.anchorOffset = 0;
		this.focusNode = null;
		this.focusOffset = 0;
	}
	set(sel) {
		this.anchorNode = sel.anchorNode;
		this.anchorOffset = sel.anchorOffset;
		this.focusNode = sel.focusNode;
		this.focusOffset = sel.focusOffset;
	}
	clear() {
		this.anchorNode = this.focusNode = null;
	}
	eq(sel) {
		return sel.anchorNode == this.anchorNode && sel.anchorOffset == this.anchorOffset && sel.focusNode == this.focusNode && sel.focusOffset == this.focusOffset;
	}
};
var DOMObserver = class {
	constructor(view, handleDOMChange) {
		this.view = view;
		this.handleDOMChange = handleDOMChange;
		this.queue = [];
		this.flushingSoon = -1;
		this.observer = null;
		this.currentSelection = new SelectionState();
		this.onCharData = null;
		this.suppressingSelectionUpdates = false;
		this.lastChangedTextNode = null;
		this.observer = window.MutationObserver && new window.MutationObserver((mutations) => {
			for (let i$1 = 0; i$1 < mutations.length; i$1++) this.queue.push(mutations[i$1]);
			if (ie$2 && ie_version <= 11 && mutations.some((m$1) => m$1.type == "childList" && m$1.removedNodes.length || m$1.type == "characterData" && m$1.oldValue.length > m$1.target.nodeValue.length)) this.flushSoon();
			else if (safari && view.composing && mutations.some((m$1) => m$1.type == "childList" && m$1.target.nodeName == "TR")) {
				view.input.badSafariComposition = true;
				this.flushSoon();
			} else this.flush();
		});
		if (useCharData) this.onCharData = (e) => {
			this.queue.push({
				target: e.target,
				type: "characterData",
				oldValue: e.prevValue
			});
			this.flushSoon();
		};
		this.onSelectionChange = this.onSelectionChange.bind(this);
	}
	flushSoon() {
		if (this.flushingSoon < 0) this.flushingSoon = window.setTimeout(() => {
			this.flushingSoon = -1;
			this.flush();
		}, 20);
	}
	forceFlush() {
		if (this.flushingSoon > -1) {
			window.clearTimeout(this.flushingSoon);
			this.flushingSoon = -1;
			this.flush();
		}
	}
	start() {
		if (this.observer) {
			this.observer.takeRecords();
			this.observer.observe(this.view.dom, observeOptions);
		}
		if (this.onCharData) this.view.dom.addEventListener("DOMCharacterDataModified", this.onCharData);
		this.connectSelection();
	}
	stop() {
		if (this.observer) {
			let take = this.observer.takeRecords();
			if (take.length) {
				for (let i$1 = 0; i$1 < take.length; i$1++) this.queue.push(take[i$1]);
				window.setTimeout(() => this.flush(), 20);
			}
			this.observer.disconnect();
		}
		if (this.onCharData) this.view.dom.removeEventListener("DOMCharacterDataModified", this.onCharData);
		this.disconnectSelection();
	}
	connectSelection() {
		this.view.dom.ownerDocument.addEventListener("selectionchange", this.onSelectionChange);
	}
	disconnectSelection() {
		this.view.dom.ownerDocument.removeEventListener("selectionchange", this.onSelectionChange);
	}
	suppressSelectionUpdates() {
		this.suppressingSelectionUpdates = true;
		setTimeout(() => this.suppressingSelectionUpdates = false, 50);
	}
	onSelectionChange() {
		if (!hasFocusAndSelection(this.view)) return;
		if (this.suppressingSelectionUpdates) return selectionToDOM(this.view);
		if (ie$2 && ie_version <= 11 && !this.view.state.selection.empty) {
			let sel = this.view.domSelectionRange();
			if (sel.focusNode && isEquivalentPosition(sel.focusNode, sel.focusOffset, sel.anchorNode, sel.anchorOffset)) return this.flushSoon();
		}
		this.flush();
	}
	setCurSelection() {
		this.currentSelection.set(this.view.domSelectionRange());
	}
	ignoreSelectionChange(sel) {
		if (!sel.focusNode) return true;
		let ancestors = /* @__PURE__ */ new Set(), container;
		for (let scan = sel.focusNode; scan; scan = parentNode(scan)) ancestors.add(scan);
		for (let scan = sel.anchorNode; scan; scan = parentNode(scan)) if (ancestors.has(scan)) {
			container = scan;
			break;
		}
		let desc = container && this.view.docView.nearestDesc(container);
		if (desc && desc.ignoreMutation({
			type: "selection",
			target: container.nodeType == 3 ? container.parentNode : container
		})) {
			this.setCurSelection();
			return true;
		}
	}
	pendingRecords() {
		if (this.observer) for (let mut of this.observer.takeRecords()) this.queue.push(mut);
		return this.queue;
	}
	flush() {
		let { view } = this;
		if (!view.docView || this.flushingSoon > -1) return;
		let mutations = this.pendingRecords();
		if (mutations.length) this.queue = [];
		let sel = view.domSelectionRange();
		let newSel = !this.suppressingSelectionUpdates && !this.currentSelection.eq(sel) && hasFocusAndSelection(view) && !this.ignoreSelectionChange(sel);
		let from = -1, to = -1, typeOver = false, added = [];
		if (view.editable) for (let i$1 = 0; i$1 < mutations.length; i$1++) {
			let result = this.registerMutation(mutations[i$1], added);
			if (result) {
				from = from < 0 ? result.from : Math.min(result.from, from);
				to = to < 0 ? result.to : Math.max(result.to, to);
				if (result.typeOver) typeOver = true;
			}
		}
		if (added.some((n) => n.nodeName == "BR") && (view.input.lastKeyCode == 8 || view.input.lastKeyCode == 46)) {
			for (let node of added) if (node.nodeName == "BR" && node.parentNode) {
				let after = node.nextSibling;
				while (after && after.nodeType == 1) {
					if (after.contentEditable == "false") {
						node.parentNode.removeChild(node);
						break;
					}
					after = after.firstChild;
				}
			}
		} else if (gecko && added.length) {
			let brs = added.filter((n) => n.nodeName == "BR");
			if (brs.length == 2) {
				let [a, b$1] = brs;
				if (a.parentNode && a.parentNode.parentNode == b$1.parentNode) b$1.remove();
				else a.remove();
			} else {
				let { focusNode } = this.currentSelection;
				for (let br of brs) {
					let parent = br.parentNode;
					if (parent && parent.nodeName == "LI" && (!focusNode || blockParent(view, focusNode) != parent)) br.remove();
				}
			}
		}
		let readSel = null;
		if (from < 0 && newSel && view.input.lastFocus > Date.now() - 200 && Math.max(view.input.lastTouch, view.input.lastClick.time) < Date.now() - 300 && selectionCollapsed(sel) && (readSel = selectionFromDOM(view)) && readSel.eq(Selection.near(view.state.doc.resolve(0), 1))) {
			view.input.lastFocus = 0;
			selectionToDOM(view);
			this.currentSelection.set(sel);
			view.scrollToSelection();
		} else if (from > -1 || newSel) {
			if (from > -1) {
				view.docView.markDirty(from, to);
				checkCSS(view);
			}
			if (view.input.badSafariComposition) {
				view.input.badSafariComposition = false;
				fixUpBadSafariComposition(view, added);
			}
			this.handleDOMChange(from, to, typeOver, added);
			if (view.docView && view.docView.dirty) view.updateState(view.state);
			else if (!this.currentSelection.eq(sel)) selectionToDOM(view);
			this.currentSelection.set(sel);
		}
	}
	registerMutation(mut, added) {
		if (added.indexOf(mut.target) > -1) return null;
		let desc = this.view.docView.nearestDesc(mut.target);
		if (mut.type == "attributes" && (desc == this.view.docView || mut.attributeName == "contenteditable" || mut.attributeName == "style" && !mut.oldValue && !mut.target.getAttribute("style"))) return null;
		if (!desc || desc.ignoreMutation(mut)) return null;
		if (mut.type == "childList") {
			for (let i$1 = 0; i$1 < mut.addedNodes.length; i$1++) {
				let node = mut.addedNodes[i$1];
				added.push(node);
				if (node.nodeType == 3) this.lastChangedTextNode = node;
			}
			if (desc.contentDOM && desc.contentDOM != desc.dom && !desc.contentDOM.contains(mut.target)) return {
				from: desc.posBefore,
				to: desc.posAfter
			};
			let prev = mut.previousSibling, next = mut.nextSibling;
			if (ie$2 && ie_version <= 11 && mut.addedNodes.length) for (let i$1 = 0; i$1 < mut.addedNodes.length; i$1++) {
				let { previousSibling, nextSibling } = mut.addedNodes[i$1];
				if (!previousSibling || Array.prototype.indexOf.call(mut.addedNodes, previousSibling) < 0) prev = previousSibling;
				if (!nextSibling || Array.prototype.indexOf.call(mut.addedNodes, nextSibling) < 0) next = nextSibling;
			}
			let fromOffset = prev && prev.parentNode == mut.target ? domIndex(prev) + 1 : 0;
			let from = desc.localPosFromDOM(mut.target, fromOffset, -1);
			let toOffset = next && next.parentNode == mut.target ? domIndex(next) : mut.target.childNodes.length;
			return {
				from,
				to: desc.localPosFromDOM(mut.target, toOffset, 1)
			};
		} else if (mut.type == "attributes") return {
			from: desc.posAtStart - desc.border,
			to: desc.posAtEnd + desc.border
		};
		else {
			this.lastChangedTextNode = mut.target;
			return {
				from: desc.posAtStart,
				to: desc.posAtEnd,
				typeOver: mut.target.nodeValue == mut.oldValue
			};
		}
	}
};
var cssChecked = /* @__PURE__ */ new WeakMap();
var cssCheckWarned = false;
function checkCSS(view) {
	if (cssChecked.has(view)) return;
	cssChecked.set(view, null);
	if ([
		"normal",
		"nowrap",
		"pre-line"
	].indexOf(getComputedStyle(view.dom).whiteSpace) !== -1) {
		view.requiresGeckoHackNode = gecko;
		if (cssCheckWarned) return;
		console["warn"]("ProseMirror expects the CSS white-space property to be set, preferably to 'pre-wrap'. It is recommended to load style/prosemirror.css from the prosemirror-view package.");
		cssCheckWarned = true;
	}
}
function rangeToSelectionRange(view, range) {
	let anchorNode = range.startContainer, anchorOffset = range.startOffset;
	let focusNode = range.endContainer, focusOffset = range.endOffset;
	let currentAnchor = view.domAtPos(view.state.selection.anchor);
	if (isEquivalentPosition(currentAnchor.node, currentAnchor.offset, focusNode, focusOffset)) [anchorNode, anchorOffset, focusNode, focusOffset] = [
		focusNode,
		focusOffset,
		anchorNode,
		anchorOffset
	];
	return {
		anchorNode,
		anchorOffset,
		focusNode,
		focusOffset
	};
}
function safariShadowSelectionRange(view, selection) {
	if (selection.getComposedRanges) {
		let range = selection.getComposedRanges(view.root)[0];
		if (range) return rangeToSelectionRange(view, range);
	}
	let found$1;
	function read(event) {
		event.preventDefault();
		event.stopImmediatePropagation();
		found$1 = event.getTargetRanges()[0];
	}
	view.dom.addEventListener("beforeinput", read, true);
	document.execCommand("indent");
	view.dom.removeEventListener("beforeinput", read, true);
	return found$1 ? rangeToSelectionRange(view, found$1) : null;
}
function blockParent(view, node) {
	for (let p = node.parentNode; p && p != view.dom; p = p.parentNode) {
		let desc = view.docView.nearestDesc(p, true);
		if (desc && desc.node.isBlock) return p;
	}
	return null;
}
function fixUpBadSafariComposition(view, addedNodes) {
	var _a;
	let { focusNode, focusOffset } = view.domSelectionRange();
	for (let node of addedNodes) if (((_a = node.parentNode) === null || _a === void 0 ? void 0 : _a.nodeName) == "TR") {
		let nextCell$1 = node.nextSibling;
		while (nextCell$1 && nextCell$1.nodeName != "TD" && nextCell$1.nodeName != "TH") nextCell$1 = nextCell$1.nextSibling;
		if (nextCell$1) {
			let parent = nextCell$1;
			for (;;) {
				let first$1 = parent.firstChild;
				if (!first$1 || first$1.nodeType != 1 || first$1.contentEditable == "false" || /^(BR|IMG)$/.test(first$1.nodeName)) break;
				parent = first$1;
			}
			parent.insertBefore(node, parent.firstChild);
			if (focusNode == node) view.domSelection().collapse(node, focusOffset);
		} else node.parentNode.removeChild(node);
	}
}
function parseBetween(view, from_, to_) {
	let { node: parent, fromOffset, toOffset, from, to } = view.docView.parseRange(from_, to_);
	let domSel = view.domSelectionRange();
	let find$1;
	let anchor = domSel.anchorNode;
	if (anchor && view.dom.contains(anchor.nodeType == 1 ? anchor : anchor.parentNode)) {
		find$1 = [{
			node: anchor,
			offset: domSel.anchorOffset
		}];
		if (!selectionCollapsed(domSel)) find$1.push({
			node: domSel.focusNode,
			offset: domSel.focusOffset
		});
	}
	if (chrome && view.input.lastKeyCode === 8) for (let off = toOffset; off > fromOffset; off--) {
		let node = parent.childNodes[off - 1], desc = node.pmViewDesc;
		if (node.nodeName == "BR" && !desc) {
			toOffset = off;
			break;
		}
		if (!desc || desc.size) break;
	}
	let startDoc = view.state.doc;
	let parser = view.someProp("domParser") || DOMParser.fromSchema(view.state.schema);
	let $from = startDoc.resolve(from);
	let sel = null, doc$2 = parser.parse(parent, {
		topNode: $from.parent,
		topMatch: $from.parent.contentMatchAt($from.index()),
		topOpen: true,
		from: fromOffset,
		to: toOffset,
		preserveWhitespace: $from.parent.type.whitespace == "pre" ? "full" : true,
		findPositions: find$1,
		ruleFromNode,
		context: $from
	});
	if (find$1 && find$1[0].pos != null) {
		let anchor$1 = find$1[0].pos, head = find$1[1] && find$1[1].pos;
		if (head == null) head = anchor$1;
		sel = {
			anchor: anchor$1 + from,
			head: head + from
		};
	}
	return {
		doc: doc$2,
		sel,
		from,
		to
	};
}
function ruleFromNode(dom) {
	let desc = dom.pmViewDesc;
	if (desc) return desc.parseRule();
	else if (dom.nodeName == "BR" && dom.parentNode) {
		if (safari && /^(ul|ol)$/i.test(dom.parentNode.nodeName)) {
			let skip = document.createElement("div");
			skip.appendChild(document.createElement("li"));
			return { skip };
		} else if (dom.parentNode.lastChild == dom || safari && /^(tr|table)$/i.test(dom.parentNode.nodeName)) return { ignore: true };
	} else if (dom.nodeName == "IMG" && dom.getAttribute("mark-placeholder")) return { ignore: true };
	return null;
}
var isInline = /^(a|abbr|acronym|b|bd[io]|big|br|button|cite|code|data(list)?|del|dfn|em|i|img|ins|kbd|label|map|mark|meter|output|q|ruby|s|samp|small|span|strong|su[bp]|time|u|tt|var)$/i;
function readDOMChange(view, from, to, typeOver, addedNodes) {
	let compositionID = view.input.compositionPendingChanges || (view.composing ? view.input.compositionID : 0);
	view.input.compositionPendingChanges = 0;
	if (from < 0) {
		let origin = view.input.lastSelectionTime > Date.now() - 50 ? view.input.lastSelectionOrigin : null;
		let newSel = selectionFromDOM(view, origin);
		if (newSel && !view.state.selection.eq(newSel)) {
			if (chrome && android && view.input.lastKeyCode === 13 && Date.now() - 100 < view.input.lastKeyCodeTime && view.someProp("handleKeyDown", (f) => f(view, keyEvent(13, "Enter")))) return;
			let tr$1 = view.state.tr.setSelection(newSel);
			if (origin == "pointer") tr$1.setMeta("pointer", true);
			else if (origin == "key") tr$1.scrollIntoView();
			if (compositionID) tr$1.setMeta("composition", compositionID);
			view.dispatch(tr$1);
		}
		return;
	}
	let $before = view.state.doc.resolve(from);
	let shared = $before.sharedDepth(to);
	from = $before.before(shared + 1);
	to = view.state.doc.resolve(to).after(shared + 1);
	let sel = view.state.selection;
	let parse = parseBetween(view, from, to);
	let doc$2 = view.state.doc, compare = doc$2.slice(parse.from, parse.to);
	let preferredPos, preferredSide;
	if (view.input.lastKeyCode === 8 && Date.now() - 100 < view.input.lastKeyCodeTime) {
		preferredPos = view.state.selection.to;
		preferredSide = "end";
	} else {
		preferredPos = view.state.selection.from;
		preferredSide = "start";
	}
	view.input.lastKeyCode = null;
	let change = findDiff(compare.content, parse.doc.content, parse.from, preferredPos, preferredSide);
	if (change) view.input.domChangeCount++;
	if ((ios && view.input.lastIOSEnter > Date.now() - 225 || android) && addedNodes.some((n) => n.nodeType == 1 && !isInline.test(n.nodeName)) && (!change || change.endA >= change.endB) && view.someProp("handleKeyDown", (f) => f(view, keyEvent(13, "Enter")))) {
		view.input.lastIOSEnter = 0;
		return;
	}
	if (!change) if (typeOver && sel instanceof TextSelection && !sel.empty && sel.$head.sameParent(sel.$anchor) && !view.composing && !(parse.sel && parse.sel.anchor != parse.sel.head)) change = {
		start: sel.from,
		endA: sel.to,
		endB: sel.to
	};
	else {
		if (parse.sel) {
			let sel$1 = resolveSelection(view, view.state.doc, parse.sel);
			if (sel$1 && !sel$1.eq(view.state.selection)) {
				let tr$1 = view.state.tr.setSelection(sel$1);
				if (compositionID) tr$1.setMeta("composition", compositionID);
				view.dispatch(tr$1);
			}
		}
		return;
	}
	if (view.state.selection.from < view.state.selection.to && change.start == change.endB && view.state.selection instanceof TextSelection) {
		if (change.start > view.state.selection.from && change.start <= view.state.selection.from + 2 && view.state.selection.from >= parse.from) change.start = view.state.selection.from;
		else if (change.endA < view.state.selection.to && change.endA >= view.state.selection.to - 2 && view.state.selection.to <= parse.to) {
			change.endB += view.state.selection.to - change.endA;
			change.endA = view.state.selection.to;
		}
	}
	if (ie$2 && ie_version <= 11 && change.endB == change.start + 1 && change.endA == change.start && change.start > parse.from && parse.doc.textBetween(change.start - parse.from - 1, change.start - parse.from + 1) == " \xA0") {
		change.start--;
		change.endA--;
		change.endB--;
	}
	let $from = parse.doc.resolveNoCache(change.start - parse.from);
	let $to = parse.doc.resolveNoCache(change.endB - parse.from);
	let $fromA = doc$2.resolve(change.start);
	let inlineChange = $from.sameParent($to) && $from.parent.inlineContent && $fromA.end() >= change.endA;
	if ((ios && view.input.lastIOSEnter > Date.now() - 225 && (!inlineChange || addedNodes.some((n) => n.nodeName == "DIV" || n.nodeName == "P")) || !inlineChange && $from.pos < parse.doc.content.size && (!$from.sameParent($to) || !$from.parent.inlineContent) && $from.pos < $to.pos && !/\S/.test(parse.doc.textBetween($from.pos, $to.pos, "", ""))) && view.someProp("handleKeyDown", (f) => f(view, keyEvent(13, "Enter")))) {
		view.input.lastIOSEnter = 0;
		return;
	}
	if (view.state.selection.anchor > change.start && looksLikeBackspace(doc$2, change.start, change.endA, $from, $to) && view.someProp("handleKeyDown", (f) => f(view, keyEvent(8, "Backspace")))) {
		if (android && chrome) view.domObserver.suppressSelectionUpdates();
		return;
	}
	if (chrome && change.endB == change.start) view.input.lastChromeDelete = Date.now();
	if (android && !inlineChange && $from.start() != $to.start() && $to.parentOffset == 0 && $from.depth == $to.depth && parse.sel && parse.sel.anchor == parse.sel.head && parse.sel.head == change.endA) {
		change.endB -= 2;
		$to = parse.doc.resolveNoCache(change.endB - parse.from);
		setTimeout(() => {
			view.someProp("handleKeyDown", function(f) {
				return f(view, keyEvent(13, "Enter"));
			});
		}, 20);
	}
	let chFrom = change.start, chTo = change.endA;
	let mkTr = (base$1) => {
		let tr$1 = base$1 || view.state.tr.replace(chFrom, chTo, parse.doc.slice(change.start - parse.from, change.endB - parse.from));
		if (parse.sel) {
			let sel$1 = resolveSelection(view, tr$1.doc, parse.sel);
			if (sel$1 && !(chrome && view.composing && sel$1.empty && (change.start != change.endB || view.input.lastChromeDelete < Date.now() - 100) && (sel$1.head == chFrom || sel$1.head == tr$1.mapping.map(chTo) - 1) || ie$2 && sel$1.empty && sel$1.head == chFrom)) tr$1.setSelection(sel$1);
		}
		if (compositionID) tr$1.setMeta("composition", compositionID);
		return tr$1.scrollIntoView();
	};
	let markChange;
	if (inlineChange) if ($from.pos == $to.pos) {
		if (ie$2 && ie_version <= 11 && $from.parentOffset == 0) {
			view.domObserver.suppressSelectionUpdates();
			setTimeout(() => selectionToDOM(view), 20);
		}
		let tr$1 = mkTr(view.state.tr.delete(chFrom, chTo));
		let marks = doc$2.resolve(change.start).marksAcross(doc$2.resolve(change.endA));
		if (marks) tr$1.ensureMarks(marks);
		view.dispatch(tr$1);
	} else if (change.endA == change.endB && (markChange = isMarkChange($from.parent.content.cut($from.parentOffset, $to.parentOffset), $fromA.parent.content.cut($fromA.parentOffset, change.endA - $fromA.start())))) {
		let tr$1 = mkTr(view.state.tr);
		if (markChange.type == "add") tr$1.addMark(chFrom, chTo, markChange.mark);
		else tr$1.removeMark(chFrom, chTo, markChange.mark);
		view.dispatch(tr$1);
	} else if ($from.parent.child($from.index()).isText && $from.index() == $to.index() - ($to.textOffset ? 0 : 1)) {
		let text = $from.parent.textBetween($from.parentOffset, $to.parentOffset);
		let deflt = () => mkTr(view.state.tr.insertText(text, chFrom, chTo));
		if (!view.someProp("handleTextInput", (f) => f(view, chFrom, chTo, text, deflt))) view.dispatch(deflt());
	} else view.dispatch(mkTr());
	else view.dispatch(mkTr());
}
function resolveSelection(view, doc$2, parsedSel) {
	if (Math.max(parsedSel.anchor, parsedSel.head) > doc$2.content.size) return null;
	return selectionBetween(view, doc$2.resolve(parsedSel.anchor), doc$2.resolve(parsedSel.head));
}
function isMarkChange(cur, prev) {
	let curMarks = cur.firstChild.marks, prevMarks = prev.firstChild.marks;
	let added = curMarks, removed = prevMarks, type, mark, update;
	for (let i$1 = 0; i$1 < prevMarks.length; i$1++) added = prevMarks[i$1].removeFromSet(added);
	for (let i$1 = 0; i$1 < curMarks.length; i$1++) removed = curMarks[i$1].removeFromSet(removed);
	if (added.length == 1 && removed.length == 0) {
		mark = added[0];
		type = "add";
		update = (node) => node.mark(mark.addToSet(node.marks));
	} else if (added.length == 0 && removed.length == 1) {
		mark = removed[0];
		type = "remove";
		update = (node) => node.mark(mark.removeFromSet(node.marks));
	} else return null;
	let updated = [];
	for (let i$1 = 0; i$1 < prev.childCount; i$1++) updated.push(update(prev.child(i$1)));
	if (Fragment.from(updated).eq(cur)) return {
		mark,
		type
	};
}
function looksLikeBackspace(old, start, end, $newStart, $newEnd) {
	if (end - start <= $newEnd.pos - $newStart.pos || skipClosingAndOpening($newStart, true, false) < $newEnd.pos) return false;
	let $start = old.resolve(start);
	if (!$newStart.parent.isTextblock) {
		let after = $start.nodeAfter;
		return after != null && end == start + after.nodeSize;
	}
	if ($start.parentOffset < $start.parent.content.size || !$start.parent.isTextblock) return false;
	let $next = old.resolve(skipClosingAndOpening($start, true, true));
	if (!$next.parent.isTextblock || $next.pos > end || skipClosingAndOpening($next, true, false) < end) return false;
	return $newStart.parent.content.cut($newStart.parentOffset).eq($next.parent.content);
}
function skipClosingAndOpening($pos, fromEnd, mayOpen) {
	let depth = $pos.depth, end = fromEnd ? $pos.end() : $pos.pos;
	while (depth > 0 && (fromEnd || $pos.indexAfter(depth) == $pos.node(depth).childCount)) {
		depth--;
		end++;
		fromEnd = false;
	}
	if (mayOpen) {
		let next = $pos.node(depth).maybeChild($pos.indexAfter(depth));
		while (next && !next.isLeaf) {
			next = next.firstChild;
			end++;
		}
	}
	return end;
}
function findDiff(a, b$1, pos, preferredPos, preferredSide) {
	let start = a.findDiffStart(b$1, pos);
	if (start == null) return null;
	let { a: endA, b: endB } = a.findDiffEnd(b$1, pos + a.size, pos + b$1.size);
	if (preferredSide == "end") {
		let adjust = Math.max(0, start - Math.min(endA, endB));
		preferredPos -= endA + adjust - start;
	}
	if (endA < start && a.size < b$1.size) {
		let move = preferredPos <= start && preferredPos >= endA ? start - preferredPos : 0;
		start -= move;
		if (start && start < b$1.size && isSurrogatePair(b$1.textBetween(start - 1, start + 1))) start += move ? 1 : -1;
		endB = start + (endB - endA);
		endA = start;
	} else if (endB < start) {
		let move = preferredPos <= start && preferredPos >= endB ? start - preferredPos : 0;
		start -= move;
		if (start && start < a.size && isSurrogatePair(a.textBetween(start - 1, start + 1))) start += move ? 1 : -1;
		endA = start + (endA - endB);
		endB = start;
	}
	return {
		start,
		endA,
		endB
	};
}
function isSurrogatePair(str) {
	if (str.length != 2) return false;
	let a = str.charCodeAt(0), b$1 = str.charCodeAt(1);
	return a >= 56320 && a <= 57343 && b$1 >= 55296 && b$1 <= 56319;
}
var EditorView = class {
	constructor(place, props) {
		this._root = null;
		this.focused = false;
		this.trackWrites = null;
		this.mounted = false;
		this.markCursor = null;
		this.cursorWrapper = null;
		this.lastSelectedViewDesc = void 0;
		this.input = new InputState();
		this.prevDirectPlugins = [];
		this.pluginViews = [];
		this.requiresGeckoHackNode = false;
		this.dragging = null;
		this._props = props;
		this.state = props.state;
		this.directPlugins = props.plugins || [];
		this.directPlugins.forEach(checkStateComponent);
		this.dispatch = this.dispatch.bind(this);
		this.dom = place && place.mount || document.createElement("div");
		if (place) {
			if (place.appendChild) place.appendChild(this.dom);
			else if (typeof place == "function") place(this.dom);
			else if (place.mount) this.mounted = true;
		}
		this.editable = getEditable(this);
		updateCursorWrapper(this);
		this.nodeViews = buildNodeViews(this);
		this.docView = docViewDesc(this.state.doc, computeDocDeco(this), viewDecorations(this), this.dom, this);
		this.domObserver = new DOMObserver(this, (from, to, typeOver, added) => readDOMChange(this, from, to, typeOver, added));
		this.domObserver.start();
		initInput(this);
		this.updatePluginViews();
	}
	get composing() {
		return this.input.composing;
	}
	get props() {
		if (this._props.state != this.state) {
			let prev = this._props;
			this._props = {};
			for (let name in prev) this._props[name] = prev[name];
			this._props.state = this.state;
		}
		return this._props;
	}
	update(props) {
		if (props.handleDOMEvents != this._props.handleDOMEvents) ensureListeners(this);
		let prevProps = this._props;
		this._props = props;
		if (props.plugins) {
			props.plugins.forEach(checkStateComponent);
			this.directPlugins = props.plugins;
		}
		this.updateStateInner(props.state, prevProps);
	}
	setProps(props) {
		let updated = {};
		for (let name in this._props) updated[name] = this._props[name];
		updated.state = this.state;
		for (let name in props) updated[name] = props[name];
		this.update(updated);
	}
	updateState(state) {
		this.updateStateInner(state, this._props);
	}
	updateStateInner(state, prevProps) {
		var _a;
		let prev = this.state, redraw = false, updateSel = false;
		if (state.storedMarks && this.composing) {
			clearComposition(this);
			updateSel = true;
		}
		this.state = state;
		let pluginsChanged = prev.plugins != state.plugins || this._props.plugins != prevProps.plugins;
		if (pluginsChanged || this._props.plugins != prevProps.plugins || this._props.nodeViews != prevProps.nodeViews) {
			let nodeViews = buildNodeViews(this);
			if (changedNodeViews(nodeViews, this.nodeViews)) {
				this.nodeViews = nodeViews;
				redraw = true;
			}
		}
		if (pluginsChanged || prevProps.handleDOMEvents != this._props.handleDOMEvents) ensureListeners(this);
		this.editable = getEditable(this);
		updateCursorWrapper(this);
		let innerDeco = viewDecorations(this), outerDeco = computeDocDeco(this);
		let scroll = prev.plugins != state.plugins && !prev.doc.eq(state.doc) ? "reset" : state.scrollToSelection > prev.scrollToSelection ? "to selection" : "preserve";
		let updateDoc = redraw || !this.docView.matchesNode(state.doc, outerDeco, innerDeco);
		if (updateDoc || !state.selection.eq(prev.selection)) updateSel = true;
		let oldScrollPos = scroll == "preserve" && updateSel && this.dom.style.overflowAnchor == null && storeScrollPos(this);
		if (updateSel) {
			this.domObserver.stop();
			let forceSelUpdate = updateDoc && (ie$2 || chrome) && !this.composing && !prev.selection.empty && !state.selection.empty && selectionContextChanged(prev.selection, state.selection);
			if (updateDoc) {
				let chromeKludge = chrome ? this.trackWrites = this.domSelectionRange().focusNode : null;
				if (this.composing) this.input.compositionNode = findCompositionNode(this);
				if (redraw || !this.docView.update(state.doc, outerDeco, innerDeco, this)) {
					this.docView.updateOuterDeco(outerDeco);
					this.docView.destroy();
					this.docView = docViewDesc(state.doc, outerDeco, innerDeco, this.dom, this);
				}
				if (chromeKludge && (!this.trackWrites || !this.dom.contains(this.trackWrites))) forceSelUpdate = true;
			}
			if (forceSelUpdate || !(this.input.mouseDown && this.domObserver.currentSelection.eq(this.domSelectionRange()) && anchorInRightPlace(this))) selectionToDOM(this, forceSelUpdate);
			else {
				syncNodeSelection(this, state.selection);
				this.domObserver.setCurSelection();
			}
			this.domObserver.start();
		}
		this.updatePluginViews(prev);
		if (((_a = this.dragging) === null || _a === void 0 ? void 0 : _a.node) && !prev.doc.eq(state.doc)) this.updateDraggedNode(this.dragging, prev);
		if (scroll == "reset") this.dom.scrollTop = 0;
		else if (scroll == "to selection") this.scrollToSelection();
		else if (oldScrollPos) resetScrollPos(oldScrollPos);
	}
	scrollToSelection() {
		let startDOM = this.domSelectionRange().focusNode;
		if (!startDOM || !this.dom.contains(startDOM.nodeType == 1 ? startDOM : startDOM.parentNode));
		else if (this.someProp("handleScrollToSelection", (f) => f(this)));
		else if (this.state.selection instanceof NodeSelection) {
			let target = this.docView.domAfterPos(this.state.selection.from);
			if (target.nodeType == 1) scrollRectIntoView(this, target.getBoundingClientRect(), startDOM);
		} else scrollRectIntoView(this, this.coordsAtPos(this.state.selection.head, 1), startDOM);
	}
	destroyPluginViews() {
		let view;
		while (view = this.pluginViews.pop()) if (view.destroy) view.destroy();
	}
	updatePluginViews(prevState) {
		if (!prevState || prevState.plugins != this.state.plugins || this.directPlugins != this.prevDirectPlugins) {
			this.prevDirectPlugins = this.directPlugins;
			this.destroyPluginViews();
			for (let i$1 = 0; i$1 < this.directPlugins.length; i$1++) {
				let plugin = this.directPlugins[i$1];
				if (plugin.spec.view) this.pluginViews.push(plugin.spec.view(this));
			}
			for (let i$1 = 0; i$1 < this.state.plugins.length; i$1++) {
				let plugin = this.state.plugins[i$1];
				if (plugin.spec.view) this.pluginViews.push(plugin.spec.view(this));
			}
		} else for (let i$1 = 0; i$1 < this.pluginViews.length; i$1++) {
			let pluginView = this.pluginViews[i$1];
			if (pluginView.update) pluginView.update(this, prevState);
		}
	}
	updateDraggedNode(dragging, prev) {
		let sel = dragging.node, found$1 = -1;
		if (sel.from < this.state.doc.content.size && this.state.doc.nodeAt(sel.from) == sel.node) found$1 = sel.from;
		else {
			let movedPos = sel.from + (this.state.doc.content.size - prev.doc.content.size);
			if ((movedPos > 0 && movedPos < this.state.doc.content.size && this.state.doc.nodeAt(movedPos)) == sel.node) found$1 = movedPos;
		}
		this.dragging = new Dragging(dragging.slice, dragging.move, found$1 < 0 ? void 0 : NodeSelection.create(this.state.doc, found$1));
	}
	someProp(propName, f) {
		let prop = this._props && this._props[propName], value;
		if (prop != null && (value = f ? f(prop) : prop)) return value;
		for (let i$1 = 0; i$1 < this.directPlugins.length; i$1++) {
			let prop$1 = this.directPlugins[i$1].props[propName];
			if (prop$1 != null && (value = f ? f(prop$1) : prop$1)) return value;
		}
		let plugins = this.state.plugins;
		if (plugins) for (let i$1 = 0; i$1 < plugins.length; i$1++) {
			let prop$1 = plugins[i$1].props[propName];
			if (prop$1 != null && (value = f ? f(prop$1) : prop$1)) return value;
		}
	}
	hasFocus() {
		if (ie$2) {
			let node = this.root.activeElement;
			if (node == this.dom) return true;
			if (!node || !this.dom.contains(node)) return false;
			while (node && this.dom != node && this.dom.contains(node)) {
				if (node.contentEditable == "false") return false;
				node = node.parentElement;
			}
			return true;
		}
		return this.root.activeElement == this.dom;
	}
	focus() {
		this.domObserver.stop();
		if (this.editable) focusPreventScroll(this.dom);
		selectionToDOM(this);
		this.domObserver.start();
	}
	get root() {
		let cached = this._root;
		if (cached == null) {
			for (let search = this.dom.parentNode; search; search = search.parentNode) if (search.nodeType == 9 || search.nodeType == 11 && search.host) {
				if (!search.getSelection) Object.getPrototypeOf(search).getSelection = () => search.ownerDocument.getSelection();
				return this._root = search;
			}
		}
		return cached || document;
	}
	updateRoot() {
		this._root = null;
	}
	posAtCoords(coords) {
		return posAtCoords(this, coords);
	}
	coordsAtPos(pos, side = 1) {
		return coordsAtPos(this, pos, side);
	}
	domAtPos(pos, side = 0) {
		return this.docView.domFromPos(pos, side);
	}
	nodeDOM(pos) {
		let desc = this.docView.descAt(pos);
		return desc ? desc.nodeDOM : null;
	}
	posAtDOM(node, offset, bias = -1) {
		let pos = this.docView.posFromDOM(node, offset, bias);
		if (pos == null) throw new RangeError("DOM position not inside the editor");
		return pos;
	}
	endOfTextblock(dir, state) {
		return endOfTextblock(this, state || this.state, dir);
	}
	pasteHTML(html, event) {
		return doPaste(this, "", html, false, event || new ClipboardEvent("paste"));
	}
	pasteText(text, event) {
		return doPaste(this, text, null, true, event || new ClipboardEvent("paste"));
	}
	serializeForClipboard(slice) {
		return serializeForClipboard(this, slice);
	}
	destroy() {
		if (!this.docView) return;
		destroyInput(this);
		this.destroyPluginViews();
		if (this.mounted) {
			this.docView.update(this.state.doc, [], viewDecorations(this), this);
			this.dom.textContent = "";
		} else if (this.dom.parentNode) this.dom.parentNode.removeChild(this.dom);
		this.docView.destroy();
		this.docView = null;
		clearReusedRange();
	}
	get isDestroyed() {
		return this.docView == null;
	}
	dispatchEvent(event) {
		return dispatchEvent(this, event);
	}
	domSelectionRange() {
		let sel = this.domSelection();
		if (!sel) return {
			focusNode: null,
			focusOffset: 0,
			anchorNode: null,
			anchorOffset: 0
		};
		return safari && this.root.nodeType === 11 && deepActiveElement(this.dom.ownerDocument) == this.dom && safariShadowSelectionRange(this, sel) || sel;
	}
	domSelection() {
		return this.root.getSelection();
	}
};
EditorView.prototype.dispatch = function(tr$1) {
	let dispatchTransaction = this._props.dispatchTransaction;
	if (dispatchTransaction) dispatchTransaction.call(this, tr$1);
	else this.updateState(this.state.apply(tr$1));
};
function computeDocDeco(view) {
	let attrs = Object.create(null);
	attrs.class = "ProseMirror";
	attrs.contenteditable = String(view.editable);
	view.someProp("attributes", (value) => {
		if (typeof value == "function") value = value(view.state);
		if (value) {
			for (let attr in value) if (attr == "class") attrs.class += " " + value[attr];
			else if (attr == "style") attrs.style = (attrs.style ? attrs.style + ";" : "") + value[attr];
			else if (!attrs[attr] && attr != "contenteditable" && attr != "nodeName") attrs[attr] = String(value[attr]);
		}
	});
	if (!attrs.translate) attrs.translate = "no";
	return [Decoration.node(0, view.state.doc.content.size, attrs)];
}
function updateCursorWrapper(view) {
	if (view.markCursor) {
		let dom = document.createElement("img");
		dom.className = "ProseMirror-separator";
		dom.setAttribute("mark-placeholder", "true");
		dom.setAttribute("alt", "");
		view.cursorWrapper = {
			dom,
			deco: Decoration.widget(view.state.selection.from, dom, {
				raw: true,
				marks: view.markCursor
			})
		};
	} else view.cursorWrapper = null;
}
function getEditable(view) {
	return !view.someProp("editable", (value) => value(view.state) === false);
}
function selectionContextChanged(sel1, sel2) {
	let depth = Math.min(sel1.$anchor.sharedDepth(sel1.head), sel2.$anchor.sharedDepth(sel2.head));
	return sel1.$anchor.start(depth) != sel2.$anchor.start(depth);
}
function buildNodeViews(view) {
	let result = Object.create(null);
	function add(obj) {
		for (let prop in obj) if (!Object.prototype.hasOwnProperty.call(result, prop)) result[prop] = obj[prop];
	}
	view.someProp("nodeViews", add);
	view.someProp("markViews", add);
	return result;
}
function changedNodeViews(a, b$1) {
	let nA = 0, nB = 0;
	for (let prop in a) {
		if (a[prop] != b$1[prop]) return true;
		nA++;
	}
	for (let _$1 in b$1) nB++;
	return nA != nB;
}
function checkStateComponent(plugin) {
	if (plugin.spec.state || plugin.spec.filterTransaction || plugin.spec.appendTransaction) throw new RangeError("Plugins passed directly to the view must not have a state component");
}
var base = {
	8: "Backspace",
	9: "Tab",
	10: "Enter",
	12: "NumLock",
	13: "Enter",
	16: "Shift",
	17: "Control",
	18: "Alt",
	20: "CapsLock",
	27: "Escape",
	32: " ",
	33: "PageUp",
	34: "PageDown",
	35: "End",
	36: "Home",
	37: "ArrowLeft",
	38: "ArrowUp",
	39: "ArrowRight",
	40: "ArrowDown",
	44: "PrintScreen",
	45: "Insert",
	46: "Delete",
	59: ";",
	61: "=",
	91: "Meta",
	92: "Meta",
	106: "*",
	107: "+",
	108: ",",
	109: "-",
	110: ".",
	111: "/",
	144: "NumLock",
	145: "ScrollLock",
	160: "Shift",
	161: "Shift",
	162: "Control",
	163: "Control",
	164: "Alt",
	165: "Alt",
	173: "-",
	186: ";",
	187: "=",
	188: ",",
	189: "-",
	190: ".",
	191: "/",
	192: "`",
	219: "[",
	220: "\\",
	221: "]",
	222: "'"
};
var shift = {
	48: ")",
	49: "!",
	50: "@",
	51: "#",
	52: "$",
	53: "%",
	54: "^",
	55: "&",
	56: "*",
	57: "(",
	59: ":",
	61: "+",
	173: "_",
	186: ":",
	187: "+",
	188: "<",
	189: "_",
	190: ">",
	191: "?",
	192: "~",
	219: "{",
	220: "|",
	221: "}",
	222: "\""
};
var mac$1 = typeof navigator != "undefined" && /Mac/.test(navigator.platform);
var ie$1 = typeof navigator != "undefined" && /MSIE \d|Trident\/(?:[7-9]|\d{2,})\..*rv:(\d+)/.exec(navigator.userAgent);
for (var i = 0; i < 10; i++) base[48 + i] = base[96 + i] = String(i);
for (var i = 1; i <= 24; i++) base[i + 111] = "F" + i;
for (var i = 65; i <= 90; i++) {
	base[i] = String.fromCharCode(i + 32);
	shift[i] = String.fromCharCode(i);
}
for (var code in base) if (!shift.hasOwnProperty(code)) shift[code] = base[code];
function keyName(event) {
	var name = !(mac$1 && event.metaKey && event.shiftKey && !event.ctrlKey && !event.altKey || ie$1 && event.shiftKey && event.key && event.key.length == 1 || event.key == "Unidentified") && event.key || (event.shiftKey ? shift : base)[event.keyCode] || event.key || "Unidentified";
	if (name == "Esc") name = "Escape";
	if (name == "Del") name = "Delete";
	if (name == "Left") name = "ArrowLeft";
	if (name == "Up") name = "ArrowUp";
	if (name == "Right") name = "ArrowRight";
	if (name == "Down") name = "ArrowDown";
	return name;
}
var mac = typeof navigator != "undefined" && /Mac|iP(hone|[oa]d)/.test(navigator.platform);
var windows = typeof navigator != "undefined" && /Win/.test(navigator.platform);
function normalizeKeyName$1(name) {
	let parts = name.split(/-(?!$)/), result = parts[parts.length - 1];
	if (result == "Space") result = " ";
	let alt, ctrl, shift$1, meta;
	for (let i$1 = 0; i$1 < parts.length - 1; i$1++) {
		let mod = parts[i$1];
		if (/^(cmd|meta|m)$/i.test(mod)) meta = true;
		else if (/^a(lt)?$/i.test(mod)) alt = true;
		else if (/^(c|ctrl|control)$/i.test(mod)) ctrl = true;
		else if (/^s(hift)?$/i.test(mod)) shift$1 = true;
		else if (/^mod$/i.test(mod)) if (mac) meta = true;
		else ctrl = true;
		else throw new Error("Unrecognized modifier name: " + mod);
	}
	if (alt) result = "Alt-" + result;
	if (ctrl) result = "Ctrl-" + result;
	if (meta) result = "Meta-" + result;
	if (shift$1) result = "Shift-" + result;
	return result;
}
function normalize(map) {
	let copy$1 = Object.create(null);
	for (let prop in map) copy$1[normalizeKeyName$1(prop)] = map[prop];
	return copy$1;
}
function modifiers(name, event, shift$1 = true) {
	if (event.altKey) name = "Alt-" + name;
	if (event.ctrlKey) name = "Ctrl-" + name;
	if (event.metaKey) name = "Meta-" + name;
	if (shift$1 && event.shiftKey) name = "Shift-" + name;
	return name;
}
function keymap(bindings) {
	return new Plugin({ props: { handleKeyDown: keydownHandler(bindings) } });
}
function keydownHandler(bindings) {
	let map = normalize(bindings);
	return function(view, event) {
		let name = keyName(event), baseName, direct = map[modifiers(name, event)];
		if (direct && direct(view.state, view.dispatch, view)) return true;
		if (name.length == 1 && name != " ") {
			if (event.shiftKey) {
				let noShift = map[modifiers(name, event, false)];
				if (noShift && noShift(view.state, view.dispatch, view)) return true;
			}
			if ((event.altKey || event.metaKey || event.ctrlKey) && !(windows && event.ctrlKey && event.altKey) && (baseName = base[event.keyCode]) && baseName != name) {
				let fromCode = map[modifiers(baseName, event)];
				if (fromCode && fromCode(view.state, view.dispatch, view)) return true;
			}
		}
		return false;
	};
}
var __defProp$1 = Object.defineProperty;
var __export$1 = (target, all) => {
	for (var name in all) __defProp$1(target, name, {
		get: all[name],
		enumerable: true
	});
};
function createChainableState(config) {
	const { state, transaction } = config;
	let { selection } = transaction;
	let { doc: doc$2 } = transaction;
	let { storedMarks } = transaction;
	return {
		...state,
		apply: state.apply.bind(state),
		applyTransaction: state.applyTransaction.bind(state),
		plugins: state.plugins,
		schema: state.schema,
		reconfigure: state.reconfigure.bind(state),
		toJSON: state.toJSON.bind(state),
		get storedMarks() {
			return storedMarks;
		},
		get selection() {
			return selection;
		},
		get doc() {
			return doc$2;
		},
		get tr() {
			selection = transaction.selection;
			doc$2 = transaction.doc;
			storedMarks = transaction.storedMarks;
			return transaction;
		}
	};
}
var CommandManager = class {
	constructor(props) {
		this.editor = props.editor;
		this.rawCommands = this.editor.extensionManager.commands;
		this.customState = props.state;
	}
	get hasCustomState() {
		return !!this.customState;
	}
	get state() {
		return this.customState || this.editor.state;
	}
	get commands() {
		const { rawCommands, editor, state } = this;
		const { view } = editor;
		const { tr: tr$1 } = state;
		const props = this.buildProps(tr$1);
		return Object.fromEntries(Object.entries(rawCommands).map(([name, command2]) => {
			const method = (...args) => {
				const callback = command2(...args)(props);
				if (!tr$1.getMeta("preventDispatch") && !this.hasCustomState) view.dispatch(tr$1);
				return callback;
			};
			return [name, method];
		}));
	}
	get chain() {
		return () => this.createChain();
	}
	get can() {
		return () => this.createCan();
	}
	createChain(startTr, shouldDispatch = true) {
		const { rawCommands, editor, state } = this;
		const { view } = editor;
		const callbacks = [];
		const hasStartTransaction = !!startTr;
		const tr$1 = startTr || state.tr;
		const run3 = () => {
			if (!hasStartTransaction && shouldDispatch && !tr$1.getMeta("preventDispatch") && !this.hasCustomState) view.dispatch(tr$1);
			return callbacks.every((callback) => callback === true);
		};
		const chain = {
			...Object.fromEntries(Object.entries(rawCommands).map(([name, command2]) => {
				const chainedCommand = (...args) => {
					const props = this.buildProps(tr$1, shouldDispatch);
					const callback = command2(...args)(props);
					callbacks.push(callback);
					return chain;
				};
				return [name, chainedCommand];
			})),
			run: run3
		};
		return chain;
	}
	createCan(startTr) {
		const { rawCommands, state } = this;
		const dispatch = false;
		const tr$1 = startTr || state.tr;
		const props = this.buildProps(tr$1, dispatch);
		return {
			...Object.fromEntries(Object.entries(rawCommands).map(([name, command2]) => {
				return [name, (...args) => command2(...args)({
					...props,
					dispatch: void 0
				})];
			})),
			chain: () => this.createChain(tr$1, dispatch)
		};
	}
	buildProps(tr$1, shouldDispatch = true) {
		const { rawCommands, editor, state } = this;
		const { view } = editor;
		const props = {
			tr: tr$1,
			editor,
			view,
			state: createChainableState({
				state,
				transaction: tr$1
			}),
			dispatch: shouldDispatch ? () => void 0 : void 0,
			chain: () => this.createChain(tr$1, shouldDispatch),
			can: () => this.createCan(tr$1),
			get commands() {
				return Object.fromEntries(Object.entries(rawCommands).map(([name, command2]) => {
					return [name, (...args) => command2(...args)(props)];
				}));
			}
		};
		return props;
	}
};
var commands_exports = {};
__export$1(commands_exports, {
	blur: () => blur,
	clearContent: () => clearContent,
	clearNodes: () => clearNodes,
	command: () => command,
	createParagraphNear: () => createParagraphNear$1,
	cut: () => cut,
	deleteCurrentNode: () => deleteCurrentNode,
	deleteNode: () => deleteNode,
	deleteRange: () => deleteRange,
	deleteSelection: () => deleteSelection$1,
	enter: () => enter,
	exitCode: () => exitCode$1,
	extendMarkRange: () => extendMarkRange,
	first: () => first,
	focus: () => focus,
	forEach: () => forEach,
	insertContent: () => insertContent,
	insertContentAt: () => insertContentAt,
	joinBackward: () => joinBackward$1,
	joinDown: () => joinDown$1,
	joinForward: () => joinForward$1,
	joinItemBackward: () => joinItemBackward,
	joinItemForward: () => joinItemForward,
	joinTextblockBackward: () => joinTextblockBackward$1,
	joinTextblockForward: () => joinTextblockForward$1,
	joinUp: () => joinUp$1,
	keyboardShortcut: () => keyboardShortcut,
	lift: () => lift$1,
	liftEmptyBlock: () => liftEmptyBlock$1,
	liftListItem: () => liftListItem$1,
	newlineInCode: () => newlineInCode$1,
	resetAttributes: () => resetAttributes,
	scrollIntoView: () => scrollIntoView,
	selectAll: () => selectAll,
	selectNodeBackward: () => selectNodeBackward$1,
	selectNodeForward: () => selectNodeForward$1,
	selectParentNode: () => selectParentNode$1,
	selectTextblockEnd: () => selectTextblockEnd$1,
	selectTextblockStart: () => selectTextblockStart$1,
	setContent: () => setContent,
	setMark: () => setMark,
	setMeta: () => setMeta,
	setNode: () => setNode,
	setNodeSelection: () => setNodeSelection,
	setTextDirection: () => setTextDirection,
	setTextSelection: () => setTextSelection,
	sinkListItem: () => sinkListItem$1,
	splitBlock: () => splitBlock,
	splitListItem: () => splitListItem,
	toggleList: () => toggleList,
	toggleMark: () => toggleMark,
	toggleNode: () => toggleNode,
	toggleWrap: () => toggleWrap,
	undoInputRule: () => undoInputRule,
	unsetAllMarks: () => unsetAllMarks,
	unsetMark: () => unsetMark,
	unsetTextDirection: () => unsetTextDirection,
	updateAttributes: () => updateAttributes,
	wrapIn: () => wrapIn$1,
	wrapInList: () => wrapInList$1
});
var blur = () => ({ editor, view }) => {
	requestAnimationFrame(() => {
		var _a;
		if (!editor.isDestroyed) {
			view.dom.blur();
			(_a = window == null ? void 0 : window.getSelection()) == null || _a.removeAllRanges();
		}
	});
	return true;
};
var clearContent = (emitUpdate = true) => ({ commands }) => {
	return commands.setContent("", { emitUpdate });
};
var clearNodes = () => ({ state, tr: tr$1, dispatch }) => {
	const { selection } = tr$1;
	const { ranges } = selection;
	if (!dispatch) return true;
	ranges.forEach(({ $from, $to }) => {
		state.doc.nodesBetween($from.pos, $to.pos, (node, pos) => {
			if (node.type.isText) return;
			const { doc: doc$2, mapping } = tr$1;
			const $mappedFrom = doc$2.resolve(mapping.map(pos));
			const $mappedTo = doc$2.resolve(mapping.map(pos + node.nodeSize));
			const nodeRange = $mappedFrom.blockRange($mappedTo);
			if (!nodeRange) return;
			const targetLiftDepth = liftTarget(nodeRange);
			if (node.type.isTextblock) {
				const { defaultType } = $mappedFrom.parent.contentMatchAt($mappedFrom.index());
				tr$1.setNodeMarkup(nodeRange.start, defaultType);
			}
			if (targetLiftDepth || targetLiftDepth === 0) tr$1.lift(nodeRange, targetLiftDepth);
		});
	});
	return true;
};
var command = (fn) => (props) => {
	return fn(props);
};
var createParagraphNear$1 = () => ({ state, dispatch }) => {
	return createParagraphNear(state, dispatch);
};
var cut = (originRange, targetPos) => ({ editor, tr: tr$1 }) => {
	const { state } = editor;
	const contentSlice = state.doc.slice(originRange.from, originRange.to);
	tr$1.deleteRange(originRange.from, originRange.to);
	const newPos = tr$1.mapping.map(targetPos);
	tr$1.insert(newPos, contentSlice.content);
	tr$1.setSelection(new TextSelection(tr$1.doc.resolve(Math.max(newPos - 1, 0))));
	return true;
};
var deleteCurrentNode = () => ({ tr: tr$1, dispatch }) => {
	const { selection } = tr$1;
	const currentNode = selection.$anchor.node();
	if (currentNode.content.size > 0) return false;
	const $pos = tr$1.selection.$anchor;
	for (let depth = $pos.depth; depth > 0; depth -= 1) if ($pos.node(depth).type === currentNode.type) {
		if (dispatch) {
			const from = $pos.before(depth);
			const to = $pos.after(depth);
			tr$1.delete(from, to).scrollIntoView();
		}
		return true;
	}
	return false;
};
function getNodeType(nameOrType, schema) {
	if (typeof nameOrType === "string") {
		if (!schema.nodes[nameOrType]) throw Error(`There is no node type named '${nameOrType}'. Maybe you forgot to add the extension?`);
		return schema.nodes[nameOrType];
	}
	return nameOrType;
}
var deleteNode = (typeOrName) => ({ tr: tr$1, state, dispatch }) => {
	const type = getNodeType(typeOrName, state.schema);
	const $pos = tr$1.selection.$anchor;
	for (let depth = $pos.depth; depth > 0; depth -= 1) if ($pos.node(depth).type === type) {
		if (dispatch) {
			const from = $pos.before(depth);
			const to = $pos.after(depth);
			tr$1.delete(from, to).scrollIntoView();
		}
		return true;
	}
	return false;
};
var deleteRange = (range) => ({ tr: tr$1, dispatch }) => {
	const { from, to } = range;
	if (dispatch) tr$1.delete(from, to);
	return true;
};
var deleteSelection$1 = () => ({ state, dispatch }) => {
	return deleteSelection(state, dispatch);
};
var enter = () => ({ commands }) => {
	return commands.keyboardShortcut("Enter");
};
var exitCode$1 = () => ({ state, dispatch }) => {
	return exitCode(state, dispatch);
};
function isRegExp(value) {
	return Object.prototype.toString.call(value) === "[object RegExp]";
}
function objectIncludes(object1, object2, options = { strict: true }) {
	const keys$2 = Object.keys(object2);
	if (!keys$2.length) return true;
	return keys$2.every((key) => {
		if (options.strict) return object2[key] === object1[key];
		if (isRegExp(object2[key])) return object2[key].test(object1[key]);
		return object2[key] === object1[key];
	});
}
function findMarkInSet(marks, type, attributes = {}) {
	return marks.find((item) => {
		return item.type === type && objectIncludes(Object.fromEntries(Object.keys(attributes).map((k$1) => [k$1, item.attrs[k$1]])), attributes);
	});
}
function isMarkInSet(marks, type, attributes = {}) {
	return !!findMarkInSet(marks, type, attributes);
}
function getMarkRange($pos, type, attributes) {
	if (!$pos || !type) return;
	let start = $pos.parent.childAfter($pos.parentOffset);
	if (!start.node || !start.node.marks.some((mark2) => mark2.type === type)) start = $pos.parent.childBefore($pos.parentOffset);
	if (!start.node || !start.node.marks.some((mark2) => mark2.type === type)) return;
	if (!attributes) {
		const firstMark = start.node.marks.find((mark2) => mark2.type === type);
		if (firstMark) attributes = firstMark.attrs;
	}
	if (!findMarkInSet([...start.node.marks], type, attributes)) return;
	let startIndex = start.index;
	let startPos = $pos.start() + start.offset;
	let endIndex = startIndex + 1;
	let endPos = startPos + start.node.nodeSize;
	while (startIndex > 0 && isMarkInSet([...$pos.parent.child(startIndex - 1).marks], type, attributes)) {
		startIndex -= 1;
		startPos -= $pos.parent.child(startIndex).nodeSize;
	}
	while (endIndex < $pos.parent.childCount && isMarkInSet([...$pos.parent.child(endIndex).marks], type, attributes)) {
		endPos += $pos.parent.child(endIndex).nodeSize;
		endIndex += 1;
	}
	return {
		from: startPos,
		to: endPos
	};
}
function getMarkType(nameOrType, schema) {
	if (typeof nameOrType === "string") {
		if (!schema.marks[nameOrType]) throw Error(`There is no mark type named '${nameOrType}'. Maybe you forgot to add the extension?`);
		return schema.marks[nameOrType];
	}
	return nameOrType;
}
var extendMarkRange = (typeOrName, attributes) => ({ tr: tr$1, state, dispatch }) => {
	const type = getMarkType(typeOrName, state.schema);
	const { doc: doc$2, selection } = tr$1;
	const { $from, from, to } = selection;
	if (dispatch) {
		const range = getMarkRange($from, type, attributes);
		if (range && range.from <= from && range.to >= to) {
			const newSelection = TextSelection.create(doc$2, range.from, range.to);
			tr$1.setSelection(newSelection);
		}
	}
	return true;
};
var first = (commands) => (props) => {
	const items = typeof commands === "function" ? commands(props) : commands;
	for (let i$1 = 0; i$1 < items.length; i$1 += 1) if (items[i$1](props)) return true;
	return false;
};
function isTextSelection(value) {
	return value instanceof TextSelection;
}
function minMax(value = 0, min = 0, max = 0) {
	return Math.min(Math.max(value, min), max);
}
function resolveFocusPosition(doc$2, position = null) {
	if (!position) return null;
	const selectionAtStart = Selection.atStart(doc$2);
	const selectionAtEnd = Selection.atEnd(doc$2);
	if (position === "start" || position === true) return selectionAtStart;
	if (position === "end") return selectionAtEnd;
	const minPos = selectionAtStart.from;
	const maxPos = selectionAtEnd.to;
	if (position === "all") return TextSelection.create(doc$2, minMax(0, minPos, maxPos), minMax(doc$2.content.size, minPos, maxPos));
	return TextSelection.create(doc$2, minMax(position, minPos, maxPos), minMax(position, minPos, maxPos));
}
function isAndroid() {
	return navigator.platform === "Android" || /android/i.test(navigator.userAgent);
}
function isiOS() {
	return [
		"iPad Simulator",
		"iPhone Simulator",
		"iPod Simulator",
		"iPad",
		"iPhone",
		"iPod"
	].includes(navigator.platform) || navigator.userAgent.includes("Mac") && "ontouchend" in document;
}
function isSafari() {
	return typeof navigator !== "undefined" ? /^((?!chrome|android).)*safari/i.test(navigator.userAgent) : false;
}
var focus = (position = null, options = {}) => ({ editor, view, tr: tr$1, dispatch }) => {
	options = {
		scrollIntoView: true,
		...options
	};
	const delayedFocus = () => {
		if (isiOS() || isAndroid()) view.dom.focus();
		if (isSafari() && !isiOS() && !isAndroid()) view.dom.focus({ preventScroll: true });
		requestAnimationFrame(() => {
			if (!editor.isDestroyed) {
				view.focus();
				if (options == null ? void 0 : options.scrollIntoView) editor.commands.scrollIntoView();
			}
		});
	};
	try {
		if (view.hasFocus() && position === null || position === false) return true;
	} catch {
		return false;
	}
	if (dispatch && position === null && !isTextSelection(editor.state.selection)) {
		delayedFocus();
		return true;
	}
	const selection = resolveFocusPosition(tr$1.doc, position) || editor.state.selection;
	const isSameSelection = editor.state.selection.eq(selection);
	if (dispatch) {
		if (!isSameSelection) tr$1.setSelection(selection);
		if (isSameSelection && tr$1.storedMarks) tr$1.setStoredMarks(tr$1.storedMarks);
		delayedFocus();
	}
	return true;
};
var forEach = (items, fn) => (props) => {
	return items.every((item, index) => fn(item, {
		...props,
		index
	}));
};
var insertContent = (value, options) => ({ tr: tr$1, commands }) => {
	return commands.insertContentAt({
		from: tr$1.selection.from,
		to: tr$1.selection.to
	}, value, options);
};
var removeWhitespaces = (node) => {
	const children = node.childNodes;
	for (let i$1 = children.length - 1; i$1 >= 0; i$1 -= 1) {
		const child = children[i$1];
		if (child.nodeType === 3 && child.nodeValue && /^(\n\s\s|\n)$/.test(child.nodeValue)) node.removeChild(child);
		else if (child.nodeType === 1) removeWhitespaces(child);
	}
	return node;
};
function elementFromString(value) {
	if (typeof window === "undefined") throw new Error("[tiptap error]: there is no window object available, so this function cannot be used");
	const wrappedValue = `<body>${value}</body>`;
	const html = new window.DOMParser().parseFromString(wrappedValue, "text/html").body;
	return removeWhitespaces(html);
}
function createNodeFromContent(content, schema, options) {
	if (content instanceof Node || content instanceof Fragment) return content;
	options = {
		slice: true,
		parseOptions: {},
		...options
	};
	const isJSONContent = typeof content === "object" && content !== null;
	const isTextContent = typeof content === "string";
	if (isJSONContent) try {
		if (Array.isArray(content) && content.length > 0) return Fragment.fromArray(content.map((item) => schema.nodeFromJSON(item)));
		const node = schema.nodeFromJSON(content);
		if (options.errorOnInvalidContent) node.check();
		return node;
	} catch (error) {
		if (options.errorOnInvalidContent) throw new Error("[tiptap error]: Invalid JSON content", { cause: error });
		console.warn("[tiptap warn]: Invalid content.", "Passed value:", content, "Error:", error);
		return createNodeFromContent("", schema, options);
	}
	if (isTextContent) {
		if (options.errorOnInvalidContent) {
			let hasInvalidContent = false;
			let invalidContent = "";
			const contentCheckSchema = new Schema({
				topNode: schema.spec.topNode,
				marks: schema.spec.marks,
				nodes: schema.spec.nodes.append({ __tiptap__private__unknown__catch__all__node: {
					content: "inline*",
					group: "block",
					parseDOM: [{
						tag: "*",
						getAttrs: (e) => {
							hasInvalidContent = true;
							invalidContent = typeof e === "string" ? e : e.outerHTML;
							return null;
						}
					}]
				} })
			});
			if (options.slice) DOMParser.fromSchema(contentCheckSchema).parseSlice(elementFromString(content), options.parseOptions);
			else DOMParser.fromSchema(contentCheckSchema).parse(elementFromString(content), options.parseOptions);
			if (options.errorOnInvalidContent && hasInvalidContent) throw new Error("[tiptap error]: Invalid HTML content", { cause: /* @__PURE__ */ new Error(`Invalid element found: ${invalidContent}`) });
		}
		const parser = DOMParser.fromSchema(schema);
		if (options.slice) return parser.parseSlice(elementFromString(content), options.parseOptions).content;
		return parser.parse(elementFromString(content), options.parseOptions);
	}
	return createNodeFromContent("", schema, options);
}
function selectionToInsertionEnd(tr$1, startLen, bias) {
	const last = tr$1.steps.length - 1;
	if (last < startLen) return;
	const step$1 = tr$1.steps[last];
	if (!(step$1 instanceof ReplaceStep || step$1 instanceof ReplaceAroundStep)) return;
	const map = tr$1.mapping.maps[last];
	let end = 0;
	map.forEach((_from, _to, _newFrom, newTo) => {
		if (end === 0) end = newTo;
	});
	tr$1.setSelection(Selection.near(tr$1.doc.resolve(end), bias));
}
var isFragment = (nodeOrFragment) => {
	return !("type" in nodeOrFragment);
};
var insertContentAt = (position, value, options) => ({ tr: tr$1, dispatch, editor }) => {
	var _a;
	if (dispatch) {
		options = {
			parseOptions: editor.options.parseOptions,
			updateSelection: true,
			applyInputRules: false,
			applyPasteRules: false,
			...options
		};
		let content;
		const emitContentError = (error) => {
			editor.emit("contentError", {
				editor,
				error,
				disableCollaboration: () => {
					if ("collaboration" in editor.storage && typeof editor.storage.collaboration === "object" && editor.storage.collaboration) editor.storage.collaboration.isDisabled = true;
				}
			});
		};
		const parseOptions = {
			preserveWhitespace: "full",
			...options.parseOptions
		};
		if (!options.errorOnInvalidContent && !editor.options.enableContentCheck && editor.options.emitContentError) try {
			createNodeFromContent(value, editor.schema, {
				parseOptions,
				errorOnInvalidContent: true
			});
		} catch (e) {
			emitContentError(e);
		}
		try {
			content = createNodeFromContent(value, editor.schema, {
				parseOptions,
				errorOnInvalidContent: (_a = options.errorOnInvalidContent) != null ? _a : editor.options.enableContentCheck
			});
		} catch (e) {
			emitContentError(e);
			return false;
		}
		let { from, to } = typeof position === "number" ? {
			from: position,
			to: position
		} : {
			from: position.from,
			to: position.to
		};
		let isOnlyTextContent = true;
		let isOnlyBlockContent = true;
		(isFragment(content) ? content : [content]).forEach((node) => {
			node.check();
			isOnlyTextContent = isOnlyTextContent ? node.isText && node.marks.length === 0 : false;
			isOnlyBlockContent = isOnlyBlockContent ? node.isBlock : false;
		});
		if (from === to && isOnlyBlockContent) {
			const { parent } = tr$1.doc.resolve(from);
			if (parent.isTextblock && !parent.type.spec.code && !parent.childCount) {
				from -= 1;
				to += 1;
			}
		}
		let newContent;
		if (isOnlyTextContent) {
			if (Array.isArray(value)) newContent = value.map((v$1) => v$1.text || "").join("");
			else if (value instanceof Fragment) {
				let text = "";
				value.forEach((node) => {
					if (node.text) text += node.text;
				});
				newContent = text;
			} else if (typeof value === "object" && !!value && !!value.text) newContent = value.text;
			else newContent = value;
			tr$1.insertText(newContent, from, to);
		} else {
			newContent = content;
			const $from = tr$1.doc.resolve(from);
			const $fromNode = $from.node();
			const fromSelectionAtStart = $from.parentOffset === 0;
			const isTextSelection2 = $fromNode.isText || $fromNode.isTextblock;
			const hasContent = $fromNode.content.size > 0;
			if (fromSelectionAtStart && isTextSelection2 && hasContent && isOnlyBlockContent) from = Math.max(0, from - 1);
			tr$1.replaceWith(from, to, newContent);
		}
		if (options.updateSelection) selectionToInsertionEnd(tr$1, tr$1.steps.length - 1, -1);
		if (options.applyInputRules) tr$1.setMeta("applyInputRules", {
			from,
			text: newContent
		});
		if (options.applyPasteRules) tr$1.setMeta("applyPasteRules", {
			from,
			text: newContent
		});
	}
	return true;
};
var joinUp$1 = () => ({ state, dispatch }) => {
	return joinUp(state, dispatch);
};
var joinDown$1 = () => ({ state, dispatch }) => {
	return joinDown(state, dispatch);
};
var joinBackward$1 = () => ({ state, dispatch }) => {
	return joinBackward(state, dispatch);
};
var joinForward$1 = () => ({ state, dispatch }) => {
	return joinForward(state, dispatch);
};
var joinItemBackward = () => ({ state, dispatch, tr: tr$1 }) => {
	try {
		const point = joinPoint(state.doc, state.selection.$from.pos, -1);
		if (point === null || point === void 0) return false;
		tr$1.join(point, 2);
		if (dispatch) dispatch(tr$1);
		return true;
	} catch {
		return false;
	}
};
var joinItemForward = () => ({ state, dispatch, tr: tr$1 }) => {
	try {
		const point = joinPoint(state.doc, state.selection.$from.pos, 1);
		if (point === null || point === void 0) return false;
		tr$1.join(point, 2);
		if (dispatch) dispatch(tr$1);
		return true;
	} catch {
		return false;
	}
};
var joinTextblockBackward$1 = () => ({ state, dispatch }) => {
	return joinTextblockBackward(state, dispatch);
};
var joinTextblockForward$1 = () => ({ state, dispatch }) => {
	return joinTextblockForward(state, dispatch);
};
function isMacOS() {
	return typeof navigator !== "undefined" ? /Mac/.test(navigator.platform) : false;
}
function normalizeKeyName(name) {
	const parts = name.split(/-(?!$)/);
	let result = parts[parts.length - 1];
	if (result === "Space") result = " ";
	let alt;
	let ctrl;
	let shift$1;
	let meta;
	for (let i$1 = 0; i$1 < parts.length - 1; i$1 += 1) {
		const mod = parts[i$1];
		if (/^(cmd|meta|m)$/i.test(mod)) meta = true;
		else if (/^a(lt)?$/i.test(mod)) alt = true;
		else if (/^(c|ctrl|control)$/i.test(mod)) ctrl = true;
		else if (/^s(hift)?$/i.test(mod)) shift$1 = true;
		else if (/^mod$/i.test(mod)) if (isiOS() || isMacOS()) meta = true;
		else ctrl = true;
		else throw new Error(`Unrecognized modifier name: ${mod}`);
	}
	if (alt) result = `Alt-${result}`;
	if (ctrl) result = `Ctrl-${result}`;
	if (meta) result = `Meta-${result}`;
	if (shift$1) result = `Shift-${result}`;
	return result;
}
var keyboardShortcut = (name) => ({ editor, view, tr: tr$1, dispatch }) => {
	const keys$2 = normalizeKeyName(name).split(/-(?!$)/);
	const key = keys$2.find((item) => ![
		"Alt",
		"Ctrl",
		"Meta",
		"Shift"
	].includes(item));
	const event = new KeyboardEvent("keydown", {
		key: key === "Space" ? " " : key,
		altKey: keys$2.includes("Alt"),
		ctrlKey: keys$2.includes("Ctrl"),
		metaKey: keys$2.includes("Meta"),
		shiftKey: keys$2.includes("Shift"),
		bubbles: true,
		cancelable: true
	});
	editor.captureTransaction(() => {
		view.someProp("handleKeyDown", (f) => f(view, event));
	})?.steps.forEach((step$1) => {
		const newStep = step$1.map(tr$1.mapping);
		if (newStep && dispatch) tr$1.maybeStep(newStep);
	});
	return true;
};
function isNodeActive(state, typeOrName, attributes = {}) {
	const { from, to, empty: empty$1 } = state.selection;
	const type = typeOrName ? getNodeType(typeOrName, state.schema) : null;
	const nodeRanges = [];
	state.doc.nodesBetween(from, to, (node, pos) => {
		if (node.isText) return;
		const relativeFrom = Math.max(from, pos);
		const relativeTo = Math.min(to, pos + node.nodeSize);
		nodeRanges.push({
			node,
			from: relativeFrom,
			to: relativeTo
		});
	});
	const selectionRange = to - from;
	const matchedNodeRanges = nodeRanges.filter((nodeRange) => {
		if (!type) return true;
		return type.name === nodeRange.node.type.name;
	}).filter((nodeRange) => objectIncludes(nodeRange.node.attrs, attributes, { strict: false }));
	if (empty$1) return !!matchedNodeRanges.length;
	return matchedNodeRanges.reduce((sum, nodeRange) => sum + nodeRange.to - nodeRange.from, 0) >= selectionRange;
}
var lift$1 = (typeOrName, attributes = {}) => ({ state, dispatch }) => {
	if (!isNodeActive(state, getNodeType(typeOrName, state.schema), attributes)) return false;
	return lift(state, dispatch);
};
var liftEmptyBlock$1 = () => ({ state, dispatch }) => {
	return liftEmptyBlock(state, dispatch);
};
var liftListItem$1 = (typeOrName) => ({ state, dispatch }) => {
	return liftListItem(getNodeType(typeOrName, state.schema))(state, dispatch);
};
var newlineInCode$1 = () => ({ state, dispatch }) => {
	return newlineInCode(state, dispatch);
};
function getSchemaTypeNameByName(name, schema) {
	if (schema.nodes[name]) return "node";
	if (schema.marks[name]) return "mark";
	return null;
}
function deleteProps(obj, propOrProps) {
	const props = typeof propOrProps === "string" ? [propOrProps] : propOrProps;
	return Object.keys(obj).reduce((newObj, prop) => {
		if (!props.includes(prop)) newObj[prop] = obj[prop];
		return newObj;
	}, {});
}
var resetAttributes = (typeOrName, attributes) => ({ tr: tr$1, state, dispatch }) => {
	let nodeType = null;
	let markType = null;
	const schemaType = getSchemaTypeNameByName(typeof typeOrName === "string" ? typeOrName : typeOrName.name, state.schema);
	if (!schemaType) return false;
	if (schemaType === "node") nodeType = getNodeType(typeOrName, state.schema);
	if (schemaType === "mark") markType = getMarkType(typeOrName, state.schema);
	let canReset = false;
	tr$1.selection.ranges.forEach((range) => {
		state.doc.nodesBetween(range.$from.pos, range.$to.pos, (node, pos) => {
			if (nodeType && nodeType === node.type) {
				canReset = true;
				if (dispatch) tr$1.setNodeMarkup(pos, void 0, deleteProps(node.attrs, attributes));
			}
			if (markType && node.marks.length) node.marks.forEach((mark) => {
				if (markType === mark.type) {
					canReset = true;
					if (dispatch) tr$1.addMark(pos, pos + node.nodeSize, markType.create(deleteProps(mark.attrs, attributes)));
				}
			});
		});
	});
	return canReset;
};
var scrollIntoView = () => ({ tr: tr$1, dispatch }) => {
	if (dispatch) tr$1.scrollIntoView();
	return true;
};
var selectAll = () => ({ tr: tr$1, dispatch }) => {
	if (dispatch) {
		const selection = new AllSelection(tr$1.doc);
		tr$1.setSelection(selection);
	}
	return true;
};
var selectNodeBackward$1 = () => ({ state, dispatch }) => {
	return selectNodeBackward(state, dispatch);
};
var selectNodeForward$1 = () => ({ state, dispatch }) => {
	return selectNodeForward(state, dispatch);
};
var selectParentNode$1 = () => ({ state, dispatch }) => {
	return selectParentNode(state, dispatch);
};
var selectTextblockEnd$1 = () => ({ state, dispatch }) => {
	return selectTextblockEnd(state, dispatch);
};
var selectTextblockStart$1 = () => ({ state, dispatch }) => {
	return selectTextblockStart(state, dispatch);
};
function createDocument(content, schema, parseOptions = {}, options = {}) {
	return createNodeFromContent(content, schema, {
		slice: false,
		parseOptions,
		errorOnInvalidContent: options.errorOnInvalidContent
	});
}
var setContent = (content, { errorOnInvalidContent, emitUpdate = true, parseOptions = {} } = {}) => ({ editor, tr: tr$1, dispatch, commands }) => {
	const { doc: doc$2 } = tr$1;
	if (parseOptions.preserveWhitespace !== "full") {
		const document2 = createDocument(content, editor.schema, parseOptions, { errorOnInvalidContent: errorOnInvalidContent != null ? errorOnInvalidContent : editor.options.enableContentCheck });
		if (dispatch) tr$1.replaceWith(0, doc$2.content.size, document2).setMeta("preventUpdate", !emitUpdate);
		return true;
	}
	if (dispatch) tr$1.setMeta("preventUpdate", !emitUpdate);
	return commands.insertContentAt({
		from: 0,
		to: doc$2.content.size
	}, content, {
		parseOptions,
		errorOnInvalidContent: errorOnInvalidContent != null ? errorOnInvalidContent : editor.options.enableContentCheck
	});
};
function getMarkAttributes(state, typeOrName) {
	const type = getMarkType(typeOrName, state.schema);
	const { from, to, empty: empty$1 } = state.selection;
	const marks = [];
	if (empty$1) {
		if (state.storedMarks) marks.push(...state.storedMarks);
		marks.push(...state.selection.$head.marks());
	} else state.doc.nodesBetween(from, to, (node) => {
		marks.push(...node.marks);
	});
	const mark = marks.find((markItem) => markItem.type.name === type.name);
	if (!mark) return {};
	return { ...mark.attrs };
}
function combineTransactionSteps(oldDoc, transactions) {
	const transform = new Transform(oldDoc);
	transactions.forEach((transaction) => {
		transaction.steps.forEach((step$1) => {
			transform.step(step$1);
		});
	});
	return transform;
}
function defaultBlockAt(match) {
	for (let i$1 = 0; i$1 < match.edgeCount; i$1 += 1) {
		const { type } = match.edge(i$1);
		if (type.isTextblock && !type.hasRequiredAttrs()) return type;
	}
	return null;
}
function findChildren(node, predicate) {
	const nodesWithPos = [];
	node.descendants((child, pos) => {
		if (predicate(child)) nodesWithPos.push({
			node: child,
			pos
		});
	});
	return nodesWithPos;
}
function findChildrenInRange(node, range, predicate) {
	const nodesWithPos = [];
	node.nodesBetween(range.from, range.to, (child, pos) => {
		if (predicate(child)) nodesWithPos.push({
			node: child,
			pos
		});
	});
	return nodesWithPos;
}
function findParentNodeClosestToPos($pos, predicate) {
	for (let i$1 = $pos.depth; i$1 > 0; i$1 -= 1) {
		const node = $pos.node(i$1);
		if (predicate(node)) return {
			pos: i$1 > 0 ? $pos.before(i$1) : 0,
			start: $pos.start(i$1),
			depth: i$1,
			node
		};
	}
}
function findParentNode(predicate) {
	return (selection) => findParentNodeClosestToPos(selection.$from, predicate);
}
function getExtensionField(extension, field, context) {
	if (extension.config[field] === void 0 && extension.parent) return getExtensionField(extension.parent, field, context);
	if (typeof extension.config[field] === "function") return extension.config[field].bind({
		...context,
		parent: extension.parent ? getExtensionField(extension.parent, field, context) : null
	});
	return extension.config[field];
}
function flattenExtensions(extensions) {
	return extensions.map((extension) => {
		const addExtensions = getExtensionField(extension, "addExtensions", {
			name: extension.name,
			options: extension.options,
			storage: extension.storage
		});
		if (addExtensions) return [extension, ...flattenExtensions(addExtensions())];
		return extension;
	}).flat(10);
}
function getHTMLFromFragment(fragment, schema) {
	const documentFragment = DOMSerializer.fromSchema(schema).serializeFragment(fragment);
	const container = document.implementation.createHTMLDocument().createElement("div");
	container.appendChild(documentFragment);
	return container.innerHTML;
}
function isFunction$1(value) {
	return typeof value === "function";
}
function callOrReturn(value, context = void 0, ...props) {
	if (isFunction$1(value)) {
		if (context) return value.bind(context)(...props);
		return value(...props);
	}
	return value;
}
function isEmptyObject(value = {}) {
	return Object.keys(value).length === 0 && value.constructor === Object;
}
function splitExtensions(extensions) {
	return {
		baseExtensions: extensions.filter((extension) => extension.type === "extension"),
		nodeExtensions: extensions.filter((extension) => extension.type === "node"),
		markExtensions: extensions.filter((extension) => extension.type === "mark")
	};
}
function getAttributesFromExtensions(extensions) {
	const extensionAttributes = [];
	const { nodeExtensions, markExtensions } = splitExtensions(extensions);
	const nodeAndMarkExtensions = [...nodeExtensions, ...markExtensions];
	const defaultAttribute = {
		default: null,
		validate: void 0,
		rendered: true,
		renderHTML: null,
		parseHTML: null,
		keepOnSplit: true,
		isRequired: false
	};
	const nodeExtensionTypes = nodeExtensions.filter((ext) => ext.name !== "text").map((ext) => ext.name);
	const markExtensionTypes = markExtensions.map((ext) => ext.name);
	const allExtensionTypes = [...nodeExtensionTypes, ...markExtensionTypes];
	extensions.forEach((extension) => {
		const addGlobalAttributes = getExtensionField(extension, "addGlobalAttributes", {
			name: extension.name,
			options: extension.options,
			storage: extension.storage,
			extensions: nodeAndMarkExtensions
		});
		if (!addGlobalAttributes) return;
		addGlobalAttributes().forEach((globalAttribute) => {
			let resolvedTypes;
			if (Array.isArray(globalAttribute.types)) resolvedTypes = globalAttribute.types;
			else if (globalAttribute.types === "*") resolvedTypes = allExtensionTypes;
			else if (globalAttribute.types === "nodes") resolvedTypes = nodeExtensionTypes;
			else if (globalAttribute.types === "marks") resolvedTypes = markExtensionTypes;
			else resolvedTypes = [];
			resolvedTypes.forEach((type) => {
				Object.entries(globalAttribute.attributes).forEach(([name, attribute]) => {
					extensionAttributes.push({
						type,
						name,
						attribute: {
							...defaultAttribute,
							...attribute
						}
					});
				});
			});
		});
	});
	nodeAndMarkExtensions.forEach((extension) => {
		const addAttributes = getExtensionField(extension, "addAttributes", {
			name: extension.name,
			options: extension.options,
			storage: extension.storage
		});
		if (!addAttributes) return;
		const attributes = addAttributes();
		Object.entries(attributes).forEach(([name, attribute]) => {
			const mergedAttr = {
				...defaultAttribute,
				...attribute
			};
			if (typeof (mergedAttr == null ? void 0 : mergedAttr.default) === "function") mergedAttr.default = mergedAttr.default();
			if ((mergedAttr == null ? void 0 : mergedAttr.isRequired) && (mergedAttr == null ? void 0 : mergedAttr.default) === void 0) delete mergedAttr.default;
			extensionAttributes.push({
				type: extension.name,
				name,
				attribute: mergedAttr
			});
		});
	});
	return extensionAttributes;
}
function splitStyleDeclarations(styles) {
	const result = [];
	let current = "";
	let inSingleQuote = false;
	let inDoubleQuote = false;
	let parenDepth = 0;
	const length = styles.length;
	for (let i$1 = 0; i$1 < length; i$1 += 1) {
		const char = styles[i$1];
		if (char === "'" && !inDoubleQuote) {
			inSingleQuote = !inSingleQuote;
			current += char;
			continue;
		}
		if (char === "\"" && !inSingleQuote) {
			inDoubleQuote = !inDoubleQuote;
			current += char;
			continue;
		}
		if (!inSingleQuote && !inDoubleQuote) {
			if (char === "(") {
				parenDepth += 1;
				current += char;
				continue;
			}
			if (char === ")" && parenDepth > 0) {
				parenDepth -= 1;
				current += char;
				continue;
			}
			if (char === ";" && parenDepth === 0) {
				result.push(current);
				current = "";
				continue;
			}
		}
		current += char;
	}
	if (current) result.push(current);
	return result;
}
function parseStyleEntries(styles) {
	const pairs = [];
	const declarations = splitStyleDeclarations(styles || "");
	const numDeclarations = declarations.length;
	for (let i$1 = 0; i$1 < numDeclarations; i$1 += 1) {
		const declaration = declarations[i$1];
		const firstColonIndex = declaration.indexOf(":");
		if (firstColonIndex === -1) continue;
		const property = declaration.slice(0, firstColonIndex).trim();
		const value = declaration.slice(firstColonIndex + 1).trim();
		if (property && value) pairs.push([property, value]);
	}
	return pairs;
}
function mergeAttributes(...objects) {
	return objects.filter((item) => !!item).reduce((items, item) => {
		const mergedAttributes = { ...items };
		Object.entries(item).forEach(([key, value]) => {
			if (!mergedAttributes[key]) {
				mergedAttributes[key] = value;
				return;
			}
			if (key === "class") {
				const valueClasses = value ? String(value).split(" ") : [];
				const existingClasses = mergedAttributes[key] ? mergedAttributes[key].split(" ") : [];
				const insertClasses = valueClasses.filter((valueClass) => !existingClasses.includes(valueClass));
				mergedAttributes[key] = [...existingClasses, ...insertClasses].join(" ");
			} else if (key === "style") {
				const styleMap = new Map([...parseStyleEntries(mergedAttributes[key]), ...parseStyleEntries(value)]);
				mergedAttributes[key] = Array.from(styleMap.entries()).map(([property, val]) => `${property}: ${val}`).join("; ");
			} else mergedAttributes[key] = value;
		});
		return mergedAttributes;
	}, {});
}
function getRenderedAttributes(nodeOrMark, extensionAttributes) {
	return extensionAttributes.filter((attribute) => attribute.type === nodeOrMark.type.name).filter((item) => item.attribute.rendered).map((item) => {
		if (!item.attribute.renderHTML) return { [item.name]: nodeOrMark.attrs[item.name] };
		return item.attribute.renderHTML(nodeOrMark.attrs) || {};
	}).reduce((attributes, attribute) => mergeAttributes(attributes, attribute), {});
}
function fromString(value) {
	if (typeof value !== "string") return value;
	if (value.match(/^[+-]?(?:\d*\.)?\d+$/)) return Number(value);
	if (value === "true") return true;
	if (value === "false") return false;
	return value;
}
function injectExtensionAttributesToParseRule(parseRule, extensionAttributes) {
	if ("style" in parseRule) return parseRule;
	return {
		...parseRule,
		getAttrs: (node) => {
			const oldAttributes = parseRule.getAttrs ? parseRule.getAttrs(node) : parseRule.attrs;
			if (oldAttributes === false) return false;
			const newAttributes = extensionAttributes.reduce((items, item) => {
				const value = item.attribute.parseHTML ? item.attribute.parseHTML(node) : fromString(node.getAttribute(item.name));
				if (value === null || value === void 0) return items;
				return {
					...items,
					[item.name]: value
				};
			}, {});
			return {
				...oldAttributes,
				...newAttributes
			};
		}
	};
}
function cleanUpSchemaItem(data) {
	return Object.fromEntries(Object.entries(data).filter(([key, value]) => {
		if (key === "attrs" && isEmptyObject(value)) return false;
		return value !== null && value !== void 0;
	}));
}
function buildAttributeSpec(extensionAttribute) {
	var _a, _b;
	const spec = {};
	if (!((_a = extensionAttribute == null ? void 0 : extensionAttribute.attribute) == null ? void 0 : _a.isRequired) && "default" in ((extensionAttribute == null ? void 0 : extensionAttribute.attribute) || {})) spec.default = extensionAttribute.attribute.default;
	if (((_b = extensionAttribute == null ? void 0 : extensionAttribute.attribute) == null ? void 0 : _b.validate) !== void 0) spec.validate = extensionAttribute.attribute.validate;
	return [extensionAttribute.name, spec];
}
function getSchemaByResolvedExtensions(extensions, editor) {
	var _a;
	const allAttributes = getAttributesFromExtensions(extensions);
	const { nodeExtensions, markExtensions } = splitExtensions(extensions);
	return new Schema({
		topNode: (_a = nodeExtensions.find((extension) => getExtensionField(extension, "topNode"))) == null ? void 0 : _a.name,
		nodes: Object.fromEntries(nodeExtensions.map((extension) => {
			const extensionAttributes = allAttributes.filter((attribute) => attribute.type === extension.name);
			const context = {
				name: extension.name,
				options: extension.options,
				storage: extension.storage,
				editor
			};
			const schema = cleanUpSchemaItem({
				...extensions.reduce((fields, e) => {
					const extendNodeSchema = getExtensionField(e, "extendNodeSchema", context);
					return {
						...fields,
						...extendNodeSchema ? extendNodeSchema(extension) : {}
					};
				}, {}),
				content: callOrReturn(getExtensionField(extension, "content", context)),
				marks: callOrReturn(getExtensionField(extension, "marks", context)),
				group: callOrReturn(getExtensionField(extension, "group", context)),
				inline: callOrReturn(getExtensionField(extension, "inline", context)),
				atom: callOrReturn(getExtensionField(extension, "atom", context)),
				selectable: callOrReturn(getExtensionField(extension, "selectable", context)),
				draggable: callOrReturn(getExtensionField(extension, "draggable", context)),
				code: callOrReturn(getExtensionField(extension, "code", context)),
				whitespace: callOrReturn(getExtensionField(extension, "whitespace", context)),
				linebreakReplacement: callOrReturn(getExtensionField(extension, "linebreakReplacement", context)),
				defining: callOrReturn(getExtensionField(extension, "defining", context)),
				isolating: callOrReturn(getExtensionField(extension, "isolating", context)),
				attrs: Object.fromEntries(extensionAttributes.map(buildAttributeSpec))
			});
			const parseHTML = callOrReturn(getExtensionField(extension, "parseHTML", context));
			if (parseHTML) schema.parseDOM = parseHTML.map((parseRule) => injectExtensionAttributesToParseRule(parseRule, extensionAttributes));
			const renderHTML = getExtensionField(extension, "renderHTML", context);
			if (renderHTML) schema.toDOM = (node) => renderHTML({
				node,
				HTMLAttributes: getRenderedAttributes(node, extensionAttributes)
			});
			const renderText = getExtensionField(extension, "renderText", context);
			if (renderText) schema.toText = renderText;
			return [extension.name, schema];
		})),
		marks: Object.fromEntries(markExtensions.map((extension) => {
			const extensionAttributes = allAttributes.filter((attribute) => attribute.type === extension.name);
			const context = {
				name: extension.name,
				options: extension.options,
				storage: extension.storage,
				editor
			};
			const schema = cleanUpSchemaItem({
				...extensions.reduce((fields, e) => {
					const extendMarkSchema = getExtensionField(e, "extendMarkSchema", context);
					return {
						...fields,
						...extendMarkSchema ? extendMarkSchema(extension) : {}
					};
				}, {}),
				inclusive: callOrReturn(getExtensionField(extension, "inclusive", context)),
				excludes: callOrReturn(getExtensionField(extension, "excludes", context)),
				group: callOrReturn(getExtensionField(extension, "group", context)),
				spanning: callOrReturn(getExtensionField(extension, "spanning", context)),
				code: callOrReturn(getExtensionField(extension, "code", context)),
				attrs: Object.fromEntries(extensionAttributes.map(buildAttributeSpec))
			});
			const parseHTML = callOrReturn(getExtensionField(extension, "parseHTML", context));
			if (parseHTML) schema.parseDOM = parseHTML.map((parseRule) => injectExtensionAttributesToParseRule(parseRule, extensionAttributes));
			const renderHTML = getExtensionField(extension, "renderHTML", context);
			if (renderHTML) schema.toDOM = (mark) => renderHTML({
				mark,
				HTMLAttributes: getRenderedAttributes(mark, extensionAttributes)
			});
			return [extension.name, schema];
		}))
	});
}
function findDuplicates(items) {
	const filtered = items.filter((el, index) => items.indexOf(el) !== index);
	return Array.from(new Set(filtered));
}
function sortExtensions(extensions) {
	const defaultPriority = 100;
	return extensions.sort((a, b$1) => {
		const priorityA = getExtensionField(a, "priority") || defaultPriority;
		const priorityB = getExtensionField(b$1, "priority") || defaultPriority;
		if (priorityA > priorityB) return -1;
		if (priorityA < priorityB) return 1;
		return 0;
	});
}
function resolveExtensions(extensions) {
	const resolvedExtensions = sortExtensions(flattenExtensions(extensions));
	const duplicatedNames = findDuplicates(resolvedExtensions.map((extension) => extension.name));
	if (duplicatedNames.length) console.warn(`[tiptap warn]: Duplicate extension names found: [${duplicatedNames.map((item) => `'${item}'`).join(", ")}]. This can lead to issues.`);
	return resolvedExtensions;
}
function getSchema(extensions, editor) {
	return getSchemaByResolvedExtensions(resolveExtensions(extensions), editor);
}
function generateJSON(html, extensions) {
	const schema = getSchema(extensions);
	const dom = elementFromString(html);
	return DOMParser.fromSchema(schema).parse(dom).toJSON();
}
function getTextBetween(startNode, range, options) {
	const { from, to } = range;
	const { blockSeparator = "\n\n", textSerializers = {} } = options || {};
	let text = "";
	startNode.nodesBetween(from, to, (node, pos, parent, index) => {
		var _a;
		if (node.isBlock && pos > from) text += blockSeparator;
		const textSerializer = textSerializers == null ? void 0 : textSerializers[node.type.name];
		if (textSerializer) {
			if (parent) text += textSerializer({
				node,
				pos,
				parent,
				index,
				range
			});
			return false;
		}
		if (node.isText) text += (_a = node == null ? void 0 : node.text) == null ? void 0 : _a.slice(Math.max(from, pos) - pos, to - pos);
	});
	return text;
}
function getText(node, options) {
	return getTextBetween(node, {
		from: 0,
		to: node.content.size
	}, options);
}
function getTextSerializersFromSchema(schema) {
	return Object.fromEntries(Object.entries(schema.nodes).filter(([, node]) => node.spec.toText).map(([name, node]) => [name, node.spec.toText]));
}
function getNodeAttributes(state, typeOrName) {
	const type = getNodeType(typeOrName, state.schema);
	const { from, to } = state.selection;
	const nodes = [];
	state.doc.nodesBetween(from, to, (node2) => {
		nodes.push(node2);
	});
	const node = nodes.reverse().find((nodeItem) => nodeItem.type.name === type.name);
	if (!node) return {};
	return { ...node.attrs };
}
function getAttributes(state, typeOrName) {
	const schemaType = getSchemaTypeNameByName(typeof typeOrName === "string" ? typeOrName : typeOrName.name, state.schema);
	if (schemaType === "node") return getNodeAttributes(state, typeOrName);
	if (schemaType === "mark") return getMarkAttributes(state, typeOrName);
	return {};
}
function removeDuplicates(array, by = JSON.stringify) {
	const seen = {};
	return array.filter((item) => {
		const key = by(item);
		return Object.prototype.hasOwnProperty.call(seen, key) ? false : seen[key] = true;
	});
}
function simplifyChangedRanges(changes) {
	const uniqueChanges = removeDuplicates(changes);
	return uniqueChanges.length === 1 ? uniqueChanges : uniqueChanges.filter((change, index) => {
		return !uniqueChanges.filter((_$1, i$1) => i$1 !== index).some((otherChange) => {
			return change.oldRange.from >= otherChange.oldRange.from && change.oldRange.to <= otherChange.oldRange.to && change.newRange.from >= otherChange.newRange.from && change.newRange.to <= otherChange.newRange.to;
		});
	});
}
function getChangedRanges(transform) {
	const { mapping, steps } = transform;
	const changes = [];
	mapping.maps.forEach((stepMap, index) => {
		const ranges = [];
		if (!stepMap.ranges.length) {
			const { from, to } = steps[index];
			if (from === void 0 || to === void 0) return;
			ranges.push({
				from,
				to
			});
		} else stepMap.forEach((from, to) => {
			ranges.push({
				from,
				to
			});
		});
		ranges.forEach(({ from, to }) => {
			const newStart = mapping.slice(index).map(from, -1);
			const newEnd = mapping.slice(index).map(to);
			const oldStart = mapping.invert().map(newStart, -1);
			const oldEnd = mapping.invert().map(newEnd);
			changes.push({
				oldRange: {
					from: oldStart,
					to: oldEnd
				},
				newRange: {
					from: newStart,
					to: newEnd
				}
			});
		});
	});
	return simplifyChangedRanges(changes);
}
function getMarksBetween(from, to, doc$2) {
	const marks = [];
	if (from === to) doc$2.resolve(from).marks().forEach((mark) => {
		const range = getMarkRange(doc$2.resolve(from), mark.type);
		if (!range) return;
		marks.push({
			mark,
			...range
		});
	});
	else doc$2.nodesBetween(from, to, (node, pos) => {
		if (!node || (node == null ? void 0 : node.nodeSize) === void 0) return;
		marks.push(...node.marks.map((mark) => ({
			from: pos,
			to: pos + node.nodeSize,
			mark
		})));
	});
	return marks;
}
var getNodeAtPosition = (state, typeOrName, pos, maxDepth = 20) => {
	const $pos = state.doc.resolve(pos);
	let currentDepth = maxDepth;
	let node = null;
	while (currentDepth > 0 && node === null) {
		const currentNode = $pos.node(currentDepth);
		if ((currentNode == null ? void 0 : currentNode.type.name) === typeOrName) node = currentNode;
		else currentDepth -= 1;
	}
	return [node, currentDepth];
};
function getSchemaTypeByName(name, schema) {
	return schema.nodes[name] || schema.marks[name] || null;
}
function getSplittedAttributes(extensionAttributes, typeName, attributes) {
	return Object.fromEntries(Object.entries(attributes).filter(([name]) => {
		const extensionAttribute = extensionAttributes.find((item) => {
			return item.type === typeName && item.name === name;
		});
		if (!extensionAttribute) return false;
		return extensionAttribute.attribute.keepOnSplit;
	}));
}
var getTextContentFromNodes = ($from, maxMatch = 500) => {
	let textBefore = "";
	const sliceEndPos = $from.parentOffset;
	$from.parent.nodesBetween(Math.max(0, sliceEndPos - maxMatch), sliceEndPos, (node, pos, parent, index) => {
		var _a, _b;
		const chunk = ((_b = (_a = node.type.spec).toText) == null ? void 0 : _b.call(_a, {
			node,
			pos,
			parent,
			index
		})) || node.textContent || "%leaf%";
		textBefore += node.isAtom && !node.isText ? chunk : chunk.slice(0, Math.max(0, sliceEndPos - pos));
	});
	return textBefore;
};
function isMarkActive(state, typeOrName, attributes = {}) {
	const { empty: empty$1, ranges } = state.selection;
	const type = typeOrName ? getMarkType(typeOrName, state.schema) : null;
	if (empty$1) return !!(state.storedMarks || state.selection.$from.marks()).filter((mark) => {
		if (!type) return true;
		return type.name === mark.type.name;
	}).find((mark) => objectIncludes(mark.attrs, attributes, { strict: false }));
	let selectionRange = 0;
	const markRanges = [];
	ranges.forEach(({ $from, $to }) => {
		const from = $from.pos;
		const to = $to.pos;
		state.doc.nodesBetween(from, to, (node, pos) => {
			if (type && node.inlineContent && !node.type.allowsMarkType(type)) return false;
			if (!node.isText && !node.marks.length) return;
			const relativeFrom = Math.max(from, pos);
			const relativeTo = Math.min(to, pos + node.nodeSize);
			const range2 = relativeTo - relativeFrom;
			selectionRange += range2;
			markRanges.push(...node.marks.map((mark) => ({
				mark,
				from: relativeFrom,
				to: relativeTo
			})));
		});
	});
	if (selectionRange === 0) return false;
	const matchedRange = markRanges.filter((markRange) => {
		if (!type) return true;
		return type.name === markRange.mark.type.name;
	}).filter((markRange) => objectIncludes(markRange.mark.attrs, attributes, { strict: false })).reduce((sum, markRange) => sum + markRange.to - markRange.from, 0);
	const excludedRange = markRanges.filter((markRange) => {
		if (!type) return true;
		return markRange.mark.type !== type && markRange.mark.type.excludes(type);
	}).reduce((sum, markRange) => sum + markRange.to - markRange.from, 0);
	return (matchedRange > 0 ? matchedRange + excludedRange : matchedRange) >= selectionRange;
}
function isActive(state, name, attributes = {}) {
	if (!name) return isNodeActive(state, null, attributes) || isMarkActive(state, null, attributes);
	const schemaType = getSchemaTypeNameByName(name, state.schema);
	if (schemaType === "node") return isNodeActive(state, name, attributes);
	if (schemaType === "mark") return isMarkActive(state, name, attributes);
	return false;
}
var isAtEndOfNode = (state, nodeType) => {
	const { $from, $to, $anchor } = state.selection;
	if (nodeType) {
		const parentNode$1 = findParentNode((node) => node.type.name === nodeType)(state.selection);
		if (!parentNode$1) return false;
		const $parentPos = state.doc.resolve(parentNode$1.pos + 1);
		if ($anchor.pos + 1 === $parentPos.end()) return true;
		return false;
	}
	if ($to.parentOffset < $to.parent.nodeSize - 2 || $from.pos !== $to.pos) return false;
	return true;
};
var isAtStartOfNode = (state) => {
	const { $from, $to } = state.selection;
	if ($from.parentOffset > 0 || $from.pos !== $to.pos) return false;
	return true;
};
function isExtensionRulesEnabled(extension, enabled) {
	if (Array.isArray(enabled)) return enabled.some((enabledExtension) => {
		return (typeof enabledExtension === "string" ? enabledExtension : enabledExtension.name) === extension.name;
	});
	return enabled;
}
function isList(name, extensions) {
	const { nodeExtensions } = splitExtensions(extensions);
	const extension = nodeExtensions.find((item) => item.name === name);
	if (!extension) return false;
	const group = callOrReturn(getExtensionField(extension, "group", {
		name: extension.name,
		options: extension.options,
		storage: extension.storage
	}));
	if (typeof group !== "string") return false;
	return group.split(" ").includes("list");
}
function isNodeEmpty(node, { checkChildren = true, ignoreWhitespace = false } = {}) {
	var _a;
	if (ignoreWhitespace) {
		if (node.type.name === "hardBreak") return true;
		if (node.isText) return !/\S/.test((_a = node.text) != null ? _a : "");
	}
	if (node.isText) return !node.text;
	if (node.isAtom || node.isLeaf) return false;
	if (node.content.childCount === 0) return true;
	if (checkChildren) {
		let isContentEmpty = true;
		node.content.forEach((childNode) => {
			if (isContentEmpty === false) return;
			if (!isNodeEmpty(childNode, {
				ignoreWhitespace,
				checkChildren
			})) isContentEmpty = false;
		});
		return isContentEmpty;
	}
	return false;
}
function isNodeSelection(value) {
	return value instanceof NodeSelection;
}
function isNodeViewSelected({ selection, pos, nodeSize: nodeSize$1, selectedOnTextSelection = false }) {
	const { from, to } = selection;
	if (from <= pos && to >= pos + nodeSize$1) return true;
	if (selectedOnTextSelection && isTextSelection(selection) && from > pos && to < pos + nodeSize$1) return true;
	return false;
}
var MappablePosition = class _MappablePosition {
	constructor(position) {
		this.position = position;
	}
	static fromJSON(json) {
		return new _MappablePosition(json.position);
	}
	toJSON() {
		return { position: this.position };
	}
};
function getUpdatedPosition(position, transaction) {
	const mapResult = transaction.mapping.mapResult(position.position);
	return {
		position: new MappablePosition(mapResult.pos),
		mapResult
	};
}
function createMappablePosition(position) {
	return new MappablePosition(position);
}
function canSetMark(state, tr$1, newMarkType) {
	var _a;
	const { selection } = tr$1;
	let cursor = null;
	if (isTextSelection(selection)) cursor = selection.$cursor;
	if (cursor) {
		const currentMarks = (_a = state.storedMarks) != null ? _a : cursor.marks();
		return cursor.parent.type.allowsMarkType(newMarkType) && (!!newMarkType.isInSet(currentMarks) || !currentMarks.some((mark) => mark.type.excludes(newMarkType)));
	}
	const { ranges } = selection;
	return ranges.some(({ $from, $to }) => {
		let someNodeSupportsMark = $from.depth === 0 ? state.doc.inlineContent && state.doc.type.allowsMarkType(newMarkType) : false;
		state.doc.nodesBetween($from.pos, $to.pos, (node, _pos, parent) => {
			if (someNodeSupportsMark) return false;
			if (node.isInline) {
				const parentAllowsMarkType = !parent || parent.type.allowsMarkType(newMarkType);
				const currentMarksAllowMarkType = !!newMarkType.isInSet(node.marks) || !node.marks.some((otherMark) => otherMark.type.excludes(newMarkType));
				someNodeSupportsMark = parentAllowsMarkType && currentMarksAllowMarkType;
			}
			return !someNodeSupportsMark;
		});
		return someNodeSupportsMark;
	});
}
var setMark = (typeOrName, attributes = {}) => ({ tr: tr$1, state, dispatch }) => {
	const { selection } = tr$1;
	const { empty: empty$1, ranges } = selection;
	const type = getMarkType(typeOrName, state.schema);
	if (dispatch) if (empty$1) {
		const oldAttributes = getMarkAttributes(state, type);
		tr$1.addStoredMark(type.create({
			...oldAttributes,
			...attributes
		}));
	} else ranges.forEach((range) => {
		const from = range.$from.pos;
		const to = range.$to.pos;
		state.doc.nodesBetween(from, to, (node, pos) => {
			const trimmedFrom = Math.max(pos, from);
			const trimmedTo = Math.min(pos + node.nodeSize, to);
			if (node.marks.find((mark) => mark.type === type)) node.marks.forEach((mark) => {
				if (type === mark.type) tr$1.addMark(trimmedFrom, trimmedTo, type.create({
					...mark.attrs,
					...attributes
				}));
			});
			else tr$1.addMark(trimmedFrom, trimmedTo, type.create(attributes));
		});
	});
	return canSetMark(state, tr$1, type);
};
var setMeta = (key, value) => ({ tr: tr$1 }) => {
	tr$1.setMeta(key, value);
	return true;
};
var setNode = (typeOrName, attributes = {}) => ({ state, dispatch, chain }) => {
	const type = getNodeType(typeOrName, state.schema);
	let attributesToCopy;
	if (state.selection.$anchor.sameParent(state.selection.$head)) attributesToCopy = state.selection.$anchor.parent.attrs;
	if (!type.isTextblock) {
		console.warn("[tiptap warn]: Currently \"setNode()\" only supports text block nodes.");
		return false;
	}
	return chain().command(({ commands }) => {
		if (setBlockType(type, {
			...attributesToCopy,
			...attributes
		})(state)) return true;
		return commands.clearNodes();
	}).command(({ state: updatedState }) => {
		return setBlockType(type, {
			...attributesToCopy,
			...attributes
		})(updatedState, dispatch);
	}).run();
};
var setNodeSelection = (position) => ({ tr: tr$1, dispatch }) => {
	if (dispatch) {
		const { doc: doc$2 } = tr$1;
		const from = minMax(position, 0, doc$2.content.size);
		const selection = NodeSelection.create(doc$2, from);
		tr$1.setSelection(selection);
	}
	return true;
};
var setTextDirection = (direction, position) => ({ tr: tr$1, state, dispatch }) => {
	const { selection } = state;
	let from;
	let to;
	if (typeof position === "number") {
		from = position;
		to = position;
	} else if (position && "from" in position && "to" in position) {
		from = position.from;
		to = position.to;
	} else {
		from = selection.from;
		to = selection.to;
	}
	if (dispatch) tr$1.doc.nodesBetween(from, to, (node, pos) => {
		if (node.isText) return;
		tr$1.setNodeMarkup(pos, void 0, {
			...node.attrs,
			dir: direction
		});
	});
	return true;
};
var setTextSelection = (position) => ({ tr: tr$1, dispatch }) => {
	if (dispatch) {
		const { doc: doc$2 } = tr$1;
		const { from, to } = typeof position === "number" ? {
			from: position,
			to: position
		} : position;
		const minPos = TextSelection.atStart(doc$2).from;
		const maxPos = TextSelection.atEnd(doc$2).to;
		const resolvedFrom = minMax(from, minPos, maxPos);
		const resolvedEnd = minMax(to, minPos, maxPos);
		const selection = TextSelection.create(doc$2, resolvedFrom, resolvedEnd);
		tr$1.setSelection(selection);
	}
	return true;
};
var sinkListItem$1 = (typeOrName) => ({ state, dispatch }) => {
	return sinkListItem(getNodeType(typeOrName, state.schema))(state, dispatch);
};
function ensureMarks(state, splittableMarks) {
	const marks = state.storedMarks || state.selection.$to.parentOffset && state.selection.$from.marks();
	if (marks) {
		const filteredMarks = marks.filter((mark) => splittableMarks == null ? void 0 : splittableMarks.includes(mark.type.name));
		state.tr.ensureMarks(filteredMarks);
	}
}
var splitBlock = ({ keepMarks = true } = {}) => ({ tr: tr$1, state, dispatch, editor }) => {
	const { selection, doc: doc$2 } = tr$1;
	const { $from, $to } = selection;
	const extensionAttributes = editor.extensionManager.attributes;
	const newAttributes = getSplittedAttributes(extensionAttributes, $from.node().type.name, $from.node().attrs);
	if (selection instanceof NodeSelection && selection.node.isBlock) {
		if (!$from.parentOffset || !canSplit(doc$2, $from.pos)) return false;
		if (dispatch) {
			if (keepMarks) ensureMarks(state, editor.extensionManager.splittableMarks);
			tr$1.split($from.pos).scrollIntoView();
		}
		return true;
	}
	if (!$from.parent.isBlock) return false;
	const atEnd = $to.parentOffset === $to.parent.content.size;
	const deflt = $from.depth === 0 ? void 0 : defaultBlockAt($from.node(-1).contentMatchAt($from.indexAfter(-1)));
	let types = atEnd && deflt ? [{
		type: deflt,
		attrs: newAttributes
	}] : void 0;
	let can = canSplit(tr$1.doc, tr$1.mapping.map($from.pos), 1, types);
	if (!types && !can && canSplit(tr$1.doc, tr$1.mapping.map($from.pos), 1, deflt ? [{ type: deflt }] : void 0)) {
		can = true;
		types = deflt ? [{
			type: deflt,
			attrs: newAttributes
		}] : void 0;
	}
	if (dispatch) {
		if (can) {
			if (selection instanceof TextSelection) tr$1.deleteSelection();
			tr$1.split(tr$1.mapping.map($from.pos), 1, types);
			if (deflt && !atEnd && !$from.parentOffset && $from.parent.type !== deflt) {
				const first2 = tr$1.mapping.map($from.before());
				const $first = tr$1.doc.resolve(first2);
				if ($from.node(-1).canReplaceWith($first.index(), $first.index() + 1, deflt)) tr$1.setNodeMarkup(tr$1.mapping.map($from.before()), deflt);
			}
		}
		if (keepMarks) ensureMarks(state, editor.extensionManager.splittableMarks);
		tr$1.scrollIntoView();
	}
	return can;
};
var splitListItem = (typeOrName, overrideAttrs = {}) => ({ tr: tr$1, state, dispatch, editor }) => {
	var _a;
	const type = getNodeType(typeOrName, state.schema);
	const { $from, $to } = state.selection;
	const node = state.selection.node;
	if (node && node.isBlock || $from.depth < 2 || !$from.sameParent($to)) return false;
	const grandParent = $from.node(-1);
	if (grandParent.type !== type) return false;
	const extensionAttributes = editor.extensionManager.attributes;
	if ($from.parent.content.size === 0 && $from.node(-1).childCount === $from.indexAfter(-1)) {
		if ($from.depth === 2 || $from.node(-3).type !== type || $from.index(-2) !== $from.node(-2).childCount - 1) return false;
		if (dispatch) {
			let wrap$1 = Fragment.empty;
			const depthBefore = $from.index(-1) ? 1 : $from.index(-2) ? 2 : 3;
			for (let d = $from.depth - depthBefore; d >= $from.depth - 3; d -= 1) wrap$1 = Fragment.from($from.node(d).copy(wrap$1));
			const depthAfter = $from.indexAfter(-1) < $from.node(-2).childCount ? 1 : $from.indexAfter(-2) < $from.node(-3).childCount ? 2 : 3;
			const newNextTypeAttributes2 = {
				...getSplittedAttributes(extensionAttributes, $from.node().type.name, $from.node().attrs),
				...overrideAttrs
			};
			const nextType2 = ((_a = type.contentMatch.defaultType) == null ? void 0 : _a.createAndFill(newNextTypeAttributes2)) || void 0;
			wrap$1 = wrap$1.append(Fragment.from(type.createAndFill(null, nextType2) || void 0));
			const start = $from.before($from.depth - (depthBefore - 1));
			tr$1.replace(start, $from.after(-depthAfter), new Slice(wrap$1, 4 - depthBefore, 0));
			let sel = -1;
			tr$1.doc.nodesBetween(start, tr$1.doc.content.size, (n, pos) => {
				if (sel > -1) return false;
				if (n.isTextblock && n.content.size === 0) sel = pos + 1;
			});
			if (sel > -1) tr$1.setSelection(TextSelection.near(tr$1.doc.resolve(sel)));
			tr$1.scrollIntoView();
		}
		return true;
	}
	const nextType = $to.pos === $from.end() ? grandParent.contentMatchAt(0).defaultType : null;
	const newTypeAttributes = {
		...getSplittedAttributes(extensionAttributes, grandParent.type.name, grandParent.attrs),
		...overrideAttrs
	};
	const newNextTypeAttributes = {
		...getSplittedAttributes(extensionAttributes, $from.node().type.name, $from.node().attrs),
		...overrideAttrs
	};
	tr$1.delete($from.pos, $to.pos);
	const types = nextType ? [{
		type,
		attrs: newTypeAttributes
	}, {
		type: nextType,
		attrs: newNextTypeAttributes
	}] : [{
		type,
		attrs: newTypeAttributes
	}];
	if (!canSplit(tr$1.doc, $from.pos, 2)) return false;
	if (dispatch) {
		const { selection, storedMarks } = state;
		const { splittableMarks } = editor.extensionManager;
		const marks = storedMarks || selection.$to.parentOffset && selection.$from.marks();
		tr$1.split($from.pos, 2, types).scrollIntoView();
		if (!marks || !dispatch) return true;
		const filteredMarks = marks.filter((mark) => splittableMarks.includes(mark.type.name));
		tr$1.ensureMarks(filteredMarks);
	}
	return true;
};
var joinListBackwards = (tr$1, listType) => {
	const list = findParentNode((node) => node.type === listType)(tr$1.selection);
	if (!list) return true;
	const before = tr$1.doc.resolve(Math.max(0, list.pos - 1)).before(list.depth);
	if (before === void 0) return true;
	const nodeBefore = tr$1.doc.nodeAt(before);
	if (!(list.node.type === (nodeBefore == null ? void 0 : nodeBefore.type) && canJoin(tr$1.doc, list.pos))) return true;
	tr$1.join(list.pos);
	return true;
};
var joinListForwards = (tr$1, listType) => {
	const list = findParentNode((node) => node.type === listType)(tr$1.selection);
	if (!list) return true;
	const after = tr$1.doc.resolve(list.start).after(list.depth);
	if (after === void 0) return true;
	const nodeAfter = tr$1.doc.nodeAt(after);
	if (!(list.node.type === (nodeAfter == null ? void 0 : nodeAfter.type) && canJoin(tr$1.doc, after))) return true;
	tr$1.join(after);
	return true;
};
function createInnerSelectionForWholeDocList(tr$1) {
	const doc$2 = tr$1.doc;
	const list = doc$2.firstChild;
	if (!list) return null;
	const $start = doc$2.resolve(1);
	const $end = doc$2.resolve(list.nodeSize - 1);
	return TextSelection.between($start, $end);
}
var toggleList = (listTypeOrName, itemTypeOrName, keepMarks, attributes = {}) => ({ editor, tr: tr$1, state, dispatch, chain, commands, can }) => {
	const { extensions, splittableMarks } = editor.extensionManager;
	const listType = getNodeType(listTypeOrName, state.schema);
	const itemType = getNodeType(itemTypeOrName, state.schema);
	const { selection, storedMarks } = state;
	const { $from, $to } = selection;
	const range = $from.blockRange($to);
	const marks = storedMarks || selection.$to.parentOffset && selection.$from.marks();
	if (!range) return false;
	const parentList = findParentNode((node) => isList(node.type.name, extensions))(selection);
	const isAllSelection = selection.from === 0 && selection.to === state.doc.content.size;
	const topLevelNodes = state.doc.content.content;
	const soleTopLevelNode = topLevelNodes.length === 1 ? topLevelNodes[0] : null;
	const allSelectionList = isAllSelection && soleTopLevelNode && isList(soleTopLevelNode.type.name, extensions) ? {
		node: soleTopLevelNode,
		pos: 0,
		depth: 0
	} : null;
	const currentList = parentList != null ? parentList : allSelectionList;
	const isInsideExistingList = !!parentList && range.depth >= 1 && range.depth - parentList.depth <= 1;
	const hasWholeDocSelectedList = !!allSelectionList;
	if ((isInsideExistingList || hasWholeDocSelectedList) && currentList) {
		if (currentList.node.type === listType) {
			if (isAllSelection && hasWholeDocSelectedList) return chain().command(({ tr: trx, dispatch: disp }) => {
				const nextSelection = createInnerSelectionForWholeDocList(trx);
				if (!nextSelection) return false;
				trx.setSelection(nextSelection);
				if (disp) disp(trx);
				return true;
			}).liftListItem(itemType).run();
			return commands.liftListItem(itemType);
		}
		if (isList(currentList.node.type.name, extensions) && listType.validContent(currentList.node.content)) return chain().command(() => {
			tr$1.setNodeMarkup(currentList.pos, listType);
			return true;
		}).command(() => joinListBackwards(tr$1, listType)).command(() => joinListForwards(tr$1, listType)).run();
	}
	if (!keepMarks || !marks || !dispatch) return chain().command(() => {
		if (can().wrapInList(listType, attributes)) return true;
		return commands.clearNodes();
	}).wrapInList(listType, attributes).command(() => joinListBackwards(tr$1, listType)).command(() => joinListForwards(tr$1, listType)).run();
	return chain().command(() => {
		const canWrapInList = can().wrapInList(listType, attributes);
		const filteredMarks = marks.filter((mark) => splittableMarks.includes(mark.type.name));
		tr$1.ensureMarks(filteredMarks);
		if (canWrapInList) return true;
		return commands.clearNodes();
	}).wrapInList(listType, attributes).command(() => joinListBackwards(tr$1, listType)).command(() => joinListForwards(tr$1, listType)).run();
};
var toggleMark = (typeOrName, attributes = {}, options = {}) => ({ state, commands }) => {
	const { extendEmptyMarkRange = false } = options;
	const type = getMarkType(typeOrName, state.schema);
	if (isMarkActive(state, type, attributes)) return commands.unsetMark(type, { extendEmptyMarkRange });
	return commands.setMark(type, attributes);
};
var toggleNode = (typeOrName, toggleTypeOrName, attributes = {}) => ({ state, commands }) => {
	const type = getNodeType(typeOrName, state.schema);
	const toggleType = getNodeType(toggleTypeOrName, state.schema);
	const isActive2 = isNodeActive(state, type, attributes);
	let attributesToCopy;
	if (state.selection.$anchor.sameParent(state.selection.$head)) attributesToCopy = state.selection.$anchor.parent.attrs;
	if (isActive2) return commands.setNode(toggleType, attributesToCopy);
	return commands.setNode(type, {
		...attributesToCopy,
		...attributes
	});
};
var toggleWrap = (typeOrName, attributes = {}) => ({ state, commands }) => {
	const type = getNodeType(typeOrName, state.schema);
	if (isNodeActive(state, type, attributes)) return commands.lift(type);
	return commands.wrapIn(type, attributes);
};
var undoInputRule = () => ({ state, dispatch }) => {
	const plugins = state.plugins;
	for (let i$1 = 0; i$1 < plugins.length; i$1 += 1) {
		const plugin = plugins[i$1];
		let undoable;
		if (plugin.spec.isInputRules && (undoable = plugin.getState(state))) {
			if (dispatch) {
				const tr$1 = state.tr;
				const toUndo = undoable.transform;
				for (let j$1 = toUndo.steps.length - 1; j$1 >= 0; j$1 -= 1) tr$1.step(toUndo.steps[j$1].invert(toUndo.docs[j$1]));
				if (undoable.text) {
					const marks = tr$1.doc.resolve(undoable.from).marks();
					tr$1.replaceWith(undoable.from, undoable.to, state.schema.text(undoable.text, marks));
				} else tr$1.delete(undoable.from, undoable.to);
			}
			return true;
		}
	}
	return false;
};
var unsetAllMarks = () => ({ tr: tr$1, dispatch }) => {
	const { selection } = tr$1;
	const { empty: empty$1, ranges } = selection;
	if (empty$1) return true;
	if (dispatch) ranges.forEach((range) => {
		tr$1.removeMark(range.$from.pos, range.$to.pos);
	});
	return true;
};
var unsetMark = (typeOrName, options = {}) => ({ tr: tr$1, state, dispatch }) => {
	var _a;
	const { extendEmptyMarkRange = false } = options;
	const { selection } = tr$1;
	const type = getMarkType(typeOrName, state.schema);
	const { $from, empty: empty$1, ranges } = selection;
	if (!dispatch) return true;
	if (empty$1 && extendEmptyMarkRange) {
		let { from, to } = selection;
		const range = getMarkRange($from, type, (_a = $from.marks().find((mark) => mark.type === type)) == null ? void 0 : _a.attrs);
		if (range) {
			from = range.from;
			to = range.to;
		}
		tr$1.removeMark(from, to, type);
	} else ranges.forEach((range) => {
		tr$1.removeMark(range.$from.pos, range.$to.pos, type);
	});
	tr$1.removeStoredMark(type);
	return true;
};
var unsetTextDirection = (position) => ({ tr: tr$1, state, dispatch }) => {
	const { selection } = state;
	let from;
	let to;
	if (typeof position === "number") {
		from = position;
		to = position;
	} else if (position && "from" in position && "to" in position) {
		from = position.from;
		to = position.to;
	} else {
		from = selection.from;
		to = selection.to;
	}
	if (dispatch) tr$1.doc.nodesBetween(from, to, (node, pos) => {
		if (node.isText) return;
		const newAttrs = { ...node.attrs };
		delete newAttrs.dir;
		tr$1.setNodeMarkup(pos, void 0, newAttrs);
	});
	return true;
};
var updateAttributes = (typeOrName, attributes = {}) => ({ tr: tr$1, state, dispatch }) => {
	let nodeType = null;
	let markType = null;
	const schemaType = getSchemaTypeNameByName(typeof typeOrName === "string" ? typeOrName : typeOrName.name, state.schema);
	if (!schemaType) return false;
	if (schemaType === "node") nodeType = getNodeType(typeOrName, state.schema);
	if (schemaType === "mark") markType = getMarkType(typeOrName, state.schema);
	let canUpdate = false;
	tr$1.selection.ranges.forEach((range) => {
		const from = range.$from.pos;
		const to = range.$to.pos;
		let lastPos;
		let lastNode;
		let trimmedFrom;
		let trimmedTo;
		if (tr$1.selection.empty) state.doc.nodesBetween(from, to, (node, pos) => {
			if (nodeType && nodeType === node.type) {
				canUpdate = true;
				trimmedFrom = Math.max(pos, from);
				trimmedTo = Math.min(pos + node.nodeSize, to);
				lastPos = pos;
				lastNode = node;
			}
		});
		else state.doc.nodesBetween(from, to, (node, pos) => {
			if (pos < from && nodeType && nodeType === node.type) {
				canUpdate = true;
				trimmedFrom = Math.max(pos, from);
				trimmedTo = Math.min(pos + node.nodeSize, to);
				lastPos = pos;
				lastNode = node;
			}
			if (pos >= from && pos <= to) {
				if (nodeType && nodeType === node.type) {
					canUpdate = true;
					if (dispatch) tr$1.setNodeMarkup(pos, void 0, {
						...node.attrs,
						...attributes
					});
				}
				if (markType && node.marks.length) node.marks.forEach((mark) => {
					if (markType === mark.type) {
						canUpdate = true;
						if (dispatch) {
							const trimmedFrom2 = Math.max(pos, from);
							const trimmedTo2 = Math.min(pos + node.nodeSize, to);
							tr$1.addMark(trimmedFrom2, trimmedTo2, markType.create({
								...mark.attrs,
								...attributes
							}));
						}
					}
				});
			}
		});
		if (lastNode) {
			if (lastPos !== void 0 && dispatch) tr$1.setNodeMarkup(lastPos, void 0, {
				...lastNode.attrs,
				...attributes
			});
			if (markType && lastNode.marks.length) lastNode.marks.forEach((mark) => {
				if (markType === mark.type && dispatch) tr$1.addMark(trimmedFrom, trimmedTo, markType.create({
					...mark.attrs,
					...attributes
				}));
			});
		}
	});
	return canUpdate;
};
var wrapIn$1 = (typeOrName, attributes = {}) => ({ state, dispatch }) => {
	return wrapIn(getNodeType(typeOrName, state.schema), attributes)(state, dispatch);
};
var wrapInList$1 = (typeOrName, attributes = {}) => ({ state, dispatch }) => {
	return wrapInList(getNodeType(typeOrName, state.schema), attributes)(state, dispatch);
};
var EventEmitter = class {
	constructor() {
		this.callbacks = {};
	}
	on(event, fn) {
		if (!this.callbacks[event]) this.callbacks[event] = [];
		this.callbacks[event].push(fn);
		return this;
	}
	emit(event, ...args) {
		const callbacks = this.callbacks[event];
		if (callbacks) callbacks.forEach((callback) => callback.apply(this, args));
		return this;
	}
	off(event, fn) {
		const callbacks = this.callbacks[event];
		if (callbacks) if (fn) this.callbacks[event] = callbacks.filter((callback) => callback !== fn);
		else delete this.callbacks[event];
		return this;
	}
	once(event, fn) {
		const onceFn = (...args) => {
			this.off(event, onceFn);
			fn.apply(this, args);
		};
		return this.on(event, onceFn);
	}
	removeAllListeners() {
		this.callbacks = {};
	}
};
var InputRule = class {
	constructor(config) {
		var _a;
		this.find = config.find;
		this.handler = config.handler;
		this.undoable = (_a = config.undoable) != null ? _a : true;
	}
};
var inputRuleMatcherHandler = (text, find$1) => {
	if (isRegExp(find$1)) return find$1.exec(text);
	const inputRuleMatch = find$1(text);
	if (!inputRuleMatch) return null;
	const result = [inputRuleMatch.text];
	result.index = inputRuleMatch.index;
	result.input = text;
	result.data = inputRuleMatch.data;
	if (inputRuleMatch.replaceWith) {
		if (!inputRuleMatch.text.includes(inputRuleMatch.replaceWith)) console.warn("[tiptap warn]: \"inputRuleMatch.replaceWith\" must be part of \"inputRuleMatch.text\".");
		result.push(inputRuleMatch.replaceWith);
	}
	return result;
};
function run$2(config) {
	var _a;
	const { editor, from, to, text, rules, plugin } = config;
	const { view } = editor;
	if (view.composing) return false;
	const $from = view.state.doc.resolve(from);
	if ($from.parent.type.spec.code || !!((_a = $from.nodeBefore || $from.nodeAfter) == null ? void 0 : _a.marks.find((mark) => mark.type.spec.code))) return false;
	let matched = false;
	const textBefore = getTextContentFromNodes($from) + text;
	rules.forEach((rule) => {
		if (matched) return;
		const match = inputRuleMatcherHandler(textBefore, rule.find);
		if (!match) return;
		const tr$1 = view.state.tr;
		const state = createChainableState({
			state: view.state,
			transaction: tr$1
		});
		const range = {
			from: from - (match[0].length - text.length),
			to
		};
		const { commands, chain, can } = new CommandManager({
			editor,
			state
		});
		if (rule.handler({
			state,
			range,
			match,
			commands,
			chain,
			can
		}) === null || !tr$1.steps.length) return;
		if (rule.undoable) tr$1.setMeta(plugin, {
			transform: tr$1,
			from,
			to,
			text
		});
		view.dispatch(tr$1);
		matched = true;
	});
	return matched;
}
function inputRulesPlugin(props) {
	const { editor, rules } = props;
	const plugin = new Plugin({
		state: {
			init() {
				return null;
			},
			apply(tr$1, prev, state) {
				const stored = tr$1.getMeta(plugin);
				if (stored) return stored;
				const simulatedInputMeta = tr$1.getMeta("applyInputRules");
				if (!!simulatedInputMeta) setTimeout(() => {
					let { text } = simulatedInputMeta;
					if (typeof text === "string") text = text;
					else text = getHTMLFromFragment(Fragment.from(text), state.schema);
					const { from } = simulatedInputMeta;
					run$2({
						editor,
						from,
						to: from + text.length,
						text,
						rules,
						plugin
					});
				});
				return tr$1.selectionSet || tr$1.docChanged ? null : prev;
			}
		},
		props: {
			handleTextInput(view, from, to, text) {
				return run$2({
					editor,
					from,
					to,
					text,
					rules,
					plugin
				});
			},
			handleDOMEvents: { compositionend: (view) => {
				setTimeout(() => {
					const { $cursor } = view.state.selection;
					if ($cursor) run$2({
						editor,
						from: $cursor.pos,
						to: $cursor.pos,
						text: "",
						rules,
						plugin
					});
				});
				return false;
			} },
			handleKeyDown(view, event) {
				if (event.key !== "Enter") return false;
				const { $cursor } = view.state.selection;
				if ($cursor) return run$2({
					editor,
					from: $cursor.pos,
					to: $cursor.pos,
					text: "\n",
					rules,
					plugin
				});
				return false;
			}
		},
		isInputRules: true
	});
	return plugin;
}
function getType(value) {
	return Object.prototype.toString.call(value).slice(8, -1);
}
function isPlainObject(value) {
	if (getType(value) !== "Object") return false;
	return value.constructor === Object && Object.getPrototypeOf(value) === Object.prototype;
}
function mergeDeep(target, source) {
	const output = { ...target };
	if (isPlainObject(target) && isPlainObject(source)) Object.keys(source).forEach((key) => {
		if (isPlainObject(source[key]) && isPlainObject(target[key])) output[key] = mergeDeep(target[key], source[key]);
		else output[key] = source[key];
	});
	return output;
}
var Extendable = class {
	constructor(config = {}) {
		this.type = "extendable";
		this.parent = null;
		this.child = null;
		this.name = "";
		this.config = { name: this.name };
		this.config = {
			...this.config,
			...config
		};
		this.name = this.config.name;
	}
	get options() {
		return { ...callOrReturn(getExtensionField(this, "addOptions", { name: this.name })) || {} };
	}
	get storage() {
		return { ...callOrReturn(getExtensionField(this, "addStorage", {
			name: this.name,
			options: this.options
		})) || {} };
	}
	configure(options = {}) {
		const extension = this.extend({
			...this.config,
			addOptions: () => {
				return mergeDeep(this.options, options);
			}
		});
		extension.name = this.name;
		extension.parent = this.parent;
		return extension;
	}
	extend(extendedConfig = {}) {
		const extension = new this.constructor({
			...this.config,
			...extendedConfig
		});
		extension.parent = this;
		this.child = extension;
		extension.name = "name" in extendedConfig ? extendedConfig.name : extension.parent.name;
		return extension;
	}
};
var Mark = class _Mark extends Extendable {
	constructor() {
		super(...arguments);
		this.type = "mark";
	}
	static create(config = {}) {
		return new _Mark(typeof config === "function" ? config() : config);
	}
	static handleExit({ editor, mark }) {
		const { tr: tr$1 } = editor.state;
		const currentPos = editor.state.selection.$from;
		if (currentPos.pos === currentPos.end()) {
			const currentMarks = currentPos.marks();
			if (!!!currentMarks.find((m$1) => (m$1 == null ? void 0 : m$1.type.name) === mark.name)) return false;
			const removeMark$1 = currentMarks.find((m$1) => (m$1 == null ? void 0 : m$1.type.name) === mark.name);
			if (removeMark$1) tr$1.removeStoredMark(removeMark$1);
			tr$1.insertText(" ", currentPos.pos);
			editor.view.dispatch(tr$1);
			return true;
		}
		return false;
	}
	configure(options) {
		return super.configure(options);
	}
	extend(extendedConfig) {
		const resolvedConfig = typeof extendedConfig === "function" ? extendedConfig() : extendedConfig;
		return super.extend(resolvedConfig);
	}
};
function isNumber(value) {
	return typeof value === "number";
}
var PasteRule = class {
	constructor(config) {
		this.find = config.find;
		this.handler = config.handler;
	}
};
var pasteRuleMatcherHandler = (text, find$1, event) => {
	if (isRegExp(find$1)) return [...text.matchAll(find$1)];
	const matches$1 = find$1(text, event);
	if (!matches$1) return [];
	return matches$1.map((pasteRuleMatch) => {
		const result = [pasteRuleMatch.text];
		result.index = pasteRuleMatch.index;
		result.input = text;
		result.data = pasteRuleMatch.data;
		if (pasteRuleMatch.replaceWith) {
			if (!pasteRuleMatch.text.includes(pasteRuleMatch.replaceWith)) console.warn("[tiptap warn]: \"pasteRuleMatch.replaceWith\" must be part of \"pasteRuleMatch.text\".");
			result.push(pasteRuleMatch.replaceWith);
		}
		return result;
	});
};
function run2(config) {
	const { editor, state, from, to, rule, pasteEvent, dropEvent } = config;
	const { commands, chain, can } = new CommandManager({
		editor,
		state
	});
	const handlers$1 = [];
	state.doc.nodesBetween(from, to, (node, pos) => {
		var _a, _b, _c, _d, _e$1;
		if (((_b = (_a = node.type) == null ? void 0 : _a.spec) == null ? void 0 : _b.code) || !(node.isText || node.isTextblock || node.isInline)) return;
		const contentSize = (_e$1 = (_d = (_c = node.content) == null ? void 0 : _c.size) != null ? _d : node.nodeSize) != null ? _e$1 : 0;
		const resolvedFrom = Math.max(from, pos);
		const resolvedTo = Math.min(to, pos + contentSize);
		if (resolvedFrom >= resolvedTo) return;
		pasteRuleMatcherHandler(node.isText ? node.text || "" : node.textBetween(resolvedFrom - pos, resolvedTo - pos, void 0, "￼"), rule.find, pasteEvent).forEach((match) => {
			if (match.index === void 0) return;
			const start = resolvedFrom + match.index + 1;
			const end = start + match[0].length;
			const range = {
				from: state.tr.mapping.map(start),
				to: state.tr.mapping.map(end)
			};
			const handler = rule.handler({
				state,
				range,
				match,
				commands,
				chain,
				can,
				pasteEvent,
				dropEvent
			});
			handlers$1.push(handler);
		});
	});
	return handlers$1.every((handler) => handler !== null);
}
var tiptapDragFromOtherEditor = null;
var createClipboardPasteEvent = (text) => {
	var _a;
	const event = new ClipboardEvent("paste", { clipboardData: new DataTransfer() });
	(_a = event.clipboardData) == null || _a.setData("text/html", text);
	return event;
};
function pasteRulesPlugin(props) {
	const { editor, rules } = props;
	let dragSourceElement = null;
	let isPastedFromProseMirror = false;
	let isDroppedFromProseMirror = false;
	let pasteEvent = typeof ClipboardEvent !== "undefined" ? new ClipboardEvent("paste") : null;
	let dropEvent;
	try {
		dropEvent = typeof DragEvent !== "undefined" ? new DragEvent("drop") : null;
	} catch {
		dropEvent = null;
	}
	const processEvent = ({ state, from, to, rule, pasteEvt }) => {
		const tr$1 = state.tr;
		if (!run2({
			editor,
			state: createChainableState({
				state,
				transaction: tr$1
			}),
			from: Math.max(from - 1, 0),
			to: to.b - 1,
			rule,
			pasteEvent: pasteEvt,
			dropEvent
		}) || !tr$1.steps.length) return;
		try {
			dropEvent = typeof DragEvent !== "undefined" ? new DragEvent("drop") : null;
		} catch {
			dropEvent = null;
		}
		pasteEvent = typeof ClipboardEvent !== "undefined" ? new ClipboardEvent("paste") : null;
		return tr$1;
	};
	return rules.map((rule) => {
		return new Plugin({
			view(view) {
				const handleDragstart = (event) => {
					var _a;
					dragSourceElement = ((_a = view.dom.parentElement) == null ? void 0 : _a.contains(event.target)) ? view.dom.parentElement : null;
					if (dragSourceElement) tiptapDragFromOtherEditor = editor;
				};
				const handleDragend = () => {
					if (tiptapDragFromOtherEditor) tiptapDragFromOtherEditor = null;
				};
				window.addEventListener("dragstart", handleDragstart);
				window.addEventListener("dragend", handleDragend);
				return { destroy() {
					window.removeEventListener("dragstart", handleDragstart);
					window.removeEventListener("dragend", handleDragend);
				} };
			},
			props: { handleDOMEvents: {
				drop: (view, event) => {
					isDroppedFromProseMirror = dragSourceElement === view.dom.parentElement;
					dropEvent = event;
					if (!isDroppedFromProseMirror) {
						const dragFromOtherEditor = tiptapDragFromOtherEditor;
						if (dragFromOtherEditor == null ? void 0 : dragFromOtherEditor.isEditable) setTimeout(() => {
							const selection = dragFromOtherEditor.state.selection;
							if (selection) dragFromOtherEditor.commands.deleteRange({
								from: selection.from,
								to: selection.to
							});
						}, 10);
					}
					return false;
				},
				paste: (_view, event) => {
					var _a;
					const html = (_a = event.clipboardData) == null ? void 0 : _a.getData("text/html");
					pasteEvent = event;
					isPastedFromProseMirror = !!(html == null ? void 0 : html.includes("data-pm-slice"));
					return false;
				}
			} },
			appendTransaction: (transactions, oldState, state) => {
				const transaction = transactions[0];
				const isPaste = transaction.getMeta("uiEvent") === "paste" && !isPastedFromProseMirror;
				const isDrop = transaction.getMeta("uiEvent") === "drop" && !isDroppedFromProseMirror;
				const simulatedPasteMeta = transaction.getMeta("applyPasteRules");
				const isSimulatedPaste = !!simulatedPasteMeta;
				if (!isPaste && !isDrop && !isSimulatedPaste) return;
				if (isSimulatedPaste) {
					let { text } = simulatedPasteMeta;
					if (typeof text === "string") text = text;
					else text = getHTMLFromFragment(Fragment.from(text), state.schema);
					const { from: from2 } = simulatedPasteMeta;
					const to2 = from2 + text.length;
					const pasteEvt = createClipboardPasteEvent(text);
					return processEvent({
						rule,
						state,
						from: from2,
						to: { b: to2 },
						pasteEvt
					});
				}
				const from = oldState.doc.content.findDiffStart(state.doc.content);
				const to = oldState.doc.content.findDiffEnd(state.doc.content);
				if (!isNumber(from) || !to || from === to.b) return;
				return processEvent({
					rule,
					state,
					from,
					to,
					pasteEvt: pasteEvent
				});
			}
		});
	});
}
var ExtensionManager = class {
	constructor(extensions, editor) {
		this.splittableMarks = [];
		this.editor = editor;
		this.baseExtensions = extensions;
		this.extensions = resolveExtensions(extensions);
		this.schema = getSchemaByResolvedExtensions(this.extensions, editor);
		this.setupExtensions();
	}
	get commands() {
		return this.extensions.reduce((commands, extension) => {
			const addCommands = getExtensionField(extension, "addCommands", {
				name: extension.name,
				options: extension.options,
				storage: this.editor.extensionStorage[extension.name],
				editor: this.editor,
				type: getSchemaTypeByName(extension.name, this.schema)
			});
			if (!addCommands) return commands;
			return {
				...commands,
				...addCommands()
			};
		}, {});
	}
	get plugins() {
		const { editor } = this;
		return sortExtensions([...this.extensions].reverse()).flatMap((extension) => {
			const context = {
				name: extension.name,
				options: extension.options,
				storage: this.editor.extensionStorage[extension.name],
				editor,
				type: getSchemaTypeByName(extension.name, this.schema)
			};
			const plugins = [];
			const addKeyboardShortcuts = getExtensionField(extension, "addKeyboardShortcuts", context);
			let defaultBindings = {};
			if (extension.type === "mark" && getExtensionField(extension, "exitable", context)) defaultBindings.ArrowRight = () => Mark.handleExit({
				editor,
				mark: extension
			});
			if (addKeyboardShortcuts) {
				const bindings = Object.fromEntries(Object.entries(addKeyboardShortcuts()).map(([shortcut, method]) => {
					return [shortcut, () => method({ editor })];
				}));
				defaultBindings = {
					...defaultBindings,
					...bindings
				};
			}
			const keyMapPlugin = keymap(defaultBindings);
			plugins.push(keyMapPlugin);
			const addInputRules = getExtensionField(extension, "addInputRules", context);
			if (isExtensionRulesEnabled(extension, editor.options.enableInputRules) && addInputRules) {
				const rules = addInputRules();
				if (rules && rules.length) {
					const inputResult = inputRulesPlugin({
						editor,
						rules
					});
					const inputPlugins = Array.isArray(inputResult) ? inputResult : [inputResult];
					plugins.push(...inputPlugins);
				}
			}
			const addPasteRules = getExtensionField(extension, "addPasteRules", context);
			if (isExtensionRulesEnabled(extension, editor.options.enablePasteRules) && addPasteRules) {
				const rules = addPasteRules();
				if (rules && rules.length) {
					const pasteRules = pasteRulesPlugin({
						editor,
						rules
					});
					plugins.push(...pasteRules);
				}
			}
			const addProseMirrorPlugins = getExtensionField(extension, "addProseMirrorPlugins", context);
			if (addProseMirrorPlugins) {
				const proseMirrorPlugins = addProseMirrorPlugins();
				plugins.push(...proseMirrorPlugins);
			}
			return plugins;
		});
	}
	get attributes() {
		return getAttributesFromExtensions(this.extensions);
	}
	get nodeViews() {
		const { editor } = this;
		const { nodeExtensions } = splitExtensions(this.extensions);
		return Object.fromEntries(nodeExtensions.filter((extension) => !!getExtensionField(extension, "addNodeView")).map((extension) => {
			const extensionAttributes = this.attributes.filter((attribute) => attribute.type === extension.name);
			const addNodeView = getExtensionField(extension, "addNodeView", {
				name: extension.name,
				options: extension.options,
				storage: this.editor.extensionStorage[extension.name],
				editor,
				type: getNodeType(extension.name, this.schema)
			});
			if (!addNodeView) return [];
			const nodeViewResult = addNodeView();
			if (!nodeViewResult) return [];
			const nodeview = (node, view, getPos, decorations, innerDecorations) => {
				return nodeViewResult({
					node,
					view,
					getPos,
					decorations,
					innerDecorations,
					editor,
					extension,
					HTMLAttributes: getRenderedAttributes(node, extensionAttributes)
				});
			};
			return [extension.name, nodeview];
		}));
	}
	dispatchTransaction(baseDispatch) {
		const { editor } = this;
		return sortExtensions([...this.extensions].reverse()).reduceRight((next, extension) => {
			const context = {
				name: extension.name,
				options: extension.options,
				storage: this.editor.extensionStorage[extension.name],
				editor,
				type: getSchemaTypeByName(extension.name, this.schema)
			};
			const dispatchTransaction = getExtensionField(extension, "dispatchTransaction", context);
			if (!dispatchTransaction) return next;
			return (transaction) => {
				dispatchTransaction.call(context, {
					transaction,
					next
				});
			};
		}, baseDispatch);
	}
	transformPastedHTML(baseTransform) {
		const { editor } = this;
		return sortExtensions([...this.extensions]).reduce((transform, extension) => {
			const context = {
				name: extension.name,
				options: extension.options,
				storage: this.editor.extensionStorage[extension.name],
				editor,
				type: getSchemaTypeByName(extension.name, this.schema)
			};
			const extensionTransform = getExtensionField(extension, "transformPastedHTML", context);
			if (!extensionTransform) return transform;
			return (html, view) => {
				const transformedHtml = transform(html, view);
				return extensionTransform.call(context, transformedHtml);
			};
		}, baseTransform || ((html) => html));
	}
	get markViews() {
		const { editor } = this;
		const { markExtensions } = splitExtensions(this.extensions);
		return Object.fromEntries(markExtensions.filter((extension) => !!getExtensionField(extension, "addMarkView")).map((extension) => {
			const extensionAttributes = this.attributes.filter((attribute) => attribute.type === extension.name);
			const addMarkView = getExtensionField(extension, "addMarkView", {
				name: extension.name,
				options: extension.options,
				storage: this.editor.extensionStorage[extension.name],
				editor,
				type: getMarkType(extension.name, this.schema)
			});
			if (!addMarkView) return [];
			const markView = (mark, view, inline) => {
				const HTMLAttributes = getRenderedAttributes(mark, extensionAttributes);
				return addMarkView()({
					mark,
					view,
					inline,
					editor,
					extension,
					HTMLAttributes,
					updateAttributes: (attrs) => {
						updateMarkViewAttributes(mark, editor, attrs);
					}
				});
			};
			return [extension.name, markView];
		}));
	}
	setupExtensions() {
		const extensions = this.extensions;
		this.editor.extensionStorage = Object.fromEntries(extensions.map((extension) => [extension.name, extension.storage]));
		extensions.forEach((extension) => {
			var _a;
			const context = {
				name: extension.name,
				options: extension.options,
				storage: this.editor.extensionStorage[extension.name],
				editor: this.editor,
				type: getSchemaTypeByName(extension.name, this.schema)
			};
			if (extension.type === "mark") {
				if ((_a = callOrReturn(getExtensionField(extension, "keepOnSplit", context))) != null ? _a : true) this.splittableMarks.push(extension.name);
			}
			const onBeforeCreate = getExtensionField(extension, "onBeforeCreate", context);
			const onCreate = getExtensionField(extension, "onCreate", context);
			const onUpdate = getExtensionField(extension, "onUpdate", context);
			const onSelectionUpdate = getExtensionField(extension, "onSelectionUpdate", context);
			const onTransaction = getExtensionField(extension, "onTransaction", context);
			const onFocus = getExtensionField(extension, "onFocus", context);
			const onBlur = getExtensionField(extension, "onBlur", context);
			const onDestroy = getExtensionField(extension, "onDestroy", context);
			if (onBeforeCreate) this.editor.on("beforeCreate", onBeforeCreate);
			if (onCreate) this.editor.on("create", onCreate);
			if (onUpdate) this.editor.on("update", onUpdate);
			if (onSelectionUpdate) this.editor.on("selectionUpdate", onSelectionUpdate);
			if (onTransaction) this.editor.on("transaction", onTransaction);
			if (onFocus) this.editor.on("focus", onFocus);
			if (onBlur) this.editor.on("blur", onBlur);
			if (onDestroy) this.editor.on("destroy", onDestroy);
		});
	}
};
ExtensionManager.resolve = resolveExtensions;
ExtensionManager.sort = sortExtensions;
ExtensionManager.flatten = flattenExtensions;
__export$1({}, {
	ClipboardTextSerializer: () => ClipboardTextSerializer,
	Commands: () => Commands,
	Delete: () => Delete,
	Drop: () => Drop,
	Editable: () => Editable,
	FocusEvents: () => FocusEvents,
	Keymap: () => Keymap,
	Paste: () => Paste,
	Tabindex: () => Tabindex,
	TextDirection: () => TextDirection,
	focusEventsPluginKey: () => focusEventsPluginKey
});
var Extension = class _Extension extends Extendable {
	constructor() {
		super(...arguments);
		this.type = "extension";
	}
	static create(config = {}) {
		return new _Extension(typeof config === "function" ? config() : config);
	}
	configure(options) {
		return super.configure(options);
	}
	extend(extendedConfig) {
		const resolvedConfig = typeof extendedConfig === "function" ? extendedConfig() : extendedConfig;
		return super.extend(resolvedConfig);
	}
};
var ClipboardTextSerializer = Extension.create({
	name: "clipboardTextSerializer",
	addOptions() {
		return { blockSeparator: void 0 };
	},
	addProseMirrorPlugins() {
		return [new Plugin({
			key: new PluginKey("clipboardTextSerializer"),
			props: { clipboardTextSerializer: () => {
				const { editor } = this;
				const { state, schema } = editor;
				const { doc: doc$2, selection } = state;
				const { ranges } = selection;
				const from = Math.min(...ranges.map((range2) => range2.$from.pos));
				const to = Math.max(...ranges.map((range2) => range2.$to.pos));
				const textSerializers = getTextSerializersFromSchema(schema);
				return getTextBetween(doc$2, {
					from,
					to
				}, {
					...this.options.blockSeparator !== void 0 ? { blockSeparator: this.options.blockSeparator } : {},
					textSerializers
				});
			} }
		})];
	}
});
var Commands = Extension.create({
	name: "commands",
	addCommands() {
		return { ...commands_exports };
	}
});
var Delete = Extension.create({
	name: "delete",
	onUpdate({ transaction, appendedTransactions }) {
		var _a, _b, _c;
		const callback = () => {
			var _a2, _b2, _c2, _d;
			if ((_d = (_c2 = (_b2 = (_a2 = this.editor.options.coreExtensionOptions) == null ? void 0 : _a2.delete) == null ? void 0 : _b2.filterTransaction) == null ? void 0 : _c2.call(_b2, transaction)) != null ? _d : transaction.getMeta("y-sync$")) return;
			const nextTransaction = combineTransactionSteps(transaction.before, [transaction, ...appendedTransactions]);
			getChangedRanges(nextTransaction).forEach((change) => {
				if (nextTransaction.mapping.mapResult(change.oldRange.from).deletedAfter && nextTransaction.mapping.mapResult(change.oldRange.to).deletedBefore) nextTransaction.before.nodesBetween(change.oldRange.from, change.oldRange.to, (node, from) => {
					const to = from + node.nodeSize - 2;
					const isFullyWithinRange = change.oldRange.from <= from && to <= change.oldRange.to;
					this.editor.emit("delete", {
						type: "node",
						node,
						from,
						to,
						newFrom: nextTransaction.mapping.map(from),
						newTo: nextTransaction.mapping.map(to),
						deletedRange: change.oldRange,
						newRange: change.newRange,
						partial: !isFullyWithinRange,
						editor: this.editor,
						transaction,
						combinedTransform: nextTransaction
					});
				});
			});
			const mapping = nextTransaction.mapping;
			nextTransaction.steps.forEach((step$1, index) => {
				var _a3, _b3;
				if (step$1 instanceof RemoveMarkStep) {
					const newStart = mapping.slice(index).map(step$1.from, -1);
					const newEnd = mapping.slice(index).map(step$1.to);
					const oldStart = mapping.invert().map(newStart, -1);
					const oldEnd = mapping.invert().map(newEnd);
					const foundBeforeMark = newStart > 0 ? (_a3 = nextTransaction.doc.nodeAt(newStart - 1)) == null ? void 0 : _a3.marks.some((mark) => mark.eq(step$1.mark)) : false;
					const foundAfterMark = (_b3 = nextTransaction.doc.nodeAt(newEnd)) == null ? void 0 : _b3.marks.some((mark) => mark.eq(step$1.mark));
					this.editor.emit("delete", {
						type: "mark",
						mark: step$1.mark,
						from: step$1.from,
						to: step$1.to,
						deletedRange: {
							from: oldStart,
							to: oldEnd
						},
						newRange: {
							from: newStart,
							to: newEnd
						},
						partial: Boolean(foundAfterMark || foundBeforeMark),
						editor: this.editor,
						transaction,
						combinedTransform: nextTransaction
					});
				}
			});
		};
		if ((_c = (_b = (_a = this.editor.options.coreExtensionOptions) == null ? void 0 : _a.delete) == null ? void 0 : _b.async) != null ? _c : true) setTimeout(callback, 0);
		else callback();
	}
});
var Drop = Extension.create({
	name: "drop",
	addProseMirrorPlugins() {
		return [new Plugin({
			key: new PluginKey("tiptapDrop"),
			props: { handleDrop: (_$1, e, slice, moved) => {
				this.editor.emit("drop", {
					editor: this.editor,
					event: e,
					slice,
					moved
				});
			} }
		})];
	}
});
var Editable = Extension.create({
	name: "editable",
	addProseMirrorPlugins() {
		return [new Plugin({
			key: new PluginKey("editable"),
			props: { editable: () => this.editor.options.editable }
		})];
	}
});
var focusEventsPluginKey = new PluginKey("focusEvents");
var FocusEvents = Extension.create({
	name: "focusEvents",
	addProseMirrorPlugins() {
		const { editor } = this;
		return [new Plugin({
			key: focusEventsPluginKey,
			props: { handleDOMEvents: {
				focus: (view, event) => {
					editor.isFocused = true;
					const transaction = editor.state.tr.setMeta("focus", { event }).setMeta("addToHistory", false);
					view.dispatch(transaction);
					return false;
				},
				blur: (view, event) => {
					editor.isFocused = false;
					const transaction = editor.state.tr.setMeta("blur", { event }).setMeta("addToHistory", false);
					view.dispatch(transaction);
					return false;
				}
			} }
		})];
	}
});
var Keymap = Extension.create({
	name: "keymap",
	addKeyboardShortcuts() {
		const handleBackspace$1 = () => this.editor.commands.first(({ commands }) => [
			() => commands.undoInputRule(),
			() => commands.command(({ tr: tr$1 }) => {
				const { selection, doc: doc$2 } = tr$1;
				const { empty: empty$1, $anchor } = selection;
				const { pos, parent } = $anchor;
				const $parentPos = $anchor.parent.isTextblock && pos > 0 ? tr$1.doc.resolve(pos - 1) : $anchor;
				const parentIsIsolating = $parentPos.parent.type.spec.isolating;
				const parentPos = $anchor.pos - $anchor.parentOffset;
				const isAtStart = parentIsIsolating && $parentPos.parent.childCount === 1 ? parentPos === $anchor.pos : Selection.atStart(doc$2).from === pos;
				if (!empty$1 || !parent.type.isTextblock || parent.textContent.length || !isAtStart || isAtStart && $anchor.parent.type.name === "paragraph") return false;
				return commands.clearNodes();
			}),
			() => commands.deleteSelection(),
			() => commands.joinBackward(),
			() => commands.selectNodeBackward()
		]);
		const handleDelete$1 = () => this.editor.commands.first(({ commands }) => [
			() => commands.deleteSelection(),
			() => commands.deleteCurrentNode(),
			() => commands.joinForward(),
			() => commands.selectNodeForward()
		]);
		const handleEnter = () => this.editor.commands.first(({ commands }) => [
			() => commands.newlineInCode(),
			() => commands.createParagraphNear(),
			() => commands.liftEmptyBlock(),
			() => commands.splitBlock()
		]);
		const baseKeymap$1 = {
			Enter: handleEnter,
			"Mod-Enter": () => this.editor.commands.exitCode(),
			Backspace: handleBackspace$1,
			"Mod-Backspace": handleBackspace$1,
			"Shift-Backspace": handleBackspace$1,
			Delete: handleDelete$1,
			"Mod-Delete": handleDelete$1,
			"Mod-a": () => this.editor.commands.selectAll()
		};
		const pcKeymap = { ...baseKeymap$1 };
		const macKeymap = {
			...baseKeymap$1,
			"Ctrl-h": handleBackspace$1,
			"Alt-Backspace": handleBackspace$1,
			"Ctrl-d": handleDelete$1,
			"Ctrl-Alt-Backspace": handleDelete$1,
			"Alt-Delete": handleDelete$1,
			"Alt-d": handleDelete$1,
			"Ctrl-a": () => this.editor.commands.selectTextblockStart(),
			"Ctrl-e": () => this.editor.commands.selectTextblockEnd()
		};
		if (isiOS() || isMacOS()) return macKeymap;
		return pcKeymap;
	},
	addProseMirrorPlugins() {
		return [new Plugin({
			key: new PluginKey("clearDocument"),
			appendTransaction: (transactions, oldState, newState) => {
				if (transactions.some((tr2) => tr2.getMeta("composition"))) return;
				const docChanges = transactions.some((transaction) => transaction.docChanged) && !oldState.doc.eq(newState.doc);
				const ignoreTr = transactions.some((transaction) => transaction.getMeta("preventClearDocument"));
				if (!docChanges || ignoreTr) return;
				const { empty: empty$1, from, to } = oldState.selection;
				const allFrom = Selection.atStart(oldState.doc).from;
				const allEnd = Selection.atEnd(oldState.doc).to;
				if (empty$1 || !(from === allFrom && to === allEnd)) return;
				if (!isNodeEmpty(newState.doc)) return;
				const tr$1 = newState.tr;
				const state = createChainableState({
					state: newState,
					transaction: tr$1
				});
				const { commands } = new CommandManager({
					editor: this.editor,
					state
				});
				commands.clearNodes();
				if (!tr$1.steps.length) return;
				return tr$1;
			}
		})];
	}
});
var Paste = Extension.create({
	name: "paste",
	addProseMirrorPlugins() {
		return [new Plugin({
			key: new PluginKey("tiptapPaste"),
			props: { handlePaste: (_view, e, slice) => {
				this.editor.emit("paste", {
					editor: this.editor,
					event: e,
					slice
				});
			} }
		})];
	}
});
var Tabindex = Extension.create({
	name: "tabindex",
	addProseMirrorPlugins() {
		return [new Plugin({
			key: new PluginKey("tabindex"),
			props: { attributes: () => this.editor.isEditable ? { tabindex: "0" } : {} }
		})];
	}
});
var TextDirection = Extension.create({
	name: "textDirection",
	addOptions() {
		return { direction: void 0 };
	},
	addGlobalAttributes() {
		if (!this.options.direction) return [];
		const { nodeExtensions } = splitExtensions(this.extensions);
		return [{
			types: nodeExtensions.filter((extension) => extension.name !== "text").map((extension) => extension.name),
			attributes: { dir: {
				default: this.options.direction,
				parseHTML: (element) => {
					const dir = element.getAttribute("dir");
					if (dir && (dir === "ltr" || dir === "rtl" || dir === "auto")) return dir;
					return this.options.direction;
				},
				renderHTML: (attributes) => {
					if (!attributes.dir) return {};
					return { dir: attributes.dir };
				}
			} }
		}];
	},
	addProseMirrorPlugins() {
		return [new Plugin({
			key: new PluginKey("textDirection"),
			props: { attributes: () => {
				const direction = this.options.direction;
				if (!direction) return {};
				return { dir: direction };
			} }
		})];
	}
});
var NodePos = class _NodePos {
	constructor(pos, editor, isBlock = false, node = null) {
		this.currentNode = null;
		this.actualDepth = null;
		this.isBlock = isBlock;
		this.resolvedPos = pos;
		this.editor = editor;
		this.currentNode = node;
	}
	get name() {
		return this.node.type.name;
	}
	get node() {
		return this.currentNode || this.resolvedPos.node();
	}
	get element() {
		return this.editor.view.domAtPos(this.pos).node;
	}
	get depth() {
		var _a;
		return (_a = this.actualDepth) != null ? _a : this.resolvedPos.depth;
	}
	get pos() {
		return this.resolvedPos.pos;
	}
	get content() {
		return this.node.content;
	}
	set content(content) {
		let from = this.from;
		let to = this.to;
		if (this.isBlock) {
			if (this.content.size === 0) {
				console.error(`You can\u2019t set content on a block node. Tried to set content on ${this.name} at ${this.pos}`);
				return;
			}
			from = this.from + 1;
			to = this.to - 1;
		}
		this.editor.commands.insertContentAt({
			from,
			to
		}, content);
	}
	get attributes() {
		return this.node.attrs;
	}
	get textContent() {
		return this.node.textContent;
	}
	get size() {
		return this.node.nodeSize;
	}
	get from() {
		if (this.isBlock) return this.pos;
		return this.resolvedPos.start(this.resolvedPos.depth);
	}
	get range() {
		return {
			from: this.from,
			to: this.to
		};
	}
	get to() {
		if (this.isBlock) return this.pos + this.size;
		return this.resolvedPos.end(this.resolvedPos.depth) + (this.node.isText ? 0 : 1);
	}
	get parent() {
		if (this.depth === 0) return null;
		const parentPos = this.resolvedPos.start(this.resolvedPos.depth - 1);
		return new _NodePos(this.resolvedPos.doc.resolve(parentPos), this.editor);
	}
	get before() {
		let $pos = this.resolvedPos.doc.resolve(this.from - (this.isBlock ? 1 : 2));
		if ($pos.depth !== this.depth) $pos = this.resolvedPos.doc.resolve(this.from - 3);
		return new _NodePos($pos, this.editor);
	}
	get after() {
		let $pos = this.resolvedPos.doc.resolve(this.to + (this.isBlock ? 2 : 1));
		if ($pos.depth !== this.depth) $pos = this.resolvedPos.doc.resolve(this.to + 3);
		return new _NodePos($pos, this.editor);
	}
	get children() {
		const children = [];
		this.node.content.forEach((node, offset) => {
			const isBlock = node.isBlock && !node.isTextblock;
			const isNonTextAtom = node.isAtom && !node.isText;
			const isInline$1 = node.isInline;
			const targetPos = this.pos + offset + (isNonTextAtom ? 0 : 1);
			if (targetPos < 0 || targetPos > this.resolvedPos.doc.nodeSize - 2) return;
			const $pos = this.resolvedPos.doc.resolve(targetPos);
			if (!isBlock && !isInline$1 && $pos.depth <= this.depth) return;
			const childNodePos = new _NodePos($pos, this.editor, isBlock, isBlock || isInline$1 ? node : null);
			if (isBlock) childNodePos.actualDepth = this.depth + 1;
			children.push(childNodePos);
		});
		return children;
	}
	get firstChild() {
		return this.children[0] || null;
	}
	get lastChild() {
		const children = this.children;
		return children[children.length - 1] || null;
	}
	closest(selector, attributes = {}) {
		let node = null;
		let currentNode = this.parent;
		while (currentNode && !node) {
			if (currentNode.node.type.name === selector) if (Object.keys(attributes).length > 0) {
				const nodeAttributes = currentNode.node.attrs;
				const attrKeys = Object.keys(attributes);
				for (let index = 0; index < attrKeys.length; index += 1) {
					const key = attrKeys[index];
					if (nodeAttributes[key] !== attributes[key]) break;
				}
			} else node = currentNode;
			currentNode = currentNode.parent;
		}
		return node;
	}
	querySelector(selector, attributes = {}) {
		return this.querySelectorAll(selector, attributes, true)[0] || null;
	}
	querySelectorAll(selector, attributes = {}, firstItemOnly = false) {
		let nodes = [];
		if (!this.children || this.children.length === 0) return nodes;
		const attrKeys = Object.keys(attributes);
		this.children.forEach((childPos) => {
			if (firstItemOnly && nodes.length > 0) return;
			if (childPos.node.type.name === selector) {
				if (attrKeys.every((key) => attributes[key] === childPos.node.attrs[key])) nodes.push(childPos);
			}
			if (firstItemOnly && nodes.length > 0) return;
			nodes = nodes.concat(childPos.querySelectorAll(selector, attributes, firstItemOnly));
		});
		return nodes;
	}
	setAttribute(attributes) {
		const { tr: tr$1 } = this.editor.state;
		tr$1.setNodeMarkup(this.from, void 0, {
			...this.node.attrs,
			...attributes
		});
		this.editor.view.dispatch(tr$1);
	}
};
var style = `.ProseMirror {
  position: relative;
}

.ProseMirror {
  word-wrap: break-word;
  white-space: pre-wrap;
  white-space: break-spaces;
  -webkit-font-variant-ligatures: none;
  font-variant-ligatures: none;
  font-feature-settings: "liga" 0; /* the above doesn't seem to work in Edge */
}

.ProseMirror [contenteditable="false"] {
  white-space: normal;
}

.ProseMirror [contenteditable="false"] [contenteditable="true"] {
  white-space: pre-wrap;
}

.ProseMirror pre {
  white-space: pre-wrap;
}

img.ProseMirror-separator {
  display: inline !important;
  border: none !important;
  margin: 0 !important;
  width: 0 !important;
  height: 0 !important;
}

.ProseMirror-gapcursor {
  display: none;
  pointer-events: none;
  position: absolute;
  margin: 0;
}

.ProseMirror-gapcursor:after {
  content: "";
  display: block;
  position: absolute;
  top: -2px;
  width: 20px;
  border-top: 1px solid black;
  animation: ProseMirror-cursor-blink 1.1s steps(2, start) infinite;
}

@keyframes ProseMirror-cursor-blink {
  to {
    visibility: hidden;
  }
}

.ProseMirror-hideselection *::selection {
  background: transparent;
}

.ProseMirror-hideselection *::-moz-selection {
  background: transparent;
}

.ProseMirror-hideselection * {
  caret-color: transparent;
}

.ProseMirror-focused .ProseMirror-gapcursor {
  display: block;
}`;
function createStyleTag(style2, nonce, suffix) {
	const tiptapStyleTag = document.querySelector(`style[data-tiptap-style${suffix ? `-${suffix}` : ""}]`);
	if (tiptapStyleTag !== null) return tiptapStyleTag;
	const styleNode = document.createElement("style");
	if (nonce) styleNode.setAttribute("nonce", nonce);
	styleNode.setAttribute(`data-tiptap-style${suffix ? `-${suffix}` : ""}`, "");
	styleNode.innerHTML = style2;
	document.getElementsByTagName("head")[0].appendChild(styleNode);
	return styleNode;
}
var Editor = class extends EventEmitter {
	constructor(options = {}) {
		super();
		this.css = null;
		this.className = "tiptap";
		this.editorView = null;
		this.isFocused = false;
		this.isInitialized = false;
		this.extensionStorage = {};
		this.instanceId = Math.random().toString(36).slice(2, 9);
		this.options = {
			element: typeof document !== "undefined" ? document.createElement("div") : null,
			content: "",
			injectCSS: true,
			injectNonce: void 0,
			extensions: [],
			autofocus: false,
			editable: true,
			textDirection: void 0,
			editorProps: {},
			parseOptions: {},
			coreExtensionOptions: {},
			enableInputRules: true,
			enablePasteRules: true,
			enableCoreExtensions: true,
			enableContentCheck: false,
			emitContentError: false,
			onBeforeCreate: () => null,
			onCreate: () => null,
			onMount: () => null,
			onUnmount: () => null,
			onUpdate: () => null,
			onSelectionUpdate: () => null,
			onTransaction: () => null,
			onFocus: () => null,
			onBlur: () => null,
			onDestroy: () => null,
			onContentError: ({ error }) => {
				throw error;
			},
			onPaste: () => null,
			onDrop: () => null,
			onDelete: () => null,
			enableExtensionDispatchTransaction: true
		};
		this.isCapturingTransaction = false;
		this.capturedTransaction = null;
		this.utils = {
			getUpdatedPosition,
			createMappablePosition
		};
		this.setOptions(options);
		this.createExtensionManager();
		this.createCommandManager();
		this.createSchema();
		this.on("beforeCreate", this.options.onBeforeCreate);
		this.emit("beforeCreate", { editor: this });
		this.on("mount", this.options.onMount);
		this.on("unmount", this.options.onUnmount);
		this.on("contentError", this.options.onContentError);
		this.on("create", this.options.onCreate);
		this.on("update", this.options.onUpdate);
		this.on("selectionUpdate", this.options.onSelectionUpdate);
		this.on("transaction", this.options.onTransaction);
		this.on("focus", this.options.onFocus);
		this.on("blur", this.options.onBlur);
		this.on("destroy", this.options.onDestroy);
		this.on("drop", ({ event, slice, moved }) => this.options.onDrop(event, slice, moved));
		this.on("paste", ({ event, slice }) => this.options.onPaste(event, slice));
		this.on("delete", this.options.onDelete);
		const initialDoc = this.createDoc();
		const selection = resolveFocusPosition(initialDoc, this.options.autofocus);
		this.editorState = EditorState.create({
			doc: initialDoc,
			schema: this.schema,
			selection: selection || void 0
		});
		if (this.options.element) this.mount(this.options.element);
	}
	mount(el) {
		if (typeof document === "undefined") throw new Error(`[tiptap error]: The editor cannot be mounted because there is no 'document' defined in this environment.`);
		this.createView(el);
		this.emit("mount", { editor: this });
		if (this.css && !document.head.contains(this.css)) document.head.appendChild(this.css);
		window.setTimeout(() => {
			if (this.isDestroyed) return;
			if (this.options.autofocus !== false && this.options.autofocus !== null) this.commands.focus(this.options.autofocus);
			this.emit("create", { editor: this });
			this.isInitialized = true;
		}, 0);
	}
	unmount() {
		if (this.editorView) {
			const dom = this.editorView.dom;
			if (dom == null ? void 0 : dom.editor) delete dom.editor;
			this.editorView.destroy();
		}
		this.editorView = null;
		this.isInitialized = false;
		if (this.css && !document.querySelectorAll(`.${this.className}`).length) try {
			if (typeof this.css.remove === "function") this.css.remove();
			else if (this.css.parentNode) this.css.parentNode.removeChild(this.css);
		} catch (error) {
			console.warn("Failed to remove CSS element:", error);
		}
		this.css = null;
		this.emit("unmount", { editor: this });
	}
	get storage() {
		return this.extensionStorage;
	}
	get commands() {
		return this.commandManager.commands;
	}
	chain() {
		return this.commandManager.chain();
	}
	can() {
		return this.commandManager.can();
	}
	injectCSS() {
		if (this.options.injectCSS && typeof document !== "undefined") this.css = createStyleTag(style, this.options.injectNonce);
	}
	setOptions(options = {}) {
		this.options = {
			...this.options,
			...options
		};
		if (!this.editorView || !this.state || this.isDestroyed) return;
		if (this.options.editorProps) this.view.setProps(this.options.editorProps);
		this.view.updateState(this.state);
	}
	setEditable(editable, emitUpdate = true) {
		this.setOptions({ editable });
		if (emitUpdate) this.emit("update", {
			editor: this,
			transaction: this.state.tr,
			appendedTransactions: []
		});
	}
	get isEditable() {
		return this.options.editable && this.view && this.view.editable;
	}
	get view() {
		if (this.editorView) return this.editorView;
		return new Proxy({
			state: this.editorState,
			updateState: (state) => {
				this.editorState = state;
			},
			dispatch: (tr$1) => {
				this.dispatchTransaction(tr$1);
			},
			composing: false,
			dragging: null,
			editable: true,
			isDestroyed: false
		}, { get: (obj, key) => {
			if (this.editorView) return this.editorView[key];
			if (key === "state") return this.editorState;
			if (key in obj) return Reflect.get(obj, key);
			throw new Error(`[tiptap error]: The editor view is not available. Cannot access view['${key}']. The editor may not be mounted yet.`);
		} });
	}
	get state() {
		if (this.editorView) this.editorState = this.view.state;
		return this.editorState;
	}
	registerPlugin(plugin, handlePlugins) {
		const plugins = isFunction$1(handlePlugins) ? handlePlugins(plugin, [...this.state.plugins]) : [...this.state.plugins, plugin];
		const state = this.state.reconfigure({ plugins });
		this.view.updateState(state);
		return state;
	}
	unregisterPlugin(nameOrPluginKeyToRemove) {
		if (this.isDestroyed) return;
		const prevPlugins = this.state.plugins;
		let plugins = prevPlugins;
		[].concat(nameOrPluginKeyToRemove).forEach((nameOrPluginKey) => {
			const name = typeof nameOrPluginKey === "string" ? `${nameOrPluginKey}$` : nameOrPluginKey.key;
			plugins = plugins.filter((plugin) => !plugin.key.startsWith(name));
		});
		if (prevPlugins.length === plugins.length) return;
		const state = this.state.reconfigure({ plugins });
		this.view.updateState(state);
		return state;
	}
	createExtensionManager() {
		var _a, _b;
		this.extensionManager = new ExtensionManager([...this.options.enableCoreExtensions ? [
			Editable,
			ClipboardTextSerializer.configure({ blockSeparator: (_b = (_a = this.options.coreExtensionOptions) == null ? void 0 : _a.clipboardTextSerializer) == null ? void 0 : _b.blockSeparator }),
			Commands,
			FocusEvents,
			Keymap,
			Tabindex,
			Drop,
			Paste,
			Delete,
			TextDirection.configure({ direction: this.options.textDirection })
		].filter((ext) => {
			if (typeof this.options.enableCoreExtensions === "object") return this.options.enableCoreExtensions[ext.name] !== false;
			return true;
		}) : [], ...this.options.extensions].filter((extension) => {
			return [
				"extension",
				"node",
				"mark"
			].includes(extension == null ? void 0 : extension.type);
		}), this);
	}
	createCommandManager() {
		this.commandManager = new CommandManager({ editor: this });
	}
	createSchema() {
		this.schema = this.extensionManager.schema;
	}
	createDoc() {
		let doc$2;
		try {
			doc$2 = createDocument(this.options.content, this.schema, this.options.parseOptions, { errorOnInvalidContent: this.options.enableContentCheck });
		} catch (e) {
			if (!(e instanceof Error) || !["[tiptap error]: Invalid JSON content", "[tiptap error]: Invalid HTML content"].includes(e.message)) throw e;
			this.emit("contentError", {
				editor: this,
				error: e,
				disableCollaboration: () => {
					if ("collaboration" in this.storage && typeof this.storage.collaboration === "object" && this.storage.collaboration) this.storage.collaboration.isDisabled = true;
					this.options.extensions = this.options.extensions.filter((extension) => extension.name !== "collaboration");
					this.createExtensionManager();
				}
			});
			doc$2 = createDocument(this.options.content, this.schema, this.options.parseOptions, { errorOnInvalidContent: false });
		}
		return doc$2;
	}
	createView(element) {
		const { editorProps, enableExtensionDispatchTransaction } = this.options;
		const baseDispatch = editorProps.dispatchTransaction || this.dispatchTransaction.bind(this);
		const dispatch = enableExtensionDispatchTransaction ? this.extensionManager.dispatchTransaction(baseDispatch) : baseDispatch;
		const baseTransformPastedHTML = editorProps.transformPastedHTML;
		const transformPastedHTML = this.extensionManager.transformPastedHTML(baseTransformPastedHTML);
		this.editorView = new EditorView(element, {
			...editorProps,
			attributes: {
				role: "textbox",
				...editorProps == null ? void 0 : editorProps.attributes
			},
			dispatchTransaction: dispatch,
			transformPastedHTML,
			state: this.editorState,
			markViews: this.extensionManager.markViews,
			nodeViews: this.extensionManager.nodeViews
		});
		const newState = this.state.reconfigure({ plugins: this.extensionManager.plugins });
		this.view.updateState(newState);
		this.prependClass();
		this.injectCSS();
		const dom = this.view.dom;
		dom.editor = this;
	}
	createNodeViews() {
		if (this.view.isDestroyed) return;
		this.view.setProps({
			markViews: this.extensionManager.markViews,
			nodeViews: this.extensionManager.nodeViews
		});
	}
	prependClass() {
		this.view.dom.className = `${this.className} ${this.view.dom.className}`;
	}
	captureTransaction(fn) {
		this.isCapturingTransaction = true;
		fn();
		this.isCapturingTransaction = false;
		const tr$1 = this.capturedTransaction;
		this.capturedTransaction = null;
		return tr$1;
	}
	dispatchTransaction(transaction) {
		if (this.view.isDestroyed) return;
		if (this.isCapturingTransaction) {
			if (!this.capturedTransaction) {
				this.capturedTransaction = transaction;
				return;
			}
			transaction.steps.forEach((step$1) => {
				var _a;
				return (_a = this.capturedTransaction) == null ? void 0 : _a.step(step$1);
			});
			return;
		}
		const { state, transactions } = this.state.applyTransaction(transaction);
		const selectionHasChanged = !this.state.selection.eq(state.selection);
		const rootTrWasApplied = transactions.includes(transaction);
		const prevState = this.state;
		this.emit("beforeTransaction", {
			editor: this,
			transaction,
			nextState: state
		});
		if (!rootTrWasApplied) return;
		this.view.updateState(state);
		this.emit("transaction", {
			editor: this,
			transaction,
			appendedTransactions: transactions.slice(1)
		});
		if (selectionHasChanged) this.emit("selectionUpdate", {
			editor: this,
			transaction
		});
		const mostRecentFocusTr = transactions.findLast((tr$1) => tr$1.getMeta("focus") || tr$1.getMeta("blur"));
		const focus2 = mostRecentFocusTr == null ? void 0 : mostRecentFocusTr.getMeta("focus");
		const blur2 = mostRecentFocusTr == null ? void 0 : mostRecentFocusTr.getMeta("blur");
		if (focus2) this.emit("focus", {
			editor: this,
			event: focus2.event,
			transaction: mostRecentFocusTr
		});
		if (blur2) this.emit("blur", {
			editor: this,
			event: blur2.event,
			transaction: mostRecentFocusTr
		});
		if (transaction.getMeta("preventUpdate") || !transactions.some((tr$1) => tr$1.docChanged) || prevState.doc.eq(state.doc)) return;
		this.emit("update", {
			editor: this,
			transaction,
			appendedTransactions: transactions.slice(1)
		});
	}
	getAttributes(nameOrType) {
		return getAttributes(this.state, nameOrType);
	}
	isActive(nameOrAttributes, attributesOrUndefined) {
		const name = typeof nameOrAttributes === "string" ? nameOrAttributes : null;
		const attributes = typeof nameOrAttributes === "string" ? attributesOrUndefined : nameOrAttributes;
		return isActive(this.state, name, attributes);
	}
	getJSON() {
		return this.state.doc.toJSON();
	}
	getHTML() {
		return getHTMLFromFragment(this.state.doc.content, this.schema);
	}
	getText(options) {
		const { blockSeparator = "\n\n", textSerializers = {} } = options || {};
		return getText(this.state.doc, {
			blockSeparator,
			textSerializers: {
				...getTextSerializersFromSchema(this.schema),
				...textSerializers
			}
		});
	}
	get isEmpty() {
		return isNodeEmpty(this.state.doc);
	}
	destroy() {
		this.emit("destroy");
		this.unmount();
		this.removeAllListeners();
	}
	get isDestroyed() {
		var _a, _b;
		return (_b = (_a = this.editorView) == null ? void 0 : _a.isDestroyed) != null ? _b : true;
	}
	$node(selector, attributes) {
		var _a;
		return ((_a = this.$doc) == null ? void 0 : _a.querySelector(selector, attributes)) || null;
	}
	$nodes(selector, attributes) {
		var _a;
		return ((_a = this.$doc) == null ? void 0 : _a.querySelectorAll(selector, attributes)) || null;
	}
	$pos(pos) {
		return new NodePos(this.state.doc.resolve(pos), this);
	}
	get $doc() {
		return this.$pos(0);
	}
};
function markInputRule(config) {
	return new InputRule({
		find: config.find,
		handler: ({ state, range, match }) => {
			const attributes = callOrReturn(config.getAttributes, void 0, match);
			if (attributes === false || attributes === null) return null;
			const { tr: tr$1 } = state;
			const captureGroup = match[match.length - 1];
			const fullMatch = match[0];
			if (captureGroup) {
				const startSpaces = fullMatch.search(/\S/);
				const textStart = range.from + fullMatch.indexOf(captureGroup);
				const textEnd = textStart + captureGroup.length;
				if (getMarksBetween(range.from, range.to, state.doc).filter((item) => {
					return item.mark.type.excluded.find((type) => type === config.type && type !== item.mark.type);
				}).filter((item) => item.to > textStart).length) return null;
				if (textEnd < range.to) tr$1.delete(textEnd, range.to);
				if (textStart > range.from) tr$1.delete(range.from + startSpaces, textStart);
				const markEnd = range.from + startSpaces + captureGroup.length;
				tr$1.addMark(range.from + startSpaces, markEnd, config.type.create(attributes || {}));
				tr$1.removeStoredMark(config.type);
			}
		},
		undoable: config.undoable
	});
}
function nodeInputRule(config) {
	return new InputRule({
		find: config.find,
		handler: ({ state, range, match }) => {
			const attributes = callOrReturn(config.getAttributes, void 0, match) || {};
			const { tr: tr$1 } = state;
			const start = range.from;
			let end = range.to;
			const newNode = config.type.create(attributes);
			if (match[1]) {
				let matchStart = start + match[0].lastIndexOf(match[1]);
				if (matchStart > end) matchStart = end;
				else end = matchStart + match[1].length;
				const lastChar = match[0][match[0].length - 1];
				tr$1.insertText(lastChar, start + match[0].length - 1);
				tr$1.replaceWith(matchStart, end, newNode);
			} else if (match[0]) {
				const insertionStart = config.type.isInline ? start : start - 1;
				tr$1.insert(insertionStart, config.type.create(attributes)).delete(tr$1.mapping.map(start), tr$1.mapping.map(end));
			}
			tr$1.scrollIntoView();
		},
		undoable: config.undoable
	});
}
function textblockTypeInputRule(config) {
	return new InputRule({
		find: config.find,
		handler: ({ state, range, match }) => {
			const $start = state.doc.resolve(range.from);
			const attributes = callOrReturn(config.getAttributes, void 0, match) || {};
			if (!$start.node(-1).canReplaceWith($start.index(-1), $start.indexAfter(-1), config.type)) return null;
			state.tr.delete(range.from, range.to).setBlockType(range.from, range.from, config.type, attributes);
		},
		undoable: config.undoable
	});
}
function wrappingInputRule(config) {
	return new InputRule({
		find: config.find,
		handler: ({ state, range, match, chain }) => {
			const attributes = callOrReturn(config.getAttributes, void 0, match) || {};
			const tr$1 = state.tr.delete(range.from, range.to);
			const blockRange = tr$1.doc.resolve(range.from).blockRange();
			const wrapping = blockRange && findWrapping(blockRange, config.type, attributes);
			if (!wrapping) return null;
			tr$1.wrap(blockRange, wrapping);
			if (config.keepMarks && config.editor) {
				const { selection, storedMarks } = state;
				const { splittableMarks } = config.editor.extensionManager;
				const marks = storedMarks || selection.$to.parentOffset && selection.$from.marks();
				if (marks) {
					const filteredMarks = marks.filter((mark) => splittableMarks.includes(mark.type.name));
					tr$1.ensureMarks(filteredMarks);
				}
			}
			if (config.keepAttributes) {
				const nodeType = config.type.name === "bulletList" || config.type.name === "orderedList" ? "listItem" : "taskList";
				chain().updateAttributes(nodeType, attributes).run();
			}
			const before = tr$1.doc.resolve(range.from - 1).nodeBefore;
			if (before && before.type === config.type && canJoin(tr$1.doc, range.from - 1) && (!config.joinPredicate || config.joinPredicate(match, before))) tr$1.join(range.from - 1);
		},
		undoable: config.undoable
	});
}
var isTouchEvent = (e) => {
	return "touches" in e;
};
var ResizableNodeView = class {
	constructor(options) {
		this.directions = [
			"bottom-left",
			"bottom-right",
			"top-left",
			"top-right"
		];
		this.minSize = {
			height: 8,
			width: 8
		};
		this.preserveAspectRatio = false;
		this.classNames = {
			container: "",
			wrapper: "",
			handle: "",
			resizing: ""
		};
		this.initialWidth = 0;
		this.initialHeight = 0;
		this.aspectRatio = 1;
		this.isResizing = false;
		this.activeHandle = null;
		this.startX = 0;
		this.startY = 0;
		this.startWidth = 0;
		this.startHeight = 0;
		this.isShiftKeyPressed = false;
		this.lastEditableState = void 0;
		this.handleMap = /* @__PURE__ */ new Map();
		this.handleMouseMove = (event) => {
			if (!this.isResizing || !this.activeHandle) return;
			const deltaX = event.clientX - this.startX;
			const deltaY = event.clientY - this.startY;
			this.handleResize(deltaX, deltaY);
		};
		this.handleTouchMove = (event) => {
			if (!this.isResizing || !this.activeHandle) return;
			const touch = event.touches[0];
			if (!touch) return;
			const deltaX = touch.clientX - this.startX;
			const deltaY = touch.clientY - this.startY;
			this.handleResize(deltaX, deltaY);
		};
		this.handleMouseUp = () => {
			if (!this.isResizing) return;
			const finalWidth = this.element.offsetWidth;
			const finalHeight = this.element.offsetHeight;
			this.onCommit(finalWidth, finalHeight);
			this.isResizing = false;
			this.activeHandle = null;
			this.container.dataset.resizeState = "false";
			if (this.classNames.resizing) this.container.classList.remove(this.classNames.resizing);
			document.removeEventListener("mousemove", this.handleMouseMove);
			document.removeEventListener("mouseup", this.handleMouseUp);
			document.removeEventListener("keydown", this.handleKeyDown);
			document.removeEventListener("keyup", this.handleKeyUp);
		};
		this.handleKeyDown = (event) => {
			if (event.key === "Shift") this.isShiftKeyPressed = true;
		};
		this.handleKeyUp = (event) => {
			if (event.key === "Shift") this.isShiftKeyPressed = false;
		};
		var _a, _b, _c, _d, _e$1, _f;
		this.node = options.node;
		this.editor = options.editor;
		this.element = options.element;
		this.contentElement = options.contentElement;
		this.getPos = options.getPos;
		this.onResize = options.onResize;
		this.onCommit = options.onCommit;
		this.onUpdate = options.onUpdate;
		if ((_a = options.options) == null ? void 0 : _a.min) this.minSize = {
			...this.minSize,
			...options.options.min
		};
		if ((_b = options.options) == null ? void 0 : _b.max) this.maxSize = options.options.max;
		if ((_c = options == null ? void 0 : options.options) == null ? void 0 : _c.directions) this.directions = options.options.directions;
		if ((_d = options.options) == null ? void 0 : _d.preserveAspectRatio) this.preserveAspectRatio = options.options.preserveAspectRatio;
		if ((_e$1 = options.options) == null ? void 0 : _e$1.className) this.classNames = {
			container: options.options.className.container || "",
			wrapper: options.options.className.wrapper || "",
			handle: options.options.className.handle || "",
			resizing: options.options.className.resizing || ""
		};
		if ((_f = options.options) == null ? void 0 : _f.createCustomHandle) this.createCustomHandle = options.options.createCustomHandle;
		this.wrapper = this.createWrapper();
		this.container = this.createContainer();
		this.applyInitialSize();
		this.attachHandles();
		this.editor.on("update", this.handleEditorUpdate.bind(this));
	}
	get dom() {
		return this.container;
	}
	get contentDOM() {
		var _a;
		return (_a = this.contentElement) != null ? _a : null;
	}
	handleEditorUpdate() {
		const isEditable = this.editor.isEditable;
		if (isEditable === this.lastEditableState) return;
		this.lastEditableState = isEditable;
		if (!isEditable) this.removeHandles();
		else if (isEditable && this.handleMap.size === 0) this.attachHandles();
	}
	update(node, decorations, innerDecorations) {
		if (node.type !== this.node.type) return false;
		this.node = node;
		if (this.onUpdate) return this.onUpdate(node, decorations, innerDecorations);
		return true;
	}
	destroy() {
		if (this.isResizing) {
			this.container.dataset.resizeState = "false";
			if (this.classNames.resizing) this.container.classList.remove(this.classNames.resizing);
			document.removeEventListener("mousemove", this.handleMouseMove);
			document.removeEventListener("mouseup", this.handleMouseUp);
			document.removeEventListener("keydown", this.handleKeyDown);
			document.removeEventListener("keyup", this.handleKeyUp);
			this.isResizing = false;
			this.activeHandle = null;
		}
		this.editor.off("update", this.handleEditorUpdate.bind(this));
		this.container.remove();
	}
	createContainer() {
		const element = document.createElement("div");
		element.dataset.resizeContainer = "";
		element.dataset.node = this.node.type.name;
		element.style.display = this.node.type.isInline ? "inline-flex" : "flex";
		if (this.classNames.container) element.className = this.classNames.container;
		element.appendChild(this.wrapper);
		return element;
	}
	createWrapper() {
		const element = document.createElement("div");
		element.style.position = "relative";
		element.style.display = "block";
		element.dataset.resizeWrapper = "";
		if (this.classNames.wrapper) element.className = this.classNames.wrapper;
		element.appendChild(this.element);
		return element;
	}
	createHandle(direction) {
		const handle = document.createElement("div");
		handle.dataset.resizeHandle = direction;
		handle.style.position = "absolute";
		if (this.classNames.handle) handle.className = this.classNames.handle;
		return handle;
	}
	positionHandle(handle, direction) {
		const isTop = direction.includes("top");
		const isBottom = direction.includes("bottom");
		const isLeft = direction.includes("left");
		const isRight = direction.includes("right");
		if (isTop) handle.style.top = "0";
		if (isBottom) handle.style.bottom = "0";
		if (isLeft) handle.style.left = "0";
		if (isRight) handle.style.right = "0";
		if (direction === "top" || direction === "bottom") {
			handle.style.left = "0";
			handle.style.right = "0";
		}
		if (direction === "left" || direction === "right") {
			handle.style.top = "0";
			handle.style.bottom = "0";
		}
	}
	attachHandles() {
		this.directions.forEach((direction) => {
			let handle;
			if (this.createCustomHandle) handle = this.createCustomHandle(direction);
			else handle = this.createHandle(direction);
			if (!(handle instanceof HTMLElement)) {
				console.warn(`[ResizableNodeView] createCustomHandle("${direction}") did not return an HTMLElement. Falling back to default handle.`);
				handle = this.createHandle(direction);
			}
			if (!this.createCustomHandle) this.positionHandle(handle, direction);
			handle.addEventListener("mousedown", (event) => this.handleResizeStart(event, direction));
			handle.addEventListener("touchstart", (event) => this.handleResizeStart(event, direction));
			this.handleMap.set(direction, handle);
			this.wrapper.appendChild(handle);
		});
	}
	removeHandles() {
		this.handleMap.forEach((el) => el.remove());
		this.handleMap.clear();
	}
	applyInitialSize() {
		const width = this.node.attrs.width;
		const height = this.node.attrs.height;
		if (width) {
			this.element.style.width = `${width}px`;
			this.initialWidth = width;
		} else this.initialWidth = this.element.offsetWidth;
		if (height) {
			this.element.style.height = `${height}px`;
			this.initialHeight = height;
		} else this.initialHeight = this.element.offsetHeight;
		if (this.initialWidth > 0 && this.initialHeight > 0) this.aspectRatio = this.initialWidth / this.initialHeight;
	}
	handleResizeStart(event, direction) {
		event.preventDefault();
		event.stopPropagation();
		this.isResizing = true;
		this.activeHandle = direction;
		if (isTouchEvent(event)) {
			this.startX = event.touches[0].clientX;
			this.startY = event.touches[0].clientY;
		} else {
			this.startX = event.clientX;
			this.startY = event.clientY;
		}
		this.startWidth = this.element.offsetWidth;
		this.startHeight = this.element.offsetHeight;
		if (this.startWidth > 0 && this.startHeight > 0) this.aspectRatio = this.startWidth / this.startHeight;
		if (this.getPos() !== void 0) {}
		this.container.dataset.resizeState = "true";
		if (this.classNames.resizing) this.container.classList.add(this.classNames.resizing);
		document.addEventListener("mousemove", this.handleMouseMove);
		document.addEventListener("touchmove", this.handleTouchMove);
		document.addEventListener("mouseup", this.handleMouseUp);
		document.addEventListener("keydown", this.handleKeyDown);
		document.addEventListener("keyup", this.handleKeyUp);
	}
	handleResize(deltaX, deltaY) {
		if (!this.activeHandle) return;
		const shouldPreserveAspectRatio = this.preserveAspectRatio || this.isShiftKeyPressed;
		const { width, height } = this.calculateNewDimensions(this.activeHandle, deltaX, deltaY);
		const constrained = this.applyConstraints(width, height, shouldPreserveAspectRatio);
		this.element.style.width = `${constrained.width}px`;
		this.element.style.height = `${constrained.height}px`;
		if (this.onResize) this.onResize(constrained.width, constrained.height);
	}
	calculateNewDimensions(direction, deltaX, deltaY) {
		let newWidth = this.startWidth;
		let newHeight = this.startHeight;
		const isRight = direction.includes("right");
		const isLeft = direction.includes("left");
		const isBottom = direction.includes("bottom");
		const isTop = direction.includes("top");
		if (isRight) newWidth = this.startWidth + deltaX;
		else if (isLeft) newWidth = this.startWidth - deltaX;
		if (isBottom) newHeight = this.startHeight + deltaY;
		else if (isTop) newHeight = this.startHeight - deltaY;
		if (direction === "right" || direction === "left") newWidth = this.startWidth + (isRight ? deltaX : -deltaX);
		if (direction === "top" || direction === "bottom") newHeight = this.startHeight + (isBottom ? deltaY : -deltaY);
		if (this.preserveAspectRatio || this.isShiftKeyPressed) return this.applyAspectRatio(newWidth, newHeight, direction);
		return {
			width: newWidth,
			height: newHeight
		};
	}
	applyConstraints(width, height, preserveAspectRatio) {
		var _a, _b, _c, _d;
		if (!preserveAspectRatio) {
			let constrainedWidth2 = Math.max(this.minSize.width, width);
			let constrainedHeight2 = Math.max(this.minSize.height, height);
			if ((_a = this.maxSize) == null ? void 0 : _a.width) constrainedWidth2 = Math.min(this.maxSize.width, constrainedWidth2);
			if ((_b = this.maxSize) == null ? void 0 : _b.height) constrainedHeight2 = Math.min(this.maxSize.height, constrainedHeight2);
			return {
				width: constrainedWidth2,
				height: constrainedHeight2
			};
		}
		let constrainedWidth = width;
		let constrainedHeight = height;
		if (constrainedWidth < this.minSize.width) {
			constrainedWidth = this.minSize.width;
			constrainedHeight = constrainedWidth / this.aspectRatio;
		}
		if (constrainedHeight < this.minSize.height) {
			constrainedHeight = this.minSize.height;
			constrainedWidth = constrainedHeight * this.aspectRatio;
		}
		if (((_c = this.maxSize) == null ? void 0 : _c.width) && constrainedWidth > this.maxSize.width) {
			constrainedWidth = this.maxSize.width;
			constrainedHeight = constrainedWidth / this.aspectRatio;
		}
		if (((_d = this.maxSize) == null ? void 0 : _d.height) && constrainedHeight > this.maxSize.height) {
			constrainedHeight = this.maxSize.height;
			constrainedWidth = constrainedHeight * this.aspectRatio;
		}
		return {
			width: constrainedWidth,
			height: constrainedHeight
		};
	}
	applyAspectRatio(width, height, direction) {
		const isHorizontal = direction === "left" || direction === "right";
		const isVertical = direction === "top" || direction === "bottom";
		if (isHorizontal) return {
			width,
			height: width / this.aspectRatio
		};
		if (isVertical) return {
			width: height * this.aspectRatio,
			height
		};
		return {
			width,
			height: width / this.aspectRatio
		};
	}
};
function canInsertNode(state, nodeType) {
	const { selection } = state;
	const { $from } = selection;
	if (selection instanceof NodeSelection) {
		const index = $from.index();
		return $from.parent.canReplaceWith(index, index + 1, nodeType);
	}
	let depth = $from.depth;
	while (depth >= 0) {
		const index = $from.index(depth);
		if ($from.node(depth).contentMatchAt(index).matchType(nodeType)) return true;
		depth -= 1;
	}
	return false;
}
function decodeHtmlEntities(text) {
	return text.replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, "\"").replace(/&amp;/g, "&");
}
function encodeHtmlEntities(text) {
	return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}
__export$1({}, {
	createAtomBlockMarkdownSpec: () => createAtomBlockMarkdownSpec,
	createBlockMarkdownSpec: () => createBlockMarkdownSpec,
	createInlineMarkdownSpec: () => createInlineMarkdownSpec,
	parseAttributes: () => parseAttributes,
	parseIndentedBlocks: () => parseIndentedBlocks,
	renderNestedMarkdownContent: () => renderNestedMarkdownContent,
	serializeAttributes: () => serializeAttributes
});
function parseAttributes(attrString) {
	if (!(attrString == null ? void 0 : attrString.trim())) return {};
	const attributes = {};
	const quotedStrings = [];
	const tempString = attrString.replace(/["']([^"']*)["']/g, (match) => {
		quotedStrings.push(match);
		return `__QUOTED_${quotedStrings.length - 1}__`;
	});
	const classMatches = tempString.match(/(?:^|\s)\.([a-zA-Z][\w-]*)/g);
	if (classMatches) attributes.class = classMatches.map((match) => match.trim().slice(1)).join(" ");
	const idMatch = tempString.match(/(?:^|\s)#([a-zA-Z][\w-]*)/);
	if (idMatch) attributes.id = idMatch[1];
	Array.from(tempString.matchAll(/([a-zA-Z][\w-]*)\s*=\s*(__QUOTED_\d+__)/g)).forEach(([, key, quotedRef]) => {
		var _a;
		const quotedValue = quotedStrings[parseInt(((_a = quotedRef.match(/__QUOTED_(\d+)__/)) == null ? void 0 : _a[1]) || "0", 10)];
		if (quotedValue) attributes[key] = quotedValue.slice(1, -1);
	});
	const cleanString = tempString.replace(/(?:^|\s)\.([a-zA-Z][\w-]*)/g, "").replace(/(?:^|\s)#([a-zA-Z][\w-]*)/g, "").replace(/([a-zA-Z][\w-]*)\s*=\s*__QUOTED_\d+__/g, "").trim();
	if (cleanString) cleanString.split(/\s+/).filter(Boolean).forEach((attr) => {
		if (attr.match(/^[a-zA-Z][\w-]*$/)) attributes[attr] = true;
	});
	return attributes;
}
function serializeAttributes(attributes) {
	if (!attributes || Object.keys(attributes).length === 0) return "";
	const parts = [];
	if (attributes.class) String(attributes.class).split(/\s+/).filter(Boolean).forEach((cls) => parts.push(`.${cls}`));
	if (attributes.id) parts.push(`#${attributes.id}`);
	Object.entries(attributes).forEach(([key, value]) => {
		if (key === "class" || key === "id") return;
		if (value === true) parts.push(key);
		else if (value !== false && value != null) parts.push(`${key}="${String(value)}"`);
	});
	return parts.join(" ");
}
function createAtomBlockMarkdownSpec(options) {
	const { nodeName, name: markdownName, parseAttributes: parseAttributes2 = parseAttributes, serializeAttributes: serializeAttributes2 = serializeAttributes, defaultAttributes = {}, requiredAttributes = [], allowedAttributes } = options;
	const blockName = markdownName || nodeName;
	const filterAttributes = (attrs) => {
		if (!allowedAttributes) return attrs;
		const filtered = {};
		allowedAttributes.forEach((key) => {
			if (key in attrs) filtered[key] = attrs[key];
		});
		return filtered;
	};
	return {
		parseMarkdown: (token, h2) => {
			const attrs = {
				...defaultAttributes,
				...token.attributes
			};
			return h2.createNode(nodeName, attrs, []);
		},
		markdownTokenizer: {
			name: nodeName,
			level: "block",
			start(src) {
				var _a;
				const regex = new RegExp(`^:::${blockName}(?:\\s|$)`, "m");
				const index = (_a = src.match(regex)) == null ? void 0 : _a.index;
				return index !== void 0 ? index : -1;
			},
			tokenize(src, _tokens, _lexer) {
				const regex = /* @__PURE__ */ new RegExp(`^:::${blockName}(?:\\s+\\{([^}]*)\\})?\\s*:::(?:\\n|$)`);
				const match = src.match(regex);
				if (!match) return;
				const attributes = parseAttributes2(match[1] || "");
				if (requiredAttributes.find((required) => !(required in attributes))) return;
				return {
					type: nodeName,
					raw: match[0],
					attributes
				};
			}
		},
		renderMarkdown: (node) => {
			const attrs = serializeAttributes2(filterAttributes(node.attrs || {}));
			return `:::${blockName}${attrs ? ` {${attrs}}` : ""} :::`;
		}
	};
}
function createBlockMarkdownSpec(options) {
	const { nodeName, name: markdownName, getContent, parseAttributes: parseAttributes2 = parseAttributes, serializeAttributes: serializeAttributes2 = serializeAttributes, defaultAttributes = {}, content = "block", allowedAttributes } = options;
	const blockName = markdownName || nodeName;
	const filterAttributes = (attrs) => {
		if (!allowedAttributes) return attrs;
		const filtered = {};
		allowedAttributes.forEach((key) => {
			if (key in attrs) filtered[key] = attrs[key];
		});
		return filtered;
	};
	return {
		parseMarkdown: (token, h2) => {
			let nodeContent;
			if (getContent) {
				const contentResult = getContent(token);
				nodeContent = typeof contentResult === "string" ? [{
					type: "text",
					text: contentResult
				}] : contentResult;
			} else if (content === "block") nodeContent = h2.parseChildren(token.tokens || []);
			else nodeContent = h2.parseInline(token.tokens || []);
			const attrs = {
				...defaultAttributes,
				...token.attributes
			};
			return h2.createNode(nodeName, attrs, nodeContent);
		},
		markdownTokenizer: {
			name: nodeName,
			level: "block",
			start(src) {
				var _a;
				const regex = new RegExp(`^:::${blockName}`, "m");
				const index = (_a = src.match(regex)) == null ? void 0 : _a.index;
				return index !== void 0 ? index : -1;
			},
			tokenize(src, _tokens, lexer) {
				var _a;
				const openingRegex = /* @__PURE__ */ new RegExp(`^:::${blockName}(?:\\s+\\{([^}]*)\\})?\\s*\\n`);
				const openingMatch = src.match(openingRegex);
				if (!openingMatch) return;
				const [openingTag, attrString = ""] = openingMatch;
				const attributes = parseAttributes2(attrString);
				let level = 1;
				const position = openingTag.length;
				let matchedContent = "";
				const blockPattern = /^:::([\w-]*)(\s.*)?/gm;
				const remaining = src.slice(position);
				blockPattern.lastIndex = 0;
				for (;;) {
					const match = blockPattern.exec(remaining);
					if (match === null) break;
					const matchPos = match.index;
					const blockType = match[1];
					if ((_a = match[2]) == null ? void 0 : _a.endsWith(":::")) continue;
					if (blockType) level += 1;
					else {
						level -= 1;
						if (level === 0) {
							const rawContent = remaining.slice(0, matchPos);
							matchedContent = rawContent.trim();
							const fullMatch = src.slice(0, position + matchPos + match[0].length);
							let contentTokens = [];
							if (matchedContent) if (content === "block") {
								contentTokens = lexer.blockTokens(rawContent);
								contentTokens.forEach((token) => {
									if (token.text && (!token.tokens || token.tokens.length === 0)) token.tokens = lexer.inlineTokens(token.text);
								});
								while (contentTokens.length > 0) {
									const lastToken = contentTokens[contentTokens.length - 1];
									if (lastToken.type === "paragraph" && (!lastToken.text || lastToken.text.trim() === "")) contentTokens.pop();
									else break;
								}
							} else contentTokens = lexer.inlineTokens(matchedContent);
							return {
								type: nodeName,
								raw: fullMatch,
								attributes,
								content: matchedContent,
								tokens: contentTokens
							};
						}
					}
				}
			}
		},
		renderMarkdown: (node, h2) => {
			const attrs = serializeAttributes2(filterAttributes(node.attrs || {}));
			return `:::${blockName}${attrs ? ` {${attrs}}` : ""}

${h2.renderChildren(node.content || [], "\n\n")}

:::`;
		}
	};
}
function parseShortcodeAttributes(attrString) {
	if (!attrString.trim()) return {};
	const attributes = {};
	const regex = /(\w+)=(?:"([^"]*)"|'([^']*)')/g;
	let match = regex.exec(attrString);
	while (match !== null) {
		const [, key, doubleQuoted, singleQuoted] = match;
		attributes[key] = doubleQuoted || singleQuoted;
		match = regex.exec(attrString);
	}
	return attributes;
}
function serializeShortcodeAttributes(attrs) {
	return Object.entries(attrs).filter(([, value]) => value !== void 0 && value !== null).map(([key, value]) => `${key}="${value}"`).join(" ");
}
function createInlineMarkdownSpec(options) {
	const { nodeName, name: shortcodeName, getContent, parseAttributes: parseAttributes2 = parseShortcodeAttributes, serializeAttributes: serializeAttributes2 = serializeShortcodeAttributes, defaultAttributes = {}, selfClosing = false, allowedAttributes } = options;
	const shortcode = shortcodeName || nodeName;
	const filterAttributes = (attrs) => {
		if (!allowedAttributes) return attrs;
		const filtered = {};
		allowedAttributes.forEach((attr) => {
			const attrName = typeof attr === "string" ? attr : attr.name;
			const skipIfDefault = typeof attr === "string" ? void 0 : attr.skipIfDefault;
			if (attrName in attrs) {
				const value = attrs[attrName];
				if (skipIfDefault !== void 0 && value === skipIfDefault) return;
				filtered[attrName] = value;
			}
		});
		return filtered;
	};
	const escapedShortcode = shortcode.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
	return {
		parseMarkdown: (token, h2) => {
			const attrs = {
				...defaultAttributes,
				...token.attributes
			};
			if (selfClosing) return h2.createNode(nodeName, attrs);
			const content = getContent ? getContent(token) : token.content || "";
			if (content) return h2.createNode(nodeName, attrs, [h2.createTextNode(content)]);
			return h2.createNode(nodeName, attrs, []);
		},
		markdownTokenizer: {
			name: nodeName,
			level: "inline",
			start(src) {
				const startPattern = selfClosing ? /* @__PURE__ */ new RegExp(`\\[${escapedShortcode}\\s*[^\\]]*\\]`) : /* @__PURE__ */ new RegExp(`\\[${escapedShortcode}\\s*[^\\]]*\\][\\s\\S]*?\\[\\/${escapedShortcode}\\]`);
				const match = src.match(startPattern);
				const index = match == null ? void 0 : match.index;
				return index !== void 0 ? index : -1;
			},
			tokenize(src, _tokens, _lexer) {
				const tokenPattern = selfClosing ? /* @__PURE__ */ new RegExp(`^\\[${escapedShortcode}\\s*([^\\]]*)\\]`) : /* @__PURE__ */ new RegExp(`^\\[${escapedShortcode}\\s*([^\\]]*)\\]([\\s\\S]*?)\\[\\/${escapedShortcode}\\]`);
				const match = src.match(tokenPattern);
				if (!match) return;
				let content = "";
				let attrString = "";
				if (selfClosing) {
					const [, attrs] = match;
					attrString = attrs;
				} else {
					const [, attrs, contentMatch] = match;
					attrString = attrs;
					content = contentMatch || "";
				}
				const attributes = parseAttributes2(attrString.trim());
				return {
					type: nodeName,
					raw: match[0],
					content: content.trim(),
					attributes
				};
			}
		},
		renderMarkdown: (node) => {
			let content = "";
			if (getContent) content = getContent(node);
			else if (node.content && node.content.length > 0) content = node.content.filter((child) => child.type === "text").map((child) => child.text).join("");
			const attrs = serializeAttributes2(filterAttributes(node.attrs || {}));
			const attrString = attrs ? ` ${attrs}` : "";
			if (selfClosing) return `[${shortcode}${attrString}]`;
			return `[${shortcode}${attrString}]${content}[/${shortcode}]`;
		}
	};
}
function parseIndentedBlocks(src, config, lexer) {
	var _a, _b, _c, _d;
	const lines = src.split("\n");
	const items = [];
	let totalRaw = "";
	let i$1 = 0;
	const baseIndentSize = config.baseIndentSize || 2;
	while (i$1 < lines.length) {
		const currentLine = lines[i$1];
		const itemMatch = currentLine.match(config.itemPattern);
		if (!itemMatch) if (items.length > 0) break;
		else if (currentLine.trim() === "") {
			i$1 += 1;
			totalRaw = `${totalRaw}${currentLine}
`;
			continue;
		} else return;
		const itemData = config.extractItemData(itemMatch);
		const { indentLevel, mainContent } = itemData;
		totalRaw = `${totalRaw}${currentLine}
`;
		const itemContent = [mainContent];
		i$1 += 1;
		while (i$1 < lines.length) {
			const nextLine = lines[i$1];
			if (nextLine.trim() === "") {
				const nextNonEmptyIndex = lines.slice(i$1 + 1).findIndex((l) => l.trim() !== "");
				if (nextNonEmptyIndex === -1) break;
				if ((((_b = (_a = lines[i$1 + 1 + nextNonEmptyIndex].match(/^(\s*)/)) == null ? void 0 : _a[1]) == null ? void 0 : _b.length) || 0) > indentLevel) {
					itemContent.push(nextLine);
					totalRaw = `${totalRaw}${nextLine}
`;
					i$1 += 1;
					continue;
				} else break;
			}
			if ((((_d = (_c = nextLine.match(/^(\s*)/)) == null ? void 0 : _c[1]) == null ? void 0 : _d.length) || 0) > indentLevel) {
				itemContent.push(nextLine);
				totalRaw = `${totalRaw}${nextLine}
`;
				i$1 += 1;
			} else break;
		}
		let nestedTokens;
		const nestedContent = itemContent.slice(1);
		if (nestedContent.length > 0) {
			const dedentedNested = nestedContent.map((nestedLine) => nestedLine.slice(indentLevel + baseIndentSize)).join("\n");
			if (dedentedNested.trim()) if (config.customNestedParser) nestedTokens = config.customNestedParser(dedentedNested);
			else nestedTokens = lexer.blockTokens(dedentedNested);
		}
		const token = config.createToken(itemData, nestedTokens);
		items.push(token);
	}
	if (items.length === 0) return;
	return {
		items,
		raw: totalRaw
	};
}
function renderNestedMarkdownContent(node, h2, prefixOrGenerator, ctx) {
	if (!node || !Array.isArray(node.content)) return "";
	const prefix = typeof prefixOrGenerator === "function" ? prefixOrGenerator(ctx) : prefixOrGenerator;
	const [content, ...children] = node.content;
	let output = `${prefix}${h2.renderChildren([content])}`;
	if (children && children.length > 0) children.forEach((child, index) => {
		var _a, _b;
		const childContent = (_b = (_a = h2.renderChild) == null ? void 0 : _a.call(h2, child, index + 1)) != null ? _b : h2.renderChildren([child]);
		if (childContent !== void 0 && childContent !== null) {
			const indentedChild = childContent.split("\n").map((line) => line ? h2.indent(line) : h2.indent("")).join("\n");
			output += child.type === "paragraph" ? `

${indentedChild}` : `
${indentedChild}`;
		}
	});
	return output;
}
var positionUpdateRegistries = /* @__PURE__ */ new WeakMap();
function schedulePositionCheck(editor, callback) {
	let registry = positionUpdateRegistries.get(editor);
	if (!registry) {
		const newRegistry = {
			callbacks: /* @__PURE__ */ new Set(),
			rafId: null,
			handler: () => {
				if (newRegistry.rafId !== null) cancelAnimationFrame(newRegistry.rafId);
				newRegistry.rafId = requestAnimationFrame(() => {
					newRegistry.rafId = null;
					newRegistry.callbacks.forEach((cb) => cb());
				});
			}
		};
		positionUpdateRegistries.set(editor, newRegistry);
		editor.on("update", newRegistry.handler);
		registry = newRegistry;
	}
	registry.callbacks.add(callback);
}
function cancelPositionCheck(editor, callback) {
	const registry = positionUpdateRegistries.get(editor);
	if (!registry) return;
	registry.callbacks.delete(callback);
	if (registry.callbacks.size === 0) {
		if (registry.rafId !== null) cancelAnimationFrame(registry.rafId);
		editor.off("update", registry.handler);
		positionUpdateRegistries.delete(editor);
	}
}
function updateMarkViewAttributes(checkMark, editor, attrs = {}) {
	const { state } = editor;
	const { doc: doc$2, tr: tr$1 } = state;
	const thisMark = checkMark;
	doc$2.descendants((node, pos) => {
		const from = tr$1.mapping.map(pos);
		const to = tr$1.mapping.map(pos) + node.nodeSize;
		let foundMark = null;
		node.marks.forEach((mark) => {
			if (mark !== thisMark) return false;
			foundMark = mark;
		});
		if (!foundMark) return;
		let needsUpdate = false;
		Object.keys(attrs).forEach((k$1) => {
			if (attrs[k$1] !== foundMark.attrs[k$1]) needsUpdate = true;
		});
		if (needsUpdate) {
			const updatedMark = checkMark.type.create({
				...checkMark.attrs,
				...attrs
			});
			tr$1.removeMark(from, to, checkMark.type);
			tr$1.addMark(from, to, updatedMark);
		}
	});
	if (tr$1.docChanged) editor.view.dispatch(tr$1);
}
var Node3 = class _Node extends Extendable {
	constructor() {
		super(...arguments);
		this.type = "node";
	}
	static create(config = {}) {
		return new _Node(typeof config === "function" ? config() : config);
	}
	configure(options) {
		return super.configure(options);
	}
	extend(extendedConfig) {
		const resolvedConfig = typeof extendedConfig === "function" ? extendedConfig() : extendedConfig;
		return super.extend(resolvedConfig);
	}
};
var NodeView = class {
	constructor(component, props, options) {
		this.isDragging = false;
		this.component = component;
		this.editor = props.editor;
		this.options = {
			stopEvent: null,
			ignoreMutation: null,
			...options
		};
		this.extension = props.extension;
		this.node = props.node;
		this.decorations = props.decorations;
		this.innerDecorations = props.innerDecorations;
		this.view = props.view;
		this.HTMLAttributes = props.HTMLAttributes;
		this.getPos = props.getPos;
		this.mount();
	}
	mount() {}
	get dom() {
		return this.editor.view.dom;
	}
	get contentDOM() {
		return null;
	}
	onDragStart(event) {
		var _a, _b, _c, _d, _e$1, _f, _g;
		const { view } = this.editor;
		const target = event.target;
		const dragHandle = target.nodeType === 3 ? (_a = target.parentElement) == null ? void 0 : _a.closest("[data-drag-handle]") : target.closest("[data-drag-handle]");
		if (!this.dom || ((_b = this.contentDOM) == null ? void 0 : _b.contains(target)) || !dragHandle) return;
		let x$1 = 0;
		let y$1 = 0;
		if (this.dom !== dragHandle) {
			const domBox = this.dom.getBoundingClientRect();
			const handleBox = dragHandle.getBoundingClientRect();
			const offsetX = (_d = event.offsetX) != null ? _d : (_c = event.nativeEvent) == null ? void 0 : _c.offsetX;
			const offsetY = (_f = event.offsetY) != null ? _f : (_e$1 = event.nativeEvent) == null ? void 0 : _e$1.offsetY;
			x$1 = handleBox.x - domBox.x + offsetX;
			y$1 = handleBox.y - domBox.y + offsetY;
		}
		const clonedNode = this.dom.cloneNode(true);
		try {
			const domBox = this.dom.getBoundingClientRect();
			clonedNode.style.width = `${Math.round(domBox.width)}px`;
			clonedNode.style.height = `${Math.round(domBox.height)}px`;
			clonedNode.style.boxSizing = "border-box";
			clonedNode.style.pointerEvents = "none";
		} catch {}
		let dragImageWrapper = null;
		try {
			dragImageWrapper = document.createElement("div");
			dragImageWrapper.style.position = "absolute";
			dragImageWrapper.style.top = "-9999px";
			dragImageWrapper.style.left = "-9999px";
			dragImageWrapper.style.pointerEvents = "none";
			dragImageWrapper.appendChild(clonedNode);
			document.body.appendChild(dragImageWrapper);
			(_g = event.dataTransfer) == null || _g.setDragImage(clonedNode, x$1, y$1);
		} finally {
			if (dragImageWrapper) setTimeout(() => {
				try {
					dragImageWrapper?.remove();
				} catch {}
			}, 0);
		}
		const pos = this.getPos();
		if (typeof pos !== "number") return;
		const selection = NodeSelection.create(view.state.doc, pos);
		const transaction = view.state.tr.setSelection(selection);
		view.dispatch(transaction);
	}
	stopEvent(event) {
		var _a;
		if (!this.dom) return false;
		if (typeof this.options.stopEvent === "function") return this.options.stopEvent({ event });
		const target = event.target;
		if (!(this.dom.contains(target) && !((_a = this.contentDOM) == null ? void 0 : _a.contains(target)))) return false;
		const isDragEvent = event.type.startsWith("drag");
		const isDragOverEnterEvent = event.type === "dragover" || event.type === "dragenter";
		const isDropEvent = event.type === "drop";
		if (([
			"INPUT",
			"BUTTON",
			"SELECT",
			"TEXTAREA"
		].includes(target.tagName) || target.isContentEditable) && !isDropEvent && !isDragEvent) return true;
		const { isEditable } = this.editor;
		const { isDragging } = this;
		const isDraggable = !!this.node.type.spec.draggable;
		const isSelectable = NodeSelection.isSelectable(this.node);
		const isCopyEvent = event.type === "copy";
		const isPasteEvent = event.type === "paste";
		const isCutEvent = event.type === "cut";
		const isClickEvent = event.type === "mousedown";
		if (!isDraggable && isSelectable && isDragEvent && event.target === this.dom) event.preventDefault();
		if (isDraggable && isDragEvent && !isDragging && event.target === this.dom) {
			event.preventDefault();
			return false;
		}
		if (isDraggable && isEditable && !isDragging && isClickEvent) {
			const dragHandle = target.closest("[data-drag-handle]");
			if (dragHandle && (this.dom === dragHandle || this.dom.contains(dragHandle))) {
				this.isDragging = true;
				document.addEventListener("dragend", () => {
					this.isDragging = false;
				}, { once: true });
				document.addEventListener("drop", () => {
					this.isDragging = false;
				}, { once: true });
				document.addEventListener("mouseup", () => {
					this.isDragging = false;
				}, { once: true });
			}
		}
		if (isDragging || isDragOverEnterEvent || isDropEvent || isCopyEvent || isPasteEvent || isCutEvent || isClickEvent && isSelectable) return false;
		return true;
	}
	ignoreMutation(mutation) {
		if (!this.dom || !this.contentDOM) return true;
		if (typeof this.options.ignoreMutation === "function") return this.options.ignoreMutation({ mutation });
		if (this.node.isLeaf || this.node.isAtom) return true;
		if (mutation.type === "selection") return false;
		if (this.dom.contains(mutation.target) && mutation.type === "childList" && (isiOS() || isAndroid()) && this.editor.isFocused) {
			if ([...Array.from(mutation.addedNodes), ...Array.from(mutation.removedNodes)].every((node) => node.isContentEditable)) return false;
		}
		if (this.contentDOM === mutation.target && mutation.type === "attributes") return true;
		if (this.contentDOM.contains(mutation.target)) return false;
		return true;
	}
	updateAttributes(attributes) {
		this.editor.commands.command(({ tr: tr$1 }) => {
			const pos = this.getPos();
			if (typeof pos !== "number") return false;
			tr$1.setNodeMarkup(pos, void 0, {
				...this.node.attrs,
				...attributes
			});
			return true;
		});
	}
	deleteNode() {
		const from = this.getPos();
		if (typeof from !== "number") return;
		const to = from + this.node.nodeSize;
		this.editor.commands.deleteRange({
			from,
			to
		});
	}
};
function markPasteRule(config) {
	return new PasteRule({
		find: config.find,
		handler: ({ state, range, match, pasteEvent }) => {
			const attributes = callOrReturn(config.getAttributes, void 0, match, pasteEvent);
			if (attributes === false || attributes === null) return null;
			const { tr: tr$1 } = state;
			const captureGroup = match[match.length - 1];
			const fullMatch = match[0];
			let markEnd = range.to;
			if (captureGroup) {
				const startSpaces = fullMatch.search(/\S/);
				const textStart = range.from + fullMatch.indexOf(captureGroup);
				const textEnd = textStart + captureGroup.length;
				if (getMarksBetween(range.from, range.to, state.doc).filter((item) => {
					return item.mark.type.excluded.find((type) => type === config.type && type !== item.mark.type);
				}).filter((item) => item.to > textStart).length) return null;
				if (textEnd < range.to) tr$1.delete(textEnd, range.to);
				if (textStart > range.from) tr$1.delete(range.from + startSpaces, textStart);
				markEnd = range.from + startSpaces + captureGroup.length;
				tr$1.addMark(range.from + startSpaces, markEnd, config.type.create(attributes || {}));
				if (!(match.index !== void 0 && match.input !== void 0 && match.index + match[0].length >= match.input.length)) tr$1.removeStoredMark(config.type);
			}
		}
	});
}
var { getOwnPropertyNames, getOwnPropertySymbols } = Object;
var { hasOwnProperty } = Object.prototype;
function combineComparators(comparatorA, comparatorB) {
	return function isEqual(a, b$1, state) {
		return comparatorA(a, b$1, state) && comparatorB(a, b$1, state);
	};
}
function createIsCircular(areItemsEqual) {
	return function isCircular(a, b$1, state) {
		if (!a || !b$1 || typeof a !== "object" || typeof b$1 !== "object") return areItemsEqual(a, b$1, state);
		const { cache } = state;
		const cachedA = cache.get(a);
		const cachedB = cache.get(b$1);
		if (cachedA && cachedB) return cachedA === b$1 && cachedB === a;
		cache.set(a, b$1);
		cache.set(b$1, a);
		const result = areItemsEqual(a, b$1, state);
		cache.delete(a);
		cache.delete(b$1);
		return result;
	};
}
function getShortTag(value) {
	return value != null ? value[Symbol.toStringTag] : void 0;
}
function getStrictProperties(object) {
	return getOwnPropertyNames(object).concat(getOwnPropertySymbols(object));
}
var hasOwn = Object.hasOwn || ((object, property) => hasOwnProperty.call(object, property));
function sameValueZeroEqual(a, b$1) {
	return a === b$1 || !a && !b$1 && a !== a && b$1 !== b$1;
}
var PREACT_VNODE = "__v";
var PREACT_OWNER = "__o";
var REACT_OWNER = "_owner";
var { getOwnPropertyDescriptor, keys } = Object;
function areArrayBuffersEqual(a, b$1) {
	return a.byteLength === b$1.byteLength && areTypedArraysEqual(new Uint8Array(a), new Uint8Array(b$1));
}
function areArraysEqual(a, b$1, state) {
	let index = a.length;
	if (b$1.length !== index) return false;
	while (index-- > 0) if (!state.equals(a[index], b$1[index], index, index, a, b$1, state)) return false;
	return true;
}
function areDataViewsEqual(a, b$1) {
	return a.byteLength === b$1.byteLength && areTypedArraysEqual(new Uint8Array(a.buffer, a.byteOffset, a.byteLength), new Uint8Array(b$1.buffer, b$1.byteOffset, b$1.byteLength));
}
function areDatesEqual(a, b$1) {
	return sameValueZeroEqual(a.getTime(), b$1.getTime());
}
function areErrorsEqual(a, b$1) {
	return a.name === b$1.name && a.message === b$1.message && a.cause === b$1.cause && a.stack === b$1.stack;
}
function areFunctionsEqual(a, b$1) {
	return a === b$1;
}
function areMapsEqual(a, b$1, state) {
	const size = a.size;
	if (size !== b$1.size) return false;
	if (!size) return true;
	const matchedIndices = new Array(size);
	const aIterable = a.entries();
	let aResult;
	let bResult;
	let index = 0;
	while (aResult = aIterable.next()) {
		if (aResult.done) break;
		const bIterable = b$1.entries();
		let hasMatch = false;
		let matchIndex = 0;
		while (bResult = bIterable.next()) {
			if (bResult.done) break;
			if (matchedIndices[matchIndex]) {
				matchIndex++;
				continue;
			}
			const aEntry = aResult.value;
			const bEntry = bResult.value;
			if (state.equals(aEntry[0], bEntry[0], index, matchIndex, a, b$1, state) && state.equals(aEntry[1], bEntry[1], aEntry[0], bEntry[0], a, b$1, state)) {
				hasMatch = matchedIndices[matchIndex] = true;
				break;
			}
			matchIndex++;
		}
		if (!hasMatch) return false;
		index++;
	}
	return true;
}
var areNumbersEqual = sameValueZeroEqual;
function areObjectsEqual(a, b$1, state) {
	const properties = keys(a);
	let index = properties.length;
	if (keys(b$1).length !== index) return false;
	while (index-- > 0) if (!isPropertyEqual(a, b$1, state, properties[index])) return false;
	return true;
}
function areObjectsEqualStrict(a, b$1, state) {
	const properties = getStrictProperties(a);
	let index = properties.length;
	if (getStrictProperties(b$1).length !== index) return false;
	let property;
	let descriptorA;
	let descriptorB;
	while (index-- > 0) {
		property = properties[index];
		if (!isPropertyEqual(a, b$1, state, property)) return false;
		descriptorA = getOwnPropertyDescriptor(a, property);
		descriptorB = getOwnPropertyDescriptor(b$1, property);
		if ((descriptorA || descriptorB) && (!descriptorA || !descriptorB || descriptorA.configurable !== descriptorB.configurable || descriptorA.enumerable !== descriptorB.enumerable || descriptorA.writable !== descriptorB.writable)) return false;
	}
	return true;
}
function arePrimitiveWrappersEqual(a, b$1) {
	return sameValueZeroEqual(a.valueOf(), b$1.valueOf());
}
function areRegExpsEqual(a, b$1) {
	return a.source === b$1.source && a.flags === b$1.flags;
}
function areSetsEqual(a, b$1, state) {
	const size = a.size;
	if (size !== b$1.size) return false;
	if (!size) return true;
	const matchedIndices = new Array(size);
	const aIterable = a.values();
	let aResult;
	let bResult;
	while (aResult = aIterable.next()) {
		if (aResult.done) break;
		const bIterable = b$1.values();
		let hasMatch = false;
		let matchIndex = 0;
		while (bResult = bIterable.next()) {
			if (bResult.done) break;
			if (!matchedIndices[matchIndex] && state.equals(aResult.value, bResult.value, aResult.value, bResult.value, a, b$1, state)) {
				hasMatch = matchedIndices[matchIndex] = true;
				break;
			}
			matchIndex++;
		}
		if (!hasMatch) return false;
	}
	return true;
}
function areTypedArraysEqual(a, b$1) {
	let index = a.byteLength;
	if (b$1.byteLength !== index || a.byteOffset !== b$1.byteOffset) return false;
	while (index-- > 0) if (a[index] !== b$1[index]) return false;
	return true;
}
function areUrlsEqual(a, b$1) {
	return a.hostname === b$1.hostname && a.pathname === b$1.pathname && a.protocol === b$1.protocol && a.port === b$1.port && a.hash === b$1.hash && a.username === b$1.username && a.password === b$1.password;
}
function isPropertyEqual(a, b$1, state, property) {
	if ((property === REACT_OWNER || property === PREACT_OWNER || property === PREACT_VNODE) && (a.$$typeof || b$1.$$typeof)) return true;
	return hasOwn(b$1, property) && state.equals(a[property], b$1[property], property, property, a, b$1, state);
}
var ARRAY_BUFFER_TAG = "[object ArrayBuffer]";
var ARGUMENTS_TAG = "[object Arguments]";
var BOOLEAN_TAG = "[object Boolean]";
var DATA_VIEW_TAG = "[object DataView]";
var DATE_TAG = "[object Date]";
var ERROR_TAG = "[object Error]";
var MAP_TAG = "[object Map]";
var NUMBER_TAG = "[object Number]";
var OBJECT_TAG = "[object Object]";
var REG_EXP_TAG = "[object RegExp]";
var SET_TAG = "[object Set]";
var STRING_TAG = "[object String]";
var TYPED_ARRAY_TAGS = {
	"[object Int8Array]": true,
	"[object Uint8Array]": true,
	"[object Uint8ClampedArray]": true,
	"[object Int16Array]": true,
	"[object Uint16Array]": true,
	"[object Int32Array]": true,
	"[object Uint32Array]": true,
	"[object Float16Array]": true,
	"[object Float32Array]": true,
	"[object Float64Array]": true,
	"[object BigInt64Array]": true,
	"[object BigUint64Array]": true
};
var URL_TAG = "[object URL]";
var toString = Object.prototype.toString;
function createEqualityComparator({ areArrayBuffersEqual: areArrayBuffersEqual$1, areArraysEqual: areArraysEqual$1, areDataViewsEqual: areDataViewsEqual$1, areDatesEqual: areDatesEqual$1, areErrorsEqual: areErrorsEqual$1, areFunctionsEqual: areFunctionsEqual$1, areMapsEqual: areMapsEqual$1, areNumbersEqual: areNumbersEqual$1, areObjectsEqual: areObjectsEqual$1, arePrimitiveWrappersEqual: arePrimitiveWrappersEqual$1, areRegExpsEqual: areRegExpsEqual$1, areSetsEqual: areSetsEqual$1, areTypedArraysEqual: areTypedArraysEqual$1, areUrlsEqual: areUrlsEqual$1, unknownTagComparators }) {
	return function comparator(a, b$1, state) {
		if (a === b$1) return true;
		if (a == null || b$1 == null) return false;
		const type = typeof a;
		if (type !== typeof b$1) return false;
		if (type !== "object") {
			if (type === "number") return areNumbersEqual$1(a, b$1, state);
			if (type === "function") return areFunctionsEqual$1(a, b$1, state);
			return false;
		}
		const constructor = a.constructor;
		if (constructor !== b$1.constructor) return false;
		if (constructor === Object) return areObjectsEqual$1(a, b$1, state);
		if (Array.isArray(a)) return areArraysEqual$1(a, b$1, state);
		if (constructor === Date) return areDatesEqual$1(a, b$1, state);
		if (constructor === RegExp) return areRegExpsEqual$1(a, b$1, state);
		if (constructor === Map) return areMapsEqual$1(a, b$1, state);
		if (constructor === Set) return areSetsEqual$1(a, b$1, state);
		const tag = toString.call(a);
		if (tag === DATE_TAG) return areDatesEqual$1(a, b$1, state);
		if (tag === REG_EXP_TAG) return areRegExpsEqual$1(a, b$1, state);
		if (tag === MAP_TAG) return areMapsEqual$1(a, b$1, state);
		if (tag === SET_TAG) return areSetsEqual$1(a, b$1, state);
		if (tag === OBJECT_TAG) return typeof a.then !== "function" && typeof b$1.then !== "function" && areObjectsEqual$1(a, b$1, state);
		if (tag === URL_TAG) return areUrlsEqual$1(a, b$1, state);
		if (tag === ERROR_TAG) return areErrorsEqual$1(a, b$1, state);
		if (tag === ARGUMENTS_TAG) return areObjectsEqual$1(a, b$1, state);
		if (TYPED_ARRAY_TAGS[tag]) return areTypedArraysEqual$1(a, b$1, state);
		if (tag === ARRAY_BUFFER_TAG) return areArrayBuffersEqual$1(a, b$1, state);
		if (tag === DATA_VIEW_TAG) return areDataViewsEqual$1(a, b$1, state);
		if (tag === BOOLEAN_TAG || tag === NUMBER_TAG || tag === STRING_TAG) return arePrimitiveWrappersEqual$1(a, b$1, state);
		if (unknownTagComparators) {
			let unknownTagComparator = unknownTagComparators[tag];
			if (!unknownTagComparator) {
				const shortTag = getShortTag(a);
				if (shortTag) unknownTagComparator = unknownTagComparators[shortTag];
			}
			if (unknownTagComparator) return unknownTagComparator(a, b$1, state);
		}
		return false;
	};
}
function createEqualityComparatorConfig({ circular, createCustomConfig, strict }) {
	let config = {
		areArrayBuffersEqual,
		areArraysEqual: strict ? areObjectsEqualStrict : areArraysEqual,
		areDataViewsEqual,
		areDatesEqual,
		areErrorsEqual,
		areFunctionsEqual,
		areMapsEqual: strict ? combineComparators(areMapsEqual, areObjectsEqualStrict) : areMapsEqual,
		areNumbersEqual,
		areObjectsEqual: strict ? areObjectsEqualStrict : areObjectsEqual,
		arePrimitiveWrappersEqual,
		areRegExpsEqual,
		areSetsEqual: strict ? combineComparators(areSetsEqual, areObjectsEqualStrict) : areSetsEqual,
		areTypedArraysEqual: strict ? combineComparators(areTypedArraysEqual, areObjectsEqualStrict) : areTypedArraysEqual,
		areUrlsEqual,
		unknownTagComparators: void 0
	};
	if (createCustomConfig) config = Object.assign({}, config, createCustomConfig(config));
	if (circular) {
		const areArraysEqual$1 = createIsCircular(config.areArraysEqual);
		const areMapsEqual$1 = createIsCircular(config.areMapsEqual);
		const areObjectsEqual$1 = createIsCircular(config.areObjectsEqual);
		const areSetsEqual$1 = createIsCircular(config.areSetsEqual);
		config = Object.assign({}, config, {
			areArraysEqual: areArraysEqual$1,
			areMapsEqual: areMapsEqual$1,
			areObjectsEqual: areObjectsEqual$1,
			areSetsEqual: areSetsEqual$1
		});
	}
	return config;
}
function createInternalEqualityComparator(compare) {
	return function(a, b$1, _indexOrKeyA, _indexOrKeyB, _parentA, _parentB, state) {
		return compare(a, b$1, state);
	};
}
function createIsEqual({ circular, comparator, createState, equals, strict }) {
	if (createState) return function isEqual(a, b$1) {
		const { cache = circular ? /* @__PURE__ */ new WeakMap() : void 0, meta } = createState();
		return comparator(a, b$1, {
			cache,
			equals,
			meta,
			strict
		});
	};
	if (circular) return function isEqual(a, b$1) {
		return comparator(a, b$1, {
			cache: /* @__PURE__ */ new WeakMap(),
			equals,
			meta: void 0,
			strict
		});
	};
	const state = {
		cache: void 0,
		equals,
		meta: void 0,
		strict
	};
	return function isEqual(a, b$1) {
		return comparator(a, b$1, state);
	};
}
var deepEqual = createCustomEqual();
createCustomEqual({ strict: true });
createCustomEqual({ circular: true });
createCustomEqual({
	circular: true,
	strict: true
});
createCustomEqual({ createInternalComparator: () => sameValueZeroEqual });
createCustomEqual({
	strict: true,
	createInternalComparator: () => sameValueZeroEqual
});
createCustomEqual({
	circular: true,
	createInternalComparator: () => sameValueZeroEqual
});
createCustomEqual({
	circular: true,
	createInternalComparator: () => sameValueZeroEqual,
	strict: true
});
function createCustomEqual(options = {}) {
	const { circular = false, createInternalComparator: createCustomInternalComparator, createState, strict = false } = options;
	const comparator = createEqualityComparator(createEqualityComparatorConfig(options));
	return createIsEqual({
		circular,
		comparator,
		createState,
		equals: createCustomInternalComparator ? createCustomInternalComparator(comparator) : createInternalEqualityComparator(comparator),
		strict
	});
}
/**
* @license React
* use-sync-external-store-shim/with-selector.production.js
*
* Copyright (c) Meta Platforms, Inc. and affiliates.
*
* This source code is licensed under the MIT license found in the
* LICENSE file in the root directory of this source tree.
*/
var require_with_selector_production = /* @__PURE__ */ __commonJSMin(((exports) => {
	var React$2 = require_react(), shim = require_shim();
	function is(x$1, y$1) {
		return x$1 === y$1 && (0 !== x$1 || 1 / x$1 === 1 / y$1) || x$1 !== x$1 && y$1 !== y$1;
	}
	var objectIs = "function" === typeof Object.is ? Object.is : is, useSyncExternalStore$1 = shim.useSyncExternalStore, useRef$2 = React$2.useRef, useEffect$1 = React$2.useEffect, useMemo$1 = React$2.useMemo, useDebugValue$1 = React$2.useDebugValue;
	exports.useSyncExternalStoreWithSelector = function(subscribe, getSnapshot, getServerSnapshot, selector, isEqual) {
		var instRef = useRef$2(null);
		if (null === instRef.current) {
			var inst = {
				hasValue: !1,
				value: null
			};
			instRef.current = inst;
		} else inst = instRef.current;
		instRef = useMemo$1(function() {
			function memoizedSelector(nextSnapshot) {
				if (!hasMemo) {
					hasMemo = !0;
					memoizedSnapshot = nextSnapshot;
					nextSnapshot = selector(nextSnapshot);
					if (void 0 !== isEqual && inst.hasValue) {
						var currentSelection = inst.value;
						if (isEqual(currentSelection, nextSnapshot)) return memoizedSelection = currentSelection;
					}
					return memoizedSelection = nextSnapshot;
				}
				currentSelection = memoizedSelection;
				if (objectIs(memoizedSnapshot, nextSnapshot)) return currentSelection;
				var nextSelection = selector(nextSnapshot);
				if (void 0 !== isEqual && isEqual(currentSelection, nextSelection)) return memoizedSnapshot = nextSnapshot, currentSelection;
				memoizedSnapshot = nextSnapshot;
				return memoizedSelection = nextSelection;
			}
			var hasMemo = !1, memoizedSnapshot, memoizedSelection, maybeGetServerSnapshot = void 0 === getServerSnapshot ? null : getServerSnapshot;
			return [function() {
				return memoizedSelector(getSnapshot());
			}, null === maybeGetServerSnapshot ? void 0 : function() {
				return memoizedSelector(maybeGetServerSnapshot());
			}];
		}, [
			getSnapshot,
			getServerSnapshot,
			selector,
			isEqual
		]);
		var value = useSyncExternalStore$1(subscribe, instRef[0], instRef[1]);
		useEffect$1(function() {
			inst.hasValue = !0;
			inst.value = value;
		}, [value]);
		useDebugValue$1(value);
		return value;
	};
}));
var require_with_selector = /* @__PURE__ */ __commonJSMin(((exports, module) => {
	module.exports = require_with_selector_production();
}));
var import_react = /* @__PURE__ */ __toESM(require_react(), 1);
var import_react_dom = /* @__PURE__ */ __toESM(require_react_dom(), 1);
var import_shim = require_shim();
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime(), 1);
var import_shim$1 = require_shim();
var import_with_selector = require_with_selector();
var import_react_dom$1 = require_react_dom();
var mergeRefs = (...refs) => {
	return (node) => {
		refs.forEach((ref) => {
			if (typeof ref === "function") ref(node);
			else if (ref) ref.current = node;
		});
	};
};
var Portals = ({ contentComponent }) => {
	const renderers = (0, import_shim.useSyncExternalStore)(contentComponent.subscribe, contentComponent.getSnapshot, contentComponent.getServerSnapshot);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(import_jsx_runtime.Fragment, { children: Object.values(renderers) });
};
function getInstance() {
	const subscribers = /* @__PURE__ */ new Set();
	let renderers = {};
	return {
		subscribe(callback) {
			subscribers.add(callback);
			return () => {
				subscribers.delete(callback);
			};
		},
		getSnapshot() {
			return renderers;
		},
		getServerSnapshot() {
			return renderers;
		},
		setRenderer(id, renderer) {
			renderers = {
				...renderers,
				[id]: import_react_dom.createPortal(renderer.reactElement, renderer.element, id)
			};
			subscribers.forEach((subscriber) => subscriber());
		},
		removeRenderer(id) {
			const nextRenderers = { ...renderers };
			delete nextRenderers[id];
			renderers = nextRenderers;
			subscribers.forEach((subscriber) => subscriber());
		}
	};
}
var PureEditorContent = class extends import_react.Component {
	constructor(props) {
		super(props);
		this.editorContentRef = import_react.createRef();
	}
	componentDidMount() {
		this.init();
	}
	componentDidUpdate() {
		this.init();
	}
	init() {
		var _a;
		const editor = this.props.editor;
		if (editor && !editor.isDestroyed && ((_a = editor.view.dom) == null ? void 0 : _a.parentNode)) {
			if (editor.contentComponent) return;
			const element = this.editorContentRef.current;
			element.append(...editor.view.dom.parentNode.childNodes);
			editor.setOptions({ element });
			editor.contentComponent = getInstance();
			editor.createNodeViews();
			editor.isEditorContentInitialized = true;
			this.forceUpdate();
		}
	}
	componentWillUnmount() {
		var _a;
		const editor = this.props.editor;
		if (!editor) return;
		editor.isEditorContentInitialized = false;
		if (!editor.isDestroyed) editor.view.setProps({ nodeViews: {} });
		editor.contentComponent = null;
		try {
			if (!((_a = editor.view.dom) == null ? void 0 : _a.parentNode)) return;
			const newElement = document.createElement("div");
			newElement.append(...editor.view.dom.parentNode.childNodes);
			editor.setOptions({ element: newElement });
		} catch {}
	}
	render() {
		const { editor, innerRef, ...rest } = this.props;
		return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			ref: mergeRefs(innerRef, this.editorContentRef),
			...rest
		}), (editor == null ? void 0 : editor.contentComponent) && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Portals, { contentComponent: editor.contentComponent })] });
	}
};
var EditorContentWithKey = (0, import_react.forwardRef)((props, ref) => {
	const key = import_react.useMemo(() => {
		return Math.floor(Math.random() * 4294967295).toString();
	}, [props.editor]);
	return import_react.createElement(PureEditorContent, {
		key,
		innerRef: ref,
		...props
	});
});
var EditorContent = import_react.memo(EditorContentWithKey);
var useIsomorphicLayoutEffect = typeof window !== "undefined" ? import_react.useLayoutEffect : import_react.useEffect;
var EditorStateManager = class {
	constructor(initialEditor) {
		this.transactionNumber = 0;
		this.lastTransactionNumber = 0;
		this.subscribers = /* @__PURE__ */ new Set();
		this.editor = initialEditor;
		this.lastSnapshot = {
			editor: initialEditor,
			transactionNumber: 0
		};
		this.getSnapshot = this.getSnapshot.bind(this);
		this.getServerSnapshot = this.getServerSnapshot.bind(this);
		this.watch = this.watch.bind(this);
		this.subscribe = this.subscribe.bind(this);
	}
	getSnapshot() {
		if (this.transactionNumber === this.lastTransactionNumber) return this.lastSnapshot;
		this.lastTransactionNumber = this.transactionNumber;
		this.lastSnapshot = {
			editor: this.editor,
			transactionNumber: this.transactionNumber
		};
		return this.lastSnapshot;
	}
	getServerSnapshot() {
		return {
			editor: null,
			transactionNumber: 0
		};
	}
	subscribe(callback) {
		this.subscribers.add(callback);
		return () => {
			this.subscribers.delete(callback);
		};
	}
	watch(nextEditor) {
		this.editor = nextEditor;
		if (this.editor) {
			const fn = () => {
				this.transactionNumber += 1;
				this.subscribers.forEach((callback) => callback());
			};
			const currentEditor = this.editor;
			currentEditor.on("transaction", fn);
			return () => {
				currentEditor.off("transaction", fn);
			};
		}
	}
};
function useEditorState(options) {
	var _a;
	const [editorStateManager] = (0, import_react.useState)(() => new EditorStateManager(options.editor));
	const selectedState = (0, import_with_selector.useSyncExternalStoreWithSelector)(editorStateManager.subscribe, editorStateManager.getSnapshot, editorStateManager.getServerSnapshot, options.selector, (_a = options.equalityFn) != null ? _a : deepEqual);
	useIsomorphicLayoutEffect(() => {
		return editorStateManager.watch(options.editor);
	}, [options.editor, editorStateManager]);
	(0, import_react.useDebugValue)(selectedState);
	return selectedState;
}
var isDev = false;
var isSSR = typeof window === "undefined";
var isNext = isSSR || Boolean(typeof window !== "undefined" && window.next);
var EditorInstanceManager = class _EditorInstanceManager {
	constructor(options) {
		this.editor = null;
		this.subscriptions = /* @__PURE__ */ new Set();
		this.isComponentMounted = false;
		this.previousDeps = null;
		this.instanceId = "";
		this.options = options;
		this.subscriptions = /* @__PURE__ */ new Set();
		this.setEditor(this.getInitialEditor());
		this.scheduleDestroy();
		this.getEditor = this.getEditor.bind(this);
		this.getServerSnapshot = this.getServerSnapshot.bind(this);
		this.subscribe = this.subscribe.bind(this);
		this.refreshEditorInstance = this.refreshEditorInstance.bind(this);
		this.scheduleDestroy = this.scheduleDestroy.bind(this);
		this.onRender = this.onRender.bind(this);
		this.createEditor = this.createEditor.bind(this);
	}
	setEditor(editor) {
		this.editor = editor;
		this.instanceId = Math.random().toString(36).slice(2, 9);
		this.subscriptions.forEach((cb) => cb());
	}
	getInitialEditor() {
		if (this.options.current.immediatelyRender === void 0) {
			if (isSSR || isNext) {
				if (isDev) throw new Error("Tiptap Error: SSR has been detected, please set `immediatelyRender` explicitly to `false` to avoid hydration mismatches.");
				return null;
			}
			return this.createEditor();
		}
		if (this.options.current.immediatelyRender && isSSR && isDev) throw new Error("Tiptap Error: SSR has been detected, and `immediatelyRender` has been set to `true` this is an unsupported configuration that may result in errors, explicitly set `immediatelyRender` to `false` to avoid hydration mismatches.");
		if (this.options.current.immediatelyRender) return this.createEditor();
		return null;
	}
	createEditor() {
		return new Editor({
			...this.options.current,
			onBeforeCreate: (...args) => {
				var _a, _b;
				return (_b = (_a = this.options.current).onBeforeCreate) == null ? void 0 : _b.call(_a, ...args);
			},
			onBlur: (...args) => {
				var _a, _b;
				return (_b = (_a = this.options.current).onBlur) == null ? void 0 : _b.call(_a, ...args);
			},
			onCreate: (...args) => {
				var _a, _b;
				return (_b = (_a = this.options.current).onCreate) == null ? void 0 : _b.call(_a, ...args);
			},
			onDestroy: (...args) => {
				var _a, _b;
				return (_b = (_a = this.options.current).onDestroy) == null ? void 0 : _b.call(_a, ...args);
			},
			onFocus: (...args) => {
				var _a, _b;
				return (_b = (_a = this.options.current).onFocus) == null ? void 0 : _b.call(_a, ...args);
			},
			onSelectionUpdate: (...args) => {
				var _a, _b;
				return (_b = (_a = this.options.current).onSelectionUpdate) == null ? void 0 : _b.call(_a, ...args);
			},
			onTransaction: (...args) => {
				var _a, _b;
				return (_b = (_a = this.options.current).onTransaction) == null ? void 0 : _b.call(_a, ...args);
			},
			onUpdate: (...args) => {
				var _a, _b;
				return (_b = (_a = this.options.current).onUpdate) == null ? void 0 : _b.call(_a, ...args);
			},
			onContentError: (...args) => {
				var _a, _b;
				return (_b = (_a = this.options.current).onContentError) == null ? void 0 : _b.call(_a, ...args);
			},
			onDrop: (...args) => {
				var _a, _b;
				return (_b = (_a = this.options.current).onDrop) == null ? void 0 : _b.call(_a, ...args);
			},
			onPaste: (...args) => {
				var _a, _b;
				return (_b = (_a = this.options.current).onPaste) == null ? void 0 : _b.call(_a, ...args);
			},
			onDelete: (...args) => {
				var _a, _b;
				return (_b = (_a = this.options.current).onDelete) == null ? void 0 : _b.call(_a, ...args);
			}
		});
	}
	getEditor() {
		return this.editor;
	}
	getServerSnapshot() {
		return null;
	}
	subscribe(onStoreChange) {
		this.subscriptions.add(onStoreChange);
		return () => {
			this.subscriptions.delete(onStoreChange);
		};
	}
	static compareOptions(a, b$1) {
		return Object.keys(a).every((key) => {
			if ([
				"onCreate",
				"onBeforeCreate",
				"onDestroy",
				"onUpdate",
				"onTransaction",
				"onFocus",
				"onBlur",
				"onSelectionUpdate",
				"onContentError",
				"onDrop",
				"onPaste"
			].includes(key)) return true;
			if (key === "extensions" && a.extensions && b$1.extensions) {
				if (a.extensions.length !== b$1.extensions.length) return false;
				return a.extensions.every((extension, index) => {
					var _a;
					if (extension !== ((_a = b$1.extensions) == null ? void 0 : _a[index])) return false;
					return true;
				});
			}
			if (a[key] !== b$1[key]) return false;
			return true;
		});
	}
	onRender(deps) {
		return () => {
			this.isComponentMounted = true;
			clearTimeout(this.scheduledDestructionTimeout);
			if (this.editor && !this.editor.isDestroyed && deps.length === 0) {
				if (!_EditorInstanceManager.compareOptions(this.options.current, this.editor.options)) this.editor.setOptions({
					...this.options.current,
					editable: this.editor.isEditable
				});
			} else this.refreshEditorInstance(deps);
			return () => {
				this.isComponentMounted = false;
				this.scheduleDestroy();
			};
		};
	}
	refreshEditorInstance(deps) {
		if (this.editor && !this.editor.isDestroyed) {
			if (this.previousDeps === null) {
				this.previousDeps = deps;
				return;
			}
			if (this.previousDeps.length === deps.length && this.previousDeps.every((dep, index) => dep === deps[index])) return;
		}
		if (this.editor && !this.editor.isDestroyed) this.editor.destroy();
		this.setEditor(this.createEditor());
		this.previousDeps = deps;
	}
	scheduleDestroy() {
		const currentInstanceId = this.instanceId;
		const currentEditor = this.editor;
		this.scheduledDestructionTimeout = setTimeout(() => {
			if (this.isComponentMounted && this.instanceId === currentInstanceId) {
				if (currentEditor) currentEditor.setOptions(this.options.current);
				return;
			}
			if (currentEditor && !currentEditor.isDestroyed) {
				currentEditor.destroy();
				if (this.instanceId === currentInstanceId) this.setEditor(null);
			}
		}, 1);
	}
};
function useEditor(options = {}, deps = []) {
	const mostRecentOptions = (0, import_react.useRef)(options);
	mostRecentOptions.current = options;
	const [instanceManager] = (0, import_react.useState)(() => new EditorInstanceManager(mostRecentOptions));
	const editor = (0, import_shim$1.useSyncExternalStore)(instanceManager.subscribe, instanceManager.getEditor, instanceManager.getServerSnapshot);
	(0, import_react.useDebugValue)(editor);
	(0, import_react.useEffect)(instanceManager.onRender(deps));
	useEditorState({
		editor,
		selector: ({ transactionNumber }) => {
			if (options.shouldRerenderOnTransaction === false || options.shouldRerenderOnTransaction === void 0) return null;
			if (options.immediatelyRender && transactionNumber === 0) return 0;
			return transactionNumber + 1;
		}
	});
	return editor;
}
var EditorContext = (0, import_react.createContext)({ editor: null });
EditorContext.Consumer;
var ReactNodeViewContext = (0, import_react.createContext)({
	onDragStart: () => {},
	nodeViewContentChildren: void 0,
	nodeViewContentRef: () => {}
});
var useReactNodeView = () => (0, import_react.useContext)(ReactNodeViewContext);
function NodeViewContent({ as: Tag = "div", ...props }) {
	const { nodeViewContentRef, nodeViewContentChildren } = useReactNodeView();
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Tag, {
		...props,
		ref: nodeViewContentRef,
		"data-node-view-content": "",
		style: {
			whiteSpace: "pre-wrap",
			...props.style
		},
		children: nodeViewContentChildren
	});
}
var NodeViewWrapper = import_react.forwardRef((props, ref) => {
	const { onDragStart } = useReactNodeView();
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(props.as || "div", {
		...props,
		ref,
		"data-node-view-wrapper": "",
		onDragStart,
		style: {
			whiteSpace: "normal",
			...props.style
		}
	});
});
function isClassComponent(Component) {
	return !!(typeof Component === "function" && Component.prototype && Component.prototype.isReactComponent);
}
function isForwardRefComponent(Component) {
	return !!(typeof Component === "object" && Component.$$typeof && (Component.$$typeof.toString() === "Symbol(react.forward_ref)" || Component.$$typeof.description === "react.forward_ref"));
}
function isMemoComponent(Component) {
	return !!(typeof Component === "object" && Component.$$typeof && (Component.$$typeof.toString() === "Symbol(react.memo)" || Component.$$typeof.description === "react.memo"));
}
function canReceiveRef(Component) {
	if (isClassComponent(Component)) return true;
	if (isForwardRefComponent(Component)) return true;
	if (isMemoComponent(Component)) {
		const wrappedComponent = Component.type;
		if (wrappedComponent) return isClassComponent(wrappedComponent) || isForwardRefComponent(wrappedComponent);
	}
	return false;
}
function isReact19Plus() {
	try {
		if (import_react.version) return parseInt(import_react.version.split(".")[0], 10) >= 19;
	} catch {}
	return false;
}
var ReactRenderer = class {
	constructor(component, { editor, props = {}, as = "div", className = "" }) {
		this.ref = null;
		this.destroyed = false;
		this.id = Math.floor(Math.random() * 4294967295).toString();
		this.component = component;
		this.editor = editor;
		this.props = props;
		this.element = document.createElement(as);
		this.element.classList.add("react-renderer");
		if (className) this.element.classList.add(...className.split(" "));
		if (this.editor.isEditorContentInitialized) (0, import_react_dom$1.flushSync)(() => {
			this.render();
		});
		else queueMicrotask(() => {
			if (this.destroyed) return;
			this.render();
		});
	}
	render() {
		var _a;
		if (this.destroyed) return;
		const Component = this.component;
		const props = this.props;
		const editor = this.editor;
		const isReact19 = isReact19Plus();
		const componentCanReceiveRef = canReceiveRef(Component);
		const elementProps = { ...props };
		if (elementProps.ref && !(isReact19 || componentCanReceiveRef)) delete elementProps.ref;
		if (!elementProps.ref && (isReact19 || componentCanReceiveRef)) elementProps.ref = (ref) => {
			this.ref = ref;
		};
		this.reactElement = /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Component, { ...elementProps });
		(_a = editor == null ? void 0 : editor.contentComponent) == null || _a.setRenderer(this.id, this);
	}
	updateProps(props = {}) {
		if (this.destroyed) return;
		this.props = {
			...this.props,
			...props
		};
		this.render();
	}
	destroy() {
		var _a;
		this.destroyed = true;
		const editor = this.editor;
		(_a = editor == null ? void 0 : editor.contentComponent) == null || _a.removeRenderer(this.id);
		try {
			if (this.element && this.element.parentNode) this.element.parentNode.removeChild(this.element);
		} catch {}
	}
	updateAttributes(attributes) {
		Object.keys(attributes).forEach((key) => {
			this.element.setAttribute(key, attributes[key]);
		});
	}
};
import_react.createContext({ markViewContentRef: () => {} });
var ReactNodeView = class extends NodeView {
	constructor(component, props, options) {
		super(component, props, options);
		this.selectionRafId = null;
		this.positionCheckCallback = null;
		this.cachedExtensionWithSyncedStorage = null;
		if (!this.node.isLeaf) {
			if (this.options.contentDOMElementTag) this.contentDOMElement = document.createElement(this.options.contentDOMElementTag);
			else this.contentDOMElement = document.createElement(this.node.isInline ? "span" : "div");
			this.contentDOMElement.dataset.nodeViewContentReact = "";
			this.contentDOMElement.dataset.nodeViewWrapper = "";
			this.contentDOMElement.style.whiteSpace = "inherit";
			const contentTarget = this.dom.querySelector("[data-node-view-content]");
			if (!contentTarget) return;
			contentTarget.appendChild(this.contentDOMElement);
		}
	}
	get extensionWithSyncedStorage() {
		if (!this.cachedExtensionWithSyncedStorage) {
			const editor = this.editor;
			const extension = this.extension;
			this.cachedExtensionWithSyncedStorage = new Proxy(extension, { get(target, prop, receiver) {
				var _a;
				if (prop === "storage") return (_a = editor.storage[extension.name]) != null ? _a : {};
				return Reflect.get(target, prop, receiver);
			} });
		}
		return this.cachedExtensionWithSyncedStorage;
	}
	mount() {
		const props = {
			editor: this.editor,
			node: this.node,
			decorations: this.decorations,
			innerDecorations: this.innerDecorations,
			view: this.view,
			selected: false,
			extension: this.extensionWithSyncedStorage,
			HTMLAttributes: this.HTMLAttributes,
			getPos: () => this.getPos(),
			updateAttributes: (attributes = {}) => this.updateAttributes(attributes),
			deleteNode: () => this.deleteNode(),
			ref: (0, import_react.createRef)()
		};
		if (!this.component.displayName) {
			const capitalizeFirstChar = (string) => {
				return string.charAt(0).toUpperCase() + string.substring(1);
			};
			this.component.displayName = capitalizeFirstChar(this.extension.name);
		}
		const onDragStart = this.onDragStart.bind(this);
		const nodeViewContentRef = (element) => {
			if (element && this.contentDOMElement && element.firstChild !== this.contentDOMElement) {
				if (element.hasAttribute("data-node-view-wrapper")) element.removeAttribute("data-node-view-wrapper");
				element.appendChild(this.contentDOMElement);
			}
		};
		const context = {
			onDragStart,
			nodeViewContentRef
		};
		const Component = this.component;
		const ReactNodeViewProvider = (0, import_react.memo)((componentProps) => {
			return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ReactNodeViewContext.Provider, {
				value: context,
				children: (0, import_react.createElement)(Component, componentProps)
			});
		});
		ReactNodeViewProvider.displayName = "ReactNodeView";
		let as = this.node.isInline ? "span" : "div";
		if (this.options.as) as = this.options.as;
		const { className = "" } = this.options;
		this.handleSelectionUpdate = this.handleSelectionUpdate.bind(this);
		this.renderer = new ReactRenderer(ReactNodeViewProvider, {
			editor: this.editor,
			props,
			as,
			className: `node-${this.node.type.name} ${className}`.trim()
		});
		this.editor.on("selectionUpdate", this.handleSelectionUpdate);
		this.updateElementAttributes();
		this.currentPos = this.getPos();
		this.positionCheckCallback = () => {
			const newPos = this.getPos();
			if (typeof newPos !== "number" || newPos === this.currentPos) return;
			this.currentPos = newPos;
			this.renderer.updateProps({ getPos: () => this.getPos() });
			if (typeof this.options.attrs === "function") this.updateElementAttributes();
		};
		schedulePositionCheck(this.editor, this.positionCheckCallback);
	}
	get dom() {
		var _a;
		if (this.renderer.element.firstElementChild && !((_a = this.renderer.element.firstElementChild) == null ? void 0 : _a.hasAttribute("data-node-view-wrapper"))) throw Error("Please use the NodeViewWrapper component for your node view.");
		return this.renderer.element;
	}
	get contentDOM() {
		if (this.node.isLeaf) return null;
		return this.contentDOMElement;
	}
	handleSelectionUpdate() {
		if (this.selectionRafId) {
			cancelAnimationFrame(this.selectionRafId);
			this.selectionRafId = null;
		}
		this.selectionRafId = requestAnimationFrame(() => {
			this.selectionRafId = null;
			const pos = this.currentPos;
			if (typeof pos !== "number") return;
			if (isNodeViewSelected({
				selection: this.editor.state.selection,
				pos,
				nodeSize: this.node.nodeSize,
				selectedOnTextSelection: this.options.selectedOnTextSelection
			})) {
				if (this.renderer.props.selected) return;
				this.selectNode();
			} else {
				if (!this.renderer.props.selected) return;
				this.deselectNode();
			}
		});
	}
	update(node, decorations, innerDecorations) {
		const rerenderComponent = (props) => {
			this.renderer.updateProps(props);
			if (typeof this.options.attrs === "function") this.updateElementAttributes();
		};
		if (node.type !== this.node.type) return false;
		if (typeof this.options.update === "function") {
			const oldNode = this.node;
			const oldDecorations = this.decorations;
			const oldInnerDecorations = this.innerDecorations;
			this.node = node;
			this.decorations = decorations;
			this.innerDecorations = innerDecorations;
			this.currentPos = this.getPos();
			return this.options.update({
				oldNode,
				oldDecorations,
				newNode: node,
				newDecorations: decorations,
				oldInnerDecorations,
				innerDecorations,
				updateProps: () => rerenderComponent({
					node,
					decorations,
					innerDecorations,
					extension: this.extensionWithSyncedStorage
				})
			});
		}
		const newPos = this.getPos();
		if (node === this.node && this.decorations === decorations && this.innerDecorations === innerDecorations) {
			if (newPos === this.currentPos) return true;
			this.currentPos = newPos;
			rerenderComponent({
				node,
				decorations,
				innerDecorations,
				extension: this.extensionWithSyncedStorage,
				getPos: () => this.getPos()
			});
			return true;
		}
		this.node = node;
		this.decorations = decorations;
		this.innerDecorations = innerDecorations;
		this.currentPos = newPos;
		rerenderComponent({
			node,
			decorations,
			innerDecorations,
			extension: this.extensionWithSyncedStorage
		});
		return true;
	}
	selectNode() {
		this.renderer.updateProps({ selected: true });
		this.renderer.element.classList.add("ProseMirror-selectednode");
	}
	deselectNode() {
		this.renderer.updateProps({ selected: false });
		this.renderer.element.classList.remove("ProseMirror-selectednode");
	}
	destroy() {
		this.renderer.destroy();
		this.editor.off("selectionUpdate", this.handleSelectionUpdate);
		if (this.positionCheckCallback) {
			cancelPositionCheck(this.editor, this.positionCheckCallback);
			this.positionCheckCallback = null;
		}
		this.contentDOMElement = null;
		if (this.selectionRafId) {
			cancelAnimationFrame(this.selectionRafId);
			this.selectionRafId = null;
		}
	}
	updateElementAttributes() {
		if (this.options.attrs) {
			let attrsObj = {};
			if (typeof this.options.attrs === "function") {
				const extensionAttributes = this.editor.extensionManager.attributes;
				const HTMLAttributes = getRenderedAttributes(this.node, extensionAttributes);
				attrsObj = this.options.attrs({
					node: this.node,
					HTMLAttributes
				});
			} else attrsObj = this.options.attrs;
			this.renderer.updateAttributes(attrsObj);
		}
	}
};
function ReactNodeViewRenderer(component, options) {
	return (props) => {
		if (!props.editor.contentComponent) return {};
		return new ReactNodeView(component, props, options);
	};
}
var TiptapContext = (0, import_react.createContext)({ get editor() {
	throw new Error("useTiptap must be used within a <Tiptap> provider");
} });
TiptapContext.displayName = "TiptapContext";
var useTiptap = () => (0, import_react.useContext)(TiptapContext);
function TiptapWrapper({ editor, instance, children }) {
	const resolvedEditor = editor != null ? editor : instance;
	if (!resolvedEditor) throw new Error("Tiptap: An editor instance is required. Pass a non-null `editor` prop.");
	const tiptapContextValue = (0, import_react.useMemo)(() => ({ editor: resolvedEditor }), [resolvedEditor]);
	const legacyContextValue = (0, import_react.useMemo)(() => ({ editor: resolvedEditor }), [resolvedEditor]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(EditorContext.Provider, {
		value: legacyContextValue,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TiptapContext.Provider, {
			value: tiptapContextValue,
			children
		})
	});
}
TiptapWrapper.displayName = "Tiptap";
function TiptapContent({ ...rest }) {
	const { editor } = useTiptap();
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(EditorContent, {
		editor,
		...rest
	});
}
TiptapContent.displayName = "Tiptap.Content";
Object.assign(TiptapWrapper, { Content: TiptapContent });
function dropCursor(options = {}) {
	return new Plugin({ view(editorView) {
		return new DropCursorView(editorView, options);
	} });
}
var DropCursorView = class {
	constructor(editorView, options) {
		var _a;
		this.editorView = editorView;
		this.cursorPos = null;
		this.element = null;
		this.timeout = -1;
		this.width = (_a = options.width) !== null && _a !== void 0 ? _a : 1;
		this.color = options.color === false ? void 0 : options.color || "black";
		this.class = options.class;
		this.handlers = [
			"dragover",
			"dragend",
			"drop",
			"dragleave"
		].map((name) => {
			let handler = (e) => {
				this[name](e);
			};
			editorView.dom.addEventListener(name, handler);
			return {
				name,
				handler
			};
		});
	}
	destroy() {
		this.handlers.forEach(({ name, handler }) => this.editorView.dom.removeEventListener(name, handler));
	}
	update(editorView, prevState) {
		if (this.cursorPos != null && prevState.doc != editorView.state.doc) if (this.cursorPos > editorView.state.doc.content.size) this.setCursor(null);
		else this.updateOverlay();
	}
	setCursor(pos) {
		if (pos == this.cursorPos) return;
		this.cursorPos = pos;
		if (pos == null) {
			this.element.parentNode.removeChild(this.element);
			this.element = null;
		} else this.updateOverlay();
	}
	updateOverlay() {
		let $pos = this.editorView.state.doc.resolve(this.cursorPos);
		let isBlock = !$pos.parent.inlineContent, rect;
		let editorDOM = this.editorView.dom, editorRect = editorDOM.getBoundingClientRect();
		let scaleX = editorRect.width / editorDOM.offsetWidth, scaleY = editorRect.height / editorDOM.offsetHeight;
		if (isBlock) {
			let before = $pos.nodeBefore, after = $pos.nodeAfter;
			if (before || after) {
				let node = this.editorView.nodeDOM(this.cursorPos - (before ? before.nodeSize : 0));
				if (node) {
					let nodeRect = node.getBoundingClientRect();
					let top = before ? nodeRect.bottom : nodeRect.top;
					if (before && after) top = (top + this.editorView.nodeDOM(this.cursorPos).getBoundingClientRect().top) / 2;
					let halfWidth = this.width / 2 * scaleY;
					rect = {
						left: nodeRect.left,
						right: nodeRect.right,
						top: top - halfWidth,
						bottom: top + halfWidth
					};
				}
			}
		}
		if (!rect) {
			let coords = this.editorView.coordsAtPos(this.cursorPos);
			let halfWidth = this.width / 2 * scaleX;
			rect = {
				left: coords.left - halfWidth,
				right: coords.left + halfWidth,
				top: coords.top,
				bottom: coords.bottom
			};
		}
		let parent = this.editorView.dom.offsetParent;
		if (!this.element) {
			this.element = parent.appendChild(document.createElement("div"));
			if (this.class) this.element.className = this.class;
			this.element.style.cssText = "position: absolute; z-index: 50; pointer-events: none;";
			if (this.color) this.element.style.backgroundColor = this.color;
		}
		this.element.classList.toggle("prosemirror-dropcursor-block", isBlock);
		this.element.classList.toggle("prosemirror-dropcursor-inline", !isBlock);
		let parentLeft, parentTop;
		if (!parent || parent == document.body && getComputedStyle(parent).position == "static") {
			parentLeft = -pageXOffset;
			parentTop = -pageYOffset;
		} else {
			let rect$1 = parent.getBoundingClientRect();
			let parentScaleX = rect$1.width / parent.offsetWidth, parentScaleY = rect$1.height / parent.offsetHeight;
			parentLeft = rect$1.left - parent.scrollLeft * parentScaleX;
			parentTop = rect$1.top - parent.scrollTop * parentScaleY;
		}
		this.element.style.left = (rect.left - parentLeft) / scaleX + "px";
		this.element.style.top = (rect.top - parentTop) / scaleY + "px";
		this.element.style.width = (rect.right - rect.left) / scaleX + "px";
		this.element.style.height = (rect.bottom - rect.top) / scaleY + "px";
	}
	scheduleRemoval(timeout) {
		clearTimeout(this.timeout);
		this.timeout = setTimeout(() => this.setCursor(null), timeout);
	}
	dragover(event) {
		if (!this.editorView.editable) return;
		let pos = this.editorView.posAtCoords({
			left: event.clientX,
			top: event.clientY
		});
		let node = pos && pos.inside >= 0 && this.editorView.state.doc.nodeAt(pos.inside);
		let disableDropCursor = node && node.type.spec.disableDropCursor;
		let disabled = typeof disableDropCursor == "function" ? disableDropCursor(this.editorView, pos, event) : disableDropCursor;
		if (pos && !disabled) {
			let target = pos.pos;
			if (this.editorView.dragging && this.editorView.dragging.slice) {
				let point = dropPoint(this.editorView.state.doc, target, this.editorView.dragging.slice);
				if (point != null) target = point;
			}
			this.setCursor(target);
			this.scheduleRemoval(5e3);
		}
	}
	dragend() {
		this.scheduleRemoval(20);
	}
	drop() {
		this.scheduleRemoval(20);
	}
	dragleave(event) {
		if (!this.editorView.dom.contains(event.relatedTarget)) this.setCursor(null);
	}
};
var GapCursor = class GapCursor extends Selection {
	constructor($pos) {
		super($pos, $pos);
	}
	map(doc$2, mapping) {
		let $pos = doc$2.resolve(mapping.map(this.head));
		return GapCursor.valid($pos) ? new GapCursor($pos) : Selection.near($pos);
	}
	content() {
		return Slice.empty;
	}
	eq(other) {
		return other instanceof GapCursor && other.head == this.head;
	}
	toJSON() {
		return {
			type: "gapcursor",
			pos: this.head
		};
	}
	static fromJSON(doc$2, json) {
		if (typeof json.pos != "number") throw new RangeError("Invalid input for GapCursor.fromJSON");
		return new GapCursor(doc$2.resolve(json.pos));
	}
	getBookmark() {
		return new GapBookmark(this.anchor);
	}
	static valid($pos) {
		let parent = $pos.parent;
		if (parent.inlineContent || !closedBefore($pos) || !closedAfter($pos)) return false;
		let override = parent.type.spec.allowGapCursor;
		if (override != null) return override;
		let deflt = parent.contentMatchAt($pos.index()).defaultType;
		return deflt && deflt.isTextblock;
	}
	static findGapCursorFrom($pos, dir, mustMove = false) {
		search: for (;;) {
			if (!mustMove && GapCursor.valid($pos)) return $pos;
			let pos = $pos.pos, next = null;
			for (let d = $pos.depth;; d--) {
				let parent = $pos.node(d);
				if (dir > 0 ? $pos.indexAfter(d) < parent.childCount : $pos.index(d) > 0) {
					next = parent.child(dir > 0 ? $pos.indexAfter(d) : $pos.index(d) - 1);
					break;
				} else if (d == 0) return null;
				pos += dir;
				let $cur = $pos.doc.resolve(pos);
				if (GapCursor.valid($cur)) return $cur;
			}
			for (;;) {
				let inside = dir > 0 ? next.firstChild : next.lastChild;
				if (!inside) {
					if (next.isAtom && !next.isText && !NodeSelection.isSelectable(next)) {
						$pos = $pos.doc.resolve(pos + next.nodeSize * dir);
						mustMove = false;
						continue search;
					}
					break;
				}
				next = inside;
				pos += dir;
				let $cur = $pos.doc.resolve(pos);
				if (GapCursor.valid($cur)) return $cur;
			}
			return null;
		}
	}
};
GapCursor.prototype.visible = false;
GapCursor.findFrom = GapCursor.findGapCursorFrom;
Selection.jsonID("gapcursor", GapCursor);
var GapBookmark = class GapBookmark {
	constructor(pos) {
		this.pos = pos;
	}
	map(mapping) {
		return new GapBookmark(mapping.map(this.pos));
	}
	resolve(doc$2) {
		let $pos = doc$2.resolve(this.pos);
		return GapCursor.valid($pos) ? new GapCursor($pos) : Selection.near($pos);
	}
};
function needsGap(type) {
	return type.isAtom || type.spec.isolating || type.spec.createGapCursor;
}
function closedBefore($pos) {
	for (let d = $pos.depth; d >= 0; d--) {
		let index = $pos.index(d), parent = $pos.node(d);
		if (index == 0) {
			if (parent.type.spec.isolating) return true;
			continue;
		}
		for (let before = parent.child(index - 1);; before = before.lastChild) {
			if (before.childCount == 0 && !before.inlineContent || needsGap(before.type)) return true;
			if (before.inlineContent) return false;
		}
	}
	return true;
}
function closedAfter($pos) {
	for (let d = $pos.depth; d >= 0; d--) {
		let index = $pos.indexAfter(d), parent = $pos.node(d);
		if (index == parent.childCount) {
			if (parent.type.spec.isolating) return true;
			continue;
		}
		for (let after = parent.child(index);; after = after.firstChild) {
			if (after.childCount == 0 && !after.inlineContent || needsGap(after.type)) return true;
			if (after.inlineContent) return false;
		}
	}
	return true;
}
function gapCursor() {
	return new Plugin({ props: {
		decorations: drawGapCursor,
		createSelectionBetween(_view, $anchor, $head) {
			return $anchor.pos == $head.pos && GapCursor.valid($head) ? new GapCursor($head) : null;
		},
		handleClick,
		handleKeyDown: handleKeyDown$1,
		handleDOMEvents: { beforeinput }
	} });
}
var handleKeyDown$1 = keydownHandler({
	"ArrowLeft": arrow$1("horiz", -1),
	"ArrowRight": arrow$1("horiz", 1),
	"ArrowUp": arrow$1("vert", -1),
	"ArrowDown": arrow$1("vert", 1)
});
function arrow$1(axis, dir) {
	const dirStr = axis == "vert" ? dir > 0 ? "down" : "up" : dir > 0 ? "right" : "left";
	return function(state, dispatch, view) {
		let sel = state.selection;
		let $start = dir > 0 ? sel.$to : sel.$from, mustMove = sel.empty;
		if (sel instanceof TextSelection) {
			if (!view.endOfTextblock(dirStr) || $start.depth == 0) return false;
			mustMove = false;
			$start = state.doc.resolve(dir > 0 ? $start.after() : $start.before());
		}
		let $found = GapCursor.findGapCursorFrom($start, dir, mustMove);
		if (!$found) return false;
		if (dispatch) dispatch(state.tr.setSelection(new GapCursor($found)));
		return true;
	};
}
function handleClick(view, pos, event) {
	if (!view || !view.editable) return false;
	let $pos = view.state.doc.resolve(pos);
	if (!GapCursor.valid($pos)) return false;
	let clickPos = view.posAtCoords({
		left: event.clientX,
		top: event.clientY
	});
	if (clickPos && clickPos.inside > -1 && NodeSelection.isSelectable(view.state.doc.nodeAt(clickPos.inside))) return false;
	view.dispatch(view.state.tr.setSelection(new GapCursor($pos)));
	return true;
}
function beforeinput(view, event) {
	if (event.inputType != "insertCompositionText" || !(view.state.selection instanceof GapCursor)) return false;
	let { $from } = view.state.selection;
	let insert = $from.parent.contentMatchAt($from.index()).findWrapping(view.state.schema.nodes.text);
	if (!insert) return false;
	let frag = Fragment.empty;
	for (let i$1 = insert.length - 1; i$1 >= 0; i$1--) frag = Fragment.from(insert[i$1].createAndFill(null, frag));
	let tr$1 = view.state.tr.replace($from.pos, $from.pos, new Slice(frag, 0, 0));
	tr$1.setSelection(TextSelection.near(tr$1.doc.resolve($from.pos + 1)));
	view.dispatch(tr$1);
	return false;
}
function drawGapCursor(state) {
	if (!(state.selection instanceof GapCursor)) return null;
	let node = document.createElement("div");
	node.className = "ProseMirror-gapcursor";
	return DecorationSet.create(state.doc, [Decoration.widget(state.selection.head, node, { key: "gapcursor" })]);
}
var GOOD_LEAF_SIZE = 200;
var RopeSequence = function RopeSequence$1() {};
RopeSequence.prototype.append = function append(other) {
	if (!other.length) return this;
	other = RopeSequence.from(other);
	return !this.length && other || other.length < GOOD_LEAF_SIZE && this.leafAppend(other) || this.length < GOOD_LEAF_SIZE && other.leafPrepend(this) || this.appendInner(other);
};
RopeSequence.prototype.prepend = function prepend(other) {
	if (!other.length) return this;
	return RopeSequence.from(other).append(this);
};
RopeSequence.prototype.appendInner = function appendInner(other) {
	return new Append(this, other);
};
RopeSequence.prototype.slice = function slice(from, to) {
	if (from === void 0) from = 0;
	if (to === void 0) to = this.length;
	if (from >= to) return RopeSequence.empty;
	return this.sliceInner(Math.max(0, from), Math.min(this.length, to));
};
RopeSequence.prototype.get = function get(i$1) {
	if (i$1 < 0 || i$1 >= this.length) return;
	return this.getInner(i$1);
};
RopeSequence.prototype.forEach = function forEach$1(f, from, to) {
	if (from === void 0) from = 0;
	if (to === void 0) to = this.length;
	if (from <= to) this.forEachInner(f, from, to, 0);
	else this.forEachInvertedInner(f, from, to, 0);
};
RopeSequence.prototype.map = function map(f, from, to) {
	if (from === void 0) from = 0;
	if (to === void 0) to = this.length;
	var result = [];
	this.forEach(function(elt, i$1) {
		return result.push(f(elt, i$1));
	}, from, to);
	return result;
};
RopeSequence.from = function from(values) {
	if (values instanceof RopeSequence) return values;
	return values && values.length ? new Leaf(values) : RopeSequence.empty;
};
var Leaf = /* @__PURE__ */ function(RopeSequence$1) {
	function Leaf$1(values) {
		RopeSequence$1.call(this);
		this.values = values;
	}
	if (RopeSequence$1) Leaf$1.__proto__ = RopeSequence$1;
	Leaf$1.prototype = Object.create(RopeSequence$1 && RopeSequence$1.prototype);
	Leaf$1.prototype.constructor = Leaf$1;
	var prototypeAccessors = {
		length: { configurable: true },
		depth: { configurable: true }
	};
	Leaf$1.prototype.flatten = function flatten() {
		return this.values;
	};
	Leaf$1.prototype.sliceInner = function sliceInner(from, to) {
		if (from == 0 && to == this.length) return this;
		return new Leaf$1(this.values.slice(from, to));
	};
	Leaf$1.prototype.getInner = function getInner(i$1) {
		return this.values[i$1];
	};
	Leaf$1.prototype.forEachInner = function forEachInner(f, from, to, start) {
		for (var i$1 = from; i$1 < to; i$1++) if (f(this.values[i$1], start + i$1) === false) return false;
	};
	Leaf$1.prototype.forEachInvertedInner = function forEachInvertedInner(f, from, to, start) {
		for (var i$1 = from - 1; i$1 >= to; i$1--) if (f(this.values[i$1], start + i$1) === false) return false;
	};
	Leaf$1.prototype.leafAppend = function leafAppend(other) {
		if (this.length + other.length <= GOOD_LEAF_SIZE) return new Leaf$1(this.values.concat(other.flatten()));
	};
	Leaf$1.prototype.leafPrepend = function leafPrepend(other) {
		if (this.length + other.length <= GOOD_LEAF_SIZE) return new Leaf$1(other.flatten().concat(this.values));
	};
	prototypeAccessors.length.get = function() {
		return this.values.length;
	};
	prototypeAccessors.depth.get = function() {
		return 0;
	};
	Object.defineProperties(Leaf$1.prototype, prototypeAccessors);
	return Leaf$1;
}(RopeSequence);
RopeSequence.empty = new Leaf([]);
var Append = /* @__PURE__ */ function(RopeSequence$1) {
	function Append$1(left, right) {
		RopeSequence$1.call(this);
		this.left = left;
		this.right = right;
		this.length = left.length + right.length;
		this.depth = Math.max(left.depth, right.depth) + 1;
	}
	if (RopeSequence$1) Append$1.__proto__ = RopeSequence$1;
	Append$1.prototype = Object.create(RopeSequence$1 && RopeSequence$1.prototype);
	Append$1.prototype.constructor = Append$1;
	Append$1.prototype.flatten = function flatten() {
		return this.left.flatten().concat(this.right.flatten());
	};
	Append$1.prototype.getInner = function getInner(i$1) {
		return i$1 < this.left.length ? this.left.get(i$1) : this.right.get(i$1 - this.left.length);
	};
	Append$1.prototype.forEachInner = function forEachInner(f, from, to, start) {
		var leftLen = this.left.length;
		if (from < leftLen && this.left.forEachInner(f, from, Math.min(to, leftLen), start) === false) return false;
		if (to > leftLen && this.right.forEachInner(f, Math.max(from - leftLen, 0), Math.min(this.length, to) - leftLen, start + leftLen) === false) return false;
	};
	Append$1.prototype.forEachInvertedInner = function forEachInvertedInner(f, from, to, start) {
		var leftLen = this.left.length;
		if (from > leftLen && this.right.forEachInvertedInner(f, from - leftLen, Math.max(to, leftLen) - leftLen, start + leftLen) === false) return false;
		if (to < leftLen && this.left.forEachInvertedInner(f, Math.min(from, leftLen), to, start) === false) return false;
	};
	Append$1.prototype.sliceInner = function sliceInner(from, to) {
		if (from == 0 && to == this.length) return this;
		var leftLen = this.left.length;
		if (to <= leftLen) return this.left.slice(from, to);
		if (from >= leftLen) return this.right.slice(from - leftLen, to - leftLen);
		return this.left.slice(from, leftLen).append(this.right.slice(0, to - leftLen));
	};
	Append$1.prototype.leafAppend = function leafAppend(other) {
		var inner = this.right.leafAppend(other);
		if (inner) return new Append$1(this.left, inner);
	};
	Append$1.prototype.leafPrepend = function leafPrepend(other) {
		var inner = this.left.leafPrepend(other);
		if (inner) return new Append$1(inner, this.right);
	};
	Append$1.prototype.appendInner = function appendInner(other) {
		if (this.left.depth >= Math.max(this.right.depth, other.depth) + 1) return new Append$1(this.left, new Append$1(this.right, other));
		return new Append$1(this, other);
	};
	return Append$1;
}(RopeSequence);
var dist_default = RopeSequence;
var max_empty_items = 500;
var Branch = class Branch {
	constructor(items, eventCount) {
		this.items = items;
		this.eventCount = eventCount;
	}
	popEvent(state, preserveItems) {
		if (this.eventCount == 0) return null;
		let end = this.items.length;
		for (;; end--) if (this.items.get(end - 1).selection) {
			--end;
			break;
		}
		let remap, mapFrom;
		if (preserveItems) {
			remap = this.remapping(end, this.items.length);
			mapFrom = remap.maps.length;
		}
		let transform = state.tr;
		let selection, remaining;
		let addAfter = [], addBefore = [];
		this.items.forEach((item, i$1) => {
			if (!item.step) {
				if (!remap) {
					remap = this.remapping(end, i$1 + 1);
					mapFrom = remap.maps.length;
				}
				mapFrom--;
				addBefore.push(item);
				return;
			}
			if (remap) {
				addBefore.push(new Item(item.map));
				let step$1 = item.step.map(remap.slice(mapFrom)), map;
				if (step$1 && transform.maybeStep(step$1).doc) {
					map = transform.mapping.maps[transform.mapping.maps.length - 1];
					addAfter.push(new Item(map, void 0, void 0, addAfter.length + addBefore.length));
				}
				mapFrom--;
				if (map) remap.appendMap(map, mapFrom);
			} else transform.maybeStep(item.step);
			if (item.selection) {
				selection = remap ? item.selection.map(remap.slice(mapFrom)) : item.selection;
				remaining = new Branch(this.items.slice(0, end).append(addBefore.reverse().concat(addAfter)), this.eventCount - 1);
				return false;
			}
		}, this.items.length, 0);
		return {
			remaining,
			transform,
			selection
		};
	}
	addTransform(transform, selection, histOptions, preserveItems) {
		let newItems = [], eventCount = this.eventCount;
		let oldItems = this.items, lastItem = !preserveItems && oldItems.length ? oldItems.get(oldItems.length - 1) : null;
		for (let i$1 = 0; i$1 < transform.steps.length; i$1++) {
			let step$1 = transform.steps[i$1].invert(transform.docs[i$1]);
			let item = new Item(transform.mapping.maps[i$1], step$1, selection), merged;
			if (merged = lastItem && lastItem.merge(item)) {
				item = merged;
				if (i$1) newItems.pop();
				else oldItems = oldItems.slice(0, oldItems.length - 1);
			}
			newItems.push(item);
			if (selection) {
				eventCount++;
				selection = void 0;
			}
			if (!preserveItems) lastItem = item;
		}
		let overflow = eventCount - histOptions.depth;
		if (overflow > DEPTH_OVERFLOW) {
			oldItems = cutOffEvents(oldItems, overflow);
			eventCount -= overflow;
		}
		return new Branch(oldItems.append(newItems), eventCount);
	}
	remapping(from, to) {
		let maps = new Mapping();
		this.items.forEach((item, i$1) => {
			let mirrorPos = item.mirrorOffset != null && i$1 - item.mirrorOffset >= from ? maps.maps.length - item.mirrorOffset : void 0;
			maps.appendMap(item.map, mirrorPos);
		}, from, to);
		return maps;
	}
	addMaps(array) {
		if (this.eventCount == 0) return this;
		return new Branch(this.items.append(array.map((map) => new Item(map))), this.eventCount);
	}
	rebased(rebasedTransform, rebasedCount) {
		if (!this.eventCount) return this;
		let rebasedItems = [], start = Math.max(0, this.items.length - rebasedCount);
		let mapping = rebasedTransform.mapping;
		let newUntil = rebasedTransform.steps.length;
		let eventCount = this.eventCount;
		this.items.forEach((item) => {
			if (item.selection) eventCount--;
		}, start);
		let iRebased = rebasedCount;
		this.items.forEach((item) => {
			let pos = mapping.getMirror(--iRebased);
			if (pos == null) return;
			newUntil = Math.min(newUntil, pos);
			let map = mapping.maps[pos];
			if (item.step) {
				let step$1 = rebasedTransform.steps[pos].invert(rebasedTransform.docs[pos]);
				let selection = item.selection && item.selection.map(mapping.slice(iRebased + 1, pos));
				if (selection) eventCount++;
				rebasedItems.push(new Item(map, step$1, selection));
			} else rebasedItems.push(new Item(map));
		}, start);
		let newMaps = [];
		for (let i$1 = rebasedCount; i$1 < newUntil; i$1++) newMaps.push(new Item(mapping.maps[i$1]));
		let branch = new Branch(this.items.slice(0, start).append(newMaps).append(rebasedItems), eventCount);
		if (branch.emptyItemCount() > max_empty_items) branch = branch.compress(this.items.length - rebasedItems.length);
		return branch;
	}
	emptyItemCount() {
		let count = 0;
		this.items.forEach((item) => {
			if (!item.step) count++;
		});
		return count;
	}
	compress(upto = this.items.length) {
		let remap = this.remapping(0, upto), mapFrom = remap.maps.length;
		let items = [], events = 0;
		this.items.forEach((item, i$1) => {
			if (i$1 >= upto) {
				items.push(item);
				if (item.selection) events++;
			} else if (item.step) {
				let step$1 = item.step.map(remap.slice(mapFrom)), map = step$1 && step$1.getMap();
				mapFrom--;
				if (map) remap.appendMap(map, mapFrom);
				if (step$1) {
					let selection = item.selection && item.selection.map(remap.slice(mapFrom));
					if (selection) events++;
					let newItem = new Item(map.invert(), step$1, selection), merged, last = items.length - 1;
					if (merged = items.length && items[last].merge(newItem)) items[last] = merged;
					else items.push(newItem);
				}
			} else if (item.map) mapFrom--;
		}, this.items.length, 0);
		return new Branch(dist_default.from(items.reverse()), events);
	}
};
Branch.empty = new Branch(dist_default.empty, 0);
function cutOffEvents(items, n) {
	let cutPoint;
	items.forEach((item, i$1) => {
		if (item.selection && n-- == 0) {
			cutPoint = i$1;
			return false;
		}
	});
	return items.slice(cutPoint);
}
var Item = class Item {
	constructor(map, step$1, selection, mirrorOffset) {
		this.map = map;
		this.step = step$1;
		this.selection = selection;
		this.mirrorOffset = mirrorOffset;
	}
	merge(other) {
		if (this.step && other.step && !other.selection) {
			let step$1 = other.step.merge(this.step);
			if (step$1) return new Item(step$1.getMap().invert(), step$1, this.selection);
		}
	}
};
var HistoryState = class {
	constructor(done, undone, prevRanges, prevTime, prevComposition) {
		this.done = done;
		this.undone = undone;
		this.prevRanges = prevRanges;
		this.prevTime = prevTime;
		this.prevComposition = prevComposition;
	}
};
var DEPTH_OVERFLOW = 20;
function applyTransaction(history$1, state, tr$1, options) {
	let historyTr = tr$1.getMeta(historyKey), rebased;
	if (historyTr) return historyTr.historyState;
	if (tr$1.getMeta(closeHistoryKey)) history$1 = new HistoryState(history$1.done, history$1.undone, null, 0, -1);
	let appended = tr$1.getMeta("appendedTransaction");
	if (tr$1.steps.length == 0) return history$1;
	else if (appended && appended.getMeta(historyKey)) if (appended.getMeta(historyKey).redo) return new HistoryState(history$1.done.addTransform(tr$1, void 0, options, mustPreserveItems(state)), history$1.undone, rangesFor(tr$1.mapping.maps), history$1.prevTime, history$1.prevComposition);
	else return new HistoryState(history$1.done, history$1.undone.addTransform(tr$1, void 0, options, mustPreserveItems(state)), null, history$1.prevTime, history$1.prevComposition);
	else if (tr$1.getMeta("addToHistory") !== false && !(appended && appended.getMeta("addToHistory") === false)) {
		let composition = tr$1.getMeta("composition");
		let newGroup = history$1.prevTime == 0 || !appended && history$1.prevComposition != composition && (history$1.prevTime < (tr$1.time || 0) - options.newGroupDelay || !isAdjacentTo(tr$1, history$1.prevRanges));
		let prevRanges = appended ? mapRanges(history$1.prevRanges, tr$1.mapping) : rangesFor(tr$1.mapping.maps);
		return new HistoryState(history$1.done.addTransform(tr$1, newGroup ? state.selection.getBookmark() : void 0, options, mustPreserveItems(state)), Branch.empty, prevRanges, tr$1.time, composition == null ? history$1.prevComposition : composition);
	} else if (rebased = tr$1.getMeta("rebased")) return new HistoryState(history$1.done.rebased(tr$1, rebased), history$1.undone.rebased(tr$1, rebased), mapRanges(history$1.prevRanges, tr$1.mapping), history$1.prevTime, history$1.prevComposition);
	else return new HistoryState(history$1.done.addMaps(tr$1.mapping.maps), history$1.undone.addMaps(tr$1.mapping.maps), mapRanges(history$1.prevRanges, tr$1.mapping), history$1.prevTime, history$1.prevComposition);
}
function isAdjacentTo(transform, prevRanges) {
	if (!prevRanges) return false;
	if (!transform.docChanged) return true;
	let adjacent = false;
	transform.mapping.maps[0].forEach((start, end) => {
		for (let i$1 = 0; i$1 < prevRanges.length; i$1 += 2) if (start <= prevRanges[i$1 + 1] && end >= prevRanges[i$1]) adjacent = true;
	});
	return adjacent;
}
function rangesFor(maps) {
	let result = [];
	for (let i$1 = maps.length - 1; i$1 >= 0 && result.length == 0; i$1--) maps[i$1].forEach((_from, _to, from, to) => result.push(from, to));
	return result;
}
function mapRanges(ranges, mapping) {
	if (!ranges) return null;
	let result = [];
	for (let i$1 = 0; i$1 < ranges.length; i$1 += 2) {
		let from = mapping.map(ranges[i$1], 1), to = mapping.map(ranges[i$1 + 1], -1);
		if (from <= to) result.push(from, to);
	}
	return result;
}
function histTransaction(history$1, state, redo$1) {
	let preserveItems = mustPreserveItems(state);
	let histOptions = historyKey.get(state).spec.config;
	let pop = (redo$1 ? history$1.undone : history$1.done).popEvent(state, preserveItems);
	if (!pop) return null;
	let selection = pop.selection.resolve(pop.transform.doc);
	let added = (redo$1 ? history$1.done : history$1.undone).addTransform(pop.transform, state.selection.getBookmark(), histOptions, preserveItems);
	let newHist = new HistoryState(redo$1 ? added : pop.remaining, redo$1 ? pop.remaining : added, null, 0, -1);
	return pop.transform.setSelection(selection).setMeta(historyKey, {
		redo: redo$1,
		historyState: newHist
	});
}
var cachedPreserveItems = false, cachedPreserveItemsPlugins = null;
function mustPreserveItems(state) {
	let plugins = state.plugins;
	if (cachedPreserveItemsPlugins != plugins) {
		cachedPreserveItems = false;
		cachedPreserveItemsPlugins = plugins;
		for (let i$1 = 0; i$1 < plugins.length; i$1++) if (plugins[i$1].spec.historyPreserveItems) {
			cachedPreserveItems = true;
			break;
		}
	}
	return cachedPreserveItems;
}
var historyKey = new PluginKey("history");
var closeHistoryKey = new PluginKey("closeHistory");
function history(config = {}) {
	config = {
		depth: config.depth || 100,
		newGroupDelay: config.newGroupDelay || 500
	};
	return new Plugin({
		key: historyKey,
		state: {
			init() {
				return new HistoryState(Branch.empty, Branch.empty, null, 0, -1);
			},
			apply(tr$1, hist, state) {
				return applyTransaction(hist, state, tr$1, config);
			}
		},
		config,
		props: { handleDOMEvents: { beforeinput(view, e) {
			let inputType = e.inputType;
			let command$1 = inputType == "historyUndo" ? undo : inputType == "historyRedo" ? redo : null;
			if (!command$1 || !view.editable) return false;
			e.preventDefault();
			return command$1(view.state, view.dispatch);
		} } }
	});
}
function buildCommand(redo$1, scroll) {
	return (state, dispatch) => {
		let hist = historyKey.getState(state);
		if (!hist || (redo$1 ? hist.undone : hist.done).eventCount == 0) return false;
		if (dispatch) {
			let tr$1 = histTransaction(hist, state, redo$1);
			if (tr$1) dispatch(scroll ? tr$1.scrollIntoView() : tr$1);
		}
		return true;
	};
}
var undo = buildCommand(false, true);
var redo = buildCommand(true, true);
buildCommand(false, false);
buildCommand(true, false);
Extension.create({
	name: "characterCount",
	addOptions() {
		return {
			limit: null,
			mode: "textSize",
			textCounter: (text) => text.length,
			wordCounter: (text) => text.split(" ").filter((word) => word !== "").length
		};
	},
	addStorage() {
		return {
			characters: () => 0,
			words: () => 0
		};
	},
	onBeforeCreate() {
		this.storage.characters = (options) => {
			const node = (options == null ? void 0 : options.node) || this.editor.state.doc;
			if (((options == null ? void 0 : options.mode) || this.options.mode) === "textSize") {
				const text = node.textBetween(0, node.content.size, void 0, " ");
				return this.options.textCounter(text);
			}
			return node.nodeSize;
		};
		this.storage.words = (options) => {
			const node = (options == null ? void 0 : options.node) || this.editor.state.doc;
			const text = node.textBetween(0, node.content.size, " ", " ");
			return this.options.wordCounter(text);
		};
	},
	addProseMirrorPlugins() {
		let initialEvaluationDone = false;
		return [new Plugin({
			key: new PluginKey("characterCount"),
			appendTransaction: (transactions, oldState, newState) => {
				if (initialEvaluationDone) return;
				const limit = this.options.limit;
				if (limit === null || limit === void 0 || limit === 0) {
					initialEvaluationDone = true;
					return;
				}
				const initialContentSize = this.storage.characters({ node: newState.doc });
				if (initialContentSize > limit) {
					const over = initialContentSize - limit;
					const from = 0;
					const to = over;
					console.warn(`[CharacterCount] Initial content exceeded limit of ${limit} characters. Content was automatically trimmed.`);
					const tr$1 = newState.tr.deleteRange(from, to);
					initialEvaluationDone = true;
					return tr$1;
				}
				initialEvaluationDone = true;
			},
			filterTransaction: (transaction, state) => {
				const limit = this.options.limit;
				if (!transaction.docChanged || limit === 0 || limit === null || limit === void 0) return true;
				const oldSize = this.storage.characters({ node: state.doc });
				const newSize = this.storage.characters({ node: transaction.doc });
				if (newSize <= limit) return true;
				if (oldSize > limit && newSize > limit && newSize <= oldSize) return true;
				if (oldSize > limit && newSize > limit && newSize > oldSize) return false;
				if (!transaction.getMeta("paste")) return false;
				const pos = transaction.selection.$head.pos;
				const from = pos - (newSize - limit);
				const to = pos;
				transaction.deleteRange(from, to);
				if (this.storage.characters({ node: transaction.doc }) > limit) return false;
				return true;
			}
		})];
	}
});
var Dropcursor = Extension.create({
	name: "dropCursor",
	addOptions() {
		return {
			color: "currentColor",
			width: 1,
			class: void 0
		};
	},
	addProseMirrorPlugins() {
		return [dropCursor(this.options)];
	}
});
Extension.create({
	name: "focus",
	addOptions() {
		return {
			className: "has-focus",
			mode: "all"
		};
	},
	addProseMirrorPlugins() {
		return [new Plugin({
			key: new PluginKey("focus"),
			props: { decorations: ({ doc: doc$2, selection }) => {
				const { isEditable, isFocused } = this.editor;
				const { anchor } = selection;
				const decorations = [];
				if (!isEditable || !isFocused) return DecorationSet.create(doc$2, []);
				let maxLevels = 0;
				if (this.options.mode === "deepest") doc$2.descendants((node, pos) => {
					if (node.isText) return;
					if (!(anchor >= pos && anchor <= pos + node.nodeSize - 1)) return false;
					maxLevels += 1;
				});
				let currentLevel = 0;
				doc$2.descendants((node, pos) => {
					if (node.isText) return false;
					if (!(anchor >= pos && anchor <= pos + node.nodeSize - 1)) return false;
					currentLevel += 1;
					if (this.options.mode === "deepest" && maxLevels - currentLevel > 0 || this.options.mode === "shallowest" && currentLevel > 1) return this.options.mode === "deepest";
					decorations.push(Decoration.node(pos, pos + node.nodeSize, { class: this.options.className }));
				});
				return DecorationSet.create(doc$2, decorations);
			} }
		})];
	}
});
var Gapcursor = Extension.create({
	name: "gapCursor",
	addProseMirrorPlugins() {
		return [gapCursor()];
	},
	extendNodeSchema(extension) {
		var _a;
		return { allowGapCursor: (_a = callOrReturn(getExtensionField(extension, "allowGapCursor", {
			name: extension.name,
			options: extension.options,
			storage: extension.storage
		}))) != null ? _a : null };
	}
});
var DEFAULT_DATA_ATTRIBUTE = "placeholder";
function preparePlaceholderAttribute(attr) {
	return attr.replace(/\s+/g, "-").replace(/[^a-zA-Z0-9-]/g, "").replace(/^[0-9-]+/, "").replace(/^-+/, "").toLowerCase();
}
var Placeholder = Extension.create({
	name: "placeholder",
	addOptions() {
		return {
			emptyEditorClass: "is-editor-empty",
			emptyNodeClass: "is-empty",
			dataAttribute: DEFAULT_DATA_ATTRIBUTE,
			placeholder: "Write something …",
			showOnlyWhenEditable: true,
			showOnlyCurrent: true,
			includeChildren: false
		};
	},
	addProseMirrorPlugins() {
		const dataAttribute = this.options.dataAttribute ? `data-${preparePlaceholderAttribute(this.options.dataAttribute)}` : `data-${DEFAULT_DATA_ATTRIBUTE}`;
		return [new Plugin({
			key: new PluginKey("placeholder"),
			props: { decorations: ({ doc: doc$2, selection }) => {
				const active = this.editor.isEditable || !this.options.showOnlyWhenEditable;
				const { anchor } = selection;
				const decorations = [];
				if (!active) return null;
				const isEmptyDoc = this.editor.isEmpty;
				doc$2.descendants((node, pos) => {
					const hasAnchor = anchor >= pos && anchor <= pos + node.nodeSize;
					const isEmpty$1 = !node.isLeaf && isNodeEmpty(node);
					if (!node.type.isTextblock) return this.options.includeChildren;
					if ((hasAnchor || !this.options.showOnlyCurrent) && isEmpty$1) {
						const classes = [this.options.emptyNodeClass];
						if (isEmptyDoc) classes.push(this.options.emptyEditorClass);
						const decoration = Decoration.node(pos, pos + node.nodeSize, {
							class: classes.join(" "),
							[dataAttribute]: typeof this.options.placeholder === "function" ? this.options.placeholder({
								editor: this.editor,
								node,
								pos,
								hasAnchor
							}) : this.options.placeholder
						});
						decorations.push(decoration);
					}
					return this.options.includeChildren;
				});
				return DecorationSet.create(doc$2, decorations);
			} }
		})];
	}
});
Extension.create({
	name: "selection",
	addOptions() {
		return { className: "selection" };
	},
	addProseMirrorPlugins() {
		const { editor, options } = this;
		return [new Plugin({
			key: new PluginKey("selection"),
			props: { decorations(state) {
				if (state.selection.empty || editor.isFocused || !editor.isEditable || isNodeSelection(state.selection) || editor.view.dragging) return null;
				return DecorationSet.create(state.doc, [Decoration.inline(state.selection.from, state.selection.to, { class: options.className })]);
			} }
		})];
	}
});
function nodeEqualsType({ types, node }) {
	return node && Array.isArray(types) && types.includes(node.type) || (node == null ? void 0 : node.type) === types;
}
var TrailingNode = Extension.create({
	name: "trailingNode",
	addOptions() {
		return {
			node: void 0,
			notAfter: []
		};
	},
	addProseMirrorPlugins() {
		var _a;
		const plugin = new PluginKey(this.name);
		const defaultNode = this.options.node || ((_a = this.editor.schema.topNodeType.contentMatch.defaultType) == null ? void 0 : _a.name) || "paragraph";
		const disabledNodes = Object.entries(this.editor.schema.nodes).map(([, value]) => value).filter((node) => (this.options.notAfter || []).concat(defaultNode).includes(node.name));
		return [new Plugin({
			key: plugin,
			appendTransaction: (transactions, __, state) => {
				const { doc: doc$2, tr: tr$1, schema } = state;
				const shouldInsertNodeAtEnd = plugin.getState(state);
				const endPosition = doc$2.content.size;
				const type = schema.nodes[defaultNode];
				if (transactions.some((transaction) => transaction.getMeta("skipTrailingNode"))) return;
				if (!shouldInsertNodeAtEnd) return;
				return tr$1.insert(endPosition, type.create());
			},
			state: {
				init: (_$1, state) => {
					const lastNode = state.tr.doc.lastChild;
					return !nodeEqualsType({
						node: lastNode,
						types: disabledNodes
					});
				},
				apply: (tr$1, value) => {
					if (!tr$1.docChanged) return value;
					if (tr$1.getMeta("__uniqueIDTransaction")) return value;
					const lastNode = tr$1.doc.lastChild;
					return !nodeEqualsType({
						node: lastNode,
						types: disabledNodes
					});
				}
			}
		})];
	}
});
var UndoRedo = Extension.create({
	name: "undoRedo",
	addOptions() {
		return {
			depth: 100,
			newGroupDelay: 500
		};
	},
	addCommands() {
		return {
			undo: () => ({ state, dispatch }) => {
				return undo(state, dispatch);
			},
			redo: () => ({ state, dispatch }) => {
				return redo(state, dispatch);
			}
		};
	},
	addProseMirrorPlugins() {
		return [history(this.options)];
	},
	addKeyboardShortcuts() {
		return {
			"Mod-z": () => this.editor.commands.undo(),
			"Shift-Mod-z": () => this.editor.commands.redo(),
			"Mod-y": () => this.editor.commands.redo(),
			"Mod-я": () => this.editor.commands.undo(),
			"Shift-Mod-я": () => this.editor.commands.redo()
		};
	}
});
var index_default$4 = Placeholder;
var h = (tag, attributes) => {
	if (tag === "slot") return 0;
	if (tag instanceof Function) return tag(attributes);
	const { children, ...rest } = attributes != null ? attributes : {};
	if (tag === "svg") throw new Error("SVG elements are not supported in the JSX syntax, use the array syntax instead");
	return [
		tag,
		rest,
		children
	];
};
var inputRegex$4 = /^\s*>\s$/;
var Blockquote = Node3.create({
	name: "blockquote",
	addOptions() {
		return { HTMLAttributes: {} };
	},
	content: "block+",
	group: "block",
	defining: true,
	parseHTML() {
		return [{ tag: "blockquote" }];
	},
	renderHTML({ HTMLAttributes }) {
		return /* @__PURE__ */ h("blockquote", {
			...mergeAttributes(this.options.HTMLAttributes, HTMLAttributes),
			children: /* @__PURE__ */ h("slot", {})
		});
	},
	parseMarkdown: (token, helpers) => {
		var _a;
		const parseBlockChildren = (_a = helpers.parseBlockChildren) != null ? _a : helpers.parseChildren;
		return helpers.createNode("blockquote", void 0, parseBlockChildren(token.tokens || []));
	},
	renderMarkdown: (node, h$1) => {
		if (!node.content) return "";
		const prefix = ">";
		const result = [];
		node.content.forEach((child, index) => {
			var _a, _b;
			const linesWithPrefix = ((_b = (_a = h$1.renderChild) == null ? void 0 : _a.call(h$1, child, index)) != null ? _b : h$1.renderChildren([child])).split("\n").map((line) => {
				if (line.trim() === "") return prefix;
				return `${prefix} ${line}`;
			});
			result.push(linesWithPrefix.join("\n"));
		});
		return result.join(`
${prefix}
`);
	},
	addCommands() {
		return {
			setBlockquote: () => ({ commands }) => {
				return commands.wrapIn(this.name);
			},
			toggleBlockquote: () => ({ commands }) => {
				return commands.toggleWrap(this.name);
			},
			unsetBlockquote: () => ({ commands }) => {
				return commands.lift(this.name);
			}
		};
	},
	addKeyboardShortcuts() {
		return { "Mod-Shift-b": () => this.editor.commands.toggleBlockquote() };
	},
	addInputRules() {
		return [wrappingInputRule({
			find: inputRegex$4,
			type: this.type
		})];
	}
});
var starInputRegex$1 = /(?:^|\s)(\*\*(?!\s+\*\*)((?:[^*]+))\*\*(?!\s+\*\*))$/;
var starPasteRegex$1 = /(?:^|\s)(\*\*(?!\s+\*\*)((?:[^*]+))\*\*(?!\s+\*\*))/g;
var underscoreInputRegex$1 = /(?:^|\s)(__(?!\s+__)((?:[^_]+))__(?!\s+__))$/;
var underscorePasteRegex$1 = /(?:^|\s)(__(?!\s+__)((?:[^_]+))__(?!\s+__))/g;
var Bold = Mark.create({
	name: "bold",
	addOptions() {
		return { HTMLAttributes: {} };
	},
	parseHTML() {
		return [
			{ tag: "strong" },
			{
				tag: "b",
				getAttrs: (node) => node.style.fontWeight !== "normal" && null
			},
			{
				style: "font-weight=400",
				clearMark: (mark) => mark.type.name === this.name
			},
			{
				style: "font-weight",
				getAttrs: (value) => /^(bold(er)?|[5-9]\d{2,})$/.test(value) && null
			}
		];
	},
	renderHTML({ HTMLAttributes }) {
		return /* @__PURE__ */ h("strong", {
			...mergeAttributes(this.options.HTMLAttributes, HTMLAttributes),
			children: /* @__PURE__ */ h("slot", {})
		});
	},
	markdownTokenName: "strong",
	parseMarkdown: (token, helpers) => {
		return helpers.applyMark("bold", helpers.parseInline(token.tokens || []));
	},
	markdownOptions: { htmlReopen: {
		open: "<strong>",
		close: "</strong>"
	} },
	renderMarkdown: (node, h$1) => {
		return `**${h$1.renderChildren(node)}**`;
	},
	addCommands() {
		return {
			setBold: () => ({ commands }) => {
				return commands.setMark(this.name);
			},
			toggleBold: () => ({ commands }) => {
				return commands.toggleMark(this.name);
			},
			unsetBold: () => ({ commands }) => {
				return commands.unsetMark(this.name);
			}
		};
	},
	addKeyboardShortcuts() {
		return {
			"Mod-b": () => this.editor.commands.toggleBold(),
			"Mod-B": () => this.editor.commands.toggleBold()
		};
	},
	addInputRules() {
		return [markInputRule({
			find: starInputRegex$1,
			type: this.type
		}), markInputRule({
			find: underscoreInputRegex$1,
			type: this.type
		})];
	},
	addPasteRules() {
		return [markPasteRule({
			find: starPasteRegex$1,
			type: this.type
		}), markPasteRule({
			find: underscorePasteRegex$1,
			type: this.type
		})];
	}
});
var inputRegex$3 = /(^|[^`])`([^`]+)`(?!`)$/;
var pasteRegex$1 = /(^|[^`])`([^`]+)`(?!`)/g;
var Code = Mark.create({
	name: "code",
	addOptions() {
		return { HTMLAttributes: {} };
	},
	excludes: "_",
	code: true,
	exitable: true,
	parseHTML() {
		return [{ tag: "code" }];
	},
	renderHTML({ HTMLAttributes }) {
		return [
			"code",
			mergeAttributes(this.options.HTMLAttributes, HTMLAttributes),
			0
		];
	},
	markdownTokenName: "codespan",
	parseMarkdown: (token, helpers) => {
		return helpers.applyMark("code", [{
			type: "text",
			text: token.text || ""
		}]);
	},
	renderMarkdown: (node, h$1) => {
		if (!node.content) return "";
		return `\`${h$1.renderChildren(node.content)}\``;
	},
	addCommands() {
		return {
			setCode: () => ({ commands }) => {
				return commands.setMark(this.name);
			},
			toggleCode: () => ({ commands }) => {
				return commands.toggleMark(this.name);
			},
			unsetCode: () => ({ commands }) => {
				return commands.unsetMark(this.name);
			}
		};
	},
	addKeyboardShortcuts() {
		return { "Mod-e": () => this.editor.commands.toggleCode() };
	},
	addInputRules() {
		return [markInputRule({
			find: inputRegex$3,
			type: this.type
		})];
	},
	addPasteRules() {
		return [markPasteRule({
			find: pasteRegex$1,
			type: this.type
		})];
	}
});
var DEFAULT_TAB_SIZE = 4;
var backtickInputRegex = /^```([a-z]+)?[\s\n]$/;
var tildeInputRegex = /^~~~([a-z]+)?[\s\n]$/;
var CodeBlock = Node3.create({
	name: "codeBlock",
	addOptions() {
		return {
			languageClassPrefix: "language-",
			exitOnTripleEnter: true,
			exitOnArrowDown: true,
			defaultLanguage: null,
			enableTabIndentation: false,
			tabSize: DEFAULT_TAB_SIZE,
			HTMLAttributes: {}
		};
	},
	content: "text*",
	marks: "",
	group: "block",
	code: true,
	defining: true,
	addAttributes() {
		return { language: {
			default: this.options.defaultLanguage,
			parseHTML: (element) => {
				var _a;
				const { languageClassPrefix } = this.options;
				if (!languageClassPrefix) return null;
				const language = [...((_a = element.firstElementChild) == null ? void 0 : _a.classList) || []].filter((className) => className.startsWith(languageClassPrefix)).map((className) => className.replace(languageClassPrefix, ""))[0];
				if (!language) return null;
				return language;
			},
			rendered: false
		} };
	},
	parseHTML() {
		return [{
			tag: "pre",
			preserveWhitespace: "full"
		}];
	},
	renderHTML({ node, HTMLAttributes }) {
		return [
			"pre",
			mergeAttributes(this.options.HTMLAttributes, HTMLAttributes),
			[
				"code",
				{ class: node.attrs.language ? this.options.languageClassPrefix + node.attrs.language : null },
				0
			]
		];
	},
	markdownTokenName: "code",
	parseMarkdown: (token, helpers) => {
		var _a, _b;
		if (((_a = token.raw) == null ? void 0 : _a.startsWith("```")) === false && ((_b = token.raw) == null ? void 0 : _b.startsWith("~~~")) === false && token.codeBlockStyle !== "indented") return [];
		return helpers.createNode("codeBlock", { language: token.lang || null }, token.text ? [helpers.createTextNode(token.text)] : []);
	},
	renderMarkdown: (node, h$1) => {
		var _a;
		let output = "";
		const language = ((_a = node.attrs) == null ? void 0 : _a.language) || "";
		if (!node.content) output = `\`\`\`${language}

\`\`\``;
		else output = [
			`\`\`\`${language}`,
			h$1.renderChildren(node.content),
			"```"
		].join("\n");
		return output;
	},
	addCommands() {
		return {
			setCodeBlock: (attributes) => ({ commands }) => {
				return commands.setNode(this.name, attributes);
			},
			toggleCodeBlock: (attributes) => ({ commands }) => {
				return commands.toggleNode(this.name, "paragraph", attributes);
			}
		};
	},
	addKeyboardShortcuts() {
		return {
			"Mod-Alt-c": () => this.editor.commands.toggleCodeBlock(),
			Backspace: () => {
				const { empty: empty$1, $anchor } = this.editor.state.selection;
				const isAtStart = $anchor.pos === 1;
				if (!empty$1 || $anchor.parent.type.name !== this.name) return false;
				if (isAtStart || !$anchor.parent.textContent.length) return this.editor.commands.clearNodes();
				return false;
			},
			Tab: ({ editor }) => {
				var _a;
				if (!this.options.enableTabIndentation) return false;
				const tabSize = (_a = this.options.tabSize) != null ? _a : DEFAULT_TAB_SIZE;
				const { state } = editor;
				const { selection } = state;
				const { $from, empty: empty$1 } = selection;
				if ($from.parent.type !== this.type) return false;
				const indent = " ".repeat(tabSize);
				if (empty$1) return editor.commands.insertContent(indent);
				return editor.commands.command(({ tr: tr$1 }) => {
					const { from, to } = selection;
					const indentedText = state.doc.textBetween(from, to, "\n", "\n").split("\n").map((line) => indent + line).join("\n");
					tr$1.replaceWith(from, to, state.schema.text(indentedText));
					return true;
				});
			},
			"Shift-Tab": ({ editor }) => {
				var _a;
				if (!this.options.enableTabIndentation) return false;
				const tabSize = (_a = this.options.tabSize) != null ? _a : DEFAULT_TAB_SIZE;
				const { state } = editor;
				const { selection } = state;
				const { $from, empty: empty$1 } = selection;
				if ($from.parent.type !== this.type) return false;
				if (empty$1) return editor.commands.command(({ tr: tr$1 }) => {
					var _a2;
					const { pos } = $from;
					const codeBlockStart = $from.start();
					const codeBlockEnd = $from.end();
					const lines = state.doc.textBetween(codeBlockStart, codeBlockEnd, "\n", "\n").split("\n");
					let currentLineIndex = 0;
					let charCount = 0;
					const relativeCursorPos = pos - codeBlockStart;
					for (let i$1 = 0; i$1 < lines.length; i$1 += 1) {
						if (charCount + lines[i$1].length >= relativeCursorPos) {
							currentLineIndex = i$1;
							break;
						}
						charCount += lines[i$1].length + 1;
					}
					const leadingSpaces = ((_a2 = lines[currentLineIndex].match(/^ */)) == null ? void 0 : _a2[0]) || "";
					const spacesToRemove = Math.min(leadingSpaces.length, tabSize);
					if (spacesToRemove === 0) return true;
					let lineStartPos = codeBlockStart;
					for (let i$1 = 0; i$1 < currentLineIndex; i$1 += 1) lineStartPos += lines[i$1].length + 1;
					tr$1.delete(lineStartPos, lineStartPos + spacesToRemove);
					if (pos - lineStartPos <= spacesToRemove) tr$1.setSelection(TextSelection.create(tr$1.doc, lineStartPos));
					return true;
				});
				return editor.commands.command(({ tr: tr$1 }) => {
					const { from, to } = selection;
					const reverseIndentText = state.doc.textBetween(from, to, "\n", "\n").split("\n").map((line) => {
						var _a2;
						const leadingSpaces = ((_a2 = line.match(/^ */)) == null ? void 0 : _a2[0]) || "";
						const spacesToRemove = Math.min(leadingSpaces.length, tabSize);
						return line.slice(spacesToRemove);
					}).join("\n");
					tr$1.replaceWith(from, to, state.schema.text(reverseIndentText));
					return true;
				});
			},
			Enter: ({ editor }) => {
				if (!this.options.exitOnTripleEnter) return false;
				const { state } = editor;
				const { selection } = state;
				const { $from, empty: empty$1 } = selection;
				if (!empty$1 || $from.parent.type !== this.type) return false;
				const isAtEnd = $from.parentOffset === $from.parent.nodeSize - 2;
				const endsWithDoubleNewline = $from.parent.textContent.endsWith("\n\n");
				if (!isAtEnd || !endsWithDoubleNewline) return false;
				return editor.chain().command(({ tr: tr$1 }) => {
					tr$1.delete($from.pos - 2, $from.pos);
					return true;
				}).exitCode().run();
			},
			ArrowDown: ({ editor }) => {
				if (!this.options.exitOnArrowDown) return false;
				const { state } = editor;
				const { selection, doc: doc$2 } = state;
				const { $from, empty: empty$1 } = selection;
				if (!empty$1 || $from.parent.type !== this.type) return false;
				if (!($from.parentOffset === $from.parent.nodeSize - 2)) return false;
				const after = $from.after();
				if (after === void 0) return false;
				if (doc$2.nodeAt(after)) return editor.commands.command(({ tr: tr$1 }) => {
					tr$1.setSelection(Selection.near(doc$2.resolve(after)));
					return true;
				});
				return editor.commands.exitCode();
			}
		};
	},
	addInputRules() {
		return [textblockTypeInputRule({
			find: backtickInputRegex,
			type: this.type,
			getAttributes: (match) => ({ language: match[1] })
		}), textblockTypeInputRule({
			find: tildeInputRegex,
			type: this.type,
			getAttributes: (match) => ({ language: match[1] })
		})];
	},
	addProseMirrorPlugins() {
		return [new Plugin({
			key: new PluginKey("codeBlockVSCodeHandler"),
			props: { handlePaste: (view, event) => {
				if (!event.clipboardData) return false;
				if (this.editor.isActive(this.type.name)) return false;
				const text = event.clipboardData.getData("text/plain");
				const vscode = event.clipboardData.getData("vscode-editor-data");
				const vscodeData = vscode ? JSON.parse(vscode) : void 0;
				const language = vscodeData == null ? void 0 : vscodeData.mode;
				if (!text || !language) return false;
				const { tr: tr$1, schema } = view.state;
				const textNode = schema.text(text.replace(/\r\n?/g, "\n"));
				tr$1.replaceSelectionWith(this.type.create({ language }, textNode));
				if (tr$1.selection.$from.parent.type !== this.type) tr$1.setSelection(TextSelection.near(tr$1.doc.resolve(Math.max(0, tr$1.selection.from - 2))));
				tr$1.setMeta("paste", true);
				view.dispatch(tr$1);
				return true;
			} }
		})];
	}
});
var Document = Node3.create({
	name: "doc",
	topNode: true,
	content: "block+",
	renderMarkdown: (node, h$1) => {
		if (!node.content) return "";
		return h$1.renderChildren(node.content, "\n\n");
	}
});
var HardBreak = Node3.create({
	name: "hardBreak",
	markdownTokenName: "br",
	addOptions() {
		return {
			keepMarks: true,
			HTMLAttributes: {}
		};
	},
	inline: true,
	group: "inline",
	selectable: false,
	linebreakReplacement: true,
	parseHTML() {
		return [{ tag: "br" }];
	},
	renderHTML({ HTMLAttributes }) {
		return ["br", mergeAttributes(this.options.HTMLAttributes, HTMLAttributes)];
	},
	renderText() {
		return "\n";
	},
	renderMarkdown: () => `  
`,
	parseMarkdown: () => {
		return { type: "hardBreak" };
	},
	addCommands() {
		return { setHardBreak: () => ({ commands, chain, state, editor }) => {
			return commands.first([() => commands.exitCode(), () => commands.command(() => {
				const { selection, storedMarks } = state;
				if (selection.$from.parent.type.spec.isolating) return false;
				const { keepMarks } = this.options;
				const { splittableMarks } = editor.extensionManager;
				const marks = storedMarks || selection.$to.parentOffset && selection.$from.marks();
				return chain().insertContent({ type: this.name }).command(({ tr: tr$1, dispatch }) => {
					if (dispatch && marks && keepMarks) {
						const filteredMarks = marks.filter((mark) => splittableMarks.includes(mark.type.name));
						tr$1.ensureMarks(filteredMarks);
					}
					return true;
				}).run();
			})]);
		} };
	},
	addKeyboardShortcuts() {
		return {
			"Mod-Enter": () => this.editor.commands.setHardBreak(),
			"Shift-Enter": () => this.editor.commands.setHardBreak()
		};
	}
});
var Heading = Node3.create({
	name: "heading",
	addOptions() {
		return {
			levels: [
				1,
				2,
				3,
				4,
				5,
				6
			],
			HTMLAttributes: {}
		};
	},
	content: "inline*",
	group: "block",
	defining: true,
	addAttributes() {
		return { level: {
			default: 1,
			rendered: false
		} };
	},
	parseHTML() {
		return this.options.levels.map((level) => ({
			tag: `h${level}`,
			attrs: { level }
		}));
	},
	renderHTML({ node, HTMLAttributes }) {
		return [
			`h${this.options.levels.includes(node.attrs.level) ? node.attrs.level : this.options.levels[0]}`,
			mergeAttributes(this.options.HTMLAttributes, HTMLAttributes),
			0
		];
	},
	parseMarkdown: (token, helpers) => {
		return helpers.createNode("heading", { level: token.depth || 1 }, helpers.parseInline(token.tokens || []));
	},
	renderMarkdown: (node, h$1) => {
		var _a;
		const level = ((_a = node.attrs) == null ? void 0 : _a.level) ? parseInt(node.attrs.level, 10) : 1;
		const headingChars = "#".repeat(level);
		if (!node.content) return "";
		return `${headingChars} ${h$1.renderChildren(node.content)}`;
	},
	addCommands() {
		return {
			setHeading: (attributes) => ({ commands }) => {
				if (!this.options.levels.includes(attributes.level)) return false;
				return commands.setNode(this.name, attributes);
			},
			toggleHeading: (attributes) => ({ commands }) => {
				if (!this.options.levels.includes(attributes.level)) return false;
				return commands.toggleNode(this.name, "paragraph", attributes);
			}
		};
	},
	addKeyboardShortcuts() {
		return this.options.levels.reduce((items, level) => ({
			...items,
			[`Mod-Alt-${level}`]: () => this.editor.commands.toggleHeading({ level })
		}), {});
	},
	addInputRules() {
		return this.options.levels.map((level) => {
			return textblockTypeInputRule({
				find: /* @__PURE__ */ new RegExp(`^(#{${Math.min(...this.options.levels)},${level}})\\s$`),
				type: this.type,
				getAttributes: { level }
			});
		});
	}
});
var HorizontalRule = Node3.create({
	name: "horizontalRule",
	addOptions() {
		return {
			HTMLAttributes: {},
			nextNodeType: "paragraph"
		};
	},
	group: "block",
	parseHTML() {
		return [{ tag: "hr" }];
	},
	renderHTML({ HTMLAttributes }) {
		return ["hr", mergeAttributes(this.options.HTMLAttributes, HTMLAttributes)];
	},
	markdownTokenName: "hr",
	parseMarkdown: (token, helpers) => {
		return helpers.createNode("horizontalRule");
	},
	renderMarkdown: () => {
		return "---";
	},
	addCommands() {
		return { setHorizontalRule: () => ({ chain, state }) => {
			if (!canInsertNode(state, state.schema.nodes[this.name])) return false;
			const { selection } = state;
			const { $to: $originTo } = selection;
			const currentChain = chain();
			if (isNodeSelection(selection)) currentChain.insertContentAt($originTo.pos, { type: this.name });
			else currentChain.insertContent({ type: this.name });
			return currentChain.command(({ state: chainState, tr: tr$1, dispatch }) => {
				if (dispatch) {
					const { $to } = tr$1.selection;
					const posAfter = $to.end();
					if ($to.nodeAfter) if ($to.nodeAfter.isTextblock) tr$1.setSelection(TextSelection.create(tr$1.doc, $to.pos + 1));
					else if ($to.nodeAfter.isBlock) tr$1.setSelection(NodeSelection.create(tr$1.doc, $to.pos));
					else tr$1.setSelection(TextSelection.create(tr$1.doc, $to.pos));
					else {
						const nodeType = chainState.schema.nodes[this.options.nextNodeType] || $to.parent.type.contentMatch.defaultType;
						const node = nodeType == null ? void 0 : nodeType.create();
						if (node) {
							tr$1.insert(posAfter, node);
							tr$1.setSelection(TextSelection.create(tr$1.doc, posAfter + 1));
						}
					}
					tr$1.scrollIntoView();
				}
				return true;
			}).run();
		} };
	},
	addInputRules() {
		return [nodeInputRule({
			find: /^(?:---|—-|___\s|\*\*\*\s)$/,
			type: this.type
		})];
	}
});
var starInputRegex = /(?:^|\s)(\*(?!\s+\*)((?:[^*]+))\*(?!\s+\*))$/;
var starPasteRegex = /(?:^|\s)(\*(?!\s+\*)((?:[^*]+))\*(?!\s+\*))/g;
var underscoreInputRegex = /(?:^|\s)(_(?!\s+_)((?:[^_]+))_(?!\s+_))$/;
var underscorePasteRegex = /(?:^|\s)(_(?!\s+_)((?:[^_]+))_(?!\s+_))/g;
var Italic = Mark.create({
	name: "italic",
	addOptions() {
		return { HTMLAttributes: {} };
	},
	parseHTML() {
		return [
			{ tag: "em" },
			{
				tag: "i",
				getAttrs: (node) => node.style.fontStyle !== "normal" && null
			},
			{
				style: "font-style=normal",
				clearMark: (mark) => mark.type.name === this.name
			},
			{ style: "font-style=italic" }
		];
	},
	renderHTML({ HTMLAttributes }) {
		return [
			"em",
			mergeAttributes(this.options.HTMLAttributes, HTMLAttributes),
			0
		];
	},
	addCommands() {
		return {
			setItalic: () => ({ commands }) => {
				return commands.setMark(this.name);
			},
			toggleItalic: () => ({ commands }) => {
				return commands.toggleMark(this.name);
			},
			unsetItalic: () => ({ commands }) => {
				return commands.unsetMark(this.name);
			}
		};
	},
	markdownTokenName: "em",
	parseMarkdown: (token, helpers) => {
		return helpers.applyMark("italic", helpers.parseInline(token.tokens || []));
	},
	markdownOptions: { htmlReopen: {
		open: "<em>",
		close: "</em>"
	} },
	renderMarkdown: (node, h$1) => {
		return `*${h$1.renderChildren(node)}*`;
	},
	addKeyboardShortcuts() {
		return {
			"Mod-i": () => this.editor.commands.toggleItalic(),
			"Mod-I": () => this.editor.commands.toggleItalic()
		};
	},
	addInputRules() {
		return [markInputRule({
			find: starInputRegex,
			type: this.type
		}), markInputRule({
			find: underscoreInputRegex,
			type: this.type
		})];
	},
	addPasteRules() {
		return [markPasteRule({
			find: starPasteRegex,
			type: this.type
		}), markPasteRule({
			find: underscorePasteRegex,
			type: this.type
		})];
	}
});
var encodedTlds = "aaa1rp3bb0ott3vie4c1le2ogado5udhabi7c0ademy5centure6ountant0s9o1tor4d0s1ult4e0g1ro2tna4f0l1rica5g0akhan5ency5i0g1rbus3force5tel5kdn3l0ibaba4pay4lfinanz6state5y2sace3tom5m0azon4ericanexpress7family11x2fam3ica3sterdam8nalytics7droid5quan4z2o0l2partments8p0le4q0uarelle8r0ab1mco4chi3my2pa2t0e3s0da2ia2sociates9t0hleta5torney7u0ction5di0ble3o3spost5thor3o0s4w0s2x0a2z0ure5ba0by2idu3namex4d1k2r0celona5laycard4s5efoot5gains6seball5ketball8uhaus5yern5b0c1t1va3cg1n2d1e0ats2uty4er2rlin4st0buy5t2f1g1h0arti5i0ble3d1ke2ng0o3o1z2j1lack0friday9ockbuster8g1omberg7ue3m0s1w2n0pparibas9o0ats3ehringer8fa2m1nd2o0k0ing5sch2tik2on4t1utique6x2r0adesco6idgestone9oadway5ker3ther5ussels7s1t1uild0ers6siness6y1zz3v1w1y1z0h3ca0b1fe2l0l1vinklein9m0era3p2non3petown5ital0one8r0avan4ds2e0er0s4s2sa1e1h1ino4t0ering5holic7ba1n1re3c1d1enter4o1rn3f0a1d2g1h0anel2nel4rity4se2t2eap3intai5ristmas6ome4urch5i0priani6rcle4sco3tadel4i0c2y3k1l0aims4eaning6ick2nic1que6othing5ud3ub0med6m1n1o0ach3des3ffee4llege4ogne5m0mbank4unity6pany2re3uter5sec4ndos3struction8ulting7tact3ractors9oking4l1p2rsica5untry4pon0s4rses6pa2r0edit0card4union9icket5own3s1uise0s6u0isinella9v1w1x1y0mru3ou3z2dad1nce3ta1e1ing3sun4y2clk3ds2e0al0er2s3gree4livery5l1oitte5ta3mocrat6ntal2ist5si0gn4v2hl2iamonds6et2gital5rect0ory7scount3ver5h2y2j1k1m1np2o0cs1tor4g1mains5t1wnload7rive4tv2ubai3nlop4pont4rban5vag2r2z2earth3t2c0o2deka3u0cation8e1g1mail3erck5nergy4gineer0ing9terprises10pson4quipment8r0icsson6ni3s0q1tate5t1u0rovision8s2vents5xchange6pert3osed4ress5traspace10fage2il1rwinds6th3mily4n0s2rm0ers5shion4t3edex3edback6rrari3ero6i0delity5o2lm2nal1nce1ial7re0stone6mdale6sh0ing5t0ness6j1k1lickr3ghts4r2orist4wers5y2m1o0o0d1tball6rd1ex2sale4um3undation8x2r0ee1senius7l1ogans4ntier7tr2ujitsu5n0d2rniture7tbol5yi3ga0l0lery3o1up4me0s3p1rden4y2b0iz3d0n2e0a1nt0ing5orge5f1g0ee3h1i0ft0s3ves2ing5l0ass3e1obal2o4m0ail3bh2o1x2n1odaddy5ld0point6f2o0dyear5g0le4p1t1v2p1q1r0ainger5phics5tis4een3ipe3ocery4up4s1t1u0cci3ge2ide2tars5ru3w1y2hair2mburg5ngout5us3bo2dfc0bank7ealth0care8lp1sinki6re1mes5iphop4samitsu7tachi5v2k0t2m1n1ockey4ldings5iday5medepot5goods5s0ense7nda3rse3spital5t0ing5t0els3mail5use3w2r1sbc3t1u0ghes5yatt3undai7ibm2cbc2e1u2d1e0ee3fm2kano4l1m0amat4db2mo0bilien9n0c1dustries8finiti5o2g1k1stitute6urance4e4t0ernational10uit4vestments10o1piranga7q1r0ish4s0maili5t0anbul7t0au2v3jaguar4va3cb2e0ep2tzt3welry6io2ll2m0p2nj2o0bs1urg4t1y2p0morgan6rs3uegos4niper7kaufen5ddi3e0rryhotels6properties14fh2g1h1i0a1ds2m1ndle4tchen5wi3m1n1oeln3matsu5sher5p0mg2n2r0d1ed3uokgroup8w1y0oto4z2la0caixa5mborghini8er3nd0rover6xess5salle5t0ino3robe5w0yer5b1c1ds2ease3clerc5frak4gal2o2xus4gbt3i0dl2fe0insurance9style7ghting6ke2lly3mited4o2ncoln4k2ve1ing5k1lc1p2oan0s3cker3us3l1ndon4tte1o3ve3pl0financial11r1s1t0d0a3u0ndbeck6xe1ury5v1y2ma0drid4if1son4keup4n0agement7go3p1rket0ing3s4riott5shalls7ttel5ba2c0kinsey7d1e0d0ia3et2lbourne7me1orial6n0u2rckmsd7g1h1iami3crosoft7l1ni1t2t0subishi9k1l0b1s2m0a2n1o0bi0le4da2e1i1m1nash3ey2ster5rmon3tgage6scow4to0rcycles9v0ie4p1q1r1s0d2t0n1r2u0seum3ic4v1w1x1y1z2na0b1goya4me2vy3ba2c1e0c1t0bank4flix4work5ustar5w0s2xt0direct7us4f0l2g0o2hk2i0co2ke1on3nja3ssan1y5l1o0kia3rton4w0ruz3tv4p1r0a1w2tt2u1yc2z2obi1server7ffice5kinawa6layan0group9lo3m0ega4ne1g1l0ine5oo2pen3racle3nge4g0anic5igins6saka4tsuka4t2vh3pa0ge2nasonic7ris2s1tners4s1y3y2ccw3e0t2f0izer5g1h0armacy6d1ilips5one2to0graphy6s4ysio5ics1tet2ures6d1n0g1k2oneer5zza4k1l0ace2y0station9umbing5s3m1n0c2ohl2ker3litie5rn2st3r0axi3ess3ime3o0d0uctions8f1gressive8mo2perties3y5tection8u0dential9s1t1ub2w0c2y2qa1pon3uebec3st5racing4dio4e0ad1lestate6tor2y4cipes5d0stone5umbrella9hab3ise0n3t2liance6n0t0als5pair3ort3ublican8st0aurant8view0s5xroth6ich0ardli6oh3l1o1p2o0cks3deo3gers4om3s0vp3u0gby3hr2n2w0e2yukyu6sa0arland6fe0ty4kura4le1on3msclub4ung5ndvik0coromant12ofi4p1rl2s1ve2xo3b0i1s2c0b1haeffler7midt4olarships8ol3ule3warz5ience5ot3d1e0arch3t2cure1ity6ek2lect4ner3rvices6ven3w1x0y3fr2g1h0angrila6rp3ell3ia1ksha5oes2p0ping5uji3w3i0lk2na1gles5te3j1k0i0n2y0pe4l0ing4m0art3ile4n0cf3o0ccer3ial4ftbank4ware6hu2lar2utions7ng1y2y2pa0ce3ort2t3r0l2s1t0ada2ples4r1tebank4farm7c0group6ockholm6rage3e3ream4udio2y3yle4u0cks3pplies3y2ort5rf1gery5zuki5v1watch4iss4x1y0dney4stems6z2tab1ipei4lk2obao4rget4tamotors6r2too4x0i3c0i2d0k2eam2ch0nology8l1masek5nnis4va3f1g1h0d1eater2re6iaa2ckets5enda4ps2res2ol4j0maxx4x2k0maxx5l1m0all4n1o0day3kyo3ols3p1ray3shiba5tal3urs3wn2yota3s3r0ade1ing4ining5vel0ers0insurance16ust3v2t1ube2i1nes3shu4v0s2w1z2ua1bank3s2g1k1nicom3versity8o2ol2ps2s1y1z2va0cations7na1guard7c1e0gas3ntures6risign5mögensberater2ung14sicherung10t2g1i0ajes4deo3g1king4llas4n1p1rgin4sa1ion4va1o3laanderen9n1odka3lvo3te1ing3o2yage5u2wales2mart4ter4ng0gou5tch0es6eather0channel12bcam3er2site5d0ding5ibo2r3f1hoswho6ien2ki2lliamhill9n0dows4e1ners6me2olterskluwer11odside6rk0s2ld3w2s1tc1f3xbox3erox4ihuan4n2xx2yz3yachts4hoo3maxun5ndex5e1odobashi7ga2kohama6u0tube6t1un3za0ppos4ra3ero3ip2m1one3uerich6w2";
var encodedUtlds = "ελ1υ2бг1ел3дети4ею2католик6ом3мкд2он1сква6онлайн5рг3рус2ф2сайт3рб3укр3қаз3հայ3ישראל5קום3ابوظبي5رامكو5لاردن4بحرين5جزائر5سعودية6عليان5مغرب5مارات5یران5بارت2زار4يتك3ھارت5تونس4سودان3رية5شبكة4عراق2ب2مان4فلسطين6قطر3كاثوليك6وم3مصر2ليسيا5وريتانيا7قع4همراه5پاکستان7ڀارت4कॉम3नेट3भारत0म्3ोत5संगठन5বাংলা5ভারত2ৰত4ਭਾਰਤ4ભારત4ଭାରତ4இந்தியா6லங்கை6சிங்கப்பூர்11భారత్5ಭಾರತ4ഭാരതം5ලංකා4คอม3ไทย3ລາວ3გე2みんな3アマゾン4クラウド4グーグル4コム2ストア3セール3ファッション6ポイント4世界2中信1国1國1文网3亚马逊3企业2佛山2信息2健康2八卦2公司1益2台湾1灣2商城1店1标2嘉里0大酒店5在线2大拿2天主教3娱乐2家電2广东2微博2慈善2我爱你3手机2招聘2政务1府2新加坡2闻2时尚2書籍2机构2淡马锡3游戏2澳門2点看2移动2组织机构4网址1店1站1络2联通2谷歌2购物2通販2集团2電訊盈科4飞利浦3食品2餐厅2香格里拉3港2닷넷1컴2삼성2한국2";
var numeric = "numeric";
var ascii = "ascii";
var alpha = "alpha";
var asciinumeric = "asciinumeric";
var alphanumeric = "alphanumeric";
var domain = "domain";
var emoji = "emoji";
var scheme = "scheme";
var slashscheme = "slashscheme";
var whitespace = "whitespace";
function registerGroup(name, groups) {
	if (!(name in groups)) groups[name] = [];
	return groups[name];
}
function addToGroups(t, flags, groups) {
	if (flags[numeric]) {
		flags[asciinumeric] = true;
		flags[alphanumeric] = true;
	}
	if (flags[ascii]) {
		flags[asciinumeric] = true;
		flags[alpha] = true;
	}
	if (flags[asciinumeric]) flags[alphanumeric] = true;
	if (flags[alpha]) flags[alphanumeric] = true;
	if (flags[alphanumeric]) flags[domain] = true;
	if (flags[emoji]) flags[domain] = true;
	for (const k$1 in flags) {
		const group = registerGroup(k$1, groups);
		if (group.indexOf(t) < 0) group.push(t);
	}
}
function flagsForToken(t, groups) {
	const result = {};
	for (const c in groups) if (groups[c].indexOf(t) >= 0) result[c] = true;
	return result;
}
function State(token = null) {
	this.j = {};
	this.jr = [];
	this.jd = null;
	this.t = token;
}
State.groups = {};
State.prototype = {
	accepts() {
		return !!this.t;
	},
	go(input) {
		const state = this;
		const nextState = state.j[input];
		if (nextState) return nextState;
		for (let i$1 = 0; i$1 < state.jr.length; i$1++) {
			const regex = state.jr[i$1][0];
			const nextState$1 = state.jr[i$1][1];
			if (nextState$1 && regex.test(input)) return nextState$1;
		}
		return state.jd;
	},
	has(input, exactOnly = false) {
		return exactOnly ? input in this.j : !!this.go(input);
	},
	ta(inputs, next, flags, groups) {
		for (let i$1 = 0; i$1 < inputs.length; i$1++) this.tt(inputs[i$1], next, flags, groups);
	},
	tr(regexp, next, flags, groups) {
		groups = groups || State.groups;
		let nextState;
		if (next && next.j) nextState = next;
		else {
			nextState = new State(next);
			if (flags && groups) addToGroups(next, flags, groups);
		}
		this.jr.push([regexp, nextState]);
		return nextState;
	},
	ts(input, next, flags, groups) {
		let state = this;
		const len = input.length;
		if (!len) return state;
		for (let i$1 = 0; i$1 < len - 1; i$1++) state = state.tt(input[i$1]);
		return state.tt(input[len - 1], next, flags, groups);
	},
	tt(input, next, flags, groups) {
		groups = groups || State.groups;
		const state = this;
		if (next && next.j) {
			state.j[input] = next;
			return next;
		}
		const t = next;
		let nextState, templateState = state.go(input);
		if (templateState) {
			nextState = new State();
			Object.assign(nextState.j, templateState.j);
			nextState.jr.push.apply(nextState.jr, templateState.jr);
			nextState.jd = templateState.jd;
			nextState.t = templateState.t;
		} else nextState = new State();
		if (t) {
			if (groups) {
				if (nextState.t && typeof nextState.t === "string") addToGroups(t, Object.assign(flagsForToken(nextState.t, groups), flags), groups);
				else if (flags) addToGroups(t, flags, groups);
			}
			nextState.t = t;
		}
		state.j[input] = nextState;
		return nextState;
	}
};
var ta = (state, input, next, flags, groups) => state.ta(input, next, flags, groups);
var tr = (state, regexp, next, flags, groups) => state.tr(regexp, next, flags, groups);
var ts = (state, input, next, flags, groups) => state.ts(input, next, flags, groups);
var tt$1 = (state, input, next, flags, groups) => state.tt(input, next, flags, groups);
var WORD = "WORD";
var UWORD = "UWORD";
var ASCIINUMERICAL = "ASCIINUMERICAL";
var ALPHANUMERICAL = "ALPHANUMERICAL";
var LOCALHOST = "LOCALHOST";
var TLD = "TLD";
var UTLD = "UTLD";
var SCHEME = "SCHEME";
var SLASH_SCHEME = "SLASH_SCHEME";
var NUM = "NUM";
var WS = "WS";
var NL = "NL";
var OPENBRACE = "OPENBRACE";
var CLOSEBRACE = "CLOSEBRACE";
var OPENBRACKET = "OPENBRACKET";
var CLOSEBRACKET = "CLOSEBRACKET";
var OPENPAREN = "OPENPAREN";
var CLOSEPAREN = "CLOSEPAREN";
var OPENANGLEBRACKET = "OPENANGLEBRACKET";
var CLOSEANGLEBRACKET = "CLOSEANGLEBRACKET";
var FULLWIDTHLEFTPAREN = "FULLWIDTHLEFTPAREN";
var FULLWIDTHRIGHTPAREN = "FULLWIDTHRIGHTPAREN";
var LEFTCORNERBRACKET = "LEFTCORNERBRACKET";
var RIGHTCORNERBRACKET = "RIGHTCORNERBRACKET";
var LEFTWHITECORNERBRACKET = "LEFTWHITECORNERBRACKET";
var RIGHTWHITECORNERBRACKET = "RIGHTWHITECORNERBRACKET";
var FULLWIDTHLESSTHAN = "FULLWIDTHLESSTHAN";
var FULLWIDTHGREATERTHAN = "FULLWIDTHGREATERTHAN";
var AMPERSAND = "AMPERSAND";
var APOSTROPHE = "APOSTROPHE";
var ASTERISK = "ASTERISK";
var AT = "AT";
var BACKSLASH = "BACKSLASH";
var BACKTICK = "BACKTICK";
var CARET = "CARET";
var COLON = "COLON";
var COMMA = "COMMA";
var DOLLAR = "DOLLAR";
var DOT = "DOT";
var EQUALS = "EQUALS";
var EXCLAMATION = "EXCLAMATION";
var HYPHEN = "HYPHEN";
var PERCENT = "PERCENT";
var PIPE = "PIPE";
var PLUS = "PLUS";
var POUND = "POUND";
var QUERY = "QUERY";
var QUOTE = "QUOTE";
var FULLWIDTHMIDDLEDOT = "FULLWIDTHMIDDLEDOT";
var SEMI = "SEMI";
var SLASH = "SLASH";
var TILDE = "TILDE";
var UNDERSCORE = "UNDERSCORE";
var EMOJI$1 = "EMOJI";
var SYM = "SYM";
var tk = /* @__PURE__ */ Object.freeze({
	__proto__: null,
	ALPHANUMERICAL,
	AMPERSAND,
	APOSTROPHE,
	ASCIINUMERICAL,
	ASTERISK,
	AT,
	BACKSLASH,
	BACKTICK,
	CARET,
	CLOSEANGLEBRACKET,
	CLOSEBRACE,
	CLOSEBRACKET,
	CLOSEPAREN,
	COLON,
	COMMA,
	DOLLAR,
	DOT,
	EMOJI: EMOJI$1,
	EQUALS,
	EXCLAMATION,
	FULLWIDTHGREATERTHAN,
	FULLWIDTHLEFTPAREN,
	FULLWIDTHLESSTHAN,
	FULLWIDTHMIDDLEDOT,
	FULLWIDTHRIGHTPAREN,
	HYPHEN,
	LEFTCORNERBRACKET,
	LEFTWHITECORNERBRACKET,
	LOCALHOST,
	NL,
	NUM,
	OPENANGLEBRACKET,
	OPENBRACE,
	OPENBRACKET,
	OPENPAREN,
	PERCENT,
	PIPE,
	PLUS,
	POUND,
	QUERY,
	QUOTE,
	RIGHTCORNERBRACKET,
	RIGHTWHITECORNERBRACKET,
	SCHEME,
	SEMI,
	SLASH,
	SLASH_SCHEME,
	SYM,
	TILDE,
	TLD,
	UNDERSCORE,
	UTLD,
	UWORD,
	WORD,
	WS
});
var ASCII_LETTER = /[a-z]/;
var LETTER = /\p{L}/u;
var EMOJI = /\p{Emoji}/u;
var DIGIT = /\d/;
var SPACE = /\s/;
var CR = "\r";
var LF = "\n";
var EMOJI_VARIATION = "️";
var EMOJI_JOINER = "‍";
var OBJECT_REPLACEMENT = "￼";
var tlds = null, utlds = null;
function init$2(customSchemes = []) {
	const groups = {};
	State.groups = groups;
	const Start = new State();
	if (tlds == null) tlds = decodeTlds(encodedTlds);
	if (utlds == null) utlds = decodeTlds(encodedUtlds);
	tt$1(Start, "'", APOSTROPHE);
	tt$1(Start, "{", OPENBRACE);
	tt$1(Start, "}", CLOSEBRACE);
	tt$1(Start, "[", OPENBRACKET);
	tt$1(Start, "]", CLOSEBRACKET);
	tt$1(Start, "(", OPENPAREN);
	tt$1(Start, ")", CLOSEPAREN);
	tt$1(Start, "<", OPENANGLEBRACKET);
	tt$1(Start, ">", CLOSEANGLEBRACKET);
	tt$1(Start, "（", FULLWIDTHLEFTPAREN);
	tt$1(Start, "）", FULLWIDTHRIGHTPAREN);
	tt$1(Start, "「", LEFTCORNERBRACKET);
	tt$1(Start, "」", RIGHTCORNERBRACKET);
	tt$1(Start, "『", LEFTWHITECORNERBRACKET);
	tt$1(Start, "』", RIGHTWHITECORNERBRACKET);
	tt$1(Start, "＜", FULLWIDTHLESSTHAN);
	tt$1(Start, "＞", FULLWIDTHGREATERTHAN);
	tt$1(Start, "&", AMPERSAND);
	tt$1(Start, "*", ASTERISK);
	tt$1(Start, "@", AT);
	tt$1(Start, "`", BACKTICK);
	tt$1(Start, "^", CARET);
	tt$1(Start, ":", COLON);
	tt$1(Start, ",", COMMA);
	tt$1(Start, "$", DOLLAR);
	tt$1(Start, ".", DOT);
	tt$1(Start, "=", EQUALS);
	tt$1(Start, "!", EXCLAMATION);
	tt$1(Start, "-", HYPHEN);
	tt$1(Start, "%", PERCENT);
	tt$1(Start, "|", PIPE);
	tt$1(Start, "+", PLUS);
	tt$1(Start, "#", POUND);
	tt$1(Start, "?", QUERY);
	tt$1(Start, "\"", QUOTE);
	tt$1(Start, "/", SLASH);
	tt$1(Start, ";", SEMI);
	tt$1(Start, "~", TILDE);
	tt$1(Start, "_", UNDERSCORE);
	tt$1(Start, "\\", BACKSLASH);
	tt$1(Start, "・", FULLWIDTHMIDDLEDOT);
	const Num = tr(Start, DIGIT, NUM, { [numeric]: true });
	tr(Num, DIGIT, Num);
	const Asciinumeric = tr(Num, ASCII_LETTER, ASCIINUMERICAL, { [asciinumeric]: true });
	const Alphanumeric = tr(Num, LETTER, ALPHANUMERICAL, { [alphanumeric]: true });
	const Word = tr(Start, ASCII_LETTER, WORD, { [ascii]: true });
	tr(Word, DIGIT, Asciinumeric);
	tr(Word, ASCII_LETTER, Word);
	tr(Asciinumeric, DIGIT, Asciinumeric);
	tr(Asciinumeric, ASCII_LETTER, Asciinumeric);
	const UWord = tr(Start, LETTER, UWORD, { [alpha]: true });
	tr(UWord, ASCII_LETTER);
	tr(UWord, DIGIT, Alphanumeric);
	tr(UWord, LETTER, UWord);
	tr(Alphanumeric, DIGIT, Alphanumeric);
	tr(Alphanumeric, ASCII_LETTER);
	tr(Alphanumeric, LETTER, Alphanumeric);
	const Nl$1 = tt$1(Start, LF, NL, { [whitespace]: true });
	const Cr = tt$1(Start, CR, WS, { [whitespace]: true });
	const Ws = tr(Start, SPACE, WS, { [whitespace]: true });
	tt$1(Start, OBJECT_REPLACEMENT, Ws);
	tt$1(Cr, LF, Nl$1);
	tt$1(Cr, OBJECT_REPLACEMENT, Ws);
	tr(Cr, SPACE, Ws);
	tt$1(Ws, CR);
	tt$1(Ws, LF);
	tr(Ws, SPACE, Ws);
	tt$1(Ws, OBJECT_REPLACEMENT, Ws);
	const Emoji = tr(Start, EMOJI, EMOJI$1, { [emoji]: true });
	tt$1(Emoji, "#");
	tr(Emoji, EMOJI, Emoji);
	tt$1(Emoji, EMOJI_VARIATION, Emoji);
	const EmojiJoiner = tt$1(Emoji, EMOJI_JOINER);
	tt$1(EmojiJoiner, "#");
	tr(EmojiJoiner, EMOJI, Emoji);
	const wordjr = [[ASCII_LETTER, Word], [DIGIT, Asciinumeric]];
	const uwordjr = [
		[ASCII_LETTER, null],
		[LETTER, UWord],
		[DIGIT, Alphanumeric]
	];
	for (let i$1 = 0; i$1 < tlds.length; i$1++) fastts(Start, tlds[i$1], TLD, WORD, wordjr);
	for (let i$1 = 0; i$1 < utlds.length; i$1++) fastts(Start, utlds[i$1], UTLD, UWORD, uwordjr);
	addToGroups(TLD, {
		tld: true,
		ascii: true
	}, groups);
	addToGroups(UTLD, {
		utld: true,
		alpha: true
	}, groups);
	fastts(Start, "file", SCHEME, WORD, wordjr);
	fastts(Start, "mailto", SCHEME, WORD, wordjr);
	fastts(Start, "http", SLASH_SCHEME, WORD, wordjr);
	fastts(Start, "https", SLASH_SCHEME, WORD, wordjr);
	fastts(Start, "ftp", SLASH_SCHEME, WORD, wordjr);
	fastts(Start, "ftps", SLASH_SCHEME, WORD, wordjr);
	addToGroups(SCHEME, {
		scheme: true,
		ascii: true
	}, groups);
	addToGroups(SLASH_SCHEME, {
		slashscheme: true,
		ascii: true
	}, groups);
	customSchemes = customSchemes.sort((a, b$1) => a[0] > b$1[0] ? 1 : -1);
	for (let i$1 = 0; i$1 < customSchemes.length; i$1++) {
		const sch = customSchemes[i$1][0];
		const flags = customSchemes[i$1][1] ? { [scheme]: true } : { [slashscheme]: true };
		if (sch.indexOf("-") >= 0) flags[domain] = true;
		else if (!ASCII_LETTER.test(sch)) flags[numeric] = true;
		else if (DIGIT.test(sch)) flags[asciinumeric] = true;
		else flags[ascii] = true;
		ts(Start, sch, sch, flags);
	}
	ts(Start, "localhost", LOCALHOST, { ascii: true });
	Start.jd = new State(SYM);
	return {
		start: Start,
		tokens: Object.assign({ groups }, tk)
	};
}
function run$1(start, str) {
	const iterable = stringToArray(str.replace(/[A-Z]/g, (c) => c.toLowerCase()));
	const charCount = iterable.length;
	const tokens = [];
	let cursor = 0;
	let charCursor = 0;
	while (charCursor < charCount) {
		let state = start;
		let nextState = null;
		let tokenLength = 0;
		let latestAccepting = null;
		let sinceAccepts = -1;
		let charsSinceAccepts = -1;
		while (charCursor < charCount && (nextState = state.go(iterable[charCursor]))) {
			state = nextState;
			if (state.accepts()) {
				sinceAccepts = 0;
				charsSinceAccepts = 0;
				latestAccepting = state;
			} else if (sinceAccepts >= 0) {
				sinceAccepts += iterable[charCursor].length;
				charsSinceAccepts++;
			}
			tokenLength += iterable[charCursor].length;
			cursor += iterable[charCursor].length;
			charCursor++;
		}
		cursor -= sinceAccepts;
		charCursor -= charsSinceAccepts;
		tokenLength -= sinceAccepts;
		tokens.push({
			t: latestAccepting.t,
			v: str.slice(cursor - tokenLength, cursor),
			s: cursor - tokenLength,
			e: cursor
		});
	}
	return tokens;
}
function stringToArray(str) {
	const result = [];
	const len = str.length;
	let index = 0;
	while (index < len) {
		let first$1 = str.charCodeAt(index);
		let second;
		let char = first$1 < 55296 || first$1 > 56319 || index + 1 === len || (second = str.charCodeAt(index + 1)) < 56320 || second > 57343 ? str[index] : str.slice(index, index + 2);
		result.push(char);
		index += char.length;
	}
	return result;
}
function fastts(state, input, t, defaultt, jr) {
	let next;
	const len = input.length;
	for (let i$1 = 0; i$1 < len - 1; i$1++) {
		const char = input[i$1];
		if (state.j[char]) next = state.j[char];
		else {
			next = new State(defaultt);
			next.jr = jr.slice();
			state.j[char] = next;
		}
		state = next;
	}
	next = new State(t);
	next.jr = jr.slice();
	state.j[input[len - 1]] = next;
	return next;
}
function decodeTlds(encoded) {
	const words = [];
	const stack = [];
	let i$1 = 0;
	let digits = "0123456789";
	while (i$1 < encoded.length) {
		let popDigitCount = 0;
		while (digits.indexOf(encoded[i$1 + popDigitCount]) >= 0) popDigitCount++;
		if (popDigitCount > 0) {
			words.push(stack.join(""));
			for (let popCount = parseInt(encoded.substring(i$1, i$1 + popDigitCount), 10); popCount > 0; popCount--) stack.pop();
			i$1 += popDigitCount;
		} else {
			stack.push(encoded[i$1]);
			i$1++;
		}
	}
	return words;
}
var defaults = {
	defaultProtocol: "http",
	events: null,
	format: noop,
	formatHref: noop,
	nl2br: false,
	tagName: "a",
	target: null,
	rel: null,
	validate: true,
	truncate: Infinity,
	className: null,
	attributes: null,
	ignoreTags: [],
	render: null
};
function Options(opts, defaultRender = null) {
	let o = Object.assign({}, defaults);
	if (opts) o = Object.assign(o, opts instanceof Options ? opts.o : opts);
	const ignoredTags = o.ignoreTags;
	const uppercaseIgnoredTags = [];
	for (let i$1 = 0; i$1 < ignoredTags.length; i$1++) uppercaseIgnoredTags.push(ignoredTags[i$1].toUpperCase());
	this.o = o;
	if (defaultRender) this.defaultRender = defaultRender;
	this.ignoreTags = uppercaseIgnoredTags;
}
Options.prototype = {
	o: defaults,
	ignoreTags: [],
	defaultRender(ir) {
		return ir;
	},
	check(token) {
		return this.get("validate", token.toString(), token);
	},
	get(key, operator, token) {
		const isCallable = operator != null;
		let option = this.o[key];
		if (!option) return option;
		if (typeof option === "object") {
			option = token.t in option ? option[token.t] : defaults[key];
			if (typeof option === "function" && isCallable) option = option(operator, token);
		} else if (typeof option === "function" && isCallable) option = option(operator, token.t, token);
		return option;
	},
	getObj(key, operator, token) {
		let obj = this.o[key];
		if (typeof obj === "function" && operator != null) obj = obj(operator, token.t, token);
		return obj;
	},
	render(token) {
		const ir = token.render(this);
		return (this.get("render", null, token) || this.defaultRender)(ir, token.t, token);
	}
};
function noop(val) {
	return val;
}
function MultiToken(value, tokens) {
	this.t = "token";
	this.v = value;
	this.tk = tokens;
}
MultiToken.prototype = {
	isLink: false,
	toString() {
		return this.v;
	},
	toHref(scheme$1) {
		return this.toString();
	},
	toFormattedString(options) {
		const val = this.toString();
		const truncate = options.get("truncate", val, this);
		const formatted = options.get("format", val, this);
		return truncate && formatted.length > truncate ? formatted.substring(0, truncate) + "…" : formatted;
	},
	toFormattedHref(options) {
		return options.get("formatHref", this.toHref(options.get("defaultProtocol")), this);
	},
	startIndex() {
		return this.tk[0].s;
	},
	endIndex() {
		return this.tk[this.tk.length - 1].e;
	},
	toObject(protocol = defaults.defaultProtocol) {
		return {
			type: this.t,
			value: this.toString(),
			isLink: this.isLink,
			href: this.toHref(protocol),
			start: this.startIndex(),
			end: this.endIndex()
		};
	},
	toFormattedObject(options) {
		return {
			type: this.t,
			value: this.toFormattedString(options),
			isLink: this.isLink,
			href: this.toFormattedHref(options),
			start: this.startIndex(),
			end: this.endIndex()
		};
	},
	validate(options) {
		return options.get("validate", this.toString(), this);
	},
	render(options) {
		const token = this;
		const href = this.toHref(options.get("defaultProtocol"));
		const formattedHref = options.get("formatHref", href, this);
		const tagName = options.get("tagName", href, token);
		const content = this.toFormattedString(options);
		const attributes = {};
		const className = options.get("className", href, token);
		const target = options.get("target", href, token);
		const rel = options.get("rel", href, token);
		const attrs = options.getObj("attributes", href, token);
		const eventListeners = options.getObj("events", href, token);
		attributes.href = formattedHref;
		if (className) attributes.class = className;
		if (target) attributes.target = target;
		if (rel) attributes.rel = rel;
		if (attrs) Object.assign(attributes, attrs);
		return {
			tagName,
			attributes,
			content,
			eventListeners
		};
	}
};
function createTokenClass(type, props) {
	class Token extends MultiToken {
		constructor(value, tokens) {
			super(value, tokens);
			this.t = type;
		}
	}
	for (const p in props) Token.prototype[p] = props[p];
	Token.t = type;
	return Token;
}
var Email = createTokenClass("email", {
	isLink: true,
	toHref() {
		return "mailto:" + this.toString();
	}
});
var Text$1 = createTokenClass("text");
var Nl = createTokenClass("nl");
var Url = createTokenClass("url", {
	isLink: true,
	toHref(scheme$1 = defaults.defaultProtocol) {
		return this.hasProtocol() ? this.v : `${scheme$1}://${this.v}`;
	},
	hasProtocol() {
		const tokens = this.tk;
		return tokens.length >= 2 && tokens[0].t !== LOCALHOST && tokens[1].t === COLON;
	}
});
var makeState = (arg) => new State(arg);
function init$1({ groups }) {
	const qsAccepting = groups.domain.concat([
		AMPERSAND,
		ASTERISK,
		AT,
		BACKSLASH,
		BACKTICK,
		CARET,
		DOLLAR,
		EQUALS,
		HYPHEN,
		NUM,
		PERCENT,
		PIPE,
		PLUS,
		POUND,
		SLASH,
		SYM,
		TILDE,
		UNDERSCORE
	]);
	const qsNonAccepting = [
		APOSTROPHE,
		COLON,
		COMMA,
		DOT,
		EXCLAMATION,
		PERCENT,
		QUERY,
		QUOTE,
		SEMI,
		OPENANGLEBRACKET,
		CLOSEANGLEBRACKET,
		OPENBRACE,
		CLOSEBRACE,
		CLOSEBRACKET,
		OPENBRACKET,
		OPENPAREN,
		CLOSEPAREN,
		FULLWIDTHLEFTPAREN,
		FULLWIDTHRIGHTPAREN,
		LEFTCORNERBRACKET,
		RIGHTCORNERBRACKET,
		LEFTWHITECORNERBRACKET,
		RIGHTWHITECORNERBRACKET,
		FULLWIDTHLESSTHAN,
		FULLWIDTHGREATERTHAN
	];
	const localpartAccepting = [
		AMPERSAND,
		APOSTROPHE,
		ASTERISK,
		BACKSLASH,
		BACKTICK,
		CARET,
		DOLLAR,
		EQUALS,
		HYPHEN,
		OPENBRACE,
		CLOSEBRACE,
		PERCENT,
		PIPE,
		PLUS,
		POUND,
		QUERY,
		SLASH,
		SYM,
		TILDE,
		UNDERSCORE
	];
	const Start = makeState();
	const Localpart = tt$1(Start, TILDE);
	ta(Localpart, localpartAccepting, Localpart);
	ta(Localpart, groups.domain, Localpart);
	const Domain = makeState(), Scheme = makeState(), SlashScheme = makeState();
	ta(Start, groups.domain, Domain);
	ta(Start, groups.scheme, Scheme);
	ta(Start, groups.slashscheme, SlashScheme);
	ta(Domain, localpartAccepting, Localpart);
	ta(Domain, groups.domain, Domain);
	const LocalpartAt = tt$1(Domain, AT);
	tt$1(Localpart, AT, LocalpartAt);
	tt$1(Scheme, AT, LocalpartAt);
	tt$1(SlashScheme, AT, LocalpartAt);
	const LocalpartDot = tt$1(Localpart, DOT);
	ta(LocalpartDot, localpartAccepting, Localpart);
	ta(LocalpartDot, groups.domain, Localpart);
	const EmailDomain = makeState();
	ta(LocalpartAt, groups.domain, EmailDomain);
	ta(EmailDomain, groups.domain, EmailDomain);
	const EmailDomainDot = tt$1(EmailDomain, DOT);
	ta(EmailDomainDot, groups.domain, EmailDomain);
	const Email$1 = makeState(Email);
	ta(EmailDomainDot, groups.tld, Email$1);
	ta(EmailDomainDot, groups.utld, Email$1);
	tt$1(LocalpartAt, LOCALHOST, Email$1);
	const EmailDomainHyphen = tt$1(EmailDomain, HYPHEN);
	tt$1(EmailDomainHyphen, HYPHEN, EmailDomainHyphen);
	ta(EmailDomainHyphen, groups.domain, EmailDomain);
	ta(Email$1, groups.domain, EmailDomain);
	tt$1(Email$1, DOT, EmailDomainDot);
	tt$1(Email$1, HYPHEN, EmailDomainHyphen);
	ta(tt$1(Email$1, COLON), groups.numeric, Email);
	const DomainHyphen = tt$1(Domain, HYPHEN);
	const DomainDot = tt$1(Domain, DOT);
	tt$1(DomainHyphen, HYPHEN, DomainHyphen);
	ta(DomainHyphen, groups.domain, Domain);
	ta(DomainDot, localpartAccepting, Localpart);
	ta(DomainDot, groups.domain, Domain);
	const DomainDotTld = makeState(Url);
	ta(DomainDot, groups.tld, DomainDotTld);
	ta(DomainDot, groups.utld, DomainDotTld);
	ta(DomainDotTld, groups.domain, Domain);
	ta(DomainDotTld, localpartAccepting, Localpart);
	tt$1(DomainDotTld, DOT, DomainDot);
	tt$1(DomainDotTld, HYPHEN, DomainHyphen);
	tt$1(DomainDotTld, AT, LocalpartAt);
	const DomainDotTldColon = tt$1(DomainDotTld, COLON);
	const DomainDotTldColonPort = makeState(Url);
	ta(DomainDotTldColon, groups.numeric, DomainDotTldColonPort);
	const Url$1 = makeState(Url);
	const UrlNonaccept = makeState();
	ta(Url$1, qsAccepting, Url$1);
	ta(Url$1, qsNonAccepting, UrlNonaccept);
	ta(UrlNonaccept, qsAccepting, Url$1);
	ta(UrlNonaccept, qsNonAccepting, UrlNonaccept);
	tt$1(DomainDotTld, SLASH, Url$1);
	tt$1(DomainDotTldColonPort, SLASH, Url$1);
	const SchemeColon = tt$1(Scheme, COLON);
	const UriPrefix = tt$1(tt$1(tt$1(SlashScheme, COLON), SLASH), SLASH);
	ta(Scheme, groups.domain, Domain);
	tt$1(Scheme, DOT, DomainDot);
	tt$1(Scheme, HYPHEN, DomainHyphen);
	ta(SlashScheme, groups.domain, Domain);
	tt$1(SlashScheme, DOT, DomainDot);
	tt$1(SlashScheme, HYPHEN, DomainHyphen);
	ta(SchemeColon, groups.domain, Url$1);
	tt$1(SchemeColon, SLASH, Url$1);
	tt$1(SchemeColon, QUERY, Url$1);
	ta(UriPrefix, groups.domain, Url$1);
	ta(UriPrefix, qsAccepting, Url$1);
	tt$1(UriPrefix, SLASH, Url$1);
	const bracketPairs = [
		[OPENBRACE, CLOSEBRACE],
		[OPENBRACKET, CLOSEBRACKET],
		[OPENPAREN, CLOSEPAREN],
		[OPENANGLEBRACKET, CLOSEANGLEBRACKET],
		[FULLWIDTHLEFTPAREN, FULLWIDTHRIGHTPAREN],
		[LEFTCORNERBRACKET, RIGHTCORNERBRACKET],
		[LEFTWHITECORNERBRACKET, RIGHTWHITECORNERBRACKET],
		[FULLWIDTHLESSTHAN, FULLWIDTHGREATERTHAN]
	];
	for (let i$1 = 0; i$1 < bracketPairs.length; i$1++) {
		const [OPEN, CLOSE] = bracketPairs[i$1];
		const UrlOpen = tt$1(Url$1, OPEN);
		tt$1(UrlNonaccept, OPEN, UrlOpen);
		tt$1(UrlOpen, CLOSE, Url$1);
		const UrlOpenQ = makeState(Url);
		ta(UrlOpen, qsAccepting, UrlOpenQ);
		const UrlOpenSyms = makeState();
		ta(UrlOpen, qsNonAccepting);
		ta(UrlOpenQ, qsAccepting, UrlOpenQ);
		ta(UrlOpenQ, qsNonAccepting, UrlOpenSyms);
		ta(UrlOpenSyms, qsAccepting, UrlOpenQ);
		ta(UrlOpenSyms, qsNonAccepting, UrlOpenSyms);
		tt$1(UrlOpenQ, CLOSE, Url$1);
		tt$1(UrlOpenSyms, CLOSE, Url$1);
	}
	tt$1(Start, LOCALHOST, DomainDotTld);
	tt$1(Start, NL, Nl);
	return {
		start: Start,
		tokens: tk
	};
}
function run(start, input, tokens) {
	let len = tokens.length;
	let cursor = 0;
	let multis = [];
	let textTokens = [];
	while (cursor < len) {
		let state = start;
		let secondState = null;
		let nextState = null;
		let multiLength = 0;
		let latestAccepting = null;
		let sinceAccepts = -1;
		while (cursor < len && !(secondState = state.go(tokens[cursor].t))) textTokens.push(tokens[cursor++]);
		while (cursor < len && (nextState = secondState || state.go(tokens[cursor].t))) {
			secondState = null;
			state = nextState;
			if (state.accepts()) {
				sinceAccepts = 0;
				latestAccepting = state;
			} else if (sinceAccepts >= 0) sinceAccepts++;
			cursor++;
			multiLength++;
		}
		if (sinceAccepts < 0) {
			cursor -= multiLength;
			if (cursor < len) {
				textTokens.push(tokens[cursor]);
				cursor++;
			}
		} else {
			if (textTokens.length > 0) {
				multis.push(initMultiToken(Text$1, input, textTokens));
				textTokens = [];
			}
			cursor -= sinceAccepts;
			multiLength -= sinceAccepts;
			const Multi = latestAccepting.t;
			const subtokens = tokens.slice(cursor - multiLength, cursor);
			multis.push(initMultiToken(Multi, input, subtokens));
		}
	}
	if (textTokens.length > 0) multis.push(initMultiToken(Text$1, input, textTokens));
	return multis;
}
function initMultiToken(Multi, input, tokens) {
	const startIdx = tokens[0].s;
	const endIdx = tokens[tokens.length - 1].e;
	return new Multi(input.slice(startIdx, endIdx), tokens);
}
var warn = typeof console !== "undefined" && console && console.warn || (() => {});
var warnAdvice = "until manual call of linkify.init(). Register all schemes and plugins before invoking linkify the first time.";
var INIT = {
	scanner: null,
	parser: null,
	tokenQueue: [],
	pluginQueue: [],
	customSchemes: [],
	initialized: false
};
function reset() {
	State.groups = {};
	INIT.scanner = null;
	INIT.parser = null;
	INIT.tokenQueue = [];
	INIT.pluginQueue = [];
	INIT.customSchemes = [];
	INIT.initialized = false;
	return INIT;
}
function registerCustomProtocol(scheme$1, optionalSlashSlash = false) {
	if (INIT.initialized) warn(`linkifyjs: already initialized - will not register custom scheme "${scheme$1}" ${warnAdvice}`);
	if (!/^[0-9a-z]+(-[0-9a-z]+)*$/.test(scheme$1)) throw new Error(`linkifyjs: incorrect scheme format.
1. Must only contain digits, lowercase ASCII letters or "-"
2. Cannot start or end with "-"
3. "-" cannot repeat`);
	INIT.customSchemes.push([scheme$1, optionalSlashSlash]);
}
function init() {
	INIT.scanner = init$2(INIT.customSchemes);
	for (let i$1 = 0; i$1 < INIT.tokenQueue.length; i$1++) INIT.tokenQueue[i$1][1]({ scanner: INIT.scanner });
	INIT.parser = init$1(INIT.scanner.tokens);
	for (let i$1 = 0; i$1 < INIT.pluginQueue.length; i$1++) INIT.pluginQueue[i$1][1]({
		scanner: INIT.scanner,
		parser: INIT.parser
	});
	INIT.initialized = true;
	return INIT;
}
function tokenize(str) {
	if (!INIT.initialized) init();
	return run(INIT.parser.start, str, run$1(INIT.scanner.start, str));
}
tokenize.scan = run$1;
function find(str, type = null, opts = null) {
	if (type && typeof type === "object") {
		if (opts) throw Error(`linkifyjs: Invalid link type ${type}; must be a string`);
		opts = type;
		type = null;
	}
	const options = new Options(opts);
	const tokens = tokenize(str);
	const filtered = [];
	for (let i$1 = 0; i$1 < tokens.length; i$1++) {
		const token = tokens[i$1];
		if (token.isLink && (!type || token.t === type) && options.check(token)) filtered.push(token.toFormattedObject(options));
	}
	return filtered;
}
var UNICODE_WHITESPACE_PATTERN = "[\0- \xA0 ᠎ -\u2029 　]";
var UNICODE_WHITESPACE_REGEX = new RegExp(UNICODE_WHITESPACE_PATTERN);
var UNICODE_WHITESPACE_REGEX_END = /* @__PURE__ */ new RegExp(`${UNICODE_WHITESPACE_PATTERN}$`);
var UNICODE_WHITESPACE_REGEX_GLOBAL = new RegExp(UNICODE_WHITESPACE_PATTERN, "g");
function isValidLinkStructure(tokens) {
	if (tokens.length === 1) return tokens[0].isLink;
	if (tokens.length === 3 && tokens[1].isLink) return ["()", "[]"].includes(tokens[0].value + tokens[2].value);
	return false;
}
function autolink(options) {
	return new Plugin({
		key: new PluginKey("autolink"),
		appendTransaction: (transactions, oldState, newState) => {
			const docChanges = transactions.some((transaction) => transaction.docChanged) && !oldState.doc.eq(newState.doc);
			const preventAutolink = transactions.some((transaction) => transaction.getMeta("preventAutolink"));
			if (!docChanges || preventAutolink) return;
			const { tr: tr$1 } = newState;
			getChangedRanges(combineTransactionSteps(oldState.doc, [...transactions])).forEach(({ newRange }) => {
				const nodesInChangedRanges = findChildrenInRange(newState.doc, newRange, (node) => node.isTextblock);
				let textBlock;
				let textBeforeWhitespace;
				if (nodesInChangedRanges.length > 1) {
					textBlock = nodesInChangedRanges[0];
					textBeforeWhitespace = newState.doc.textBetween(textBlock.pos, textBlock.pos + textBlock.node.nodeSize, void 0, " ");
				} else if (nodesInChangedRanges.length) {
					const endText = newState.doc.textBetween(newRange.from, newRange.to, " ", " ");
					if (!UNICODE_WHITESPACE_REGEX_END.test(endText)) return;
					textBlock = nodesInChangedRanges[0];
					textBeforeWhitespace = newState.doc.textBetween(textBlock.pos, newRange.to, void 0, " ");
				}
				if (textBlock && textBeforeWhitespace) {
					const wordsBeforeWhitespace = textBeforeWhitespace.split(UNICODE_WHITESPACE_REGEX).filter(Boolean);
					if (wordsBeforeWhitespace.length <= 0) return false;
					const lastWordBeforeSpace = wordsBeforeWhitespace[wordsBeforeWhitespace.length - 1];
					const lastWordAndBlockOffset = textBlock.pos + textBeforeWhitespace.lastIndexOf(lastWordBeforeSpace);
					if (!lastWordBeforeSpace) return false;
					const linksBeforeSpace = tokenize(lastWordBeforeSpace).map((t) => t.toObject(options.defaultProtocol));
					if (!isValidLinkStructure(linksBeforeSpace)) return false;
					linksBeforeSpace.filter((link) => link.isLink).map((link) => ({
						...link,
						from: lastWordAndBlockOffset + link.start + 1,
						to: lastWordAndBlockOffset + link.end + 1
					})).filter((link) => {
						if (!newState.schema.marks.code) return true;
						return !newState.doc.rangeHasMark(link.from, link.to, newState.schema.marks.code);
					}).filter((link) => options.validate(link.value)).filter((link) => options.shouldAutoLink(link.value)).forEach((link) => {
						if (getMarksBetween(link.from, link.to, newState.doc).some((item) => item.mark.type === options.type)) return;
						tr$1.addMark(link.from, link.to, options.type.create({ href: link.href }));
					});
				}
			});
			if (!tr$1.steps.length) return;
			return tr$1;
		}
	});
}
function clickHandler(options) {
	return new Plugin({
		key: new PluginKey("handleClickLink"),
		props: { handleClick: (view, pos, event) => {
			var _a, _b;
			if (event.button !== 0) return false;
			if (!view.editable) return false;
			let link = null;
			if (event.target instanceof HTMLAnchorElement) link = event.target;
			else {
				const target = event.target;
				if (!target) return false;
				const root = options.editor.view.dom;
				link = target.closest("a");
				if (link && !root.contains(link)) link = null;
			}
			if (!link) return false;
			let handled = false;
			if (options.enableClickSelection) handled = options.editor.commands.extendMarkRange(options.type.name);
			if (options.openOnClick) {
				const attrs = getAttributes(view.state, options.type.name);
				const href = (_a = link.href) != null ? _a : attrs.href;
				const target = (_b = link.target) != null ? _b : attrs.target;
				if (href) {
					window.open(href, target);
					handled = true;
				}
			}
			return handled;
		} }
	});
}
function pasteHandler(options) {
	return new Plugin({
		key: new PluginKey("handlePasteLink"),
		props: { handlePaste: (view, _event, slice) => {
			const { shouldAutoLink } = options;
			const { state } = view;
			const { selection } = state;
			const { empty: empty$1 } = selection;
			if (empty$1) return false;
			let textContent = "";
			slice.content.forEach((node) => {
				textContent += node.textContent;
			});
			const link = find(textContent, { defaultProtocol: options.defaultProtocol }).find((item) => item.isLink && item.value === textContent);
			if (!textContent || !link || shouldAutoLink !== void 0 && !shouldAutoLink(link.value)) return false;
			return options.editor.commands.setMark(options.type, { href: link.href });
		} }
	});
}
function isAllowedUri(uri, protocols) {
	const allowedProtocols = [
		"http",
		"https",
		"ftp",
		"ftps",
		"mailto",
		"tel",
		"callto",
		"sms",
		"cid",
		"xmpp"
	];
	if (protocols) protocols.forEach((protocol) => {
		const nextProtocol = typeof protocol === "string" ? protocol : protocol.scheme;
		if (nextProtocol) allowedProtocols.push(nextProtocol);
	});
	return !uri || uri.replace(UNICODE_WHITESPACE_REGEX_GLOBAL, "").match(new RegExp(`^(?:(?:${allowedProtocols.join("|")}):|[^a-z]|[a-z0-9+.-]+(?:[^a-z+.-:]|$))`, "i"));
}
var Link = Mark.create({
	name: "link",
	priority: 1e3,
	keepOnSplit: false,
	exitable: true,
	onCreate() {
		if (this.options.validate && !this.options.shouldAutoLink) {
			this.options.shouldAutoLink = this.options.validate;
			console.warn("The `validate` option is deprecated. Rename to the `shouldAutoLink` option instead.");
		}
		this.options.protocols.forEach((protocol) => {
			if (typeof protocol === "string") {
				registerCustomProtocol(protocol);
				return;
			}
			registerCustomProtocol(protocol.scheme, protocol.optionalSlashes);
		});
	},
	onDestroy() {
		reset();
	},
	inclusive() {
		return this.options.autolink;
	},
	addOptions() {
		return {
			openOnClick: true,
			enableClickSelection: false,
			linkOnPaste: true,
			autolink: true,
			protocols: [],
			defaultProtocol: "http",
			HTMLAttributes: {
				target: "_blank",
				rel: "noopener noreferrer nofollow",
				class: null
			},
			isAllowedUri: (url, ctx) => !!isAllowedUri(url, ctx.protocols),
			validate: (url) => !!url,
			shouldAutoLink: (url) => {
				const hasProtocol = /^[a-z][a-z0-9+.-]*:\/\//i.test(url);
				const hasMaybeProtocol = /^[a-z][a-z0-9+.-]*:/i.test(url);
				if (hasProtocol || hasMaybeProtocol && !url.includes("@")) return true;
				const hostname = (url.includes("@") ? url.split("@").pop() : url).split(/[/?#:]/)[0];
				if (/^\d{1,3}(\.\d{1,3}){3}$/.test(hostname)) return false;
				if (!/\./.test(hostname)) return false;
				return true;
			}
		};
	},
	addAttributes() {
		return {
			href: {
				default: null,
				parseHTML(element) {
					return element.getAttribute("href");
				}
			},
			target: { default: this.options.HTMLAttributes.target },
			rel: { default: this.options.HTMLAttributes.rel },
			class: { default: this.options.HTMLAttributes.class },
			title: { default: null }
		};
	},
	parseHTML() {
		return [{
			tag: "a[href]",
			getAttrs: (dom) => {
				const href = dom.getAttribute("href");
				if (!href || !this.options.isAllowedUri(href, {
					defaultValidate: (url) => !!isAllowedUri(url, this.options.protocols),
					protocols: this.options.protocols,
					defaultProtocol: this.options.defaultProtocol
				})) return false;
				return null;
			}
		}];
	},
	renderHTML({ HTMLAttributes }) {
		if (!this.options.isAllowedUri(HTMLAttributes.href, {
			defaultValidate: (href) => !!isAllowedUri(href, this.options.protocols),
			protocols: this.options.protocols,
			defaultProtocol: this.options.defaultProtocol
		})) return [
			"a",
			mergeAttributes(this.options.HTMLAttributes, {
				...HTMLAttributes,
				href: ""
			}),
			0
		];
		return [
			"a",
			mergeAttributes(this.options.HTMLAttributes, HTMLAttributes),
			0
		];
	},
	markdownTokenName: "link",
	parseMarkdown: (token, helpers) => {
		return helpers.applyMark("link", helpers.parseInline(token.tokens || []), {
			href: token.href,
			title: token.title || null
		});
	},
	renderMarkdown: (node, h$1) => {
		var _a, _b, _c, _d;
		const href = (_b = (_a = node.attrs) == null ? void 0 : _a.href) != null ? _b : "";
		const title = (_d = (_c = node.attrs) == null ? void 0 : _c.title) != null ? _d : "";
		const text = h$1.renderChildren(node);
		return title ? `[${text}](${href} "${title}")` : `[${text}](${href})`;
	},
	addCommands() {
		return {
			setLink: (attributes) => ({ chain }) => {
				const { href } = attributes;
				if (!this.options.isAllowedUri(href, {
					defaultValidate: (url) => !!isAllowedUri(url, this.options.protocols),
					protocols: this.options.protocols,
					defaultProtocol: this.options.defaultProtocol
				})) return false;
				return chain().setMark(this.name, attributes).setMeta("preventAutolink", true).run();
			},
			toggleLink: (attributes) => ({ chain }) => {
				const { href } = attributes || {};
				if (href && !this.options.isAllowedUri(href, {
					defaultValidate: (url) => !!isAllowedUri(url, this.options.protocols),
					protocols: this.options.protocols,
					defaultProtocol: this.options.defaultProtocol
				})) return false;
				return chain().toggleMark(this.name, attributes, { extendEmptyMarkRange: true }).setMeta("preventAutolink", true).run();
			},
			unsetLink: () => ({ chain }) => {
				return chain().unsetMark(this.name, { extendEmptyMarkRange: true }).setMeta("preventAutolink", true).run();
			}
		};
	},
	addPasteRules() {
		return [markPasteRule({
			find: (text) => {
				const foundLinks = [];
				if (text) {
					const { protocols, defaultProtocol } = this.options;
					const links = find(text).filter((item) => item.isLink && this.options.isAllowedUri(item.value, {
						defaultValidate: (href) => !!isAllowedUri(href, protocols),
						protocols,
						defaultProtocol
					}));
					if (links.length) links.forEach((link) => {
						if (!this.options.shouldAutoLink(link.value)) return;
						foundLinks.push({
							text: link.value,
							data: { href: link.href },
							index: link.start
						});
					});
				}
				return foundLinks;
			},
			type: this.type,
			getAttributes: (match) => {
				var _a;
				return { href: (_a = match.data) == null ? void 0 : _a.href };
			}
		})];
	},
	addProseMirrorPlugins() {
		const plugins = [];
		const { protocols, defaultProtocol } = this.options;
		if (this.options.autolink) plugins.push(autolink({
			type: this.type,
			defaultProtocol: this.options.defaultProtocol,
			validate: (url) => this.options.isAllowedUri(url, {
				defaultValidate: (href) => !!isAllowedUri(href, protocols),
				protocols,
				defaultProtocol
			}),
			shouldAutoLink: this.options.shouldAutoLink
		}));
		plugins.push(clickHandler({
			type: this.type,
			editor: this.editor,
			openOnClick: this.options.openOnClick === "whenNotEditable" ? true : this.options.openOnClick,
			enableClickSelection: this.options.enableClickSelection
		}));
		if (this.options.linkOnPaste) plugins.push(pasteHandler({
			editor: this.editor,
			defaultProtocol: this.options.defaultProtocol,
			type: this.type,
			shouldAutoLink: this.options.shouldAutoLink
		}));
		return plugins;
	}
});
var index_default$1 = Link;
var __defProp = Object.defineProperty;
var __export = (target, all) => {
	for (var name in all) __defProp(target, name, {
		get: all[name],
		enumerable: true
	});
};
var ListItemName = "listItem";
var TextStyleName = "textStyle";
var bulletListInputRegex = /^\s*([-+*])\s$/;
var BulletList = Node3.create({
	name: "bulletList",
	addOptions() {
		return {
			itemTypeName: "listItem",
			HTMLAttributes: {},
			keepMarks: false,
			keepAttributes: false
		};
	},
	group: "block list",
	content() {
		return `${this.options.itemTypeName}+`;
	},
	parseHTML() {
		return [{ tag: "ul" }];
	},
	renderHTML({ HTMLAttributes }) {
		return [
			"ul",
			mergeAttributes(this.options.HTMLAttributes, HTMLAttributes),
			0
		];
	},
	markdownTokenName: "list",
	parseMarkdown: (token, helpers) => {
		if (token.type !== "list" || token.ordered) return [];
		return {
			type: "bulletList",
			content: token.items ? helpers.parseChildren(token.items) : []
		};
	},
	renderMarkdown: (node, h$1) => {
		if (!node.content) return "";
		return h$1.renderChildren(node.content, "\n");
	},
	markdownOptions: { indentsContent: true },
	addCommands() {
		return { toggleBulletList: () => ({ commands, chain }) => {
			if (this.options.keepAttributes) return chain().toggleList(this.name, this.options.itemTypeName, this.options.keepMarks).updateAttributes(ListItemName, this.editor.getAttributes(TextStyleName)).run();
			return commands.toggleList(this.name, this.options.itemTypeName, this.options.keepMarks);
		} };
	},
	addKeyboardShortcuts() {
		return { "Mod-Shift-8": () => this.editor.commands.toggleBulletList() };
	},
	addInputRules() {
		let inputRule = wrappingInputRule({
			find: bulletListInputRegex,
			type: this.type
		});
		if (this.options.keepMarks || this.options.keepAttributes) inputRule = wrappingInputRule({
			find: bulletListInputRegex,
			type: this.type,
			keepMarks: this.options.keepMarks,
			keepAttributes: this.options.keepAttributes,
			getAttributes: () => {
				return this.editor.getAttributes(TextStyleName);
			},
			editor: this.editor
		});
		return [inputRule];
	}
});
var ListItem = Node3.create({
	name: "listItem",
	addOptions() {
		return {
			HTMLAttributes: {},
			bulletListTypeName: "bulletList",
			orderedListTypeName: "orderedList"
		};
	},
	content: "paragraph block*",
	defining: true,
	parseHTML() {
		return [{ tag: "li" }];
	},
	renderHTML({ HTMLAttributes }) {
		return [
			"li",
			mergeAttributes(this.options.HTMLAttributes, HTMLAttributes),
			0
		];
	},
	markdownTokenName: "list_item",
	parseMarkdown: (token, helpers) => {
		var _a;
		if (token.type !== "list_item") return [];
		const parseBlockChildren = (_a = helpers.parseBlockChildren) != null ? _a : helpers.parseChildren;
		let content = [];
		if (token.tokens && token.tokens.length > 0) if (token.tokens.some((t) => t.type === "paragraph")) content = parseBlockChildren(token.tokens);
		else {
			const firstToken = token.tokens[0];
			if (firstToken && firstToken.type === "text" && firstToken.tokens && firstToken.tokens.length > 0) {
				content = [{
					type: "paragraph",
					content: helpers.parseInline(firstToken.tokens)
				}];
				if (token.tokens.length > 1) {
					const additionalContent = parseBlockChildren(token.tokens.slice(1));
					content.push(...additionalContent);
				}
			} else content = parseBlockChildren(token.tokens);
		}
		if (content.length === 0) content = [{
			type: "paragraph",
			content: []
		}];
		return {
			type: "listItem",
			content
		};
	},
	renderMarkdown: (node, h$1, ctx) => {
		return renderNestedMarkdownContent(node, h$1, (context) => {
			var _a, _b;
			if (context.parentType === "bulletList") return "- ";
			if (context.parentType === "orderedList") return `${(((_b = (_a = context.meta) == null ? void 0 : _a.parentAttrs) == null ? void 0 : _b.start) || 1) + context.index}. `;
			return "- ";
		}, ctx);
	},
	addKeyboardShortcuts() {
		return {
			Enter: () => this.editor.commands.splitListItem(this.name),
			Tab: () => this.editor.commands.sinkListItem(this.name),
			"Shift-Tab": () => this.editor.commands.liftListItem(this.name)
		};
	}
});
__export({}, {
	findListItemPos: () => findListItemPos,
	getNextListDepth: () => getNextListDepth,
	handleBackspace: () => handleBackspace,
	handleDelete: () => handleDelete,
	hasListBefore: () => hasListBefore,
	hasListItemAfter: () => hasListItemAfter,
	hasListItemBefore: () => hasListItemBefore,
	listItemHasSubList: () => listItemHasSubList,
	nextListIsDeeper: () => nextListIsDeeper,
	nextListIsHigher: () => nextListIsHigher
});
var findListItemPos = (typeOrName, state) => {
	const { $from } = state.selection;
	const nodeType = getNodeType(typeOrName, state.schema);
	let currentNode = null;
	let currentDepth = $from.depth;
	let currentPos = $from.pos;
	let targetDepth = null;
	while (currentDepth > 0 && targetDepth === null) {
		currentNode = $from.node(currentDepth);
		if (currentNode.type === nodeType) targetDepth = currentDepth;
		else {
			currentDepth -= 1;
			currentPos -= 1;
		}
	}
	if (targetDepth === null) return null;
	return {
		$pos: state.doc.resolve(currentPos),
		depth: targetDepth
	};
};
var getNextListDepth = (typeOrName, state) => {
	const listItemPos = findListItemPos(typeOrName, state);
	if (!listItemPos) return false;
	const [, depth] = getNodeAtPosition(state, typeOrName, listItemPos.$pos.pos + 4);
	return depth;
};
var hasListBefore = (editorState, name, parentListTypes) => {
	const { $anchor } = editorState.selection;
	const previousNodePos = Math.max(0, $anchor.pos - 2);
	const previousNode = editorState.doc.resolve(previousNodePos).node();
	if (!previousNode || !parentListTypes.includes(previousNode.type.name)) return false;
	return true;
};
var hasListItemBefore = (typeOrName, state) => {
	var _a;
	const { $anchor } = state.selection;
	const $targetPos = state.doc.resolve($anchor.pos - 2);
	if ($targetPos.index() === 0) return false;
	if (((_a = $targetPos.nodeBefore) == null ? void 0 : _a.type.name) !== typeOrName) return false;
	return true;
};
var listItemHasSubList = (typeOrName, state, node) => {
	if (!node) return false;
	const nodeType = getNodeType(typeOrName, state.schema);
	let hasSubList = false;
	node.descendants((child) => {
		if (child.type === nodeType) hasSubList = true;
	});
	return hasSubList;
};
var handleBackspace = (editor, name, parentListTypes) => {
	if (editor.commands.undoInputRule()) return true;
	if (editor.state.selection.from !== editor.state.selection.to) return false;
	if (!isNodeActive(editor.state, name) && hasListBefore(editor.state, name, parentListTypes)) {
		const { $anchor } = editor.state.selection;
		const $listPos = editor.state.doc.resolve($anchor.before() - 1);
		const listDescendants = [];
		$listPos.node().descendants((node, pos) => {
			if (node.type.name === name) listDescendants.push({
				node,
				pos
			});
		});
		const lastItem = listDescendants.at(-1);
		if (!lastItem) return false;
		const $lastItemPos = editor.state.doc.resolve($listPos.start() + lastItem.pos + 1);
		return editor.chain().cut({
			from: $anchor.start() - 1,
			to: $anchor.end() + 1
		}, $lastItemPos.end()).joinForward().run();
	}
	if (!isNodeActive(editor.state, name)) return false;
	if (!isAtStartOfNode(editor.state)) return false;
	const listItemPos = findListItemPos(name, editor.state);
	if (!listItemPos) return false;
	const prevNode = editor.state.doc.resolve(listItemPos.$pos.pos - 2).node(listItemPos.depth);
	const previousListItemHasSubList = listItemHasSubList(name, editor.state, prevNode);
	if (hasListItemBefore(name, editor.state) && !previousListItemHasSubList) return editor.commands.joinItemBackward();
	return editor.chain().liftListItem(name).run();
};
var nextListIsDeeper = (typeOrName, state) => {
	const listDepth = getNextListDepth(typeOrName, state);
	const listItemPos = findListItemPos(typeOrName, state);
	if (!listItemPos || !listDepth) return false;
	if (listDepth > listItemPos.depth) return true;
	return false;
};
var nextListIsHigher = (typeOrName, state) => {
	const listDepth = getNextListDepth(typeOrName, state);
	const listItemPos = findListItemPos(typeOrName, state);
	if (!listItemPos || !listDepth) return false;
	if (listDepth < listItemPos.depth) return true;
	return false;
};
var handleDelete = (editor, name) => {
	if (!isNodeActive(editor.state, name)) return false;
	if (!isAtEndOfNode(editor.state, name)) return false;
	const { selection } = editor.state;
	const { $from, $to } = selection;
	if (!selection.empty && $from.sameParent($to)) return false;
	if (nextListIsDeeper(name, editor.state)) return editor.chain().focus(editor.state.selection.from + 4).lift(name).joinBackward().run();
	if (nextListIsHigher(name, editor.state)) return editor.chain().joinForward().joinBackward().run();
	return editor.commands.joinItemForward();
};
var hasListItemAfter = (typeOrName, state) => {
	var _a;
	const { $anchor } = state.selection;
	const $targetPos = state.doc.resolve($anchor.pos - $anchor.parentOffset - 2);
	if ($targetPos.index() === $targetPos.parent.childCount - 1) return false;
	if (((_a = $targetPos.nodeAfter) == null ? void 0 : _a.type.name) !== typeOrName) return false;
	return true;
};
var ListKeymap = Extension.create({
	name: "listKeymap",
	addOptions() {
		return { listTypes: [{
			itemName: "listItem",
			wrapperNames: ["bulletList", "orderedList"]
		}, {
			itemName: "taskItem",
			wrapperNames: ["taskList"]
		}] };
	},
	addKeyboardShortcuts() {
		return {
			Delete: ({ editor }) => {
				let handled = false;
				this.options.listTypes.forEach(({ itemName }) => {
					if (editor.state.schema.nodes[itemName] === void 0) return;
					if (handleDelete(editor, itemName)) handled = true;
				});
				return handled;
			},
			"Mod-Delete": ({ editor }) => {
				let handled = false;
				this.options.listTypes.forEach(({ itemName }) => {
					if (editor.state.schema.nodes[itemName] === void 0) return;
					if (handleDelete(editor, itemName)) handled = true;
				});
				return handled;
			},
			Backspace: ({ editor }) => {
				let handled = false;
				this.options.listTypes.forEach(({ itemName, wrapperNames }) => {
					if (editor.state.schema.nodes[itemName] === void 0) return;
					if (handleBackspace(editor, itemName, wrapperNames)) handled = true;
				});
				return handled;
			},
			"Mod-Backspace": ({ editor }) => {
				let handled = false;
				this.options.listTypes.forEach(({ itemName, wrapperNames }) => {
					if (editor.state.schema.nodes[itemName] === void 0) return;
					if (handleBackspace(editor, itemName, wrapperNames)) handled = true;
				});
				return handled;
			}
		};
	}
});
var ORDERED_LIST_ITEM_REGEX = /^(\s*)(\d+)\.\s+(.*)$/;
var INDENTED_LINE_REGEX = /^\s/;
function isBlockContentLine(line) {
	const trimmedLine = line.trimStart();
	return /^[-+*]\s+/.test(trimmedLine) || /^\d+\.\s+/.test(trimmedLine) || /^>\s?/.test(trimmedLine) || /^```/.test(trimmedLine) || /^~~~/.test(trimmedLine);
}
function splitItemContent(contentLines) {
	const paragraphLines = [];
	const blockLines = [];
	let reachedBlockBoundary = false;
	contentLines.forEach((line) => {
		if (reachedBlockBoundary) {
			blockLines.push(line);
			return;
		}
		if (line.trim() === "") {
			reachedBlockBoundary = true;
			blockLines.push(line);
			return;
		}
		if (paragraphLines.length > 0 && isBlockContentLine(line)) {
			reachedBlockBoundary = true;
			blockLines.push(line);
			return;
		}
		paragraphLines.push(line);
	});
	return {
		paragraphLines,
		blockLines
	};
}
function collectOrderedListItems(lines) {
	const listItems = [];
	let currentLineIndex = 0;
	let consumed = 0;
	while (currentLineIndex < lines.length) {
		const line = lines[currentLineIndex];
		const match = line.match(ORDERED_LIST_ITEM_REGEX);
		if (!match) break;
		const [, indent, number, content] = match;
		const indentLevel = indent.length;
		const itemContentLines = [content];
		let nextLineIndex = currentLineIndex + 1;
		const itemLines = [line];
		let sawBlankLine = false;
		while (nextLineIndex < lines.length) {
			const nextLine = lines[nextLineIndex];
			if (nextLine.match(ORDERED_LIST_ITEM_REGEX)) break;
			if (nextLine.trim() === "") {
				itemLines.push(nextLine);
				itemContentLines.push("");
				sawBlankLine = true;
				nextLineIndex += 1;
			} else if (nextLine.match(INDENTED_LINE_REGEX)) {
				itemLines.push(nextLine);
				itemContentLines.push(nextLine.slice(indentLevel + 2));
				nextLineIndex += 1;
			} else {
				if (sawBlankLine) break;
				itemLines.push(nextLine);
				itemContentLines.push(nextLine);
				nextLineIndex += 1;
			}
		}
		listItems.push({
			indent: indentLevel,
			number: parseInt(number, 10),
			content: itemContentLines.join("\n").trim(),
			contentLines: itemContentLines,
			raw: itemLines.join("\n")
		});
		consumed = nextLineIndex;
		currentLineIndex = nextLineIndex;
	}
	return [listItems, consumed];
}
function buildNestedStructure(items, baseIndent, lexer) {
	const result = [];
	let currentIndex = 0;
	while (currentIndex < items.length) {
		const item = items[currentIndex];
		if (item.indent === baseIndent) {
			const { paragraphLines, blockLines } = splitItemContent(item.contentLines);
			const mainText = paragraphLines.join("\n").trim();
			const tokens = [];
			if (mainText) tokens.push({
				type: "paragraph",
				raw: mainText,
				tokens: lexer.inlineTokens(mainText)
			});
			const additionalContent = blockLines.join("\n").trim();
			if (additionalContent) {
				const blockTokens = lexer.blockTokens(additionalContent);
				tokens.push(...blockTokens);
			}
			let lookAheadIndex = currentIndex + 1;
			const nestedItems = [];
			while (lookAheadIndex < items.length && items[lookAheadIndex].indent > baseIndent) {
				nestedItems.push(items[lookAheadIndex]);
				lookAheadIndex += 1;
			}
			if (nestedItems.length > 0) {
				const nestedListItems = buildNestedStructure(nestedItems, Math.min(...nestedItems.map((nestedItem) => nestedItem.indent)), lexer);
				tokens.push({
					type: "list",
					ordered: true,
					start: nestedItems[0].number,
					items: nestedListItems,
					raw: nestedItems.map((nestedItem) => nestedItem.raw).join("\n")
				});
			}
			result.push({
				type: "list_item",
				raw: item.raw,
				tokens
			});
			currentIndex = lookAheadIndex;
		} else currentIndex += 1;
	}
	return result;
}
function parseListItems(items, helpers) {
	return items.map((item) => {
		if (item.type !== "list_item") return helpers.parseChildren([item])[0];
		const content = [];
		if (item.tokens && item.tokens.length > 0) item.tokens.forEach((itemToken) => {
			if (itemToken.type === "paragraph" || itemToken.type === "list" || itemToken.type === "blockquote" || itemToken.type === "code") content.push(...helpers.parseChildren([itemToken]));
			else if (itemToken.type === "text" && itemToken.tokens) {
				const inlineContent = helpers.parseChildren([itemToken]);
				content.push({
					type: "paragraph",
					content: inlineContent
				});
			} else {
				const parsed = helpers.parseChildren([itemToken]);
				if (parsed.length > 0) content.push(...parsed);
			}
		});
		return {
			type: "listItem",
			content
		};
	});
}
var ListItemName2 = "listItem";
var TextStyleName2 = "textStyle";
var orderedListInputRegex = /^(\d+)\.\s$/;
var OrderedList = Node3.create({
	name: "orderedList",
	addOptions() {
		return {
			itemTypeName: "listItem",
			HTMLAttributes: {},
			keepMarks: false,
			keepAttributes: false
		};
	},
	group: "block list",
	content() {
		return `${this.options.itemTypeName}+`;
	},
	addAttributes() {
		return {
			start: {
				default: 1,
				parseHTML: (element) => {
					return element.hasAttribute("start") ? parseInt(element.getAttribute("start") || "", 10) : 1;
				}
			},
			type: {
				default: null,
				parseHTML: (element) => element.getAttribute("type")
			}
		};
	},
	parseHTML() {
		return [{ tag: "ol" }];
	},
	renderHTML({ HTMLAttributes }) {
		const { start, ...attributesWithoutStart } = HTMLAttributes;
		return start === 1 ? [
			"ol",
			mergeAttributes(this.options.HTMLAttributes, attributesWithoutStart),
			0
		] : [
			"ol",
			mergeAttributes(this.options.HTMLAttributes, HTMLAttributes),
			0
		];
	},
	markdownTokenName: "list",
	parseMarkdown: (token, helpers) => {
		if (token.type !== "list" || !token.ordered) return [];
		const startValue = token.start || 1;
		const content = token.items ? parseListItems(token.items, helpers) : [];
		if (startValue !== 1) return {
			type: "orderedList",
			attrs: { start: startValue },
			content
		};
		return {
			type: "orderedList",
			content
		};
	},
	renderMarkdown: (node, h$1) => {
		if (!node.content) return "";
		return h$1.renderChildren(node.content, "\n");
	},
	markdownTokenizer: {
		name: "orderedList",
		level: "block",
		start: (src) => {
			const match = src.match(/^(\s*)(\d+)\.\s+/);
			const index = match == null ? void 0 : match.index;
			return index !== void 0 ? index : -1;
		},
		tokenize: (src, _tokens, lexer) => {
			var _a;
			const lines = src.split("\n");
			const [listItems, consumed] = collectOrderedListItems(lines);
			if (listItems.length === 0) return;
			const items = buildNestedStructure(listItems, 0, lexer);
			if (items.length === 0) return;
			return {
				type: "list",
				ordered: true,
				start: ((_a = listItems[0]) == null ? void 0 : _a.number) || 1,
				items,
				raw: lines.slice(0, consumed).join("\n")
			};
		}
	},
	markdownOptions: { indentsContent: true },
	addCommands() {
		return { toggleOrderedList: () => ({ commands, chain }) => {
			if (this.options.keepAttributes) return chain().toggleList(this.name, this.options.itemTypeName, this.options.keepMarks).updateAttributes(ListItemName2, this.editor.getAttributes(TextStyleName2)).run();
			return commands.toggleList(this.name, this.options.itemTypeName, this.options.keepMarks);
		} };
	},
	addKeyboardShortcuts() {
		return { "Mod-Shift-7": () => this.editor.commands.toggleOrderedList() };
	},
	addInputRules() {
		let inputRule = wrappingInputRule({
			find: orderedListInputRegex,
			type: this.type,
			getAttributes: (match) => ({ start: +match[1] }),
			joinPredicate: (match, node) => node.childCount + node.attrs.start === +match[1]
		});
		if (this.options.keepMarks || this.options.keepAttributes) inputRule = wrappingInputRule({
			find: orderedListInputRegex,
			type: this.type,
			keepMarks: this.options.keepMarks,
			keepAttributes: this.options.keepAttributes,
			getAttributes: (match) => ({
				start: +match[1],
				...this.editor.getAttributes(TextStyleName2)
			}),
			joinPredicate: (match, node) => node.childCount + node.attrs.start === +match[1],
			editor: this.editor
		});
		return [inputRule];
	}
});
var inputRegex$2 = /^\s*(\[([( |x])?\])\s$/;
var TaskItem = Node3.create({
	name: "taskItem",
	addOptions() {
		return {
			nested: false,
			HTMLAttributes: {},
			taskListTypeName: "taskList",
			a11y: void 0
		};
	},
	content() {
		return this.options.nested ? "paragraph block*" : "paragraph+";
	},
	defining: true,
	addAttributes() {
		return { checked: {
			default: false,
			keepOnSplit: false,
			parseHTML: (element) => {
				const dataChecked = element.getAttribute("data-checked");
				return dataChecked === "" || dataChecked === "true";
			},
			renderHTML: (attributes) => ({ "data-checked": attributes.checked })
		} };
	},
	parseHTML() {
		return [{
			tag: `li[data-type="${this.name}"]`,
			priority: 51
		}];
	},
	renderHTML({ node, HTMLAttributes }) {
		return [
			"li",
			mergeAttributes(this.options.HTMLAttributes, HTMLAttributes, { "data-type": this.name }),
			[
				"label",
				["input", {
					type: "checkbox",
					checked: node.attrs.checked ? "checked" : null
				}],
				["span"]
			],
			["div", 0]
		];
	},
	parseMarkdown: (token, h$1) => {
		const content = [];
		if (token.tokens && token.tokens.length > 0) content.push(h$1.createNode("paragraph", {}, h$1.parseInline(token.tokens)));
		else if (token.text) content.push(h$1.createNode("paragraph", {}, [h$1.createNode("text", { text: token.text })]));
		else content.push(h$1.createNode("paragraph", {}, []));
		if (token.nestedTokens && token.nestedTokens.length > 0) {
			const nestedContent = h$1.parseChildren(token.nestedTokens);
			content.push(...nestedContent);
		}
		return h$1.createNode("taskItem", { checked: token.checked || false }, content);
	},
	renderMarkdown: (node, h$1) => {
		var _a;
		return renderNestedMarkdownContent(node, h$1, `- [${((_a = node.attrs) == null ? void 0 : _a.checked) ? "x" : " "}] `);
	},
	addKeyboardShortcuts() {
		const shortcuts = {
			Enter: () => this.editor.commands.splitListItem(this.name),
			"Shift-Tab": () => this.editor.commands.liftListItem(this.name)
		};
		if (!this.options.nested) return shortcuts;
		return {
			...shortcuts,
			Tab: () => this.editor.commands.sinkListItem(this.name)
		};
	},
	addNodeView() {
		return ({ node, HTMLAttributes, getPos, editor }) => {
			const listItem = document.createElement("li");
			const checkboxWrapper = document.createElement("label");
			const checkboxStyler = document.createElement("span");
			const checkbox = document.createElement("input");
			const content = document.createElement("div");
			const updateA11Y = (currentNode) => {
				var _a, _b;
				checkbox.ariaLabel = ((_b = (_a = this.options.a11y) == null ? void 0 : _a.checkboxLabel) == null ? void 0 : _b.call(_a, currentNode, checkbox.checked)) || `Task item checkbox for ${currentNode.textContent || "empty task item"}`;
			};
			updateA11Y(node);
			checkboxWrapper.contentEditable = "false";
			checkbox.type = "checkbox";
			checkbox.addEventListener("mousedown", (event) => event.preventDefault());
			checkbox.addEventListener("change", (event) => {
				if (!editor.isEditable && !this.options.onReadOnlyChecked) {
					checkbox.checked = !checkbox.checked;
					return;
				}
				const { checked } = event.target;
				if (editor.isEditable && typeof getPos === "function") editor.chain().focus(void 0, { scrollIntoView: false }).command(({ tr: tr$1 }) => {
					const position = getPos();
					if (typeof position !== "number") return false;
					const currentNode = tr$1.doc.nodeAt(position);
					tr$1.setNodeMarkup(position, void 0, {
						...currentNode == null ? void 0 : currentNode.attrs,
						checked
					});
					return true;
				}).run();
				if (!editor.isEditable && this.options.onReadOnlyChecked) {
					if (!this.options.onReadOnlyChecked(node, checked)) checkbox.checked = !checkbox.checked;
				}
			});
			Object.entries(this.options.HTMLAttributes).forEach(([key, value]) => {
				listItem.setAttribute(key, value);
			});
			listItem.dataset.checked = node.attrs.checked;
			checkbox.checked = node.attrs.checked;
			checkboxWrapper.append(checkbox, checkboxStyler);
			listItem.append(checkboxWrapper, content);
			Object.entries(HTMLAttributes).forEach(([key, value]) => {
				listItem.setAttribute(key, value);
			});
			let prevRenderedAttributeKeys = new Set(Object.keys(HTMLAttributes));
			return {
				dom: listItem,
				contentDOM: content,
				update: (updatedNode) => {
					if (updatedNode.type !== this.type) return false;
					listItem.dataset.checked = updatedNode.attrs.checked;
					checkbox.checked = updatedNode.attrs.checked;
					updateA11Y(updatedNode);
					const extensionAttributes = editor.extensionManager.attributes;
					const newHTMLAttributes = getRenderedAttributes(updatedNode, extensionAttributes);
					const newKeys = new Set(Object.keys(newHTMLAttributes));
					const staticAttrs = this.options.HTMLAttributes;
					prevRenderedAttributeKeys.forEach((key) => {
						if (!newKeys.has(key)) if (key in staticAttrs) listItem.setAttribute(key, staticAttrs[key]);
						else listItem.removeAttribute(key);
					});
					Object.entries(newHTMLAttributes).forEach(([key, value]) => {
						if (value === null || value === void 0) if (key in staticAttrs) listItem.setAttribute(key, staticAttrs[key]);
						else listItem.removeAttribute(key);
						else listItem.setAttribute(key, value);
					});
					prevRenderedAttributeKeys = newKeys;
					return true;
				}
			};
		};
	},
	addInputRules() {
		return [wrappingInputRule({
			find: inputRegex$2,
			type: this.type,
			getAttributes: (match) => ({ checked: match[match.length - 1] === "x" })
		})];
	}
});
var TaskList = Node3.create({
	name: "taskList",
	addOptions() {
		return {
			itemTypeName: "taskItem",
			HTMLAttributes: {}
		};
	},
	group: "block list",
	content() {
		return `${this.options.itemTypeName}+`;
	},
	parseHTML() {
		return [{
			tag: `ul[data-type="${this.name}"]`,
			priority: 51
		}];
	},
	renderHTML({ HTMLAttributes }) {
		return [
			"ul",
			mergeAttributes(this.options.HTMLAttributes, HTMLAttributes, { "data-type": this.name }),
			0
		];
	},
	parseMarkdown: (token, h$1) => {
		return h$1.createNode("taskList", {}, h$1.parseChildren(token.items || []));
	},
	renderMarkdown: (node, h$1) => {
		if (!node.content) return "";
		return h$1.renderChildren(node.content, "\n");
	},
	markdownTokenizer: {
		name: "taskList",
		level: "block",
		start(src) {
			var _a;
			const index = (_a = src.match(/^\s*[-+*]\s+\[([ xX])\]\s+/)) == null ? void 0 : _a.index;
			return index !== void 0 ? index : -1;
		},
		tokenize(src, tokens, lexer) {
			const parseTaskListContent = (content) => {
				const nestedResult = parseIndentedBlocks(content, {
					itemPattern: /^(\s*)([-+*])\s+\[([ xX])\]\s+(.*)$/,
					extractItemData: (match) => ({
						indentLevel: match[1].length,
						mainContent: match[4],
						checked: match[3].toLowerCase() === "x"
					}),
					createToken: (data, nestedTokens) => ({
						type: "taskItem",
						raw: "",
						mainContent: data.mainContent,
						indentLevel: data.indentLevel,
						checked: data.checked,
						text: data.mainContent,
						tokens: lexer.inlineTokens(data.mainContent),
						nestedTokens
					}),
					customNestedParser: parseTaskListContent
				}, lexer);
				if (nestedResult) return [{
					type: "taskList",
					raw: nestedResult.raw,
					items: nestedResult.items
				}];
				return lexer.blockTokens(content);
			};
			const result = parseIndentedBlocks(src, {
				itemPattern: /^(\s*)([-+*])\s+\[([ xX])\]\s+(.*)$/,
				extractItemData: (match) => ({
					indentLevel: match[1].length,
					mainContent: match[4],
					checked: match[3].toLowerCase() === "x"
				}),
				createToken: (data, nestedTokens) => ({
					type: "taskItem",
					raw: "",
					mainContent: data.mainContent,
					indentLevel: data.indentLevel,
					checked: data.checked,
					text: data.mainContent,
					tokens: lexer.inlineTokens(data.mainContent),
					nestedTokens
				}),
				customNestedParser: parseTaskListContent
			}, lexer);
			if (!result) return;
			return {
				type: "taskList",
				raw: result.raw,
				items: result.items
			};
		}
	},
	markdownOptions: { indentsContent: true },
	addCommands() {
		return { toggleTaskList: () => ({ commands }) => {
			return commands.toggleList(this.name, this.options.itemTypeName);
		} };
	},
	addKeyboardShortcuts() {
		return { "Mod-Shift-9": () => this.editor.commands.toggleTaskList() };
	}
});
Extension.create({
	name: "listKit",
	addExtensions() {
		const extensions = [];
		if (this.options.bulletList !== false) extensions.push(BulletList.configure(this.options.bulletList));
		if (this.options.listItem !== false) extensions.push(ListItem.configure(this.options.listItem));
		if (this.options.listKeymap !== false) extensions.push(ListKeymap.configure(this.options.listKeymap));
		if (this.options.orderedList !== false) extensions.push(OrderedList.configure(this.options.orderedList));
		if (this.options.taskItem !== false) extensions.push(TaskItem.configure(this.options.taskItem));
		if (this.options.taskList !== false) extensions.push(TaskList.configure(this.options.taskList));
		return extensions;
	}
});
var EMPTY_PARAGRAPH_MARKDOWN = "&nbsp;";
var NBSP_CHAR = "\xA0";
var Paragraph = Node3.create({
	name: "paragraph",
	priority: 1e3,
	addOptions() {
		return { HTMLAttributes: {} };
	},
	group: "block",
	content: "inline*",
	parseHTML() {
		return [{ tag: "p" }];
	},
	renderHTML({ HTMLAttributes }) {
		return [
			"p",
			mergeAttributes(this.options.HTMLAttributes, HTMLAttributes),
			0
		];
	},
	parseMarkdown: (token, helpers) => {
		const tokens = token.tokens || [];
		if (tokens.length === 1 && tokens[0].type === "image") return helpers.parseChildren([tokens[0]]);
		const content = helpers.parseInline(tokens);
		if (tokens.length === 1 && tokens[0].type === "text" && (tokens[0].raw === EMPTY_PARAGRAPH_MARKDOWN || tokens[0].text === EMPTY_PARAGRAPH_MARKDOWN || tokens[0].raw === NBSP_CHAR || tokens[0].text === NBSP_CHAR) && content.length === 1 && content[0].type === "text" && (content[0].text === EMPTY_PARAGRAPH_MARKDOWN || content[0].text === NBSP_CHAR)) return helpers.createNode("paragraph", void 0, []);
		return helpers.createNode("paragraph", void 0, content);
	},
	renderMarkdown: (node, h$1, ctx) => {
		var _a, _b;
		if (!node) return "";
		const content = Array.isArray(node.content) ? node.content : [];
		if (content.length === 0) {
			const previousContent = Array.isArray((_a = ctx == null ? void 0 : ctx.previousNode) == null ? void 0 : _a.content) ? ctx.previousNode.content : [];
			return ((_b = ctx == null ? void 0 : ctx.previousNode) == null ? void 0 : _b.type) === "paragraph" && previousContent.length === 0 ? EMPTY_PARAGRAPH_MARKDOWN : "";
		}
		return h$1.renderChildren(content);
	},
	addCommands() {
		return { setParagraph: () => ({ commands }) => {
			return commands.setNode(this.name);
		} };
	},
	addKeyboardShortcuts() {
		return { "Mod-Alt-0": () => this.editor.commands.setParagraph() };
	}
});
var inputRegex$1 = /(?:^|\s)(~~(?!\s+~~)((?:[^~]+))~~(?!\s+~~))$/;
var pasteRegex = /(?:^|\s)(~~(?!\s+~~)((?:[^~]+))~~(?!\s+~~))/g;
var Strike = Mark.create({
	name: "strike",
	addOptions() {
		return { HTMLAttributes: {} };
	},
	parseHTML() {
		return [
			{ tag: "s" },
			{ tag: "del" },
			{ tag: "strike" },
			{
				style: "text-decoration",
				consuming: false,
				getAttrs: (style$1) => style$1.includes("line-through") ? {} : false
			}
		];
	},
	renderHTML({ HTMLAttributes }) {
		return [
			"s",
			mergeAttributes(this.options.HTMLAttributes, HTMLAttributes),
			0
		];
	},
	markdownTokenName: "del",
	parseMarkdown: (token, helpers) => {
		return helpers.applyMark("strike", helpers.parseInline(token.tokens || []));
	},
	renderMarkdown: (node, h$1) => {
		return `~~${h$1.renderChildren(node)}~~`;
	},
	addCommands() {
		return {
			setStrike: () => ({ commands }) => {
				return commands.setMark(this.name);
			},
			toggleStrike: () => ({ commands }) => {
				return commands.toggleMark(this.name);
			},
			unsetStrike: () => ({ commands }) => {
				return commands.unsetMark(this.name);
			}
		};
	},
	addKeyboardShortcuts() {
		return { "Mod-Shift-s": () => this.editor.commands.toggleStrike() };
	},
	addInputRules() {
		return [markInputRule({
			find: inputRegex$1,
			type: this.type
		})];
	},
	addPasteRules() {
		return [markPasteRule({
			find: pasteRegex,
			type: this.type
		})];
	}
});
var Text = Node3.create({
	name: "text",
	group: "inline",
	parseMarkdown: (token) => {
		return {
			type: "text",
			text: token.text || ""
		};
	},
	renderMarkdown: (node) => node.text || ""
});
var Underline = Mark.create({
	name: "underline",
	addOptions() {
		return { HTMLAttributes: {} };
	},
	parseHTML() {
		return [{ tag: "u" }, {
			style: "text-decoration",
			consuming: false,
			getAttrs: (style$1) => style$1.includes("underline") ? {} : false
		}];
	},
	renderHTML({ HTMLAttributes }) {
		return [
			"u",
			mergeAttributes(this.options.HTMLAttributes, HTMLAttributes),
			0
		];
	},
	parseMarkdown(token, helpers) {
		return helpers.applyMark(this.name || "underline", helpers.parseInline(token.tokens || []));
	},
	renderMarkdown(node, helpers) {
		return `++${helpers.renderChildren(node)}++`;
	},
	markdownTokenizer: {
		name: "underline",
		level: "inline",
		start(src) {
			return src.indexOf("++");
		},
		tokenize(src, _tokens, lexer) {
			const match = /^(\+\+)([\s\S]+?)(\+\+)/.exec(src);
			if (!match) return;
			const innerContent = match[2].trim();
			return {
				type: "underline",
				raw: match[0],
				text: innerContent,
				tokens: lexer.inlineTokens(innerContent)
			};
		}
	},
	addCommands() {
		return {
			setUnderline: () => ({ commands }) => {
				return commands.setMark(this.name);
			},
			toggleUnderline: () => ({ commands }) => {
				return commands.toggleMark(this.name);
			},
			unsetUnderline: () => ({ commands }) => {
				return commands.unsetMark(this.name);
			}
		};
	},
	addKeyboardShortcuts() {
		return {
			"Mod-u": () => this.editor.commands.toggleUnderline(),
			"Mod-U": () => this.editor.commands.toggleUnderline()
		};
	}
});
var index_default = Extension.create({
	name: "starterKit",
	addExtensions() {
		var _a, _b, _c, _d;
		const extensions = [];
		if (this.options.bold !== false) extensions.push(Bold.configure(this.options.bold));
		if (this.options.blockquote !== false) extensions.push(Blockquote.configure(this.options.blockquote));
		if (this.options.bulletList !== false) extensions.push(BulletList.configure(this.options.bulletList));
		if (this.options.code !== false) extensions.push(Code.configure(this.options.code));
		if (this.options.codeBlock !== false) extensions.push(CodeBlock.configure(this.options.codeBlock));
		if (this.options.document !== false) extensions.push(Document.configure(this.options.document));
		if (this.options.dropcursor !== false) extensions.push(Dropcursor.configure(this.options.dropcursor));
		if (this.options.gapcursor !== false) extensions.push(Gapcursor.configure(this.options.gapcursor));
		if (this.options.hardBreak !== false) extensions.push(HardBreak.configure(this.options.hardBreak));
		if (this.options.heading !== false) extensions.push(Heading.configure(this.options.heading));
		if (this.options.undoRedo !== false) extensions.push(UndoRedo.configure(this.options.undoRedo));
		if (this.options.horizontalRule !== false) extensions.push(HorizontalRule.configure(this.options.horizontalRule));
		if (this.options.italic !== false) extensions.push(Italic.configure(this.options.italic));
		if (this.options.listItem !== false) extensions.push(ListItem.configure(this.options.listItem));
		if (this.options.listKeymap !== false) extensions.push(ListKeymap.configure((_a = this.options) == null ? void 0 : _a.listKeymap));
		if (this.options.link !== false) extensions.push(Link.configure((_b = this.options) == null ? void 0 : _b.link));
		if (this.options.orderedList !== false) extensions.push(OrderedList.configure(this.options.orderedList));
		if (this.options.paragraph !== false) extensions.push(Paragraph.configure(this.options.paragraph));
		if (this.options.strike !== false) extensions.push(Strike.configure(this.options.strike));
		if (this.options.text !== false) extensions.push(Text.configure(this.options.text));
		if (this.options.underline !== false) extensions.push(Underline.configure((_c = this.options) == null ? void 0 : _c.underline));
		if (this.options.trailingNode !== false) extensions.push(TrailingNode.configure((_d = this.options) == null ? void 0 : _d.trailingNode));
		return extensions;
	}
});
var inputRegex = /(?:^|\s)(!\[(.+|:?)]\((\S+)(?:(?:\s+)["'](\S+)["'])?\))$/;
var index_default$2 = Node3.create({
	name: "image",
	addOptions() {
		return {
			inline: false,
			allowBase64: false,
			HTMLAttributes: {},
			resize: false
		};
	},
	inline() {
		return this.options.inline;
	},
	group() {
		return this.options.inline ? "inline" : "block";
	},
	draggable: true,
	addAttributes() {
		return {
			src: { default: null },
			alt: { default: null },
			title: { default: null },
			width: { default: null },
			height: { default: null }
		};
	},
	parseHTML() {
		return [{ tag: this.options.allowBase64 ? "img[src]" : "img[src]:not([src^=\"data:\"])" }];
	},
	renderHTML({ HTMLAttributes }) {
		return ["img", mergeAttributes(this.options.HTMLAttributes, HTMLAttributes)];
	},
	parseMarkdown: (token, helpers) => {
		return helpers.createNode("image", {
			src: token.href,
			title: token.title,
			alt: token.text
		});
	},
	renderMarkdown: (node) => {
		var _a, _b, _c, _d, _e$1, _f;
		const src = (_b = (_a = node.attrs) == null ? void 0 : _a.src) != null ? _b : "";
		const alt = (_d = (_c = node.attrs) == null ? void 0 : _c.alt) != null ? _d : "";
		const title = (_f = (_e$1 = node.attrs) == null ? void 0 : _e$1.title) != null ? _f : "";
		return title ? `![${alt}](${src} "${title}")` : `![${alt}](${src})`;
	},
	addNodeView() {
		if (!this.options.resize || !this.options.resize.enabled || typeof document === "undefined") return null;
		const { directions, minWidth, minHeight, alwaysPreserveAspectRatio } = this.options.resize;
		return ({ node, getPos, HTMLAttributes, editor }) => {
			const el = document.createElement("img");
			Object.entries(HTMLAttributes).forEach(([key, value]) => {
				if (value != null) switch (key) {
					case "width":
					case "height": break;
					default:
						el.setAttribute(key, value);
						break;
				}
			});
			el.src = HTMLAttributes.src;
			const nodeView = new ResizableNodeView({
				element: el,
				editor,
				node,
				getPos,
				onResize: (width, height) => {
					el.style.width = `${width}px`;
					el.style.height = `${height}px`;
				},
				onCommit: (width, height) => {
					const pos = getPos();
					if (pos === void 0) return;
					this.editor.chain().setNodeSelection(pos).updateAttributes(this.name, {
						width,
						height
					}).run();
				},
				onUpdate: (updatedNode, _decorations, _innerDecorations) => {
					if (updatedNode.type !== node.type) return false;
					return true;
				},
				options: {
					directions,
					min: {
						width: minWidth,
						height: minHeight
					},
					preserveAspectRatio: alwaysPreserveAspectRatio === true
				}
			});
			const dom = nodeView.dom;
			dom.style.visibility = "hidden";
			dom.style.pointerEvents = "none";
			el.onload = () => {
				dom.style.visibility = "";
				dom.style.pointerEvents = "";
			};
			return nodeView;
		};
	},
	addCommands() {
		return { setImage: (options) => ({ commands }) => {
			return commands.insertContent({
				type: this.name,
				attrs: options
			});
		} };
	},
	addInputRules() {
		return [nodeInputRule({
			find: inputRegex,
			type: this.type,
			getAttributes: (match) => {
				const [, , alt, src, title] = match;
				return {
					src,
					alt,
					title
				};
			}
		})];
	}
});
function parseNodes(nodes, className = []) {
	return nodes.flatMap((node) => {
		const classes = [...className, ...node.properties ? node.properties.className : []];
		if (node.children) return parseNodes(node.children, classes);
		return {
			text: node.value,
			classes
		};
	});
}
function getHighlightNodes(result) {
	return result.value || result.children || [];
}
function registered(aliasOrLanguage) {
	return Boolean(core_default.getLanguage(aliasOrLanguage));
}
function getDecorations({ doc: doc$2, name, lowlight: lowlight$1, defaultLanguage }) {
	const decorations = [];
	findChildren(doc$2, (node) => node.type.name === name).forEach((block) => {
		var _a;
		let from = block.pos + 1;
		const language = block.node.attrs.language || defaultLanguage;
		const languages = lowlight$1.listLanguages();
		parseNodes(language && (languages.includes(language) || registered(language) || ((_a = lowlight$1.registered) == null ? void 0 : _a.call(lowlight$1, language))) ? getHighlightNodes(lowlight$1.highlight(language, block.node.textContent)) : getHighlightNodes(lowlight$1.highlightAuto(block.node.textContent))).forEach((node) => {
			const to = from + node.text.length;
			if (node.classes.length) {
				const decoration = Decoration.inline(from, to, { class: node.classes.join(" ") });
				decorations.push(decoration);
			}
			from = to;
		});
	});
	return DecorationSet.create(doc$2, decorations);
}
function isFunction(param) {
	return typeof param === "function";
}
function LowlightPlugin({ name, lowlight: lowlight$1, defaultLanguage }) {
	if (![
		"highlight",
		"highlightAuto",
		"listLanguages"
	].every((api) => isFunction(lowlight$1[api]))) throw Error("You should provide an instance of lowlight to use the code-block-lowlight extension");
	const lowlightPlugin = new Plugin({
		key: new PluginKey("lowlight"),
		state: {
			init: (_$1, { doc: doc$2 }) => getDecorations({
				doc: doc$2,
				name,
				lowlight: lowlight$1,
				defaultLanguage
			}),
			apply: (transaction, decorationSet, oldState, newState) => {
				const oldNodeName = oldState.selection.$head.parent.type.name;
				const newNodeName = newState.selection.$head.parent.type.name;
				const oldNodes = findChildren(oldState.doc, (node) => node.type.name === name);
				const newNodes = findChildren(newState.doc, (node) => node.type.name === name);
				if (transaction.docChanged && ([oldNodeName, newNodeName].includes(name) || newNodes.length !== oldNodes.length || transaction.steps.some((step$1) => {
					return step$1.from !== void 0 && step$1.to !== void 0 && oldNodes.some((node) => {
						return node.pos >= step$1.from && node.pos + node.node.nodeSize <= step$1.to;
					});
				}))) return getDecorations({
					doc: transaction.doc,
					name,
					lowlight: lowlight$1,
					defaultLanguage
				});
				return decorationSet.map(transaction.mapping, transaction.doc);
			}
		},
		props: { decorations(state) {
			return lowlightPlugin.getState(state);
		} }
	});
	return lowlightPlugin;
}
var index_default$3 = CodeBlock.extend({
	addOptions() {
		var _a;
		return {
			...(_a = this.parent) == null ? void 0 : _a.call(this),
			lowlight: {},
			languageClassPrefix: "language-",
			exitOnTripleEnter: true,
			exitOnArrowDown: true,
			defaultLanguage: null,
			enableTabIndentation: false,
			tabSize: 4,
			HTMLAttributes: {}
		};
	},
	addProseMirrorPlugins() {
		var _a;
		return [...((_a = this.parent) == null ? void 0 : _a.call(this)) || [], LowlightPlugin({
			name: this.name,
			lowlight: this.options.lowlight,
			defaultLanguage: this.options.defaultLanguage
		})];
	}
});
var index_default$5 = TaskItem;
var readFromCache;
var addToCache;
if (typeof WeakMap != "undefined") {
	let cache = /* @__PURE__ */ new WeakMap();
	readFromCache = (key) => cache.get(key);
	addToCache = (key, value) => {
		cache.set(key, value);
		return value;
	};
} else {
	const cache = [];
	const cacheSize = 10;
	let cachePos = 0;
	readFromCache = (key) => {
		for (let i$1 = 0; i$1 < cache.length; i$1 += 2) if (cache[i$1] == key) return cache[i$1 + 1];
	};
	addToCache = (key, value) => {
		if (cachePos == cacheSize) cachePos = 0;
		cache[cachePos++] = key;
		return cache[cachePos++] = value;
	};
}
var TableMap = class {
	constructor(width, height, map, problems) {
		this.width = width;
		this.height = height;
		this.map = map;
		this.problems = problems;
	}
	findCell(pos) {
		for (let i$1 = 0; i$1 < this.map.length; i$1++) {
			const curPos = this.map[i$1];
			if (curPos != pos) continue;
			const left = i$1 % this.width;
			const top = i$1 / this.width | 0;
			let right = left + 1;
			let bottom = top + 1;
			for (let j$1 = 1; right < this.width && this.map[i$1 + j$1] == curPos; j$1++) right++;
			for (let j$1 = 1; bottom < this.height && this.map[i$1 + this.width * j$1] == curPos; j$1++) bottom++;
			return {
				left,
				top,
				right,
				bottom
			};
		}
		throw new RangeError(`No cell with offset ${pos} found`);
	}
	colCount(pos) {
		for (let i$1 = 0; i$1 < this.map.length; i$1++) if (this.map[i$1] == pos) return i$1 % this.width;
		throw new RangeError(`No cell with offset ${pos} found`);
	}
	nextCell(pos, axis, dir) {
		const { left, right, top, bottom } = this.findCell(pos);
		if (axis == "horiz") {
			if (dir < 0 ? left == 0 : right == this.width) return null;
			return this.map[top * this.width + (dir < 0 ? left - 1 : right)];
		} else {
			if (dir < 0 ? top == 0 : bottom == this.height) return null;
			return this.map[left + this.width * (dir < 0 ? top - 1 : bottom)];
		}
	}
	rectBetween(a, b$1) {
		const { left: leftA, right: rightA, top: topA, bottom: bottomA } = this.findCell(a);
		const { left: leftB, right: rightB, top: topB, bottom: bottomB } = this.findCell(b$1);
		return {
			left: Math.min(leftA, leftB),
			top: Math.min(topA, topB),
			right: Math.max(rightA, rightB),
			bottom: Math.max(bottomA, bottomB)
		};
	}
	cellsInRect(rect) {
		const result = [];
		const seen = {};
		for (let row = rect.top; row < rect.bottom; row++) for (let col = rect.left; col < rect.right; col++) {
			const index = row * this.width + col;
			const pos = this.map[index];
			if (seen[pos]) continue;
			seen[pos] = true;
			if (col == rect.left && col && this.map[index - 1] == pos || row == rect.top && row && this.map[index - this.width] == pos) continue;
			result.push(pos);
		}
		return result;
	}
	positionAt(row, col, table) {
		for (let i$1 = 0, rowStart = 0;; i$1++) {
			const rowEnd = rowStart + table.child(i$1).nodeSize;
			if (i$1 == row) {
				let index = col + row * this.width;
				const rowEndIndex = (row + 1) * this.width;
				while (index < rowEndIndex && this.map[index] < rowStart) index++;
				return index == rowEndIndex ? rowEnd - 1 : this.map[index];
			}
			rowStart = rowEnd;
		}
	}
	static get(table) {
		return readFromCache(table) || addToCache(table, computeMap(table));
	}
};
function computeMap(table) {
	if (table.type.spec.tableRole != "table") throw new RangeError("Not a table node: " + table.type.name);
	const width = findWidth(table), height = table.childCount;
	const map = [];
	let mapPos = 0;
	let problems = null;
	const colWidths = [];
	for (let i$1 = 0, e = width * height; i$1 < e; i$1++) map[i$1] = 0;
	for (let row = 0, pos = 0; row < height; row++) {
		const rowNode = table.child(row);
		pos++;
		for (let i$1 = 0;; i$1++) {
			while (mapPos < map.length && map[mapPos] != 0) mapPos++;
			if (i$1 == rowNode.childCount) break;
			const cellNode = rowNode.child(i$1);
			const { colspan, rowspan, colwidth } = cellNode.attrs;
			for (let h$1 = 0; h$1 < rowspan; h$1++) {
				if (h$1 + row >= height) {
					(problems || (problems = [])).push({
						type: "overlong_rowspan",
						pos,
						n: rowspan - h$1
					});
					break;
				}
				const start = mapPos + h$1 * width;
				for (let w$1 = 0; w$1 < colspan; w$1++) {
					if (map[start + w$1] == 0) map[start + w$1] = pos;
					else (problems || (problems = [])).push({
						type: "collision",
						row,
						pos,
						n: colspan - w$1
					});
					const colW = colwidth && colwidth[w$1];
					if (colW) {
						const widthIndex = (start + w$1) % width * 2, prev = colWidths[widthIndex];
						if (prev == null || prev != colW && colWidths[widthIndex + 1] == 1) {
							colWidths[widthIndex] = colW;
							colWidths[widthIndex + 1] = 1;
						} else if (prev == colW) colWidths[widthIndex + 1]++;
					}
				}
			}
			mapPos += colspan;
			pos += cellNode.nodeSize;
		}
		const expectedPos = (row + 1) * width;
		let missing = 0;
		while (mapPos < expectedPos) if (map[mapPos++] == 0) missing++;
		if (missing) (problems || (problems = [])).push({
			type: "missing",
			row,
			n: missing
		});
		pos++;
	}
	if (width === 0 || height === 0) (problems || (problems = [])).push({ type: "zero_sized" });
	const tableMap = new TableMap(width, height, map, problems);
	let badWidths = false;
	for (let i$1 = 0; !badWidths && i$1 < colWidths.length; i$1 += 2) if (colWidths[i$1] != null && colWidths[i$1 + 1] < height) badWidths = true;
	if (badWidths) findBadColWidths(tableMap, colWidths, table);
	return tableMap;
}
function findWidth(table) {
	let width = -1;
	let hasRowSpan = false;
	for (let row = 0; row < table.childCount; row++) {
		const rowNode = table.child(row);
		let rowWidth = 0;
		if (hasRowSpan) for (let j$1 = 0; j$1 < row; j$1++) {
			const prevRow = table.child(j$1);
			for (let i$1 = 0; i$1 < prevRow.childCount; i$1++) {
				const cell = prevRow.child(i$1);
				if (j$1 + cell.attrs.rowspan > row) rowWidth += cell.attrs.colspan;
			}
		}
		for (let i$1 = 0; i$1 < rowNode.childCount; i$1++) {
			const cell = rowNode.child(i$1);
			rowWidth += cell.attrs.colspan;
			if (cell.attrs.rowspan > 1) hasRowSpan = true;
		}
		if (width == -1) width = rowWidth;
		else if (width != rowWidth) width = Math.max(width, rowWidth);
	}
	return width;
}
function findBadColWidths(map, colWidths, table) {
	if (!map.problems) map.problems = [];
	const seen = {};
	for (let i$1 = 0; i$1 < map.map.length; i$1++) {
		const pos = map.map[i$1];
		if (seen[pos]) continue;
		seen[pos] = true;
		const node = table.nodeAt(pos);
		if (!node) throw new RangeError(`No cell with offset ${pos} found`);
		let updated = null;
		const attrs = node.attrs;
		for (let j$1 = 0; j$1 < attrs.colspan; j$1++) {
			const colWidth = colWidths[(i$1 + j$1) % map.width * 2];
			if (colWidth != null && (!attrs.colwidth || attrs.colwidth[j$1] != colWidth)) (updated || (updated = freshColWidth(attrs)))[j$1] = colWidth;
		}
		if (updated) map.problems.unshift({
			type: "colwidth mismatch",
			pos,
			colwidth: updated
		});
	}
}
function freshColWidth(attrs) {
	if (attrs.colwidth) return attrs.colwidth.slice();
	const result = [];
	for (let i$1 = 0; i$1 < attrs.colspan; i$1++) result.push(0);
	return result;
}
function tableNodeTypes(schema) {
	let result = schema.cached.tableNodeTypes;
	if (!result) {
		result = schema.cached.tableNodeTypes = {};
		for (const name in schema.nodes) {
			const type = schema.nodes[name], role = type.spec.tableRole;
			if (role) result[role] = type;
		}
	}
	return result;
}
var tableEditingKey = new PluginKey("selectingCells");
function cellAround($pos) {
	for (let d = $pos.depth - 1; d > 0; d--) if ($pos.node(d).type.spec.tableRole == "row") return $pos.node(0).resolve($pos.before(d + 1));
	return null;
}
function cellWrapping($pos) {
	for (let d = $pos.depth; d > 0; d--) {
		const role = $pos.node(d).type.spec.tableRole;
		if (role === "cell" || role === "header_cell") return $pos.node(d);
	}
	return null;
}
function isInTable(state) {
	const $head = state.selection.$head;
	for (let d = $head.depth; d > 0; d--) if ($head.node(d).type.spec.tableRole == "row") return true;
	return false;
}
function selectionCell(state) {
	const sel = state.selection;
	if ("$anchorCell" in sel && sel.$anchorCell) return sel.$anchorCell.pos > sel.$headCell.pos ? sel.$anchorCell : sel.$headCell;
	else if ("node" in sel && sel.node && sel.node.type.spec.tableRole == "cell") return sel.$anchor;
	const $cell = cellAround(sel.$head) || cellNear(sel.$head);
	if ($cell) return $cell;
	throw new RangeError(`No cell found around position ${sel.head}`);
}
function cellNear($pos) {
	for (let after = $pos.nodeAfter, pos = $pos.pos; after; after = after.firstChild, pos++) {
		const role = after.type.spec.tableRole;
		if (role == "cell" || role == "header_cell") return $pos.doc.resolve(pos);
	}
	for (let before = $pos.nodeBefore, pos = $pos.pos; before; before = before.lastChild, pos--) {
		const role = before.type.spec.tableRole;
		if (role == "cell" || role == "header_cell") return $pos.doc.resolve(pos - before.nodeSize);
	}
}
function pointsAtCell($pos) {
	return $pos.parent.type.spec.tableRole == "row" && !!$pos.nodeAfter;
}
function moveCellForward($pos) {
	return $pos.node(0).resolve($pos.pos + $pos.nodeAfter.nodeSize);
}
function inSameTable($cellA, $cellB) {
	return $cellA.depth == $cellB.depth && $cellA.pos >= $cellB.start(-1) && $cellA.pos <= $cellB.end(-1);
}
function nextCell($pos, axis, dir) {
	const table = $pos.node(-1);
	const map = TableMap.get(table);
	const tableStart = $pos.start(-1);
	const moved = map.nextCell($pos.pos - tableStart, axis, dir);
	return moved == null ? null : $pos.node(0).resolve(tableStart + moved);
}
function removeColSpan(attrs, pos, n = 1) {
	const result = {
		...attrs,
		colspan: attrs.colspan - n
	};
	if (result.colwidth) {
		result.colwidth = result.colwidth.slice();
		result.colwidth.splice(pos, n);
		if (!result.colwidth.some((w$1) => w$1 > 0)) result.colwidth = null;
	}
	return result;
}
function addColSpan(attrs, pos, n = 1) {
	const result = {
		...attrs,
		colspan: attrs.colspan + n
	};
	if (result.colwidth) {
		result.colwidth = result.colwidth.slice();
		for (let i$1 = 0; i$1 < n; i$1++) result.colwidth.splice(pos, 0, 0);
	}
	return result;
}
function columnIsHeader(map, table, col) {
	const headerCell = tableNodeTypes(table.type.schema).header_cell;
	for (let row = 0; row < map.height; row++) if (table.nodeAt(map.map[col + row * map.width]).type != headerCell) return false;
	return true;
}
var CellSelection = class CellSelection$1 extends Selection {
	constructor($anchorCell, $headCell = $anchorCell) {
		const table = $anchorCell.node(-1);
		const map = TableMap.get(table);
		const tableStart = $anchorCell.start(-1);
		const rect = map.rectBetween($anchorCell.pos - tableStart, $headCell.pos - tableStart);
		const doc$2 = $anchorCell.node(0);
		const cells = map.cellsInRect(rect).filter((p) => p != $headCell.pos - tableStart);
		cells.unshift($headCell.pos - tableStart);
		const ranges = cells.map((pos) => {
			const cell = table.nodeAt(pos);
			if (!cell) throw new RangeError(`No cell with offset ${pos} found`);
			const from = tableStart + pos + 1;
			return new SelectionRange(doc$2.resolve(from), doc$2.resolve(from + cell.content.size));
		});
		super(ranges[0].$from, ranges[0].$to, ranges);
		this.$anchorCell = $anchorCell;
		this.$headCell = $headCell;
	}
	map(doc$2, mapping) {
		const $anchorCell = doc$2.resolve(mapping.map(this.$anchorCell.pos));
		const $headCell = doc$2.resolve(mapping.map(this.$headCell.pos));
		if (pointsAtCell($anchorCell) && pointsAtCell($headCell) && inSameTable($anchorCell, $headCell)) {
			const tableChanged = this.$anchorCell.node(-1) != $anchorCell.node(-1);
			if (tableChanged && this.isRowSelection()) return CellSelection$1.rowSelection($anchorCell, $headCell);
			else if (tableChanged && this.isColSelection()) return CellSelection$1.colSelection($anchorCell, $headCell);
			else return new CellSelection$1($anchorCell, $headCell);
		}
		return TextSelection.between($anchorCell, $headCell);
	}
	content() {
		const table = this.$anchorCell.node(-1);
		const map = TableMap.get(table);
		const tableStart = this.$anchorCell.start(-1);
		const rect = map.rectBetween(this.$anchorCell.pos - tableStart, this.$headCell.pos - tableStart);
		const seen = {};
		const rows = [];
		for (let row = rect.top; row < rect.bottom; row++) {
			const rowContent = [];
			for (let index = row * map.width + rect.left, col = rect.left; col < rect.right; col++, index++) {
				const pos = map.map[index];
				if (seen[pos]) continue;
				seen[pos] = true;
				const cellRect = map.findCell(pos);
				let cell = table.nodeAt(pos);
				if (!cell) throw new RangeError(`No cell with offset ${pos} found`);
				const extraLeft = rect.left - cellRect.left;
				const extraRight = cellRect.right - rect.right;
				if (extraLeft > 0 || extraRight > 0) {
					let attrs = cell.attrs;
					if (extraLeft > 0) attrs = removeColSpan(attrs, 0, extraLeft);
					if (extraRight > 0) attrs = removeColSpan(attrs, attrs.colspan - extraRight, extraRight);
					if (cellRect.left < rect.left) {
						cell = cell.type.createAndFill(attrs);
						if (!cell) throw new RangeError(`Could not create cell with attrs ${JSON.stringify(attrs)}`);
					} else cell = cell.type.create(attrs, cell.content);
				}
				if (cellRect.top < rect.top || cellRect.bottom > rect.bottom) {
					const attrs = {
						...cell.attrs,
						rowspan: Math.min(cellRect.bottom, rect.bottom) - Math.max(cellRect.top, rect.top)
					};
					if (cellRect.top < rect.top) cell = cell.type.createAndFill(attrs);
					else cell = cell.type.create(attrs, cell.content);
				}
				rowContent.push(cell);
			}
			rows.push(table.child(row).copy(Fragment.from(rowContent)));
		}
		const fragment = this.isColSelection() && this.isRowSelection() ? table : rows;
		return new Slice(Fragment.from(fragment), 1, 1);
	}
	replace(tr$1, content = Slice.empty) {
		const mapFrom = tr$1.steps.length, ranges = this.ranges;
		for (let i$1 = 0; i$1 < ranges.length; i$1++) {
			const { $from, $to } = ranges[i$1], mapping = tr$1.mapping.slice(mapFrom);
			tr$1.replace(mapping.map($from.pos), mapping.map($to.pos), i$1 ? Slice.empty : content);
		}
		const sel = Selection.findFrom(tr$1.doc.resolve(tr$1.mapping.slice(mapFrom).map(this.to)), -1);
		if (sel) tr$1.setSelection(sel);
	}
	replaceWith(tr$1, node) {
		this.replace(tr$1, new Slice(Fragment.from(node), 0, 0));
	}
	forEachCell(f) {
		const table = this.$anchorCell.node(-1);
		const map = TableMap.get(table);
		const tableStart = this.$anchorCell.start(-1);
		const cells = map.cellsInRect(map.rectBetween(this.$anchorCell.pos - tableStart, this.$headCell.pos - tableStart));
		for (let i$1 = 0; i$1 < cells.length; i$1++) f(table.nodeAt(cells[i$1]), tableStart + cells[i$1]);
	}
	isColSelection() {
		const anchorTop = this.$anchorCell.index(-1);
		const headTop = this.$headCell.index(-1);
		if (Math.min(anchorTop, headTop) > 0) return false;
		const anchorBottom = anchorTop + this.$anchorCell.nodeAfter.attrs.rowspan;
		const headBottom = headTop + this.$headCell.nodeAfter.attrs.rowspan;
		return Math.max(anchorBottom, headBottom) == this.$headCell.node(-1).childCount;
	}
	static colSelection($anchorCell, $headCell = $anchorCell) {
		const table = $anchorCell.node(-1);
		const map = TableMap.get(table);
		const tableStart = $anchorCell.start(-1);
		const anchorRect = map.findCell($anchorCell.pos - tableStart);
		const headRect = map.findCell($headCell.pos - tableStart);
		const doc$2 = $anchorCell.node(0);
		if (anchorRect.top <= headRect.top) {
			if (anchorRect.top > 0) $anchorCell = doc$2.resolve(tableStart + map.map[anchorRect.left]);
			if (headRect.bottom < map.height) $headCell = doc$2.resolve(tableStart + map.map[map.width * (map.height - 1) + headRect.right - 1]);
		} else {
			if (headRect.top > 0) $headCell = doc$2.resolve(tableStart + map.map[headRect.left]);
			if (anchorRect.bottom < map.height) $anchorCell = doc$2.resolve(tableStart + map.map[map.width * (map.height - 1) + anchorRect.right - 1]);
		}
		return new CellSelection$1($anchorCell, $headCell);
	}
	isRowSelection() {
		const table = this.$anchorCell.node(-1);
		const map = TableMap.get(table);
		const tableStart = this.$anchorCell.start(-1);
		const anchorLeft = map.colCount(this.$anchorCell.pos - tableStart);
		const headLeft = map.colCount(this.$headCell.pos - tableStart);
		if (Math.min(anchorLeft, headLeft) > 0) return false;
		const anchorRight = anchorLeft + this.$anchorCell.nodeAfter.attrs.colspan;
		const headRight = headLeft + this.$headCell.nodeAfter.attrs.colspan;
		return Math.max(anchorRight, headRight) == map.width;
	}
	eq(other) {
		return other instanceof CellSelection$1 && other.$anchorCell.pos == this.$anchorCell.pos && other.$headCell.pos == this.$headCell.pos;
	}
	static rowSelection($anchorCell, $headCell = $anchorCell) {
		const table = $anchorCell.node(-1);
		const map = TableMap.get(table);
		const tableStart = $anchorCell.start(-1);
		const anchorRect = map.findCell($anchorCell.pos - tableStart);
		const headRect = map.findCell($headCell.pos - tableStart);
		const doc$2 = $anchorCell.node(0);
		if (anchorRect.left <= headRect.left) {
			if (anchorRect.left > 0) $anchorCell = doc$2.resolve(tableStart + map.map[anchorRect.top * map.width]);
			if (headRect.right < map.width) $headCell = doc$2.resolve(tableStart + map.map[map.width * (headRect.top + 1) - 1]);
		} else {
			if (headRect.left > 0) $headCell = doc$2.resolve(tableStart + map.map[headRect.top * map.width]);
			if (anchorRect.right < map.width) $anchorCell = doc$2.resolve(tableStart + map.map[map.width * (anchorRect.top + 1) - 1]);
		}
		return new CellSelection$1($anchorCell, $headCell);
	}
	toJSON() {
		return {
			type: "cell",
			anchor: this.$anchorCell.pos,
			head: this.$headCell.pos
		};
	}
	static fromJSON(doc$2, json) {
		return new CellSelection$1(doc$2.resolve(json.anchor), doc$2.resolve(json.head));
	}
	static create(doc$2, anchorCell, headCell = anchorCell) {
		return new CellSelection$1(doc$2.resolve(anchorCell), doc$2.resolve(headCell));
	}
	getBookmark() {
		return new CellBookmark(this.$anchorCell.pos, this.$headCell.pos);
	}
};
CellSelection.prototype.visible = false;
Selection.jsonID("cell", CellSelection);
var CellBookmark = class CellBookmark$1 {
	constructor(anchor, head) {
		this.anchor = anchor;
		this.head = head;
	}
	map(mapping) {
		return new CellBookmark$1(mapping.map(this.anchor), mapping.map(this.head));
	}
	resolve(doc$2) {
		const $anchorCell = doc$2.resolve(this.anchor), $headCell = doc$2.resolve(this.head);
		if ($anchorCell.parent.type.spec.tableRole == "row" && $headCell.parent.type.spec.tableRole == "row" && $anchorCell.index() < $anchorCell.parent.childCount && $headCell.index() < $headCell.parent.childCount && inSameTable($anchorCell, $headCell)) return new CellSelection($anchorCell, $headCell);
		else return Selection.near($headCell, 1);
	}
};
function drawCellSelection(state) {
	if (!(state.selection instanceof CellSelection)) return null;
	const cells = [];
	state.selection.forEachCell((node, pos) => {
		cells.push(Decoration.node(pos, pos + node.nodeSize, { class: "selectedCell" }));
	});
	return DecorationSet.create(state.doc, cells);
}
function isCellBoundarySelection({ $from, $to }) {
	if ($from.pos == $to.pos || $from.pos < $to.pos - 6) return false;
	let afterFrom = $from.pos;
	let beforeTo = $to.pos;
	let depth = $from.depth;
	for (; depth >= 0; depth--, afterFrom++) if ($from.after(depth + 1) < $from.end(depth)) break;
	for (let d = $to.depth; d >= 0; d--, beforeTo--) if ($to.before(d + 1) > $to.start(d)) break;
	return afterFrom == beforeTo && /row|table/.test($from.node(depth).type.spec.tableRole);
}
function isTextSelectionAcrossCells({ $from, $to }) {
	let fromCellBoundaryNode;
	let toCellBoundaryNode;
	for (let i$1 = $from.depth; i$1 > 0; i$1--) {
		const node = $from.node(i$1);
		if (node.type.spec.tableRole === "cell" || node.type.spec.tableRole === "header_cell") {
			fromCellBoundaryNode = node;
			break;
		}
	}
	for (let i$1 = $to.depth; i$1 > 0; i$1--) {
		const node = $to.node(i$1);
		if (node.type.spec.tableRole === "cell" || node.type.spec.tableRole === "header_cell") {
			toCellBoundaryNode = node;
			break;
		}
	}
	return fromCellBoundaryNode !== toCellBoundaryNode && $to.parentOffset === 0;
}
function normalizeSelection(state, tr$1, allowTableNodeSelection) {
	const sel = (tr$1 || state).selection;
	const doc$2 = (tr$1 || state).doc;
	let normalize$1;
	let role;
	if (sel instanceof NodeSelection && (role = sel.node.type.spec.tableRole)) {
		if (role == "cell" || role == "header_cell") normalize$1 = CellSelection.create(doc$2, sel.from);
		else if (role == "row") {
			const $cell = doc$2.resolve(sel.from + 1);
			normalize$1 = CellSelection.rowSelection($cell, $cell);
		} else if (!allowTableNodeSelection) {
			const map = TableMap.get(sel.node);
			const start = sel.from + 1;
			const lastCell = start + map.map[map.width * map.height - 1];
			normalize$1 = CellSelection.create(doc$2, start + 1, lastCell);
		}
	} else if (sel instanceof TextSelection && isCellBoundarySelection(sel)) normalize$1 = TextSelection.create(doc$2, sel.from);
	else if (sel instanceof TextSelection && isTextSelectionAcrossCells(sel)) normalize$1 = TextSelection.create(doc$2, sel.$from.start(), sel.$from.end());
	if (normalize$1) (tr$1 || (tr$1 = state.tr)).setSelection(normalize$1);
	return tr$1;
}
var fixTablesKey = new PluginKey("fix-tables");
function changedDescendants(old, cur, offset, f) {
	const oldSize = old.childCount, curSize = cur.childCount;
	outer: for (let i$1 = 0, j$1 = 0; i$1 < curSize; i$1++) {
		const child = cur.child(i$1);
		for (let scan = j$1, e = Math.min(oldSize, i$1 + 3); scan < e; scan++) if (old.child(scan) == child) {
			j$1 = scan + 1;
			offset += child.nodeSize;
			continue outer;
		}
		f(child, offset);
		if (j$1 < oldSize && old.child(j$1).sameMarkup(child)) changedDescendants(old.child(j$1), child, offset + 1, f);
		else child.nodesBetween(0, child.content.size, f, offset + 1);
		offset += child.nodeSize;
	}
}
function fixTables(state, oldState) {
	let tr$1;
	const check = (node, pos) => {
		if (node.type.spec.tableRole == "table") tr$1 = fixTable(state, node, pos, tr$1);
	};
	if (!oldState) state.doc.descendants(check);
	else if (oldState.doc != state.doc) changedDescendants(oldState.doc, state.doc, 0, check);
	return tr$1;
}
function fixTable(state, table, tablePos, tr$1) {
	const map = TableMap.get(table);
	if (!map.problems) return tr$1;
	if (!tr$1) tr$1 = state.tr;
	const mustAdd = [];
	for (let i$1 = 0; i$1 < map.height; i$1++) mustAdd.push(0);
	for (let i$1 = 0; i$1 < map.problems.length; i$1++) {
		const prob = map.problems[i$1];
		if (prob.type == "collision") {
			const cell = table.nodeAt(prob.pos);
			if (!cell) continue;
			const attrs = cell.attrs;
			for (let j$1 = 0; j$1 < attrs.rowspan; j$1++) mustAdd[prob.row + j$1] += prob.n;
			tr$1.setNodeMarkup(tr$1.mapping.map(tablePos + 1 + prob.pos), null, removeColSpan(attrs, attrs.colspan - prob.n, prob.n));
		} else if (prob.type == "missing") mustAdd[prob.row] += prob.n;
		else if (prob.type == "overlong_rowspan") {
			const cell = table.nodeAt(prob.pos);
			if (!cell) continue;
			tr$1.setNodeMarkup(tr$1.mapping.map(tablePos + 1 + prob.pos), null, {
				...cell.attrs,
				rowspan: cell.attrs.rowspan - prob.n
			});
		} else if (prob.type == "colwidth mismatch") {
			const cell = table.nodeAt(prob.pos);
			if (!cell) continue;
			tr$1.setNodeMarkup(tr$1.mapping.map(tablePos + 1 + prob.pos), null, {
				...cell.attrs,
				colwidth: prob.colwidth
			});
		} else if (prob.type == "zero_sized") {
			const pos = tr$1.mapping.map(tablePos);
			tr$1.delete(pos, pos + table.nodeSize);
		}
	}
	let first$1, last;
	for (let i$1 = 0; i$1 < mustAdd.length; i$1++) if (mustAdd[i$1]) {
		if (first$1 == null) first$1 = i$1;
		last = i$1;
	}
	for (let i$1 = 0, pos = tablePos + 1; i$1 < map.height; i$1++) {
		const row = table.child(i$1);
		const end = pos + row.nodeSize;
		const add = mustAdd[i$1];
		if (add > 0) {
			let role = "cell";
			if (row.firstChild) role = row.firstChild.type.spec.tableRole;
			const nodes = [];
			for (let j$1 = 0; j$1 < add; j$1++) {
				const node = tableNodeTypes(state.schema)[role].createAndFill();
				if (node) nodes.push(node);
			}
			const side = (i$1 == 0 || first$1 == i$1 - 1) && last == i$1 ? pos + 1 : end - 1;
			tr$1.insert(tr$1.mapping.map(side), nodes);
		}
		pos = end;
	}
	return tr$1.setMeta(fixTablesKey, { fixTables: true });
}
function selectedRect(state) {
	const sel = state.selection;
	const $pos = selectionCell(state);
	const table = $pos.node(-1);
	const tableStart = $pos.start(-1);
	const map = TableMap.get(table);
	return {
		...sel instanceof CellSelection ? map.rectBetween(sel.$anchorCell.pos - tableStart, sel.$headCell.pos - tableStart) : map.findCell($pos.pos - tableStart),
		tableStart,
		map,
		table
	};
}
function addColumn(tr$1, { map, tableStart, table }, col) {
	let refColumn = col > 0 ? -1 : 0;
	if (columnIsHeader(map, table, col + refColumn)) refColumn = col == 0 || col == map.width ? null : 0;
	for (let row = 0; row < map.height; row++) {
		const index = row * map.width + col;
		if (col > 0 && col < map.width && map.map[index - 1] == map.map[index]) {
			const pos = map.map[index];
			const cell = table.nodeAt(pos);
			tr$1.setNodeMarkup(tr$1.mapping.map(tableStart + pos), null, addColSpan(cell.attrs, col - map.colCount(pos)));
			row += cell.attrs.rowspan - 1;
		} else {
			const type = refColumn == null ? tableNodeTypes(table.type.schema).cell : table.nodeAt(map.map[index + refColumn]).type;
			const pos = map.positionAt(row, col, table);
			tr$1.insert(tr$1.mapping.map(tableStart + pos), type.createAndFill());
		}
	}
	return tr$1;
}
function addColumnBefore(state, dispatch) {
	if (!isInTable(state)) return false;
	if (dispatch) {
		const rect = selectedRect(state);
		dispatch(addColumn(state.tr, rect, rect.left));
	}
	return true;
}
function addColumnAfter(state, dispatch) {
	if (!isInTable(state)) return false;
	if (dispatch) {
		const rect = selectedRect(state);
		dispatch(addColumn(state.tr, rect, rect.right));
	}
	return true;
}
function removeColumn(tr$1, { map, table, tableStart }, col) {
	const mapStart = tr$1.mapping.maps.length;
	for (let row = 0; row < map.height;) {
		const index = row * map.width + col;
		const pos = map.map[index];
		const cell = table.nodeAt(pos);
		const attrs = cell.attrs;
		if (col > 0 && map.map[index - 1] == pos || col < map.width - 1 && map.map[index + 1] == pos) tr$1.setNodeMarkup(tr$1.mapping.slice(mapStart).map(tableStart + pos), null, removeColSpan(attrs, col - map.colCount(pos)));
		else {
			const start = tr$1.mapping.slice(mapStart).map(tableStart + pos);
			tr$1.delete(start, start + cell.nodeSize);
		}
		row += attrs.rowspan;
	}
}
function deleteColumn(state, dispatch) {
	if (!isInTable(state)) return false;
	if (dispatch) {
		const rect = selectedRect(state);
		const tr$1 = state.tr;
		if (rect.left == 0 && rect.right == rect.map.width) return false;
		for (let i$1 = rect.right - 1;; i$1--) {
			removeColumn(tr$1, rect, i$1);
			if (i$1 == rect.left) break;
			const table = rect.tableStart ? tr$1.doc.nodeAt(rect.tableStart - 1) : tr$1.doc;
			if (!table) throw new RangeError("No table found");
			rect.table = table;
			rect.map = TableMap.get(table);
		}
		dispatch(tr$1);
	}
	return true;
}
function rowIsHeader(map, table, row) {
	var _table$nodeAt;
	const headerCell = tableNodeTypes(table.type.schema).header_cell;
	for (let col = 0; col < map.width; col++) if (((_table$nodeAt = table.nodeAt(map.map[col + row * map.width])) === null || _table$nodeAt === void 0 ? void 0 : _table$nodeAt.type) != headerCell) return false;
	return true;
}
function addRow(tr$1, { map, tableStart, table }, row) {
	let rowPos = tableStart;
	for (let i$1 = 0; i$1 < row; i$1++) rowPos += table.child(i$1).nodeSize;
	const cells = [];
	let refRow = row > 0 ? -1 : 0;
	if (rowIsHeader(map, table, row + refRow)) refRow = row == 0 || row == map.height ? null : 0;
	for (let col = 0, index = map.width * row; col < map.width; col++, index++) if (row > 0 && row < map.height && map.map[index] == map.map[index - map.width]) {
		const pos = map.map[index];
		const attrs = table.nodeAt(pos).attrs;
		tr$1.setNodeMarkup(tableStart + pos, null, {
			...attrs,
			rowspan: attrs.rowspan + 1
		});
		col += attrs.colspan - 1;
	} else {
		var _table$nodeAt2;
		const type = refRow == null ? tableNodeTypes(table.type.schema).cell : (_table$nodeAt2 = table.nodeAt(map.map[index + refRow * map.width])) === null || _table$nodeAt2 === void 0 ? void 0 : _table$nodeAt2.type;
		const node = type === null || type === void 0 ? void 0 : type.createAndFill();
		if (node) cells.push(node);
	}
	tr$1.insert(rowPos, tableNodeTypes(table.type.schema).row.create(null, cells));
	return tr$1;
}
function addRowBefore(state, dispatch) {
	if (!isInTable(state)) return false;
	if (dispatch) {
		const rect = selectedRect(state);
		dispatch(addRow(state.tr, rect, rect.top));
	}
	return true;
}
function addRowAfter(state, dispatch) {
	if (!isInTable(state)) return false;
	if (dispatch) {
		const rect = selectedRect(state);
		dispatch(addRow(state.tr, rect, rect.bottom));
	}
	return true;
}
function removeRow(tr$1, { map, table, tableStart }, row) {
	let rowPos = 0;
	for (let i$1 = 0; i$1 < row; i$1++) rowPos += table.child(i$1).nodeSize;
	const nextRow = rowPos + table.child(row).nodeSize;
	const mapFrom = tr$1.mapping.maps.length;
	tr$1.delete(rowPos + tableStart, nextRow + tableStart);
	const seen = /* @__PURE__ */ new Set();
	for (let col = 0, index = row * map.width; col < map.width; col++, index++) {
		const pos = map.map[index];
		if (seen.has(pos)) continue;
		seen.add(pos);
		if (row > 0 && pos == map.map[index - map.width]) {
			const attrs = table.nodeAt(pos).attrs;
			tr$1.setNodeMarkup(tr$1.mapping.slice(mapFrom).map(pos + tableStart), null, {
				...attrs,
				rowspan: attrs.rowspan - 1
			});
			col += attrs.colspan - 1;
		} else if (row < map.height && pos == map.map[index + map.width]) {
			const cell = table.nodeAt(pos);
			const attrs = cell.attrs;
			const copy$1 = cell.type.create({
				...attrs,
				rowspan: cell.attrs.rowspan - 1
			}, cell.content);
			const newPos = map.positionAt(row + 1, col, table);
			tr$1.insert(tr$1.mapping.slice(mapFrom).map(tableStart + newPos), copy$1);
			col += attrs.colspan - 1;
		}
	}
}
function deleteRow(state, dispatch) {
	if (!isInTable(state)) return false;
	if (dispatch) {
		const rect = selectedRect(state), tr$1 = state.tr;
		if (rect.top == 0 && rect.bottom == rect.map.height) return false;
		for (let i$1 = rect.bottom - 1;; i$1--) {
			removeRow(tr$1, rect, i$1);
			if (i$1 == rect.top) break;
			const table = rect.tableStart ? tr$1.doc.nodeAt(rect.tableStart - 1) : tr$1.doc;
			if (!table) throw new RangeError("No table found");
			rect.table = table;
			rect.map = TableMap.get(rect.table);
		}
		dispatch(tr$1);
	}
	return true;
}
function isEmpty(cell) {
	const c = cell.content;
	return c.childCount == 1 && c.child(0).isTextblock && c.child(0).childCount == 0;
}
function cellsOverlapRectangle({ width, height, map }, rect) {
	let indexTop = rect.top * width + rect.left, indexLeft = indexTop;
	let indexBottom = (rect.bottom - 1) * width + rect.left, indexRight = indexTop + (rect.right - rect.left - 1);
	for (let i$1 = rect.top; i$1 < rect.bottom; i$1++) {
		if (rect.left > 0 && map[indexLeft] == map[indexLeft - 1] || rect.right < width && map[indexRight] == map[indexRight + 1]) return true;
		indexLeft += width;
		indexRight += width;
	}
	for (let i$1 = rect.left; i$1 < rect.right; i$1++) {
		if (rect.top > 0 && map[indexTop] == map[indexTop - width] || rect.bottom < height && map[indexBottom] == map[indexBottom + width]) return true;
		indexTop++;
		indexBottom++;
	}
	return false;
}
function mergeCells(state, dispatch) {
	const sel = state.selection;
	if (!(sel instanceof CellSelection) || sel.$anchorCell.pos == sel.$headCell.pos) return false;
	const rect = selectedRect(state), { map } = rect;
	if (cellsOverlapRectangle(map, rect)) return false;
	if (dispatch) {
		const tr$1 = state.tr;
		const seen = {};
		let content = Fragment.empty;
		let mergedPos;
		let mergedCell;
		for (let row = rect.top; row < rect.bottom; row++) for (let col = rect.left; col < rect.right; col++) {
			const cellPos = map.map[row * map.width + col];
			const cell = rect.table.nodeAt(cellPos);
			if (seen[cellPos] || !cell) continue;
			seen[cellPos] = true;
			if (mergedPos == null) {
				mergedPos = cellPos;
				mergedCell = cell;
			} else {
				if (!isEmpty(cell)) content = content.append(cell.content);
				const mapped = tr$1.mapping.map(cellPos + rect.tableStart);
				tr$1.delete(mapped, mapped + cell.nodeSize);
			}
		}
		if (mergedPos == null || mergedCell == null) return true;
		tr$1.setNodeMarkup(mergedPos + rect.tableStart, null, {
			...addColSpan(mergedCell.attrs, mergedCell.attrs.colspan, rect.right - rect.left - mergedCell.attrs.colspan),
			rowspan: rect.bottom - rect.top
		});
		if (content.size > 0) {
			const end = mergedPos + 1 + mergedCell.content.size;
			const start = isEmpty(mergedCell) ? mergedPos + 1 : end;
			tr$1.replaceWith(start + rect.tableStart, end + rect.tableStart, content);
		}
		tr$1.setSelection(new CellSelection(tr$1.doc.resolve(mergedPos + rect.tableStart)));
		dispatch(tr$1);
	}
	return true;
}
function splitCell(state, dispatch) {
	const nodeTypes = tableNodeTypes(state.schema);
	return splitCellWithType(({ node }) => {
		return nodeTypes[node.type.spec.tableRole];
	})(state, dispatch);
}
function splitCellWithType(getCellType) {
	return (state, dispatch) => {
		const sel = state.selection;
		let cellNode;
		let cellPos;
		if (!(sel instanceof CellSelection)) {
			var _cellAround;
			cellNode = cellWrapping(sel.$from);
			if (!cellNode) return false;
			cellPos = (_cellAround = cellAround(sel.$from)) === null || _cellAround === void 0 ? void 0 : _cellAround.pos;
		} else {
			if (sel.$anchorCell.pos != sel.$headCell.pos) return false;
			cellNode = sel.$anchorCell.nodeAfter;
			cellPos = sel.$anchorCell.pos;
		}
		if (cellNode == null || cellPos == null) return false;
		if (cellNode.attrs.colspan == 1 && cellNode.attrs.rowspan == 1) return false;
		if (dispatch) {
			let baseAttrs = cellNode.attrs;
			const attrs = [];
			const colwidth = baseAttrs.colwidth;
			if (baseAttrs.rowspan > 1) baseAttrs = {
				...baseAttrs,
				rowspan: 1
			};
			if (baseAttrs.colspan > 1) baseAttrs = {
				...baseAttrs,
				colspan: 1
			};
			const rect = selectedRect(state), tr$1 = state.tr;
			for (let i$1 = 0; i$1 < rect.right - rect.left; i$1++) attrs.push(colwidth ? {
				...baseAttrs,
				colwidth: colwidth && colwidth[i$1] ? [colwidth[i$1]] : null
			} : baseAttrs);
			let lastCell;
			for (let row = rect.top; row < rect.bottom; row++) {
				let pos = rect.map.positionAt(row, rect.left, rect.table);
				if (row == rect.top) pos += cellNode.nodeSize;
				for (let col = rect.left, i$1 = 0; col < rect.right; col++, i$1++) {
					if (col == rect.left && row == rect.top) continue;
					tr$1.insert(lastCell = tr$1.mapping.map(pos + rect.tableStart, 1), getCellType({
						node: cellNode,
						row,
						col
					}).createAndFill(attrs[i$1]));
				}
			}
			tr$1.setNodeMarkup(cellPos, getCellType({
				node: cellNode,
				row: rect.top,
				col: rect.left
			}), attrs[0]);
			if (sel instanceof CellSelection) tr$1.setSelection(new CellSelection(tr$1.doc.resolve(sel.$anchorCell.pos), lastCell ? tr$1.doc.resolve(lastCell) : void 0));
			dispatch(tr$1);
		}
		return true;
	};
}
function setCellAttr(name, value) {
	return function(state, dispatch) {
		if (!isInTable(state)) return false;
		const $cell = selectionCell(state);
		if ($cell.nodeAfter.attrs[name] === value) return false;
		if (dispatch) {
			const tr$1 = state.tr;
			if (state.selection instanceof CellSelection) state.selection.forEachCell((node, pos) => {
				if (node.attrs[name] !== value) tr$1.setNodeMarkup(pos, null, {
					...node.attrs,
					[name]: value
				});
			});
			else tr$1.setNodeMarkup($cell.pos, null, {
				...$cell.nodeAfter.attrs,
				[name]: value
			});
			dispatch(tr$1);
		}
		return true;
	};
}
function deprecated_toggleHeader(type) {
	return function(state, dispatch) {
		if (!isInTable(state)) return false;
		if (dispatch) {
			const types = tableNodeTypes(state.schema);
			const rect = selectedRect(state), tr$1 = state.tr;
			const cells = rect.map.cellsInRect(type == "column" ? {
				left: rect.left,
				top: 0,
				right: rect.right,
				bottom: rect.map.height
			} : type == "row" ? {
				left: 0,
				top: rect.top,
				right: rect.map.width,
				bottom: rect.bottom
			} : rect);
			const nodes = cells.map((pos) => rect.table.nodeAt(pos));
			for (let i$1 = 0; i$1 < cells.length; i$1++) if (nodes[i$1].type == types.header_cell) tr$1.setNodeMarkup(rect.tableStart + cells[i$1], types.cell, nodes[i$1].attrs);
			if (tr$1.steps.length === 0) for (let i$1 = 0; i$1 < cells.length; i$1++) tr$1.setNodeMarkup(rect.tableStart + cells[i$1], types.header_cell, nodes[i$1].attrs);
			dispatch(tr$1);
		}
		return true;
	};
}
function isHeaderEnabledByType(type, rect, types) {
	const cellPositions = rect.map.cellsInRect({
		left: 0,
		top: 0,
		right: type == "row" ? rect.map.width : 1,
		bottom: type == "column" ? rect.map.height : 1
	});
	for (let i$1 = 0; i$1 < cellPositions.length; i$1++) {
		const cell = rect.table.nodeAt(cellPositions[i$1]);
		if (cell && cell.type !== types.header_cell) return false;
	}
	return true;
}
function toggleHeader(type, options) {
	options = options || { useDeprecatedLogic: false };
	if (options.useDeprecatedLogic) return deprecated_toggleHeader(type);
	return function(state, dispatch) {
		if (!isInTable(state)) return false;
		if (dispatch) {
			const types = tableNodeTypes(state.schema);
			const rect = selectedRect(state), tr$1 = state.tr;
			const isHeaderRowEnabled = isHeaderEnabledByType("row", rect, types);
			const isHeaderColumnEnabled = isHeaderEnabledByType("column", rect, types);
			const selectionStartsAt = (type === "column" ? isHeaderRowEnabled : type === "row" ? isHeaderColumnEnabled : false) ? 1 : 0;
			const cellsRect = type == "column" ? {
				left: 0,
				top: selectionStartsAt,
				right: 1,
				bottom: rect.map.height
			} : type == "row" ? {
				left: selectionStartsAt,
				top: 0,
				right: rect.map.width,
				bottom: 1
			} : rect;
			const newType = type == "column" ? isHeaderColumnEnabled ? types.cell : types.header_cell : type == "row" ? isHeaderRowEnabled ? types.cell : types.header_cell : types.cell;
			rect.map.cellsInRect(cellsRect).forEach((relativeCellPos) => {
				const cellPos = relativeCellPos + rect.tableStart;
				const cell = tr$1.doc.nodeAt(cellPos);
				if (cell) tr$1.setNodeMarkup(cellPos, newType, cell.attrs);
			});
			dispatch(tr$1);
		}
		return true;
	};
}
toggleHeader("row", { useDeprecatedLogic: true });
toggleHeader("column", { useDeprecatedLogic: true });
var toggleHeaderCell = toggleHeader("cell", { useDeprecatedLogic: true });
function findNextCell($cell, dir) {
	if (dir < 0) {
		const before = $cell.nodeBefore;
		if (before) return $cell.pos - before.nodeSize;
		for (let row = $cell.index(-1) - 1, rowEnd = $cell.before(); row >= 0; row--) {
			const rowNode = $cell.node(-1).child(row);
			const lastChild = rowNode.lastChild;
			if (lastChild) return rowEnd - 1 - lastChild.nodeSize;
			rowEnd -= rowNode.nodeSize;
		}
	} else {
		if ($cell.index() < $cell.parent.childCount - 1) return $cell.pos + $cell.nodeAfter.nodeSize;
		const table = $cell.node(-1);
		for (let row = $cell.indexAfter(-1), rowStart = $cell.after(); row < table.childCount; row++) {
			const rowNode = table.child(row);
			if (rowNode.childCount) return rowStart + 1;
			rowStart += rowNode.nodeSize;
		}
	}
	return null;
}
function goToNextCell(direction) {
	return function(state, dispatch) {
		if (!isInTable(state)) return false;
		const cell = findNextCell(selectionCell(state), direction);
		if (cell == null) return false;
		if (dispatch) {
			const $cell = state.doc.resolve(cell);
			dispatch(state.tr.setSelection(TextSelection.between($cell, moveCellForward($cell))).scrollIntoView());
		}
		return true;
	};
}
function deleteTable(state, dispatch) {
	const $pos = state.selection.$anchor;
	for (let d = $pos.depth; d > 0; d--) if ($pos.node(d).type.spec.tableRole == "table") {
		if (dispatch) dispatch(state.tr.delete($pos.before(d), $pos.after(d)).scrollIntoView());
		return true;
	}
	return false;
}
function deleteCellSelection(state, dispatch) {
	const sel = state.selection;
	if (!(sel instanceof CellSelection)) return false;
	if (dispatch) {
		const tr$1 = state.tr;
		const baseContent = tableNodeTypes(state.schema).cell.createAndFill().content;
		sel.forEachCell((cell, pos) => {
			if (!cell.content.eq(baseContent)) tr$1.replace(tr$1.mapping.map(pos + 1), tr$1.mapping.map(pos + cell.nodeSize - 1), new Slice(baseContent, 0, 0));
		});
		if (tr$1.docChanged) dispatch(tr$1);
	}
	return true;
}
function pastedCells(slice) {
	if (slice.size === 0) return null;
	let { content, openStart, openEnd } = slice;
	while (content.childCount == 1 && (openStart > 0 && openEnd > 0 || content.child(0).type.spec.tableRole == "table")) {
		openStart--;
		openEnd--;
		content = content.child(0).content;
	}
	const first$1 = content.child(0);
	const role = first$1.type.spec.tableRole;
	const schema = first$1.type.schema, rows = [];
	if (role == "row") for (let i$1 = 0; i$1 < content.childCount; i$1++) {
		let cells = content.child(i$1).content;
		const left = i$1 ? 0 : Math.max(0, openStart - 1);
		const right = i$1 < content.childCount - 1 ? 0 : Math.max(0, openEnd - 1);
		if (left || right) cells = fitSlice(tableNodeTypes(schema).row, new Slice(cells, left, right)).content;
		rows.push(cells);
	}
	else if (role == "cell" || role == "header_cell") rows.push(openStart || openEnd ? fitSlice(tableNodeTypes(schema).row, new Slice(content, openStart, openEnd)).content : content);
	else return null;
	return ensureRectangular(schema, rows);
}
function ensureRectangular(schema, rows) {
	const widths = [];
	for (let i$1 = 0; i$1 < rows.length; i$1++) {
		const row = rows[i$1];
		for (let j$1 = row.childCount - 1; j$1 >= 0; j$1--) {
			const { rowspan, colspan } = row.child(j$1).attrs;
			for (let r = i$1; r < i$1 + rowspan; r++) widths[r] = (widths[r] || 0) + colspan;
		}
	}
	let width = 0;
	for (let r = 0; r < widths.length; r++) width = Math.max(width, widths[r]);
	for (let r = 0; r < widths.length; r++) {
		if (r >= rows.length) rows.push(Fragment.empty);
		if (widths[r] < width) {
			const empty$1 = tableNodeTypes(schema).cell.createAndFill();
			const cells = [];
			for (let i$1 = widths[r]; i$1 < width; i$1++) cells.push(empty$1);
			rows[r] = rows[r].append(Fragment.from(cells));
		}
	}
	return {
		height: rows.length,
		width,
		rows
	};
}
function fitSlice(nodeType, slice) {
	const node = nodeType.createAndFill();
	return new Transform(node).replace(0, node.content.size, slice).doc;
}
function clipCells({ width, height, rows }, newWidth, newHeight) {
	if (width != newWidth) {
		const added = [];
		const newRows = [];
		for (let row = 0; row < rows.length; row++) {
			const frag = rows[row], cells = [];
			for (let col = added[row] || 0, i$1 = 0; col < newWidth; i$1++) {
				let cell = frag.child(i$1 % frag.childCount);
				if (col + cell.attrs.colspan > newWidth) cell = cell.type.createChecked(removeColSpan(cell.attrs, cell.attrs.colspan, col + cell.attrs.colspan - newWidth), cell.content);
				cells.push(cell);
				col += cell.attrs.colspan;
				for (let j$1 = 1; j$1 < cell.attrs.rowspan; j$1++) added[row + j$1] = (added[row + j$1] || 0) + cell.attrs.colspan;
			}
			newRows.push(Fragment.from(cells));
		}
		rows = newRows;
		width = newWidth;
	}
	if (height != newHeight) {
		const newRows = [];
		for (let row = 0, i$1 = 0; row < newHeight; row++, i$1++) {
			const cells = [], source = rows[i$1 % height];
			for (let j$1 = 0; j$1 < source.childCount; j$1++) {
				let cell = source.child(j$1);
				if (row + cell.attrs.rowspan > newHeight) cell = cell.type.create({
					...cell.attrs,
					rowspan: Math.max(1, newHeight - cell.attrs.rowspan)
				}, cell.content);
				cells.push(cell);
			}
			newRows.push(Fragment.from(cells));
		}
		rows = newRows;
		height = newHeight;
	}
	return {
		width,
		height,
		rows
	};
}
function growTable(tr$1, map, table, start, width, height, mapFrom) {
	const schema = tr$1.doc.type.schema;
	const types = tableNodeTypes(schema);
	let empty$1;
	let emptyHead;
	if (width > map.width) for (let row = 0, rowEnd = 0; row < map.height; row++) {
		const rowNode = table.child(row);
		rowEnd += rowNode.nodeSize;
		const cells = [];
		let add;
		if (rowNode.lastChild == null || rowNode.lastChild.type == types.cell) add = empty$1 || (empty$1 = types.cell.createAndFill());
		else add = emptyHead || (emptyHead = types.header_cell.createAndFill());
		for (let i$1 = map.width; i$1 < width; i$1++) cells.push(add);
		tr$1.insert(tr$1.mapping.slice(mapFrom).map(rowEnd - 1 + start), cells);
	}
	if (height > map.height) {
		const cells = [];
		for (let i$1 = 0, start$1 = (map.height - 1) * map.width; i$1 < Math.max(map.width, width); i$1++) {
			const header = i$1 >= map.width ? false : table.nodeAt(map.map[start$1 + i$1]).type == types.header_cell;
			cells.push(header ? emptyHead || (emptyHead = types.header_cell.createAndFill()) : empty$1 || (empty$1 = types.cell.createAndFill()));
		}
		const emptyRow = types.row.create(null, Fragment.from(cells)), rows = [];
		for (let i$1 = map.height; i$1 < height; i$1++) rows.push(emptyRow);
		tr$1.insert(tr$1.mapping.slice(mapFrom).map(start + table.nodeSize - 2), rows);
	}
	return !!(empty$1 || emptyHead);
}
function isolateHorizontal(tr$1, map, table, start, left, right, top, mapFrom) {
	if (top == 0 || top == map.height) return false;
	let found$1 = false;
	for (let col = left; col < right; col++) {
		const index = top * map.width + col, pos = map.map[index];
		if (map.map[index - map.width] == pos) {
			found$1 = true;
			const cell = table.nodeAt(pos);
			const { top: cellTop, left: cellLeft } = map.findCell(pos);
			tr$1.setNodeMarkup(tr$1.mapping.slice(mapFrom).map(pos + start), null, {
				...cell.attrs,
				rowspan: top - cellTop
			});
			tr$1.insert(tr$1.mapping.slice(mapFrom).map(map.positionAt(top, cellLeft, table)), cell.type.createAndFill({
				...cell.attrs,
				rowspan: cellTop + cell.attrs.rowspan - top
			}));
			col += cell.attrs.colspan - 1;
		}
	}
	return found$1;
}
function isolateVertical(tr$1, map, table, start, top, bottom, left, mapFrom) {
	if (left == 0 || left == map.width) return false;
	let found$1 = false;
	for (let row = top; row < bottom; row++) {
		const index = row * map.width + left, pos = map.map[index];
		if (map.map[index - 1] == pos) {
			found$1 = true;
			const cell = table.nodeAt(pos);
			const cellLeft = map.colCount(pos);
			const updatePos = tr$1.mapping.slice(mapFrom).map(pos + start);
			tr$1.setNodeMarkup(updatePos, null, removeColSpan(cell.attrs, left - cellLeft, cell.attrs.colspan - (left - cellLeft)));
			tr$1.insert(updatePos + cell.nodeSize, cell.type.createAndFill(removeColSpan(cell.attrs, 0, left - cellLeft)));
			row += cell.attrs.rowspan - 1;
		}
	}
	return found$1;
}
function insertCells(state, dispatch, tableStart, rect, cells) {
	let table = tableStart ? state.doc.nodeAt(tableStart - 1) : state.doc;
	if (!table) throw new Error("No table found");
	let map = TableMap.get(table);
	const { top, left } = rect;
	const right = left + cells.width, bottom = top + cells.height;
	const tr$1 = state.tr;
	let mapFrom = 0;
	function recomp() {
		table = tableStart ? tr$1.doc.nodeAt(tableStart - 1) : tr$1.doc;
		if (!table) throw new Error("No table found");
		map = TableMap.get(table);
		mapFrom = tr$1.mapping.maps.length;
	}
	if (growTable(tr$1, map, table, tableStart, right, bottom, mapFrom)) recomp();
	if (isolateHorizontal(tr$1, map, table, tableStart, left, right, top, mapFrom)) recomp();
	if (isolateHorizontal(tr$1, map, table, tableStart, left, right, bottom, mapFrom)) recomp();
	if (isolateVertical(tr$1, map, table, tableStart, top, bottom, left, mapFrom)) recomp();
	if (isolateVertical(tr$1, map, table, tableStart, top, bottom, right, mapFrom)) recomp();
	for (let row = top; row < bottom; row++) {
		const from = map.positionAt(row, left, table), to = map.positionAt(row, right, table);
		tr$1.replace(tr$1.mapping.slice(mapFrom).map(from + tableStart), tr$1.mapping.slice(mapFrom).map(to + tableStart), new Slice(cells.rows[row - top], 0, 0));
	}
	recomp();
	tr$1.setSelection(new CellSelection(tr$1.doc.resolve(tableStart + map.positionAt(top, left, table)), tr$1.doc.resolve(tableStart + map.positionAt(bottom - 1, right - 1, table))));
	dispatch(tr$1);
}
var handleKeyDown = keydownHandler({
	ArrowLeft: arrow("horiz", -1),
	ArrowRight: arrow("horiz", 1),
	ArrowUp: arrow("vert", -1),
	ArrowDown: arrow("vert", 1),
	"Shift-ArrowLeft": shiftArrow("horiz", -1),
	"Shift-ArrowRight": shiftArrow("horiz", 1),
	"Shift-ArrowUp": shiftArrow("vert", -1),
	"Shift-ArrowDown": shiftArrow("vert", 1),
	Backspace: deleteCellSelection,
	"Mod-Backspace": deleteCellSelection,
	Delete: deleteCellSelection,
	"Mod-Delete": deleteCellSelection
});
function maybeSetSelection(state, dispatch, selection) {
	if (selection.eq(state.selection)) return false;
	if (dispatch) dispatch(state.tr.setSelection(selection).scrollIntoView());
	return true;
}
function arrow(axis, dir) {
	return (state, dispatch, view) => {
		if (!view) return false;
		const sel = state.selection;
		if (sel instanceof CellSelection) return maybeSetSelection(state, dispatch, Selection.near(sel.$headCell, dir));
		if (axis != "horiz" && !sel.empty) return false;
		const end = atEndOfCell(view, axis, dir);
		if (end == null) return false;
		if (axis == "horiz") return maybeSetSelection(state, dispatch, Selection.near(state.doc.resolve(sel.head + dir), dir));
		else {
			const $cell = state.doc.resolve(end);
			const $next = nextCell($cell, axis, dir);
			let newSel;
			if ($next) newSel = Selection.near($next, 1);
			else if (dir < 0) newSel = Selection.near(state.doc.resolve($cell.before(-1)), -1);
			else newSel = Selection.near(state.doc.resolve($cell.after(-1)), 1);
			return maybeSetSelection(state, dispatch, newSel);
		}
	};
}
function shiftArrow(axis, dir) {
	return (state, dispatch, view) => {
		if (!view) return false;
		const sel = state.selection;
		let cellSel;
		if (sel instanceof CellSelection) cellSel = sel;
		else {
			const end = atEndOfCell(view, axis, dir);
			if (end == null) return false;
			cellSel = new CellSelection(state.doc.resolve(end));
		}
		const $head = nextCell(cellSel.$headCell, axis, dir);
		if (!$head) return false;
		return maybeSetSelection(state, dispatch, new CellSelection(cellSel.$anchorCell, $head));
	};
}
function handleTripleClick(view, pos) {
	const doc$2 = view.state.doc, $cell = cellAround(doc$2.resolve(pos));
	if (!$cell) return false;
	view.dispatch(view.state.tr.setSelection(new CellSelection($cell)));
	return true;
}
function handlePaste(view, _$1, slice) {
	if (!isInTable(view.state)) return false;
	let cells = pastedCells(slice);
	const sel = view.state.selection;
	if (sel instanceof CellSelection) {
		if (!cells) cells = {
			width: 1,
			height: 1,
			rows: [Fragment.from(fitSlice(tableNodeTypes(view.state.schema).cell, slice))]
		};
		const table = sel.$anchorCell.node(-1);
		const start = sel.$anchorCell.start(-1);
		const rect = TableMap.get(table).rectBetween(sel.$anchorCell.pos - start, sel.$headCell.pos - start);
		cells = clipCells(cells, rect.right - rect.left, rect.bottom - rect.top);
		insertCells(view.state, view.dispatch, start, rect, cells);
		return true;
	} else if (cells) {
		const $cell = selectionCell(view.state);
		const start = $cell.start(-1);
		insertCells(view.state, view.dispatch, start, TableMap.get($cell.node(-1)).findCell($cell.pos - start), cells);
		return true;
	} else return false;
}
function handleMouseDown$1(view, startEvent) {
	var _cellUnderMouse;
	if (startEvent.button != 0) return;
	if (startEvent.ctrlKey || startEvent.metaKey) return;
	const startDOMCell = domInCell(view, startEvent.target);
	let $anchor;
	if (startEvent.shiftKey && view.state.selection instanceof CellSelection) {
		setCellSelection(view.state.selection.$anchorCell, startEvent);
		startEvent.preventDefault();
	} else if (startEvent.shiftKey && startDOMCell && ($anchor = cellAround(view.state.selection.$anchor)) != null && ((_cellUnderMouse = cellUnderMouse(view, startEvent)) === null || _cellUnderMouse === void 0 ? void 0 : _cellUnderMouse.pos) != $anchor.pos) {
		setCellSelection($anchor, startEvent);
		startEvent.preventDefault();
	} else if (!startDOMCell) return;
	function setCellSelection($anchor$1, event) {
		let $head = cellUnderMouse(view, event);
		const starting = tableEditingKey.getState(view.state) == null;
		if (!$head || !inSameTable($anchor$1, $head)) if (starting) $head = $anchor$1;
		else return;
		const selection = new CellSelection($anchor$1, $head);
		if (starting || !view.state.selection.eq(selection)) {
			const tr$1 = view.state.tr.setSelection(selection);
			if (starting) tr$1.setMeta(tableEditingKey, $anchor$1.pos);
			view.dispatch(tr$1);
		}
	}
	function stop() {
		view.root.removeEventListener("mouseup", stop);
		view.root.removeEventListener("dragstart", stop);
		view.root.removeEventListener("mousemove", move);
		if (tableEditingKey.getState(view.state) != null) view.dispatch(view.state.tr.setMeta(tableEditingKey, -1));
	}
	function move(_event) {
		const event = _event;
		const anchor = tableEditingKey.getState(view.state);
		let $anchor$1;
		if (anchor != null) $anchor$1 = view.state.doc.resolve(anchor);
		else if (domInCell(view, event.target) != startDOMCell) {
			$anchor$1 = cellUnderMouse(view, startEvent);
			if (!$anchor$1) return stop();
		}
		if ($anchor$1) setCellSelection($anchor$1, event);
	}
	view.root.addEventListener("mouseup", stop);
	view.root.addEventListener("dragstart", stop);
	view.root.addEventListener("mousemove", move);
}
function atEndOfCell(view, axis, dir) {
	if (!(view.state.selection instanceof TextSelection)) return null;
	const { $head } = view.state.selection;
	for (let d = $head.depth - 1; d >= 0; d--) {
		const parent = $head.node(d);
		if ((dir < 0 ? $head.index(d) : $head.indexAfter(d)) != (dir < 0 ? 0 : parent.childCount)) return null;
		if (parent.type.spec.tableRole == "cell" || parent.type.spec.tableRole == "header_cell") {
			const cellPos = $head.before(d);
			const dirStr = axis == "vert" ? dir > 0 ? "down" : "up" : dir > 0 ? "right" : "left";
			return view.endOfTextblock(dirStr) ? cellPos : null;
		}
	}
	return null;
}
function domInCell(view, dom) {
	for (; dom && dom != view.dom; dom = dom.parentNode) if (dom.nodeName == "TD" || dom.nodeName == "TH") return dom;
	return null;
}
function cellUnderMouse(view, event) {
	const mousePos = view.posAtCoords({
		left: event.clientX,
		top: event.clientY
	});
	if (!mousePos) return null;
	let { inside, pos } = mousePos;
	return inside >= 0 && cellAround(view.state.doc.resolve(inside)) || cellAround(view.state.doc.resolve(pos));
}
var TableView$1 = class {
	constructor(node, defaultCellMinWidth) {
		this.node = node;
		this.defaultCellMinWidth = defaultCellMinWidth;
		this.dom = document.createElement("div");
		this.dom.className = "tableWrapper";
		this.table = this.dom.appendChild(document.createElement("table"));
		this.table.style.setProperty("--default-cell-min-width", `${defaultCellMinWidth}px`);
		this.colgroup = this.table.appendChild(document.createElement("colgroup"));
		updateColumnsOnResize(node, this.colgroup, this.table, defaultCellMinWidth);
		this.contentDOM = this.table.appendChild(document.createElement("tbody"));
	}
	update(node) {
		if (node.type != this.node.type) return false;
		this.node = node;
		updateColumnsOnResize(node, this.colgroup, this.table, this.defaultCellMinWidth);
		return true;
	}
	ignoreMutation(record) {
		return record.type == "attributes" && (record.target == this.table || this.colgroup.contains(record.target));
	}
};
function updateColumnsOnResize(node, colgroup, table, defaultCellMinWidth, overrideCol, overrideValue) {
	let totalWidth = 0;
	let fixedWidth = true;
	let nextDOM = colgroup.firstChild;
	const row = node.firstChild;
	if (!row) return;
	for (let i$1 = 0, col = 0; i$1 < row.childCount; i$1++) {
		const { colspan, colwidth } = row.child(i$1).attrs;
		for (let j$1 = 0; j$1 < colspan; j$1++, col++) {
			const hasWidth = overrideCol == col ? overrideValue : colwidth && colwidth[j$1];
			const cssWidth = hasWidth ? hasWidth + "px" : "";
			totalWidth += hasWidth || defaultCellMinWidth;
			if (!hasWidth) fixedWidth = false;
			if (!nextDOM) {
				const col$1 = document.createElement("col");
				col$1.style.width = cssWidth;
				colgroup.appendChild(col$1);
			} else {
				if (nextDOM.style.width != cssWidth) nextDOM.style.width = cssWidth;
				nextDOM = nextDOM.nextSibling;
			}
		}
	}
	while (nextDOM) {
		var _nextDOM$parentNode;
		const after = nextDOM.nextSibling;
		(_nextDOM$parentNode = nextDOM.parentNode) === null || _nextDOM$parentNode === void 0 || _nextDOM$parentNode.removeChild(nextDOM);
		nextDOM = after;
	}
	if (fixedWidth) {
		table.style.width = totalWidth + "px";
		table.style.minWidth = "";
	} else {
		table.style.width = "";
		table.style.minWidth = totalWidth + "px";
	}
}
var columnResizingPluginKey = new PluginKey("tableColumnResizing");
function columnResizing({ handleWidth = 5, cellMinWidth = 25, defaultCellMinWidth = 100, View = TableView$1, lastColumnResizable = true } = {}) {
	const plugin = new Plugin({
		key: columnResizingPluginKey,
		state: {
			init(_$1, state) {
				var _plugin$spec;
				const nodeViews = (_plugin$spec = plugin.spec) === null || _plugin$spec === void 0 || (_plugin$spec = _plugin$spec.props) === null || _plugin$spec === void 0 ? void 0 : _plugin$spec.nodeViews;
				const tableName = tableNodeTypes(state.schema).table.name;
				if (View && nodeViews) nodeViews[tableName] = (node, view) => {
					return new View(node, defaultCellMinWidth, view);
				};
				return new ResizeState(-1, false);
			},
			apply(tr$1, prev) {
				return prev.apply(tr$1);
			}
		},
		props: {
			attributes: (state) => {
				const pluginState = columnResizingPluginKey.getState(state);
				return pluginState && pluginState.activeHandle > -1 ? { class: "resize-cursor" } : {};
			},
			handleDOMEvents: {
				mousemove: (view, event) => {
					handleMouseMove(view, event, handleWidth, lastColumnResizable);
				},
				mouseleave: (view) => {
					handleMouseLeave(view);
				},
				mousedown: (view, event) => {
					handleMouseDown(view, event, cellMinWidth, defaultCellMinWidth);
				}
			},
			decorations: (state) => {
				const pluginState = columnResizingPluginKey.getState(state);
				if (pluginState && pluginState.activeHandle > -1) return handleDecorations(state, pluginState.activeHandle);
			},
			nodeViews: {}
		}
	});
	return plugin;
}
var ResizeState = class ResizeState$1 {
	constructor(activeHandle, dragging) {
		this.activeHandle = activeHandle;
		this.dragging = dragging;
	}
	apply(tr$1) {
		const state = this;
		const action = tr$1.getMeta(columnResizingPluginKey);
		if (action && action.setHandle != null) return new ResizeState$1(action.setHandle, false);
		if (action && action.setDragging !== void 0) return new ResizeState$1(state.activeHandle, action.setDragging);
		if (state.activeHandle > -1 && tr$1.docChanged) {
			let handle = tr$1.mapping.map(state.activeHandle, -1);
			if (!pointsAtCell(tr$1.doc.resolve(handle))) handle = -1;
			return new ResizeState$1(handle, state.dragging);
		}
		return state;
	}
};
function handleMouseMove(view, event, handleWidth, lastColumnResizable) {
	if (!view.editable) return;
	const pluginState = columnResizingPluginKey.getState(view.state);
	if (!pluginState) return;
	if (!pluginState.dragging) {
		const target = domCellAround(event.target);
		let cell = -1;
		if (target) {
			const { left, right } = target.getBoundingClientRect();
			if (event.clientX - left <= handleWidth) cell = edgeCell(view, event, "left", handleWidth);
			else if (right - event.clientX <= handleWidth) cell = edgeCell(view, event, "right", handleWidth);
		}
		if (cell != pluginState.activeHandle) {
			if (!lastColumnResizable && cell !== -1) {
				const $cell = view.state.doc.resolve(cell);
				const table = $cell.node(-1);
				const map = TableMap.get(table);
				const tableStart = $cell.start(-1);
				if (map.colCount($cell.pos - tableStart) + $cell.nodeAfter.attrs.colspan - 1 == map.width - 1) return;
			}
			updateHandle(view, cell);
		}
	}
}
function handleMouseLeave(view) {
	if (!view.editable) return;
	const pluginState = columnResizingPluginKey.getState(view.state);
	if (pluginState && pluginState.activeHandle > -1 && !pluginState.dragging) updateHandle(view, -1);
}
function handleMouseDown(view, event, cellMinWidth, defaultCellMinWidth) {
	var _view$dom$ownerDocume;
	if (!view.editable) return false;
	const win = (_view$dom$ownerDocume = view.dom.ownerDocument.defaultView) !== null && _view$dom$ownerDocume !== void 0 ? _view$dom$ownerDocume : window;
	const pluginState = columnResizingPluginKey.getState(view.state);
	if (!pluginState || pluginState.activeHandle == -1 || pluginState.dragging) return false;
	const cell = view.state.doc.nodeAt(pluginState.activeHandle);
	const width = currentColWidth(view, pluginState.activeHandle, cell.attrs);
	view.dispatch(view.state.tr.setMeta(columnResizingPluginKey, { setDragging: {
		startX: event.clientX,
		startWidth: width
	} }));
	function finish(event$1) {
		win.removeEventListener("mouseup", finish);
		win.removeEventListener("mousemove", move);
		const pluginState$1 = columnResizingPluginKey.getState(view.state);
		if (pluginState$1 === null || pluginState$1 === void 0 ? void 0 : pluginState$1.dragging) {
			updateColumnWidth(view, pluginState$1.activeHandle, draggedWidth(pluginState$1.dragging, event$1, cellMinWidth));
			view.dispatch(view.state.tr.setMeta(columnResizingPluginKey, { setDragging: null }));
		}
	}
	function move(event$1) {
		if (!event$1.which) return finish(event$1);
		const pluginState$1 = columnResizingPluginKey.getState(view.state);
		if (!pluginState$1) return;
		if (pluginState$1.dragging) {
			const dragged = draggedWidth(pluginState$1.dragging, event$1, cellMinWidth);
			displayColumnWidth(view, pluginState$1.activeHandle, dragged, defaultCellMinWidth);
		}
	}
	displayColumnWidth(view, pluginState.activeHandle, width, defaultCellMinWidth);
	win.addEventListener("mouseup", finish);
	win.addEventListener("mousemove", move);
	event.preventDefault();
	return true;
}
function currentColWidth(view, cellPos, { colspan, colwidth }) {
	const width = colwidth && colwidth[colwidth.length - 1];
	if (width) return width;
	const dom = view.domAtPos(cellPos);
	let domWidth = dom.node.childNodes[dom.offset].offsetWidth, parts = colspan;
	if (colwidth) {
		for (let i$1 = 0; i$1 < colspan; i$1++) if (colwidth[i$1]) {
			domWidth -= colwidth[i$1];
			parts--;
		}
	}
	return domWidth / parts;
}
function domCellAround(target) {
	while (target && target.nodeName != "TD" && target.nodeName != "TH") target = target.classList && target.classList.contains("ProseMirror") ? null : target.parentNode;
	return target;
}
function edgeCell(view, event, side, handleWidth) {
	const offset = side == "right" ? -handleWidth : handleWidth;
	const found$1 = view.posAtCoords({
		left: event.clientX + offset,
		top: event.clientY
	});
	if (!found$1) return -1;
	const { pos } = found$1;
	const $cell = cellAround(view.state.doc.resolve(pos));
	if (!$cell) return -1;
	if (side == "right") return $cell.pos;
	const map = TableMap.get($cell.node(-1)), start = $cell.start(-1);
	const index = map.map.indexOf($cell.pos - start);
	return index % map.width == 0 ? -1 : start + map.map[index - 1];
}
function draggedWidth(dragging, event, resizeMinWidth) {
	const offset = event.clientX - dragging.startX;
	return Math.max(resizeMinWidth, dragging.startWidth + offset);
}
function updateHandle(view, value) {
	view.dispatch(view.state.tr.setMeta(columnResizingPluginKey, { setHandle: value }));
}
function updateColumnWidth(view, cell, width) {
	const $cell = view.state.doc.resolve(cell);
	const table = $cell.node(-1), map = TableMap.get(table), start = $cell.start(-1);
	const col = map.colCount($cell.pos - start) + $cell.nodeAfter.attrs.colspan - 1;
	const tr$1 = view.state.tr;
	for (let row = 0; row < map.height; row++) {
		const mapIndex = row * map.width + col;
		if (row && map.map[mapIndex] == map.map[mapIndex - map.width]) continue;
		const pos = map.map[mapIndex];
		const attrs = table.nodeAt(pos).attrs;
		const index = attrs.colspan == 1 ? 0 : col - map.colCount(pos);
		if (attrs.colwidth && attrs.colwidth[index] == width) continue;
		const colwidth = attrs.colwidth ? attrs.colwidth.slice() : zeroes(attrs.colspan);
		colwidth[index] = width;
		tr$1.setNodeMarkup(start + pos, null, {
			...attrs,
			colwidth
		});
	}
	if (tr$1.docChanged) view.dispatch(tr$1);
}
function displayColumnWidth(view, cell, width, defaultCellMinWidth) {
	const $cell = view.state.doc.resolve(cell);
	const table = $cell.node(-1), start = $cell.start(-1);
	const col = TableMap.get(table).colCount($cell.pos - start) + $cell.nodeAfter.attrs.colspan - 1;
	let dom = view.domAtPos($cell.start(-1)).node;
	while (dom && dom.nodeName != "TABLE") dom = dom.parentNode;
	if (!dom) return;
	updateColumnsOnResize(table, dom.firstChild, dom, defaultCellMinWidth, col, width);
}
function zeroes(n) {
	return Array(n).fill(0);
}
function handleDecorations(state, cell) {
	const decorations = [];
	const $cell = state.doc.resolve(cell);
	const table = $cell.node(-1);
	if (!table) return DecorationSet.empty;
	const map = TableMap.get(table);
	const start = $cell.start(-1);
	const col = map.colCount($cell.pos - start) + $cell.nodeAfter.attrs.colspan - 1;
	for (let row = 0; row < map.height; row++) {
		const index = col + row * map.width;
		if ((col == map.width - 1 || map.map[index] != map.map[index + 1]) && (row == 0 || map.map[index] != map.map[index - map.width])) {
			var _columnResizingPlugin;
			const cellPos = map.map[index];
			const pos = start + cellPos + table.nodeAt(cellPos).nodeSize - 1;
			const dom = document.createElement("div");
			dom.className = "column-resize-handle";
			if ((_columnResizingPlugin = columnResizingPluginKey.getState(state)) === null || _columnResizingPlugin === void 0 ? void 0 : _columnResizingPlugin.dragging) decorations.push(Decoration.node(start + cellPos, start + cellPos + table.nodeAt(cellPos).nodeSize, { class: "column-resize-dragging" }));
			decorations.push(Decoration.widget(pos, dom));
		}
	}
	return DecorationSet.create(state.doc, decorations);
}
function tableEditing({ allowTableNodeSelection = false } = {}) {
	return new Plugin({
		key: tableEditingKey,
		state: {
			init() {
				return null;
			},
			apply(tr$1, cur) {
				const set = tr$1.getMeta(tableEditingKey);
				if (set != null) return set == -1 ? null : set;
				if (cur == null || !tr$1.docChanged) return cur;
				const { deleted, pos } = tr$1.mapping.mapResult(cur);
				return deleted ? null : pos;
			}
		},
		props: {
			decorations: drawCellSelection,
			handleDOMEvents: { mousedown: handleMouseDown$1 },
			createSelectionBetween(view) {
				return tableEditingKey.getState(view.state) != null ? view.state.selection : null;
			},
			handleTripleClick,
			handleKeyDown,
			handlePaste
		},
		appendTransaction(_$1, oldState, state) {
			return normalizeSelection(state, fixTables(state, oldState), allowTableNodeSelection);
		}
	});
}
function normalizeTableCellAlign(value) {
	if (value === "left" || value === "right" || value === "center") return value;
	return null;
}
function parseAlign(element) {
	const styleAlign = (element.style.textAlign || "").trim().toLowerCase();
	const attrAlign = (element.getAttribute("align") || "").trim().toLowerCase();
	return normalizeTableCellAlign(styleAlign || attrAlign);
}
function normalizeTableCellAlignFromAttributes(attributes) {
	return normalizeTableCellAlign(attributes == null ? void 0 : attributes.align);
}
function createAlignAttribute() {
	return {
		default: null,
		parseHTML: (element) => parseAlign(element),
		renderHTML: (attributes) => {
			if (!attributes.align) return {};
			return { style: `text-align: ${attributes.align}` };
		}
	};
}
var TableCell = Node3.create({
	name: "tableCell",
	addOptions() {
		return { HTMLAttributes: {} };
	},
	content: "block+",
	addAttributes() {
		return {
			colspan: { default: 1 },
			rowspan: { default: 1 },
			colwidth: {
				default: null,
				parseHTML: (element) => {
					var _a, _b;
					const colwidth = element.getAttribute("colwidth");
					const value = colwidth ? colwidth.split(",").map((width) => parseInt(width, 10)) : null;
					if (!value) {
						const cols = (_a = element.closest("table")) == null ? void 0 : _a.querySelectorAll("colgroup > col");
						const cellIndex = Array.from(((_b = element.parentElement) == null ? void 0 : _b.children) || []).indexOf(element);
						if (cellIndex && cellIndex > -1 && cols && cols[cellIndex]) {
							const colWidth = cols[cellIndex].getAttribute("width");
							return colWidth ? [parseInt(colWidth, 10)] : null;
						}
					}
					return value;
				}
			},
			align: createAlignAttribute()
		};
	},
	tableRole: "cell",
	isolating: true,
	parseHTML() {
		return [{ tag: "td" }];
	},
	renderHTML({ HTMLAttributes }) {
		return [
			"td",
			mergeAttributes(this.options.HTMLAttributes, HTMLAttributes),
			0
		];
	}
});
var TableHeader = Node3.create({
	name: "tableHeader",
	addOptions() {
		return { HTMLAttributes: {} };
	},
	content: "block+",
	addAttributes() {
		return {
			colspan: { default: 1 },
			rowspan: { default: 1 },
			colwidth: {
				default: null,
				parseHTML: (element) => {
					const colwidth = element.getAttribute("colwidth");
					return colwidth ? colwidth.split(",").map((width) => parseInt(width, 10)) : null;
				}
			},
			align: createAlignAttribute()
		};
	},
	tableRole: "header_cell",
	isolating: true,
	parseHTML() {
		return [{ tag: "th" }];
	},
	renderHTML({ HTMLAttributes }) {
		return [
			"th",
			mergeAttributes(this.options.HTMLAttributes, HTMLAttributes),
			0
		];
	}
});
var TableRow = Node3.create({
	name: "tableRow",
	addOptions() {
		return { HTMLAttributes: {} };
	},
	content: "(tableCell | tableHeader)*",
	tableRole: "row",
	parseHTML() {
		return [{ tag: "tr" }];
	},
	renderHTML({ HTMLAttributes }) {
		return [
			"tr",
			mergeAttributes(this.options.HTMLAttributes, HTMLAttributes),
			0
		];
	}
});
function getColStyleDeclaration(minWidth, width) {
	if (width) return ["width", `${Math.max(width, minWidth)}px`];
	return ["min-width", `${minWidth}px`];
}
function updateColumns(node, colgroup, table, cellMinWidth, overrideCol, overrideValue) {
	var _a;
	let totalWidth = 0;
	let fixedWidth = true;
	let nextDOM = colgroup.firstChild;
	const row = node.firstChild;
	if (row !== null) for (let i$1 = 0, col = 0; i$1 < row.childCount; i$1 += 1) {
		const { colspan, colwidth } = row.child(i$1).attrs;
		for (let j$1 = 0; j$1 < colspan; j$1 += 1, col += 1) {
			const hasWidth = overrideCol === col ? overrideValue : colwidth && colwidth[j$1];
			const cssWidth = hasWidth ? `${hasWidth}px` : "";
			totalWidth += hasWidth || cellMinWidth;
			if (!hasWidth) fixedWidth = false;
			if (!nextDOM) {
				const colElement = document.createElement("col");
				const [propertyKey, propertyValue] = getColStyleDeclaration(cellMinWidth, hasWidth);
				colElement.style.setProperty(propertyKey, propertyValue);
				colgroup.appendChild(colElement);
			} else {
				if (nextDOM.style.width !== cssWidth) {
					const [propertyKey, propertyValue] = getColStyleDeclaration(cellMinWidth, hasWidth);
					nextDOM.style.setProperty(propertyKey, propertyValue);
				}
				nextDOM = nextDOM.nextSibling;
			}
		}
	}
	while (nextDOM) {
		const after = nextDOM.nextSibling;
		(_a = nextDOM.parentNode) == null || _a.removeChild(nextDOM);
		nextDOM = after;
	}
	const hasUserWidth = node.attrs.style && typeof node.attrs.style === "string" && /\bwidth\s*:/i.test(node.attrs.style);
	if (fixedWidth && !hasUserWidth) {
		table.style.width = `${totalWidth}px`;
		table.style.minWidth = "";
	} else {
		table.style.width = "";
		table.style.minWidth = `${totalWidth}px`;
	}
}
var TableView = class {
	constructor(node, cellMinWidth) {
		this.node = node;
		this.cellMinWidth = cellMinWidth;
		this.dom = document.createElement("div");
		this.dom.className = "tableWrapper";
		this.table = this.dom.appendChild(document.createElement("table"));
		if (node.attrs.style) this.table.style.cssText = node.attrs.style;
		this.colgroup = this.table.appendChild(document.createElement("colgroup"));
		updateColumns(node, this.colgroup, this.table, cellMinWidth);
		this.contentDOM = this.table.appendChild(document.createElement("tbody"));
	}
	update(node) {
		if (node.type !== this.node.type) return false;
		this.node = node;
		updateColumns(node, this.colgroup, this.table, this.cellMinWidth);
		return true;
	}
	ignoreMutation(mutation) {
		const target = mutation.target;
		const isInsideWrapper = this.dom.contains(target);
		const isInsideContent = this.contentDOM.contains(target);
		if (isInsideWrapper && !isInsideContent) {
			if (mutation.type === "attributes" || mutation.type === "childList" || mutation.type === "characterData") return true;
		}
		return false;
	}
};
function createColGroup(node, cellMinWidth, overrideCol, overrideValue) {
	let totalWidth = 0;
	let fixedWidth = true;
	const cols = [];
	const row = node.firstChild;
	if (!row) return {};
	for (let i$1 = 0, col = 0; i$1 < row.childCount; i$1 += 1) {
		const { colspan, colwidth } = row.child(i$1).attrs;
		for (let j$1 = 0; j$1 < colspan; j$1 += 1, col += 1) {
			const hasWidth = overrideCol === col ? overrideValue : colwidth && colwidth[j$1];
			totalWidth += hasWidth || cellMinWidth;
			if (!hasWidth) fixedWidth = false;
			const [property, value] = getColStyleDeclaration(cellMinWidth, hasWidth);
			cols.push(["col", { style: `${property}: ${value}` }]);
		}
	}
	const tableWidth = fixedWidth ? `${totalWidth}px` : "";
	const tableMinWidth = fixedWidth ? "" : `${totalWidth}px`;
	return {
		colgroup: [
			"colgroup",
			{},
			...cols
		],
		tableWidth,
		tableMinWidth
	};
}
function createCell(cellType, cellContent) {
	if (cellContent) return cellType.createChecked(null, cellContent);
	return cellType.createAndFill();
}
function getTableNodeTypes(schema) {
	if (schema.cached.tableNodeTypes) return schema.cached.tableNodeTypes;
	const roles = {};
	Object.keys(schema.nodes).forEach((type) => {
		const nodeType = schema.nodes[type];
		if (nodeType.spec.tableRole) roles[nodeType.spec.tableRole] = nodeType;
	});
	schema.cached.tableNodeTypes = roles;
	return roles;
}
function createTable(schema, rowsCount, colsCount, withHeaderRow, cellContent) {
	const types = getTableNodeTypes(schema);
	const headerCells = [];
	const cells = [];
	for (let index = 0; index < colsCount; index += 1) {
		const cell = createCell(types.cell, cellContent);
		if (cell) cells.push(cell);
		if (withHeaderRow) {
			const headerCell = createCell(types.header_cell, cellContent);
			if (headerCell) headerCells.push(headerCell);
		}
	}
	const rows = [];
	for (let index = 0; index < rowsCount; index += 1) rows.push(types.row.createChecked(null, withHeaderRow && index === 0 ? headerCells : cells));
	return types.table.createChecked(null, rows);
}
function isCellSelection(value) {
	return value instanceof CellSelection;
}
var deleteTableWhenAllCellsSelected = ({ editor }) => {
	const { selection } = editor.state;
	if (!isCellSelection(selection)) return false;
	let cellCount = 0;
	findParentNodeClosestToPos(selection.ranges[0].$from, (node) => {
		return node.type.name === "table";
	})?.node.descendants((node) => {
		if (node.type.name === "table") return false;
		if (["tableCell", "tableHeader"].includes(node.type.name)) cellCount += 1;
	});
	if (!(cellCount === selection.ranges.length)) return false;
	editor.commands.deleteTable();
	return true;
};
function collapseWhitespace(s) {
	return (s || "").replace(/\s+/g, " ").trim();
}
function renderTableToMarkdown(node, h$1, options = {}) {
	var _a;
	const cellSep = (_a = options.cellLineSeparator) != null ? _a : "";
	if (!node || !node.content || node.content.length === 0) return "";
	const rows = [];
	node.content.forEach((rowNode) => {
		const cells = [];
		if (rowNode.content) rowNode.content.forEach((cellNode) => {
			let raw = "";
			if (cellNode.content && Array.isArray(cellNode.content) && cellNode.content.length > 1) raw = cellNode.content.map((child) => h$1.renderChildren(child)).join(cellSep);
			else raw = cellNode.content ? h$1.renderChildren(cellNode.content) : "";
			const text = collapseWhitespace(raw);
			const isHeader = cellNode.type === "tableHeader";
			const align = normalizeTableCellAlignFromAttributes(cellNode.attrs);
			cells.push({
				text,
				isHeader,
				align
			});
		});
		rows.push(cells);
	});
	const columnCount = rows.reduce((max, r) => Math.max(max, r.length), 0);
	if (columnCount === 0) return "";
	const colWidths = new Array(columnCount).fill(0);
	rows.forEach((r) => {
		var _a2;
		for (let i$1 = 0; i$1 < columnCount; i$1 += 1) {
			const len = (((_a2 = r[i$1]) == null ? void 0 : _a2.text) || "").length;
			if (len > colWidths[i$1]) colWidths[i$1] = len;
			if (colWidths[i$1] < 3) colWidths[i$1] = 3;
		}
	});
	const pad = (s, width) => s + " ".repeat(Math.max(0, width - s.length));
	const headerRow = rows[0];
	const hasHeader = headerRow.some((c) => c.isHeader);
	const colAlignments = new Array(columnCount).fill(null);
	rows.forEach((r) => {
		var _a2;
		for (let i$1 = 0; i$1 < columnCount; i$1 += 1) if (!colAlignments[i$1] && ((_a2 = r[i$1]) == null ? void 0 : _a2.align)) colAlignments[i$1] = r[i$1].align;
	});
	let out = "\n";
	const headerTexts = new Array(columnCount).fill(0).map((_$1, i$1) => hasHeader ? headerRow[i$1] && headerRow[i$1].text || "" : "");
	out += `| ${headerTexts.map((t, i$1) => pad(t, colWidths[i$1])).join(" | ")} |
`;
	out += `| ${colWidths.map((w$1, index) => {
		const dashCount = Math.max(3, w$1);
		const alignment = colAlignments[index];
		if (alignment === "left") return `:${"-".repeat(dashCount)}`;
		if (alignment === "right") return `${"-".repeat(dashCount)}:`;
		if (alignment === "center") return `:${"-".repeat(dashCount)}:`;
		return "-".repeat(dashCount);
	}).join(" | ")} |
`;
	(hasHeader ? rows.slice(1) : rows).forEach((r) => {
		out += `| ${new Array(columnCount).fill(0).map((_$1, i$1) => pad(r[i$1] && r[i$1].text || "", colWidths[i$1])).join(" | ")} |
`;
	});
	return out;
}
var markdown_default = renderTableToMarkdown;
var Table = Node3.create({
	name: "table",
	addOptions() {
		return {
			HTMLAttributes: {},
			resizable: false,
			renderWrapper: false,
			handleWidth: 5,
			cellMinWidth: 25,
			View: TableView,
			lastColumnResizable: true,
			allowTableNodeSelection: false
		};
	},
	content: "tableRow+",
	tableRole: "table",
	isolating: true,
	group: "block",
	parseHTML() {
		return [{ tag: "table" }];
	},
	renderHTML({ node, HTMLAttributes }) {
		const { colgroup, tableWidth, tableMinWidth } = createColGroup(node, this.options.cellMinWidth);
		const userStyles = HTMLAttributes.style;
		function getTableStyle() {
			if (userStyles) return userStyles;
			return tableWidth ? `width: ${tableWidth}` : `min-width: ${tableMinWidth}`;
		}
		const table = [
			"table",
			mergeAttributes(this.options.HTMLAttributes, HTMLAttributes, { style: getTableStyle() }),
			colgroup,
			["tbody", 0]
		];
		return this.options.renderWrapper ? [
			"div",
			{ class: "tableWrapper" },
			table
		] : table;
	},
	parseMarkdown: (token, h$1) => {
		const rows = [];
		const alignments = Array.isArray(token.align) ? token.align : [];
		if (token.header) {
			const headerCells = [];
			token.header.forEach((cell, index) => {
				var _a;
				const align = normalizeTableCellAlign((_a = alignments[index]) != null ? _a : cell.align);
				const attrs = align ? { align } : {};
				headerCells.push(h$1.createNode("tableHeader", attrs, [{
					type: "paragraph",
					content: h$1.parseInline(cell.tokens)
				}]));
			});
			rows.push(h$1.createNode("tableRow", {}, headerCells));
		}
		if (token.rows) token.rows.forEach((row) => {
			const bodyCells = [];
			row.forEach((cell, index) => {
				var _a;
				const align = normalizeTableCellAlign((_a = alignments[index]) != null ? _a : cell.align);
				const attrs = align ? { align } : {};
				bodyCells.push(h$1.createNode("tableCell", attrs, [{
					type: "paragraph",
					content: h$1.parseInline(cell.tokens)
				}]));
			});
			rows.push(h$1.createNode("tableRow", {}, bodyCells));
		});
		return h$1.createNode("table", void 0, rows);
	},
	renderMarkdown: (node, h$1) => {
		return markdown_default(node, h$1);
	},
	addCommands() {
		return {
			insertTable: ({ rows = 3, cols = 3, withHeaderRow = true } = {}) => ({ tr: tr$1, dispatch, editor }) => {
				const node = createTable(editor.schema, rows, cols, withHeaderRow);
				if (dispatch) {
					const offset = tr$1.selection.from + 1;
					tr$1.replaceSelectionWith(node).scrollIntoView().setSelection(TextSelection.near(tr$1.doc.resolve(offset)));
				}
				return true;
			},
			addColumnBefore: () => ({ state, dispatch }) => {
				return addColumnBefore(state, dispatch);
			},
			addColumnAfter: () => ({ state, dispatch }) => {
				return addColumnAfter(state, dispatch);
			},
			deleteColumn: () => ({ state, dispatch }) => {
				return deleteColumn(state, dispatch);
			},
			addRowBefore: () => ({ state, dispatch }) => {
				return addRowBefore(state, dispatch);
			},
			addRowAfter: () => ({ state, dispatch }) => {
				return addRowAfter(state, dispatch);
			},
			deleteRow: () => ({ state, dispatch }) => {
				return deleteRow(state, dispatch);
			},
			deleteTable: () => ({ state, dispatch }) => {
				return deleteTable(state, dispatch);
			},
			mergeCells: () => ({ state, dispatch }) => {
				return mergeCells(state, dispatch);
			},
			splitCell: () => ({ state, dispatch }) => {
				return splitCell(state, dispatch);
			},
			toggleHeaderColumn: () => ({ state, dispatch }) => {
				return toggleHeader("column")(state, dispatch);
			},
			toggleHeaderRow: () => ({ state, dispatch }) => {
				return toggleHeader("row")(state, dispatch);
			},
			toggleHeaderCell: () => ({ state, dispatch }) => {
				return toggleHeaderCell(state, dispatch);
			},
			mergeOrSplit: () => ({ state, dispatch }) => {
				if (mergeCells(state, dispatch)) return true;
				return splitCell(state, dispatch);
			},
			setCellAttribute: (name, value) => ({ state, dispatch }) => {
				return setCellAttr(name, value)(state, dispatch);
			},
			goToNextCell: () => ({ state, dispatch }) => {
				return goToNextCell(1)(state, dispatch);
			},
			goToPreviousCell: () => ({ state, dispatch }) => {
				return goToNextCell(-1)(state, dispatch);
			},
			fixTables: () => ({ state, dispatch }) => {
				if (dispatch) fixTables(state);
				return true;
			},
			setCellSelection: (position) => ({ tr: tr$1, dispatch }) => {
				if (dispatch) {
					const selection = CellSelection.create(tr$1.doc, position.anchorCell, position.headCell);
					tr$1.setSelection(selection);
				}
				return true;
			}
		};
	},
	addKeyboardShortcuts() {
		return {
			Tab: () => {
				if (this.editor.commands.goToNextCell()) return true;
				if (!this.editor.can().addRowAfter()) return false;
				return this.editor.chain().addRowAfter().goToNextCell().run();
			},
			"Shift-Tab": () => this.editor.commands.goToPreviousCell(),
			Backspace: deleteTableWhenAllCellsSelected,
			"Mod-Backspace": deleteTableWhenAllCellsSelected,
			Delete: deleteTableWhenAllCellsSelected,
			"Mod-Delete": deleteTableWhenAllCellsSelected
		};
	},
	addProseMirrorPlugins() {
		return [...this.options.resizable && this.editor.isEditable ? [columnResizing({
			handleWidth: this.options.handleWidth,
			cellMinWidth: this.options.cellMinWidth,
			defaultCellMinWidth: this.options.cellMinWidth,
			View: this.options.View,
			lastColumnResizable: this.options.lastColumnResizable
		})] : [], tableEditing({ allowTableNodeSelection: this.options.allowTableNodeSelection })];
	},
	extendNodeSchema(extension) {
		return { tableRole: callOrReturn(getExtensionField(extension, "tableRole", {
			name: extension.name,
			options: extension.options,
			storage: extension.storage
		})) };
	}
});
Extension.create({
	name: "tableKit",
	addExtensions() {
		const extensions = [];
		if (this.options.table !== false) extensions.push(Table.configure(this.options.table));
		if (this.options.tableCell !== false) extensions.push(TableCell.configure(this.options.tableCell));
		if (this.options.tableHeader !== false) extensions.push(TableHeader.configure(this.options.tableHeader));
		if (this.options.tableRow !== false) extensions.push(TableRow.configure(this.options.tableRow));
		return extensions;
	}
});
var BlockMath = Node3.create({
	name: "blockMath",
	group: "block",
	atom: true,
	addOptions() {
		return {
			onClick: void 0,
			katexOptions: void 0
		};
	},
	addAttributes() {
		return { latex: {
			default: "",
			parseHTML: (element) => element.getAttribute("data-latex"),
			renderHTML: (attributes) => {
				return { "data-latex": attributes.latex };
			}
		} };
	},
	addCommands() {
		return {
			insertBlockMath: (options) => ({ commands, editor }) => {
				const { latex, pos } = options;
				if (!latex) return false;
				return commands.insertContentAt(pos != null ? pos : editor.state.selection.from, {
					type: this.name,
					attrs: { latex }
				});
			},
			deleteBlockMath: (options) => ({ editor, tr: tr$1 }) => {
				var _a;
				const pos = (_a = options == null ? void 0 : options.pos) != null ? _a : editor.state.selection.$from.pos;
				const node = editor.state.doc.nodeAt(pos);
				if (!node || node.type.name !== this.name) return false;
				tr$1.delete(pos, pos + node.nodeSize);
				return true;
			},
			updateBlockMath: (options) => ({ editor, tr: tr$1 }) => {
				const latex = options == null ? void 0 : options.latex;
				let pos = options == null ? void 0 : options.pos;
				if (pos === void 0) pos = editor.state.selection.$from.pos;
				const node = editor.state.doc.nodeAt(pos);
				if (!node || node.type.name !== this.name) return false;
				tr$1.setNodeMarkup(pos, this.type, {
					...node.attrs,
					latex: latex || node.attrs.latex
				});
				return true;
			}
		};
	},
	parseHTML() {
		return [{ tag: "div[data-type=\"block-math\"]" }];
	},
	renderHTML({ HTMLAttributes }) {
		return ["div", mergeAttributes(HTMLAttributes, { "data-type": "block-math" })];
	},
	parseMarkdown: (token) => {
		return {
			type: "blockMath",
			attrs: { latex: token.latex }
		};
	},
	renderMarkdown: (node) => {
		var _a;
		return [
			"$$",
			((_a = node.attrs) == null ? void 0 : _a.latex) || "",
			"$$"
		].join("\n");
	},
	markdownTokenizer: {
		name: "blockMath",
		level: "block",
		start: (src) => src.indexOf("$$"),
		tokenize: (src) => {
			const match = src.match(/^\$\$([^$]+)\$\$/);
			if (!match) return;
			const [fullMatch, latex] = match;
			return {
				type: "blockMath",
				raw: fullMatch,
				latex: latex.trim()
			};
		}
	},
	addInputRules() {
		return [new InputRule({
			find: /^\$\$\$([^$]+)\$\$\$$/,
			handler: ({ state, range, match }) => {
				const [, latex] = match;
				const { tr: tr$1 } = state;
				const start = range.from;
				const end = range.to;
				tr$1.replaceWith(start, end, this.type.create({ latex }));
			}
		})];
	},
	addNodeView() {
		const { katexOptions } = this.options;
		return ({ node, getPos }) => {
			const wrapper = document.createElement("div");
			const innerWrapper = document.createElement("div");
			wrapper.className = "tiptap-mathematics-render";
			if (this.editor.isEditable) wrapper.classList.add("tiptap-mathematics-render--editable");
			innerWrapper.className = "block-math-inner";
			wrapper.dataset.type = "block-math";
			wrapper.setAttribute("data-latex", node.attrs.latex);
			wrapper.appendChild(innerWrapper);
			function renderMath() {
				try {
					katex.render(node.attrs.latex, innerWrapper, katexOptions);
					wrapper.classList.remove("block-math-error");
				} catch {
					wrapper.textContent = node.attrs.latex;
					wrapper.classList.add("block-math-error");
				}
			}
			const handleClick$1 = (event) => {
				event.preventDefault();
				event.stopPropagation();
				const pos = getPos();
				if (pos == null) return;
				if (this.options.onClick) this.options.onClick(node, pos);
			};
			if (this.options.onClick) wrapper.addEventListener("click", handleClick$1);
			renderMath();
			return {
				dom: wrapper,
				destroy() {
					wrapper.removeEventListener("click", handleClick$1);
				}
			};
		};
	}
});
var InlineMath = Node3.create({
	name: "inlineMath",
	group: "inline",
	inline: true,
	atom: true,
	addOptions() {
		return {
			onClick: void 0,
			katexOptions: void 0
		};
	},
	addAttributes() {
		return { latex: {
			default: "",
			parseHTML: (element) => element.getAttribute("data-latex"),
			renderHTML: (attributes) => {
				return { "data-latex": attributes.latex };
			}
		} };
	},
	addCommands() {
		return {
			insertInlineMath: (options) => ({ editor, tr: tr$1 }) => {
				var _a;
				const latex = options.latex;
				const from = (_a = options == null ? void 0 : options.pos) != null ? _a : editor.state.selection.from;
				if (!latex) return false;
				tr$1.replaceWith(from, from, this.type.create({ latex }));
				return true;
			},
			deleteInlineMath: (options) => ({ editor, tr: tr$1 }) => {
				var _a;
				const pos = (_a = options == null ? void 0 : options.pos) != null ? _a : editor.state.selection.$from.pos;
				const node = editor.state.doc.nodeAt(pos);
				if (!node || node.type.name !== this.name) return false;
				tr$1.delete(pos, pos + node.nodeSize);
				return true;
			},
			updateInlineMath: (options) => ({ editor, tr: tr$1 }) => {
				const latex = options == null ? void 0 : options.latex;
				let pos = options == null ? void 0 : options.pos;
				if (pos === void 0) pos = editor.state.selection.$from.pos;
				const node = editor.state.doc.nodeAt(pos);
				if (!node || node.type.name !== this.name) return false;
				tr$1.setNodeMarkup(pos, this.type, {
					...node.attrs,
					latex
				});
				return true;
			}
		};
	},
	parseHTML() {
		return [{ tag: "span[data-type=\"inline-math\"]" }];
	},
	renderHTML({ HTMLAttributes }) {
		return ["span", mergeAttributes(HTMLAttributes, { "data-type": "inline-math" })];
	},
	parseMarkdown: (token) => {
		return {
			type: "inlineMath",
			attrs: { latex: token.latex }
		};
	},
	renderMarkdown: (node) => {
		var _a;
		return `$${((_a = node.attrs) == null ? void 0 : _a.latex) || ""}$`;
	},
	markdownTokenizer: {
		name: "inlineMath",
		level: "inline",
		start: (src) => src.indexOf("$"),
		tokenize: (src) => {
			const match = src.match(/^\$([^$]+)\$(?!\$)/);
			if (!match) return;
			const [fullMatch, latex] = match;
			return {
				type: "inlineMath",
				raw: fullMatch,
				latex: latex.trim()
			};
		}
	},
	addInputRules() {
		return [new InputRule({
			find: /(?<!\$)(\$\$([^$\n]+?)\$\$)(?!\$)/,
			handler: ({ state, range, match }) => {
				const latex = match[2];
				const { tr: tr$1 } = state;
				const start = range.from;
				const end = range.to;
				tr$1.replaceWith(start, end, this.type.create({ latex }));
			}
		})];
	},
	addNodeView() {
		const { katexOptions } = this.options;
		return ({ node, getPos }) => {
			const wrapper = document.createElement("span");
			wrapper.className = "tiptap-mathematics-render";
			if (this.editor.isEditable) wrapper.classList.add("tiptap-mathematics-render--editable");
			wrapper.dataset.type = "inline-math";
			wrapper.setAttribute("data-latex", node.attrs.latex);
			function renderMath() {
				try {
					katex.render(node.attrs.latex, wrapper, katexOptions);
					wrapper.classList.remove("inline-math-error");
				} catch {
					wrapper.textContent = node.attrs.latex;
					wrapper.classList.add("inline-math-error");
				}
			}
			const handleClick$1 = (event) => {
				event.preventDefault();
				event.stopPropagation();
				const pos = getPos();
				if (pos == null) return;
				if (this.options.onClick) this.options.onClick(node, pos);
			};
			if (this.options.onClick) wrapper.addEventListener("click", handleClick$1);
			renderMath();
			return {
				dom: wrapper,
				destroy() {
					wrapper.removeEventListener("click", handleClick$1);
				}
			};
		};
	}
});
Extension.create({
	name: "Mathematics",
	addOptions() {
		return {
			inlineOptions: void 0,
			blockOptions: void 0,
			katexOptions: void 0
		};
	},
	addExtensions() {
		return [BlockMath.configure({
			...this.options.blockOptions,
			katexOptions: this.options.katexOptions
		}), InlineMath.configure({
			...this.options.inlineOptions,
			katexOptions: this.options.katexOptions
		})];
	}
});
init_defineProperty();
var _Class;
function M() {
	return {
		async: !1,
		breaks: !1,
		extensions: null,
		gfm: !0,
		hooks: null,
		pedantic: !1,
		renderer: null,
		silent: !1,
		tokenizer: null,
		walkTokens: null
	};
}
var O = M();
function G(u) {
	O = u;
}
var _ = { exec: () => null };
function k(u, e = "") {
	let t = typeof u == "string" ? u : u.source, n = {
		replace: (r, i$1) => {
			let s = typeof i$1 == "string" ? i$1 : i$1.source;
			return s = s.replace(m.caret, "$1"), t = t.replace(r, s), n;
		},
		getRegex: () => new RegExp(t, e)
	};
	return n;
}
var be = (() => {
	try {
		return true;
	} catch {
		return !1;
	}
})(), m = {
	codeRemoveIndent: /^(?: {1,4}| {0,3}\t)/gm,
	outputLinkReplace: /\\([\[\]])/g,
	indentCodeCompensation: /^(\s+)(?:```)/,
	beginningSpace: /^\s+/,
	endingHash: /#$/,
	startingSpaceChar: /^ /,
	endingSpaceChar: / $/,
	nonSpaceChar: /[^ ]/,
	newLineCharGlobal: /\n/g,
	tabCharGlobal: /\t/g,
	multipleSpaceGlobal: /\s+/g,
	blankLine: /^[ \t]*$/,
	doubleBlankLine: /\n[ \t]*\n[ \t]*$/,
	blockquoteStart: /^ {0,3}>/,
	blockquoteSetextReplace: /\n {0,3}((?:=+|-+) *)(?=\n|$)/g,
	blockquoteSetextReplace2: /^ {0,3}>[ \t]?/gm,
	listReplaceNesting: /^ {1,4}(?=( {4})*[^ ])/g,
	listIsTask: /^\[[ xX]\] +\S/,
	listReplaceTask: /^\[[ xX]\] +/,
	listTaskCheckbox: /\[[ xX]\]/,
	anyLine: /\n.*\n/,
	hrefBrackets: /^<(.*)>$/,
	tableDelimiter: /[:|]/,
	tableAlignChars: /^\||\| *$/g,
	tableRowBlankLine: /\n[ \t]*$/,
	tableAlignRight: /^ *-+: *$/,
	tableAlignCenter: /^ *:-+: *$/,
	tableAlignLeft: /^ *:-+ *$/,
	startATag: /^<a /i,
	endATag: /^<\/a>/i,
	startPreScriptTag: /^<(pre|code|kbd|script)(\s|>)/i,
	endPreScriptTag: /^<\/(pre|code|kbd|script)(\s|>)/i,
	startAngleBracket: /^</,
	endAngleBracket: />$/,
	pedanticHrefTitle: /^([^'"]*[^\s])\s+(['"])(.*)\2/,
	unicodeAlphaNumeric: /[\p{L}\p{N}]/u,
	escapeTest: /[&<>"']/,
	escapeReplace: /[&<>"']/g,
	escapeTestNoEncode: /[<>"']|&(?!(#\d{1,7}|#[Xx][a-fA-F0-9]{1,6}|\w+);)/,
	escapeReplaceNoEncode: /[<>"']|&(?!(#\d{1,7}|#[Xx][a-fA-F0-9]{1,6}|\w+);)/g,
	caret: /(^|[^\[])\^/g,
	percentDecode: /%25/g,
	findPipe: /\|/g,
	splitPipe: / \|/,
	slashPipe: /\\\|/g,
	carriageReturn: /\r\n|\r/g,
	spaceLine: /^ +$/gm,
	notSpaceStart: /^\S*/,
	endingNewline: /\n$/,
	listItemRegex: (u) => /* @__PURE__ */ new RegExp(`^( {0,3}${u})((?:[	 ][^\\n]*)?(?:\\n|$))`),
	nextBulletRegex: (u) => /* @__PURE__ */ new RegExp(`^ {0,${Math.min(3, u - 1)}}(?:[*+-]|\\d{1,9}[.)])((?:[ 	][^\\n]*)?(?:\\n|$))`),
	hrRegex: (u) => /* @__PURE__ */ new RegExp(`^ {0,${Math.min(3, u - 1)}}((?:- *){3,}|(?:_ *){3,}|(?:\\* *){3,})(?:\\n+|$)`),
	fencesBeginRegex: (u) => /* @__PURE__ */ new RegExp(`^ {0,${Math.min(3, u - 1)}}(?:\`\`\`|~~~)`),
	headingBeginRegex: (u) => /* @__PURE__ */ new RegExp(`^ {0,${Math.min(3, u - 1)}}#`),
	htmlBeginRegex: (u) => new RegExp(`^ {0,${Math.min(3, u - 1)}}<(?:[a-z].*>|!--)`, "i"),
	blockquoteBeginRegex: (u) => /* @__PURE__ */ new RegExp(`^ {0,${Math.min(3, u - 1)}}>`)
}, Re = /^(?:[ \t]*(?:\n|$))+/, Oe = /^((?: {4}| {0,3}\t)[^\n]+(?:\n(?:[ \t]*(?:\n|$))*)?)+/, Te = /^ {0,3}(`{3,}(?=[^`\n]*(?:\n|$))|~{3,})([^\n]*)(?:\n|$)(?:|([\s\S]*?)(?:\n|$))(?: {0,3}\1[~`]* *(?=\n|$)|$)/, C = /^ {0,3}((?:-[\t ]*){3,}|(?:_[ \t]*){3,}|(?:\*[ \t]*){3,})(?:\n+|$)/, we = /^ {0,3}(#{1,6})(?=\s|$)(.*)(?:\n+|$)/, Q = / {0,3}(?:[*+-]|\d{1,9}[.)])/, se = /^(?!bull |blockCode|fences|blockquote|heading|html|table)((?:.|\n(?!\s*?\n|bull |blockCode|fences|blockquote|heading|html|table))+?)\n {0,3}(=+|-+) *(?:\n+|$)/, ie = k(se).replace(/bull/g, Q).replace(/blockCode/g, /(?: {4}| {0,3}\t)/).replace(/fences/g, / {0,3}(?:`{3,}|~{3,})/).replace(/blockquote/g, / {0,3}>/).replace(/heading/g, / {0,3}#{1,6}/).replace(/html/g, / {0,3}<[^\n>]+>\n/).replace(/\|table/g, "").getRegex(), ye = k(se).replace(/bull/g, Q).replace(/blockCode/g, /(?: {4}| {0,3}\t)/).replace(/fences/g, / {0,3}(?:`{3,}|~{3,})/).replace(/blockquote/g, / {0,3}>/).replace(/heading/g, / {0,3}#{1,6}/).replace(/html/g, / {0,3}<[^\n>]+>\n/).replace(/table/g, / {0,3}\|?(?:[:\- ]*\|)+[\:\- ]*\n/).getRegex(), j = /^([^\n]+(?:\n(?!hr|heading|lheading|blockquote|fences|list|html|table| +\n)[^\n]+)*)/, Pe = /^[^\n]+/, F = /(?!\s*\])(?:\\[\s\S]|[^\[\]\\])+/, Se = k(/^ {0,3}\[(label)\]: *(?:\n[ \t]*)?([^<\s][^\s]*|<.*?>)(?:(?: +(?:\n[ \t]*)?| *\n[ \t]*)(title))? *(?:\n+|$)/).replace("label", F).replace("title", /(?:"(?:\\"?|[^"\\])*"|'[^'\n]*(?:\n[^'\n]+)*\n?'|\([^()]*\))/).getRegex(), $e = k(/^(bull)([ \t][^\n]+?)?(?:\n|$)/).replace(/bull/g, Q).getRegex(), v = "address|article|aside|base|basefont|blockquote|body|caption|center|col|colgroup|dd|details|dialog|dir|div|dl|dt|fieldset|figcaption|figure|footer|form|frame|frameset|h[1-6]|head|header|hr|html|iframe|legend|li|link|main|menu|menuitem|meta|nav|noframes|ol|optgroup|option|p|param|search|section|summary|table|tbody|td|tfoot|th|thead|title|tr|track|ul", U = /<!--(?:-?>|[\s\S]*?(?:-->|$))/, _e = k("^ {0,3}(?:<(script|pre|style|textarea)[\\s>][\\s\\S]*?(?:</\\1>[^\\n]*\\n+|$)|comment[^\\n]*(\\n+|$)|<\\?[\\s\\S]*?(?:\\?>\\n*|$)|<![A-Z][\\s\\S]*?(?:>\\n*|$)|<!\\[CDATA\\[[\\s\\S]*?(?:\\]\\]>\\n*|$)|</?(tag)(?: +|\\n|/?>)[\\s\\S]*?(?:(?:\\n[ 	]*)+\\n|$)|<(?!script|pre|style|textarea)([a-z][\\w-]*)(?:attribute)*? */?>(?=[ \\t]*(?:\\n|$))[\\s\\S]*?(?:(?:\\n[ 	]*)+\\n|$)|</(?!script|pre|style|textarea)[a-z][\\w-]*\\s*>(?=[ \\t]*(?:\\n|$))[\\s\\S]*?(?:(?:\\n[ 	]*)+\\n|$))", "i").replace("comment", U).replace("tag", v).replace("attribute", / +[a-zA-Z:_][\w.:-]*(?: *= *"[^"\n]*"| *= *'[^'\n]*'| *= *[^\s"'=<>`]+)?/).getRegex(), oe = k(j).replace("hr", C).replace("heading", " {0,3}#{1,6}(?:\\s|$)").replace("|lheading", "").replace("|table", "").replace("blockquote", " {0,3}>").replace("fences", " {0,3}(?:`{3,}(?=[^`\\n]*\\n)|~{3,})[^\\n]*\\n").replace("list", " {0,3}(?:[*+-]|1[.)])[ \\t]").replace("html", "</?(?:tag)(?: +|\\n|/?>)|<(?:script|pre|style|textarea|!--)").replace("tag", v).getRegex(), K = {
	blockquote: k(/^( {0,3}> ?(paragraph|[^\n]*)(?:\n|$))+/).replace("paragraph", oe).getRegex(),
	code: Oe,
	def: Se,
	fences: Te,
	heading: we,
	hr: C,
	html: _e,
	lheading: ie,
	list: $e,
	newline: Re,
	paragraph: oe,
	table: _,
	text: Pe
}, ne = k("^ *([^\\n ].*)\\n {0,3}((?:\\| *)?:?-+:? *(?:\\| *:?-+:? *)*(?:\\| *)?)(?:\\n((?:(?! *\\n|hr|heading|blockquote|code|fences|list|html).*(?:\\n|$))*)\\n*|$)").replace("hr", C).replace("heading", " {0,3}#{1,6}(?:\\s|$)").replace("blockquote", " {0,3}>").replace("code", "(?: {4}| {0,3}	)[^\\n]").replace("fences", " {0,3}(?:`{3,}(?=[^`\\n]*\\n)|~{3,})[^\\n]*\\n").replace("list", " {0,3}(?:[*+-]|1[.)])[ \\t]").replace("html", "</?(?:tag)(?: +|\\n|/?>)|<(?:script|pre|style|textarea|!--)").replace("tag", v).getRegex(), Me = {
	...K,
	lheading: ye,
	table: ne,
	paragraph: k(j).replace("hr", C).replace("heading", " {0,3}#{1,6}(?:\\s|$)").replace("|lheading", "").replace("table", ne).replace("blockquote", " {0,3}>").replace("fences", " {0,3}(?:`{3,}(?=[^`\\n]*\\n)|~{3,})[^\\n]*\\n").replace("list", " {0,3}(?:[*+-]|1[.)])[ \\t]").replace("html", "</?(?:tag)(?: +|\\n|/?>)|<(?:script|pre|style|textarea|!--)").replace("tag", v).getRegex()
}, ze = {
	...K,
	html: k(`^ *(?:comment *(?:\\n|\\s*$)|<(tag)[\\s\\S]+?</\\1> *(?:\\n{2,}|\\s*$)|<tag(?:"[^"]*"|'[^']*'|\\s[^'"/>\\s]*)*?/?> *(?:\\n{2,}|\\s*$))`).replace("comment", U).replace(/tag/g, "(?!(?:a|em|strong|small|s|cite|q|dfn|abbr|data|time|code|var|samp|kbd|sub|sup|i|b|u|mark|ruby|rt|rp|bdi|bdo|span|br|wbr|ins|del|img)\\b)\\w+(?!:|[^\\w\\s@]*@)\\b").getRegex(),
	def: /^ *\[([^\]]+)\]: *<?([^\s>]+)>?(?: +(["(][^\n]+[")]))? *(?:\n+|$)/,
	heading: /^(#{1,6})(.*)(?:\n+|$)/,
	fences: _,
	lheading: /^(.+?)\n {0,3}(=+|-+) *(?:\n+|$)/,
	paragraph: k(j).replace("hr", C).replace("heading", ` *#{1,6} *[^
]`).replace("lheading", ie).replace("|table", "").replace("blockquote", " {0,3}>").replace("|fences", "").replace("|list", "").replace("|html", "").replace("|tag", "").getRegex()
}, Ee = /^\\([!"#$%&'()*+,\-./:;<=>?@\[\]\\^_`{|}~])/, Ie = /^(`+)([^`]|[^`][\s\S]*?[^`])\1(?!`)/, ae = /^( {2,}|\\)\n(?!\s*$)/, Ae = /^(`+|[^`])(?:(?= {2,}\n)|[\s\S]*?(?:(?=[\\<!\[`*_]|\b_|$)|[^ ](?= {2,}\n)))/, z = /[\p{P}\p{S}]/u, H = /[\s\p{P}\p{S}]/u, W = /[^\s\p{P}\p{S}]/u, Ce = k(/^((?![*_])punctSpace)/, "u").replace(/punctSpace/g, H).getRegex(), le = /(?!~)[\p{P}\p{S}]/u, Be = /(?!~)[\s\p{P}\p{S}]/u, De = /(?:[^\s\p{P}\p{S}]|~)/u, qe = k(/link|precode-code|html/, "g").replace("link", /\[(?:[^\[\]`]|(?<a>`+)[^`]+\k<a>(?!`))*?\]\((?:\\[\s\S]|[^\\\(\)]|\((?:\\[\s\S]|[^\\\(\)])*\))*\)/).replace("precode-", be ? "(?<!`)()" : "(^^|[^`])").replace("code", /(?<b>`+)[^`]+\k<b>(?!`)/).replace("html", /<(?! )[^<>]*?>/).getRegex(), ue = /^(?:\*+(?:((?!\*)punct)|([^\s*]))?)|^_+(?:((?!_)punct)|([^\s_]))?/, ve = k(ue, "u").replace(/punct/g, z).getRegex(), He = k(ue, "u").replace(/punct/g, le).getRegex(), pe = "^[^_*]*?__[^_*]*?\\*[^_*]*?(?=__)|[^*]+(?=[^*])|(?!\\*)punct(\\*+)(?=[\\s]|$)|notPunctSpace(\\*+)(?!\\*)(?=punctSpace|$)|(?!\\*)punctSpace(\\*+)(?=notPunctSpace)|[\\s](\\*+)(?!\\*)(?=punct)|(?!\\*)punct(\\*+)(?!\\*)(?=punct)|notPunctSpace(\\*+)(?=notPunctSpace)", Ze = k(pe, "gu").replace(/notPunctSpace/g, W).replace(/punctSpace/g, H).replace(/punct/g, z).getRegex(), Ge = k(pe, "gu").replace(/notPunctSpace/g, De).replace(/punctSpace/g, Be).replace(/punct/g, le).getRegex(), Ne = k("^[^_*]*?\\*\\*[^_*]*?_[^_*]*?(?=\\*\\*)|[^_]+(?=[^_])|(?!_)punct(_+)(?=[\\s]|$)|notPunctSpace(_+)(?!_)(?=punctSpace|$)|(?!_)punctSpace(_+)(?=notPunctSpace)|[\\s](_+)(?!_)(?=punct)|(?!_)punct(_+)(?!_)(?=punct)", "gu").replace(/notPunctSpace/g, W).replace(/punctSpace/g, H).replace(/punct/g, z).getRegex(), Qe = k(/^~~?(?:((?!~)punct)|[^\s~])/, "u").replace(/punct/g, z).getRegex(), Fe = k("^[^~]+(?=[^~])|(?!~)punct(~~?)(?=[\\s]|$)|notPunctSpace(~~?)(?!~)(?=punctSpace|$)|(?!~)punctSpace(~~?)(?=notPunctSpace)|[\\s](~~?)(?!~)(?=punct)|(?!~)punct(~~?)(?!~)(?=punct)|notPunctSpace(~~?)(?=notPunctSpace)", "gu").replace(/notPunctSpace/g, W).replace(/punctSpace/g, H).replace(/punct/g, z).getRegex(), Ue = k(/\\(punct)/, "gu").replace(/punct/g, z).getRegex(), Ke = k(/^<(scheme:[^\s\x00-\x1f<>]*|email)>/).replace("scheme", /[a-zA-Z][a-zA-Z0-9+.-]{1,31}/).replace("email", /[a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+(@)[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)+(?![-_])/).getRegex(), We = k(U).replace("(?:-->|$)", "-->").getRegex(), Xe = k("^comment|^</[a-zA-Z][\\w:-]*\\s*>|^<[a-zA-Z][\\w-]*(?:attribute)*?\\s*/?>|^<\\?[\\s\\S]*?\\?>|^<![a-zA-Z]+\\s[\\s\\S]*?>|^<!\\[CDATA\\[[\\s\\S]*?\\]\\]>").replace("comment", We).replace("attribute", /\s+[a-zA-Z:_][\w.:-]*(?:\s*=\s*"[^"]*"|\s*=\s*'[^']*'|\s*=\s*[^\s"'=<>`]+)?/).getRegex(), q = /(?:\[(?:\\[\s\S]|[^\[\]\\])*\]|\\[\s\S]|`+(?!`)[^`]*?`+(?!`)|``+(?=\])|[^\[\]\\`])*?/, Je = k(/^!?\[(label)\]\(\s*(href)(?:(?:[ \t]+(?:\n[ \t]*)?|\n[ \t]*)(title))?\s*\)/).replace("label", q).replace("href", /<(?:\\.|[^\n<>\\])+>|[^ \t\n\x00-\x1f]*/).replace("title", /"(?:\\"?|[^"\\])*"|'(?:\\'?|[^'\\])*'|\((?:\\\)?|[^)\\])*\)/).getRegex(), ce = k(/^!?\[(label)\]\[(ref)\]/).replace("label", q).replace("ref", F).getRegex(), he = k(/^!?\[(ref)\](?:\[\])?/).replace("ref", F).getRegex(), Ve = k("reflink|nolink(?!\\()", "g").replace("reflink", ce).replace("nolink", he).getRegex(), re = /[hH][tT][tT][pP][sS]?|[fF][tT][pP]/, X = {
	_backpedal: _,
	anyPunctuation: Ue,
	autolink: Ke,
	blockSkip: qe,
	br: ae,
	code: Ie,
	del: _,
	delLDelim: _,
	delRDelim: _,
	emStrongLDelim: ve,
	emStrongRDelimAst: Ze,
	emStrongRDelimUnd: Ne,
	escape: Ee,
	link: Je,
	nolink: he,
	punctuation: Ce,
	reflink: ce,
	reflinkSearch: Ve,
	tag: Xe,
	text: Ae,
	url: _
}, Ye = {
	...X,
	link: k(/^!?\[(label)\]\((.*?)\)/).replace("label", q).getRegex(),
	reflink: k(/^!?\[(label)\]\s*\[([^\]]*)\]/).replace("label", q).getRegex()
}, N = {
	...X,
	emStrongRDelimAst: Ge,
	emStrongLDelim: He,
	delLDelim: Qe,
	delRDelim: Fe,
	url: k(/^((?:protocol):\/\/|www\.)(?:[a-zA-Z0-9\-]+\.?)+[^\s<]*|^email/).replace("protocol", re).replace("email", /[A-Za-z0-9._+-]+(@)[a-zA-Z0-9-_]+(?:\.[a-zA-Z0-9-_]*[a-zA-Z0-9])+(?![-_])/).getRegex(),
	_backpedal: /(?:[^?!.,:;*_'"~()&]+|\([^)]*\)|&(?![a-zA-Z0-9]+;$)|[?!.,:;*_'"~)]+(?!$))+/,
	del: /^(~~?)(?=[^\s~])((?:\\[\s\S]|[^\\])*?(?:\\[\s\S]|[^\s~\\]))\1(?=[^~]|$)/,
	text: k(/^([`~]+|[^`~])(?:(?= {2,}\n)|(?=[a-zA-Z0-9.!#$%&'*+\/=?_`{\|}~-]+@)|[\s\S]*?(?:(?=[\\<!\[`*~_]|\b_|protocol:\/\/|www\.|$)|[^ ](?= {2,}\n)|[^a-zA-Z0-9.!#$%&'*+\/=?_`{\|}~-](?=[a-zA-Z0-9.!#$%&'*+\/=?_`{\|}~-]+@)))/).replace("protocol", re).getRegex()
}, et = {
	...N,
	br: k(ae).replace("{2,}", "*").getRegex(),
	text: k(N.text).replace("\\b_", "\\b_| {2,}\\n").replace(/\{2,\}/g, "*").getRegex()
}, B = {
	normal: K,
	gfm: Me,
	pedantic: ze
}, E = {
	normal: X,
	gfm: N,
	breaks: et,
	pedantic: Ye
};
var tt = {
	"&": "&amp;",
	"<": "&lt;",
	">": "&gt;",
	"\"": "&quot;",
	"'": "&#39;"
}, ke = (u) => tt[u];
function T(u, e) {
	if (e) {
		if (m.escapeTest.test(u)) return u.replace(m.escapeReplace, ke);
	} else if (m.escapeTestNoEncode.test(u)) return u.replace(m.escapeReplaceNoEncode, ke);
	return u;
}
function J(u) {
	try {
		u = encodeURI(u).replace(m.percentDecode, "%");
	} catch {
		return null;
	}
	return u;
}
function V(u, e) {
	let n = u.replace(m.findPipe, (i$1, s, a) => {
		let o = !1, l = s;
		for (; --l >= 0 && a[l] === "\\";) o = !o;
		return o ? "|" : " |";
	}).split(m.splitPipe), r = 0;
	if (n[0].trim() || n.shift(), n.length > 0 && !n.at(-1)?.trim() && n.pop(), e) if (n.length > e) n.splice(e);
	else for (; n.length < e;) n.push("");
	for (; r < n.length; r++) n[r] = n[r].trim().replace(m.slashPipe, "|");
	return n;
}
function I(u, e, t) {
	let n = u.length;
	if (n === 0) return "";
	let r = 0;
	for (; r < n;) {
		let i$1 = u.charAt(n - r - 1);
		if (i$1 === e && !t) r++;
		else if (i$1 !== e && t) r++;
		else break;
	}
	return u.slice(0, n - r);
}
function de(u, e) {
	if (u.indexOf(e[1]) === -1) return -1;
	let t = 0;
	for (let n = 0; n < u.length; n++) if (u[n] === "\\") n++;
	else if (u[n] === e[0]) t++;
	else if (u[n] === e[1] && (t--, t < 0)) return n;
	return t > 0 ? -2 : -1;
}
function ge(u, e = 0) {
	let t = e, n = "";
	for (let r of u) if (r === "	") {
		let i$1 = 4 - t % 4;
		n += " ".repeat(i$1), t += i$1;
	} else n += r, t++;
	return n;
}
function fe(u, e, t, n, r) {
	let i$1 = e.href, s = e.title || null, a = u[1].replace(r.other.outputLinkReplace, "$1");
	n.state.inLink = !0;
	let o = {
		type: u[0].charAt(0) === "!" ? "image" : "link",
		raw: t,
		href: i$1,
		title: s,
		text: a,
		tokens: n.inlineTokens(a)
	};
	return n.state.inLink = !1, o;
}
function nt(u, e, t) {
	let n = u.match(t.other.indentCodeCompensation);
	if (n === null) return e;
	let r = n[1];
	return e.split(`
`).map((i$1) => {
		let s = i$1.match(t.other.beginningSpace);
		if (s === null) return i$1;
		let [a] = s;
		return a.length >= r.length ? i$1.slice(r.length) : i$1;
	}).join(`
`);
}
var w = class {
	constructor(e) {
		_defineProperty(this, "options", void 0);
		_defineProperty(this, "rules", void 0);
		_defineProperty(this, "lexer", void 0);
		this.options = e || O;
	}
	space(e) {
		let t = this.rules.block.newline.exec(e);
		if (t && t[0].length > 0) return {
			type: "space",
			raw: t[0]
		};
	}
	code(e) {
		let t = this.rules.block.code.exec(e);
		if (t) {
			let n = t[0].replace(this.rules.other.codeRemoveIndent, "");
			return {
				type: "code",
				raw: t[0],
				codeBlockStyle: "indented",
				text: this.options.pedantic ? n : I(n, `
`)
			};
		}
	}
	fences(e) {
		let t = this.rules.block.fences.exec(e);
		if (t) {
			let n = t[0], r = nt(n, t[3] || "", this.rules);
			return {
				type: "code",
				raw: n,
				lang: t[2] ? t[2].trim().replace(this.rules.inline.anyPunctuation, "$1") : t[2],
				text: r
			};
		}
	}
	heading(e) {
		let t = this.rules.block.heading.exec(e);
		if (t) {
			let n = t[2].trim();
			if (this.rules.other.endingHash.test(n)) {
				let r = I(n, "#");
				(this.options.pedantic || !r || this.rules.other.endingSpaceChar.test(r)) && (n = r.trim());
			}
			return {
				type: "heading",
				raw: t[0],
				depth: t[1].length,
				text: n,
				tokens: this.lexer.inline(n)
			};
		}
	}
	hr(e) {
		let t = this.rules.block.hr.exec(e);
		if (t) return {
			type: "hr",
			raw: I(t[0], `
`)
		};
	}
	blockquote(e) {
		let t = this.rules.block.blockquote.exec(e);
		if (t) {
			let n = I(t[0], `
`).split(`
`), r = "", i$1 = "", s = [];
			for (; n.length > 0;) {
				let a = !1, o = [], l;
				for (l = 0; l < n.length; l++) if (this.rules.other.blockquoteStart.test(n[l])) o.push(n[l]), a = !0;
				else if (!a) o.push(n[l]);
				else break;
				n = n.slice(l);
				let p = o.join(`
`), c = p.replace(this.rules.other.blockquoteSetextReplace, `
    $1`).replace(this.rules.other.blockquoteSetextReplace2, "");
				r = r ? `${r}
${p}` : p, i$1 = i$1 ? `${i$1}
${c}` : c;
				let d = this.lexer.state.top;
				if (this.lexer.state.top = !0, this.lexer.blockTokens(c, s, !0), this.lexer.state.top = d, n.length === 0) break;
				let h$1 = s.at(-1);
				if (h$1?.type === "code") break;
				if (h$1?.type === "blockquote") {
					let R = h$1, f = R.raw + `
` + n.join(`
`), S = this.blockquote(f);
					s[s.length - 1] = S, r = r.substring(0, r.length - R.raw.length) + S.raw, i$1 = i$1.substring(0, i$1.length - R.text.length) + S.text;
					break;
				} else if (h$1?.type === "list") {
					let R = h$1, f = R.raw + `
` + n.join(`
`), S = this.list(f);
					s[s.length - 1] = S, r = r.substring(0, r.length - h$1.raw.length) + S.raw, i$1 = i$1.substring(0, i$1.length - R.raw.length) + S.raw, n = f.substring(s.at(-1).raw.length).split(`
`);
					continue;
				}
			}
			return {
				type: "blockquote",
				raw: r,
				tokens: s,
				text: i$1
			};
		}
	}
	list(e) {
		let t = this.rules.block.list.exec(e);
		if (t) {
			let n = t[1].trim(), r = n.length > 1, i$1 = {
				type: "list",
				raw: "",
				ordered: r,
				start: r ? +n.slice(0, -1) : "",
				loose: !1,
				items: []
			};
			n = r ? `\\d{1,9}\\${n.slice(-1)}` : `\\${n}`, this.options.pedantic && (n = r ? n : "[*+-]");
			let s = this.rules.other.listItemRegex(n), a = !1;
			for (; e;) {
				let l = !1, p = "", c = "";
				if (!(t = s.exec(e)) || this.rules.block.hr.test(e)) break;
				p = t[0], e = e.substring(p.length);
				let d = ge(t[2].split(`
`, 1)[0], t[1].length), h$1 = e.split(`
`, 1)[0], R = !d.trim(), f = 0;
				if (this.options.pedantic ? (f = 2, c = d.trimStart()) : R ? f = t[1].length + 1 : (f = d.search(this.rules.other.nonSpaceChar), f = f > 4 ? 1 : f, c = d.slice(f), f += t[1].length), R && this.rules.other.blankLine.test(h$1) && (p += h$1 + `
`, e = e.substring(h$1.length + 1), l = !0), !l) {
					let S = this.rules.other.nextBulletRegex(f), Y = this.rules.other.hrRegex(f), ee = this.rules.other.fencesBeginRegex(f), te = this.rules.other.headingBeginRegex(f), me = this.rules.other.htmlBeginRegex(f), xe = this.rules.other.blockquoteBeginRegex(f);
					for (; e;) {
						let Z = e.split(`
`, 1)[0], A;
						if (h$1 = Z, this.options.pedantic ? (h$1 = h$1.replace(this.rules.other.listReplaceNesting, "  "), A = h$1) : A = h$1.replace(this.rules.other.tabCharGlobal, "    "), ee.test(h$1) || te.test(h$1) || me.test(h$1) || xe.test(h$1) || S.test(h$1) || Y.test(h$1)) break;
						if (A.search(this.rules.other.nonSpaceChar) >= f || !h$1.trim()) c += `
` + A.slice(f);
						else {
							if (R || d.replace(this.rules.other.tabCharGlobal, "    ").search(this.rules.other.nonSpaceChar) >= 4 || ee.test(d) || te.test(d) || Y.test(d)) break;
							c += `
` + h$1;
						}
						R = !h$1.trim(), p += Z + `
`, e = e.substring(Z.length + 1), d = A.slice(f);
					}
				}
				i$1.loose || (a ? i$1.loose = !0 : this.rules.other.doubleBlankLine.test(p) && (a = !0)), i$1.items.push({
					type: "list_item",
					raw: p,
					task: !!this.options.gfm && this.rules.other.listIsTask.test(c),
					loose: !1,
					text: c,
					tokens: []
				}), i$1.raw += p;
			}
			let o = i$1.items.at(-1);
			if (o) o.raw = o.raw.trimEnd(), o.text = o.text.trimEnd();
			else return;
			i$1.raw = i$1.raw.trimEnd();
			for (let l of i$1.items) {
				if (this.lexer.state.top = !1, l.tokens = this.lexer.blockTokens(l.text, []), l.task) {
					if (l.text = l.text.replace(this.rules.other.listReplaceTask, ""), l.tokens[0]?.type === "text" || l.tokens[0]?.type === "paragraph") {
						l.tokens[0].raw = l.tokens[0].raw.replace(this.rules.other.listReplaceTask, ""), l.tokens[0].text = l.tokens[0].text.replace(this.rules.other.listReplaceTask, "");
						for (let c = this.lexer.inlineQueue.length - 1; c >= 0; c--) if (this.rules.other.listIsTask.test(this.lexer.inlineQueue[c].src)) {
							this.lexer.inlineQueue[c].src = this.lexer.inlineQueue[c].src.replace(this.rules.other.listReplaceTask, "");
							break;
						}
					}
					let p = this.rules.other.listTaskCheckbox.exec(l.raw);
					if (p) {
						let c = {
							type: "checkbox",
							raw: p[0] + " ",
							checked: p[0] !== "[ ]"
						};
						l.checked = c.checked, i$1.loose ? l.tokens[0] && ["paragraph", "text"].includes(l.tokens[0].type) && "tokens" in l.tokens[0] && l.tokens[0].tokens ? (l.tokens[0].raw = c.raw + l.tokens[0].raw, l.tokens[0].text = c.raw + l.tokens[0].text, l.tokens[0].tokens.unshift(c)) : l.tokens.unshift({
							type: "paragraph",
							raw: c.raw,
							text: c.raw,
							tokens: [c]
						}) : l.tokens.unshift(c);
					}
				}
				if (!i$1.loose) {
					let p = l.tokens.filter((d) => d.type === "space");
					i$1.loose = p.length > 0 && p.some((d) => this.rules.other.anyLine.test(d.raw));
				}
			}
			if (i$1.loose) for (let l of i$1.items) {
				l.loose = !0;
				for (let p of l.tokens) p.type === "text" && (p.type = "paragraph");
			}
			return i$1;
		}
	}
	html(e) {
		let t = this.rules.block.html.exec(e);
		if (t) return {
			type: "html",
			block: !0,
			raw: t[0],
			pre: t[1] === "pre" || t[1] === "script" || t[1] === "style",
			text: t[0]
		};
	}
	def(e) {
		let t = this.rules.block.def.exec(e);
		if (t) {
			let n = t[1].toLowerCase().replace(this.rules.other.multipleSpaceGlobal, " "), r = t[2] ? t[2].replace(this.rules.other.hrefBrackets, "$1").replace(this.rules.inline.anyPunctuation, "$1") : "", i$1 = t[3] ? t[3].substring(1, t[3].length - 1).replace(this.rules.inline.anyPunctuation, "$1") : t[3];
			return {
				type: "def",
				tag: n,
				raw: t[0],
				href: r,
				title: i$1
			};
		}
	}
	table(e) {
		let t = this.rules.block.table.exec(e);
		if (!t || !this.rules.other.tableDelimiter.test(t[2])) return;
		let n = V(t[1]), r = t[2].replace(this.rules.other.tableAlignChars, "").split("|"), i$1 = t[3]?.trim() ? t[3].replace(this.rules.other.tableRowBlankLine, "").split(`
`) : [], s = {
			type: "table",
			raw: t[0],
			header: [],
			align: [],
			rows: []
		};
		if (n.length === r.length) {
			for (let a of r) this.rules.other.tableAlignRight.test(a) ? s.align.push("right") : this.rules.other.tableAlignCenter.test(a) ? s.align.push("center") : this.rules.other.tableAlignLeft.test(a) ? s.align.push("left") : s.align.push(null);
			for (let a = 0; a < n.length; a++) s.header.push({
				text: n[a],
				tokens: this.lexer.inline(n[a]),
				header: !0,
				align: s.align[a]
			});
			for (let a of i$1) s.rows.push(V(a, s.header.length).map((o, l) => ({
				text: o,
				tokens: this.lexer.inline(o),
				header: !1,
				align: s.align[l]
			})));
			return s;
		}
	}
	lheading(e) {
		let t = this.rules.block.lheading.exec(e);
		if (t) {
			let n = t[1].trim();
			return {
				type: "heading",
				raw: t[0],
				depth: t[2].charAt(0) === "=" ? 1 : 2,
				text: n,
				tokens: this.lexer.inline(n)
			};
		}
	}
	paragraph(e) {
		let t = this.rules.block.paragraph.exec(e);
		if (t) {
			let n = t[1].charAt(t[1].length - 1) === `
` ? t[1].slice(0, -1) : t[1];
			return {
				type: "paragraph",
				raw: t[0],
				text: n,
				tokens: this.lexer.inline(n)
			};
		}
	}
	text(e) {
		let t = this.rules.block.text.exec(e);
		if (t) return {
			type: "text",
			raw: t[0],
			text: t[0],
			tokens: this.lexer.inline(t[0])
		};
	}
	escape(e) {
		let t = this.rules.inline.escape.exec(e);
		if (t) return {
			type: "escape",
			raw: t[0],
			text: t[1]
		};
	}
	tag(e) {
		let t = this.rules.inline.tag.exec(e);
		if (t) return !this.lexer.state.inLink && this.rules.other.startATag.test(t[0]) ? this.lexer.state.inLink = !0 : this.lexer.state.inLink && this.rules.other.endATag.test(t[0]) && (this.lexer.state.inLink = !1), !this.lexer.state.inRawBlock && this.rules.other.startPreScriptTag.test(t[0]) ? this.lexer.state.inRawBlock = !0 : this.lexer.state.inRawBlock && this.rules.other.endPreScriptTag.test(t[0]) && (this.lexer.state.inRawBlock = !1), {
			type: "html",
			raw: t[0],
			inLink: this.lexer.state.inLink,
			inRawBlock: this.lexer.state.inRawBlock,
			block: !1,
			text: t[0]
		};
	}
	link(e) {
		let t = this.rules.inline.link.exec(e);
		if (t) {
			let n = t[2].trim();
			if (!this.options.pedantic && this.rules.other.startAngleBracket.test(n)) {
				if (!this.rules.other.endAngleBracket.test(n)) return;
				let s = I(n.slice(0, -1), "\\");
				if ((n.length - s.length) % 2 === 0) return;
			} else {
				let s = de(t[2], "()");
				if (s === -2) return;
				if (s > -1) {
					let o = (t[0].indexOf("!") === 0 ? 5 : 4) + t[1].length + s;
					t[2] = t[2].substring(0, s), t[0] = t[0].substring(0, o).trim(), t[3] = "";
				}
			}
			let r = t[2], i$1 = "";
			if (this.options.pedantic) {
				let s = this.rules.other.pedanticHrefTitle.exec(r);
				s && (r = s[1], i$1 = s[3]);
			} else i$1 = t[3] ? t[3].slice(1, -1) : "";
			return r = r.trim(), this.rules.other.startAngleBracket.test(r) && (this.options.pedantic && !this.rules.other.endAngleBracket.test(n) ? r = r.slice(1) : r = r.slice(1, -1)), fe(t, {
				href: r && r.replace(this.rules.inline.anyPunctuation, "$1"),
				title: i$1 && i$1.replace(this.rules.inline.anyPunctuation, "$1")
			}, t[0], this.lexer, this.rules);
		}
	}
	reflink(e, t) {
		let n;
		if ((n = this.rules.inline.reflink.exec(e)) || (n = this.rules.inline.nolink.exec(e))) {
			let i$1 = t[(n[2] || n[1]).replace(this.rules.other.multipleSpaceGlobal, " ").toLowerCase()];
			if (!i$1) {
				let s = n[0].charAt(0);
				return {
					type: "text",
					raw: s,
					text: s
				};
			}
			return fe(n, i$1, n[0], this.lexer, this.rules);
		}
	}
	emStrong(e, t, n = "") {
		let r = this.rules.inline.emStrongLDelim.exec(e);
		if (!r || !r[1] && !r[2] && !r[3] && !r[4] || r[4] && n.match(this.rules.other.unicodeAlphaNumeric)) return;
		if (!(r[1] || r[3] || "") || !n || this.rules.inline.punctuation.exec(n)) {
			let s = [...r[0]].length - 1, a, o, l = s, p = 0, c = r[0][0] === "*" ? this.rules.inline.emStrongRDelimAst : this.rules.inline.emStrongRDelimUnd;
			for (c.lastIndex = 0, t = t.slice(-1 * e.length + s); (r = c.exec(t)) !== null;) {
				if (a = r[1] || r[2] || r[3] || r[4] || r[5] || r[6], !a) continue;
				if (o = [...a].length, r[3] || r[4]) {
					l += o;
					continue;
				} else if ((r[5] || r[6]) && s % 3 && !((s + o) % 3)) {
					p += o;
					continue;
				}
				if (l -= o, l > 0) continue;
				o = Math.min(o, o + l + p);
				let d = [...r[0]][0].length, h$1 = e.slice(0, s + r.index + d + o);
				if (Math.min(s, o) % 2) {
					let f = h$1.slice(1, -1);
					return {
						type: "em",
						raw: h$1,
						text: f,
						tokens: this.lexer.inlineTokens(f)
					};
				}
				let R = h$1.slice(2, -2);
				return {
					type: "strong",
					raw: h$1,
					text: R,
					tokens: this.lexer.inlineTokens(R)
				};
			}
		}
	}
	codespan(e) {
		let t = this.rules.inline.code.exec(e);
		if (t) {
			let n = t[2].replace(this.rules.other.newLineCharGlobal, " "), r = this.rules.other.nonSpaceChar.test(n), i$1 = this.rules.other.startingSpaceChar.test(n) && this.rules.other.endingSpaceChar.test(n);
			return r && i$1 && (n = n.substring(1, n.length - 1)), {
				type: "codespan",
				raw: t[0],
				text: n
			};
		}
	}
	br(e) {
		let t = this.rules.inline.br.exec(e);
		if (t) return {
			type: "br",
			raw: t[0]
		};
	}
	del(e, t, n = "") {
		let r = this.rules.inline.delLDelim.exec(e);
		if (!r) return;
		if (!(r[1] || "") || !n || this.rules.inline.punctuation.exec(n)) {
			let s = [...r[0]].length - 1, a, o, l = s, p = this.rules.inline.delRDelim;
			for (p.lastIndex = 0, t = t.slice(-1 * e.length + s); (r = p.exec(t)) !== null;) {
				if (a = r[1] || r[2] || r[3] || r[4] || r[5] || r[6], !a || (o = [...a].length, o !== s)) continue;
				if (r[3] || r[4]) {
					l += o;
					continue;
				}
				if (l -= o, l > 0) continue;
				o = Math.min(o, o + l);
				let c = [...r[0]][0].length, d = e.slice(0, s + r.index + c + o), h$1 = d.slice(s, -s);
				return {
					type: "del",
					raw: d,
					text: h$1,
					tokens: this.lexer.inlineTokens(h$1)
				};
			}
		}
	}
	autolink(e) {
		let t = this.rules.inline.autolink.exec(e);
		if (t) {
			let n, r;
			return t[2] === "@" ? (n = t[1], r = "mailto:" + n) : (n = t[1], r = n), {
				type: "link",
				raw: t[0],
				text: n,
				href: r,
				tokens: [{
					type: "text",
					raw: n,
					text: n
				}]
			};
		}
	}
	url(e) {
		let t;
		if (t = this.rules.inline.url.exec(e)) {
			let n, r;
			if (t[2] === "@") n = t[0], r = "mailto:" + n;
			else {
				let i$1;
				do
					i$1 = t[0], t[0] = this.rules.inline._backpedal.exec(t[0])?.[0] ?? "";
				while (i$1 !== t[0]);
				n = t[0], t[1] === "www." ? r = "http://" + t[0] : r = t[0];
			}
			return {
				type: "link",
				raw: t[0],
				text: n,
				href: r,
				tokens: [{
					type: "text",
					raw: n,
					text: n
				}]
			};
		}
	}
	inlineText(e) {
		let t = this.rules.inline.text.exec(e);
		if (t) {
			let n = this.lexer.state.inRawBlock;
			return {
				type: "text",
				raw: t[0],
				text: t[0],
				escaped: n
			};
		}
	}
};
var x = class u {
	constructor(e) {
		_defineProperty(this, "tokens", void 0);
		_defineProperty(this, "options", void 0);
		_defineProperty(this, "state", void 0);
		_defineProperty(this, "inlineQueue", void 0);
		_defineProperty(this, "tokenizer", void 0);
		this.tokens = [], this.tokens.links = Object.create(null), this.options = e || O, this.options.tokenizer = this.options.tokenizer || new w(), this.tokenizer = this.options.tokenizer, this.tokenizer.options = this.options, this.tokenizer.lexer = this, this.inlineQueue = [], this.state = {
			inLink: !1,
			inRawBlock: !1,
			top: !0
		};
		let t = {
			other: m,
			block: B.normal,
			inline: E.normal
		};
		this.options.pedantic ? (t.block = B.pedantic, t.inline = E.pedantic) : this.options.gfm && (t.block = B.gfm, this.options.breaks ? t.inline = E.breaks : t.inline = E.gfm), this.tokenizer.rules = t;
	}
	static get rules() {
		return {
			block: B,
			inline: E
		};
	}
	static lex(e, t) {
		return new u(t).lex(e);
	}
	static lexInline(e, t) {
		return new u(t).inlineTokens(e);
	}
	lex(e) {
		e = e.replace(m.carriageReturn, `
`), this.blockTokens(e, this.tokens);
		for (let t = 0; t < this.inlineQueue.length; t++) {
			let n = this.inlineQueue[t];
			this.inlineTokens(n.src, n.tokens);
		}
		return this.inlineQueue = [], this.tokens;
	}
	blockTokens(e, t = [], n = !1) {
		for (this.tokenizer.lexer = this, this.options.pedantic && (e = e.replace(m.tabCharGlobal, "    ").replace(m.spaceLine, "")); e;) {
			let r;
			if (this.options.extensions?.block?.some((s) => (r = s.call({ lexer: this }, e, t)) ? (e = e.substring(r.raw.length), t.push(r), !0) : !1)) continue;
			if (r = this.tokenizer.space(e)) {
				e = e.substring(r.raw.length);
				let s = t.at(-1);
				r.raw.length === 1 && s !== void 0 ? s.raw += `
` : t.push(r);
				continue;
			}
			if (r = this.tokenizer.code(e)) {
				e = e.substring(r.raw.length);
				let s = t.at(-1);
				s?.type === "paragraph" || s?.type === "text" ? (s.raw += (s.raw.endsWith(`
`) ? "" : `
`) + r.raw, s.text += `
` + r.text, this.inlineQueue.at(-1).src = s.text) : t.push(r);
				continue;
			}
			if (r = this.tokenizer.fences(e)) {
				e = e.substring(r.raw.length), t.push(r);
				continue;
			}
			if (r = this.tokenizer.heading(e)) {
				e = e.substring(r.raw.length), t.push(r);
				continue;
			}
			if (r = this.tokenizer.hr(e)) {
				e = e.substring(r.raw.length), t.push(r);
				continue;
			}
			if (r = this.tokenizer.blockquote(e)) {
				e = e.substring(r.raw.length), t.push(r);
				continue;
			}
			if (r = this.tokenizer.list(e)) {
				e = e.substring(r.raw.length), t.push(r);
				continue;
			}
			if (r = this.tokenizer.html(e)) {
				e = e.substring(r.raw.length), t.push(r);
				continue;
			}
			if (r = this.tokenizer.def(e)) {
				e = e.substring(r.raw.length);
				let s = t.at(-1);
				s?.type === "paragraph" || s?.type === "text" ? (s.raw += (s.raw.endsWith(`
`) ? "" : `
`) + r.raw, s.text += `
` + r.raw, this.inlineQueue.at(-1).src = s.text) : this.tokens.links[r.tag] || (this.tokens.links[r.tag] = {
					href: r.href,
					title: r.title
				}, t.push(r));
				continue;
			}
			if (r = this.tokenizer.table(e)) {
				e = e.substring(r.raw.length), t.push(r);
				continue;
			}
			if (r = this.tokenizer.lheading(e)) {
				e = e.substring(r.raw.length), t.push(r);
				continue;
			}
			let i$1 = e;
			if (this.options.extensions?.startBlock) {
				let s = Infinity, a = e.slice(1), o;
				this.options.extensions.startBlock.forEach((l) => {
					o = l.call({ lexer: this }, a), typeof o == "number" && o >= 0 && (s = Math.min(s, o));
				}), s < Infinity && s >= 0 && (i$1 = e.substring(0, s + 1));
			}
			if (this.state.top && (r = this.tokenizer.paragraph(i$1))) {
				let s = t.at(-1);
				n && s?.type === "paragraph" ? (s.raw += (s.raw.endsWith(`
`) ? "" : `
`) + r.raw, s.text += `
` + r.text, this.inlineQueue.pop(), this.inlineQueue.at(-1).src = s.text) : t.push(r), n = i$1.length !== e.length, e = e.substring(r.raw.length);
				continue;
			}
			if (r = this.tokenizer.text(e)) {
				e = e.substring(r.raw.length);
				let s = t.at(-1);
				s?.type === "text" ? (s.raw += (s.raw.endsWith(`
`) ? "" : `
`) + r.raw, s.text += `
` + r.text, this.inlineQueue.pop(), this.inlineQueue.at(-1).src = s.text) : t.push(r);
				continue;
			}
			if (e) {
				let s = "Infinite loop on byte: " + e.charCodeAt(0);
				if (this.options.silent) {
					console.error(s);
					break;
				} else throw new Error(s);
			}
		}
		return this.state.top = !0, t;
	}
	inline(e, t = []) {
		return this.inlineQueue.push({
			src: e,
			tokens: t
		}), t;
	}
	inlineTokens(e, t = []) {
		this.tokenizer.lexer = this;
		let n = e, r = null;
		if (this.tokens.links) {
			let o = Object.keys(this.tokens.links);
			if (o.length > 0) for (; (r = this.tokenizer.rules.inline.reflinkSearch.exec(n)) !== null;) o.includes(r[0].slice(r[0].lastIndexOf("[") + 1, -1)) && (n = n.slice(0, r.index) + "[" + "a".repeat(r[0].length - 2) + "]" + n.slice(this.tokenizer.rules.inline.reflinkSearch.lastIndex));
		}
		for (; (r = this.tokenizer.rules.inline.anyPunctuation.exec(n)) !== null;) n = n.slice(0, r.index) + "++" + n.slice(this.tokenizer.rules.inline.anyPunctuation.lastIndex);
		let i$1;
		for (; (r = this.tokenizer.rules.inline.blockSkip.exec(n)) !== null;) i$1 = r[2] ? r[2].length : 0, n = n.slice(0, r.index + i$1) + "[" + "a".repeat(r[0].length - i$1 - 2) + "]" + n.slice(this.tokenizer.rules.inline.blockSkip.lastIndex);
		n = this.options.hooks?.emStrongMask?.call({ lexer: this }, n) ?? n;
		let s = !1, a = "";
		for (; e;) {
			s || (a = ""), s = !1;
			let o;
			if (this.options.extensions?.inline?.some((p) => (o = p.call({ lexer: this }, e, t)) ? (e = e.substring(o.raw.length), t.push(o), !0) : !1)) continue;
			if (o = this.tokenizer.escape(e)) {
				e = e.substring(o.raw.length), t.push(o);
				continue;
			}
			if (o = this.tokenizer.tag(e)) {
				e = e.substring(o.raw.length), t.push(o);
				continue;
			}
			if (o = this.tokenizer.link(e)) {
				e = e.substring(o.raw.length), t.push(o);
				continue;
			}
			if (o = this.tokenizer.reflink(e, this.tokens.links)) {
				e = e.substring(o.raw.length);
				let p = t.at(-1);
				o.type === "text" && p?.type === "text" ? (p.raw += o.raw, p.text += o.text) : t.push(o);
				continue;
			}
			if (o = this.tokenizer.emStrong(e, n, a)) {
				e = e.substring(o.raw.length), t.push(o);
				continue;
			}
			if (o = this.tokenizer.codespan(e)) {
				e = e.substring(o.raw.length), t.push(o);
				continue;
			}
			if (o = this.tokenizer.br(e)) {
				e = e.substring(o.raw.length), t.push(o);
				continue;
			}
			if (o = this.tokenizer.del(e, n, a)) {
				e = e.substring(o.raw.length), t.push(o);
				continue;
			}
			if (o = this.tokenizer.autolink(e)) {
				e = e.substring(o.raw.length), t.push(o);
				continue;
			}
			if (!this.state.inLink && (o = this.tokenizer.url(e))) {
				e = e.substring(o.raw.length), t.push(o);
				continue;
			}
			let l = e;
			if (this.options.extensions?.startInline) {
				let p = Infinity, c = e.slice(1), d;
				this.options.extensions.startInline.forEach((h$1) => {
					d = h$1.call({ lexer: this }, c), typeof d == "number" && d >= 0 && (p = Math.min(p, d));
				}), p < Infinity && p >= 0 && (l = e.substring(0, p + 1));
			}
			if (o = this.tokenizer.inlineText(l)) {
				e = e.substring(o.raw.length), o.raw.slice(-1) !== "_" && (a = o.raw.slice(-1)), s = !0;
				let p = t.at(-1);
				p?.type === "text" ? (p.raw += o.raw, p.text += o.text) : t.push(o);
				continue;
			}
			if (e) {
				let p = "Infinite loop on byte: " + e.charCodeAt(0);
				if (this.options.silent) {
					console.error(p);
					break;
				} else throw new Error(p);
			}
		}
		return t;
	}
};
var y = class {
	constructor(e) {
		_defineProperty(this, "options", void 0);
		_defineProperty(this, "parser", void 0);
		this.options = e || O;
	}
	space(e) {
		return "";
	}
	code({ text: e, lang: t, escaped: n }) {
		let r = (t || "").match(m.notSpaceStart)?.[0], i$1 = e.replace(m.endingNewline, "") + `
`;
		return r ? "<pre><code class=\"language-" + T(r) + "\">" + (n ? i$1 : T(i$1, !0)) + `</code></pre>
` : "<pre><code>" + (n ? i$1 : T(i$1, !0)) + `</code></pre>
`;
	}
	blockquote({ tokens: e }) {
		return `<blockquote>
${this.parser.parse(e)}</blockquote>
`;
	}
	html({ text: e }) {
		return e;
	}
	def(e) {
		return "";
	}
	heading({ tokens: e, depth: t }) {
		return `<h${t}>${this.parser.parseInline(e)}</h${t}>
`;
	}
	hr(e) {
		return `<hr>
`;
	}
	list(e) {
		let t = e.ordered, n = e.start, r = "";
		for (let a = 0; a < e.items.length; a++) {
			let o = e.items[a];
			r += this.listitem(o);
		}
		let i$1 = t ? "ol" : "ul", s = t && n !== 1 ? " start=\"" + n + "\"" : "";
		return "<" + i$1 + s + `>
` + r + "</" + i$1 + `>
`;
	}
	listitem(e) {
		return `<li>${this.parser.parse(e.tokens)}</li>
`;
	}
	checkbox({ checked: e }) {
		return "<input " + (e ? "checked=\"\" " : "") + "disabled=\"\" type=\"checkbox\"> ";
	}
	paragraph({ tokens: e }) {
		return `<p>${this.parser.parseInline(e)}</p>
`;
	}
	table(e) {
		let t = "", n = "";
		for (let i$1 = 0; i$1 < e.header.length; i$1++) n += this.tablecell(e.header[i$1]);
		t += this.tablerow({ text: n });
		let r = "";
		for (let i$1 = 0; i$1 < e.rows.length; i$1++) {
			let s = e.rows[i$1];
			n = "";
			for (let a = 0; a < s.length; a++) n += this.tablecell(s[a]);
			r += this.tablerow({ text: n });
		}
		return r && (r = `<tbody>${r}</tbody>`), `<table>
<thead>
` + t + `</thead>
` + r + `</table>
`;
	}
	tablerow({ text: e }) {
		return `<tr>
${e}</tr>
`;
	}
	tablecell(e) {
		let t = this.parser.parseInline(e.tokens), n = e.header ? "th" : "td";
		return (e.align ? `<${n} align="${e.align}">` : `<${n}>`) + t + `</${n}>
`;
	}
	strong({ tokens: e }) {
		return `<strong>${this.parser.parseInline(e)}</strong>`;
	}
	em({ tokens: e }) {
		return `<em>${this.parser.parseInline(e)}</em>`;
	}
	codespan({ text: e }) {
		return `<code>${T(e, !0)}</code>`;
	}
	br(e) {
		return "<br>";
	}
	del({ tokens: e }) {
		return `<del>${this.parser.parseInline(e)}</del>`;
	}
	link({ href: e, title: t, tokens: n }) {
		let r = this.parser.parseInline(n), i$1 = J(e);
		if (i$1 === null) return r;
		e = i$1;
		let s = "<a href=\"" + e + "\"";
		return t && (s += " title=\"" + T(t) + "\""), s += ">" + r + "</a>", s;
	}
	image({ href: e, title: t, text: n, tokens: r }) {
		r && (n = this.parser.parseInline(r, this.parser.textRenderer));
		let i$1 = J(e);
		if (i$1 === null) return T(n);
		e = i$1;
		let s = `<img src="${e}" alt="${T(n)}"`;
		return t && (s += ` title="${T(t)}"`), s += ">", s;
	}
	text(e) {
		return "tokens" in e && e.tokens ? this.parser.parseInline(e.tokens) : "escaped" in e && e.escaped ? e.text : T(e.text);
	}
};
var $ = class {
	strong({ text: e }) {
		return e;
	}
	em({ text: e }) {
		return e;
	}
	codespan({ text: e }) {
		return e;
	}
	del({ text: e }) {
		return e;
	}
	html({ text: e }) {
		return e;
	}
	text({ text: e }) {
		return e;
	}
	link({ text: e }) {
		return "" + e;
	}
	image({ text: e }) {
		return "" + e;
	}
	br() {
		return "";
	}
	checkbox({ raw: e }) {
		return e;
	}
};
var b = class u {
	constructor(e) {
		_defineProperty(this, "options", void 0);
		_defineProperty(this, "renderer", void 0);
		_defineProperty(this, "textRenderer", void 0);
		this.options = e || O, this.options.renderer = this.options.renderer || new y(), this.renderer = this.options.renderer, this.renderer.options = this.options, this.renderer.parser = this, this.textRenderer = new $();
	}
	static parse(e, t) {
		return new u(t).parse(e);
	}
	static parseInline(e, t) {
		return new u(t).parseInline(e);
	}
	parse(e) {
		this.renderer.parser = this;
		let t = "";
		for (let n = 0; n < e.length; n++) {
			let r = e[n];
			if (this.options.extensions?.renderers?.[r.type]) {
				let s = r, a = this.options.extensions.renderers[s.type].call({ parser: this }, s);
				if (a !== !1 || ![
					"space",
					"hr",
					"heading",
					"code",
					"table",
					"blockquote",
					"list",
					"html",
					"def",
					"paragraph",
					"text"
				].includes(s.type)) {
					t += a || "";
					continue;
				}
			}
			let i$1 = r;
			switch (i$1.type) {
				case "space":
					t += this.renderer.space(i$1);
					break;
				case "hr":
					t += this.renderer.hr(i$1);
					break;
				case "heading":
					t += this.renderer.heading(i$1);
					break;
				case "code":
					t += this.renderer.code(i$1);
					break;
				case "table":
					t += this.renderer.table(i$1);
					break;
				case "blockquote":
					t += this.renderer.blockquote(i$1);
					break;
				case "list":
					t += this.renderer.list(i$1);
					break;
				case "checkbox":
					t += this.renderer.checkbox(i$1);
					break;
				case "html":
					t += this.renderer.html(i$1);
					break;
				case "def":
					t += this.renderer.def(i$1);
					break;
				case "paragraph":
					t += this.renderer.paragraph(i$1);
					break;
				case "text":
					t += this.renderer.text(i$1);
					break;
				default: {
					let s = "Token with \"" + i$1.type + "\" type was not found.";
					if (this.options.silent) return console.error(s), "";
					throw new Error(s);
				}
			}
		}
		return t;
	}
	parseInline(e, t = this.renderer) {
		this.renderer.parser = this;
		let n = "";
		for (let r = 0; r < e.length; r++) {
			let i$1 = e[r];
			if (this.options.extensions?.renderers?.[i$1.type]) {
				let a = this.options.extensions.renderers[i$1.type].call({ parser: this }, i$1);
				if (a !== !1 || ![
					"escape",
					"html",
					"link",
					"image",
					"strong",
					"em",
					"codespan",
					"br",
					"del",
					"text"
				].includes(i$1.type)) {
					n += a || "";
					continue;
				}
			}
			let s = i$1;
			switch (s.type) {
				case "escape":
					n += t.text(s);
					break;
				case "html":
					n += t.html(s);
					break;
				case "link":
					n += t.link(s);
					break;
				case "image":
					n += t.image(s);
					break;
				case "checkbox":
					n += t.checkbox(s);
					break;
				case "strong":
					n += t.strong(s);
					break;
				case "em":
					n += t.em(s);
					break;
				case "codespan":
					n += t.codespan(s);
					break;
				case "br":
					n += t.br(s);
					break;
				case "del":
					n += t.del(s);
					break;
				case "text":
					n += t.text(s);
					break;
				default: {
					let a = "Token with \"" + s.type + "\" type was not found.";
					if (this.options.silent) return console.error(a), "";
					throw new Error(a);
				}
			}
		}
		return n;
	}
};
var P = (_Class = class {
	constructor(e) {
		_defineProperty(this, "options", void 0);
		_defineProperty(this, "block", void 0);
		this.options = e || O;
	}
	preprocess(e) {
		return e;
	}
	postprocess(e) {
		return e;
	}
	processAllTokens(e) {
		return e;
	}
	emStrongMask(e) {
		return e;
	}
	provideLexer(e = this.block) {
		return e ? x.lex : x.lexInline;
	}
	provideParser(e = this.block) {
		return e ? b.parse : b.parseInline;
	}
}, _defineProperty(_Class, "passThroughHooks", new Set([
	"preprocess",
	"postprocess",
	"processAllTokens",
	"emStrongMask"
])), _defineProperty(_Class, "passThroughHooksRespectAsync", new Set([
	"preprocess",
	"postprocess",
	"processAllTokens"
])), _Class);
var D = class {
	constructor(...e) {
		_defineProperty(this, "defaults", M());
		_defineProperty(this, "options", this.setOptions);
		_defineProperty(this, "parse", this.parseMarkdown(!0));
		_defineProperty(this, "parseInline", this.parseMarkdown(!1));
		_defineProperty(this, "Parser", b);
		_defineProperty(this, "Renderer", y);
		_defineProperty(this, "TextRenderer", $);
		_defineProperty(this, "Lexer", x);
		_defineProperty(this, "Tokenizer", w);
		_defineProperty(this, "Hooks", P);
		this.use(...e);
	}
	walkTokens(e, t) {
		let n = [];
		for (let r of e) switch (n = n.concat(t.call(this, r)), r.type) {
			case "table": {
				let i$1 = r;
				for (let s of i$1.header) n = n.concat(this.walkTokens(s.tokens, t));
				for (let s of i$1.rows) for (let a of s) n = n.concat(this.walkTokens(a.tokens, t));
				break;
			}
			case "list": {
				let i$1 = r;
				n = n.concat(this.walkTokens(i$1.items, t));
				break;
			}
			default: {
				let i$1 = r;
				this.defaults.extensions?.childTokens?.[i$1.type] ? this.defaults.extensions.childTokens[i$1.type].forEach((s) => {
					let a = i$1[s].flat(Infinity);
					n = n.concat(this.walkTokens(a, t));
				}) : i$1.tokens && (n = n.concat(this.walkTokens(i$1.tokens, t)));
			}
		}
		return n;
	}
	use(...e) {
		let t = this.defaults.extensions || {
			renderers: {},
			childTokens: {}
		};
		return e.forEach((n) => {
			let r = { ...n };
			if (r.async = this.defaults.async || r.async || !1, n.extensions && (n.extensions.forEach((i$1) => {
				if (!i$1.name) throw new Error("extension name required");
				if ("renderer" in i$1) {
					let s = t.renderers[i$1.name];
					s ? t.renderers[i$1.name] = function(...a) {
						let o = i$1.renderer.apply(this, a);
						return o === !1 && (o = s.apply(this, a)), o;
					} : t.renderers[i$1.name] = i$1.renderer;
				}
				if ("tokenizer" in i$1) {
					if (!i$1.level || i$1.level !== "block" && i$1.level !== "inline") throw new Error("extension level must be 'block' or 'inline'");
					let s = t[i$1.level];
					s ? s.unshift(i$1.tokenizer) : t[i$1.level] = [i$1.tokenizer], i$1.start && (i$1.level === "block" ? t.startBlock ? t.startBlock.push(i$1.start) : t.startBlock = [i$1.start] : i$1.level === "inline" && (t.startInline ? t.startInline.push(i$1.start) : t.startInline = [i$1.start]));
				}
				"childTokens" in i$1 && i$1.childTokens && (t.childTokens[i$1.name] = i$1.childTokens);
			}), r.extensions = t), n.renderer) {
				let i$1 = this.defaults.renderer || new y(this.defaults);
				for (let s in n.renderer) {
					if (!(s in i$1)) throw new Error(`renderer '${s}' does not exist`);
					if (["options", "parser"].includes(s)) continue;
					let a = s, o = n.renderer[a], l = i$1[a];
					i$1[a] = (...p) => {
						let c = o.apply(i$1, p);
						return c === !1 && (c = l.apply(i$1, p)), c || "";
					};
				}
				r.renderer = i$1;
			}
			if (n.tokenizer) {
				let i$1 = this.defaults.tokenizer || new w(this.defaults);
				for (let s in n.tokenizer) {
					if (!(s in i$1)) throw new Error(`tokenizer '${s}' does not exist`);
					if ([
						"options",
						"rules",
						"lexer"
					].includes(s)) continue;
					let a = s, o = n.tokenizer[a], l = i$1[a];
					i$1[a] = (...p) => {
						let c = o.apply(i$1, p);
						return c === !1 && (c = l.apply(i$1, p)), c;
					};
				}
				r.tokenizer = i$1;
			}
			if (n.hooks) {
				let i$1 = this.defaults.hooks || new P();
				for (let s in n.hooks) {
					if (!(s in i$1)) throw new Error(`hook '${s}' does not exist`);
					if (["options", "block"].includes(s)) continue;
					let a = s, o = n.hooks[a], l = i$1[a];
					P.passThroughHooks.has(s) ? i$1[a] = (p) => {
						if (this.defaults.async && P.passThroughHooksRespectAsync.has(s)) return (async () => {
							let d = await o.call(i$1, p);
							return l.call(i$1, d);
						})();
						let c = o.call(i$1, p);
						return l.call(i$1, c);
					} : i$1[a] = (...p) => {
						if (this.defaults.async) return (async () => {
							let d = await o.apply(i$1, p);
							return d === !1 && (d = await l.apply(i$1, p)), d;
						})();
						let c = o.apply(i$1, p);
						return c === !1 && (c = l.apply(i$1, p)), c;
					};
				}
				r.hooks = i$1;
			}
			if (n.walkTokens) {
				let i$1 = this.defaults.walkTokens, s = n.walkTokens;
				r.walkTokens = function(a) {
					let o = [];
					return o.push(s.call(this, a)), i$1 && (o = o.concat(i$1.call(this, a))), o;
				};
			}
			this.defaults = {
				...this.defaults,
				...r
			};
		}), this;
	}
	setOptions(e) {
		return this.defaults = {
			...this.defaults,
			...e
		}, this;
	}
	lexer(e, t) {
		return x.lex(e, t ?? this.defaults);
	}
	parser(e, t) {
		return b.parse(e, t ?? this.defaults);
	}
	parseMarkdown(e) {
		return (n, r) => {
			let i$1 = { ...r }, s = {
				...this.defaults,
				...i$1
			}, a = this.onError(!!s.silent, !!s.async);
			if (this.defaults.async === !0 && i$1.async === !1) return a(/* @__PURE__ */ new Error("marked(): The async option was set to true by an extension. Remove async: false from the parse options object to return a Promise."));
			if (typeof n > "u" || n === null) return a(/* @__PURE__ */ new Error("marked(): input parameter is undefined or null"));
			if (typeof n != "string") return a(/* @__PURE__ */ new Error("marked(): input parameter is of type " + Object.prototype.toString.call(n) + ", string expected"));
			if (s.hooks && (s.hooks.options = s, s.hooks.block = e), s.async) return (async () => {
				let o = s.hooks ? await s.hooks.preprocess(n) : n, p = await (s.hooks ? await s.hooks.provideLexer(e) : e ? x.lex : x.lexInline)(o, s), c = s.hooks ? await s.hooks.processAllTokens(p) : p;
				s.walkTokens && await Promise.all(this.walkTokens(c, s.walkTokens));
				let h$1 = await (s.hooks ? await s.hooks.provideParser(e) : e ? b.parse : b.parseInline)(c, s);
				return s.hooks ? await s.hooks.postprocess(h$1) : h$1;
			})().catch(a);
			try {
				s.hooks && (n = s.hooks.preprocess(n));
				let l = (s.hooks ? s.hooks.provideLexer(e) : e ? x.lex : x.lexInline)(n, s);
				s.hooks && (l = s.hooks.processAllTokens(l)), s.walkTokens && this.walkTokens(l, s.walkTokens);
				let c = (s.hooks ? s.hooks.provideParser(e) : e ? b.parse : b.parseInline)(l, s);
				return s.hooks && (c = s.hooks.postprocess(c)), c;
			} catch (o) {
				return a(o);
			}
		};
	}
	onError(e, t) {
		return (n) => {
			if (n.message += `
Please report this to https://github.com/markedjs/marked.`, e) {
				let r = "<p>An error occurred:</p><pre>" + T(n.message + "", !0) + "</pre>";
				return t ? Promise.resolve(r) : r;
			}
			if (t) return Promise.reject(n);
			throw n;
		};
	}
};
var L = new D();
function g(u, e) {
	return L.parse(u, e);
}
g.options = g.setOptions = function(u) {
	return L.setOptions(u), g.defaults = L.defaults, G(g.defaults), g;
};
g.getDefaults = M;
g.defaults = O;
g.use = function(...u) {
	return L.use(...u), g.defaults = L.defaults, G(g.defaults), g;
};
g.walkTokens = function(u, e) {
	return L.walkTokens(u, e);
};
g.parseInline = L.parseInline;
g.Parser = b;
g.parser = b.parse;
g.Renderer = y;
g.TextRenderer = $;
g.Lexer = x;
g.lexer = x.lex;
g.Tokenizer = w;
g.Hooks = P;
g.parse = g;
g.options;
g.setOptions;
g.use;
g.walkTokens;
g.parseInline;
b.parse;
x.lex;
function wrapInMarkdownBlock(prefix, content) {
	const output = content.split("\n").flatMap((line) => [line, ""]).map((line) => `${prefix}${line}`).join("\n");
	return output.slice(0, output.length - 1);
}
function findMarksToClose(currentMarks, nextNode) {
	const marksToClose = [];
	Array.from(currentMarks.keys()).forEach((markType) => {
		if (!nextNode || !nextNode.marks || !nextNode.marks.map((mark) => mark.type).includes(markType)) marksToClose.push(markType);
	});
	return marksToClose;
}
function findMarksToOpen(activeMarks, currentMarks) {
	const marksToOpen = [];
	Array.from(currentMarks.entries()).forEach(([markType, mark]) => {
		if (!activeMarks.has(markType)) marksToOpen.push({
			type: markType,
			mark
		});
	});
	return marksToOpen;
}
function findMarksToCloseAtEnd(activeMarks, currentMarks, nextNode, markSetsEqual) {
	const isLastNode = !nextNode;
	const nextNodeHasNoMarks = nextNode && nextNode.type === "text" && (!nextNode.marks || nextNode.marks.length === 0);
	const nextNodeHasDifferentMarks = nextNode && nextNode.type === "text" && nextNode.marks && !markSetsEqual(currentMarks, new Map(nextNode.marks.map((mark) => [mark.type, mark])));
	const marksToCloseAtEnd = [];
	if (isLastNode || nextNodeHasNoMarks || nextNodeHasDifferentMarks) {
		if (nextNode && nextNode.type === "text" && nextNode.marks) {
			const nextMarks = new Map(nextNode.marks.map((mark) => [mark.type, mark]));
			Array.from(activeMarks.keys()).reverse().forEach((markType) => {
				if (!nextMarks.has(markType)) marksToCloseAtEnd.push(markType);
			});
		} else if (isLastNode || nextNodeHasNoMarks) marksToCloseAtEnd.push(...Array.from(activeMarks.keys()).reverse());
	}
	return marksToCloseAtEnd;
}
function closeMarksBeforeNode(activeMarks, getMarkClosing) {
	let beforeMarkdown = "";
	Array.from(activeMarks.keys()).reverse().forEach((markType) => {
		const closeMarkdown = getMarkClosing(markType, activeMarks.get(markType));
		if (closeMarkdown) beforeMarkdown = closeMarkdown + beforeMarkdown;
	});
	activeMarks.clear();
	return beforeMarkdown;
}
function reopenMarksAfterNode(marksToReopen, activeMarks, getMarkOpening) {
	let afterMarkdown = "";
	Array.from(marksToReopen.entries()).forEach(([markType, mark]) => {
		const openMarkdown = getMarkOpening(markType, mark);
		if (openMarkdown) afterMarkdown += openMarkdown;
		activeMarks.set(markType, mark);
	});
	return afterMarkdown;
}
function isTaskItem(item) {
	const match = (item.raw || item.text || "").match(/^(\s*)[-+*]\s+\[([ xX])\]\s+/);
	if (match) return {
		isTask: true,
		checked: match[2].toLowerCase() === "x",
		indentLevel: match[1].length
	};
	return {
		isTask: false,
		indentLevel: 0
	};
}
function assumeContentType(content, contentType) {
	if (typeof content !== "string") return "json";
	return contentType;
}
var MarkdownManager = class {
	constructor(options) {
		this.activeParseLexer = null;
		this.baseExtensions = [];
		this.extensions = [];
		this.codeTypes = /* @__PURE__ */ new Set();
		this.lastParseResult = null;
		var _a, _b, _c, _d, _e$1;
		this.markedInstance = (_a = options == null ? void 0 : options.marked) != null ? _a : g;
		this.indentStyle = (_c = (_b = options == null ? void 0 : options.indentation) == null ? void 0 : _b.style) != null ? _c : "space";
		this.indentSize = (_e$1 = (_d = options == null ? void 0 : options.indentation) == null ? void 0 : _d.size) != null ? _e$1 : 2;
		this.baseExtensions = (options == null ? void 0 : options.extensions) || [];
		if ((options == null ? void 0 : options.markedOptions) && typeof this.markedInstance.setOptions === "function") this.markedInstance.setOptions(options.markedOptions);
		this.registry = /* @__PURE__ */ new Map();
		this.nodeTypeRegistry = /* @__PURE__ */ new Map();
		if (options == null ? void 0 : options.extensions) {
			this.baseExtensions = options.extensions;
			flattenExtensions(options.extensions).forEach((ext) => this.registerExtension(ext));
		}
	}
	get instance() {
		return this.markedInstance;
	}
	get indentCharacter() {
		return this.indentStyle === "space" ? " " : "	";
	}
	get indentString() {
		return this.indentCharacter.repeat(this.indentSize);
	}
	hasMarked() {
		return !!this.markedInstance;
	}
	registerExtension(extension) {
		var _a, _b;
		this.extensions.push(extension);
		const isCode = callOrReturn(getExtensionField(extension, "code"));
		const name = extension.name;
		if (isCode) this.codeTypes.add(name);
		const tokenName = getExtensionField(extension, "markdownTokenName") || name;
		const parseMarkdown = getExtensionField(extension, "parseMarkdown");
		const renderMarkdown = getExtensionField(extension, "renderMarkdown");
		const tokenizer = getExtensionField(extension, "markdownTokenizer");
		const markdownCfg = (_a = getExtensionField(extension, "markdownOptions")) != null ? _a : null;
		const spec = {
			tokenName,
			nodeName: name,
			parseMarkdown,
			renderMarkdown,
			isIndenting: (_b = markdownCfg == null ? void 0 : markdownCfg.indentsContent) != null ? _b : false,
			htmlReopen: markdownCfg == null ? void 0 : markdownCfg.htmlReopen,
			tokenizer
		};
		if (tokenName && parseMarkdown) {
			const parseExisting = this.registry.get(tokenName) || [];
			parseExisting.push(spec);
			this.registry.set(tokenName, parseExisting);
		}
		if (renderMarkdown) {
			const renderExisting = this.nodeTypeRegistry.get(name) || [];
			renderExisting.push(spec);
			this.nodeTypeRegistry.set(name, renderExisting);
		}
		if (tokenizer && this.hasMarked()) this.registerTokenizer(tokenizer);
	}
	createLexer() {
		return new this.markedInstance.Lexer();
	}
	createTokenizerHelpers(lexer) {
		return {
			inlineTokens: (src) => lexer.inlineTokens(src),
			blockTokens: (src) => lexer.blockTokens(src)
		};
	}
	tokenizeInline(src) {
		var _a;
		return ((_a = this.activeParseLexer) != null ? _a : this.createLexer()).inlineTokens(src);
	}
	registerTokenizer(tokenizer) {
		if (!this.hasMarked()) return;
		const { name, start, level = "inline", tokenize: tokenize$1 } = tokenizer;
		const createTokenizerHelpers = this.createTokenizerHelpers.bind(this);
		const createLexer = this.createLexer.bind(this);
		let startCb;
		if (!start) startCb = (src) => {
			const result = tokenize$1(src, [], this.createTokenizerHelpers(this.createLexer()));
			if (result && result.raw) return src.indexOf(result.raw);
			return -1;
		};
		else startCb = typeof start === "function" ? start : (src) => src.indexOf(start);
		const markedExtension = {
			name,
			level,
			start: startCb,
			tokenizer(src, tokens) {
				const result = tokenize$1(src, tokens, this.lexer ? createTokenizerHelpers(this.lexer) : createTokenizerHelpers(createLexer()));
				if (result && result.type) return {
					...result,
					type: result.type || name,
					raw: result.raw || "",
					tokens: result.tokens || []
				};
			},
			childTokens: []
		};
		this.markedInstance.use({ extensions: [markedExtension] });
	}
	getHandlersForToken(type) {
		try {
			return this.registry.get(type) || [];
		} catch {
			return [];
		}
	}
	getHandlerForToken(type) {
		const markdownHandlers = this.getHandlersForToken(type);
		if (markdownHandlers.length > 0) return markdownHandlers[0];
		const nodeTypeHandlers = this.getHandlersForNodeType(type);
		return nodeTypeHandlers.length > 0 ? nodeTypeHandlers[0] : void 0;
	}
	getHandlersForNodeType(type) {
		try {
			return this.nodeTypeRegistry.get(type) || [];
		} catch {
			return [];
		}
	}
	serialize(docOrContent) {
		if (!docOrContent) return "";
		const result = this.renderNodes(docOrContent, docOrContent);
		return this.isEmptyOutput(result) ? "" : result;
	}
	isEmptyOutput(markdown) {
		if (!markdown || markdown.trim() === "") return true;
		return markdown.replace(/&nbsp;/g, "").replace(/\u00A0/g, "").trim() === "";
	}
	parse(markdown) {
		if (!this.hasMarked()) throw new Error("No marked instance available for parsing");
		const previousParseLexer = this.activeParseLexer;
		const parseLexer = this.createLexer();
		this.activeParseLexer = parseLexer;
		try {
			const tokens = parseLexer.lex(markdown);
			return {
				type: "doc",
				content: this.parseTokens(tokens, true)
			};
		} finally {
			this.activeParseLexer = previousParseLexer;
		}
	}
	parseTokens(tokens, parseImplicitEmptyParagraphs = false) {
		const nonSpaceTokenIndexes = tokens.reduce((indexes, token, index) => {
			if (token.type !== "space") indexes.push(index);
			return indexes;
		}, []);
		let previousNonSpaceTokenIndex = -1;
		let nextNonSpaceTokenPointer = 0;
		return tokens.flatMap((token, index) => {
			var _a;
			while (nextNonSpaceTokenPointer < nonSpaceTokenIndexes.length && nonSpaceTokenIndexes[nextNonSpaceTokenPointer] < index) {
				previousNonSpaceTokenIndex = nonSpaceTokenIndexes[nextNonSpaceTokenPointer];
				nextNonSpaceTokenPointer += 1;
			}
			if (parseImplicitEmptyParagraphs && token.type === "space") {
				const nextNonSpaceTokenIndex = (_a = nonSpaceTokenIndexes[nextNonSpaceTokenPointer]) != null ? _a : -1;
				return this.createImplicitEmptyParagraphsFromSpace(token, previousNonSpaceTokenIndex, nextNonSpaceTokenIndex);
			}
			const parsed = this.parseToken(token, parseImplicitEmptyParagraphs);
			if (parsed === null) return [];
			return Array.isArray(parsed) ? parsed : [parsed];
		});
	}
	createImplicitEmptyParagraphsFromSpace(token, previousNonSpaceTokenIndex, nextNonSpaceTokenIndex) {
		const separatorCount = this.countParagraphSeparators(token.raw || "");
		if (separatorCount === 0) return [];
		const isBoundarySpace = previousNonSpaceTokenIndex === -1 || nextNonSpaceTokenIndex === -1;
		const emptyParagraphCount = Math.max(separatorCount - (isBoundarySpace ? 0 : 1), 0);
		return Array.from({ length: emptyParagraphCount }, () => ({
			type: "paragraph",
			content: []
		}));
	}
	countParagraphSeparators(raw) {
		return (raw.replace(/\r\n/g, "\n").match(/\n\n/g) || []).length;
	}
	parseToken(token, parseImplicitEmptyParagraphs = false) {
		if (!token.type) return null;
		if (token.type === "list") return this.parseListToken(token);
		const handlers$1 = this.getHandlersForToken(token.type);
		const helpers = this.createParseHelpers();
		if (handlers$1.find((handler) => {
			if (!handler.parseMarkdown) return false;
			const parseResult = handler.parseMarkdown(token, helpers);
			const normalized = this.normalizeParseResult(parseResult);
			if (normalized && (!Array.isArray(normalized) || normalized.length > 0)) {
				this.lastParseResult = normalized;
				return true;
			}
			return false;
		}) && this.lastParseResult) {
			const toReturn = this.lastParseResult;
			this.lastParseResult = null;
			return toReturn;
		}
		return this.parseFallbackToken(token, parseImplicitEmptyParagraphs);
	}
	parseListToken(token) {
		if (!token.items || token.items.length === 0) return this.parseTokenWithHandlers(token);
		const hasTask = token.items.some((item) => isTaskItem(item).isTask);
		const hasNonTask = token.items.some((item) => !isTaskItem(item).isTask);
		if (!hasTask || !hasNonTask || this.getHandlersForToken("taskList").length === 0) return this.parseTokenWithHandlers(token);
		const groups = [];
		let currentGroup = [];
		let currentType = null;
		for (let i$1 = 0; i$1 < token.items.length; i$1 += 1) {
			const item = token.items[i$1];
			const { isTask, checked, indentLevel } = isTaskItem(item);
			let processedItem = item;
			if (isTask) {
				const lines = (item.raw || item.text || "").split("\n");
				const firstLineMatch = lines[0].match(/^\s*[-+*]\s+\[([ xX])\]\s+(.*)$/);
				const mainContent = firstLineMatch ? firstLineMatch[2] : "";
				let nestedTokens = [];
				if (lines.length > 1) {
					if (lines.slice(1).join("\n").trim()) {
						const nestedLines = lines.slice(1);
						const nonEmptyLines = nestedLines.filter((line) => line.trim());
						if (nonEmptyLines.length > 0) {
							const minIndent = Math.min(...nonEmptyLines.map((line) => line.length - line.trimStart().length));
							const nestedContent = nestedLines.map((line) => {
								if (!line.trim()) return "";
								return line.slice(minIndent);
							}).join("\n").trim();
							if (nestedContent) nestedTokens = this.markedInstance.lexer(`${nestedContent}
`);
						}
					}
				}
				processedItem = {
					type: "taskItem",
					raw: "",
					mainContent,
					indentLevel,
					checked: checked != null ? checked : false,
					text: mainContent,
					tokens: this.tokenizeInline(mainContent),
					nestedTokens
				};
			}
			const itemType = isTask ? "taskList" : "list";
			if (currentType !== itemType) {
				if (currentGroup.length > 0) groups.push({
					type: currentType,
					items: currentGroup
				});
				currentGroup = [processedItem];
				currentType = itemType;
			} else currentGroup.push(processedItem);
		}
		if (currentGroup.length > 0) groups.push({
			type: currentType,
			items: currentGroup
		});
		const results = [];
		for (let i$1 = 0; i$1 < groups.length; i$1 += 1) {
			const group = groups[i$1];
			const subToken = {
				...token,
				type: group.type,
				items: group.items
			};
			const parsed = this.parseToken(subToken);
			if (parsed) if (Array.isArray(parsed)) results.push(...parsed);
			else results.push(parsed);
		}
		return results.length > 0 ? results : null;
	}
	parseTokenWithHandlers(token) {
		if (!token.type) return null;
		const handlers$1 = this.getHandlersForToken(token.type);
		const helpers = this.createParseHelpers();
		if (handlers$1.find((handler) => {
			if (!handler.parseMarkdown) return false;
			const parseResult = handler.parseMarkdown(token, helpers);
			const normalized = this.normalizeParseResult(parseResult);
			if (normalized && (!Array.isArray(normalized) || normalized.length > 0)) {
				this.lastParseResult = normalized;
				return true;
			}
			return false;
		}) && this.lastParseResult) {
			const toReturn = this.lastParseResult;
			this.lastParseResult = null;
			return toReturn;
		}
		return this.parseFallbackToken(token);
	}
	createParseHelpers() {
		return {
			parseInline: (tokens) => this.parseInlineTokens(tokens),
			parseChildren: (tokens) => this.parseTokens(tokens),
			parseBlockChildren: (tokens) => this.parseTokens(tokens, true),
			createTextNode: (text, marks) => {
				return {
					type: "text",
					text,
					marks: marks || void 0
				};
			},
			createNode: (type, attrs, content) => {
				const node = {
					type,
					attrs: attrs || void 0,
					content: content || void 0
				};
				if (!attrs || Object.keys(attrs).length === 0) delete node.attrs;
				return node;
			},
			applyMark: (markType, content, attrs) => ({
				mark: markType,
				content,
				attrs: attrs && Object.keys(attrs).length > 0 ? attrs : void 0
			})
		};
	}
	escapeRegex(str) {
		return str.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
	}
	parseInlineTokens(tokens) {
		var _a, _b, _c, _d;
		const result = [];
		for (let i$1 = 0; i$1 < tokens.length; i$1 += 1) {
			const token = tokens[i$1];
			if (token.type === "text") result.push({
				type: "text",
				text: decodeHtmlEntities(token.text || "")
			});
			else if (token.type === "html") {
				const raw = ((_b = (_a = token.raw) != null ? _a : token.text) != null ? _b : "").toString();
				const isClosing = /^<\/[\s]*[\w-]+/i.test(raw);
				const openMatch = raw.match(/^<[\s]*([\w-]+)(\s|>|\/|$)/i);
				if (!isClosing && openMatch && !/\/>$/.test(raw)) {
					const tagName = openMatch[1];
					const escapedTagName = this.escapeRegex(tagName);
					const closingRegex = new RegExp(`^<\\/\\s*${escapedTagName}\\b`, "i");
					let foundIndex = -1;
					const parts = [raw];
					for (let j$1 = i$1 + 1; j$1 < tokens.length; j$1 += 1) {
						const t = tokens[j$1];
						const tRaw = ((_d = (_c = t.raw) != null ? _c : t.text) != null ? _d : "").toString();
						parts.push(tRaw);
						if (t.type === "html" && closingRegex.test(tRaw)) {
							foundIndex = j$1;
							break;
						}
					}
					if (foundIndex !== -1) {
						const mergedRaw = parts.join("");
						const mergedToken = {
							type: "html",
							raw: mergedRaw,
							text: mergedRaw,
							block: false
						};
						const parsed = this.parseHTMLToken(mergedToken);
						if (parsed) {
							const normalized = this.normalizeParseResult(parsed);
							if (Array.isArray(normalized)) result.push(...normalized);
							else if (normalized) result.push(normalized);
						}
						i$1 = foundIndex;
						continue;
					}
				}
				const parsedSingle = this.parseHTMLToken(token);
				if (parsedSingle) {
					const normalized = this.normalizeParseResult(parsedSingle);
					if (Array.isArray(normalized)) result.push(...normalized);
					else if (normalized) result.push(normalized);
				}
			} else if (token.type) {
				const markHandler = this.getHandlerForToken(token.type);
				if (markHandler && markHandler.parseMarkdown) {
					const helpers = this.createParseHelpers();
					const parsed = markHandler.parseMarkdown(token, helpers);
					if (this.isMarkResult(parsed)) {
						const markedContent = this.applyMarkToContent(parsed.mark, parsed.content, parsed.attrs);
						result.push(...markedContent);
					} else {
						const normalized = this.normalizeParseResult(parsed);
						if (Array.isArray(normalized)) result.push(...normalized);
						else if (normalized) result.push(normalized);
					}
				} else if (token.tokens) result.push(...this.parseInlineTokens(token.tokens));
			}
		}
		return result;
	}
	applyMarkToContent(markType, content, attrs) {
		return content.map((node) => {
			if (node.type === "text") {
				const existingMarks = node.marks || [];
				const newMark = attrs ? {
					type: markType,
					attrs
				} : { type: markType };
				return {
					...node,
					marks: [...existingMarks, newMark]
				};
			}
			return {
				...node,
				content: node.content ? this.applyMarkToContent(markType, node.content, attrs) : void 0
			};
		});
	}
	isMarkResult(result) {
		return result && typeof result === "object" && "mark" in result;
	}
	normalizeParseResult(result) {
		if (!result) return null;
		if (this.isMarkResult(result)) return result.content;
		return result;
	}
	parseFallbackToken(token, parseImplicitEmptyParagraphs = false) {
		switch (token.type) {
			case "paragraph": return {
				type: "paragraph",
				content: token.tokens ? this.parseInlineTokens(token.tokens) : []
			};
			case "heading": return {
				type: "heading",
				attrs: { level: token.depth || 1 },
				content: token.tokens ? this.parseInlineTokens(token.tokens) : []
			};
			case "text": return {
				type: "text",
				text: decodeHtmlEntities(token.text || "")
			};
			case "html": return this.parseHTMLToken(token);
			case "space": return null;
			default:
				if (token.tokens) return this.parseTokens(token.tokens, parseImplicitEmptyParagraphs);
				return null;
		}
	}
	parseHTMLToken(token) {
		const html = token.text || token.raw || "";
		if (!html.trim()) return null;
		if (typeof window === "undefined") {
			if (token.block) return {
				type: "paragraph",
				content: [{
					type: "text",
					text: html
				}]
			};
			return {
				type: "text",
				text: html
			};
		}
		try {
			const parsed = generateJSON(html, this.baseExtensions);
			if (parsed.type === "doc" && parsed.content) {
				if (token.block) return parsed.content;
				if (parsed.content.length === 1 && parsed.content[0].type === "paragraph" && parsed.content[0].content) return parsed.content[0].content;
				return parsed.content;
			}
			return parsed;
		} catch (error) {
			throw new Error(`Failed to parse HTML in markdown: ${error}`);
		}
	}
	encodeTextForMarkdown(text, node, parentNode$1) {
		return (parentNode$1 == null ? void 0 : parentNode$1.type) != null && this.codeTypes.has(parentNode$1.type) || (node.marks || []).some((m$1) => this.codeTypes.has(typeof m$1 === "string" ? m$1 : m$1.type)) ? text : encodeHtmlEntities(text);
	}
	renderNodeToMarkdown(node, parentNode$1, index = 0, level = 0, meta = {}) {
		var _a;
		if (node.type === "text") return this.encodeTextForMarkdown(node.text || "", node, parentNode$1);
		if (!node.type) return "";
		const handler = this.getHandlerForToken(node.type);
		if (!handler) return "";
		const previousNode = Array.isArray(parentNode$1 == null ? void 0 : parentNode$1.content) && index > 0 ? parentNode$1.content[index - 1] : void 0;
		const helpers = {
			renderChildren: (nodes, separator) => {
				const childLevel = handler.isIndenting ? level + 1 : level;
				if (!Array.isArray(nodes) && nodes.content) return this.renderNodes(nodes.content, node, separator || "", index, childLevel);
				return this.renderNodes(nodes, node, separator || "", index, childLevel);
			},
			renderChild: (childNode, childIndex) => {
				const childLevel = handler.isIndenting ? level + 1 : level;
				return this.renderNodeToMarkdown(childNode, node, childIndex, childLevel);
			},
			indent: (content) => {
				return this.indentString + content;
			},
			wrapInBlock: wrapInMarkdownBlock
		};
		const context = {
			index,
			level,
			parentType: parentNode$1 == null ? void 0 : parentNode$1.type,
			previousNode,
			meta: {
				parentAttrs: parentNode$1 == null ? void 0 : parentNode$1.attrs,
				...meta
			}
		};
		return ((_a = handler.renderMarkdown) == null ? void 0 : _a.call(handler, node, helpers, context)) || "";
	}
	renderNodes(nodeOrNodes, parentNode$1, separator = "", index = 0, level = 0) {
		if (!Array.isArray(nodeOrNodes)) {
			if (!nodeOrNodes.type) return "";
			return this.renderNodeToMarkdown(nodeOrNodes, parentNode$1, index, level);
		}
		return this.renderNodesWithMarkBoundaries(nodeOrNodes, parentNode$1, separator, level);
	}
	renderNodesWithMarkBoundaries(nodes, parentNode$1, separator = "", level = 0) {
		const result = [];
		const activeMarks = /* @__PURE__ */ new Map();
		const reopenWithHtmlOnNextOpen = /* @__PURE__ */ new Set();
		const markOpeningModes = /* @__PURE__ */ new Map();
		nodes.forEach((node, i$1) => {
			const nextNode = i$1 < nodes.length - 1 ? nodes[i$1 + 1] : null;
			if (!node.type) return;
			if (node.type === "text") {
				let textContent = this.encodeTextForMarkdown(node.text || "", node, parentNode$1);
				const currentMarks = new Map((node.marks || []).map((mark) => [mark.type, mark]));
				const marksToOpen = findMarksToOpen(activeMarks, currentMarks);
				const marksToClose = findMarksToClose(currentMarks, nextNode);
				const activeMarksClosingHere = marksToClose.filter((markType) => activeMarks.has(markType));
				const hasCrossedBoundary = activeMarksClosingHere.length > 0 && marksToOpen.length > 0;
				let middleTrailingWhitespace = "";
				if (marksToClose.length > 0 && !hasCrossedBoundary) {
					const middleTrailingMatch = textContent.match(/(\s+)$/);
					if (middleTrailingMatch) {
						middleTrailingWhitespace = middleTrailingMatch[1];
						textContent = textContent.slice(0, -middleTrailingWhitespace.length);
					}
				}
				if (!hasCrossedBoundary) marksToClose.forEach((markType) => {
					if (!activeMarks.has(markType)) return;
					const mark = currentMarks.get(markType);
					const closeMarkdown = this.getMarkClosing(markType, mark, markOpeningModes.get(markType));
					if (closeMarkdown) textContent += closeMarkdown;
					if (activeMarks.has(markType)) {
						activeMarks.delete(markType);
						markOpeningModes.delete(markType);
					}
				});
				let leadingWhitespace = "";
				if (marksToOpen.length > 0) {
					const leadingMatch = textContent.match(/^(\s+)/);
					if (leadingMatch) {
						leadingWhitespace = leadingMatch[1];
						textContent = textContent.slice(leadingWhitespace.length);
					}
				}
				marksToOpen.forEach(({ type, mark }) => {
					const openingMode = reopenWithHtmlOnNextOpen.has(type) ? "html" : "markdown";
					const openMarkdown = this.getMarkOpening(type, mark, openingMode);
					if (openMarkdown) textContent = openMarkdown + textContent;
					markOpeningModes.set(type, openingMode);
					reopenWithHtmlOnNextOpen.delete(type);
				});
				if (!hasCrossedBoundary) marksToOpen.slice().reverse().forEach(({ type, mark }) => {
					activeMarks.set(type, mark);
				});
				textContent = leadingWhitespace + textContent;
				let marksToCloseAtEnd;
				if (hasCrossedBoundary) {
					const nextMarkTypes = new Set(((nextNode == null ? void 0 : nextNode.marks) || []).map((mark) => mark.type));
					marksToOpen.forEach(({ type }) => {
						if (nextMarkTypes.has(type) && this.getHtmlReopenTags(type)) reopenWithHtmlOnNextOpen.add(type);
					});
					marksToCloseAtEnd = [...marksToOpen.map((m$1) => m$1.type), ...activeMarksClosingHere];
				} else marksToCloseAtEnd = findMarksToCloseAtEnd(activeMarks, currentMarks, nextNode, this.markSetsEqual.bind(this));
				let trailingWhitespace = "";
				if (marksToCloseAtEnd.length > 0) {
					const trailingMatch = textContent.match(/(\s+)$/);
					if (trailingMatch) {
						trailingWhitespace = trailingMatch[1];
						textContent = textContent.slice(0, -trailingWhitespace.length);
					}
				}
				marksToCloseAtEnd.forEach((markType) => {
					var _a;
					const mark = (_a = activeMarks.get(markType)) != null ? _a : currentMarks.get(markType);
					const closeMarkdown = this.getMarkClosing(markType, mark, markOpeningModes.get(markType));
					if (closeMarkdown) textContent += closeMarkdown;
					activeMarks.delete(markType);
					markOpeningModes.delete(markType);
				});
				textContent += trailingWhitespace;
				textContent += middleTrailingWhitespace;
				result.push(textContent);
			} else {
				const marksToReopen = new Map(activeMarks);
				const openingModesToReopen = new Map(markOpeningModes);
				const beforeMarkdown = closeMarksBeforeNode(activeMarks, (markType, mark) => {
					return this.getMarkClosing(markType, mark, markOpeningModes.get(markType));
				});
				markOpeningModes.clear();
				const nodeContent = this.renderNodeToMarkdown(node, parentNode$1, i$1, level);
				const afterMarkdown = node.type === "hardBreak" ? "" : reopenMarksAfterNode(marksToReopen, activeMarks, (markType, mark) => {
					var _a;
					const openingMode = (_a = openingModesToReopen.get(markType)) != null ? _a : "markdown";
					markOpeningModes.set(markType, openingMode);
					return this.getMarkOpening(markType, mark, openingMode);
				});
				result.push(beforeMarkdown + nodeContent + afterMarkdown);
			}
		});
		return result.join(separator);
	}
	getMarkOpening(markType, mark, openingMode = "markdown") {
		var _a;
		if (openingMode === "html") return ((_a = this.getHtmlReopenTags(markType)) == null ? void 0 : _a.open) || "";
		const handlers$1 = this.getHandlersForNodeType(markType);
		const handler = handlers$1.length > 0 ? handlers$1[0] : void 0;
		if (!handler || !handler.renderMarkdown) return "";
		const placeholder = "__TIPTAP_MARKDOWN_PLACEHOLDER__";
		const syntheticNode = {
			type: markType,
			attrs: mark.attrs || {},
			content: [{
				type: "text",
				text: placeholder
			}]
		};
		try {
			const rendered = handler.renderMarkdown(syntheticNode, {
				renderChildren: () => placeholder,
				renderChild: () => placeholder,
				indent: (content) => content,
				wrapInBlock: (prefix, content) => prefix + content
			}, {
				index: 0,
				level: 0,
				parentType: "text",
				meta: {}
			});
			const placeholderIndex = rendered.indexOf(placeholder);
			return placeholderIndex >= 0 ? rendered.substring(0, placeholderIndex) : "";
		} catch (err) {
			throw new Error(`Failed to get mark opening for ${markType}: ${err}`);
		}
	}
	getMarkClosing(markType, mark, openingMode = "markdown") {
		var _a;
		if (openingMode === "html") return ((_a = this.getHtmlReopenTags(markType)) == null ? void 0 : _a.close) || "";
		const handlers$1 = this.getHandlersForNodeType(markType);
		const handler = handlers$1.length > 0 ? handlers$1[0] : void 0;
		if (!handler || !handler.renderMarkdown) return "";
		const placeholder = "__TIPTAP_MARKDOWN_PLACEHOLDER__";
		const syntheticNode = {
			type: markType,
			attrs: mark.attrs || {},
			content: [{
				type: "text",
				text: placeholder
			}]
		};
		try {
			const rendered = handler.renderMarkdown(syntheticNode, {
				renderChildren: () => placeholder,
				renderChild: () => placeholder,
				indent: (content) => content,
				wrapInBlock: (prefix, content) => prefix + content
			}, {
				index: 0,
				level: 0,
				parentType: "text",
				meta: {}
			});
			const placeholderIndex = rendered.indexOf(placeholder);
			const placeholderEnd = placeholderIndex + 33;
			return placeholderIndex >= 0 ? rendered.substring(placeholderEnd) : "";
		} catch (err) {
			throw new Error(`Failed to get mark closing for ${markType}: ${err}`);
		}
	}
	getHtmlReopenTags(markType) {
		const handlers$1 = this.getHandlersForNodeType(markType);
		const handler = handlers$1.length > 0 ? handlers$1[0] : void 0;
		return handler == null ? void 0 : handler.htmlReopen;
	}
	markSetsEqual(marks1, marks2) {
		if (marks1.size !== marks2.size) return false;
		return Array.from(marks1.keys()).every((type) => marks2.has(type));
	}
};
var MarkdownManager_default = MarkdownManager;
var Markdown = Extension.create({
	name: "markdown",
	addOptions() {
		return {
			indentation: {
				style: "space",
				size: 2
			},
			marked: void 0,
			markedOptions: {}
		};
	},
	addCommands() {
		return {
			setContent: (content, options) => {
				if (!(options == null ? void 0 : options.contentType)) return commands_exports.setContent(content, options);
				if (assumeContentType(content, options == null ? void 0 : options.contentType) !== "markdown" || !this.editor.markdown) return commands_exports.setContent(content, options);
				const mdContent = this.editor.markdown.parse(content);
				return commands_exports.setContent(mdContent, options);
			},
			insertContent: (value, options) => {
				if (!(options == null ? void 0 : options.contentType)) return commands_exports.insertContent(value, options);
				if (assumeContentType(value, options == null ? void 0 : options.contentType) !== "markdown" || !this.editor.markdown) return commands_exports.insertContent(value, options);
				const mdContent = this.editor.markdown.parse(value);
				return commands_exports.insertContent(mdContent, options);
			},
			insertContentAt: (position, value, options) => {
				if (!(options == null ? void 0 : options.contentType)) return commands_exports.insertContentAt(position, value, options);
				if (assumeContentType(value, options == null ? void 0 : options.contentType) !== "markdown" || !this.editor.markdown) return commands_exports.insertContentAt(position, value, options);
				const mdContent = this.editor.markdown.parse(value);
				return commands_exports.insertContentAt(position, mdContent, options);
			}
		};
	},
	addStorage() {
		return { manager: new MarkdownManager_default({
			indentation: this.options.indentation,
			marked: this.options.marked,
			markedOptions: this.options.markedOptions,
			extensions: []
		}) };
	},
	onBeforeCreate() {
		if (this.editor.markdown) {
			console.error("[tiptap][markdown]: There is already a `markdown` property on the editor instance. This might lead to unexpected behavior.");
			return;
		}
		this.storage.manager = new MarkdownManager_default({
			indentation: this.options.indentation,
			marked: this.options.marked,
			markedOptions: this.options.markedOptions,
			extensions: this.editor.extensionManager.baseExtensions
		});
		this.editor.markdown = this.storage.manager;
		this.editor.getMarkdown = () => {
			return this.storage.manager.serialize(this.editor.getJSON());
		};
		if (!this.editor.options.contentType) return;
		if (assumeContentType(this.editor.options.content, this.editor.options.contentType) !== "markdown") return;
		if (!this.editor.markdown) throw new Error("[tiptap][markdown]: The `contentType` option is set to \"markdown\", but the Markdown extension is not added to the editor. Please add the Markdown extension to use this feature.");
		if (this.editor.options.content === void 0 || typeof this.editor.options.content !== "string") throw new Error("[tiptap][markdown]: The `contentType` option is set to \"markdown\", but the initial content is not a string. Please provide the initial content as a markdown string.");
		const json = this.editor.markdown.parse(this.editor.options.content);
		this.editor.options.content = json;
	}
});
const TOGGLE_HEADING_VARIANTS = [
	"heading-1",
	"heading-2",
	"heading-3",
	"heading-4",
	"heading-5"
];
function parseToggleHeadingVariant(value) {
	return typeof value === "string" && TOGGLE_HEADING_VARIANTS.includes(value) ? value : null;
}
function escapeDetailsHtml(value) {
	return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll("\"", "&quot;");
}
function parseDetailsAttributes(rawAttributes) {
	const variantMatch = rawAttributes.match(/\sdata-orca-toggle\s*=\s*(?:"(heading-[1-5])"|'(heading-[1-5])'|(heading-[1-5]))(?:\s|$)/i);
	return {
		open: /\sopen(?:\s|=|$)/i.test(rawAttributes),
		variant: parseToggleHeadingVariant((variantMatch?.[1] ?? variantMatch?.[2] ?? variantMatch?.[3])?.toLowerCase())
	};
}
function detailsBodyHtmlToMarkdown(body) {
	return body.replace(/<p\b[^>]*>/gi, "").replace(/<\/p>/gi, "\n\n").replace(/<br\s*\/?>/gi, "\n").trim();
}
function renderDetailsAttributes(attrs) {
	const attributes = ["class=\"orca-details\""];
	const variant = parseToggleHeadingVariant(attrs?.variant);
	if (variant) attributes.push(`data-orca-toggle="${variant}"`);
	if (attrs?.open === true) attributes.push("open");
	return attributes.join(" ");
}
function markdownFenceRanges(content) {
	const ranges = [];
	let offset = 0;
	let openFence = null;
	for (const lineMatch of content.matchAll(/[^\r\n]*(?:\r\n|\n|\r|$)/g)) {
		const line = lineMatch[0];
		if (line === "") break;
		const lineText = line.replace(/(?:\r\n|\n|\r)$/u, "");
		if (openFence) {
			if ((openFence.marker === "`" ? /* @__PURE__ */ new RegExp(`^ {0,3}\`{${openFence.length},}\\s*$`) : /* @__PURE__ */ new RegExp(`^ {0,3}~{${openFence.length},}\\s*$`)).test(lineText)) {
				ranges.push([openFence.start, offset + line.length]);
				openFence = null;
			}
		} else {
			const openingFenceMatch = lineText.match(/^ {0,3}(`{3,}|~{3,})/u);
			if (openingFenceMatch?.[1]) openFence = {
				marker: openingFenceMatch[1][0],
				length: openingFenceMatch[1].length,
				start: offset
			};
		}
		offset += line.length;
	}
	if (openFence) ranges.push([openFence.start, content.length]);
	return ranges;
}
function isInsideRange(index, ranges) {
	return ranges.some(([start, end]) => index >= start && index < end);
}
function matchDetailsHtmlBlock(content, start) {
	const openingMatch = content.slice(start).match(/^<details\b[^>]*>/i);
	if (!openingMatch) return null;
	const detailsTagPattern = /<\/?details\b[^>]*>/gi;
	detailsTagPattern.lastIndex = start;
	const fenceRanges = markdownFenceRanges(content);
	let depth = 0;
	let hasNestedDetails = false;
	for (;;) {
		const tagMatch = detailsTagPattern.exec(content);
		if (!tagMatch) return null;
		const tag = tagMatch[0];
		if (tagMatch.index !== start && isInsideRange(tagMatch.index, fenceRanges)) continue;
		if (/^<\/details\b/i.test(tag)) {
			depth -= 1;
			if (depth === 0) {
				const closingEnd = tagMatch.index + tag.length;
				return {
					raw: content.slice(start, closingEnd),
					openingAttributes: openingMatch[0].replace(/^<details\b/i, "").replace(/>$/u, ""),
					inner: content.slice(start + openingMatch[0].length, tagMatch.index),
					hasNestedDetails
				};
			}
		} else {
			if (depth > 0) hasNestedDetails = true;
			depth += 1;
		}
	}
}
function hasOnlySupportedDetailsAttributes(rawAttributes) {
	return rawAttributes.replace(/\s+open(?:\s*=\s*(?:""|"open"|''|'open'|open))?(?=\s|$)/giu, "").replace(/\s+class\s*=\s*(?:"orca-details"|'orca-details'|orca-details)(?=\s|$)/giu, "").replace(/\s+data-orca-toggle\s*=\s*(?:"heading-[1-5]"|'heading-[1-5]'|heading-[1-5])(?=\s|$)/giu, "").trim() === "";
}
function hasOnlyPlainParagraphAndBreakTags(content) {
	return !/<p\b(?!\s*>)[^>]*>|<br\b(?!\s*\/?>)[^>]*>/iu.test(content);
}
function extractDetailsSummaryHtml(inner) {
	let startIndex = 0;
	while (startIndex < inner.length && isHtmlWhitespace$1(inner.charCodeAt(startIndex))) startIndex++;
	if (!startsWithAsciiIgnoreCase(inner, "<summary", startIndex)) return null;
	if (isHtmlTagNamePart(inner.charCodeAt(startIndex + 8))) return null;
	const openingEndIndex = inner.indexOf(">", startIndex + 8);
	if (openingEndIndex === -1) return null;
	const closingStartIndex = indexOfAsciiIgnoreCase(inner, "</summary>", openingEndIndex + 1);
	if (closingStartIndex === -1) return null;
	return {
		attributes: inner.slice(startIndex + 8, openingEndIndex),
		content: inner.slice(openingEndIndex + 1, closingStartIndex),
		rawLength: closingStartIndex + 10
	};
}
function isHtmlWhitespace$1(code$1) {
	return code$1 === 9 || code$1 === 10 || code$1 === 11 || code$1 === 12 || code$1 === 13 || code$1 === 32;
}
function indexOfAsciiIgnoreCase(value, search, fromIndex) {
	const lastStart = value.length - search.length;
	for (let index = Math.max(0, fromIndex); index <= lastStart; index++) if (startsWithAsciiIgnoreCase(value, search, index)) return index;
	return -1;
}
function startsWithAsciiIgnoreCase(value, search, startIndex) {
	if (startIndex < 0 || startIndex + search.length > value.length) return false;
	for (let index = 0; index < search.length; index++) if (toLowerAsciiCode(value.charCodeAt(startIndex + index)) !== search.charCodeAt(index)) return false;
	return true;
}
function toLowerAsciiCode(code$1) {
	return code$1 >= 65 && code$1 <= 90 ? code$1 + 32 : code$1;
}
function isHtmlTagNamePart(code$1) {
	return code$1 >= 48 && code$1 <= 57 || code$1 >= 65 && code$1 <= 90 || code$1 === 95 || code$1 >= 97 && code$1 <= 122;
}
function isEditableDetailsHtmlBlock(block) {
	if (block.hasNestedDetails) return false;
	if (!hasOnlySupportedDetailsAttributes(block.openingAttributes)) return false;
	const summary = extractDetailsSummaryHtml(block.inner);
	if (!summary) return false;
	if (summary.attributes.trim()) return false;
	if (/<\/?[A-Za-z][\w.:-]*(?:\s[^<>]*?)?\/?>/.test(summary.content)) return false;
	const bodyHtml = block.inner.slice(summary.rawLength);
	if (!hasOnlyPlainParagraphAndBreakTags(bodyHtml)) return false;
	const allowedHtmlRemoved = bodyHtml.replace(/<\/?p\b[^>]*>/gi, "").replace(/<br\s*\/?>/gi, "");
	return !/<\/?[A-Za-z][\w.:-]*(?:\s[^<>]*?)?\/?>/.test(allowedHtmlRemoved);
}
var REFERENCE_DEFINITION_PATTERN = /^ {0,3}\[([^\]]+)\]:[ \t]*(<[^>\n]+>|[^\s]+)(?:[ \t]+(?:"([^"]*)"|'([^']*)'|\(([^)]*)\)))?[ \t]*$/;
function normalizeReferenceLabel(label) {
	let normalized = "";
	let pendingWhitespace = false;
	for (let index = 0; index < label.length; index += 1) {
		if (isMarkdownReferenceLabelWhitespace(label.charCodeAt(index))) {
			pendingWhitespace = normalized.length > 0;
			continue;
		}
		if (pendingWhitespace) {
			normalized += " ";
			pendingWhitespace = false;
		}
		normalized += label.charAt(index);
	}
	return normalized.toLowerCase();
}
function isMarkdownReferenceLabelWhitespace(code$1) {
	return code$1 === 32 || code$1 >= 9 && code$1 <= 13 || code$1 === 160 || code$1 === 5760 || code$1 >= 8192 && code$1 <= 8202 || code$1 === 8232 || code$1 === 8233 || code$1 === 8239 || code$1 === 8287 || code$1 === 12288 || code$1 === 65279;
}
function unwrapReferenceUrl(rawUrl) {
	return rawUrl.startsWith("<") && rawUrl.endsWith(">") ? rawUrl.slice(1, -1) : rawUrl;
}
function parseReferenceDefinition(line) {
	const match = line.match(REFERENCE_DEFINITION_PATTERN);
	if (!match) return null;
	return {
		label: normalizeReferenceLabel(match[1]),
		url: unwrapReferenceUrl(match[2]),
		title: match[3] ?? match[4] ?? match[5] ?? null
	};
}
function splitReferenceDefinitions(content) {
	const definitions = /* @__PURE__ */ new Map();
	let activeFence = null;
	let activeFenceLength = 0;
	let markdown = "";
	forEachReferenceDefinitionLine(content, (line, newline) => {
		const fenceMatch = line.match(/^\s*(`{3,}|~{3,})/);
		if (fenceMatch) {
			const fenceChar = fenceMatch[1][0];
			const fenceLength = fenceMatch[1].length;
			if (activeFence === null) {
				activeFence = fenceChar;
				activeFenceLength = fenceLength;
			} else if (activeFence === fenceChar && fenceLength >= activeFenceLength) {
				activeFence = null;
				activeFenceLength = 0;
			}
		}
		const definition = activeFence === null ? parseReferenceDefinition(line) : null;
		if (definition) {
			definitions.set(definition.label, definition);
			return;
		}
		markdown += line + newline;
	});
	return {
		definitions,
		markdown
	};
}
function forEachReferenceDefinitionLine(content, visit) {
	let lineStart = 0;
	for (let index = 0; index <= content.length; index += 1) {
		const codeUnit = index < content.length ? content.charCodeAt(index) : 10;
		if (index < content.length && codeUnit !== 10 && codeUnit !== 13) continue;
		const hasLineEnding = index < content.length;
		const hasCrLf = codeUnit === 13 && content.charCodeAt(index + 1) === 10;
		const newline = hasLineEnding ? hasCrLf ? "\r\n" : content[index] : "";
		visit(content.slice(lineStart, index), newline);
		if (hasCrLf) index += 1;
		lineStart = index + 1;
	}
}
function isEscaped$1(content, index) {
	let backslashCount = 0;
	for (let cursor = index - 1; cursor >= 0 && content[cursor] === "\\"; cursor -= 1) backslashCount += 1;
	return backslashCount % 2 === 1;
}
function findClosingBracket(content, start) {
	for (let index = start; index < content.length; index += 1) if (content[index] === "]" && !isEscaped$1(content, index)) return index;
	return -1;
}
function formatInlineReferenceLink(text, definition) {
	const escapedUrl = definition.url.replace(/[()\\]/g, "\\$&");
	if (!definition.title) return `[${text}](${escapedUrl})`;
	return `[${text}](${escapedUrl} "${definition.title.replace(/["\\]/g, "\\$&")}")`;
}
function replaceReferenceLinks(markdown, definitions) {
	let result = "";
	let index = 0;
	let activeFence = null;
	let activeFenceLength = 0;
	let isLineStart = true;
	while (index < markdown.length) {
		if (isLineStart) {
			const fenceMatch = markdown.slice(index).match(/^\s*(`{3,}|~{3,})/);
			if (fenceMatch) {
				const fenceChar = fenceMatch[1][0];
				const fenceLength = fenceMatch[1].length;
				if (activeFence === null) {
					activeFence = fenceChar;
					activeFenceLength = fenceLength;
				} else if (activeFence === fenceChar && fenceLength >= activeFenceLength) {
					activeFence = null;
					activeFenceLength = 0;
				}
			}
		}
		if (activeFence || markdown[index] !== "[" || isEscaped$1(markdown, index)) {
			const nextChar = markdown[index];
			result += nextChar;
			isLineStart = nextChar === "\n";
			index += 1;
			continue;
		}
		const closingTextIndex = findClosingBracket(markdown, index + 1);
		if (closingTextIndex === -1) {
			result += markdown[index];
			isLineStart = false;
			index += 1;
			continue;
		}
		const text = markdown.slice(index + 1, closingTextIndex);
		const afterText = markdown[closingTextIndex + 1];
		if (afterText === "(") {
			result += markdown[index];
			isLineStart = false;
			index += 1;
			continue;
		}
		if (afterText === "[") {
			const closingLabelIndex = findClosingBracket(markdown, closingTextIndex + 2);
			if (closingLabelIndex !== -1) {
				const label = normalizeReferenceLabel(markdown.slice(closingTextIndex + 2, closingLabelIndex) || text);
				const definition = definitions.get(label);
				if (definition) {
					result += formatInlineReferenceLink(text, definition);
					isLineStart = false;
					index = closingLabelIndex + 1;
					continue;
				}
			}
		} else {
			const definition = definitions.get(normalizeReferenceLabel(text));
			if (definition) {
				result += formatInlineReferenceLink(text, definition);
				isLineStart = false;
				index = closingTextIndex + 1;
				continue;
			}
		}
		result += markdown[index];
		isLineStart = false;
		index += 1;
	}
	return result;
}
function normalizeMarkdownReferenceLinks(content) {
	const { definitions, markdown } = splitReferenceDefinitions(content);
	if (definitions.size === 0) return content;
	return replaceReferenceLinks(markdown, definitions);
}
function createTiptapMarkedFacade() {
	const registry = new D();
	class RegistryLexer extends x {
		constructor(options) {
			super({
				...registry.defaults,
				...options,
				extensions: registry.defaults.extensions
			});
		}
	}
	const parser = (tokens, options) => registry.parser(tokens, options);
	const lexer = (src, options) => new RegistryLexer(options).lex(src);
	const facade = new Proxy(g, {
		apply: (_target, _thisArg, args) => Reflect.apply(registry.parse, registry, args),
		get: (target, property, receiver) => {
			switch (property) {
				case "defaults": return registry.defaults;
				case "getDefaults": return M;
				case "Lexer": return RegistryLexer;
				case "Parser": return b;
				case "Renderer": return y;
				case "TextRenderer": return $;
				case "Tokenizer": return w;
				case "Hooks": return P;
				case "parse": return facade;
				case "parseInline": return registry.parseInline;
				case "parser": return parser;
				case "lexer": return lexer;
				case "walkTokens": return registry.walkTokens.bind(registry);
				case "use": return (...extensions) => {
					registry.use(...extensions);
					return facade;
				};
				case "setOptions":
				case "options": return (options) => {
					registry.setOptions(options);
					return facade;
				};
				default: return Reflect.get(target, property, receiver);
			}
		}
	});
	return facade;
}
var TRANSPORT_PREFIX = "[[ORCA_RICH_MD:";
var TRANSPORT_SUFFIX = "]]";
var KEY_PATTERN = /^[a-f0-9]{32}$/;
var TRANSPORT_BODY_PATTERN = /^ORCA_RICH_MD:[a-f0-9]{32}:(?:literal|inline-html|block-html|document-link|html-superscript-link):/;
var LEGACY_PREFIXES = [
	"ORCA_RAW_HTML_INLINE:",
	"ORCA_RAW_HTML_BLOCK:",
	"ORCA_DOC_LINK:"
];
function isLegacyRichMarkdownTransportBody(value) {
	return LEGACY_PREFIXES.some((prefix) => value.startsWith(prefix));
}
function isReservedRichMarkdownTransportBody(value) {
	return isLegacyRichMarkdownTransportBody(value) || TRANSPORT_BODY_PATTERN.test(value);
}
function createRichMarkdownEditorCodec(key = createCodecKey()) {
	return {
		transport: createRichMarkdownSourceTransport(key),
		marked: createTiptapMarkedFacade()
	};
}
function createRichMarkdownSourceTransport(key) {
	if (!KEY_PATTERN.test(key)) throw new Error("Rich Markdown transport keys must be 128-bit lowercase hex values");
	const authoredPrefix = `${TRANSPORT_PREFIX}${key}:`;
	const startFor = (kind) => `${authoredPrefix}${kind}:`;
	return {
		key,
		authoredPrefix,
		startFor,
		create: (kind, value) => `${startFor(kind)}${encodeURIComponent(value)}${TRANSPORT_SUFFIX}`,
		match: (source, kind) => {
			const prefix = startFor(kind);
			if (!source.startsWith(prefix)) return null;
			const endIndex = source.indexOf(TRANSPORT_SUFFIX, prefix.length);
			if (endIndex === -1) return null;
			const raw = source.slice(0, endIndex + 2);
			try {
				return {
					raw,
					value: decodeURIComponent(source.slice(prefix.length, endIndex))
				};
			} catch {
				return null;
			}
		}
	};
}
function createCodecKey() {
	const bytes = new Uint8Array(16);
	globalThis.crypto.getRandomValues(bytes);
	return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}
function decodeHtmlTextCharacterReferences(value) {
	const template = document.createElement("template");
	template.innerHTML = value;
	return template.content.textContent ?? "";
}
function decodeHtmlAttributeCharacterReferences(value, quote) {
	const template = document.createElement("template");
	const delimiter = quote ?? "";
	template.innerHTML = `<span data-orca-value=${delimiter}${value}${delimiter}></span>`;
	return template.content.firstElementChild?.getAttribute("data-orca-value") ?? "";
}
const HTML_SUPERSCRIPT_LINK_SOURCE_LIMIT = 16 * 1024;
var encoder = new TextEncoder();
function matchHtmlSuperscriptLinkSource(input, start = 0, stats) {
	const limit = Math.min(input.length, start + HTML_SUPERSCRIPT_LINK_SOURCE_LIMIT + 1);
	let index = matchSimpleTag(input, start, "sup", false, limit, stats);
	if (index === null) return null;
	const anchor = matchAnchorStart(input, index, limit, stats);
	if (!anchor) return null;
	index = anchor.end;
	const labelStart = index;
	while (index < limit) {
		step(stats);
		const code$1 = input.charCodeAt(index);
		if (code$1 === 60 || code$1 === 10 || code$1 === 13) break;
		index += 1;
	}
	if (index === labelStart || index >= limit || input.charCodeAt(index) !== 60) return null;
	const rawLabel = input.slice(labelStart, index);
	index = matchSimpleTag(input, index, "a", true, limit, stats) ?? -1;
	if (index < 0) return null;
	index = matchSimpleTag(input, index, "sup", true, limit, stats) ?? -1;
	if (index < 0 || index - start > 16384) return null;
	const hrefAttribute = anchor.attributes.find((attribute) => attribute.name === "href");
	const titleAttribute = anchor.attributes.find((attribute) => attribute.name === "title");
	if (!hrefAttribute) return null;
	const href = decodeHtmlAttributeCharacterReferences(hrefAttribute.rawValue, hrefAttribute.quote);
	const label = decodeHtmlTextCharacterReferences(rawLabel);
	const title = titleAttribute ? decodeHtmlAttributeCharacterReferences(titleAttribute.rawValue, titleAttribute.quote) : null;
	const source = input.slice(start, index);
	if (byteLength(source) > 16384 || byteLength(href) > 8192 || byteLength(label) > 2048 || title !== null && byteLength(title) > 2048) return null;
	return {
		end: index,
		value: {
			source,
			href,
			label,
			title
		}
	};
}
function parseHtmlSuperscriptLinkSource(source) {
	const match = matchHtmlSuperscriptLinkSource(source);
	return match?.end === source.length ? match.value : null;
}
function matchAnchorStart(input, start, limit, stats) {
	let index = start;
	if (input.charCodeAt(index) !== 60 || lowerCode(input.charCodeAt(index + 1)) !== 97) return null;
	step(stats, 2);
	index += 2;
	if (!isSingleLineHtmlWhitespace(input.charCodeAt(index))) return null;
	const attributes = [];
	while (index < limit) {
		const whitespaceStart = index;
		while (isSingleLineHtmlWhitespace(input.charCodeAt(index))) {
			step(stats);
			index += 1;
		}
		if (input.charCodeAt(index) === 62) {
			step(stats);
			return attributes.some((attribute) => attribute.name === "href") ? {
				end: index + 1,
				attributes
			} : null;
		}
		if (index === whitespaceStart) return null;
		const nameStart = index;
		while (isAttributeNameCode(input.charCodeAt(index))) {
			step(stats);
			index += 1;
		}
		if (index === nameStart) return null;
		const normalizedName = input.slice(nameStart, index).toLowerCase();
		if (normalizedName !== "href" && normalizedName !== "title") return null;
		if (attributes.some((attribute) => attribute.name === normalizedName)) return null;
		while (isSingleLineHtmlWhitespace(input.charCodeAt(index))) {
			step(stats);
			index += 1;
		}
		if (input.charCodeAt(index) !== 61) return null;
		step(stats);
		index += 1;
		while (isSingleLineHtmlWhitespace(input.charCodeAt(index))) {
			step(stats);
			index += 1;
		}
		const parsedValue = parseAttributeValue(input, index, limit, stats);
		if (!parsedValue) return null;
		attributes.push({
			name: normalizedName,
			rawValue: parsedValue.rawValue,
			quote: parsedValue.quote
		});
		index = parsedValue.end;
	}
	return null;
}
function parseAttributeValue(input, start, limit, stats) {
	const first$1 = input[start];
	if (first$1 === "\"" || first$1 === "'") {
		let index$1 = start + 1;
		while (index$1 < limit && input[index$1] !== first$1) {
			step(stats);
			const code$1 = input.charCodeAt(index$1);
			if (code$1 === 10 || code$1 === 13) return null;
			index$1 += 1;
		}
		if (index$1 >= limit) return null;
		step(stats);
		return {
			end: index$1 + 1,
			rawValue: input.slice(start + 1, index$1),
			quote: first$1
		};
	}
	let index = start;
	while (index < limit) {
		const code$1 = input.charCodeAt(index);
		if (isHtmlWhitespace(code$1) || code$1 === 62) break;
		step(stats);
		if (code$1 === 34 || code$1 === 39 || code$1 === 60 || code$1 === 61 || code$1 === 96) return null;
		index += 1;
	}
	return index === start ? null : {
		end: index,
		rawValue: input.slice(start, index),
		quote: null
	};
}
function matchSimpleTag(input, start, name, closing, limit, stats) {
	let index = start;
	if (input.charCodeAt(index) !== 60) return null;
	step(stats);
	index += 1;
	if (closing) {
		if (input.charCodeAt(index) !== 47) return null;
		step(stats);
		index += 1;
	}
	for (let nameIndex = 0; nameIndex < name.length; nameIndex += 1) {
		step(stats);
		if (lowerCode(input.charCodeAt(index)) !== name.charCodeAt(nameIndex)) return null;
		index += 1;
	}
	while (index < limit && isSingleLineHtmlWhitespace(input.charCodeAt(index))) {
		step(stats);
		index += 1;
	}
	if (input.charCodeAt(index) !== 62) return null;
	step(stats);
	return index + 1;
}
function isAttributeNameCode(code$1) {
	return code$1 >= 65 && code$1 <= 90 || code$1 >= 97 && code$1 <= 122 || code$1 >= 48 && code$1 <= 57 || code$1 === 95 || code$1 === 46 || code$1 === 58 || code$1 === 45;
}
function isHtmlWhitespace(code$1) {
	return code$1 === 9 || code$1 === 10 || code$1 === 12 || code$1 === 13 || code$1 === 32;
}
function isSingleLineHtmlWhitespace(code$1) {
	return code$1 === 9 || code$1 === 12 || code$1 === 32;
}
function lowerCode(code$1) {
	return code$1 >= 65 && code$1 <= 90 ? code$1 + 32 : code$1;
}
function byteLength(value) {
	return encoder.encode(value).byteLength;
}
function step(stats, count = 1) {
	if (stats) stats.transitions += count;
}
var INLINE_HTML_PATTERN = /^<!--[\s\S]*?-->|^<\/?[A-Za-z][\w.:-]*(?:\s[^<>]*?)?\/?>/;
function matchInlineHtml(src) {
	return src.match(INLINE_HTML_PATTERN)?.[0] ?? null;
}
function isEscaped(content, index) {
	let backslashCount = 0;
	for (let i$1 = index - 1; i$1 >= 0 && content[i$1] === "\\"; i$1 -= 1) backslashCount += 1;
	return backslashCount % 2 === 1;
}
function findLineEnd(content, start) {
	const newlineIndex = content.indexOf("\n", start);
	return newlineIndex === -1 ? content.length : newlineIndex;
}
function isLineOnlyHtml(line) {
	const trimmed = line.trim();
	if (!trimmed.startsWith("<")) return false;
	if (trimmed.startsWith("<!--")) return trimmed.includes("-->");
	return /^<\/?[A-Za-z][\w.:-]*(?:\s[^<>]*?)?\/?>$/.test(trimmed);
}
function matchBlockHtml(content, start) {
	const lineEnd = findLineEnd(content, start);
	const line = content.slice(start, lineEnd);
	if (!isLineOnlyHtml(line)) return null;
	return line;
}
function encodeRawMarkdownHtmlForRichEditor(content, codec, { htmlSuperscriptLinks = false } = {}) {
	const normalizedContent = normalizeMarkdownReferenceLinks(content);
	const { transport } = codec;
	let index = 0;
	let isLineStart = true;
	let activeFence = null;
	let activeFenceLength = 0;
	let result = "";
	while (index < normalizedContent.length) {
		if (isLineStart) {
			const fenceMatch = normalizedContent.slice(index).match(/^\s*(`{3,}|~{3,})/);
			if (fenceMatch) {
				const fenceChar = fenceMatch[1][0];
				const fenceLength = fenceMatch[1].length;
				if (activeFence === null) {
					activeFence = fenceChar;
					activeFenceLength = fenceLength;
				} else if (activeFence === fenceChar && fenceLength >= activeFenceLength) {
					activeFence = null;
					activeFenceLength = 0;
				}
			}
		}
		if (activeFence) {
			const nextChar$1 = normalizedContent[index];
			result += nextChar$1;
			isLineStart = nextChar$1 === "\n";
			index += 1;
			continue;
		}
		if (normalizedContent[index] === "`") {
			let tickCount = 0;
			while (normalizedContent[index + tickCount] === "`") tickCount += 1;
			let searchFrom = index + tickCount;
			let closingIndex = -1;
			while (searchFrom < normalizedContent.length) {
				const candidate = normalizedContent.indexOf("`".repeat(tickCount), searchFrom);
				if (candidate === -1) break;
				if ((candidate === 0 || normalizedContent[candidate - 1] !== "`") && normalizedContent[candidate + tickCount] !== "`") {
					closingIndex = candidate;
					break;
				}
				searchFrom = candidate + 1;
			}
			if (closingIndex !== -1) {
				const rawSpan = normalizedContent.slice(index, closingIndex + tickCount);
				result += rawSpan;
				isLineStart = rawSpan.endsWith("\n");
				index = closingIndex + tickCount;
				continue;
			}
		}
		if (isLineStart) {
			const detailsHtml = matchDetailsHtmlBlock(normalizedContent, index);
			if (detailsHtml && isEditableDetailsHtmlBlock(detailsHtml)) {
				result += detailsHtml.raw;
				index += detailsHtml.raw.length;
				continue;
			}
			if (detailsHtml) {
				result += transport.create("block-html", detailsHtml.raw);
				index += detailsHtml.raw.length;
				continue;
			}
			const blockHtml = matchBlockHtml(normalizedContent, index);
			if (blockHtml) {
				result += transport.create("block-html", blockHtml);
				index += blockHtml.length;
				continue;
			}
		}
		if (normalizedContent.startsWith(transport.authoredPrefix, index)) {
			const authoredEnd = normalizedContent.indexOf("]]", index + transport.authoredPrefix.length);
			const authoredOccurrence = authoredEnd === -1 ? transport.authoredPrefix : normalizedContent.slice(index, authoredEnd + 2);
			result += transport.create("literal", authoredOccurrence);
			index += authoredOccurrence.length;
			continue;
		}
		if (normalizedContent[index] === "<" && !isEscaped(normalizedContent, index)) {
			if (htmlSuperscriptLinks) {
				const superscriptLink = matchHtmlSuperscriptLinkSource(normalizedContent, index);
				if (superscriptLink) {
					result += transport.create("html-superscript-link", JSON.stringify(superscriptLink.value));
					index = superscriptLink.end;
					continue;
				}
			}
			const inlineHtml = matchInlineHtml(normalizedContent.slice(index));
			if (inlineHtml) {
				result += transport.create("inline-html", inlineHtml);
				index += inlineHtml.length;
				continue;
			}
		}
		if (normalizedContent[index] === "[" && normalizedContent[index + 1] === "[" && !isEscaped(normalizedContent, index)) {
			const closingIndex = normalizedContent.indexOf("]]", index + 2);
			if (closingIndex !== -1) {
				const rawTarget = normalizedContent.slice(index + 2, closingIndex);
				const link = parseMarkdownDocLink(rawTarget);
				if (link && !isReservedRichMarkdownTransportBody(rawTarget)) {
					result += transport.create("document-link", formatMarkdownDocLinkBody(link.target, link.alias));
					index = closingIndex + 2;
					continue;
				}
			}
		}
		const nextChar = normalizedContent[index];
		result += nextChar;
		isLineStart = nextChar === "\n";
		index += 1;
	}
	return result;
}
function createRichMarkdownLiteral(transport) {
	return createRawSourceNode({
		name: "richMarkdownLiteral",
		kind: "literal",
		inline: true,
		transport,
		marker: "data-rich-markdown-literal"
	});
}
function createRawMarkdownHtmlInline(transport) {
	return createRawSourceNode({
		name: "rawMarkdownHtmlInline",
		kind: "inline-html",
		inline: true,
		transport,
		marker: "data-raw-markdown-html-inline",
		className: "raw-markdown-html-inline"
	});
}
function createRawSourceNode({ name, kind, inline, transport, marker, className }) {
	return Node3.create({
		name,
		inline,
		group: inline ? "inline" : "block",
		atom: true,
		selectable: true,
		addAttributes() {
			return { value: {
				default: "",
				rendered: false
			} };
		},
		markdownTokenName: name,
		markdownTokenizer: {
			name,
			level: inline ? "inline" : "block",
			start: transport.startFor(kind),
			tokenize(src) {
				const matched = transport.match(src, kind);
				if (!matched) return;
				return {
					type: name,
					raw: matched.raw,
					text: matched.value,
					block: !inline
				};
			}
		},
		parseMarkdown: (token, helpers) => {
			if (token.type !== name) return [];
			return helpers.createNode(name, { value: typeof token.text === "string" ? token.text : "" });
		},
		renderMarkdown: (node) => typeof node.attrs?.value === "string" ? node.attrs.value : "",
		renderText: ({ node }) => typeof node.attrs.value === "string" ? node.attrs.value : "",
		parseHTML() {
			return [{
				tag: `${inline ? "span" : "div"}[${marker}]`,
				getAttrs: (element) => ({ value: element.textContent ?? "" })
			}];
		},
		renderHTML({ HTMLAttributes, node }) {
			const value = typeof node.attrs.value === "string" ? node.attrs.value : "";
			return [
				inline ? "span" : "div",
				mergeAttributes(HTMLAttributes, {
					[marker]: "",
					contenteditable: "false",
					class: className
				}),
				inline ? value : ["pre", value]
			];
		}
	});
}
function createRawMarkdownHtmlBlock(transport) {
	return createRawSourceNode({
		name: "rawMarkdownHtmlBlock",
		kind: "block-html",
		inline: false,
		transport,
		marker: "data-raw-markdown-html-block",
		className: "raw-markdown-html-block"
	});
}
var isNodeVisible = (position, editor) => {
	return editor.view.domAtPos(position).node.offsetParent !== null;
};
var findClosestVisibleNode = ($pos, predicate, editor) => {
	for (let i$1 = $pos.depth; i$1 > 0; i$1 -= 1) {
		const node = $pos.node(i$1);
		const match = predicate(node);
		const isVisible = isNodeVisible($pos.start(i$1), editor);
		if (match && isVisible) return {
			pos: i$1 > 0 ? $pos.before(i$1) : 0,
			start: $pos.start(i$1),
			depth: i$1,
			node
		};
	}
};
var setGapCursor = (editor, direction) => {
	const { state, view, extensionManager } = editor;
	const { schema, selection } = state;
	const { empty: empty$1, $anchor } = selection;
	const hasGapCursorExtension = !!extensionManager.extensions.find((extension) => extension.name === "gapCursor");
	if (!empty$1 || $anchor.parent.type !== schema.nodes.detailsSummary || !hasGapCursorExtension) return false;
	if (direction === "right" && $anchor.parentOffset !== $anchor.parent.nodeSize - 2) return false;
	const details = findParentNode((node) => node.type === schema.nodes.details)(selection);
	if (!details) return false;
	const detailsContent = findChildren(details.node, (node) => node.type === schema.nodes.detailsContent);
	if (!detailsContent.length) return false;
	if (isNodeVisible(details.start + detailsContent[0].pos + 1, editor)) return false;
	const $position = state.doc.resolve(details.pos + details.node.nodeSize);
	const $validPosition = GapCursor.findFrom($position, 1, false);
	if (!$validPosition) return false;
	const { tr: tr$1 } = state;
	const gapCursorSelection = new GapCursor($validPosition);
	tr$1.setSelection(gapCursorSelection);
	tr$1.scrollIntoView();
	view.dispatch(tr$1);
	return true;
};
var Details = Node3.create({
	name: "details",
	content: "detailsSummary detailsContent",
	group: "block",
	defining: true,
	isolating: true,
	allowGapCursor: false,
	addOptions() {
		return {
			persist: false,
			openClassName: "is-open",
			HTMLAttributes: {},
			renderToggleButton: ({ element, isOpen }) => {
				element.setAttribute("aria-label", isOpen ? "Collapse details content" : "Expand details content");
			}
		};
	},
	addAttributes() {
		if (!this.options.persist) return [];
		return { open: {
			default: false,
			parseHTML: (element) => element.hasAttribute("open"),
			renderHTML: ({ open }) => {
				if (!open) return {};
				return { open: "" };
			}
		} };
	},
	parseHTML() {
		return [{ tag: "details" }];
	},
	renderHTML({ HTMLAttributes }) {
		return [
			"details",
			mergeAttributes(this.options.HTMLAttributes, HTMLAttributes),
			0
		];
	},
	...createBlockMarkdownSpec({
		nodeName: "details",
		content: "block"
	}),
	addNodeView() {
		return ({ editor, getPos, node, HTMLAttributes }) => {
			const dom = document.createElement("div");
			const attributes = mergeAttributes(this.options.HTMLAttributes, HTMLAttributes, { "data-type": this.name });
			Object.entries(attributes).forEach(([key, value]) => dom.setAttribute(key, value));
			const toggle = document.createElement("button");
			toggle.type = "button";
			const renderToggleButton = (options) => {
				this.options.renderToggleButton({
					element: toggle,
					...options
				});
			};
			dom.append(toggle);
			const content = document.createElement("div");
			dom.append(content);
			const toggleDetailsContent = (options) => {
				const { setToValue, node: currentNode = node } = options || {};
				if (setToValue !== void 0) if (setToValue) {
					if (dom.classList.contains(this.options.openClassName)) return;
					dom.classList.add(this.options.openClassName);
				} else {
					if (!dom.classList.contains(this.options.openClassName)) return;
					dom.classList.remove(this.options.openClassName);
				}
				else dom.classList.toggle(this.options.openClassName);
				renderToggleButton({
					isOpen: dom.classList.contains(this.options.openClassName),
					node: currentNode
				});
				const event = new Event("toggleDetailsContent");
				content.querySelector(":scope > div[data-type=\"detailsContent\"]")?.dispatchEvent(event);
			};
			renderToggleButton({
				isOpen: Boolean(node.attrs.open),
				node
			});
			if (node.attrs.open) setTimeout(() => toggleDetailsContent());
			toggle.addEventListener("click", () => {
				toggleDetailsContent();
				if (!this.options.persist) {
					editor.commands.focus(void 0, { scrollIntoView: false });
					return;
				}
				if (editor.isEditable && typeof getPos === "function") {
					const { from, to } = editor.state.selection;
					editor.chain().command(({ tr: tr$1 }) => {
						const pos = getPos();
						if (!pos) return false;
						const currentNode = tr$1.doc.nodeAt(pos);
						if ((currentNode == null ? void 0 : currentNode.type) !== this.type) return false;
						tr$1.setNodeMarkup(pos, void 0, { open: !currentNode.attrs.open });
						return true;
					}).setTextSelection({
						from,
						to
					}).focus(void 0, { scrollIntoView: false }).run();
				}
			});
			return {
				dom,
				contentDOM: content,
				ignoreMutation(mutation) {
					if (mutation.type === "selection") return false;
					const target = mutation.target;
					const isInsideWrapper = dom.contains(target);
					return toggle.contains(target) || !isInsideWrapper || dom === target;
				},
				update: (updatedNode) => {
					if (updatedNode.type !== this.type) return false;
					if (updatedNode.attrs.open !== void 0) toggleDetailsContent({
						setToValue: updatedNode.attrs.open,
						node: updatedNode
					});
					else renderToggleButton({
						isOpen: dom.classList.contains(this.options.openClassName),
						node: updatedNode
					});
					return true;
				}
			};
		};
	},
	addCommands() {
		return {
			setDetails: () => ({ state, chain }) => {
				var _a;
				const { schema, selection } = state;
				const { $from, $to } = selection;
				const range = $from.blockRange($to);
				if (!range) return false;
				const slice = state.doc.slice(range.start, range.end);
				if (!schema.nodes.detailsContent.contentMatch.matchFragment(slice.content)) return false;
				const content = ((_a = slice.toJSON()) == null ? void 0 : _a.content) || [];
				return chain().insertContentAt({
					from: range.start,
					to: range.end
				}, {
					type: this.name,
					content: [{ type: "detailsSummary" }, {
						type: "detailsContent",
						content
					}]
				}).setTextSelection(range.start + 2).run();
			},
			unsetDetails: () => ({ state, chain }) => {
				const { selection, schema } = state;
				const details = findParentNode((node) => node.type === this.type)(selection);
				if (!details) return false;
				const detailsSummaries = findChildren(details.node, (node) => node.type === schema.nodes.detailsSummary);
				const detailsContents = findChildren(details.node, (node) => node.type === schema.nodes.detailsContent);
				if (!detailsSummaries.length || !detailsContents.length) return false;
				const detailsSummary = detailsSummaries[0];
				const detailsContent = detailsContents[0];
				const from = details.pos;
				const $from = state.doc.resolve(from);
				const range = {
					from,
					to: from + details.node.nodeSize
				};
				const content = detailsContent.node.content.toJSON() || [];
				const defaultTypeForSummary = $from.parent.type.contentMatch.defaultType;
				const mergedContent = [defaultTypeForSummary == null ? void 0 : defaultTypeForSummary.create(null, detailsSummary.node.content).toJSON(), ...content];
				return chain().insertContentAt(range, mergedContent).setTextSelection(from + 1).run();
			}
		};
	},
	addKeyboardShortcuts() {
		return {
			Backspace: () => {
				const { schema, selection } = this.editor.state;
				const { empty: empty$1, $anchor } = selection;
				if (!empty$1 || $anchor.parent.type !== schema.nodes.detailsSummary) return false;
				if ($anchor.parentOffset !== 0) return this.editor.commands.command(({ tr: tr$1 }) => {
					const from = $anchor.pos - 1;
					const to = $anchor.pos;
					tr$1.delete(from, to);
					return true;
				});
				return this.editor.commands.unsetDetails();
			},
			Enter: ({ editor }) => {
				const { state, view } = editor;
				const { schema, selection } = state;
				const { $head } = selection;
				if ($head.parent.type !== schema.nodes.detailsSummary) return false;
				const isVisible = isNodeVisible($head.after() + 1, editor);
				const above = isVisible ? state.doc.nodeAt($head.after()) : $head.node(-2);
				if (!above) return false;
				const after = isVisible ? 0 : $head.indexAfter(-1);
				const type = defaultBlockAt(above.contentMatchAt(after));
				if (!type || !above.canReplaceWith(after, after, type)) return false;
				const node = type.createAndFill();
				if (!node) return false;
				const pos = isVisible ? $head.after() + 1 : $head.after(-1);
				const tr$1 = state.tr.replaceWith(pos, pos, node);
				const $pos = tr$1.doc.resolve(pos);
				const newSelection = Selection.near($pos, 1);
				tr$1.setSelection(newSelection);
				tr$1.scrollIntoView();
				view.dispatch(tr$1);
				return true;
			},
			ArrowRight: ({ editor }) => {
				return setGapCursor(editor, "right");
			},
			ArrowDown: ({ editor }) => {
				return setGapCursor(editor, "down");
			}
		};
	},
	addProseMirrorPlugins() {
		return [new Plugin({
			key: new PluginKey("detailsSelection"),
			appendTransaction: (transactions, oldState, newState) => {
				const { editor, type } = this;
				if (editor.view.composing) return;
				if (!transactions.some((transaction2) => transaction2.selectionSet) || !oldState.selection.empty || !newState.selection.empty) return;
				if (!isActive(newState, type.name)) return;
				const { $from } = newState.selection;
				if (isNodeVisible($from.pos, editor)) return;
				const details = findClosestVisibleNode($from, (node) => node.type === type, editor);
				if (!details) return;
				const detailsSummaries = findChildren(details.node, (node) => node.type === newState.schema.nodes.detailsSummary);
				if (!detailsSummaries.length) return;
				const detailsSummary = detailsSummaries[0];
				const correctedPosition = (oldState.selection.from < newState.selection.from ? "forward" : "backward") === "forward" ? details.start + detailsSummary.pos : details.pos + detailsSummary.pos + detailsSummary.node.nodeSize;
				const selection = TextSelection.create(newState.doc, correctedPosition);
				return newState.tr.setSelection(selection);
			}
		})];
	}
});
var DetailsContent = Node3.create({
	name: "detailsContent",
	content: "block+",
	defining: true,
	selectable: false,
	addOptions() {
		return { HTMLAttributes: {} };
	},
	parseHTML() {
		return [{ tag: `div[data-type="${this.name}"]` }];
	},
	renderHTML({ HTMLAttributes }) {
		return [
			"div",
			mergeAttributes(this.options.HTMLAttributes, HTMLAttributes, { "data-type": this.name }),
			0
		];
	},
	addNodeView() {
		return ({ HTMLAttributes }) => {
			const dom = document.createElement("div");
			const attributes = mergeAttributes(this.options.HTMLAttributes, HTMLAttributes, {
				"data-type": this.name,
				hidden: "hidden"
			});
			Object.entries(attributes).forEach(([key, value]) => dom.setAttribute(key, value));
			dom.addEventListener("toggleDetailsContent", () => {
				dom.toggleAttribute("hidden");
			});
			return {
				dom,
				contentDOM: dom,
				ignoreMutation(mutation) {
					if (mutation.type === "selection") return false;
					return !dom.contains(mutation.target) || dom === mutation.target;
				},
				update: (updatedNode) => {
					if (updatedNode.type !== this.type) return false;
					return true;
				}
			};
		};
	},
	addKeyboardShortcuts() {
		return { Enter: ({ editor }) => {
			const { state, view } = editor;
			const { selection } = state;
			const { $from, empty: empty$1 } = selection;
			const detailsContent = findParentNode((node2) => node2.type === this.type)(selection);
			if (!empty$1 || !detailsContent || !detailsContent.node.childCount) return false;
			const fromIndex = $from.index(detailsContent.depth);
			const { childCount } = detailsContent.node;
			if (!(childCount === fromIndex + 1)) return false;
			const defaultChildType = detailsContent.node.type.contentMatch.defaultType;
			const defaultChildNode = defaultChildType == null ? void 0 : defaultChildType.createAndFill();
			if (!defaultChildNode) return false;
			const $childPos = state.doc.resolve(detailsContent.pos + 1);
			const lastChildIndex = childCount - 1;
			const lastChildNode = detailsContent.node.child(lastChildIndex);
			const lastChildPos = $childPos.posAtIndex(lastChildIndex, detailsContent.depth);
			if (!lastChildNode.eq(defaultChildNode)) return false;
			const above = $from.node(-3);
			if (!above) return false;
			const after = $from.indexAfter(-3);
			const type = defaultBlockAt(above.contentMatchAt(after));
			if (!type || !above.canReplaceWith(after, after, type)) return false;
			const node = type.createAndFill();
			if (!node) return false;
			const { tr: tr$1 } = state;
			const pos = $from.after(-2);
			tr$1.replaceWith(pos, pos, node);
			const $pos = tr$1.doc.resolve(pos);
			const newSelection = Selection.near($pos, 1);
			tr$1.setSelection(newSelection);
			const deleteFrom = lastChildPos;
			const deleteTo = lastChildPos + lastChildNode.nodeSize;
			tr$1.delete(deleteFrom, deleteTo);
			tr$1.scrollIntoView();
			view.dispatch(tr$1);
			return true;
		} };
	},
	...createBlockMarkdownSpec({ nodeName: "detailsContent" })
});
var DetailsSummary = Node3.create({
	name: "detailsSummary",
	content: "text*",
	defining: true,
	selectable: false,
	isolating: true,
	addOptions() {
		return { HTMLAttributes: {} };
	},
	parseHTML() {
		return [{ tag: "summary" }];
	},
	renderHTML({ HTMLAttributes }) {
		return [
			"summary",
			mergeAttributes(this.options.HTMLAttributes, HTMLAttributes),
			0
		];
	},
	...createBlockMarkdownSpec({
		nodeName: "detailsSummary",
		content: "inline"
	})
});
var RICH_MARKDOWN_PLACEHOLDER = "Write markdown… Type / for blocks.";
var TOGGLE_TEXT_PLACEHOLDER = "text";
var TOGGLE_HEADING_PLACEHOLDERS = {
	"heading-1": ["auto.components.editor.rich.markdown.slash.commands.e66e7f04c6", "Heading 1"],
	"heading-2": ["auto.components.editor.rich.markdown.slash.commands.c209a116b7", "Heading 2"],
	"heading-3": ["auto.components.editor.rich.markdown.slash.commands.30566ee962", "Heading 3"],
	"heading-4": ["auto.components.editor.rich.markdown.slash.commands.5f9a0ed7c4", "Heading 4"],
	"heading-5": ["auto.components.editor.rich.markdown.slash.commands.8440fa4acf", "Heading 5"]
};
function toggleHeadingPlaceholder(variant) {
	const [key, fallback] = TOGGLE_HEADING_PLACEHOLDERS[variant];
	return translate(key, fallback);
}
function getRichMarkdownPlaceholder({ editor, node, pos }) {
	if (node.type.name !== "detailsSummary") return RICH_MARKDOWN_PLACEHOLDER;
	const parent = editor.state.doc.resolve(pos).parent;
	if (parent.type.name !== "details") return TOGGLE_TEXT_PLACEHOLDER;
	const variant = parseToggleHeadingVariant(parent.attrs.variant);
	return variant ? toggleHeadingPlaceholder(variant) : TOGGLE_TEXT_PLACEHOLDER;
}
function moveDetailsSummarySelectionToContent(editor) {
	const { state, view } = editor;
	const { selection } = state;
	const { $from, empty: empty$1 } = selection;
	if (!empty$1 || $from.parent.type.name !== "detailsSummary") return false;
	const detailsDepth = $from.depth - 1;
	if (detailsDepth < 1) return false;
	const detailsNode = $from.node(detailsDepth);
	if (detailsNode.type.name !== "details" || detailsNode.attrs.open === false) return false;
	const detailsContent = detailsNode.child(1);
	if (detailsContent?.type.name !== "detailsContent") return false;
	const detailsContentStart = $from.before(detailsDepth) + 1 + detailsNode.child(0).nodeSize;
	if (!detailsContent.firstChild?.isTextblock) return false;
	const targetPos = detailsContentStart + 2;
	const tr$1 = state.tr.setSelection(TextSelection.near(state.doc.resolve(targetPos), 1));
	tr$1.scrollIntoView();
	view.dispatch(tr$1);
	return true;
}
function moveFromEmptyDetailsBodyToSummary(editor) {
	const { state, view } = editor;
	const { selection } = state;
	const { $from, empty: empty$1 } = selection;
	if (!empty$1 || !$from.parent.isTextblock || $from.parent.content.size !== 0) return false;
	if ($from.parentOffset !== 0) return false;
	const detailsContentDepth = $from.depth - 1;
	if (detailsContentDepth < 1) return false;
	const detailsContentNode = $from.node(detailsContentDepth);
	if (detailsContentNode.type.name !== "detailsContent" || detailsContentNode.childCount !== 1 || $from.index(detailsContentDepth) !== 0) return false;
	const detailsDepth = detailsContentDepth - 1;
	const detailsNode = $from.node(detailsDepth);
	if (detailsNode.type.name !== "details" || detailsNode.childCount < 2) return false;
	const summaryNode = detailsNode.child(0);
	if (summaryNode.type.name !== "detailsSummary") return false;
	const summaryTextEnd = $from.before(detailsDepth) + 1 + summaryNode.nodeSize - 1;
	const tr$1 = state.tr.setSelection(TextSelection.near(state.doc.resolve(summaryTextEnd), -1));
	tr$1.scrollIntoView();
	view.dispatch(tr$1);
	return true;
}
function exitEmptyDetailsBody(editor) {
	const { state, view } = editor;
	const { selection } = state;
	const { $from, empty: empty$1 } = selection;
	if (!empty$1 || !$from.parent.isTextblock || $from.parent.content.size !== 0) return false;
	const detailsContentDepth = $from.depth - 1;
	if (detailsContentDepth < 1) return false;
	const detailsContentNode = $from.node(detailsContentDepth);
	if (detailsContentNode.type.name !== "detailsContent") return false;
	if ($from.index(detailsContentDepth) !== detailsContentNode.childCount - 1) return false;
	const detailsDepth = detailsContentDepth - 1;
	if ($from.node(detailsDepth).type.name !== "details") return false;
	const paragraph = state.schema.nodes.paragraph?.createAndFill();
	if (!paragraph) return false;
	const currentBlockFrom = $from.before($from.depth);
	const currentBlockTo = $from.after($from.depth);
	const shouldRemoveTrailingEmptyBlock = detailsContentNode.childCount > 1;
	let insertPos = $from.after(detailsDepth);
	const tr$1 = state.tr;
	if (shouldRemoveTrailingEmptyBlock) {
		tr$1.delete(currentBlockFrom, currentBlockTo);
		insertPos -= currentBlockTo - currentBlockFrom;
	}
	tr$1.insert(insertPos, paragraph);
	tr$1.setSelection(TextSelection.create(tr$1.doc, insertPos + 1));
	tr$1.scrollIntoView();
	view.dispatch(tr$1);
	return true;
}
var OrcaDetails = Details.extend({
	priority: 1e3,
	addAttributes() {
		return {
			...this.parent?.(),
			variant: {
				default: null,
				parseHTML: (element) => parseToggleHeadingVariant(element.getAttribute("data-orca-toggle")),
				renderHTML: ({ variant }) => {
					const parsed = parseToggleHeadingVariant(variant);
					return parsed ? { "data-orca-toggle": parsed } : {};
				}
			}
		};
	},
	addKeyboardShortcuts() {
		const parentShortcuts = this.parent?.() ?? {};
		return {
			...parentShortcuts,
			Enter: ({ editor }) => moveDetailsSummarySelectionToContent(editor) || parentShortcuts.Enter?.({ editor }) || false
		};
	},
	markdownTokenizer: {
		name: "details",
		level: "block",
		start: "<details",
		tokenize(src, _tokens, lexer) {
			const detailsBlock = matchDetailsHtmlBlock(src, 0);
			if (!detailsBlock || !isEditableDetailsHtmlBlock(detailsBlock)) return;
			const summaryHtml = extractDetailsSummaryHtml(detailsBlock.inner);
			if (!summaryHtml) return;
			const summary = decodeHtmlEntities(summaryHtml.content.trim());
			const body = detailsBlock.inner.slice(summaryHtml.rawLength);
			return {
				type: "details",
				raw: detailsBlock.raw,
				block: true,
				attributes: parseDetailsAttributes(detailsBlock.openingAttributes),
				summaryTokens: lexer.inlineTokens(summary),
				bodyTokens: lexer.blockTokens(detailsBodyHtmlToMarkdown(body))
			};
		}
	},
	parseMarkdown: (token, helpers) => {
		const detailsToken = token;
		if (detailsToken.type !== "details") return [];
		const summary = helpers.createNode("detailsSummary", {}, helpers.parseInline(detailsToken.summaryTokens ?? []));
		const body = helpers.parseChildren(detailsToken.bodyTokens ?? []);
		const content = helpers.createNode("detailsContent", {}, body.length > 0 ? body : [helpers.createNode("paragraph")]);
		return helpers.createNode("details", detailsToken.attributes ?? {}, [summary, content]);
	},
	renderMarkdown: (node, helpers) => {
		const summary = node.content?.find((child) => child.type === "detailsSummary");
		const content = node.content?.find((child) => child.type === "detailsContent");
		const summaryText = escapeDetailsHtml(decodeHtmlEntities(helpers.renderChildren(summary?.content ?? [], "")));
		const body = helpers.renderChildren(content?.content ?? [], "\n\n").trim();
		return `<details ${renderDetailsAttributes(node.attrs)}>\n<summary>${summaryText}</summary>\n\n${body}\n\n</details>`;
	}
});
var OrcaDetailsContent = DetailsContent.extend({
	priority: 1e3,
	addKeyboardShortcuts() {
		const parentShortcuts = this.parent?.() ?? {};
		return {
			...parentShortcuts,
			Enter: ({ editor }) => exitEmptyDetailsBody(editor) || parentShortcuts.Enter?.({ editor }) || false,
			Backspace: ({ editor }) => moveFromEmptyDetailsBodyToSummary(editor) || parentShortcuts.Backspace?.({ editor }) || false
		};
	}
});
function createOrcaDetailsExtensions() {
	return [
		OrcaDetails.configure({
			persist: true,
			HTMLAttributes: { class: "orca-details" }
		}),
		DetailsSummary,
		OrcaDetailsContent
	];
}
function renderRichMarkdownDocLinkHtml(node, htmlAttributes) {
	const target = typeof node.attrs.target === "string" ? node.attrs.target : "";
	const label = typeof node.attrs.label === "string" ? node.attrs.label : null;
	return [
		"span",
		mergeAttributes(htmlAttributes, {
			"data-doc-link-target": target,
			...label ? { "data-doc-link-label": label } : {},
			contenteditable: "false",
			class: "rich-markdown-doc-link"
		}),
		label ?? target
	];
}
const DOC_LINK_PATTERN = /\[\[([^[\]\r\n]+)\]\]/g;
var DOC_LINK_OPEN = "[[";
function isDocLinkLiteralCodeTextNode(node, parent) {
	return parent?.type.spec.code === true || node.marks.some((mark) => mark.type.name === "code");
}
function canHoldDocLink(node, parent) {
	return node.type.name === "text" && !!node.text && node.text.includes(DOC_LINK_OPEN) && !isDocLinkLiteralCodeTextNode(node, parent);
}
var docLinkDissolveKey = new PluginKey("docLinkDissolve");
var docLinkAutoConvertKey = new PluginKey("docLinkAutoConvert");
var docLinkInlinePreviewKey = new PluginKey("docLinkInlinePreview");
function getDocIndex(storage) {
	if (storage.documents.length === 0) {
		storage._cachedDocs = null;
		storage._cachedIndex = null;
		return null;
	}
	if (storage._cachedDocs !== storage.documents) {
		storage._cachedIndex = createMarkdownDocumentIndex(storage.documents);
		storage._cachedDocs = storage.documents;
	}
	return storage._cachedIndex;
}
function buildPreviewDecorations(state, storage) {
	const decorations = [];
	const index = getDocIndex(storage);
	const cursor = state.selection.from;
	state.doc.descendants((node, pos, parent) => {
		if (!canHoldDocLink(node, parent)) return;
		for (const match of node.text.matchAll(DOC_LINK_PATTERN)) {
			const link = isReservedRichMarkdownTransportBody(match[1]) ? null : parseMarkdownDocLink(match[1]);
			if (!link || match.index === void 0) continue;
			const from = pos + match.index;
			const to = from + match[0].length;
			if (cursor <= from || cursor > to) continue;
			const cls = resolveAgainstIndex(link.target, index) ? "rich-markdown-doc-link-preview" : "rich-markdown-doc-link-preview rich-markdown-doc-link-preview--missing";
			decorations.push(Decoration.inline(from, to, { class: cls }));
		}
	});
	return DecorationSet.create(state.doc, decorations);
}
function resolveAgainstIndex(target, index) {
	if (!index) return false;
	return resolveMarkdownDocLink(target, index).status === "resolved";
}
function getDocLinkTarget(node) {
	return typeof node.attrs.target === "string" ? node.attrs.target : "";
}
function getDocLinkAlias(node) {
	return typeof node.attrs.label === "string" && node.attrs.label ? node.attrs.label : null;
}
function getDocLinkDisplayText(node) {
	return getDocLinkAlias(node) ?? getDocLinkTarget(node);
}
function createMarkdownDocLink(transport) {
	return Node3.create({
		name: "markdownDocLink",
		inline: true,
		group: "inline",
		atom: true,
		selectable: true,
		addStorage() {
			return {
				documents: [],
				_cachedDocs: null,
				_cachedIndex: null
			};
		},
		addAttributes() {
			return {
				target: {
					default: "",
					parseHTML: (el) => el.getAttribute("data-doc-link-target") ?? ""
				},
				label: {
					default: null,
					parseHTML: (el) => el.getAttribute("data-doc-link-label")
				}
			};
		},
		markdownTokenName: "markdownDocLink",
		markdownTokenizer: {
			name: "markdownDocLink",
			level: "inline",
			start: transport.startFor("document-link"),
			tokenize(src) {
				const matched = transport.match(src, "document-link");
				if (!matched) return;
				const link = isReservedRichMarkdownTransportBody(matched.value) ? null : parseMarkdownDocLink(matched.value);
				if (!link) return;
				return {
					type: "markdownDocLink",
					raw: matched.raw,
					text: link.target,
					label: link.alias ?? void 0
				};
			}
		},
		parseMarkdown: (token, helpers) => {
			if (token.type !== "markdownDocLink") return [];
			return helpers.createNode("markdownDocLink", {
				target: typeof token.text === "string" ? token.text : "",
				label: typeof token.label === "string" ? token.label : null
			});
		},
		renderMarkdown: (node) => formatMarkdownDocLink(typeof node.attrs?.target === "string" ? node.attrs.target : "", typeof node.attrs?.label === "string" ? node.attrs.label : null),
		renderText: ({ node }) => getDocLinkDisplayText(node),
		addNodeView() {
			const storage = this.storage;
			return ({ node }) => {
				const target = getDocLinkTarget(node);
				const dom = document.createElement("span");
				dom.setAttribute("data-doc-link-target", target);
				const alias = getDocLinkAlias(node);
				if (alias) dom.setAttribute("data-doc-link-label", alias);
				dom.setAttribute("contenteditable", "false");
				dom.textContent = getDocLinkDisplayText(node);
				const applyResolutionClass = (t) => {
					dom.className = resolveAgainstIndex(t, getDocIndex(storage)) ? "rich-markdown-doc-link" : "rich-markdown-doc-link rich-markdown-doc-link--missing";
				};
				applyResolutionClass(target);
				return {
					dom,
					update: (updatedNode) => {
						if (updatedNode.type.name !== "markdownDocLink") return false;
						const newTarget = getDocLinkTarget(updatedNode);
						const newAlias = getDocLinkAlias(updatedNode);
						dom.setAttribute("data-doc-link-target", newTarget);
						if (newAlias) dom.setAttribute("data-doc-link-label", newAlias);
						else dom.removeAttribute("data-doc-link-label");
						dom.textContent = getDocLinkDisplayText(updatedNode);
						applyResolutionClass(newTarget);
						return true;
					}
				};
			};
		},
		addProseMirrorPlugins() {
			const nodeType = this.type;
			const storage = this.storage;
			return [
				new Plugin({
					key: docLinkDissolveKey,
					props: { handleKeyDown(view, event) {
						if (event.shiftKey || event.altKey || event.metaKey || event.ctrlKey) return false;
						let direction;
						if (event.key === "ArrowLeft") direction = "left";
						else if (event.key === "ArrowRight") direction = "right";
						else return false;
						const { state } = view;
						if (!(state.selection instanceof TextSelection)) return false;
						const { $from } = state.selection;
						const adjacent = direction === "left" ? $from.nodeBefore : $from.nodeAfter;
						if (!adjacent || adjacent.type.name !== "markdownDocLink") return false;
						const text = formatMarkdownDocLink(getDocLinkTarget(adjacent), getDocLinkAlias(adjacent));
						const nodeStart = direction === "left" ? $from.pos - adjacent.nodeSize : $from.pos;
						const nodeEnd = nodeStart + adjacent.nodeSize;
						const tr$1 = state.tr.replaceWith(nodeStart, nodeEnd, state.schema.text(text));
						const cursorPos = direction === "left" ? nodeStart + text.length - 2 : nodeStart + 2;
						tr$1.setSelection(TextSelection.create(tr$1.doc, cursorPos));
						view.dispatch(tr$1);
						return true;
					} }
				}),
				new Plugin({
					key: docLinkAutoConvertKey,
					appendTransaction(_transactions, _oldState, newState) {
						const { tr: tr$1 } = newState;
						const cursor = newState.selection.from;
						let modified = false;
						newState.doc.descendants((node, pos, parent) => {
							if (!canHoldDocLink(node, parent)) return;
							for (const match of node.text.matchAll(DOC_LINK_PATTERN)) {
								const link = isReservedRichMarkdownTransportBody(match[1]) ? null : parseMarkdownDocLink(match[1]);
								if (!link || match.index === void 0) continue;
								const from = pos + match.index;
								const to = from + match[0].length;
								if (cursor > from && cursor <= to) continue;
								const docLinkNode = nodeType.create({
									target: link.target,
									label: link.alias
								});
								tr$1.replaceWith(tr$1.mapping.map(from), tr$1.mapping.map(to), docLinkNode);
								modified = true;
							}
						});
						return modified ? tr$1 : null;
					}
				}),
				new Plugin({
					key: docLinkInlinePreviewKey,
					state: {
						init(_$1, state) {
							return buildPreviewDecorations(state, storage);
						},
						apply(tr$1, prev, oldState, newState) {
							const selectionMoved = !oldState.selection.eq(newState.selection);
							if (!tr$1.docChanged && !selectionMoved && !tr$1.getMeta("docLinksUpdated")) return prev;
							return buildPreviewDecorations(newState, storage);
						}
					},
					props: { decorations(state) {
						return docLinkInlinePreviewKey.getState(state);
					} }
				})
			];
		},
		parseHTML() {
			return [{ tag: "span[data-doc-link-target]" }];
		},
		renderHTML({ HTMLAttributes, node }) {
			return renderRichMarkdownDocLinkHtml(node, HTMLAttributes);
		}
	});
}
var LANGUAGES = [
	{
		value: "",
		get label() {
			return translate("auto.components.editor.RichMarkdownCodeBlock.13822cdfda", "Plain text");
		}
	},
	{
		value: "bash",
		get label() {
			return translate("auto.components.editor.RichMarkdownCodeBlock.4227cf50fe", "Bash");
		}
	},
	{
		value: "c",
		label: "C"
	},
	{
		value: "cpp",
		get label() {
			return translate("auto.components.editor.RichMarkdownCodeBlock.4daed43ae3", "C++");
		}
	},
	{
		value: "css",
		get label() {
			return translate("auto.components.editor.RichMarkdownCodeBlock.026653f21f", "CSS");
		}
	},
	{
		value: "diff",
		get label() {
			return translate("auto.components.editor.RichMarkdownCodeBlock.bf6ee5caaa", "Diff");
		}
	},
	{
		value: "go",
		get label() {
			return translate("auto.components.editor.RichMarkdownCodeBlock.edfcc64182", "Go");
		}
	},
	{
		value: "graphql",
		get label() {
			return translate("auto.components.editor.RichMarkdownCodeBlock.706fd85738", "GraphQL");
		}
	},
	{
		value: "html",
		get label() {
			return translate("auto.components.editor.RichMarkdownCodeBlock.8c4a3fa02d", "HTML");
		}
	},
	{
		value: "java",
		get label() {
			return translate("auto.components.editor.RichMarkdownCodeBlock.36536ad539", "Java");
		}
	},
	{
		value: "javascript",
		get label() {
			return translate("auto.components.editor.RichMarkdownCodeBlock.a209c57063", "JavaScript");
		}
	},
	{
		value: "json",
		get label() {
			return translate("auto.components.editor.RichMarkdownCodeBlock.78eba32de4", "JSON");
		}
	},
	{
		value: "kotlin",
		get label() {
			return translate("auto.components.editor.RichMarkdownCodeBlock.bcb236e2d8", "Kotlin");
		}
	},
	{
		value: "markdown",
		get label() {
			return translate("auto.components.editor.RichMarkdownCodeBlock.983b9576b4", "Markdown");
		}
	},
	{
		value: "mermaid",
		get label() {
			return translate("auto.components.editor.RichMarkdownCodeBlock.89d6cc14fb", "Mermaid");
		}
	},
	{
		value: "python",
		get label() {
			return translate("auto.components.editor.RichMarkdownCodeBlock.2391f9cda9", "Python");
		}
	},
	{
		value: "ruby",
		get label() {
			return translate("auto.components.editor.RichMarkdownCodeBlock.96182a2f64", "Ruby");
		}
	},
	{
		value: "rust",
		get label() {
			return translate("auto.components.editor.RichMarkdownCodeBlock.e72e6b03f4", "Rust");
		}
	},
	{
		value: "scss",
		get label() {
			return translate("auto.components.editor.RichMarkdownCodeBlock.5af8251002", "SCSS");
		}
	},
	{
		value: "shell",
		get label() {
			return translate("auto.components.editor.RichMarkdownCodeBlock.d01f55be57", "Shell");
		}
	},
	{
		value: "sql",
		get label() {
			return translate("auto.components.editor.RichMarkdownCodeBlock.3009f722b9", "SQL");
		}
	},
	{
		value: "swift",
		get label() {
			return translate("auto.components.editor.RichMarkdownCodeBlock.9e384d48dc", "Swift");
		}
	},
	{
		value: "typescript",
		get label() {
			return translate("auto.components.editor.RichMarkdownCodeBlock.88d777bc07", "TypeScript");
		}
	},
	{
		value: "xml",
		get label() {
			return translate("auto.components.editor.RichMarkdownCodeBlock.5ef5605cb7", "XML");
		}
	},
	{
		value: "yaml",
		get label() {
			return translate("auto.components.editor.RichMarkdownCodeBlock.74eab1d9b2", "YAML");
		}
	}
];
function RichMarkdownCodeBlock({ node, updateAttributes: updateAttributes$1 }) {
	useTranslation();
	const language = node.attrs.language || "";
	const [copied, setCopied] = (0, import_react.useState)(false);
	const copiedResetTimerRef = (0, import_react.useRef)(null);
	const isMountedRef = (0, import_react.useRef)(false);
	const settings = useAppStore((s) => s.settings);
	const isDark = settings?.theme === "dark" || settings?.theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches;
	const isMermaid = language === "mermaid";
	const clearCopiedResetTimer = (0, import_react.useCallback)(() => {
		if (copiedResetTimerRef.current !== null) {
			window.clearTimeout(copiedResetTimerRef.current);
			copiedResetTimerRef.current = null;
		}
	}, []);
	const setCopyButtonRef = (0, import_react.useCallback)((node$1) => {
		isMountedRef.current = node$1 !== null;
		if (node$1 === null) clearCopiedResetTimer();
	}, [clearCopiedResetTimer]);
	const onChange = (0, import_react.useCallback)((e) => {
		updateAttributes$1({ language: e.target.value });
	}, [updateAttributes$1]);
	const handleCopy = (0, import_react.useCallback)((e) => {
		e.stopPropagation();
		const text = node.textContent;
		window.api.ui.writeClipboardText(text).then(() => {
			if (!isMountedRef.current) return;
			clearCopiedResetTimer();
			setCopied(true);
			copiedResetTimerRef.current = window.setTimeout(() => {
				copiedResetTimerRef.current = null;
				setCopied(false);
			}, 1500);
		}).catch(() => {});
	}, [clearCopiedResetTimer, node]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(NodeViewWrapper, {
		className: "rich-markdown-code-block-wrapper",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("select", {
				className: "rich-markdown-code-block-lang",
				contentEditable: false,
				value: language,
				onChange,
				children: [LANGUAGES.map((lang) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("option", {
					value: lang.value,
					children: lang.label
				}, lang.value)), language && !LANGUAGES.some((l) => l.value === language) ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("option", {
					value: language,
					children: language
				}) : null]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				ref: setCopyButtonRef,
				type: "button",
				className: "code-block-copy-btn",
				contentEditable: false,
				onClick: handleCopy,
				"aria-label": translate("auto.components.editor.RichMarkdownCodeBlock.c72beafc0f", "Copy code"),
				title: translate("auto.components.editor.RichMarkdownCodeBlock.c72beafc0f", "Copy code"),
				children: copied ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, { size: 14 }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "code-block-copy-label",
					children: translate("auto.components.editor.RichMarkdownCodeBlock.232d9ed853", "Copied")
				})] }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Copy, { size: 14 })
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(NodeViewContent, { as: "pre" }),
			isMermaid && node.textContent.trim() && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				contentEditable: false,
				className: "mermaid-preview",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MermaidBlock, {
					content: node.textContent.trim(),
					isDark,
					htmlLabels: false
				})
			})
		]
	});
}
function safeReactNodeViewRenderer(component, options) {
	const factory = ReactNodeViewRenderer(component, options);
	return (props) => {
		const nodeView = factory(props);
		if (!("handleSelectionUpdate" in nodeView)) return nodeView;
		const nv = nodeView;
		const originalBound = nv.handleSelectionUpdate;
		nv.editor.off("selectionUpdate", originalBound);
		nv.handleSelectionUpdate = function patchedHandleSelectionUpdate() {
			if (this.editor.state.selection instanceof NodeSelection) originalBound();
			else if (this.renderer?.props?.selected) this.deselectNode();
		};
		nv.handleSelectionUpdate = nv.handleSelectionUpdate.bind(nv);
		nv.editor.on("selectionUpdate", nv.handleSelectionUpdate);
		return nodeView;
	};
}
const DragSelectionGuard = Extension.create({
	name: "dragSelectionGuard",
	addProseMirrorPlugins() {
		let viewRef = null;
		let suppressedDuringDrag = false;
		return [new Plugin({
			key: new PluginKey("dragSelectionGuard"),
			filterTransaction(tr$1) {
				if (!viewRef) return true;
				const mouseDown = viewRef.input.mouseDown;
				if (mouseDown && mouseDown.allowDefault && tr$1.selection instanceof CellSelection) return false;
				return true;
			},
			view(editorView) {
				viewRef = editorView;
				const observer = viewRef.domObserver;
				const doc$2 = editorView.dom.ownerDocument;
				const originalOnSelectionChange = observer.onSelectionChange;
				let mouseUpFrameId = null;
				doc$2.removeEventListener("selectionchange", originalOnSelectionChange);
				const patchedOnSelectionChange = () => {
					const mouseDown = viewRef.input.mouseDown;
					if (mouseDown && mouseDown.allowDefault) {
						observer.setCurSelection();
						suppressedDuringDrag = true;
						return;
					}
					originalOnSelectionChange();
				};
				observer.onSelectionChange = patchedOnSelectionChange;
				doc$2.addEventListener("selectionchange", patchedOnSelectionChange);
				const handleMouseUp = () => {
					if (!suppressedDuringDrag) return;
					suppressedDuringDrag = false;
					if (mouseUpFrameId !== null) cancelAnimationFrame(mouseUpFrameId);
					mouseUpFrameId = requestAnimationFrame(() => {
						mouseUpFrameId = null;
						if (!viewRef || !editorView.dom.isConnected) return;
						const mouseDown = viewRef?.input?.mouseDown;
						if (mouseDown && mouseDown.allowDefault) return;
						const domSel = viewRef.domSelectionRange();
						const savedAnchor = domSel.anchorNode;
						const savedAnchorOff = domSel.anchorOffset;
						const savedFocus = domSel.focusNode;
						const savedFocusOff = domSel.focusOffset;
						observer.currentSelection.set({
							anchorNode: null,
							anchorOffset: 0,
							focusNode: null,
							focusOffset: 0
						});
						observer.flush();
						if (savedAnchor && savedFocus && savedAnchor.isConnected && savedFocus.isConnected) {
							const sel = doc$2.getSelection();
							if (sel) {
								observer.stop();
								sel.setBaseAndExtent(savedAnchor, savedAnchorOff, savedFocus, savedFocusOff);
								observer.setCurSelection();
								observer.start();
							}
						}
					});
				};
				doc$2.addEventListener("mouseup", handleMouseUp);
				return { destroy() {
					if (mouseUpFrameId !== null) {
						cancelAnimationFrame(mouseUpFrameId);
						mouseUpFrameId = null;
					}
					doc$2.removeEventListener("mouseup", handleMouseUp);
					doc$2.removeEventListener("selectionchange", patchedOnSelectionChange);
					observer.onSelectionChange = originalOnSelectionChange;
					doc$2.addEventListener("selectionchange", originalOnSelectionChange);
					viewRef = null;
				} };
			}
		})];
	}
});
const richMarkdownAnnotationHighlightPluginKey = new PluginKey("richMarkdownAnnotationHighlight");
function createAnnotationDecorations(doc$2, activeRange, noteRanges) {
	const decorations = [...noteRanges.map((range) => ({
		range,
		active: false
	})), ...activeRange ? [{
		range: activeRange,
		active: true
	}] : []].map((range) => {
		const from = Math.min(range.range.from, range.range.to);
		const to = Math.max(range.range.from, range.range.to);
		return from === to ? null : Decoration.inline(from, to, { class: range.active ? "rich-markdown-annotation-selection rich-markdown-annotation-selection-active" : "rich-markdown-annotation-selection" });
	}).filter((decoration) => decoration !== null);
	return decorations.length === 0 ? DecorationSet.empty : DecorationSet.create(doc$2, decorations);
}
function createRichMarkdownAnnotationHighlightPlugin() {
	return new Plugin({
		key: richMarkdownAnnotationHighlightPluginKey,
		state: {
			init: () => ({
				activeRange: null,
				noteRanges: [],
				decorations: DecorationSet.empty
			}),
			apply: (tr$1, pluginState) => {
				const meta = tr$1.getMeta(richMarkdownAnnotationHighlightPluginKey);
				if (meta === null) return {
					activeRange: null,
					noteRanges: pluginState.noteRanges,
					decorations: createAnnotationDecorations(tr$1.doc, null, pluginState.noteRanges)
				};
				if (meta) {
					const activeRange = meta.activeRange === void 0 ? pluginState.activeRange : meta.activeRange;
					const noteRanges = meta.noteRanges === void 0 ? pluginState.noteRanges : meta.noteRanges;
					return {
						activeRange,
						noteRanges,
						decorations: createAnnotationDecorations(tr$1.doc, activeRange, noteRanges)
					};
				}
				if (tr$1.docChanged) return {
					...pluginState,
					decorations: pluginState.decorations.map(tr$1.mapping, tr$1.doc)
				};
				return pluginState;
			}
		},
		props: { decorations(state) {
			return richMarkdownAnnotationHighlightPluginKey.getState(state)?.decorations ?? DecorationSet.empty;
		} }
	});
}
function createRichMarkdownAnnotationHighlightExtension() {
	return Extension.create({
		name: "richMarkdownAnnotationHighlight",
		addProseMirrorPlugins() {
			return [createRichMarkdownAnnotationHighlightPlugin()];
		}
	});
}
function createRichMarkdownHtmlSuperscriptLinkContext(initial) {
	let snapshot = {
		...initial,
		version: 0
	};
	const listeners = /* @__PURE__ */ new Set();
	return {
		getSnapshot: () => snapshot,
		subscribe: (listener) => {
			listeners.add(listener);
			return () => listeners.delete(listener);
		},
		update: (next) => {
			if (next.sourceFilePath === snapshot.sourceFilePath && next.worktreeId === snapshot.worktreeId && next.worktreeRoot === snapshot.worktreeRoot && sameOwner(next.sourceOwner, snapshot.sourceOwner)) return;
			snapshot = {
				...next,
				version: snapshot.version + 1
			};
			listeners.forEach((listener) => listener());
		}
	};
}
function classifyHtmlSuperscriptLinkAction(href, snapshot) {
	if (snapshot.sourceOwner.kind === "unknown" || /^[\t\n\f\r ]*$/.test(href)) return false;
	const target = resolveMarkdownLinkTarget(href, snapshot.sourceFilePath, snapshot.worktreeRoot);
	if (!target) return false;
	return !(target.kind === "file" && target.relativePath === void 0 && (snapshot.sourceOwner.kind === "runtime" || snapshot.sourceOwner.kind === "ssh"));
}
function sameOwner(left, right) {
	if (left.kind !== right.kind) return false;
	if (left.kind === "runtime" && right.kind === "runtime") return left.runtimeEnvironmentId === right.runtimeEnvironmentId;
	if (left.kind === "ssh" && right.kind === "ssh") return left.connectionId === right.connectionId;
	return true;
}
var CLIPBOARD_VERSION = "1";
var MARKER_ATTRIBUTE = "data-rich-markdown-html-superscript-link";
var SOURCE_ATTRIBUTE = "data-orca-superscript-link-source";
var clipboardEncoder = new TextEncoder();
function createRichMarkdownHtmlSuperscriptLink(transport, context) {
	return Node3.create({
		name: "richMarkdownHtmlSuperscriptLink",
		inline: true,
		group: "inline",
		atom: true,
		selectable: true,
		addAttributes() {
			return {
				source: {
					default: "",
					rendered: false
				},
				href: {
					default: "",
					rendered: false
				},
				label: {
					default: "",
					rendered: false
				},
				title: {
					default: null,
					rendered: false
				}
			};
		},
		markdownTokenName: "richMarkdownHtmlSuperscriptLink",
		markdownTokenizer: {
			name: "richMarkdownHtmlSuperscriptLink",
			level: "inline",
			start: transport.startFor("html-superscript-link"),
			tokenize(source) {
				const matched = transport.match(source, "html-superscript-link");
				if (!matched) return;
				const parsed = parseStructuredPayload(matched.value);
				if (!parsed) return;
				return {
					type: "richMarkdownHtmlSuperscriptLink",
					raw: matched.raw,
					citation: parsed
				};
			}
		},
		parseMarkdown: (token, helpers) => {
			const citation = token.citation;
			if (token.type !== "richMarkdownHtmlSuperscriptLink" || !citation) return [];
			return helpers.createNode("richMarkdownHtmlSuperscriptLink", citation);
		},
		renderMarkdown: (node) => String(node.attrs?.source ?? ""),
		renderText: ({ node }) => String(node.attrs.label ?? ""),
		parseHTML() {
			return [{
				tag: `sup[${MARKER_ATTRIBUTE}]`,
				getAttrs: (element) => validateClipboardElement(element)
			}];
		},
		renderHTML({ node }) {
			const citation = node.attrs;
			const projectedHref = projectMarkdownHrefForClipboard(citation.href);
			const anchorAttributes = {};
			if (projectedHref !== null) anchorAttributes.href = projectedHref;
			if (citation.title !== null) anchorAttributes.title = citation.title;
			return [
				"sup",
				{
					[MARKER_ATTRIBUTE]: CLIPBOARD_VERSION,
					[SOURCE_ATTRIBUTE]: citation.source
				},
				[
					"a",
					anchorAttributes,
					citation.label
				]
			];
		},
		addNodeView() {
			return ({ node }) => {
				const dom = document.createElement("sup");
				dom.setAttribute(MARKER_ATTRIBUTE, "");
				dom.setAttribute("contenteditable", "false");
				const label = document.createElement("span");
				label.className = "rich-markdown-html-superscript-link";
				label.textContent = String(node.attrs.label ?? "");
				dom.appendChild(label);
				const updateActionability = () => {
					const href = String(node.attrs.href ?? "");
					const actionable = classifyHtmlSuperscriptLinkAction(href, context.getSnapshot());
					label.setAttribute("aria-label", actionable ? translate("auto.components.editor.richMarkdownHtmlSuperscriptLink.availableAriaLabel", "{{value0}}, link to {{value1}}", {
						value0: String(node.attrs.label ?? ""),
						value1: href
					}) : translate("auto.components.editor.richMarkdownHtmlSuperscriptLink.unavailableAriaLabel", "{{value0}}, citation link unavailable", { value0: String(node.attrs.label ?? "") }));
					if (actionable) label.setAttribute("role", "link");
					else label.removeAttribute("role");
					label.toggleAttribute("data-actionable", actionable);
				};
				updateActionability();
				return {
					dom,
					destroy: context.subscribe(updateActionability)
				};
			};
		}
	});
}
function parseStructuredPayload(value) {
	let candidate;
	try {
		candidate = JSON.parse(value);
	} catch {
		return null;
	}
	if (!isCitationShape(candidate)) return null;
	const parsed = parseHtmlSuperscriptLinkSource(candidate.source);
	return parsed && sameCitation(parsed, candidate) ? parsed : null;
}
function validateClipboardElement(element) {
	if (element.getAttribute(MARKER_ATTRIBUTE) !== CLIPBOARD_VERSION || !hasOnlyAttributes(element, [
		MARKER_ATTRIBUTE,
		SOURCE_ATTRIBUTE,
		"data-pm-slice"
	])) return false;
	const source = element.getAttribute(SOURCE_ATTRIBUTE);
	if (!source || source.length > 16384 || clipboardEncoder.encode(source).byteLength > 16384) return false;
	const parsed = parseHtmlSuperscriptLinkSource(source);
	const anchor = element.firstElementChild;
	if (!parsed || element.childNodes.length !== 1 || element.children.length !== 1 || !anchor || element.firstChild !== anchor || anchor.tagName !== "A" || !hasOnlyAttributes(anchor, ["href", "title"]) || anchor.childNodes.length !== 1 || anchor.firstChild?.nodeType !== window.Node.TEXT_NODE || anchor.textContent !== parsed.label || anchor.getAttribute("title") !== parsed.title || anchor.getAttribute("href") !== projectMarkdownHrefForClipboard(parsed.href)) return false;
	return parsed;
}
function hasOnlyAttributes(element, allowed) {
	const allowedSet = new Set(allowed);
	return Array.from(element.attributes).every((attribute) => allowedSet.has(attribute.name));
}
function isCitationShape(value) {
	if (!value || typeof value !== "object") return false;
	const candidate = value;
	return Object.keys(candidate).length === 4 && typeof candidate.source === "string" && typeof candidate.href === "string" && typeof candidate.label === "string" && (typeof candidate.title === "string" || candidate.title === null);
}
function sameCitation(left, right) {
	return left.source === right.source && left.href === right.href && left.label === right.label && left.title === right.title;
}
var index_default$6 = TaskList;
var baseTokenizer = index_default$6.config.markdownTokenizer;
function normalizeTaskListToken(token, lexer) {
	const firstNested = token.nestedTokens?.[0];
	if (token.type === "taskItem" && firstNested?.type === "code" && firstNested.codeBlockStyle === "indented" && typeof firstNested.text === "string") {
		firstNested.type = "paragraph";
		firstNested.raw = firstNested.text;
		firstNested.tokens = lexer.inlineTokens(firstNested.text);
		delete firstNested.codeBlockStyle;
	}
	for (const child of [...token.items ?? [], ...token.nestedTokens ?? []]) normalizeTaskListToken(child, lexer);
}
const RichMarkdownTaskList = index_default$6.extend({ markdownTokenizer: {
	...baseTokenizer,
	tokenize(src, tokens, lexer) {
		const token = baseTokenizer.tokenize(src, tokens, lexer);
		if (token) normalizeTaskListToken(token, lexer);
		return token;
	}
} });
var lowlight = createLowlight(grammars);
var RichMarkdownLink = index_default$1.extend({ priority: 90 });
var RichMarkdownCode = Code.extend({ excludes: "code bold italic strike underline" });
function createRichMarkdownExtensions({ codec, includePlaceholder = false, htmlSuperscriptLinks = false, htmlSuperscriptLinkContext }) {
	if (htmlSuperscriptLinks && !htmlSuperscriptLinkContext) throw new Error("HTML superscript links require a document interaction context");
	const extensions = [
		index_default.configure({
			link: false,
			code: false,
			codeBlock: false
		}),
		RichMarkdownCode,
		index_default$3.extend({ addNodeView() {
			return safeReactNodeViewRenderer(RichMarkdownCodeBlock);
		} }).configure({
			lowlight,
			defaultLanguage: null
		}),
		RichMarkdownLink.configure({
			openOnClick: false,
			autolink: true,
			linkOnPaste: true
		}),
		index_default$2.extend({
			addStorage() {
				return {
					contextVersion: 0,
					filePath: "",
					reloadListeners: /* @__PURE__ */ new Set(),
					runtimeContext: void 0
				};
			},
			addNodeView() {
				return ({ node, HTMLAttributes }) => {
					const dom = document.createElement("div");
					dom.style.lineHeight = "0";
					const img = document.createElement("img");
					img.draggable = false;
					for (const [key, value] of Object.entries(HTMLAttributes)) if (key !== "src" && value != null && value !== false) img.setAttribute(key, String(value));
					dom.appendChild(img);
					let currentSrc = node.attrs.src;
					let currentContextVersion = getImageContextVersion(this.storage);
					const loadImage = (src) => {
						const fp = this.storage.filePath;
						const runtimeContext = this.storage.runtimeContext;
						const contextVersionAtLoad = getImageContextVersion(this.storage);
						if (src && fp) loadLocalImageSrc(src, fp, void 0, runtimeContext).then((resolved) => {
							if (currentSrc !== src || currentContextVersion !== contextVersionAtLoad) return;
							if (resolved) {
								img.src = resolved;
								return;
							}
							img.removeAttribute("src");
						});
						else if (src) img.src = src;
						else img.removeAttribute("src");
					};
					loadImage(currentSrc);
					const unsubscribe = onImageCacheInvalidated(() => {
						loadImage(currentSrc);
					});
					const reloadForContextChange = () => {
						currentContextVersion = getImageContextVersion(this.storage);
						loadImage(currentSrc);
					};
					const reloadListeners = this.storage.reloadListeners;
					if (reloadListeners instanceof Set) reloadListeners.add(reloadForContextChange);
					return {
						dom,
						update: (updatedNode) => {
							if (updatedNode.type.name !== "image") return false;
							const newSrc = updatedNode.attrs.src;
							const nextContextVersion = getImageContextVersion(this.storage);
							if (newSrc !== currentSrc || nextContextVersion !== currentContextVersion) {
								currentSrc = newSrc;
								currentContextVersion = nextContextVersion;
								loadImage(newSrc);
							}
							return true;
						},
						destroy: () => {
							if (reloadListeners instanceof Set) reloadListeners.delete(reloadForContextChange);
							unsubscribe();
						}
					};
				};
			}
		}).configure({ allowBase64: true }),
		RichMarkdownTaskList,
		index_default$5.configure({ nested: true }),
		...createOrcaDetailsExtensions(),
		Table.configure({ resizable: false }),
		TableRow,
		TableHeader,
		TableCell,
		InlineMath.configure({ katexOptions: { throwOnError: false } }),
		BlockMath.configure({ katexOptions: {
			displayMode: true,
			throwOnError: false
		} }),
		createRichMarkdownLiteral(codec.transport),
		...htmlSuperscriptLinks ? [createRichMarkdownHtmlSuperscriptLink(codec.transport, htmlSuperscriptLinkContext)] : [],
		createRawMarkdownHtmlInline(codec.transport),
		createRawMarkdownHtmlBlock(codec.transport),
		createMarkdownDocLink(codec.transport),
		DragSelectionGuard,
		Markdown.configure({
			marked: codec.marked,
			markedOptions: { gfm: true }
		}),
		createRichMarkdownAnnotationHighlightExtension()
	];
	if (includePlaceholder) extensions.push(index_default$4.configure({
		includeChildren: true,
		placeholder: getRichMarkdownPlaceholder
	}));
	return extensions;
}
function getImageContextVersion(storage) {
	const version = storage.contextVersion;
	return typeof version === "number" ? version : 0;
}
export { TextSelection as C, PluginKey as S, Fragment as T, Editor as _, encodeRawMarkdownHtmlForRichEditor as a, NodeSelection as b, TableMap as c, nextCell as d, selectionCell as f, useEditorState as g, useEditor as h, richMarkdownAnnotationHighlightPluginKey as i, isInTable as l, EditorContent as m, classifyHtmlSuperscriptLinkAction as n, createRichMarkdownEditorCodec as o, index_default$4 as p, createRichMarkdownHtmlSuperscriptLinkContext as r, CellSelection as s, createRichMarkdownExtensions as t, moveCellForward as u, Decoration as v, DOMSerializer as w, Plugin as x, DecorationSet as y };
