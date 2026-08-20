import { n as __esmMin } from "./chunk-Dhmk_5SA.js";
function _checkPrivateRedeclaration(e, t) {
	if (t.has(e)) throw new TypeError("Cannot initialize the same private elements twice on an object");
}
var init_checkPrivateRedeclaration = __esmMin((() => {}));
function _classPrivateFieldInitSpec(e, t, a) {
	_checkPrivateRedeclaration(e, t), t.set(e, a);
}
var init_classPrivateFieldInitSpec = __esmMin((() => {
	init_checkPrivateRedeclaration();
}));
function _assertClassBrand(e, t, n) {
	if ("function" == typeof e ? e === t : e.has(t)) return arguments.length < 3 ? t : n;
	throw new TypeError("Private element is not present on this object");
}
var init_assertClassBrand = __esmMin((() => {}));
function _classPrivateFieldSet2(s, a, r) {
	return s.set(_assertClassBrand(s, a), r), r;
}
var init_classPrivateFieldSet2 = __esmMin((() => {
	init_assertClassBrand();
}));
function _classPrivateFieldGet2(s, a) {
	return s.get(_assertClassBrand(s, a));
}
var init_classPrivateFieldGet2 = __esmMin((() => {
	init_assertClassBrand();
}));
export { _assertClassBrand as a, init_classPrivateFieldInitSpec as c, init_classPrivateFieldSet2 as i, _checkPrivateRedeclaration as l, init_classPrivateFieldGet2 as n, init_assertClassBrand as o, _classPrivateFieldSet2 as r, _classPrivateFieldInitSpec as s, _classPrivateFieldGet2 as t, init_checkPrivateRedeclaration as u };
