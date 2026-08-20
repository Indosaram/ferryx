import { t as __commonJSMin } from "./chunk-Dhmk_5SA.js";
var require_main = /* @__PURE__ */ __commonJSMin(((exports, module) => {
	(function(e, t) {
		"object" == typeof exports && "object" == typeof module ? module.exports = t() : "function" == typeof define && define.amd ? define([], t) : "object" == typeof exports ? exports.onig = t() : e.onig = t();
	})(exports, (() => {
		return e = {
			770: function(e$1, t$1, n) {
				"use strict";
				var r = this && this.__importDefault || function(e$2) {
					return e$2 && e$2.__esModule ? e$2 : { default: e$2 };
				};
				Object.defineProperty(t$1, "__esModule", { value: !0 }), t$1.setDefaultDebugCall = t$1.createOnigScanner = t$1.createOnigString = t$1.loadWASM = t$1.OnigScanner = t$1.OnigString = void 0;
				const i = r(n(418));
				let o = null, a = !1;
				class s {
					static _utf8ByteLength(e$2) {
						let t$2 = 0;
						for (let n$1 = 0, r$1 = e$2.length; n$1 < r$1; n$1++) {
							const i$1 = e$2.charCodeAt(n$1);
							let o$1 = i$1, a$1 = !1;
							if (i$1 >= 55296 && i$1 <= 56319 && n$1 + 1 < r$1) {
								const t$3 = e$2.charCodeAt(n$1 + 1);
								t$3 >= 56320 && t$3 <= 57343 && (o$1 = 65536 + (i$1 - 55296 << 10) | t$3 - 56320, a$1 = !0);
							}
							t$2 += o$1 <= 127 ? 1 : o$1 <= 2047 ? 2 : o$1 <= 65535 ? 3 : 4, a$1 && n$1++;
						}
						return t$2;
					}
					constructor(e$2) {
						const t$2 = e$2.length, n$1 = s._utf8ByteLength(e$2), r$1 = n$1 !== t$2, i$1 = r$1 ? new Uint32Array(t$2 + 1) : null;
						r$1 && (i$1[t$2] = n$1);
						const o$1 = r$1 ? new Uint32Array(n$1 + 1) : null;
						r$1 && (o$1[n$1] = t$2);
						const a$1 = new Uint8Array(n$1);
						let f$1 = 0;
						for (let n$2 = 0; n$2 < t$2; n$2++) {
							const s$1 = e$2.charCodeAt(n$2);
							let u$1 = s$1, c$1 = !1;
							if (s$1 >= 55296 && s$1 <= 56319 && n$2 + 1 < t$2) {
								const t$3 = e$2.charCodeAt(n$2 + 1);
								t$3 >= 56320 && t$3 <= 57343 && (u$1 = 65536 + (s$1 - 55296 << 10) | t$3 - 56320, c$1 = !0);
							}
							r$1 && (i$1[n$2] = f$1, c$1 && (i$1[n$2 + 1] = f$1), u$1 <= 127 ? o$1[f$1 + 0] = n$2 : u$1 <= 2047 ? (o$1[f$1 + 0] = n$2, o$1[f$1 + 1] = n$2) : u$1 <= 65535 ? (o$1[f$1 + 0] = n$2, o$1[f$1 + 1] = n$2, o$1[f$1 + 2] = n$2) : (o$1[f$1 + 0] = n$2, o$1[f$1 + 1] = n$2, o$1[f$1 + 2] = n$2, o$1[f$1 + 3] = n$2)), u$1 <= 127 ? a$1[f$1++] = u$1 : u$1 <= 2047 ? (a$1[f$1++] = 192 | (1984 & u$1) >>> 6, a$1[f$1++] = 128 | (63 & u$1) >>> 0) : u$1 <= 65535 ? (a$1[f$1++] = 224 | (61440 & u$1) >>> 12, a$1[f$1++] = 128 | (4032 & u$1) >>> 6, a$1[f$1++] = 128 | (63 & u$1) >>> 0) : (a$1[f$1++] = 240 | (1835008 & u$1) >>> 18, a$1[f$1++] = 128 | (258048 & u$1) >>> 12, a$1[f$1++] = 128 | (4032 & u$1) >>> 6, a$1[f$1++] = 128 | (63 & u$1) >>> 0), c$1 && n$2++;
						}
						this.utf16Length = t$2, this.utf8Length = n$1, this.utf16Value = e$2, this.utf8Value = a$1, this.utf16OffsetToUtf8 = i$1, this.utf8OffsetToUtf16 = o$1;
					}
					createString(e$2) {
						const t$2 = e$2._omalloc(this.utf8Length);
						return e$2.HEAPU8.set(this.utf8Value, t$2), t$2;
					}
				}
				class f {
					constructor(e$2) {
						if (this.id = ++f.LAST_ID, !o) throw new Error("Must invoke loadWASM first.");
						this._onigBinding = o, this.content = e$2;
						const t$2 = new s(e$2);
						this.utf16Length = t$2.utf16Length, this.utf8Length = t$2.utf8Length, this.utf16OffsetToUtf8 = t$2.utf16OffsetToUtf8, this.utf8OffsetToUtf16 = t$2.utf8OffsetToUtf16, this.utf8Length < 1e4 && !f._sharedPtrInUse ? (f._sharedPtr || (f._sharedPtr = o._omalloc(1e4)), f._sharedPtrInUse = !0, o.HEAPU8.set(t$2.utf8Value, f._sharedPtr), this.ptr = f._sharedPtr) : this.ptr = t$2.createString(o);
					}
					convertUtf8OffsetToUtf16(e$2) {
						return this.utf8OffsetToUtf16 ? e$2 < 0 ? 0 : e$2 > this.utf8Length ? this.utf16Length : this.utf8OffsetToUtf16[e$2] : e$2;
					}
					convertUtf16OffsetToUtf8(e$2) {
						return this.utf16OffsetToUtf8 ? e$2 < 0 ? 0 : e$2 > this.utf16Length ? this.utf8Length : this.utf16OffsetToUtf8[e$2] : e$2;
					}
					dispose() {
						this.ptr === f._sharedPtr ? f._sharedPtrInUse = !1 : this._onigBinding._ofree(this.ptr);
					}
				}
				t$1.OnigString = f, f.LAST_ID = 0, f._sharedPtr = 0, f._sharedPtrInUse = !1;
				class u {
					constructor(e$2, t$2) {
						var n$1, r$1;
						if (!o) throw new Error("Must invoke loadWASM first.");
						const i$1 = [], a$1 = [];
						for (let t$3 = 0, n$2 = e$2.length; t$3 < n$2; t$3++) {
							const n$3 = new s(e$2[t$3]);
							i$1[t$3] = n$3.createString(o), a$1[t$3] = n$3.utf8Length;
						}
						const f$1 = o._omalloc(4 * e$2.length);
						o.HEAPU32.set(i$1, f$1 / 4);
						const u$1 = o._omalloc(4 * e$2.length);
						o.HEAPU32.set(a$1, u$1 / 4), this._onigBinding = o, this._options = null !== (n$1 = null == t$2 ? void 0 : t$2.options) && void 0 !== n$1 ? n$1 : [10];
						const c$1 = this.onigOptions(this._options), _$1 = this.onigSyntax(null !== (r$1 = null == t$2 ? void 0 : t$2.syntax) && void 0 !== r$1 ? r$1 : 0), d = o._createOnigScanner(f$1, u$1, e$2.length, c$1, _$1);
						this._ptr = d;
						for (let t$3 = 0, n$2 = e$2.length; t$3 < n$2; t$3++) o._ofree(i$1[t$3]);
						o._ofree(u$1), o._ofree(f$1), 0 === d && function(e$3) {
							throw new Error(e$3.UTF8ToString(e$3._getLastOnigError()));
						}(o);
					}
					dispose() {
						this._onigBinding._freeOnigScanner(this._ptr);
					}
					findNextMatchSync(e$2, t$2, n$1) {
						let r$1 = a, i$1 = this._options;
						if (Array.isArray(n$1) ? (n$1.includes(25) && (r$1 = !0), i$1 = i$1.concat(n$1)) : "boolean" == typeof n$1 && (r$1 = n$1), "string" == typeof e$2) {
							e$2 = new f(e$2);
							const n$2 = this._findNextMatchSync(e$2, t$2, r$1, i$1);
							return e$2.dispose(), n$2;
						}
						return this._findNextMatchSync(e$2, t$2, r$1, i$1);
					}
					_findNextMatchSync(e$2, t$2, n$1, r$1) {
						const i$1 = this._onigBinding, o$1 = this.onigOptions(r$1);
						let a$1;
						if (a$1 = n$1 ? i$1._findNextOnigScannerMatchDbg(this._ptr, e$2.id, e$2.ptr, e$2.utf8Length, e$2.convertUtf16OffsetToUtf8(t$2), o$1) : i$1._findNextOnigScannerMatch(this._ptr, e$2.id, e$2.ptr, e$2.utf8Length, e$2.convertUtf16OffsetToUtf8(t$2), o$1), 0 === a$1) return null;
						const s$1 = i$1.HEAPU32;
						let f$1 = a$1 / 4;
						const u$1 = s$1[f$1++], c$1 = s$1[f$1++];
						let _$1 = [];
						for (let t$3 = 0; t$3 < c$1; t$3++) {
							const n$2 = e$2.convertUtf8OffsetToUtf16(s$1[f$1++]), r$2 = e$2.convertUtf8OffsetToUtf16(s$1[f$1++]);
							_$1[t$3] = {
								start: n$2,
								end: r$2,
								length: r$2 - n$2
							};
						}
						return {
							index: u$1,
							captureIndices: _$1
						};
					}
					onigOptions(e$2) {
						return e$2.map(((e$3) => this.onigOption(e$3))).reduce(((e$3, t$2) => e$3 | t$2), this._onigBinding.ONIG_OPTION_NONE);
					}
					onigSyntax(e$2) {
						switch (e$2) {
							case 0: return this._onigBinding.ONIG_SYNTAX_DEFAULT;
							case 1: return this._onigBinding.ONIG_SYNTAX_ASIS;
							case 2: return this._onigBinding.ONIG_SYNTAX_POSIX_BASIC;
							case 3: return this._onigBinding.ONIG_SYNTAX_POSIX_EXTENDED;
							case 4: return this._onigBinding.ONIG_SYNTAX_EMACS;
							case 5: return this._onigBinding.ONIG_SYNTAX_GREP;
							case 6: return this._onigBinding.ONIG_SYNTAX_GNU_REGEX;
							case 7: return this._onigBinding.ONIG_SYNTAX_JAVA;
							case 8: return this._onigBinding.ONIG_SYNTAX_PERL;
							case 9: return this._onigBinding.ONIG_SYNTAX_PERL_NG;
							case 10: return this._onigBinding.ONIG_SYNTAX_RUBY;
							case 11: return this._onigBinding.ONIG_SYNTAX_PYTHON;
							case 12: return this._onigBinding.ONIG_SYNTAX_ONIGURUMA;
						}
					}
					onigOption(e$2) {
						switch (e$2) {
							case 1: return this._onigBinding.ONIG_OPTION_NONE;
							case 0:
							case 25: return this._onigBinding.ONIG_OPTION_DEFAULT;
							case 2: return this._onigBinding.ONIG_OPTION_IGNORECASE;
							case 3: return this._onigBinding.ONIG_OPTION_EXTEND;
							case 4: return this._onigBinding.ONIG_OPTION_MULTILINE;
							case 5: return this._onigBinding.ONIG_OPTION_SINGLELINE;
							case 6: return this._onigBinding.ONIG_OPTION_FIND_LONGEST;
							case 7: return this._onigBinding.ONIG_OPTION_FIND_NOT_EMPTY;
							case 8: return this._onigBinding.ONIG_OPTION_NEGATE_SINGLELINE;
							case 9: return this._onigBinding.ONIG_OPTION_DONT_CAPTURE_GROUP;
							case 10: return this._onigBinding.ONIG_OPTION_CAPTURE_GROUP;
							case 11: return this._onigBinding.ONIG_OPTION_NOTBOL;
							case 12: return this._onigBinding.ONIG_OPTION_NOTEOL;
							case 13: return this._onigBinding.ONIG_OPTION_CHECK_VALIDITY_OF_STRING;
							case 14: return this._onigBinding.ONIG_OPTION_IGNORECASE_IS_ASCII;
							case 15: return this._onigBinding.ONIG_OPTION_WORD_IS_ASCII;
							case 16: return this._onigBinding.ONIG_OPTION_DIGIT_IS_ASCII;
							case 17: return this._onigBinding.ONIG_OPTION_SPACE_IS_ASCII;
							case 18: return this._onigBinding.ONIG_OPTION_POSIX_IS_ASCII;
							case 19: return this._onigBinding.ONIG_OPTION_TEXT_SEGMENT_EXTENDED_GRAPHEME_CLUSTER;
							case 20: return this._onigBinding.ONIG_OPTION_TEXT_SEGMENT_WORD;
							case 21: return this._onigBinding.ONIG_OPTION_NOT_BEGIN_STRING;
							case 22: return this._onigBinding.ONIG_OPTION_NOT_END_STRING;
							case 23: return this._onigBinding.ONIG_OPTION_NOT_BEGIN_POSITION;
							case 24: return this._onigBinding.ONIG_OPTION_CALLBACK_EACH_MATCH;
						}
					}
				}
				t$1.OnigScanner = u;
				let c = !1, _ = null;
				t$1.loadWASM = function(e$2) {
					if (c) return _;
					let t$2, n$1, r$1, a$1;
					if (c = !0, function(e$3) {
						return "function" == typeof e$3.instantiator;
					}(e$2)) t$2 = e$2.instantiator, n$1 = e$2.print;
					else {
						let r$2;
						!function(e$3) {
							return void 0 !== e$3.data;
						}(e$2) ? r$2 = e$2 : (r$2 = e$2.data, n$1 = e$2.print), t$2 = function(e$3) {
							return "undefined" != typeof Response && e$3 instanceof Response;
						}(r$2) ? "function" == typeof WebAssembly.instantiateStreaming ? function(e$3) {
							return (t$3) => WebAssembly.instantiateStreaming(e$3, t$3);
						}(r$2) : function(e$3) {
							return async (t$3) => {
								const n$2 = await e$3.arrayBuffer();
								return WebAssembly.instantiate(n$2, t$3);
							};
						}(r$2) : function(e$3) {
							return (t$3) => WebAssembly.instantiate(e$3, t$3);
						}(r$2);
					}
					return _ = new Promise(((e$3, t$3) => {
						r$1 = e$3, a$1 = t$3;
					})), function(e$3, t$3, n$2, r$2) {
						(0, i.default)({
							print: t$3,
							instantiateWasm: (t$4, n$3) => {
								if ("undefined" == typeof performance) {
									const e$4 = () => Date.now();
									t$4.env.emscripten_get_now = e$4, t$4.wasi_snapshot_preview1.emscripten_get_now = e$4;
								}
								return e$3(t$4).then(((e$4) => n$3(e$4.instance)), r$2), {};
							}
						}).then(((e$4) => {
							o = e$4, n$2();
						}));
					}(t$2, n$1, r$1, a$1), _;
				}, t$1.createOnigString = function(e$2) {
					return new f(e$2);
				}, t$1.createOnigScanner = function(e$2) {
					return new u(e$2);
				}, t$1.setDefaultDebugCall = function(e$2) {
					a = e$2;
				};
			},
			418: (e$1) => {
				e$1.exports = ("undefined" != typeof document && document.currentScript && document.currentScript.src, function(e$2 = {}) {
					var t$1, n, r = e$2;
					r.ready = new Promise(((e$3, r$1) => {
						t$1 = e$3, n = r$1;
					}));
					var i, o = Object.assign({}, r);
					i = (e$3) => {
						if ("function" == typeof readbuffer) return new Uint8Array(readbuffer(e$3));
						let t$2 = read(e$3, "binary");
						return "object" == typeof t$2 || P(n$1), t$2;
						var n$1;
					}, "undefined" == typeof clearTimeout && (globalThis.clearTimeout = (e$3) => {}), "undefined" == typeof setTimeout && (globalThis.setTimeout = (e$3) => "function" == typeof e$3 ? e$3() : P()), "undefined" != typeof onig_print && ("undefined" == typeof console && (console = {}), console.log = onig_print, console.warn = console.error = "undefined" != typeof printErr ? printErr : onig_print);
					var a, s, f = r.print || console.log.bind(console), u = r.printErr || console.error.bind(console);
					Object.assign(r, o), o = null, r.arguments && r.arguments, r.thisProgram && r.thisProgram, r.quit && r.quit, r.wasmBinary && (a = r.wasmBinary), r.noExitRuntime, "object" != typeof WebAssembly && P("no native wasm support detected");
					var c, _, d, g, l, h, p, O, v = !1;
					function m() {
						var e$3 = s.buffer;
						r.HEAP8 = c = new Int8Array(e$3), r.HEAP16 = d = new Int16Array(e$3), r.HEAPU8 = _ = new Uint8Array(e$3), r.HEAPU16 = g = new Uint16Array(e$3), r.HEAP32 = l = new Int32Array(e$3), r.HEAPU32 = h = new Uint32Array(e$3), r.HEAPF32 = p = new Float32Array(e$3), r.HEAPF64 = O = new Float64Array(e$3);
					}
					var y = [], I = [], T = [];
					var N = 0, A = null, S = null;
					function P(e$3) {
						r.onAbort && r.onAbort(e$3), u(e$3 = "Aborted(" + e$3 + ")"), v = !0, e$3 += ". Build with -sASSERTIONS for more info.";
						var t$2 = new WebAssembly.RuntimeError(e$3);
						throw n(t$2), t$2;
					}
					var E, w;
					function b(e$3) {
						return e$3.startsWith("data:application/octet-stream;base64,");
					}
					function C(e$3) {
						if (e$3 == E && a) return new Uint8Array(a);
						if (i) return i(e$3);
						throw "both async and sync fetching of the wasm failed";
					}
					function U(e$3, t$2, n$1) {
						return function(e$4) {
							return Promise.resolve().then((() => C(e$4)));
						}(e$3).then(((e$4) => WebAssembly.instantiate(e$4, t$2))).then(((e$4) => e$4)).then(n$1, ((e$4) => {
							u(`failed to asynchronously prepare wasm: ${e$4}`), P(e$4);
						}));
					}
					b(E = "onig.wasm") || (w = E, E = r.locateFile ? r.locateFile(w, "") : "" + w);
					var G = (e$3) => {
						for (; e$3.length > 0;) e$3.shift()(r);
					}, B = void 0, R = (e$3) => {
						for (var t$2 = "", n$1 = e$3; _[n$1];) t$2 += B[_[n$1++]];
						return t$2;
					}, W = {}, L = {}, D = {}, x = void 0, M = (e$3) => {
						throw new x(e$3);
					}, F = void 0, X = (e$3, t$2, n$1) => {
						function r$1(t$3) {
							var r$2 = n$1(t$3);
							r$2.length !== e$3.length && ((e$4) => {
								throw new F(e$4);
							})("Mismatched type converter count");
							for (var i$2 = 0; i$2 < e$3.length; ++i$2) k(e$3[i$2], r$2[i$2]);
						}
						e$3.forEach((function(e$4) {
							D[e$4] = t$2;
						}));
						var i$1 = new Array(t$2.length), o$1 = [], a$1 = 0;
						t$2.forEach(((e$4, t$3) => {
							L.hasOwnProperty(e$4) ? i$1[t$3] = L[e$4] : (o$1.push(e$4), W.hasOwnProperty(e$4) || (W[e$4] = []), W[e$4].push((() => {
								i$1[t$3] = L[e$4], ++a$1 === o$1.length && r$1(i$1);
							})));
						})), 0 === o$1.length && r$1(i$1);
					};
					function k(e$3, t$2, n$1 = {}) {
						if (!("argPackAdvance" in t$2)) throw new TypeError("registerType registeredInstance requires argPackAdvance");
						return function(e$4, t$3, n$2 = {}) {
							var r$1 = t$3.name;
							if (e$4 || M(`type "${r$1}" must have a positive integer typeid pointer`), L.hasOwnProperty(e$4)) {
								if (n$2.ignoreDuplicateRegistrations) return;
								M(`Cannot register type '${r$1}' twice`);
							}
							if (L[e$4] = t$3, delete D[e$4], W.hasOwnProperty(e$4)) {
								var i$1 = W[e$4];
								delete W[e$4], i$1.forEach(((e$5) => e$5()));
							}
						}(e$3, t$2, n$1);
					}
					function H() {
						this.allocated = [void 0], this.freelist = [];
					}
					var Y = new H(), j = () => {
						for (var e$3 = 0, t$2 = Y.reserved; t$2 < Y.allocated.length; ++t$2) void 0 !== Y.allocated[t$2] && ++e$3;
						return e$3;
					}, V = (e$3) => (e$3 || M("Cannot use deleted val. handle = " + e$3), Y.get(e$3).value), $ = (e$3) => {
						switch (e$3) {
							case void 0: return 1;
							case null: return 2;
							case !0: return 3;
							case !1: return 4;
							default: return Y.allocate({
								refcount: 1,
								value: e$3
							});
						}
					};
					function z(e$3) {
						return this.fromWireType(l[e$3 >> 2]);
					}
					var q = (e$3, t$2) => {
						switch (t$2) {
							case 4: return function(e$4) {
								return this.fromWireType(p[e$4 >> 2]);
							};
							case 8: return function(e$4) {
								return this.fromWireType(O[e$4 >> 3]);
							};
							default: throw new TypeError(`invalid float width (${t$2}): ${e$3}`);
						}
					}, K = (e$3, t$2, n$1) => {
						switch (t$2) {
							case 1: return n$1 ? (e$4) => c[e$4 >> 0] : (e$4) => _[e$4 >> 0];
							case 2: return n$1 ? (e$4) => d[e$4 >> 1] : (e$4) => g[e$4 >> 1];
							case 4: return n$1 ? (e$4) => l[e$4 >> 2] : (e$4) => h[e$4 >> 2];
							default: throw new TypeError(`invalid integer width (${t$2}): ${e$3}`);
						}
					};
					function J(e$3) {
						return this.fromWireType(h[e$3 >> 2]);
					}
					var Q, Z = "undefined" != typeof TextDecoder ? new TextDecoder("utf8") : void 0, ee = (e$3, t$2, n$1) => {
						for (var r$1 = t$2 + n$1, i$1 = t$2; e$3[i$1] && !(i$1 >= r$1);) ++i$1;
						if (i$1 - t$2 > 16 && e$3.buffer && Z) return Z.decode(e$3.subarray(t$2, i$1));
						for (var o$1 = ""; t$2 < i$1;) {
							var a$1 = e$3[t$2++];
							if (128 & a$1) {
								var s$1 = 63 & e$3[t$2++];
								if (192 != (224 & a$1)) {
									var f$1 = 63 & e$3[t$2++];
									if ((a$1 = 224 == (240 & a$1) ? (15 & a$1) << 12 | s$1 << 6 | f$1 : (7 & a$1) << 18 | s$1 << 12 | f$1 << 6 | 63 & e$3[t$2++]) < 65536) o$1 += String.fromCharCode(a$1);
									else {
										var u$1 = a$1 - 65536;
										o$1 += String.fromCharCode(55296 | u$1 >> 10, 56320 | 1023 & u$1);
									}
								} else o$1 += String.fromCharCode((31 & a$1) << 6 | s$1);
							} else o$1 += String.fromCharCode(a$1);
						}
						return o$1;
					}, te = (e$3, t$2) => e$3 ? ee(_, e$3, t$2) : "", ne = "undefined" != typeof TextDecoder ? new TextDecoder("utf-16le") : void 0, re = (e$3, t$2) => {
						for (var n$1 = e$3, r$1 = n$1 >> 1, i$1 = r$1 + t$2 / 2; !(r$1 >= i$1) && g[r$1];) ++r$1;
						if ((n$1 = r$1 << 1) - e$3 > 32 && ne) return ne.decode(_.subarray(e$3, n$1));
						for (var o$1 = "", a$1 = 0; !(a$1 >= t$2 / 2); ++a$1) {
							var s$1 = d[e$3 + 2 * a$1 >> 1];
							if (0 == s$1) break;
							o$1 += String.fromCharCode(s$1);
						}
						return o$1;
					}, ie = (e$3, t$2, n$1) => {
						if (void 0 === n$1 && (n$1 = 2147483647), n$1 < 2) return 0;
						for (var r$1 = t$2, i$1 = (n$1 -= 2) < 2 * e$3.length ? n$1 / 2 : e$3.length, o$1 = 0; o$1 < i$1; ++o$1) {
							var a$1 = e$3.charCodeAt(o$1);
							d[t$2 >> 1] = a$1, t$2 += 2;
						}
						return d[t$2 >> 1] = 0, t$2 - r$1;
					}, oe = (e$3) => 2 * e$3.length, ae = (e$3, t$2) => {
						for (var n$1 = 0, r$1 = ""; !(n$1 >= t$2 / 4);) {
							var i$1 = l[e$3 + 4 * n$1 >> 2];
							if (0 == i$1) break;
							if (++n$1, i$1 >= 65536) {
								var o$1 = i$1 - 65536;
								r$1 += String.fromCharCode(55296 | o$1 >> 10, 56320 | 1023 & o$1);
							} else r$1 += String.fromCharCode(i$1);
						}
						return r$1;
					}, se = (e$3, t$2, n$1) => {
						if (void 0 === n$1 && (n$1 = 2147483647), n$1 < 4) return 0;
						for (var r$1 = t$2, i$1 = r$1 + n$1 - 4, o$1 = 0; o$1 < e$3.length; ++o$1) {
							var a$1 = e$3.charCodeAt(o$1);
							if (a$1 >= 55296 && a$1 <= 57343 && (a$1 = 65536 + ((1023 & a$1) << 10) | 1023 & e$3.charCodeAt(++o$1)), l[t$2 >> 2] = a$1, (t$2 += 4) + 4 > i$1) break;
						}
						return l[t$2 >> 2] = 0, t$2 - r$1;
					}, fe = (e$3) => {
						for (var t$2 = 0, n$1 = 0; n$1 < e$3.length; ++n$1) {
							var r$1 = e$3.charCodeAt(n$1);
							r$1 >= 55296 && r$1 <= 57343 && ++n$1, t$2 += 4;
						}
						return t$2;
					};
					Q = () => performance.now();
					var ue = (e$3) => {
						var t$2 = (e$3 - s.buffer.byteLength + 65535) / 65536;
						try {
							return s.grow(t$2), m(), 1;
						} catch (e$4) {}
					}, ce = [
						null,
						[],
						[]
					];
					(() => {
						for (var e$3 = new Array(256), t$2 = 0; t$2 < 256; ++t$2) e$3[t$2] = String.fromCharCode(t$2);
						B = e$3;
					})(), x = r.BindingError = class extends Error {
						constructor(e$3) {
							super(e$3), this.name = "BindingError";
						}
					}, F = r.InternalError = class extends Error {
						constructor(e$3) {
							super(e$3), this.name = "InternalError";
						}
					}, Object.assign(H.prototype, {
						get(e$3) {
							return this.allocated[e$3];
						},
						has(e$3) {
							return void 0 !== this.allocated[e$3];
						},
						allocate(e$3) {
							var t$2 = this.freelist.pop() || this.allocated.length;
							return this.allocated[t$2] = e$3, t$2;
						},
						free(e$3) {
							this.allocated[e$3] = void 0, this.freelist.push(e$3);
						}
					}), Y.allocated.push({ value: void 0 }, { value: null }, { value: !0 }, { value: !1 }), Y.reserved = Y.allocated.length, r.count_emval_handles = j;
					var _e, de = {
						_embind_register_bigint: (e$3, t$2, n$1, r$1, i$1) => {},
						_embind_register_bool: (e$3, t$2, n$1, r$1) => {
							k(e$3, {
								name: t$2 = R(t$2),
								fromWireType: function(e$4) {
									return !!e$4;
								},
								toWireType: function(e$4, t$3) {
									return t$3 ? n$1 : r$1;
								},
								argPackAdvance: 8,
								readValueFromPointer: function(e$4) {
									return this.fromWireType(_[e$4]);
								},
								destructorFunction: null
							});
						},
						_embind_register_constant: (e$3, t$2, n$1) => {
							e$3 = R(e$3), X([], [t$2], (function(t$3) {
								return t$3 = t$3[0], r[e$3] = t$3.fromWireType(n$1), [];
							}));
						},
						_embind_register_emval: (e$3, t$2) => {
							k(e$3, {
								name: t$2 = R(t$2),
								fromWireType: (e$4) => {
									var t$3 = V(e$4);
									return ((e$5) => {
										e$5 >= Y.reserved && 0 == --Y.get(e$5).refcount && Y.free(e$5);
									})(e$4), t$3;
								},
								toWireType: (e$4, t$3) => $(t$3),
								argPackAdvance: 8,
								readValueFromPointer: z,
								destructorFunction: null
							});
						},
						_embind_register_float: (e$3, t$2, n$1) => {
							k(e$3, {
								name: t$2 = R(t$2),
								fromWireType: (e$4) => e$4,
								toWireType: (e$4, t$3) => t$3,
								argPackAdvance: 8,
								readValueFromPointer: q(t$2, n$1),
								destructorFunction: null
							});
						},
						_embind_register_integer: (e$3, t$2, n$1, r$1, i$1) => {
							t$2 = R(t$2), -1 === i$1 && (i$1 = 4294967295);
							var o$1 = (e$4) => e$4;
							if (0 === r$1) {
								var a$1 = 32 - 8 * n$1;
								o$1 = (e$4) => e$4 << a$1 >>> a$1;
							}
							var s$1 = t$2.includes("unsigned");
							k(e$3, {
								name: t$2,
								fromWireType: o$1,
								toWireType: s$1 ? function(e$4, t$3) {
									return this.name, t$3 >>> 0;
								} : function(e$4, t$3) {
									return this.name, t$3;
								},
								argPackAdvance: 8,
								readValueFromPointer: K(t$2, n$1, 0 !== r$1),
								destructorFunction: null
							});
						},
						_embind_register_memory_view: (e$3, t$2, n$1) => {
							var r$1 = [
								Int8Array,
								Uint8Array,
								Int16Array,
								Uint16Array,
								Int32Array,
								Uint32Array,
								Float32Array,
								Float64Array
							][t$2];
							function i$1(e$4) {
								var t$3 = h[e$4 >> 2], n$2 = h[e$4 + 4 >> 2];
								return new r$1(c.buffer, n$2, t$3);
							}
							k(e$3, {
								name: n$1 = R(n$1),
								fromWireType: i$1,
								argPackAdvance: 8,
								readValueFromPointer: i$1
							}, { ignoreDuplicateRegistrations: !0 });
						},
						_embind_register_std_string: (e$3, t$2) => {
							var n$1 = "std::string" === (t$2 = R(t$2));
							k(e$3, {
								name: t$2,
								fromWireType: (e$4) => {
									var t$3, r$1 = h[e$4 >> 2], i$1 = e$4 + 4;
									if (n$1) for (var o$1 = i$1, a$1 = 0; a$1 <= r$1; ++a$1) {
										var s$1 = i$1 + a$1;
										if (a$1 == r$1 || 0 == _[s$1]) {
											var f$1 = te(o$1, s$1 - o$1);
											void 0 === t$3 ? t$3 = f$1 : (t$3 += String.fromCharCode(0), t$3 += f$1), o$1 = s$1 + 1;
										}
									}
									else {
										var u$1 = new Array(r$1);
										for (a$1 = 0; a$1 < r$1; ++a$1) u$1[a$1] = String.fromCharCode(_[i$1 + a$1]);
										t$3 = u$1.join("");
									}
									return he(e$4), t$3;
								},
								toWireType: (e$4, t$3) => {
									var r$1;
									t$3 instanceof ArrayBuffer && (t$3 = new Uint8Array(t$3));
									var i$1 = "string" == typeof t$3;
									i$1 || t$3 instanceof Uint8Array || t$3 instanceof Uint8ClampedArray || t$3 instanceof Int8Array || M("Cannot pass non-string to std::string"), r$1 = n$1 && i$1 ? ((e$5) => {
										for (var t$4 = 0, n$2 = 0; n$2 < e$5.length; ++n$2) {
											var r$2 = e$5.charCodeAt(n$2);
											r$2 <= 127 ? t$4++ : r$2 <= 2047 ? t$4 += 2 : r$2 >= 55296 && r$2 <= 57343 ? (t$4 += 4, ++n$2) : t$4 += 3;
										}
										return t$4;
									})(t$3) : t$3.length;
									var o$1 = le(4 + r$1 + 1), a$1 = o$1 + 4;
									if (h[o$1 >> 2] = r$1, n$1 && i$1) ((e$5, t$4, n$2, r$2) => {
										if (!(r$2 > 0)) return 0;
										for (var o$2 = n$2 + r$2 - 1, a$2 = 0; a$2 < e$5.length; ++a$2) {
											var s$2 = e$5.charCodeAt(a$2);
											if (s$2 >= 55296 && s$2 <= 57343 && (s$2 = 65536 + ((1023 & s$2) << 10) | 1023 & e$5.charCodeAt(++a$2)), s$2 <= 127) {
												if (n$2 >= o$2) break;
												t$4[n$2++] = s$2;
											} else if (s$2 <= 2047) {
												if (n$2 + 1 >= o$2) break;
												t$4[n$2++] = 192 | s$2 >> 6, t$4[n$2++] = 128 | 63 & s$2;
											} else if (s$2 <= 65535) {
												if (n$2 + 2 >= o$2) break;
												t$4[n$2++] = 224 | s$2 >> 12, t$4[n$2++] = 128 | s$2 >> 6 & 63, t$4[n$2++] = 128 | 63 & s$2;
											} else {
												if (n$2 + 3 >= o$2) break;
												t$4[n$2++] = 240 | s$2 >> 18, t$4[n$2++] = 128 | s$2 >> 12 & 63, t$4[n$2++] = 128 | s$2 >> 6 & 63, t$4[n$2++] = 128 | 63 & s$2;
											}
										}
										t$4[n$2] = 0;
									})(t$3, _, a$1, r$1 + 1);
									else if (i$1) for (var s$1 = 0; s$1 < r$1; ++s$1) {
										var f$1 = t$3.charCodeAt(s$1);
										f$1 > 255 && (he(a$1), M("String has UTF-16 code units that do not fit in 8 bits")), _[a$1 + s$1] = f$1;
									}
									else for (s$1 = 0; s$1 < r$1; ++s$1) _[a$1 + s$1] = t$3[s$1];
									return null !== e$4 && e$4.push(he, o$1), o$1;
								},
								argPackAdvance: 8,
								readValueFromPointer: J,
								destructorFunction: (e$4) => he(e$4)
							});
						},
						_embind_register_std_wstring: (e$3, t$2, n$1) => {
							var r$1, i$1, o$1, a$1, s$1;
							n$1 = R(n$1), 2 === t$2 ? (r$1 = re, i$1 = ie, a$1 = oe, o$1 = () => g, s$1 = 1) : 4 === t$2 && (r$1 = ae, i$1 = se, a$1 = fe, o$1 = () => h, s$1 = 2), k(e$3, {
								name: n$1,
								fromWireType: (e$4) => {
									for (var n$2, i$2 = h[e$4 >> 2], a$2 = o$1(), f$1 = e$4 + 4, u$1 = 0; u$1 <= i$2; ++u$1) {
										var c$1 = e$4 + 4 + u$1 * t$2;
										if (u$1 == i$2 || 0 == a$2[c$1 >> s$1]) {
											var _$1 = r$1(f$1, c$1 - f$1);
											void 0 === n$2 ? n$2 = _$1 : (n$2 += String.fromCharCode(0), n$2 += _$1), f$1 = c$1 + t$2;
										}
									}
									return he(e$4), n$2;
								},
								toWireType: (e$4, r$2) => {
									"string" != typeof r$2 && M(`Cannot pass non-string to C++ string type ${n$1}`);
									var o$2 = a$1(r$2), f$1 = le(4 + o$2 + t$2);
									return h[f$1 >> 2] = o$2 >> s$1, i$1(r$2, f$1 + 4, o$2 + t$2), null !== e$4 && e$4.push(he, f$1), f$1;
								},
								argPackAdvance: 8,
								readValueFromPointer: z,
								destructorFunction: (e$4) => he(e$4)
							});
						},
						_embind_register_void: (e$3, t$2) => {
							k(e$3, {
								isVoid: !0,
								name: t$2 = R(t$2),
								argPackAdvance: 0,
								fromWireType: () => {},
								toWireType: (e$4, t$3) => {}
							});
						},
						emscripten_get_now: Q,
						emscripten_memcpy_big: (e$3, t$2, n$1) => _.copyWithin(e$3, t$2, t$2 + n$1),
						emscripten_resize_heap: (e$3) => {
							var t$2 = _.length, n$1 = 2147483648;
							if ((e$3 >>>= 0) > n$1) return !1;
							for (var r$1, i$1 = 1; i$1 <= 4; i$1 *= 2) {
								var o$1 = t$2 * (1 + .2 / i$1);
								o$1 = Math.min(o$1, e$3 + 100663296);
								if (ue(Math.min(n$1, (r$1 = Math.max(e$3, o$1)) + (65536 - r$1 % 65536) % 65536))) return !0;
							}
							return !1;
						},
						fd_write: (e$3, t$2, n$1, r$1) => {
							for (var i$1 = 0, o$1 = 0; o$1 < n$1; o$1++) {
								var a$1 = h[t$2 >> 2], s$1 = h[t$2 + 4 >> 2];
								t$2 += 8;
								for (var c$1 = 0; c$1 < s$1; c$1++) d$1 = e$3, g$1 = _[a$1 + c$1], l$1 = void 0, l$1 = ce[d$1], 0 === g$1 || 10 === g$1 ? ((1 === d$1 ? f : u)(ee(l$1, 0)), l$1.length = 0) : l$1.push(g$1);
								i$1 += s$1;
							}
							var d$1, g$1, l$1;
							return h[r$1 >> 2] = i$1, 0;
						}
					}, ge = function() {
						var e$3, t$2, i$1, o$1, f$1 = {
							env: de,
							wasi_snapshot_preview1: de
						};
						function c$1(e$4, t$3) {
							var n$1, i$2 = e$4.exports;
							return s = (ge = i$2).memory, m(), ge.__indirect_function_table, n$1 = ge.__wasm_call_ctors, I.unshift(n$1), function(e$5) {
								if (N--, r.monitorRunDependencies && r.monitorRunDependencies(N), 0 == N && (null !== A && (clearInterval(A), A = null), S)) {
									var t$4 = S;
									S = null, t$4();
								}
							}(), i$2;
						}
						if (N++, r.monitorRunDependencies && r.monitorRunDependencies(N), r.instantiateWasm) try {
							return r.instantiateWasm(f$1, c$1);
						} catch (e$4) {
							u(`Module.instantiateWasm callback failed with error: ${e$4}`), n(e$4);
						}
						return (e$3 = a, t$2 = E, i$1 = f$1, o$1 = function(e$4) {
							c$1(e$4.instance);
						}, e$3 || "function" != typeof WebAssembly.instantiateStreaming || b(t$2) || "function" != typeof fetch ? U(t$2, i$1, o$1) : fetch(t$2, { credentials: "same-origin" }).then(((e$4) => WebAssembly.instantiateStreaming(e$4, i$1).then(o$1, (function(e$5) {
							return u(`wasm streaming compile failed: ${e$5}`), u("falling back to ArrayBuffer instantiation"), U(t$2, i$1, o$1);
						}))))).catch(n), {};
					}(), le = (e$3) => (le = ge.malloc)(e$3), he = (e$3) => (he = ge.free)(e$3);
					function pe() {
						function e$3() {
							_e || (_e = !0, r.calledRun = !0, v || (G(I), t$1(r), r.onRuntimeInitialized && r.onRuntimeInitialized(), function() {
								if (r.postRun) for ("function" == typeof r.postRun && (r.postRun = [r.postRun]); r.postRun.length;) e$4 = r.postRun.shift(), T.unshift(e$4);
								var e$4;
								G(T);
							}()));
						}
						N > 0 || (function() {
							if (r.preRun) for ("function" == typeof r.preRun && (r.preRun = [r.preRun]); r.preRun.length;) e$4 = r.preRun.shift(), y.unshift(e$4);
							var e$4;
							G(y);
						}(), N > 0 || (r.setStatus ? (r.setStatus("Running..."), setTimeout((function() {
							setTimeout((function() {
								r.setStatus("");
							}), 1), e$3();
						}), 1)) : e$3()));
					}
					if (r._omalloc = (e$3) => (r._omalloc = ge.omalloc)(e$3), r._ofree = (e$3) => (r._ofree = ge.ofree)(e$3), r._getLastOnigError = () => (r._getLastOnigError = ge.getLastOnigError)(), r._createOnigScanner = (e$3, t$2, n$1, i$1, o$1) => (r._createOnigScanner = ge.createOnigScanner)(e$3, t$2, n$1, i$1, o$1), r._freeOnigScanner = (e$3) => (r._freeOnigScanner = ge.freeOnigScanner)(e$3), r._findNextOnigScannerMatch = (e$3, t$2, n$1, i$1, o$1, a$1) => (r._findNextOnigScannerMatch = ge.findNextOnigScannerMatch)(e$3, t$2, n$1, i$1, o$1, a$1), r._findNextOnigScannerMatchDbg = (e$3, t$2, n$1, i$1, o$1, a$1) => (r._findNextOnigScannerMatchDbg = ge.findNextOnigScannerMatchDbg)(e$3, t$2, n$1, i$1, o$1, a$1), r.__embind_initialize_bindings = () => (r.__embind_initialize_bindings = ge._embind_initialize_bindings)(), r.dynCall_jiji = (e$3, t$2, n$1, i$1, o$1) => (r.dynCall_jiji = ge.dynCall_jiji)(e$3, t$2, n$1, i$1, o$1), r.UTF8ToString = te, S = function e$3() {
						_e || pe(), _e || (S = e$3);
					}, r.preInit) for ("function" == typeof r.preInit && (r.preInit = [r.preInit]); r.preInit.length > 0;) r.preInit.pop()();
					return pe(), e$2.ready;
				});
			}
		}, t = {}, function n(r) {
			var i = t[r];
			if (void 0 !== i) return i.exports;
			var o = t[r] = { exports: {} };
			return e[r].call(o.exports, o, o.exports, n), o.exports;
		}(770);
		var e, t;
	}));
}));
export default require_main();
