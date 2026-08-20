const __vite__mapDeps=(i,m=__vite__mapDeps,d=(m.f||(m.f=["./textmate-token-provider-C5pkZdSV.js","./preload-helper-Cgw39-ka.js","./chunk-Dhmk_5SA.js"])))=>i.map(i=>d[i]);
import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate } from "./jsx-runtime-Cv_nyRjc.js";
import { cu as getUtf8ChunkEndIndex, no as yieldToEventLoop } from "./store-CgXrfmaH.js";
import { n as toast } from "./dist-DgqligFk.js";
import { t as __vitePreload } from "./preload-helper-Cgw39-ka.js";
import { n as measureTextControlPasteByteLengthWithYield, t as measureTextControlPasteByteLength } from "./text-control-paste-PhBVbE2p.js";
import { _ as PasteAction, g as ReferenceWidget, v as InMemoryClipboardMetadataManager, y as Delayer } from "./editor.api2-DX_-Ye6K.js";
import { i as typescriptDefaults, n as javascriptDefaults, t as JsxEmit } from "./monaco.contribution-BINL69Me.js";
import { t as editor_main_exports } from "./editor.main-BGL6BKIn.js";
function _arrayLikeToArray(r, a) {
	(null == a || a > r.length) && (a = r.length);
	for (var e = 0, n = Array(a); e < a; e++) n[e] = r[e];
	return n;
}
function _arrayWithHoles(r) {
	if (Array.isArray(r)) return r;
}
function _defineProperty$1(e, r, t) {
	return (r = _toPropertyKey(r)) in e ? Object.defineProperty(e, r, {
		value: t,
		enumerable: true,
		configurable: true,
		writable: true
	}) : e[r] = t, e;
}
function _iterableToArrayLimit(r, l$1) {
	var t = null == r ? null : "undefined" != typeof Symbol && r[Symbol.iterator] || r["@@iterator"];
	if (null != t) {
		var e, n, i, u, a = [], f = true, o = false;
		try {
			if (i = (t = t.call(r)).next, 0 === l$1);
			else for (; !(f = (e = i.call(t)).done) && (a.push(e.value), a.length !== l$1); f = !0);
		} catch (r$1) {
			o = true, n = r$1;
		} finally {
			try {
				if (!f && null != t.return && (u = t.return(), Object(u) !== u)) return;
			} finally {
				if (o) throw n;
			}
		}
		return a;
	}
}
function _nonIterableRest() {
	throw new TypeError("Invalid attempt to destructure non-iterable instance.\nIn order to be iterable, non-array objects must have a [Symbol.iterator]() method.");
}
function ownKeys$1(e, r) {
	var t = Object.keys(e);
	if (Object.getOwnPropertySymbols) {
		var o = Object.getOwnPropertySymbols(e);
		r && (o = o.filter(function(r$1) {
			return Object.getOwnPropertyDescriptor(e, r$1).enumerable;
		})), t.push.apply(t, o);
	}
	return t;
}
function _objectSpread2(e) {
	for (var r = 1; r < arguments.length; r++) {
		var t = null != arguments[r] ? arguments[r] : {};
		r % 2 ? ownKeys$1(Object(t), true).forEach(function(r$1) {
			_defineProperty$1(e, r$1, t[r$1]);
		}) : Object.getOwnPropertyDescriptors ? Object.defineProperties(e, Object.getOwnPropertyDescriptors(t)) : ownKeys$1(Object(t)).forEach(function(r$1) {
			Object.defineProperty(e, r$1, Object.getOwnPropertyDescriptor(t, r$1));
		});
	}
	return e;
}
function _objectWithoutProperties(e, t) {
	if (null == e) return {};
	var o, r, i = _objectWithoutPropertiesLoose(e, t);
	if (Object.getOwnPropertySymbols) {
		var n = Object.getOwnPropertySymbols(e);
		for (r = 0; r < n.length; r++) o = n[r], -1 === t.indexOf(o) && {}.propertyIsEnumerable.call(e, o) && (i[o] = e[o]);
	}
	return i;
}
function _objectWithoutPropertiesLoose(r, e) {
	if (null == r) return {};
	var t = {};
	for (var n in r) if ({}.hasOwnProperty.call(r, n)) {
		if (-1 !== e.indexOf(n)) continue;
		t[n] = r[n];
	}
	return t;
}
function _slicedToArray(r, e) {
	return _arrayWithHoles(r) || _iterableToArrayLimit(r, e) || _unsupportedIterableToArray(r, e) || _nonIterableRest();
}
function _toPrimitive(t, r) {
	if ("object" != typeof t || !t) return t;
	var e = t[Symbol.toPrimitive];
	if (void 0 !== e) {
		var i = e.call(t, r);
		if ("object" != typeof i) return i;
		throw new TypeError("@@toPrimitive must return a primitive value.");
	}
	return ("string" === r ? String : Number)(t);
}
function _toPropertyKey(t) {
	var i = _toPrimitive(t, "string");
	return "symbol" == typeof i ? i : i + "";
}
function _unsupportedIterableToArray(r, a) {
	if (r) {
		if ("string" == typeof r) return _arrayLikeToArray(r, a);
		var t = {}.toString.call(r).slice(8, -1);
		return "Object" === t && r.constructor && (t = r.constructor.name), "Map" === t || "Set" === t ? Array.from(r) : "Arguments" === t || /^(?:Ui|I)nt(?:8|16|32)(?:Clamped)?Array$/.test(t) ? _arrayLikeToArray(r, a) : void 0;
	}
}
function _defineProperty(obj, key, value) {
	if (key in obj) Object.defineProperty(obj, key, {
		value,
		enumerable: true,
		configurable: true,
		writable: true
	});
	else obj[key] = value;
	return obj;
}
function ownKeys(object, enumerableOnly) {
	var keys = Object.keys(object);
	if (Object.getOwnPropertySymbols) {
		var symbols = Object.getOwnPropertySymbols(object);
		if (enumerableOnly) symbols = symbols.filter(function(sym) {
			return Object.getOwnPropertyDescriptor(object, sym).enumerable;
		});
		keys.push.apply(keys, symbols);
	}
	return keys;
}
function _objectSpread2$1(target) {
	for (var i = 1; i < arguments.length; i++) {
		var source = arguments[i] != null ? arguments[i] : {};
		if (i % 2) ownKeys(Object(source), true).forEach(function(key) {
			_defineProperty(target, key, source[key]);
		});
		else if (Object.getOwnPropertyDescriptors) Object.defineProperties(target, Object.getOwnPropertyDescriptors(source));
		else ownKeys(Object(source)).forEach(function(key) {
			Object.defineProperty(target, key, Object.getOwnPropertyDescriptor(source, key));
		});
	}
	return target;
}
function compose$1() {
	for (var _len = arguments.length, fns = new Array(_len), _key = 0; _key < _len; _key++) fns[_key] = arguments[_key];
	return function(x) {
		return fns.reduceRight(function(y, f) {
			return f(y);
		}, x);
	};
}
function curry$1(fn) {
	return function curried() {
		var _this = this;
		for (var _len2 = arguments.length, args = new Array(_len2), _key2 = 0; _key2 < _len2; _key2++) args[_key2] = arguments[_key2];
		return args.length >= fn.length ? fn.apply(this, args) : function() {
			for (var _len3 = arguments.length, nextArgs = new Array(_len3), _key3 = 0; _key3 < _len3; _key3++) nextArgs[_key3] = arguments[_key3];
			return curried.apply(_this, [].concat(args, nextArgs));
		};
	};
}
function isObject$1(value) {
	return {}.toString.call(value).includes("Object");
}
function isEmpty(obj) {
	return !Object.keys(obj).length;
}
function isFunction(value) {
	return typeof value === "function";
}
function hasOwnProperty(object, property) {
	return Object.prototype.hasOwnProperty.call(object, property);
}
function validateChanges(initial, changes) {
	if (!isObject$1(changes)) errorHandler$1("changeType");
	if (Object.keys(changes).some(function(field) {
		return !hasOwnProperty(initial, field);
	})) errorHandler$1("changeField");
	return changes;
}
function validateSelector(selector) {
	if (!isFunction(selector)) errorHandler$1("selectorType");
}
function validateHandler(handler) {
	if (!(isFunction(handler) || isObject$1(handler))) errorHandler$1("handlerType");
	if (isObject$1(handler) && Object.values(handler).some(function(_handler) {
		return !isFunction(_handler);
	})) errorHandler$1("handlersType");
}
function validateInitial(initial) {
	if (!initial) errorHandler$1("initialIsRequired");
	if (!isObject$1(initial)) errorHandler$1("initialType");
	if (isEmpty(initial)) errorHandler$1("initialContent");
}
function throwError$1(errorMessages$1, type) {
	throw new Error(errorMessages$1[type] || errorMessages$1["default"]);
}
var errorHandler$1 = curry$1(throwError$1)({
	initialIsRequired: "initial state is required",
	initialType: "initial state should be an object",
	initialContent: "initial state shouldn't be an empty object",
	handlerType: "handler should be an object or a function",
	handlersType: "all handlers should be a functions",
	selectorType: "selector should be a function",
	changeType: "provided value of changes should be an object",
	changeField: "it seams you want to change a field in the state which is not specified in the \"initial\" state",
	"default": "an unknown error accured in `state-local` package"
});
var validators$1 = {
	changes: validateChanges,
	selector: validateSelector,
	handler: validateHandler,
	initial: validateInitial
};
function create(initial) {
	var handler = arguments.length > 1 && arguments[1] !== void 0 ? arguments[1] : {};
	validators$1.initial(initial);
	validators$1.handler(handler);
	var state = { current: initial };
	var didUpdate = curry$1(didStateUpdate)(state, handler);
	var update = curry$1(updateState)(state);
	var validate = curry$1(validators$1.changes)(initial);
	var getChanges = curry$1(extractChanges)(state);
	function getState$1() {
		var selector = arguments.length > 0 && arguments[0] !== void 0 ? arguments[0] : function(state$1) {
			return state$1;
		};
		validators$1.selector(selector);
		return selector(state.current);
	}
	function setState$1(causedChanges) {
		compose$1(didUpdate, update, validate, getChanges)(causedChanges);
	}
	return [getState$1, setState$1];
}
function extractChanges(state, causedChanges) {
	return isFunction(causedChanges) ? causedChanges(state.current) : causedChanges;
}
function updateState(state, changes) {
	state.current = _objectSpread2$1(_objectSpread2$1({}, state.current), changes);
	return changes;
}
function didStateUpdate(state, handler, changes) {
	isFunction(handler) ? handler(state.current) : Object.keys(changes).forEach(function(field) {
		var _handler$field;
		return (_handler$field = handler[field]) === null || _handler$field === void 0 ? void 0 : _handler$field.call(handler, state.current[field]);
	});
	return changes;
}
var state_local_default = { create };
var config = { paths: { vs: "https://cdn.jsdelivr.net/npm/monaco-editor@0.55.1/min/vs" } };
function curry(fn) {
	return function curried() {
		var _this = this;
		for (var _len = arguments.length, args = new Array(_len), _key = 0; _key < _len; _key++) args[_key] = arguments[_key];
		return args.length >= fn.length ? fn.apply(this, args) : function() {
			for (var _len2 = arguments.length, nextArgs = new Array(_len2), _key2 = 0; _key2 < _len2; _key2++) nextArgs[_key2] = arguments[_key2];
			return curried.apply(_this, [].concat(args, nextArgs));
		};
	};
}
function isObject(value) {
	return {}.toString.call(value).includes("Object");
}
function validateConfig(config$2) {
	if (!config$2) errorHandler("configIsRequired");
	if (!isObject(config$2)) errorHandler("configType");
	if (config$2.urls) {
		informAboutDeprecation();
		return { paths: { vs: config$2.urls.monacoBase } };
	}
	return config$2;
}
function informAboutDeprecation() {
	console.warn(errorMessages.deprecation);
}
function throwError(errorMessages$1, type) {
	throw new Error(errorMessages$1[type] || errorMessages$1["default"]);
}
var errorMessages = {
	configIsRequired: "the configuration object is required",
	configType: "the configuration object should be an object",
	"default": "an unknown error accured in `@monaco-editor/loader` package",
	deprecation: "Deprecation warning!\n    You are using deprecated way of configuration.\n\n    Instead of using\n      monaco.config({ urls: { monacoBase: '...' } })\n    use\n      monaco.config({ paths: { vs: '...' } })\n\n    For more please check the link https://github.com/suren-atoyan/monaco-loader#config\n  "
};
var errorHandler = curry(throwError)(errorMessages);
var validators = { config: validateConfig };
var compose = function compose$2() {
	for (var _len = arguments.length, fns = new Array(_len), _key = 0; _key < _len; _key++) fns[_key] = arguments[_key];
	return function(x) {
		return fns.reduceRight(function(y, f) {
			return f(y);
		}, x);
	};
};
function merge(target, source) {
	Object.keys(source).forEach(function(key) {
		if (source[key] instanceof Object) {
			if (target[key]) Object.assign(source[key], merge(target[key], source[key]));
		}
	});
	return _objectSpread2(_objectSpread2({}, target), source);
}
var CANCELATION_MESSAGE = {
	type: "cancelation",
	msg: "operation is manually canceled"
};
function makeCancelable(promise) {
	var hasCanceled_ = false;
	var wrappedPromise = new Promise(function(resolve, reject) {
		promise.then(function(val) {
			return hasCanceled_ ? reject(CANCELATION_MESSAGE) : resolve(val);
		});
		promise["catch"](reject);
	});
	return wrappedPromise.cancel = function() {
		return hasCanceled_ = true;
	}, wrappedPromise;
}
var _excluded = ["monaco"];
var _state$create2 = _slicedToArray(state_local_default.create({
	config,
	isInitialized: false,
	resolve: null,
	reject: null,
	monaco: null
}), 2), getState = _state$create2[0], setState = _state$create2[1];
function config$1(globalConfig) {
	var _validators$config = validators.config(globalConfig), monaco = _validators$config.monaco, config$2 = _objectWithoutProperties(_validators$config, _excluded);
	setState(function(state) {
		return {
			config: merge(state.config, config$2),
			monaco
		};
	});
}
function init() {
	var state = getState(function(_ref) {
		return {
			monaco: _ref.monaco,
			isInitialized: _ref.isInitialized,
			resolve: _ref.resolve
		};
	});
	if (!state.isInitialized) {
		setState({ isInitialized: true });
		if (state.monaco) {
			state.resolve(state.monaco);
			return makeCancelable(wrapperPromise);
		}
		if (window.monaco && window.monaco.editor) {
			storeMonacoInstance(window.monaco);
			state.resolve(window.monaco);
			return makeCancelable(wrapperPromise);
		}
		compose(injectScripts, getMonacoLoaderScript)(configureLoader);
	}
	return makeCancelable(wrapperPromise);
}
function injectScripts(script) {
	return document.body.appendChild(script);
}
function createScript(src) {
	var script = document.createElement("script");
	return src && (script.src = src), script;
}
function getMonacoLoaderScript(configureLoader$1) {
	var state = getState(function(_ref2) {
		return {
			config: _ref2.config,
			reject: _ref2.reject
		};
	});
	var loaderScript = createScript("".concat(state.config.paths.vs, "/loader.js"));
	loaderScript.onload = function() {
		return configureLoader$1();
	};
	loaderScript.onerror = state.reject;
	return loaderScript;
}
function configureLoader() {
	var state = getState(function(_ref3) {
		return {
			config: _ref3.config,
			resolve: _ref3.resolve,
			reject: _ref3.reject
		};
	});
	var require = window.require;
	require.config(state.config);
	require(["vs/editor/editor.main"], function(loaded) {
		var monaco = loaded.m || loaded;
		storeMonacoInstance(monaco);
		state.resolve(monaco);
	}, function(error) {
		state.reject(error);
	});
}
function storeMonacoInstance(monaco) {
	if (!getState().monaco) setState({ monaco });
}
function __getMonacoInstance() {
	return getState(function(_ref4) {
		return _ref4.monaco;
	});
}
var wrapperPromise = new Promise(function(resolve, reject) {
	return setState({
		resolve,
		reject
	});
});
var loader = {
	config: config$1,
	init,
	__getMonacoInstance
};
var import_react = /* @__PURE__ */ __toESM(require_react(), 1);
var v = {
	wrapper: {
		display: "flex",
		position: "relative",
		textAlign: "initial"
	},
	fullWidth: { width: "100%" },
	hide: { display: "none" }
};
var Y = { container: {
	display: "flex",
	height: "100%",
	width: "100%",
	justifyContent: "center",
	alignItems: "center"
} };
function Me({ children: e }) {
	return import_react.createElement("div", { style: Y.container }, e);
}
var $ = Me;
function Ee({ width: e, height: r, isEditorReady: n, loading: t, _ref: a, className: m, wrapperProps: E }) {
	return import_react.createElement("section", {
		style: {
			...v.wrapper,
			width: e,
			height: r
		},
		...E
	}, !n && import_react.createElement($, null, t), import_react.createElement("div", {
		ref: a,
		style: {
			...v.fullWidth,
			...!n && v.hide
		},
		className: m
	}));
}
var H = (0, import_react.memo)(Ee);
function Ce(e) {
	(0, import_react.useEffect)(e, []);
}
var k = Ce;
function he(e, r, n = !0) {
	let t = (0, import_react.useRef)(!0);
	(0, import_react.useEffect)(t.current || !n ? () => {
		t.current = !1;
	} : e, r);
}
var l = he;
function D() {}
function h(e, r, n, t) {
	return De(e, t) || be(e, r, n, t);
}
function De(e, r) {
	return e.editor.getModel(te(e, r));
}
function be(e, r, n, t) {
	return e.editor.createModel(r, n, t ? te(e, t) : void 0);
}
function te(e, r) {
	return e.Uri.parse(r);
}
function Oe({ original: e, modified: r, language: n, originalLanguage: t, modifiedLanguage: a, originalModelPath: m, modifiedModelPath: E, keepCurrentOriginalModel: g = !1, keepCurrentModifiedModel: N = !1, theme: x = "light", loading: P = "Loading...", options: y = {}, height: V = "100%", width: z = "100%", className: F, wrapperProps: j = {}, beforeMount: A = D, onMount: q = D }) {
	let [M, O] = (0, import_react.useState)(!1), [T, s] = (0, import_react.useState)(!0), u = (0, import_react.useRef)(null), c = (0, import_react.useRef)(null), w = (0, import_react.useRef)(null), d = (0, import_react.useRef)(q), o = (0, import_react.useRef)(A), b = (0, import_react.useRef)(!1);
	k(() => {
		let i = loader.init();
		return i.then((f) => (c.current = f) && s(!1)).catch((f) => f?.type !== "cancelation" && console.error("Monaco initialization: error:", f)), () => u.current ? I() : i.cancel();
	}), l(() => {
		if (u.current && c.current) {
			let i = u.current.getOriginalEditor(), f = h(c.current, e || "", t || n || "text", m || "");
			f !== i.getModel() && i.setModel(f);
		}
	}, [m], M), l(() => {
		if (u.current && c.current) {
			let i = u.current.getModifiedEditor(), f = h(c.current, r || "", a || n || "text", E || "");
			f !== i.getModel() && i.setModel(f);
		}
	}, [E], M), l(() => {
		let i = u.current.getModifiedEditor();
		i.getOption(c.current.editor.EditorOption.readOnly) ? i.setValue(r || "") : r !== i.getValue() && (i.executeEdits("", [{
			range: i.getModel().getFullModelRange(),
			text: r || "",
			forceMoveMarkers: !0
		}]), i.pushUndoStop());
	}, [r], M), l(() => {
		u.current?.getModel()?.original.setValue(e || "");
	}, [e], M), l(() => {
		let { original: i, modified: f } = u.current.getModel();
		c.current.editor.setModelLanguage(i, t || n || "text"), c.current.editor.setModelLanguage(f, a || n || "text");
	}, [
		n,
		t,
		a
	], M), l(() => {
		c.current?.editor.setTheme(x);
	}, [x], M), l(() => {
		u.current?.updateOptions(y);
	}, [y], M);
	let L = (0, import_react.useCallback)(() => {
		if (!c.current) return;
		o.current(c.current);
		let i = h(c.current, e || "", t || n || "text", m || ""), f = h(c.current, r || "", a || n || "text", E || "");
		u.current?.setModel({
			original: i,
			modified: f
		});
	}, [
		n,
		r,
		a,
		e,
		t,
		m,
		E
	]), U = (0, import_react.useCallback)(() => {
		!b.current && w.current && (u.current = c.current.editor.createDiffEditor(w.current, {
			automaticLayout: !0,
			...y
		}), L(), c.current?.editor.setTheme(x), O(!0), b.current = !0);
	}, [
		y,
		x,
		L
	]);
	(0, import_react.useEffect)(() => {
		M && d.current(u.current, c.current);
	}, [M]), (0, import_react.useEffect)(() => {
		!T && !M && U();
	}, [
		T,
		M,
		U
	]);
	function I() {
		let i = u.current?.getModel();
		g || i?.original?.dispose(), N || i?.modified?.dispose(), u.current?.dispose();
	}
	return import_react.createElement(H, {
		width: z,
		height: V,
		isEditorReady: M,
		loading: P,
		_ref: w,
		className: F,
		wrapperProps: j
	});
}
var we = (0, import_react.memo)(Oe);
function He(e) {
	let r = (0, import_react.useRef)();
	return (0, import_react.useEffect)(() => {
		r.current = e;
	}, [e]), r.current;
}
var se = He;
var _ = /* @__PURE__ */ new Map();
function Ve({ defaultValue: e, defaultLanguage: r, defaultPath: n, value: t, language: a, path: m, theme: E = "light", line: g, loading: N = "Loading...", options: x = {}, overrideServices: P = {}, saveViewState: y = !0, keepCurrentModel: V = !1, width: z = "100%", height: F = "100%", className: j, wrapperProps: A = {}, beforeMount: q = D, onMount: M = D, onChange: O, onValidate: T = D }) {
	let [s, u] = (0, import_react.useState)(!1), [c, w] = (0, import_react.useState)(!0), d = (0, import_react.useRef)(null), o = (0, import_react.useRef)(null), b = (0, import_react.useRef)(null), L = (0, import_react.useRef)(M), U = (0, import_react.useRef)(q), I = (0, import_react.useRef)(), i = (0, import_react.useRef)(t), f = se(m), Q = (0, import_react.useRef)(!1), B = (0, import_react.useRef)(!1);
	k(() => {
		let p = loader.init();
		return p.then((R) => (d.current = R) && w(!1)).catch((R) => R?.type !== "cancelation" && console.error("Monaco initialization: error:", R)), () => o.current ? pe() : p.cancel();
	}), l(() => {
		let p = h(d.current, e || t || "", r || a || "", m || n || "");
		p !== o.current?.getModel() && (y && _.set(f, o.current?.saveViewState()), o.current?.setModel(p), y && o.current?.restoreViewState(_.get(m)));
	}, [m], s), l(() => {
		o.current?.updateOptions(x);
	}, [x], s), l(() => {
		!o.current || t === void 0 || (o.current.getOption(d.current.editor.EditorOption.readOnly) ? o.current.setValue(t) : t !== o.current.getValue() && (B.current = !0, o.current.executeEdits("", [{
			range: o.current.getModel().getFullModelRange(),
			text: t,
			forceMoveMarkers: !0
		}]), o.current.pushUndoStop(), B.current = !1));
	}, [t], s), l(() => {
		let p = o.current?.getModel();
		p && a && d.current?.editor.setModelLanguage(p, a);
	}, [a], s), l(() => {
		g !== void 0 && o.current?.revealLine(g);
	}, [g], s), l(() => {
		d.current?.editor.setTheme(E);
	}, [E], s);
	let X = (0, import_react.useCallback)(() => {
		if (!(!b.current || !d.current) && !Q.current) {
			U.current(d.current);
			let p = m || n, R = h(d.current, t || e || "", r || a || "", p || "");
			o.current = d.current?.editor.create(b.current, {
				model: R,
				automaticLayout: !0,
				...x
			}, P), y && o.current.restoreViewState(_.get(p)), d.current.editor.setTheme(E), g !== void 0 && o.current.revealLine(g), u(!0), Q.current = !0;
		}
	}, [
		e,
		r,
		n,
		t,
		a,
		m,
		x,
		P,
		y,
		E,
		g
	]);
	(0, import_react.useEffect)(() => {
		s && L.current(o.current, d.current);
	}, [s]), (0, import_react.useEffect)(() => {
		!c && !s && X();
	}, [
		c,
		s,
		X
	]), i.current = t, (0, import_react.useEffect)(() => {
		s && O && (I.current?.dispose(), I.current = o.current?.onDidChangeModelContent((p) => {
			B.current || O(o.current.getValue(), p);
		}));
	}, [s, O]), (0, import_react.useEffect)(() => {
		if (s) {
			let p = d.current.editor.onDidChangeMarkers((R) => {
				let G = o.current.getModel()?.uri;
				if (G && R.find((J) => J.path === G.path)) {
					let J = d.current.editor.getModelMarkers({ resource: G });
					T?.(J);
				}
			});
			return () => {
				p?.dispose();
			};
		}
		return () => {};
	}, [s, T]);
	function pe() {
		I.current?.dispose(), V ? y && _.set(m, o.current.saveViewState()) : o.current.getModel()?.dispose(), o.current.dispose();
	}
	return import_react.createElement(H, {
		width: z,
		height: F,
		isEditorReady: s,
		loading: N,
		_ref: b,
		className: j,
		wrapperProps: A
	});
}
var Ft = (0, import_react.memo)(Ve);
function WorkerWrapper(options) {
	return new Worker("" + new URL("editor.worker-uyqomUXp.js", import.meta.url).href, {
		type: "module",
		name: options?.name
	});
}
function WorkerWrapper$1(options) {
	return new Worker("" + new URL("json.worker-CzYCmPeC.js", import.meta.url).href, {
		type: "module",
		name: options?.name
	});
}
function WorkerWrapper$2(options) {
	return new Worker("" + new URL("css.worker-HpdqqtF7.js", import.meta.url).href, {
		type: "module",
		name: options?.name
	});
}
function WorkerWrapper$3(options) {
	return new Worker("" + new URL("html.worker-DCuyvqr1.js", import.meta.url).href, {
		type: "module",
		name: options?.name
	});
}
function WorkerWrapper$4(options) {
	return new Worker("" + new URL("ts.worker-DygWnL5E.js", import.meta.url).href, {
		type: "module",
		name: options?.name
	});
}
const astroMonarchLanguage = {
	defaultToken: "",
	tokenPostfix: ".astro",
	ignoreCase: true,
	brackets: [
		{
			open: "{",
			close: "}",
			token: "delimiter.curly"
		},
		{
			open: "[",
			close: "]",
			token: "delimiter.square"
		},
		{
			open: "(",
			close: ")",
			token: "delimiter.parenthesis"
		},
		{
			open: "<",
			close: ">",
			token: "delimiter.angle"
		}
	],
	tokenizer: {
		root: [[/---\s*$/, {
			token: "keyword",
			switchTo: "@frontmatter",
			nextEmbedded: "typescript"
		}], [/(?=.)/, {
			token: "@rematch",
			switchTo: "@markupReenter"
		}]],
		frontmatter: [[/^---\s*$/, {
			token: "keyword",
			switchTo: "@markupReenter",
			nextEmbedded: "@pop"
		}]],
		markup: [
			[/<script(?=\s|>)/, {
				token: "tag",
				switchTo: "@scriptOpen.javascript",
				nextEmbedded: "@pop"
			}],
			[/<style(?=\s|>)/, {
				token: "tag",
				switchTo: "@styleOpen.css",
				nextEmbedded: "@pop"
			}],
			[/<!--/, {
				token: "comment",
				switchTo: "@comment",
				nextEmbedded: "@pop"
			}],
			[/\{/, {
				token: "delimiter.curly",
				switchTo: "@astroExpressionEnter",
				nextEmbedded: "@pop"
			}]
		],
		markupReenter: [[/(?=.)/, {
			token: "@rematch",
			switchTo: "@markup",
			nextEmbedded: "html"
		}]],
		comment: [
			[/-->/, {
				token: "comment",
				switchTo: "@markupReenter"
			}],
			[/[^-]+/, "comment"],
			[/./, "comment"]
		],
		astroExpressionEnter: [[/\}/, {
			token: "delimiter.curly",
			switchTo: "@markupReenter"
		}], [/(?=.)/, {
			token: "",
			switchTo: "@astroExpression",
			nextEmbedded: "typescript"
		}]],
		astroExpression: [[/\}/, {
			token: "delimiter.curly",
			switchTo: "@markupReenter",
			nextEmbedded: "@pop"
		}]],
		scriptOpen: [
			[/\/>/, {
				token: "tag",
				switchTo: "@markupReenter"
			}],
			[/>/, {
				token: "tag",
				switchTo: "@scriptBody.$S2",
				nextEmbedded: "$S2"
			}],
			[/lang(?=\s*=)/, {
				token: "attribute.name",
				switchTo: "@scriptLangBeforeEquals.$S2"
			}],
			{ include: "@tagAttributes" }
		],
		scriptLangBeforeEquals: [
			[/=/, {
				token: "delimiter",
				switchTo: "@scriptLangValue.$S2"
			}],
			[/\s+/, "white"],
			[/(?=.)/, {
				token: "",
				switchTo: "@scriptOpen.$S2"
			}]
		],
		scriptLangValue: [
			[/"(?:js|javascript)"/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.javascript"
			}],
			[/'(?:js|javascript)'/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.javascript"
			}],
			[/(?:js|javascript)(?=\s|\/|>|$)/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.javascript"
			}],
			[/"(?:ts|typescript)"/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.typescript"
			}],
			[/'(?:ts|typescript)'/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.typescript"
			}],
			[/(?:ts|typescript)(?=\s|\/|>|$)/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.typescript"
			}],
			[/[^\s/>]+/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.$S2"
			}],
			[/"[^"]*"/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.$S2"
			}],
			[/'[^']*'/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.$S2"
			}],
			[/\s+/, "white"]
		],
		scriptBody: [[/<\/script\s*>/, {
			token: "tag",
			switchTo: "@markupReenter",
			nextEmbedded: "@pop"
		}]],
		styleOpen: [
			[/\/>/, {
				token: "tag",
				switchTo: "@markupReenter"
			}],
			[/>/, {
				token: "tag",
				switchTo: "@styleBody.$S2",
				nextEmbedded: "$S2"
			}],
			[/lang(?=\s*=)/, {
				token: "attribute.name",
				switchTo: "@styleLangBeforeEquals.$S2"
			}],
			{ include: "@tagAttributes" }
		],
		styleLangBeforeEquals: [
			[/=/, {
				token: "delimiter",
				switchTo: "@styleLangValue.$S2"
			}],
			[/\s+/, "white"],
			[/(?=.)/, {
				token: "",
				switchTo: "@styleOpen.$S2"
			}]
		],
		styleLangValue: [
			[/"scss"/, {
				token: "attribute.value",
				switchTo: "@styleOpen.scss"
			}],
			[/'scss'/, {
				token: "attribute.value",
				switchTo: "@styleOpen.scss"
			}],
			[/scss(?=\s|\/|>|$)/, {
				token: "attribute.value",
				switchTo: "@styleOpen.scss"
			}],
			[/"sass"/, {
				token: "attribute.value",
				switchTo: "@styleOpen.scss"
			}],
			[/'sass'/, {
				token: "attribute.value",
				switchTo: "@styleOpen.scss"
			}],
			[/sass(?=\s|\/|>|$)/, {
				token: "attribute.value",
				switchTo: "@styleOpen.scss"
			}],
			[/"less"/, {
				token: "attribute.value",
				switchTo: "@styleOpen.less"
			}],
			[/'less'/, {
				token: "attribute.value",
				switchTo: "@styleOpen.less"
			}],
			[/less(?=\s|\/|>|$)/, {
				token: "attribute.value",
				switchTo: "@styleOpen.less"
			}],
			[/"css"/, {
				token: "attribute.value",
				switchTo: "@styleOpen.css"
			}],
			[/'css'/, {
				token: "attribute.value",
				switchTo: "@styleOpen.css"
			}],
			[/css(?=\s|\/|>|$)/, {
				token: "attribute.value",
				switchTo: "@styleOpen.css"
			}],
			[/[^\s/>]+/, {
				token: "attribute.value",
				switchTo: "@styleOpen.$S2"
			}],
			[/"[^"]*"/, {
				token: "attribute.value",
				switchTo: "@styleOpen.$S2"
			}],
			[/'[^']*'/, {
				token: "attribute.value",
				switchTo: "@styleOpen.$S2"
			}],
			[/\s+/, "white"]
		],
		styleBody: [[/<\/style\s*>/, {
			token: "tag",
			switchTo: "@markupReenter",
			nextEmbedded: "@pop"
		}]],
		tagAttributes: [
			[/[^\s/>=]+/, "attribute.name"],
			[/=/, "delimiter"],
			[/"[^"]*"/, "attribute.value"],
			[/'[^']*'/, "attribute.value"],
			[/\s+/, "white"]
		]
	}
};
const astroLanguageConfiguration = {
	comments: { blockComment: ["<!--", "-->"] },
	brackets: [
		["{", "}"],
		["[", "]"],
		["(", ")"],
		["<", ">"]
	],
	autoClosingPairs: [
		{
			open: "{",
			close: "}"
		},
		{
			open: "[",
			close: "]"
		},
		{
			open: "(",
			close: ")"
		},
		{
			open: "\"",
			close: "\""
		},
		{
			open: "'",
			close: "'"
		},
		{
			open: "`",
			close: "`"
		},
		{
			open: "<",
			close: ">"
		}
	],
	surroundingPairs: [
		{
			open: "{",
			close: "}"
		},
		{
			open: "[",
			close: "]"
		},
		{
			open: "(",
			close: ")"
		},
		{
			open: "\"",
			close: "\""
		},
		{
			open: "'",
			close: "'"
		},
		{
			open: "`",
			close: "`"
		},
		{
			open: "<",
			close: ">"
		}
	]
};
function registerAstroLanguage(monaco) {
	if (monaco.languages.getLanguages().some((language) => language.id === "astro")) return;
	monaco.languages.register({
		id: "astro",
		extensions: [".astro"],
		aliases: ["Astro"]
	});
	monaco.languages.setMonarchTokensProvider("astro", astroMonarchLanguage);
	monaco.languages.setLanguageConfiguration("astro", astroLanguageConfiguration);
}
const JSONL_LANGUAGE_ID = "jsonl";
const jsonlLanguageConfiguration = {
	brackets: [["{", "}"], ["[", "]"]],
	autoClosingPairs: [
		{
			open: "{",
			close: "}"
		},
		{
			open: "[",
			close: "]"
		},
		{
			open: "\"",
			close: "\""
		}
	],
	surroundingPairs: [
		{
			open: "{",
			close: "}"
		},
		{
			open: "[",
			close: "]"
		},
		{
			open: "\"",
			close: "\""
		}
	]
};
const jsonlMonarchLanguage = {
	defaultToken: "",
	tokenPostfix: ".jsonl",
	tokenizer: {
		root: [
			{ include: "@whitespace" },
			[/"(?:[^"\\]|\\.)*"(?=\s*:)/, "type.identifier"],
			[
				/"/,
				"string",
				"@string"
			],
			[/[{}[\]]/, "@brackets"],
			[/-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?/, "number"],
			[/\b(?:true|false)\b/, "keyword"],
			[/\bnull\b/, "keyword"],
			[/[:,]/, "delimiter"]
		],
		whitespace: [[/[ \t\r\n]+/, "white"]],
		string: [
			[/[^"\\]+/, "string"],
			[/\\(?:["\\/bfnrt]|u[0-9A-Fa-f]{4})/, "string.escape"],
			[/\\./, "string.escape.invalid"],
			[
				/"/,
				"string",
				"@pop"
			]
		]
	}
};
function registerJsonlLanguage(monaco) {
	if (monaco.languages.getLanguages().some((language) => language.id === "jsonl")) return;
	monaco.languages.register({
		id: JSONL_LANGUAGE_ID,
		extensions: [".jsonl"],
		aliases: [
			"JSON Lines",
			"jsonl",
			"ndjson"
		]
	});
	monaco.languages.setLanguageConfiguration(JSONL_LANGUAGE_ID, jsonlLanguageConfiguration);
	monaco.languages.setMonarchTokensProvider(JSONL_LANGUAGE_ID, jsonlMonarchLanguage);
}
function loadDefaultProviderModule() {
	return __vitePreload(() => import("./textmate-token-provider-C5pkZdSV.js"), __vite__mapDeps([0,1,2]), import.meta.url);
}
function registerTextMateLanguage(monaco, registration) {
	if (monaco.languages.getLanguages().some((language) => language.id === registration.language.id)) return;
	monaco.languages.register(registration.language);
	if (registration.configuration) monaco.languages.setLanguageConfiguration(registration.language.id, registration.configuration);
	let tokensProviderPromise;
	monaco.languages.registerTokensProviderFactory(registration.language.id, { create: () => {
		tokensProviderPromise ?? (tokensProviderPromise = (registration.loadProviderModule ?? loadDefaultProviderModule)().then(({ createTextMateTokensProvider }) => createTextMateTokensProvider({
			scopeName: registration.scopeName,
			loadGrammar: registration.loadGrammar
		})));
		return tokensProviderPromise;
	} });
}
const NIM_TEXTMATE_SCOPE = "source.nim";
const nimLanguageConfiguration = {
	comments: {
		lineComment: "#",
		blockComment: ["#[", "]#"]
	},
	brackets: [
		["{", "}"],
		["[", "]"],
		["(", ")"]
	],
	autoClosingPairs: [
		{
			open: "{",
			close: "}"
		},
		{
			open: "[",
			close: "]"
		},
		{
			open: "(",
			close: ")"
		},
		{
			open: "\"",
			close: "\""
		},
		{
			open: "'",
			close: "'"
		}
	],
	surroundingPairs: [
		{
			open: "{",
			close: "}"
		},
		{
			open: "[",
			close: "]"
		},
		{
			open: "(",
			close: ")"
		},
		{
			open: "\"",
			close: "\""
		},
		{
			open: "'",
			close: "'"
		}
	]
};
async function loadNimTextMateGrammar(scopeName) {
	if (scopeName !== "source.nim") return null;
	return (await __vitePreload(() => import("./nim.tmLanguage-CO_6Q7n2.js"), [], import.meta.url)).default;
}
function registerNimLanguage(monaco) {
	registerTextMateLanguage(monaco, {
		language: {
			id: "nim",
			extensions: [
				".nim",
				".nims",
				".nimble"
			],
			aliases: ["Nim", "nim"]
		},
		configuration: nimLanguageConfiguration,
		scopeName: NIM_TEXTMATE_SCOPE,
		loadGrammar: loadNimTextMateGrammar
	});
}
const svelteMonarchLanguage = {
	defaultToken: "",
	tokenPostfix: ".svelte",
	ignoreCase: true,
	brackets: [
		{
			open: "{",
			close: "}",
			token: "delimiter.curly"
		},
		{
			open: "[",
			close: "]",
			token: "delimiter.square"
		},
		{
			open: "(",
			close: ")",
			token: "delimiter.parenthesis"
		},
		{
			open: "<",
			close: ">",
			token: "delimiter.angle"
		}
	],
	tokenizer: {
		root: [
			[/<script(?=\s|>)/, {
				token: "tag",
				switchTo: "@scriptOpen.typescript"
			}],
			[/<style(?=\s|>)/, {
				token: "tag",
				switchTo: "@styleOpen.css"
			}],
			[/<!--/, {
				token: "comment",
				switchTo: "@comment"
			}],
			[/\{\s*\/(if|each|await|key|snippet)\s*\}/, "keyword.control"],
			[/\{\s*#(if|each|await|key|snippet)\b/, {
				token: "keyword.control",
				switchTo: "@svelteBlockExpressionEnter"
			}],
			[/\{\s*:(else|then|catch)\b/, {
				token: "keyword.control",
				switchTo: "@svelteBlockExpressionEnter"
			}],
			[/\{\s*@(html|debug|const|render)\b/, {
				token: "keyword.control",
				switchTo: "@svelteExpressionEnter"
			}],
			[/\{(?=[^#:/@])/, {
				token: "delimiter.curly",
				switchTo: "@svelteExpressionEnter"
			}],
			[/(?=.)/, {
				token: "",
				switchTo: "@markup",
				nextEmbedded: "html"
			}]
		],
		markup: [
			[/<script(?=\s|>)/, {
				token: "tag",
				switchTo: "@scriptOpen.typescript",
				nextEmbedded: "@pop"
			}],
			[/<style(?=\s|>)/, {
				token: "tag",
				switchTo: "@styleOpen.css",
				nextEmbedded: "@pop"
			}],
			[/<!--/, {
				token: "comment",
				switchTo: "@comment",
				nextEmbedded: "@pop"
			}],
			[/\{\s*\/(if|each|await|key|snippet)\s*\}/, "keyword.control"],
			[/\{\s*#(if|each|await|key|snippet)\b/, {
				token: "keyword.control",
				switchTo: "@svelteBlockExpressionEnter",
				nextEmbedded: "@pop"
			}],
			[/\{\s*:(else|then|catch)\b/, {
				token: "keyword.control",
				switchTo: "@svelteBlockExpressionEnter",
				nextEmbedded: "@pop"
			}],
			[/\{\s*@(html|debug|const|render)\b/, {
				token: "keyword.control",
				switchTo: "@svelteExpressionEnter",
				nextEmbedded: "@pop"
			}],
			[/\{(?=[^#:/@])/, {
				token: "delimiter.curly",
				switchTo: "@svelteExpressionEnter",
				nextEmbedded: "@pop"
			}]
		],
		markupReenter: [[/(?=.)/, {
			token: "@rematch",
			switchTo: "@markup",
			nextEmbedded: "html"
		}]],
		comment: [
			[/-->/, {
				token: "comment",
				switchTo: "@markupReenter"
			}],
			[/[^-]+/, "comment"],
			[/./, "comment"]
		],
		svelteExpressionEnter: [[/\}/, {
			token: "delimiter.curly",
			switchTo: "@markupReenter"
		}], [/(?=.)/, {
			token: "",
			switchTo: "@svelteExpression",
			nextEmbedded: "typescript"
		}]],
		svelteExpression: [[/\}/, {
			token: "delimiter.curly",
			switchTo: "@markupReenter",
			nextEmbedded: "@pop"
		}]],
		svelteBlockExpressionEnter: [[/\}/, {
			token: "keyword.control",
			switchTo: "@markupReenter"
		}], [/(?=.)/, {
			token: "",
			switchTo: "@svelteBlockExpression",
			nextEmbedded: "typescript"
		}]],
		svelteBlockExpression: [[/\}/, {
			token: "keyword.control",
			switchTo: "@markupReenter",
			nextEmbedded: "@pop"
		}]],
		scriptOpen: [
			[/\/>/, {
				token: "tag",
				switchTo: "@markupReenter"
			}],
			[/>/, {
				token: "tag",
				switchTo: "@scriptBody.$S2",
				nextEmbedded: "$S2"
			}],
			[/lang(?=\s*=)/, {
				token: "attribute.name",
				switchTo: "@scriptLangBeforeEquals.$S2"
			}],
			{ include: "@tagAttributes" }
		],
		scriptLangBeforeEquals: [
			[/=/, {
				token: "delimiter",
				switchTo: "@scriptLangValue.$S2"
			}],
			[/\s+/, "white"],
			[/(?=.)/, {
				token: "",
				switchTo: "@scriptOpen.$S2"
			}]
		],
		scriptLangValue: [
			[/"(?:js|javascript)"/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.javascript"
			}],
			[/'(?:js|javascript)'/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.javascript"
			}],
			[/(?:js|javascript)(?=\s|\/|>|$)/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.javascript"
			}],
			[/"(?:ts|typescript)"/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.typescript"
			}],
			[/'(?:ts|typescript)'/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.typescript"
			}],
			[/(?:ts|typescript)(?=\s|\/|>|$)/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.typescript"
			}],
			[/[^\s/>]+/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.$S2"
			}],
			[/"[^"]*"/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.$S2"
			}],
			[/'[^']*'/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.$S2"
			}],
			[/\s+/, "white"]
		],
		scriptBody: [[/<\/script\s*>/, {
			token: "tag",
			switchTo: "@markupReenter",
			nextEmbedded: "@pop"
		}]],
		styleOpen: [
			[/\/>/, {
				token: "tag",
				switchTo: "@markupReenter"
			}],
			[/>/, {
				token: "tag",
				switchTo: "@styleBody.$S2",
				nextEmbedded: "$S2"
			}],
			[/lang(?=\s*=)/, {
				token: "attribute.name",
				switchTo: "@styleLangBeforeEquals.$S2"
			}],
			{ include: "@tagAttributes" }
		],
		styleLangBeforeEquals: [
			[/=/, {
				token: "delimiter",
				switchTo: "@styleLangValue.$S2"
			}],
			[/\s+/, "white"],
			[/(?=.)/, {
				token: "",
				switchTo: "@styleOpen.$S2"
			}]
		],
		styleLangValue: [
			[/"scss"/, {
				token: "attribute.value",
				switchTo: "@styleOpen.scss"
			}],
			[/'scss'/, {
				token: "attribute.value",
				switchTo: "@styleOpen.scss"
			}],
			[/scss(?=\s|\/|>|$)/, {
				token: "attribute.value",
				switchTo: "@styleOpen.scss"
			}],
			[/"sass"/, {
				token: "attribute.value",
				switchTo: "@styleOpen.scss"
			}],
			[/'sass'/, {
				token: "attribute.value",
				switchTo: "@styleOpen.scss"
			}],
			[/sass(?=\s|\/|>|$)/, {
				token: "attribute.value",
				switchTo: "@styleOpen.scss"
			}],
			[/"less"/, {
				token: "attribute.value",
				switchTo: "@styleOpen.less"
			}],
			[/'less'/, {
				token: "attribute.value",
				switchTo: "@styleOpen.less"
			}],
			[/less(?=\s|\/|>|$)/, {
				token: "attribute.value",
				switchTo: "@styleOpen.less"
			}],
			[/"css"/, {
				token: "attribute.value",
				switchTo: "@styleOpen.css"
			}],
			[/'css'/, {
				token: "attribute.value",
				switchTo: "@styleOpen.css"
			}],
			[/css(?=\s|\/|>|$)/, {
				token: "attribute.value",
				switchTo: "@styleOpen.css"
			}],
			[/[^\s/>]+/, {
				token: "attribute.value",
				switchTo: "@styleOpen.$S2"
			}],
			[/"[^"]*"/, {
				token: "attribute.value",
				switchTo: "@styleOpen.$S2"
			}],
			[/'[^']*'/, {
				token: "attribute.value",
				switchTo: "@styleOpen.$S2"
			}],
			[/\s+/, "white"]
		],
		styleBody: [[/<\/style\s*>/, {
			token: "tag",
			switchTo: "@markupReenter",
			nextEmbedded: "@pop"
		}]],
		tagAttributes: [
			[/[^\s/>=]+/, "attribute.name"],
			[/=/, "delimiter"],
			[/"[^"]*"/, "attribute.value"],
			[/'[^']*'/, "attribute.value"],
			[/\s+/, "white"]
		]
	}
};
const svelteLanguageConfiguration = {
	comments: { blockComment: ["<!--", "-->"] },
	brackets: [
		["{", "}"],
		["[", "]"],
		["(", ")"],
		["<", ">"]
	],
	autoClosingPairs: [
		{
			open: "{",
			close: "}"
		},
		{
			open: "[",
			close: "]"
		},
		{
			open: "(",
			close: ")"
		},
		{
			open: "\"",
			close: "\""
		},
		{
			open: "'",
			close: "'"
		},
		{
			open: "`",
			close: "`"
		},
		{
			open: "<",
			close: ">"
		}
	],
	surroundingPairs: [
		{
			open: "{",
			close: "}"
		},
		{
			open: "[",
			close: "]"
		},
		{
			open: "(",
			close: ")"
		},
		{
			open: "\"",
			close: "\""
		},
		{
			open: "'",
			close: "'"
		},
		{
			open: "`",
			close: "`"
		},
		{
			open: "<",
			close: ">"
		}
	]
};
function registerSvelteLanguage(monaco) {
	if (monaco.languages.getLanguages().some((language) => language.id === "svelte")) return;
	monaco.languages.register({
		id: "svelte",
		extensions: [".svelte"],
		aliases: ["Svelte"]
	});
	monaco.languages.setMonarchTokensProvider("svelte", svelteMonarchLanguage);
	monaco.languages.setLanguageConfiguration("svelte", svelteLanguageConfiguration);
}
const vueMonarchLanguage = {
	defaultToken: "",
	tokenPostfix: ".vue",
	ignoreCase: true,
	brackets: [
		{
			open: "{",
			close: "}",
			token: "delimiter.curly"
		},
		{
			open: "[",
			close: "]",
			token: "delimiter.square"
		},
		{
			open: "(",
			close: ")",
			token: "delimiter.parenthesis"
		},
		{
			open: "<",
			close: ">",
			token: "delimiter.angle"
		}
	],
	tokenizer: {
		root: [
			[
				/<template(?=\s|>)/,
				"tag",
				"@templateOpen"
			],
			[
				/<script(?=\s|>)/,
				"tag",
				"@scriptOpen.typescript"
			],
			[
				/<style(?=\s|>)/,
				"tag",
				"@styleOpen.css"
			],
			[
				/<!--/,
				"comment",
				"@comment"
			],
			[/<\/?[A-Za-z][^>]*>/, "tag"],
			[/[^<]+/, ""]
		],
		comment: [
			[
				/-->/,
				"comment",
				"@pop"
			],
			[/[^-]+/, "comment"],
			[/./, "comment"]
		],
		templateOpen: [
			[
				/\/>/,
				"tag",
				"@pop"
			],
			[/>/, {
				token: "tag",
				switchTo: "@templateBody",
				nextEmbedded: "html"
			}],
			{ include: "@tagAttributes" }
		],
		templateBody: [
			[/\{\{/, {
				token: "delimiter.curly",
				next: "@templateExpressionEnter",
				nextEmbedded: "@pop"
			}],
			[/<\/template\s*>/, {
				token: "tag",
				next: "@pop",
				nextEmbedded: "@pop"
			}],
			[/(?=.)/, {
				token: "",
				nextEmbedded: "html"
			}]
		],
		templateExpressionEnter: [[/\}\}/, {
			token: "delimiter.curly",
			next: "@pop"
		}], [/(?=.)/, {
			token: "",
			switchTo: "@templateExpression",
			nextEmbedded: "typescript"
		}]],
		templateExpression: [[/\}\}/, {
			token: "delimiter.curly",
			next: "@pop",
			nextEmbedded: "@pop"
		}]],
		scriptOpen: [
			[
				/\/>/,
				"tag",
				"@pop"
			],
			[/>/, {
				token: "tag",
				switchTo: "@scriptBody.$S2",
				nextEmbedded: "$S2"
			}],
			[/lang(?=\s*=)/, {
				token: "attribute.name",
				switchTo: "@scriptLangBeforeEquals.$S2"
			}],
			{ include: "@tagAttributes" }
		],
		scriptLangBeforeEquals: [
			[/=/, {
				token: "delimiter",
				switchTo: "@scriptLangValue.$S2"
			}],
			[/\s+/, "white"],
			[/(?=.)/, {
				token: "",
				switchTo: "@scriptOpen.$S2"
			}]
		],
		scriptLangValue: [
			[/"(?:js|javascript)"/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.javascript"
			}],
			[/'(?:js|javascript)'/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.javascript"
			}],
			[/(?:js|javascript)(?=\s|\/|>|$)/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.javascript"
			}],
			[/"(?:ts|typescript)"/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.typescript"
			}],
			[/'(?:ts|typescript)'/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.typescript"
			}],
			[/(?:ts|typescript)(?=\s|\/|>|$)/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.typescript"
			}],
			[/[^\s/>]+/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.$S2"
			}],
			[/"[^"]*"/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.$S2"
			}],
			[/'[^']*'/, {
				token: "attribute.value",
				switchTo: "@scriptOpen.$S2"
			}],
			[/\s+/, "white"]
		],
		scriptBody: [[/<\/script\s*>/, {
			token: "tag",
			next: "@pop",
			nextEmbedded: "@pop"
		}]],
		styleOpen: [
			[
				/\/>/,
				"tag",
				"@pop"
			],
			[/>/, {
				token: "tag",
				switchTo: "@styleBody.$S2",
				nextEmbedded: "$S2"
			}],
			[/lang(?=\s*=)/, {
				token: "attribute.name",
				switchTo: "@styleLangBeforeEquals.$S2"
			}],
			{ include: "@tagAttributes" }
		],
		styleLangBeforeEquals: [
			[/=/, {
				token: "delimiter",
				switchTo: "@styleLangValue.$S2"
			}],
			[/\s+/, "white"],
			[/(?=.)/, {
				token: "",
				switchTo: "@styleOpen.$S2"
			}]
		],
		styleLangValue: [
			[/"scss"/, {
				token: "attribute.value",
				switchTo: "@styleOpen.scss"
			}],
			[/'scss'/, {
				token: "attribute.value",
				switchTo: "@styleOpen.scss"
			}],
			[/scss(?=\s|\/|>|$)/, {
				token: "attribute.value",
				switchTo: "@styleOpen.scss"
			}],
			[/"sass"/, {
				token: "attribute.value",
				switchTo: "@styleOpen.scss"
			}],
			[/'sass'/, {
				token: "attribute.value",
				switchTo: "@styleOpen.scss"
			}],
			[/sass(?=\s|\/|>|$)/, {
				token: "attribute.value",
				switchTo: "@styleOpen.scss"
			}],
			[/"less"/, {
				token: "attribute.value",
				switchTo: "@styleOpen.less"
			}],
			[/'less'/, {
				token: "attribute.value",
				switchTo: "@styleOpen.less"
			}],
			[/less(?=\s|\/|>|$)/, {
				token: "attribute.value",
				switchTo: "@styleOpen.less"
			}],
			[/"css"/, {
				token: "attribute.value",
				switchTo: "@styleOpen.css"
			}],
			[/'css'/, {
				token: "attribute.value",
				switchTo: "@styleOpen.css"
			}],
			[/css(?=\s|\/|>|$)/, {
				token: "attribute.value",
				switchTo: "@styleOpen.css"
			}],
			[/[^\s/>]+/, {
				token: "attribute.value",
				switchTo: "@styleOpen.$S2"
			}],
			[/"[^"]*"/, {
				token: "attribute.value",
				switchTo: "@styleOpen.$S2"
			}],
			[/'[^']*'/, {
				token: "attribute.value",
				switchTo: "@styleOpen.$S2"
			}],
			[/\s+/, "white"]
		],
		styleBody: [[/<\/style\s*>/, {
			token: "tag",
			next: "@pop",
			nextEmbedded: "@pop"
		}]],
		tagAttributes: [
			[/[^\s/>=]+/, "attribute.name"],
			[/=/, "delimiter"],
			[/"[^"]*"/, "attribute.value"],
			[/'[^']*'/, "attribute.value"],
			[/\s+/, "white"]
		]
	}
};
const vueLanguageConfiguration = {
	comments: { blockComment: ["<!--", "-->"] },
	brackets: [
		["{", "}"],
		["[", "]"],
		["(", ")"],
		["<", ">"]
	],
	autoClosingPairs: [
		{
			open: "{",
			close: "}"
		},
		{
			open: "[",
			close: "]"
		},
		{
			open: "(",
			close: ")"
		},
		{
			open: "\"",
			close: "\""
		},
		{
			open: "'",
			close: "'"
		},
		{
			open: "`",
			close: "`"
		},
		{
			open: "<",
			close: ">"
		}
	],
	surroundingPairs: [
		{
			open: "{",
			close: "}"
		},
		{
			open: "[",
			close: "]"
		},
		{
			open: "(",
			close: ")"
		},
		{
			open: "\"",
			close: "\""
		},
		{
			open: "'",
			close: "'"
		},
		{
			open: "`",
			close: "`"
		},
		{
			open: "<",
			close: ">"
		}
	]
};
function registerVueLanguage(monaco) {
	if (monaco.languages.getLanguages().some((language) => language.id === "vue")) return;
	monaco.languages.register({
		id: "vue",
		extensions: [".vue"],
		aliases: ["Vue"]
	});
	monaco.languages.setMonarchTokensProvider("vue", vueMonarchLanguage);
	monaco.languages.setLanguageConfiguration("vue", vueLanguageConfiguration);
}
var MONACO_CANCELLATION_NAME = "Canceled";
function isMonacoCancellationError(error) {
	return error instanceof Error && error.name === MONACO_CANCELLATION_NAME && error.message === MONACO_CANCELLATION_NAME;
}
function installMonacoDelayerCancellationGuard() {
	const delayerPrototype = Delayer.prototype;
	if (delayerPrototype.__orcaDelayerCancellationGuardInstalled) return;
	const originalCancel = delayerPrototype.cancel;
	delayerPrototype.cancel = function cancelWithHandledCancellation() {
		const completionPromise = this.completionPromise;
		if (completionPromise) completionPromise.catch((error) => {
			if (!isMonacoCancellationError(error)) throw error;
		});
		originalCancel.call(this);
	};
	delayerPrototype.__orcaDelayerCancellationGuardInstalled = true;
}
function reportMonacoDiffDisposeError(error) {
	console.warn("[monaco] Diff editor disposal threw after teardown was requested", error);
}
function guardMonacoDiffEditorDispose(diffEditor, reportError = reportMonacoDiffDisposeError) {
	const guardedDiffEditor = diffEditor;
	if (guardedDiffEditor.__orcaDiffEditorDisposeGuardInstalled) return diffEditor;
	const originalDispose = diffEditor.dispose.bind(diffEditor);
	let didDispose = false;
	guardedDiffEditor.dispose = () => {
		if (didDispose) return;
		didDispose = true;
		try {
			originalDispose();
		} catch (error) {
			reportError(error);
		}
	};
	guardedDiffEditor.__orcaDiffEditorDisposeGuardInstalled = true;
	return diffEditor;
}
function installMonacoDiffEditorDisposalGuard(monaco, reportError) {
	const editorNamespace = monaco.editor;
	if (editorNamespace.__orcaDiffEditorFactoryGuardInstalled) return;
	const createDiffEditor = editorNamespace.createDiffEditor.bind(editorNamespace);
	editorNamespace.createDiffEditor = ((...args) => guardMonacoDiffEditorDispose(createDiffEditor(...args), reportError));
	editorNamespace.__orcaDiffEditorFactoryGuardInstalled = true;
}
var PEEK_REFERENCES_PREVIEW_OPTIONS = {
	smoothScrolling: false,
	stickyScroll: { enabled: false },
	wordWrap: "off"
};
function applyPeekReferencesPreviewOptions(editor) {
	editor?.updateOptions(PEEK_REFERENCES_PREVIEW_OPTIONS);
}
function installMonacoPeekReferencesPreviewOptions(referenceWidget = ReferenceWidget) {
	const prototype = referenceWidget.prototype;
	if (prototype.__orcaPeekPreviewOptionsInstalled) return;
	const originalFillBody = prototype._fillBody;
	const originalRevealReference = prototype._revealReference;
	if (typeof originalFillBody !== "function" || typeof originalRevealReference !== "function") return;
	prototype._fillBody = function fillBodyWithPeekPreviewOptions(containerElement) {
		originalFillBody.call(this, containerElement);
		applyPeekReferencesPreviewOptions(this._preview);
	};
	prototype._revealReference = async function revealReferenceWithPeekPreviewOptions(...args) {
		applyPeekReferencesPreviewOptions(this._preview);
		try {
			return await originalRevealReference.apply(this, args);
		} finally {
			applyPeekReferencesPreviewOptions(this._preview);
		}
	};
	prototype.__orcaPeekPreviewOptionsInstalled = true;
}
const MONACO_PASTE_MAX_BYTES = 16 * 1024 * 1024;
function getPlainTextFromPasteEvent(event) {
	return event.clipboardData?.getData("text/plain") ?? "";
}
function getEndPositionAfterInsert(start, text) {
	let lineNumber = start.lineNumber;
	let column = start.column;
	for (let index$1 = 0; index$1 < text.length; index$1 += 1) {
		const codeUnit = text.charCodeAt(index$1);
		if (codeUnit === 13) {
			lineNumber += 1;
			column = 1;
			if (text.charCodeAt(index$1 + 1) === 10) index$1 += 1;
			continue;
		}
		if (codeUnit === 10) {
			lineNumber += 1;
			column = 1;
			continue;
		}
		column += 1;
	}
	return {
		lineNumber,
		column
	};
}
function snapshotMonacoPasteTarget(monacoEditor) {
	const model = monacoEditor.getModel();
	const container = monacoEditor.getContainerDomNode();
	if (!model || !container.isConnected || !monacoEditor.hasTextFocus()) return null;
	return {
		container,
		model
	};
}
function isMonacoPasteTargetCurrent(monacoEditor, snapshot) {
	return monacoEditor.getModel() === snapshot.model && monacoEditor.getContainerDomNode() === snapshot.container && snapshot.container.isConnected && monacoEditor.hasTextFocus();
}
function setCollapsedSelection(monacoEditor, position) {
	monacoEditor.setSelection({
		startLineNumber: position.lineNumber,
		startColumn: position.column,
		endLineNumber: position.lineNumber,
		endColumn: position.column
	});
}
async function insertMonacoTextInChunks(monacoEditor, text, byteLength, options) {
	const snapshot = snapshotMonacoPasteTarget(monacoEditor);
	if (!snapshot) return {
		status: "rejected",
		reason: "target-unavailable",
		byteLength,
		chunksWritten: 0
	};
	const chunkMaxBytes = Math.max(1, options.chunkMaxBytes ?? 16384);
	let textIndex = 0;
	let chunksWritten = 0;
	monacoEditor.pushUndoStop();
	while (textIndex < text.length) {
		if (!isMonacoPasteTargetCurrent(monacoEditor, snapshot)) {
			monacoEditor.pushUndoStop();
			return {
				status: "cancelled",
				reason: "target-unavailable",
				byteLength,
				chunksWritten
			};
		}
		const selection = monacoEditor.getSelection();
		if (!selection) {
			monacoEditor.pushUndoStop();
			return {
				status: "cancelled",
				reason: "target-unavailable",
				byteLength,
				chunksWritten
			};
		}
		const nextIndex = getUtf8ChunkEndIndex(text, textIndex, chunkMaxBytes);
		const chunk = text.slice(textIndex, nextIndex);
		const endPosition = getEndPositionAfterInsert({
			lineNumber: selection.startLineNumber,
			column: selection.startColumn
		}, chunk);
		if (!monacoEditor.executeEdits("orca-large-paste", [{
			range: selection,
			text: chunk,
			forceMoveMarkers: true
		}])) {
			monacoEditor.pushUndoStop();
			return {
				status: "cancelled",
				reason: "target-unavailable",
				byteLength,
				chunksWritten
			};
		}
		setCollapsedSelection(monacoEditor, endPosition);
		textIndex = nextIndex;
		chunksWritten += 1;
		if (textIndex < text.length) await (options.yieldToEventLoop ?? yieldToEventLoop)();
	}
	monacoEditor.pushUndoStop();
	return {
		status: "pasted",
		mode: "chunked",
		byteLength,
		chunksWritten
	};
}
async function executeMonacoLargeTextPaste(monacoEditor, text, options) {
	const byteLengthMeasurement = await measureTextControlPasteByteLengthWithYield(text, {
		stopAfterBytes: options.maxBytes ?? 16777216,
		yieldAfterCodeUnits: options.measureYieldAfterCodeUnits,
		yieldToEventLoop: options.yieldToEventLoop
	});
	if (byteLengthMeasurement.exceededLimit) return {
		status: "rejected",
		reason: "too-large",
		byteLength: byteLengthMeasurement.byteLength,
		chunksWritten: 0
	};
	return insertMonacoTextInChunks(monacoEditor, text, byteLengthMeasurement.byteLength, options);
}
function handleMonacoLargeTextPaste(monacoEditor, event, options = {}) {
	if (event.defaultPrevented) return {
		status: "ignored",
		reason: "already-handled"
	};
	if (options.readOnly) return {
		status: "ignored",
		reason: "read-only"
	};
	if (!monacoEditor?.getModel()) return {
		status: "ignored",
		reason: "no-editor"
	};
	const text = getPlainTextFromPasteEvent(event);
	if (!text) return {
		status: "ignored",
		reason: "empty"
	};
	const directMaxBytes = options.directMaxBytes ?? 65536;
	const maxBytes = options.maxBytes ?? 16777216;
	const ownershipMeasurement = measureTextControlPasteByteLength(text, { stopAfterBytes: Math.min(directMaxBytes, maxBytes) });
	if (!ownershipMeasurement.exceededLimit) return {
		status: "ignored",
		reason: "small"
	};
	if (maxBytes <= directMaxBytes) {
		event.preventDefault();
		event.stopPropagation();
		const result = {
			status: "rejected",
			reason: "too-large",
			byteLength: ownershipMeasurement.byteLength,
			chunksWritten: 0
		};
		options.onPasteResult?.(result);
		return result;
	}
	event.preventDefault();
	event.stopPropagation();
	options.onPasteStart?.();
	executeMonacoLargeTextPaste(monacoEditor, text, options).then(options.onPasteResult);
	return { status: "handled" };
}
const ORCA_CONTEXT_MENU_PASTE_PRIORITY = 10001;
const ORCA_CONTEXT_MENU_PASTE_NAME = "orca-ipc-paste";
function resolvePasteMetadata(editorInstance, metadata, emptySelectionClipboardOptionId) {
	if (!metadata) return {
		pasteOnNewLine: false,
		multicursorText: null,
		mode: null
	};
	return {
		pasteOnNewLine: Boolean(editorInstance.getOption(emptySelectionClipboardOptionId)) && metadata.isFromEmptySelection === true,
		multicursorText: metadata.multicursorText !== void 0 ? metadata.multicursorText ?? null : null,
		mode: metadata.mode ?? null
	};
}
function runOrcaContextMenuPaste(deps) {
	const editorInstance = deps.getFocusedEditor();
	if (!editorInstance || !editorInstance.getModel() || !editorInstance.hasTextFocus()) return false;
	if (editorInstance.getOption(deps.readOnlyOptionId)) return false;
	return performOrcaContextMenuPaste(editorInstance, deps);
}
async function performOrcaContextMenuPaste(editorInstance, deps) {
	let text;
	try {
		text = await deps.readClipboardText({ maxBytes: MONACO_PASTE_MAX_BYTES });
	} catch (error) {
		deps.onReadError?.(error);
		return {
			status: "noop",
			reason: "read-failed"
		};
	}
	if (!text) return {
		status: "noop",
		reason: "empty"
	};
	if (measureTextControlPasteByteLength(text, { stopAfterBytes: 65536 }).exceededLimit) {
		const result = await executeMonacoLargeTextPaste(editorInstance, text, { readOnly: false });
		if (result.status === "rejected" && result.reason === "too-large") {
			deps.onTooLarge?.();
			return {
				status: "noop",
				reason: "too-large"
			};
		}
		if (result.status !== "pasted") return {
			status: "noop",
			reason: "target-lost"
		};
		return {
			status: "pasted",
			mode: "chunked"
		};
	}
	if (!editorInstance.hasTextFocus()) return {
		status: "noop",
		reason: "target-lost"
	};
	const { pasteOnNewLine, multicursorText, mode } = resolvePasteMetadata(editorInstance, deps.getClipboardMetadata(text), deps.emptySelectionClipboardOptionId);
	editorInstance.trigger("keyboard", "paste", {
		text,
		pasteOnNewLine,
		multicursorText,
		mode
	});
	return {
		status: "pasted",
		mode: "native"
	};
}
var installed = false;
function installMonacoContextMenuPaste(monaco) {
	if (installed || !PasteAction) return;
	installed = true;
	PasteAction.addImplementation(ORCA_CONTEXT_MENU_PASTE_PRIORITY, ORCA_CONTEXT_MENU_PASTE_NAME, () => runOrcaContextMenuPaste({
		getFocusedEditor: () => monaco.editor.getEditors().find((candidate) => candidate.hasTextFocus()) ?? null,
		readClipboardText: (options) => window.api.ui.readClipboardText(options),
		getClipboardMetadata: (text) => InMemoryClipboardMetadataManager.INSTANCE.get(text),
		emptySelectionClipboardOptionId: monaco.editor.EditorOption.emptySelectionClipboard,
		readOnlyOptionId: monaco.editor.EditorOption.readOnly,
		onTooLarge: () => {
			toast.error(translate("auto.components.editor.MonacoEditor.largePasteTooLarge", "Paste is too large."));
		}
	}));
}
globalThis.MonacoEnvironment = { getWorker(_workerId, label) {
	switch (label) {
		case "json": return new WorkerWrapper$1();
		case "css":
		case "scss":
		case "less": return new WorkerWrapper$2();
		case "html":
		case "handlebars":
		case "razor": return new WorkerWrapper$3();
		case "typescript":
		case "javascript": return new WorkerWrapper$4();
		default: return new WorkerWrapper();
	}
} };
var diagnosticsOptions = {
	noSemanticValidation: true,
	noSuggestionDiagnostics: true,
	noSyntaxValidation: true
};
typescriptDefaults.setDiagnosticsOptions(diagnosticsOptions);
javascriptDefaults.setDiagnosticsOptions(diagnosticsOptions);
typescriptDefaults.setCompilerOptions({
	...typescriptDefaults.getCompilerOptions(),
	jsx: JsxEmit.Preserve
});
javascriptDefaults.setCompilerOptions({
	...javascriptDefaults.getCompilerOptions(),
	jsx: JsxEmit.Preserve
});
registerVueLanguage(editor_main_exports);
registerSvelteLanguage(editor_main_exports);
registerAstroLanguage(editor_main_exports);
registerNimLanguage(editor_main_exports);
registerJsonlLanguage(editor_main_exports);
installMonacoDelayerCancellationGuard();
installMonacoDiffEditorDisposalGuard(editor_main_exports);
installMonacoPeekReferencesPreviewOptions();
installMonacoContextMenuPaste(editor_main_exports);
loader.config({ monaco: editor_main_exports });
export { Ft as n, we as r, handleMonacoLargeTextPaste as t };
