import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
var isIterable = (obj) => Symbol.iterator in obj;
var hasIterableEntries = (value) => "entries" in value;
var compareEntries = (valueA, valueB) => {
	const mapA = valueA instanceof Map ? valueA : new Map(valueA.entries());
	const mapB = valueB instanceof Map ? valueB : new Map(valueB.entries());
	if (mapA.size !== mapB.size) return false;
	for (const [key, value] of mapA) if (!mapB.has(key) || !Object.is(value, mapB.get(key))) return false;
	return true;
};
var compareIterables = (valueA, valueB) => {
	const iteratorA = valueA[Symbol.iterator]();
	const iteratorB = valueB[Symbol.iterator]();
	let nextA = iteratorA.next();
	let nextB = iteratorB.next();
	while (!nextA.done && !nextB.done) {
		if (!Object.is(nextA.value, nextB.value)) return false;
		nextA = iteratorA.next();
		nextB = iteratorB.next();
	}
	return !!nextA.done && !!nextB.done;
};
function shallow(valueA, valueB) {
	if (Object.is(valueA, valueB)) return true;
	if (typeof valueA !== "object" || valueA === null || typeof valueB !== "object" || valueB === null) return false;
	if (Object.getPrototypeOf(valueA) !== Object.getPrototypeOf(valueB)) return false;
	if (isIterable(valueA) && isIterable(valueB)) {
		if (hasIterableEntries(valueA) && hasIterableEntries(valueB)) return compareEntries(valueA, valueB);
		return compareIterables(valueA, valueB);
	}
	return compareEntries({ entries: () => Object.entries(valueA) }, { entries: () => Object.entries(valueB) });
}
var import_react = /* @__PURE__ */ __toESM(require_react(), 1);
function useShallow(selector) {
	const prev = import_react.useRef(void 0);
	return (state) => {
		const next = selector(state);
		return shallow(prev.current, next) ? prev.current : prev.current = next;
	};
}
export { useShallow as t };
