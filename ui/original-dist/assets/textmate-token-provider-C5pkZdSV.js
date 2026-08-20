const __vite__mapDeps=(i,m=__vite__mapDeps,d=(m.f||(m.f=["./main-BIUnxdI8.js","./chunk-Dhmk_5SA.js"])))=>i.map(i=>d[i]);
import { a as __toDynamicImportESM, t as __commonJSMin } from "./chunk-Dhmk_5SA.js";
import { t as __vitePreload } from "./preload-helper-Cgw39-ka.js";
var import_main = (/* @__PURE__ */ __commonJSMin(((exports, module) => {
	(function(e, t) {
		"object" == typeof exports && "object" == typeof module ? module.exports = t() : "function" == typeof define && define.amd ? define([], t) : "object" == typeof exports ? exports.vscodetextmate = t() : e.vscodetextmate = t();
	})(exports, (() => (() => {
		"use strict";
		var e = {
			185: (e$1, t$1) => {
				Object.defineProperty(t$1, "__esModule", { value: !0 }), t$1.UseOnigurumaFindOptions = t$1.DebugFlags = void 0, t$1.DebugFlags = { InDebugMode: "undefined" != typeof process && !!{}.VSCODE_TEXTMATE_DEBUG }, t$1.UseOnigurumaFindOptions = !1;
			},
			151: (e$1, t$1, n) => {
				Object.defineProperty(t$1, "__esModule", { value: !0 }), t$1.applyStateStackDiff = t$1.diffStateStacksRefEq = void 0;
				const s = n(752);
				t$1.diffStateStacksRefEq = function(e$2, t$2) {
					let n$1 = 0;
					const s$1 = [];
					let r = e$2, i = t$2;
					for (; r !== i;) r && (!i || r.depth >= i.depth) ? (n$1++, r = r.parent) : (s$1.push(i.toStateStackFrame()), i = i.parent);
					return {
						pops: n$1,
						newFrames: s$1.reverse()
					};
				}, t$1.applyStateStackDiff = function(e$2, t$2) {
					let n$1 = e$2;
					for (let e$3 = 0; e$3 < t$2.pops; e$3++) n$1 = n$1.parent;
					for (const e$3 of t$2.newFrames) n$1 = s.StateStackImpl.pushFrame(n$1, e$3);
					return n$1;
				};
			},
			490: (e$1, t$1) => {
				Object.defineProperty(t$1, "__esModule", { value: !0 }), t$1.toOptionalTokenType = t$1.EncodedTokenAttributes = t$1.FontAttribute = void 0;
				class n {
					constructor(e$2, t$2, n$1) {
						this.fontFamily = e$2, this.fontSize = t$2, this.lineHeight = n$1;
					}
					static _getKey(e$2, t$2, n$1) {
						return `${e$2}|${t$2}|${n$1}`;
					}
					static _get(e$2, t$2, s$1) {
						const r = this._getKey(e$2, t$2, s$1);
						let i = this._map.get(r);
						return i || (i = new n(e$2, t$2, s$1), this._map.set(r, i)), i;
					}
					static from(e$2, t$2, s$1) {
						return new n(e$2, t$2, s$1);
					}
					with(e$2) {
						return e$2 ? n._get(e$2.fontFamily || this.fontFamily, e$2.fontSize || this.fontSize, e$2.lineHeight || this.lineHeight) : this;
					}
				}
				var s;
				t$1.FontAttribute = n, n._map = /* @__PURE__ */ new Map(), (s = t$1.EncodedTokenAttributes || (t$1.EncodedTokenAttributes = {})).toBinaryStr = function(e$2) {
					return e$2.toString(2).padStart(32, "0");
				}, s.print = function(e$2) {
					const t$2 = s.getLanguageId(e$2), n$1 = s.getTokenType(e$2), r = s.getFontStyle(e$2), i = s.getForeground(e$2), o = s.getBackground(e$2);
					console.log({
						languageId: t$2,
						tokenType: n$1,
						fontStyle: r,
						foreground: i,
						background: o
					});
				}, s.getLanguageId = function(e$2) {
					return (255 & e$2) >>> 0;
				}, s.getTokenType = function(e$2) {
					return (768 & e$2) >>> 8;
				}, s.containsBalancedBrackets = function(e$2) {
					return !!(1024 & e$2);
				}, s.getFontStyle = function(e$2) {
					return (30720 & e$2) >>> 11;
				}, s.getForeground = function(e$2) {
					return (16744448 & e$2) >>> 15;
				}, s.getBackground = function(e$2) {
					return (4278190080 & e$2) >>> 24;
				}, s.set = function(e$2, t$2, n$1, r, i, o, a) {
					let c = s.getLanguageId(e$2), l = s.getTokenType(e$2), u = s.containsBalancedBrackets(e$2) ? 1 : 0, h = s.getFontStyle(e$2), p = s.getForeground(e$2), d = s.getBackground(e$2);
					return 0 !== t$2 && (c = t$2), 8 !== n$1 && (l = n$1), null !== r && (u = r ? 1 : 0), -1 !== i && (h = i), 0 !== o && (p = o), 0 !== a && (d = a), (c | l << 8 | u << 10 | h << 11 | p << 15 | d << 24) >>> 0;
				}, t$1.toOptionalTokenType = function(e$2) {
					return e$2;
				};
			},
			214: (e$1, t$1, n) => {
				Object.defineProperty(t$1, "__esModule", { value: !0 }), t$1.BasicScopeAttributesProvider = t$1.BasicScopeAttributes = void 0;
				const s = n(807);
				class r {
					constructor(e$2, t$2) {
						this.languageId = e$2, this.tokenType = t$2;
					}
				}
				t$1.BasicScopeAttributes = r;
				class i {
					constructor(e$2, t$2) {
						this._getBasicScopeAttributes = new s.CachedFn(((e$3) => {
							return new r(this._scopeToLanguage(e$3), this._toStandardTokenType(e$3));
						})), this._defaultAttributes = new r(e$2, 8), this._embeddedLanguagesMatcher = new o(Object.entries(t$2 || {}));
					}
					getDefaultAttributes() {
						return this._defaultAttributes;
					}
					getBasicScopeAttributes(e$2) {
						return null === e$2 ? i._NULL_SCOPE_METADATA : this._getBasicScopeAttributes.get(e$2);
					}
					_scopeToLanguage(e$2) {
						return this._embeddedLanguagesMatcher.match(e$2) || 0;
					}
					_toStandardTokenType(e$2) {
						const t$2 = e$2.match(i.STANDARD_TOKEN_TYPE_REGEXP);
						if (!t$2) return 8;
						switch (t$2[1]) {
							case "comment": return 1;
							case "string": return 2;
							case "regex": return 3;
							case "meta.embedded": return 0;
						}
						throw new Error("Unexpected match for standard token type!");
					}
				}
				t$1.BasicScopeAttributesProvider = i, i._NULL_SCOPE_METADATA = new r(0, 0), i.STANDARD_TOKEN_TYPE_REGEXP = /\b(comment|string|regex|meta\.embedded)\b/;
				class o {
					constructor(e$2) {
						if (0 === e$2.length) this.values = null, this.scopesRegExp = null;
						else {
							this.values = new Map(e$2);
							const t$2 = e$2.map((([e$3, t$3]) => s.escapeRegExpCharacters(e$3)));
							t$2.sort(), t$2.reverse(), this.scopesRegExp = new RegExp(`^((${t$2.join(")|(")}))($|\\.)`, "");
						}
					}
					match(e$2) {
						if (!this.scopesRegExp) return;
						const t$2 = e$2.match(this.scopesRegExp);
						return t$2 ? this.values.get(t$2[1]) : void 0;
					}
				}
			},
			929: (e$1, t$1, n) => {
				Object.defineProperty(t$1, "__esModule", { value: !0 }), t$1.LineFonts = t$1.FontInfo = t$1.LineTokens = t$1.BalancedBracketSelectors = t$1.StateStackImpl = t$1.AttributedScopeStack = t$1.Grammar = t$1.createGrammar = void 0;
				const s = n(185), r = n(490), i = n(916), o = n(810), a = n(666), c = n(63), l = n(807), u = n(214), h = n(398);
				function p(e$2, t$2, n$1, s$1, r$1) {
					const o$1 = i.createMatchers(t$2, d), c$1 = a.RuleFactory.getCompiledRuleId(n$1, s$1, r$1.repository);
					for (const n$2 of o$1) e$2.push({
						debugSelector: t$2,
						matcher: n$2.matcher,
						ruleId: c$1,
						grammar: r$1,
						priority: n$2.priority
					});
				}
				function d(e$2, t$2) {
					if (t$2.length < e$2.length) return !1;
					let n$1 = 0;
					return e$2.every(((e$3) => {
						for (let s$1 = n$1; s$1 < t$2.length; s$1++) if (f(t$2[s$1], e$3)) return n$1 = s$1 + 1, !0;
						return !1;
					}));
				}
				function f(e$2, t$2) {
					if (!e$2) return !1;
					if (e$2 === t$2) return !0;
					const n$1 = t$2.length;
					return e$2.length > n$1 && e$2.substr(0, n$1) === t$2 && "." === e$2[n$1];
				}
				t$1.createGrammar = function(e$2, t$2, n$1, s$1, r$1, i$1, o$1, a$1) {
					return new m(e$2, t$2, n$1, s$1, r$1, i$1, o$1, a$1);
				};
				class m {
					constructor(e$2, t$2, n$1, s$1, r$1, o$1, a$1, c$1) {
						if (this._rootScopeName = e$2, this.balancedBracketSelectors = o$1, this._onigLib = c$1, this._basicScopeAttributesProvider = new u.BasicScopeAttributesProvider(n$1, s$1), this._rootId = -1, this._lastRuleId = 0, this._ruleId2desc = [null], this._includedGrammars = {}, this._grammarRepository = a$1, this._grammar = g(t$2, null), this._injections = null, this._tokenTypeMatchers = [], r$1) for (const e$3 of Object.keys(r$1)) {
							const t$3 = i.createMatchers(e$3, d);
							for (const n$2 of t$3) this._tokenTypeMatchers.push({
								matcher: n$2.matcher,
								type: r$1[e$3]
							});
						}
					}
					get themeProvider() {
						return this._grammarRepository;
					}
					dispose() {
						for (const e$2 of this._ruleId2desc) e$2 && e$2.dispose();
					}
					createOnigScanner(e$2) {
						return this._onigLib.createOnigScanner(e$2);
					}
					createOnigString(e$2) {
						return this._onigLib.createOnigString(e$2);
					}
					getMetadataForScope(e$2) {
						return this._basicScopeAttributesProvider.getBasicScopeAttributes(e$2);
					}
					_collectInjections() {
						const e$2 = [], t$2 = this._rootScopeName, n$1 = ((e$3) => e$3 === this._rootScopeName ? this._grammar : this.getExternalGrammar(e$3))(t$2);
						if (n$1) {
							const s$1 = n$1.injections;
							if (s$1) for (let t$3 in s$1) p(e$2, t$3, s$1[t$3], this, n$1);
							const r$1 = this._grammarRepository.injections(t$2);
							r$1 && r$1.forEach(((t$3) => {
								const n$2 = this.getExternalGrammar(t$3);
								if (n$2) {
									const t$4 = n$2.injectionSelector;
									t$4 && p(e$2, t$4, n$2, this, n$2);
								}
							}));
						}
						return e$2.sort(((e$3, t$3) => e$3.priority - t$3.priority)), e$2;
					}
					getInjections() {
						if (null === this._injections && (this._injections = this._collectInjections(), s.DebugFlags.InDebugMode && this._injections.length > 0)) {
							console.log(`Grammar ${this._rootScopeName} contains the following injections:`);
							for (const e$2 of this._injections) console.log(`  - ${e$2.debugSelector}`);
						}
						return this._injections;
					}
					registerRule(e$2) {
						const t$2 = ++this._lastRuleId, n$1 = e$2(a.ruleIdFromNumber(t$2));
						return this._ruleId2desc[t$2] = n$1, n$1;
					}
					getRule(e$2) {
						return this._ruleId2desc[a.ruleIdToNumber(e$2)];
					}
					getExternalGrammar(e$2, t$2) {
						if (this._includedGrammars[e$2]) return this._includedGrammars[e$2];
						if (this._grammarRepository) {
							const n$1 = this._grammarRepository.lookup(e$2);
							if (n$1) return this._includedGrammars[e$2] = g(n$1, t$2 && t$2.$base), this._includedGrammars[e$2];
						}
					}
					tokenizeLine(e$2, t$2, n$1 = 0) {
						const s$1 = this._tokenize(e$2, t$2, !1, n$1);
						return {
							tokens: s$1.lineTokens.getResult(s$1.ruleStack, s$1.lineLength),
							ruleStack: s$1.ruleStack,
							stoppedEarly: s$1.stoppedEarly,
							fonts: s$1.lineFonts.getResult()
						};
					}
					tokenizeLine2(e$2, t$2, n$1 = 0) {
						const s$1 = this._tokenize(e$2, t$2, !0, n$1);
						return {
							tokens: s$1.lineTokens.getBinaryResult(s$1.ruleStack, s$1.lineLength),
							ruleStack: s$1.ruleStack,
							stoppedEarly: s$1.stoppedEarly,
							fonts: s$1.lineFonts.getResult()
						};
					}
					_tokenize(e$2, t$2, n$1, s$1) {
						let i$1;
						if (-1 === this._rootId && (this._rootId = a.RuleFactory.getCompiledRuleId(this._grammar.repository.$self, this, this._grammar.repository), this.getInjections()), t$2 && t$2 !== b.NULL) i$1 = !1, t$2.reset();
						else {
							i$1 = !0;
							const e$3 = this._basicScopeAttributesProvider.getDefaultAttributes(), n$2 = this.themeProvider.getDefaults(), s$2 = r.EncodedTokenAttributes.set(0, e$3.languageId, e$3.tokenType, null, n$2.fontStyle, n$2.foregroundId, n$2.backgroundId), o$1 = r.FontAttribute.from(n$2.fontFamily, n$2.fontSize, n$2.lineHeight), a$1 = this.getRule(this._rootId).getName(null, null);
							let c$2;
							c$2 = a$1 ? _.createRootAndLookUpScopeName(a$1, s$2, o$1, this) : _.createRoot("unknown", s$2, o$1), t$2 = new b(null, this._rootId, -1, -1, !1, null, c$2, c$2);
						}
						e$2 += "\n";
						const c$1 = this.createOnigString(e$2), l$1 = c$1.content.length, u$1 = new y(n$1, e$2, this._tokenTypeMatchers, this.balancedBracketSelectors), p$1 = new k(), d$1 = h._tokenizeString(this, c$1, i$1, 0, t$2, u$1, p$1, !0, s$1);
						return o.disposeOnigString(c$1), {
							lineLength: l$1,
							lineTokens: u$1,
							lineFonts: p$1,
							ruleStack: d$1.stack,
							stoppedEarly: d$1.stoppedEarly
						};
					}
				}
				function g(e$2, t$2) {
					return (e$2 = l.clone(e$2)).repository = e$2.repository || {}, e$2.repository.$self = {
						$vscodeTextmateLocation: e$2.$vscodeTextmateLocation,
						patterns: e$2.patterns,
						name: e$2.scopeName
					}, e$2.repository.$base = t$2 || e$2.repository.$self, e$2;
				}
				t$1.Grammar = m;
				class _ {
					constructor(e$2, t$2, n$1, s$1, r$1) {
						this.parent = e$2, this.scopePath = t$2, this.tokenAttributes = n$1, this.fontAttributes = s$1, this.styleAttributes = r$1;
					}
					static fromExtension(e$2, t$2) {
						let n$1 = e$2, s$1 = e$2?.scopePath ?? null;
						for (const e$3 of t$2) s$1 = c.ScopeStack.push(s$1, e$3.scopeNames), n$1 = new _(n$1, s$1, e$3.encodedTokenAttributes, null, null);
						return n$1;
					}
					static createRoot(e$2, t$2, n$1) {
						return new _(null, new c.ScopeStack(null, e$2), t$2, n$1, null);
					}
					static createRootAndLookUpScopeName(e$2, t$2, n$1, s$1) {
						const r$1 = s$1.getMetadataForScope(e$2), i$1 = new c.ScopeStack(null, e$2), o$1 = s$1.themeProvider.themeMatch(i$1);
						return new _(null, i$1, _.mergeAttributes(t$2, r$1, o$1), n$1.with(o$1), o$1);
					}
					get scopeName() {
						return this.scopePath.scopeName;
					}
					toString() {
						return this.getScopeNames().join(" ");
					}
					equals(e$2) {
						return _.equals(this, e$2);
					}
					static equals(e$2, t$2) {
						for (;;) {
							if (e$2 === t$2) return !0;
							if (!e$2 && !t$2) return !0;
							if (!e$2 || !t$2) return !1;
							if (e$2.scopeName !== t$2.scopeName || e$2.tokenAttributes !== t$2.tokenAttributes) return !1;
							e$2 = e$2.parent, t$2 = t$2.parent;
						}
					}
					static mergeAttributes(e$2, t$2, n$1) {
						let s$1 = -1, i$1 = 0, o$1 = 0;
						return null !== n$1 && (s$1 = n$1.fontStyle, i$1 = n$1.foregroundId, o$1 = n$1.backgroundId), r.EncodedTokenAttributes.set(e$2, t$2.languageId, t$2.tokenType, null, s$1, i$1, o$1);
					}
					pushAttributed(e$2, t$2) {
						if (null === e$2) return this;
						if (-1 === e$2.indexOf(" ")) return _._pushAttributed(this, e$2, t$2);
						const n$1 = e$2.split(/ /g);
						let s$1 = this;
						for (const e$3 of n$1) s$1 = _._pushAttributed(s$1, e$3, t$2);
						return s$1;
					}
					static _pushAttributed(e$2, t$2, n$1) {
						const s$1 = n$1.getMetadataForScope(t$2), r$1 = e$2.scopePath.push(t$2), i$1 = n$1.themeProvider.themeMatch(r$1);
						return new _(e$2, r$1, _.mergeAttributes(e$2.tokenAttributes, s$1, i$1), e$2.fontAttributes?.with(i$1) ?? null, i$1);
					}
					getScopeNames() {
						return this.scopePath.getSegments();
					}
					getExtensionIfDefined(e$2) {
						const t$2 = [];
						let n$1 = this;
						for (; n$1 && n$1 !== e$2;) t$2.push({
							encodedTokenAttributes: n$1.tokenAttributes,
							scopeNames: n$1.scopePath.getExtensionIfDefined(n$1.parent?.scopePath ?? null)
						}), n$1 = n$1.parent;
						return n$1 === e$2 ? t$2.reverse() : void 0;
					}
				}
				t$1.AttributedScopeStack = _;
				class b {
					constructor(e$2, t$2, n$1, s$1, r$1, i$1, o$1, a$1) {
						this.parent = e$2, this.ruleId = t$2, this.beginRuleCapturedEOL = r$1, this.endRule = i$1, this.nameScopesList = o$1, this.contentNameScopesList = a$1, this._stackElementBrand = void 0, this.depth = this.parent ? this.parent.depth + 1 : 1, this._enterPos = n$1, this._anchorPos = s$1;
					}
					equals(e$2) {
						return null !== e$2 && b._equals(this, e$2);
					}
					static _equals(e$2, t$2) {
						return e$2 === t$2 || !!this._structuralEquals(e$2, t$2) && _.equals(e$2.contentNameScopesList, t$2.contentNameScopesList);
					}
					static _structuralEquals(e$2, t$2) {
						for (;;) {
							if (e$2 === t$2) return !0;
							if (!e$2 && !t$2) return !0;
							if (!e$2 || !t$2) return !1;
							if (e$2.depth !== t$2.depth || e$2.ruleId !== t$2.ruleId || e$2.endRule !== t$2.endRule) return !1;
							e$2 = e$2.parent, t$2 = t$2.parent;
						}
					}
					clone() {
						return this;
					}
					static _reset(e$2) {
						for (; e$2;) e$2._enterPos = -1, e$2._anchorPos = -1, e$2 = e$2.parent;
					}
					reset() {
						b._reset(this);
					}
					pop() {
						return this.parent;
					}
					safePop() {
						return this.parent ? this.parent : this;
					}
					push(e$2, t$2, n$1, s$1, r$1, i$1, o$1) {
						return new b(this, e$2, t$2, n$1, s$1, r$1, i$1, o$1);
					}
					getEnterPos() {
						return this._enterPos;
					}
					getAnchorPos() {
						return this._anchorPos;
					}
					getRule(e$2) {
						return e$2.getRule(this.ruleId);
					}
					toString() {
						const e$2 = [];
						return this._writeString(e$2, 0), "[" + e$2.join(",") + "]";
					}
					_writeString(e$2, t$2) {
						return this.parent && (t$2 = this.parent._writeString(e$2, t$2)), e$2[t$2++] = `(${this.ruleId}, ${this.nameScopesList?.toString()}, ${this.contentNameScopesList?.toString()})`, t$2;
					}
					withContentNameScopesList(e$2) {
						return this.contentNameScopesList === e$2 ? this : this.parent.push(this.ruleId, this._enterPos, this._anchorPos, this.beginRuleCapturedEOL, this.endRule, this.nameScopesList, e$2);
					}
					withEndRule(e$2) {
						return this.endRule === e$2 ? this : new b(this.parent, this.ruleId, this._enterPos, this._anchorPos, this.beginRuleCapturedEOL, e$2, this.nameScopesList, this.contentNameScopesList);
					}
					hasSameRuleAs(e$2) {
						let t$2 = this;
						for (; t$2 && t$2._enterPos === e$2._enterPos;) {
							if (t$2.ruleId === e$2.ruleId) return !0;
							t$2 = t$2.parent;
						}
						return !1;
					}
					toStateStackFrame() {
						return {
							ruleId: a.ruleIdToNumber(this.ruleId),
							beginRuleCapturedEOL: this.beginRuleCapturedEOL,
							endRule: this.endRule,
							nameScopesList: this.nameScopesList?.getExtensionIfDefined(this.parent?.nameScopesList ?? null) ?? [],
							contentNameScopesList: this.contentNameScopesList?.getExtensionIfDefined(this.nameScopesList) ?? []
						};
					}
					static pushFrame(e$2, t$2) {
						const n$1 = _.fromExtension(e$2?.nameScopesList ?? null, t$2.nameScopesList);
						return new b(e$2, a.ruleIdFromNumber(t$2.ruleId), t$2.enterPos ?? -1, t$2.anchorPos ?? -1, t$2.beginRuleCapturedEOL, t$2.endRule, n$1, _.fromExtension(n$1, t$2.contentNameScopesList));
					}
				}
				t$1.StateStackImpl = b, b.NULL = new b(null, 0, 0, 0, !1, null, null, null), t$1.BalancedBracketSelectors = class {
					constructor(e$2, t$2) {
						this.allowAny = !1, this.balancedBracketScopes = e$2.flatMap(((e$3) => "*" === e$3 ? (this.allowAny = !0, []) : i.createMatchers(e$3, d).map(((e$4) => e$4.matcher)))), this.unbalancedBracketScopes = t$2.flatMap(((e$3) => i.createMatchers(e$3, d).map(((e$4) => e$4.matcher))));
					}
					get matchesAlways() {
						return this.allowAny && 0 === this.unbalancedBracketScopes.length;
					}
					get matchesNever() {
						return 0 === this.balancedBracketScopes.length && !this.allowAny;
					}
					match(e$2) {
						for (const t$2 of this.unbalancedBracketScopes) if (t$2(e$2)) return !1;
						for (const t$2 of this.balancedBracketScopes) if (t$2(e$2)) return !0;
						return this.allowAny;
					}
				};
				class y {
					constructor(e$2, t$2, n$1, r$1) {
						this.balancedBracketSelectors = r$1, this._emitBinaryTokens = e$2, this._tokenTypeOverrides = n$1, s.DebugFlags.InDebugMode ? this._lineText = t$2 : this._lineText = null, this._mergeConsecutiveTokensWithEqualMetadata = !l.containsRTL(t$2), this._tokens = [], this._binaryTokens = [], this._lastTokenEndIndex = 0;
					}
					produce(e$2, t$2) {
						this.produceFromScopes(e$2.contentNameScopesList, t$2);
					}
					produceFromScopes(e$2, t$2) {
						if (this._lastTokenEndIndex >= t$2) return;
						if (this._emitBinaryTokens) {
							let n$2 = e$2?.tokenAttributes ?? 0, i$1 = !1;
							if (this.balancedBracketSelectors?.matchesAlways && (i$1 = !0), this._tokenTypeOverrides.length > 0 || this.balancedBracketSelectors && !this.balancedBracketSelectors.matchesAlways && !this.balancedBracketSelectors.matchesNever) {
								const t$3 = e$2?.getScopeNames() ?? [];
								for (const e$3 of this._tokenTypeOverrides) e$3.matcher(t$3) && (n$2 = r.EncodedTokenAttributes.set(n$2, 0, r.toOptionalTokenType(e$3.type), null, -1, 0, 0));
								this.balancedBracketSelectors && (i$1 = this.balancedBracketSelectors.match(t$3));
							}
							if (i$1 && (n$2 = r.EncodedTokenAttributes.set(n$2, 0, 8, i$1, -1, 0, 0)), this._mergeConsecutiveTokensWithEqualMetadata && this._binaryTokens.length > 0 && this._binaryTokens[this._binaryTokens.length - 1] === n$2) return void (this._lastTokenEndIndex = t$2);
							if (s.DebugFlags.InDebugMode) {
								const n$3 = e$2?.getScopeNames() ?? [];
								console.log("  token: |" + this._lineText.substring(this._lastTokenEndIndex, t$2).replace(/\n$/, "\\n") + "|");
								for (let e$3 = 0; e$3 < n$3.length; e$3++) console.log("      * " + n$3[e$3]);
							}
							this._binaryTokens.push(this._lastTokenEndIndex), this._binaryTokens.push(n$2), this._lastTokenEndIndex = t$2;
							return;
						}
						const n$1 = e$2?.getScopeNames() ?? [];
						if (s.DebugFlags.InDebugMode) {
							console.log("  token: |" + this._lineText.substring(this._lastTokenEndIndex, t$2).replace(/\n$/, "\\n") + "|");
							for (let e$3 = 0; e$3 < n$1.length; e$3++) console.log("      * " + n$1[e$3]);
						}
						this._tokens.push({
							startIndex: this._lastTokenEndIndex,
							endIndex: t$2,
							scopes: n$1
						}), this._lastTokenEndIndex = t$2;
					}
					getResult(e$2, t$2) {
						return this._tokens.length > 0 && this._tokens[this._tokens.length - 1].startIndex === t$2 - 1 && this._tokens.pop(), 0 === this._tokens.length && (this._lastTokenEndIndex = -1, this.produce(e$2, t$2), this._tokens[this._tokens.length - 1].startIndex = 0), this._tokens;
					}
					getBinaryResult(e$2, t$2) {
						this._binaryTokens.length > 0 && this._binaryTokens[this._binaryTokens.length - 2] === t$2 - 1 && (this._binaryTokens.pop(), this._binaryTokens.pop()), 0 === this._binaryTokens.length && (this._lastTokenEndIndex = -1, this.produce(e$2, t$2), this._binaryTokens[this._binaryTokens.length - 2] = 0);
						const n$1 = new Uint32Array(this._binaryTokens.length);
						for (let e$3 = 0, t$3 = this._binaryTokens.length; e$3 < t$3; e$3++) n$1[e$3] = this._binaryTokens[e$3];
						return n$1;
					}
				}
				t$1.LineTokens = y;
				class S {
					constructor(e$2, t$2, n$1, s$1, r$1) {
						this.startIndex = e$2, this.endIndex = t$2, this.fontFamily = n$1, this.fontSizeMultiplier = s$1, this.lineHeightMultiplier = r$1;
					}
					optionsEqual(e$2) {
						return this.fontFamily === e$2.fontFamily && this.fontSizeMultiplier === e$2.fontSizeMultiplier && this.lineHeightMultiplier === e$2.lineHeightMultiplier;
					}
				}
				t$1.FontInfo = S;
				class k {
					constructor() {
						this._fonts = [], this._lastIndex = 0;
					}
					produce(e$2, t$2) {
						this.produceFromScopes(e$2.contentNameScopesList, t$2);
					}
					produceFromScopes(e$2, t$2) {
						if (!e$2?.fontAttributes) return void (this._lastIndex = t$2);
						const n$1 = e$2.fontAttributes.fontFamily, s$1 = e$2.fontAttributes.fontSize, r$1 = e$2.fontAttributes.lineHeight;
						if (!n$1 && !s$1 && !r$1) return void (this._lastIndex = t$2);
						const i$1 = new S(this._lastIndex, t$2, n$1, s$1, r$1), o$1 = this._fonts[this._fonts.length - 1];
						o$1 && o$1.endIndex === this._lastIndex && o$1.optionsEqual(i$1) ? o$1.endIndex = i$1.endIndex : this._fonts.push(i$1), this._lastIndex = t$2;
					}
					getResult() {
						return this._fonts;
					}
				}
				t$1.LineFonts = k;
			},
			784: (e$1, t$1, n) => {
				Object.defineProperty(t$1, "__esModule", { value: !0 }), t$1.parseInclude = t$1.TopLevelRepositoryReference = t$1.TopLevelReference = t$1.RelativeReference = t$1.SelfReference = t$1.BaseReference = t$1.ScopeDependencyProcessor = t$1.ExternalReferenceCollector = t$1.TopLevelRepositoryRuleReference = t$1.TopLevelRuleReference = void 0;
				const s = n(807);
				class r {
					constructor(e$2) {
						this.scopeName = e$2;
					}
					toKey() {
						return this.scopeName;
					}
				}
				t$1.TopLevelRuleReference = r;
				class i {
					constructor(e$2, t$2) {
						this.scopeName = e$2, this.ruleName = t$2;
					}
					toKey() {
						return `${this.scopeName}#${this.ruleName}`;
					}
				}
				t$1.TopLevelRepositoryRuleReference = i;
				class o {
					constructor() {
						this._references = [], this._seenReferenceKeys = /* @__PURE__ */ new Set(), this.visitedRule = /* @__PURE__ */ new Set();
					}
					get references() {
						return this._references;
					}
					add(e$2) {
						const t$2 = e$2.toKey();
						this._seenReferenceKeys.has(t$2) || (this._seenReferenceKeys.add(t$2), this._references.push(e$2));
					}
				}
				function a(e$2, t$2, n$1, s$1) {
					const i$1 = n$1.lookup(e$2.scopeName);
					if (!i$1) {
						if (e$2.scopeName === t$2) throw new Error(`No grammar provided for <${t$2}>`);
						return;
					}
					const o$1 = n$1.lookup(t$2);
					e$2 instanceof r ? l({
						baseGrammar: o$1,
						selfGrammar: i$1
					}, s$1) : c(e$2.ruleName, {
						baseGrammar: o$1,
						selfGrammar: i$1,
						repository: i$1.repository
					}, s$1);
					const a$1 = n$1.injections(e$2.scopeName);
					if (a$1) for (const e$3 of a$1) s$1.add(new r(e$3));
				}
				function c(e$2, t$2, n$1) {
					t$2.repository && t$2.repository[e$2] && u([t$2.repository[e$2]], t$2, n$1);
				}
				function l(e$2, t$2) {
					e$2.selfGrammar.patterns && Array.isArray(e$2.selfGrammar.patterns) && u(e$2.selfGrammar.patterns, {
						...e$2,
						repository: e$2.selfGrammar.repository
					}, t$2), e$2.selfGrammar.injections && u(Object.values(e$2.selfGrammar.injections), {
						...e$2,
						repository: e$2.selfGrammar.repository
					}, t$2);
				}
				function u(e$2, t$2, n$1) {
					for (const o$1 of e$2) {
						if (n$1.visitedRule.has(o$1)) continue;
						n$1.visitedRule.add(o$1);
						const e$3 = o$1.repository ? s.mergeObjects({}, t$2.repository, o$1.repository) : t$2.repository;
						Array.isArray(o$1.patterns) && u(o$1.patterns, {
							...t$2,
							repository: e$3
						}, n$1);
						const a$1 = o$1.include;
						if (!a$1) continue;
						const h$1 = g(a$1);
						switch (h$1.kind) {
							case 0:
								l({
									...t$2,
									selfGrammar: t$2.baseGrammar
								}, n$1);
								break;
							case 1:
								l(t$2, n$1);
								break;
							case 2:
								c(h$1.ruleName, {
									...t$2,
									repository: e$3
								}, n$1);
								break;
							case 3:
							case 4:
								const s$1 = h$1.scopeName === t$2.selfGrammar.scopeName ? t$2.selfGrammar : h$1.scopeName === t$2.baseGrammar.scopeName ? t$2.baseGrammar : void 0;
								if (s$1) {
									const r$1 = {
										baseGrammar: t$2.baseGrammar,
										selfGrammar: s$1,
										repository: e$3
									};
									4 === h$1.kind ? c(h$1.ruleName, r$1, n$1) : l(r$1, n$1);
								} else 4 === h$1.kind ? n$1.add(new i(h$1.scopeName, h$1.ruleName)) : n$1.add(new r(h$1.scopeName));
						}
					}
				}
				t$1.ExternalReferenceCollector = o, t$1.ScopeDependencyProcessor = class {
					constructor(e$2, t$2) {
						this.repo = e$2, this.initialScopeName = t$2, this.seenFullScopeRequests = /* @__PURE__ */ new Set(), this.seenPartialScopeRequests = /* @__PURE__ */ new Set(), this.seenFullScopeRequests.add(this.initialScopeName), this.Q = [new r(this.initialScopeName)];
					}
					processQueue() {
						const e$2 = this.Q;
						this.Q = [];
						const t$2 = new o();
						for (const n$1 of e$2) a(n$1, this.initialScopeName, this.repo, t$2);
						for (const e$3 of t$2.references) if (e$3 instanceof r) {
							if (this.seenFullScopeRequests.has(e$3.scopeName)) continue;
							this.seenFullScopeRequests.add(e$3.scopeName), this.Q.push(e$3);
						} else {
							if (this.seenFullScopeRequests.has(e$3.scopeName)) continue;
							if (this.seenPartialScopeRequests.has(e$3.toKey())) continue;
							this.seenPartialScopeRequests.add(e$3.toKey()), this.Q.push(e$3);
						}
					}
				};
				class h {
					constructor() {
						this.kind = 0;
					}
				}
				t$1.BaseReference = h;
				class p {
					constructor() {
						this.kind = 1;
					}
				}
				t$1.SelfReference = p;
				class d {
					constructor(e$2) {
						this.ruleName = e$2, this.kind = 2;
					}
				}
				t$1.RelativeReference = d;
				class f {
					constructor(e$2) {
						this.scopeName = e$2, this.kind = 3;
					}
				}
				t$1.TopLevelReference = f;
				class m {
					constructor(e$2, t$2) {
						this.scopeName = e$2, this.ruleName = t$2, this.kind = 4;
					}
				}
				function g(e$2) {
					if ("$base" === e$2) return new h();
					if ("$self" === e$2) return new p();
					const t$2 = e$2.indexOf("#");
					if (-1 === t$2) return new f(e$2);
					if (0 === t$2) return new d(e$2.substring(1));
					return new m(e$2.substring(0, t$2), e$2.substring(t$2 + 1));
				}
				t$1.TopLevelRepositoryReference = m, t$1.parseInclude = g;
			},
			752: function(e$1, t$1, n) {
				var s = this && this.__createBinding || (Object.create ? function(e$2, t$2, n$1, s$1) {
					void 0 === s$1 && (s$1 = n$1), Object.defineProperty(e$2, s$1, {
						enumerable: !0,
						get: function() {
							return t$2[n$1];
						}
					});
				} : function(e$2, t$2, n$1, s$1) {
					void 0 === s$1 && (s$1 = n$1), e$2[s$1] = t$2[n$1];
				}), r = this && this.__exportStar || function(e$2, t$2) {
					for (var n$1 in e$2) "default" === n$1 || Object.prototype.hasOwnProperty.call(t$2, n$1) || s(t$2, e$2, n$1);
				};
				Object.defineProperty(t$1, "__esModule", { value: !0 }), r(n(929), t$1);
			},
			398: (e$1, t$1, n) => {
				Object.defineProperty(t$1, "__esModule", { value: !0 }), t$1.LocalStackElement = t$1._tokenizeString = void 0;
				const s = n(185), r = n(810), i = n(666), o = n(807);
				class a {
					constructor(e$2, t$2) {
						this.stack = e$2, this.stoppedEarly = t$2;
					}
				}
				function c(e$2, t$2, n$1, r$1, c$1, h$1, d$1, f, m) {
					const g = (e$3, t$3) => {
						h$1.produce(e$3, t$3), d$1.produce(e$3, t$3);
					}, _ = t$2.content.length;
					let b = !1, y = -1;
					if (f) {
						const o$1 = function(e$3, t$3, n$2, r$2, o$2, a$1, c$2) {
							const l$1 = (e$4, t$4) => {
								a$1.produce(e$4, t$4), c$2.produce(e$4, t$4);
							};
							let h$2 = o$2.beginRuleCapturedEOL ? 0 : -1;
							const d$2 = [];
							for (let t$4 = o$2; t$4; t$4 = t$4.pop()) {
								const n$3 = t$4.getRule(e$3);
								n$3 instanceof i.BeginWhileRule && d$2.push({
									rule: n$3,
									stack: t$4
								});
							}
							for (let f$1 = d$2.pop(); f$1; f$1 = d$2.pop()) {
								const { ruleScanner: d$3, findOptions: m$1 } = u(f$1.rule, e$3, f$1.stack.endRule, n$2, r$2 === h$2), g$1 = d$3.findNextMatchSync(t$3, r$2, m$1);
								if (s.DebugFlags.InDebugMode && (console.log("  scanning for while rule"), console.log(d$3.toString())), !g$1) {
									s.DebugFlags.InDebugMode && console.log("  popping " + f$1.rule.debugName + " - " + f$1.rule.debugWhileRegExp), o$2 = f$1.stack.pop();
									break;
								}
								if (g$1.ruleId !== i.whileRuleId) {
									o$2 = f$1.stack.pop();
									break;
								}
								g$1.captureIndices && g$1.captureIndices.length && (l$1(f$1.stack, g$1.captureIndices[0].start), p(e$3, t$3, n$2, f$1.stack, a$1, c$2, f$1.rule.whileCaptures, g$1.captureIndices), l$1(f$1.stack, g$1.captureIndices[0].end), h$2 = g$1.captureIndices[0].end, g$1.captureIndices[0].end > r$2 && (r$2 = g$1.captureIndices[0].end, n$2 = !1));
							}
							return {
								stack: o$2,
								linePos: r$2,
								anchorPosition: h$2,
								isFirstLine: n$2
							};
						}(e$2, t$2, n$1, r$1, c$1, h$1, d$1);
						c$1 = o$1.stack, r$1 = o$1.linePos, n$1 = o$1.isFirstLine, y = o$1.anchorPosition;
					}
					const S = Date.now();
					for (; !b;) {
						if (0 !== m && Date.now() - S > m) return new a(c$1, !0);
						k();
					}
					return new a(c$1, !1);
					function k() {
						s.DebugFlags.InDebugMode && (console.log(""), console.log(`@@scanNext ${r$1}: |${t$2.content.substr(r$1).replace(/\n$/, "\\n")}|`));
						const a$1 = function(e$3, t$3, n$2, r$2, i$1, a$2) {
							const c$2 = function(e$4, t$4, n$3, r$3, i$2, a$3) {
								const c$3 = i$2.getRule(e$4), { ruleScanner: u$3, findOptions: h$3 } = l(c$3, e$4, i$2.endRule, n$3, r$3 === a$3);
								let p$2 = 0;
								s.DebugFlags.InDebugMode && (p$2 = o.performanceNow());
								const d$3 = u$3.findNextMatchSync(t$4, r$3, h$3);
								if (s.DebugFlags.InDebugMode) {
									const e$5 = o.performanceNow() - p$2;
									e$5 > 5 && console.warn(`Rule ${c$3.debugName} (${c$3.id}) matching took ${e$5} against '${t$4}'`), console.log(`  scanning for (linePos: ${r$3}, anchorPosition: ${a$3})`), console.log(u$3.toString()), d$3 && console.log(`matched rule id: ${d$3.ruleId} from ${d$3.captureIndices[0].start} to ${d$3.captureIndices[0].end}`);
								}
								return d$3 ? {
									captureIndices: d$3.captureIndices,
									matchedRuleId: d$3.ruleId
								} : null;
							}(e$3, t$3, n$2, r$2, i$1, a$2), u$2 = e$3.getInjections();
							if (0 === u$2.length) return c$2;
							const h$2 = function(e$4, t$4, n$3, r$3, i$2, o$1, a$3) {
								let c$3, u$3 = Number.MAX_VALUE, h$3 = null, p$2 = 0;
								const d$3 = o$1.contentNameScopesList.getScopeNames();
								for (let o$2 = 0, f$2 = e$4.length; o$2 < f$2; o$2++) {
									const f$3 = e$4[o$2];
									if (!f$3.matcher(d$3)) continue;
									const { ruleScanner: g$1, findOptions: _$1 } = l(t$4.getRule(f$3.ruleId), t$4, null, r$3, i$2 === a$3), b$1 = g$1.findNextMatchSync(n$3, i$2, _$1);
									if (!b$1) continue;
									s.DebugFlags.InDebugMode && (console.log(`  matched injection: ${f$3.debugSelector}`), console.log(g$1.toString()));
									const y$1 = b$1.captureIndices[0].start;
									if (!(y$1 >= u$3) && (u$3 = y$1, h$3 = b$1.captureIndices, c$3 = b$1.ruleId, p$2 = f$3.priority, u$3 === i$2)) break;
								}
								return h$3 ? {
									priorityMatch: -1 === p$2,
									captureIndices: h$3,
									matchedRuleId: c$3
								} : null;
							}(u$2, e$3, t$3, n$2, r$2, i$1, a$2);
							if (!h$2) return c$2;
							if (!c$2) return h$2;
							const p$1 = c$2.captureIndices[0].start, d$2 = h$2.captureIndices[0].start;
							return d$2 < p$1 || h$2.priorityMatch && d$2 === p$1 ? h$2 : c$2;
						}(e$2, t$2, n$1, r$1, c$1, y);
						if (!a$1) return s.DebugFlags.InDebugMode && console.log("  no more matches."), g(c$1, _), void (b = !0);
						const u$1 = a$1.captureIndices, f$1 = a$1.matchedRuleId, m$1 = !!(u$1 && u$1.length > 0) && u$1[0].end > r$1;
						if (f$1 === i.endRuleId) {
							const i$1 = c$1.getRule(e$2);
							s.DebugFlags.InDebugMode && console.log("  popping " + i$1.debugName + " - " + i$1.debugEndRegExp), g(c$1, u$1[0].start), c$1 = c$1.withContentNameScopesList(c$1.nameScopesList), p(e$2, t$2, n$1, c$1, h$1, d$1, i$1.endCaptures, u$1), g(c$1, u$1[0].end);
							const o$1 = c$1;
							if (c$1 = c$1.parent, y = o$1.getAnchorPos(), !m$1 && o$1.getEnterPos() === r$1) return s.DebugFlags.InDebugMode && console.error("[1] - Grammar is in an endless loop - Grammar pushed & popped a rule without advancing"), g(c$1 = o$1, _), void (b = !0);
						} else {
							const o$1 = e$2.getRule(f$1);
							g(c$1, u$1[0].start);
							const a$2 = c$1, l$1 = o$1.getName(t$2.content, u$1), S$1 = c$1.contentNameScopesList.pushAttributed(l$1, e$2);
							if (c$1 = c$1.push(f$1, r$1, y, u$1[0].end === _, null, S$1, S$1), o$1 instanceof i.BeginEndRule) {
								const r$2 = o$1;
								s.DebugFlags.InDebugMode && console.log("  pushing " + r$2.debugName + " - " + r$2.debugBeginRegExp), p(e$2, t$2, n$1, c$1, h$1, d$1, r$2.beginCaptures, u$1), g(c$1, u$1[0].end), y = u$1[0].end;
								const i$1 = r$2.getContentName(t$2.content, u$1), l$2 = S$1.pushAttributed(i$1, e$2);
								if (c$1 = c$1.withContentNameScopesList(l$2), r$2.endHasBackReferences && (c$1 = c$1.withEndRule(r$2.getEndWithResolvedBackReferences(t$2.content, u$1))), !m$1 && a$2.hasSameRuleAs(c$1)) return s.DebugFlags.InDebugMode && console.error("[2] - Grammar is in an endless loop - Grammar pushed the same rule without advancing"), c$1 = c$1.pop(), g(c$1, _), void (b = !0);
							} else if (o$1 instanceof i.BeginWhileRule) {
								const r$2 = o$1;
								s.DebugFlags.InDebugMode && console.log("  pushing " + r$2.debugName), p(e$2, t$2, n$1, c$1, h$1, d$1, r$2.beginCaptures, u$1), g(c$1, u$1[0].end), y = u$1[0].end;
								const i$1 = r$2.getContentName(t$2.content, u$1), l$2 = S$1.pushAttributed(i$1, e$2);
								if (c$1 = c$1.withContentNameScopesList(l$2), r$2.whileHasBackReferences && (c$1 = c$1.withEndRule(r$2.getWhileWithResolvedBackReferences(t$2.content, u$1))), !m$1 && a$2.hasSameRuleAs(c$1)) return s.DebugFlags.InDebugMode && console.error("[3] - Grammar is in an endless loop - Grammar pushed the same rule without advancing"), c$1 = c$1.pop(), g(c$1, _), void (b = !0);
							} else {
								const r$2 = o$1;
								if (s.DebugFlags.InDebugMode && console.log("  matched " + r$2.debugName + " - " + r$2.debugMatchRegExp), p(e$2, t$2, n$1, c$1, h$1, d$1, r$2.captures, u$1), g(c$1, u$1[0].end), c$1 = c$1.pop(), !m$1) return s.DebugFlags.InDebugMode && console.error("[4] - Grammar is in an endless loop - Grammar is not advancing, nor is it pushing/popping"), c$1 = c$1.safePop(), g(c$1, _), void (b = !0);
							}
						}
						u$1[0].end > r$1 && (r$1 = u$1[0].end, n$1 = !1);
					}
				}
				function l(e$2, t$2, n$1, r$1, i$1) {
					return s.UseOnigurumaFindOptions ? {
						ruleScanner: e$2.compile(t$2, n$1),
						findOptions: h(r$1, i$1)
					} : {
						ruleScanner: e$2.compileAG(t$2, n$1, r$1, i$1),
						findOptions: 0
					};
				}
				function u(e$2, t$2, n$1, r$1, i$1) {
					return s.UseOnigurumaFindOptions ? {
						ruleScanner: e$2.compileWhile(t$2, n$1),
						findOptions: h(r$1, i$1)
					} : {
						ruleScanner: e$2.compileWhileAG(t$2, n$1, r$1, i$1),
						findOptions: 0
					};
				}
				function h(e$2, t$2) {
					let n$1 = 0;
					return e$2 || (n$1 |= 1), t$2 || (n$1 |= 4), n$1;
				}
				function p(e$2, t$2, n$1, s$1, i$1, o$1, a$1, l$1) {
					const u$1 = (e$3, t$3) => {
						i$1.produceFromScopes(e$3, t$3), o$1.produceFromScopes(e$3, t$3);
					}, h$1 = (e$3, t$3) => {
						i$1.produce(e$3, t$3), o$1.produce(e$3, t$3);
					};
					if (0 === a$1.length) return;
					const p$1 = t$2.content, f = Math.min(a$1.length, l$1.length), m = [], g = l$1[0].end;
					for (let t$3 = 0; t$3 < f; t$3++) {
						const f$1 = a$1[t$3];
						if (null === f$1) continue;
						const _ = l$1[t$3];
						if (0 === _.length) continue;
						if (_.start > g) break;
						for (; m.length > 0 && m[m.length - 1].endPos <= _.start;) u$1(m[m.length - 1].scopes, m[m.length - 1].endPos), m.pop();
						if (m.length > 0 ? u$1(m[m.length - 1].scopes, _.start) : h$1(s$1, _.start), f$1.retokenizeCapturedWithRuleId) {
							const t$4 = f$1.getName(p$1, l$1), a$2 = s$1.contentNameScopesList.pushAttributed(t$4, e$2), u$2 = f$1.getContentName(p$1, l$1), h$2 = a$2.pushAttributed(u$2, e$2), d$1 = s$1.push(f$1.retokenizeCapturedWithRuleId, _.start, -1, !1, null, a$2, h$2), m$1 = e$2.createOnigString(p$1.substring(0, _.end));
							c(e$2, m$1, n$1 && 0 === _.start, _.start, d$1, i$1, o$1, !1, 0), r.disposeOnigString(m$1);
							continue;
						}
						const b = f$1.getName(p$1, l$1);
						if (null !== b) {
							const t$4 = (m.length > 0 ? m[m.length - 1].scopes : s$1.contentNameScopesList).pushAttributed(b, e$2);
							m.push(new d(t$4, _.end));
						}
					}
					for (; m.length > 0;) u$1(m[m.length - 1].scopes, m[m.length - 1].endPos), m.pop();
				}
				t$1._tokenizeString = c;
				class d {
					constructor(e$2, t$2) {
						this.scopes = e$2, this.endPos = t$2;
					}
				}
				t$1.LocalStackElement = d;
			},
			726: (e$1, t$1) => {
				function n(e$2, t$2) {
					throw new Error("Near offset " + e$2.pos + ": " + t$2 + " ~~~" + e$2.source.substr(e$2.pos, 50) + "~~~");
				}
				Object.defineProperty(t$1, "__esModule", { value: !0 }), t$1.parseJSON = void 0, t$1.parseJSON = function(e$2, t$2, o) {
					let a = new s(e$2), c = new r(), l = 0, u = null, h = [], p = [];
					function d() {
						h.push(l), p.push(u);
					}
					function f() {
						l = h.pop(), u = p.pop();
					}
					function m(e$3) {
						n(a, e$3);
					}
					for (; i(a, c);) {
						if (0 === l) {
							if (null !== u && m("too many constructs in root"), 3 === c.type) {
								u = {}, o && (u.$vscodeTextmateLocation = c.toLocation(t$2)), d(), l = 1;
								continue;
							}
							if (2 === c.type) {
								u = [], d(), l = 4;
								continue;
							}
							m("unexpected token in root");
						}
						if (2 === l) {
							if (5 === c.type) {
								f();
								continue;
							}
							if (7 === c.type) {
								l = 3;
								continue;
							}
							m("expected , or }");
						}
						if (1 === l || 3 === l) {
							if (1 === l && 5 === c.type) {
								f();
								continue;
							}
							if (1 === c.type) {
								let e$3 = c.value;
								if (i(a, c) && 6 === c.type || m("expected colon"), i(a, c) || m("expected value"), l = 2, 1 === c.type) {
									u[e$3] = c.value;
									continue;
								}
								if (8 === c.type) {
									u[e$3] = null;
									continue;
								}
								if (9 === c.type) {
									u[e$3] = !0;
									continue;
								}
								if (10 === c.type) {
									u[e$3] = !1;
									continue;
								}
								if (11 === c.type) {
									u[e$3] = parseFloat(c.value);
									continue;
								}
								if (2 === c.type) {
									let t$3 = [];
									u[e$3] = t$3, d(), l = 4, u = t$3;
									continue;
								}
								if (3 === c.type) {
									let n$1 = {};
									o && (n$1.$vscodeTextmateLocation = c.toLocation(t$2)), u[e$3] = n$1, d(), l = 1, u = n$1;
									continue;
								}
							}
							m("unexpected token in dict");
						}
						if (5 === l) {
							if (4 === c.type) {
								f();
								continue;
							}
							if (7 === c.type) {
								l = 6;
								continue;
							}
							m("expected , or ]");
						}
						if (4 === l || 6 === l) {
							if (4 === l && 4 === c.type) {
								f();
								continue;
							}
							if (l = 5, 1 === c.type) {
								u.push(c.value);
								continue;
							}
							if (8 === c.type) {
								u.push(null);
								continue;
							}
							if (9 === c.type) {
								u.push(!0);
								continue;
							}
							if (10 === c.type) {
								u.push(!1);
								continue;
							}
							if (11 === c.type) {
								u.push(parseFloat(c.value));
								continue;
							}
							if (2 === c.type) {
								let e$3 = [];
								u.push(e$3), d(), l = 4, u = e$3;
								continue;
							}
							if (3 === c.type) {
								let e$3 = {};
								o && (e$3.$vscodeTextmateLocation = c.toLocation(t$2)), u.push(e$3), d(), l = 1, u = e$3;
								continue;
							}
							m("unexpected token in array");
						}
						m("unknown state");
					}
					return 0 !== p.length && m("unclosed constructs"), u;
				};
				class s {
					constructor(e$2) {
						this.source = e$2, this.pos = 0, this.len = e$2.length, this.line = 1, this.char = 0;
					}
				}
				class r {
					constructor() {
						this.value = null, this.type = 0, this.offset = -1, this.len = -1, this.line = -1, this.char = -1;
					}
					toLocation(e$2) {
						return {
							filename: e$2,
							line: this.line,
							char: this.char
						};
					}
				}
				function i(e$2, t$2) {
					t$2.value = null, t$2.type = 0, t$2.offset = -1, t$2.len = -1, t$2.line = -1, t$2.char = -1;
					let s$1, r$1 = e$2.source, i$1 = e$2.pos, o = e$2.len, a = e$2.line, c = e$2.char;
					for (;;) {
						if (i$1 >= o) return !1;
						if (s$1 = r$1.charCodeAt(i$1), 32 !== s$1 && 9 !== s$1 && 13 !== s$1) {
							if (10 !== s$1) break;
							i$1++, a++, c = 0;
						} else i$1++, c++;
					}
					if (t$2.offset = i$1, t$2.line = a, t$2.char = c, 34 === s$1) {
						for (t$2.type = 1, i$1++, c++;;) {
							if (i$1 >= o) return !1;
							if (s$1 = r$1.charCodeAt(i$1), i$1++, c++, 92 !== s$1) {
								if (34 === s$1) break;
							} else i$1++, c++;
						}
						t$2.value = r$1.substring(t$2.offset + 1, i$1 - 1).replace(/\\u([0-9A-Fa-f]{4})/g, ((e$3, t$3) => String.fromCodePoint(parseInt(t$3, 16)))).replace(/\\(.)/g, ((t$3, s$2) => {
							switch (s$2) {
								case "\"": return "\"";
								case "\\": return "\\";
								case "/": return "/";
								case "b": return "\b";
								case "f": return "\f";
								case "n": return "\n";
								case "r": return "\r";
								case "t": return "	";
								default: n(e$2, "invalid escape sequence");
							}
							throw new Error("unreachable");
						}));
					} else if (91 === s$1) t$2.type = 2, i$1++, c++;
					else if (123 === s$1) t$2.type = 3, i$1++, c++;
					else if (93 === s$1) t$2.type = 4, i$1++, c++;
					else if (125 === s$1) t$2.type = 5, i$1++, c++;
					else if (58 === s$1) t$2.type = 6, i$1++, c++;
					else if (44 === s$1) t$2.type = 7, i$1++, c++;
					else if (110 === s$1) {
						if (t$2.type = 8, i$1++, c++, s$1 = r$1.charCodeAt(i$1), 117 !== s$1) return !1;
						if (i$1++, c++, s$1 = r$1.charCodeAt(i$1), 108 !== s$1) return !1;
						if (i$1++, c++, s$1 = r$1.charCodeAt(i$1), 108 !== s$1) return !1;
						i$1++, c++;
					} else if (116 === s$1) {
						if (t$2.type = 9, i$1++, c++, s$1 = r$1.charCodeAt(i$1), 114 !== s$1) return !1;
						if (i$1++, c++, s$1 = r$1.charCodeAt(i$1), 117 !== s$1) return !1;
						if (i$1++, c++, s$1 = r$1.charCodeAt(i$1), 101 !== s$1) return !1;
						i$1++, c++;
					} else if (102 === s$1) {
						if (t$2.type = 10, i$1++, c++, s$1 = r$1.charCodeAt(i$1), 97 !== s$1) return !1;
						if (i$1++, c++, s$1 = r$1.charCodeAt(i$1), 108 !== s$1) return !1;
						if (i$1++, c++, s$1 = r$1.charCodeAt(i$1), 115 !== s$1) return !1;
						if (i$1++, c++, s$1 = r$1.charCodeAt(i$1), 101 !== s$1) return !1;
						i$1++, c++;
					} else for (t$2.type = 11;;) {
						if (i$1 >= o) return !1;
						if (s$1 = r$1.charCodeAt(i$1), !(46 === s$1 || s$1 >= 48 && s$1 <= 57 || 101 === s$1 || 69 === s$1 || 45 === s$1 || 43 === s$1)) break;
						i$1++, c++;
					}
					return t$2.len = i$1 - t$2.offset, null === t$2.value && (t$2.value = r$1.substr(t$2.offset, t$2.len)), e$2.pos = i$1, e$2.line = a, e$2.char = c, !0;
				}
			},
			625: function(e$1, t$1, n) {
				var s = this && this.__createBinding || (Object.create ? function(e$2, t$2, n$1, s$1) {
					void 0 === s$1 && (s$1 = n$1), Object.defineProperty(e$2, s$1, {
						enumerable: !0,
						get: function() {
							return t$2[n$1];
						}
					});
				} : function(e$2, t$2, n$1, s$1) {
					void 0 === s$1 && (s$1 = n$1), e$2[s$1] = t$2[n$1];
				}), r = this && this.__exportStar || function(e$2, t$2) {
					for (var n$1 in e$2) "default" === n$1 || Object.prototype.hasOwnProperty.call(t$2, n$1) || s(t$2, e$2, n$1);
				};
				Object.defineProperty(t$1, "__esModule", { value: !0 }), t$1.applyStateStackDiff = t$1.diffStateStacksRefEq = t$1.parseRawGrammar = t$1.INITIAL = t$1.Registry = void 0;
				const i = n(752), o = n(150), a = n(583), c = n(63), l = n(784), u = n(151);
				Object.defineProperty(t$1, "applyStateStackDiff", {
					enumerable: !0,
					get: function() {
						return u.applyStateStackDiff;
					}
				}), Object.defineProperty(t$1, "diffStateStacksRefEq", {
					enumerable: !0,
					get: function() {
						return u.diffStateStacksRefEq;
					}
				}), r(n(810), t$1), t$1.Registry = class {
					constructor(e$2) {
						this._options = e$2, this._syncRegistry = new a.SyncRegistry(c.Theme.createFromRawTheme(e$2.theme, e$2.colorMap), e$2.onigLib), this._ensureGrammarCache = /* @__PURE__ */ new Map();
					}
					dispose() {
						this._syncRegistry.dispose();
					}
					setTheme(e$2, t$2) {
						this._syncRegistry.setTheme(c.Theme.createFromRawTheme(e$2, t$2));
					}
					getColorMap() {
						return this._syncRegistry.getColorMap();
					}
					loadGrammarWithEmbeddedLanguages(e$2, t$2, n$1) {
						return this.loadGrammarWithConfiguration(e$2, t$2, { embeddedLanguages: n$1 });
					}
					loadGrammarWithConfiguration(e$2, t$2, n$1) {
						return this._loadGrammar(e$2, t$2, n$1.embeddedLanguages, n$1.tokenTypes, new i.BalancedBracketSelectors(n$1.balancedBracketSelectors || [], n$1.unbalancedBracketSelectors || []));
					}
					loadGrammar(e$2) {
						return this._loadGrammar(e$2, 0, null, null, null);
					}
					async _loadGrammar(e$2, t$2, n$1, s$1, r$1) {
						const i$1 = new l.ScopeDependencyProcessor(this._syncRegistry, e$2);
						for (; i$1.Q.length > 0;) await Promise.all(i$1.Q.map(((e$3) => this._loadSingleGrammar(e$3.scopeName)))), i$1.processQueue();
						return this._grammarForScopeName(e$2, t$2, n$1, s$1, r$1);
					}
					async _loadSingleGrammar(e$2) {
						return this._ensureGrammarCache.has(e$2) || this._ensureGrammarCache.set(e$2, this._doLoadSingleGrammar(e$2)), this._ensureGrammarCache.get(e$2);
					}
					async _doLoadSingleGrammar(e$2) {
						const t$2 = await this._options.loadGrammar(e$2);
						if (t$2) {
							const n$1 = "function" == typeof this._options.getInjections ? this._options.getInjections(e$2) : void 0;
							this._syncRegistry.addGrammar(t$2, n$1);
						}
					}
					async addGrammar(e$2, t$2 = [], n$1 = 0, s$1 = null) {
						return this._syncRegistry.addGrammar(e$2, t$2), await this._grammarForScopeName(e$2.scopeName, n$1, s$1);
					}
					_grammarForScopeName(e$2, t$2 = 0, n$1 = null, s$1 = null, r$1 = null) {
						return this._syncRegistry.grammarForScopeName(e$2, t$2, n$1, s$1, r$1);
					}
				}, t$1.INITIAL = i.StateStackImpl.NULL, t$1.parseRawGrammar = o.parseRawGrammar;
			},
			916: (e$1, t$1) => {
				function n(e$2) {
					return !!e$2 && !!e$2.match(/[\w\.:]+/);
				}
				Object.defineProperty(t$1, "__esModule", { value: !0 }), t$1.createMatchers = void 0, t$1.createMatchers = function(e$2, t$2) {
					const s = [], r = function(e$3) {
						let t$3 = /([LR]:|[\w\.:][\w\.:\-]*|[\,\|\-\(\)])/g, n$1 = t$3.exec(e$3);
						return { next: () => {
							if (!n$1) return null;
							const s$1 = n$1[0];
							return n$1 = t$3.exec(e$3), s$1;
						} };
					}(e$2);
					let i = r.next();
					for (; null !== i;) {
						let e$3 = 0;
						if (2 === i.length && ":" === i.charAt(1)) {
							switch (i.charAt(0)) {
								case "R":
									e$3 = 1;
									break;
								case "L":
									e$3 = -1;
									break;
								default: console.log(`Unknown priority ${i} in scope selector`);
							}
							i = r.next();
						}
						let t$3 = a();
						if (s.push({
							matcher: t$3,
							priority: e$3
						}), "," !== i) break;
						i = r.next();
					}
					return s;
					function o() {
						if ("-" === i) {
							i = r.next();
							const e$3 = o();
							return (t$3) => !!e$3 && !e$3(t$3);
						}
						if ("(" === i) {
							i = r.next();
							const e$3 = function() {
								const e$4 = [];
								let t$3 = a();
								for (; t$3 && (e$4.push(t$3), "|" === i || "," === i);) {
									do
										i = r.next();
									while ("|" === i || "," === i);
									t$3 = a();
								}
								return (t$4) => e$4.some(((e$5) => e$5(t$4)));
							}();
							return ")" === i && (i = r.next()), e$3;
						}
						if (n(i)) {
							const e$3 = [];
							do
								e$3.push(i), i = r.next();
							while (n(i));
							return (n$1) => t$2(e$3, n$1);
						}
						return null;
					}
					function a() {
						const e$3 = [];
						let t$3 = o();
						for (; t$3;) e$3.push(t$3), t$3 = o();
						return (t$4) => e$3.every(((e$4) => e$4(t$4)));
					}
				};
			},
			810: (e$1, t$1) => {
				Object.defineProperty(t$1, "__esModule", { value: !0 }), t$1.disposeOnigString = void 0, t$1.disposeOnigString = function(e$2) {
					"function" == typeof e$2.dispose && e$2.dispose();
				};
			},
			150: (e$1, t$1, n) => {
				Object.defineProperty(t$1, "__esModule", { value: !0 }), t$1.parseRawGrammar = void 0;
				const s = n(578), r = n(185), i = n(726);
				t$1.parseRawGrammar = function(e$2, t$2 = null) {
					return null !== t$2 && /\.json$/.test(t$2) ? (n$1 = e$2, o = t$2, r.DebugFlags.InDebugMode ? i.parseJSON(n$1, o, !0) : JSON.parse(n$1)) : function(e$3, t$3) {
						return r.DebugFlags.InDebugMode ? s.parseWithLocation(e$3, t$3, "$vscodeTextmateLocation") : s.parsePLIST(e$3);
					}(e$2, t$2);
					var n$1, o;
				};
			},
			578: (e$1, t$1) => {
				function n(e$2, t$2, n$1) {
					const s = e$2.length;
					let r = 0, i = 1, o = 0;
					function a(t$3) {
						if (null === n$1) r += t$3;
						else for (; t$3 > 0;) 10 === e$2.charCodeAt(r) ? (r++, i++, o = 0) : (r++, o++), t$3--;
					}
					function c(e$3) {
						null === n$1 ? r = e$3 : a(e$3 - r);
					}
					function l() {
						for (; r < s;) {
							let t$3 = e$2.charCodeAt(r);
							if (32 !== t$3 && 9 !== t$3 && 13 !== t$3 && 10 !== t$3) break;
							a(1);
						}
					}
					function u(t$3) {
						return e$2.substr(r, t$3.length) === t$3 && (a(t$3.length), !0);
					}
					function h(t$3) {
						let n$2 = e$2.indexOf(t$3, r);
						c(-1 !== n$2 ? n$2 + t$3.length : s);
					}
					function p(t$3) {
						let n$2 = e$2.indexOf(t$3, r);
						if (-1 !== n$2) {
							let s$1 = e$2.substring(r, n$2);
							return c(n$2 + t$3.length), s$1;
						}
						{
							let t$4 = e$2.substr(r);
							return c(s), t$4;
						}
					}
					s > 0 && 65279 === e$2.charCodeAt(0) && (r = 1);
					let d = 0, f = null, m = [], g = [], _ = null;
					function b(e$3, t$3) {
						m.push(d), g.push(f), d = e$3, f = t$3;
					}
					function y() {
						if (0 === m.length) return S("illegal state stack");
						d = m.pop(), f = g.pop();
					}
					function S(t$3) {
						throw new Error("Near offset " + r + ": " + t$3 + " ~~~" + e$2.substr(r, 50) + "~~~");
					}
					const k = function() {
						if (null === _) return S("missing <key>");
						let e$3 = {};
						null !== n$1 && (e$3[n$1] = {
							filename: t$2,
							line: i,
							char: o
						}), f[_] = e$3, _ = null, b(1, e$3);
					}, C = function() {
						if (null === _) return S("missing <key>");
						let e$3 = [];
						f[_] = e$3, _ = null, b(2, e$3);
					}, R = function() {
						let e$3 = {};
						null !== n$1 && (e$3[n$1] = {
							filename: t$2,
							line: i,
							char: o
						}), f.push(e$3), b(1, e$3);
					}, A = function() {
						let e$3 = [];
						f.push(e$3), b(2, e$3);
					};
					function w() {
						if (1 !== d) return S("unexpected </dict>");
						y();
					}
					function I() {
						return 1 === d || 2 !== d ? S("unexpected </array>") : void y();
					}
					function P(e$3) {
						if (1 === d) {
							if (null === _) return S("missing <key>");
							f[_] = e$3, _ = null;
						} else 2 === d ? f.push(e$3) : f = e$3;
					}
					function v(e$3) {
						if (isNaN(e$3)) return S("cannot parse float");
						if (1 === d) {
							if (null === _) return S("missing <key>");
							f[_] = e$3, _ = null;
						} else 2 === d ? f.push(e$3) : f = e$3;
					}
					function x(e$3) {
						if (isNaN(e$3)) return S("cannot parse integer");
						if (1 === d) {
							if (null === _) return S("missing <key>");
							f[_] = e$3, _ = null;
						} else 2 === d ? f.push(e$3) : f = e$3;
					}
					function N(e$3) {
						if (1 === d) {
							if (null === _) return S("missing <key>");
							f[_] = e$3, _ = null;
						} else 2 === d ? f.push(e$3) : f = e$3;
					}
					function E(e$3) {
						if (1 === d) {
							if (null === _) return S("missing <key>");
							f[_] = e$3, _ = null;
						} else 2 === d ? f.push(e$3) : f = e$3;
					}
					function F(e$3) {
						if (1 === d) {
							if (null === _) return S("missing <key>");
							f[_] = e$3, _ = null;
						} else 2 === d ? f.push(e$3) : f = e$3;
					}
					function T() {
						let e$3 = p(">"), t$3 = !1;
						return 47 === e$3.charCodeAt(e$3.length - 1) && (t$3 = !0, e$3 = e$3.substring(0, e$3.length - 1)), {
							name: e$3.trim(),
							isClosed: t$3
						};
					}
					function D(e$3) {
						if (e$3.isClosed) return "";
						let t$3 = p("</");
						return h(">"), t$3.replace(/&#([0-9]+);/g, (function(e$4, t$4) {
							return String.fromCodePoint(parseInt(t$4, 10));
						})).replace(/&#x([0-9a-f]+);/g, (function(e$4, t$4) {
							return String.fromCodePoint(parseInt(t$4, 16));
						})).replace(/&amp;|&lt;|&gt;|&quot;|&apos;/g, (function(e$4) {
							switch (e$4) {
								case "&amp;": return "&";
								case "&lt;": return "<";
								case "&gt;": return ">";
								case "&quot;": return "\"";
								case "&apos;": return "'";
							}
							return e$4;
						}));
					}
					for (; r < s && (l(), !(r >= s));) {
						const c$1 = e$2.charCodeAt(r);
						if (a(1), 60 !== c$1) return S("expected <");
						if (r >= s) return S("unexpected end of input");
						const p$1 = e$2.charCodeAt(r);
						if (63 === p$1) {
							a(1), h("?>");
							continue;
						}
						if (33 === p$1) {
							if (a(1), u("--")) {
								h("-->");
								continue;
							}
							h(">");
							continue;
						}
						if (47 === p$1) {
							if (a(1), l(), u("plist")) {
								h(">");
								continue;
							}
							if (u("dict")) {
								h(">"), w();
								continue;
							}
							if (u("array")) {
								h(">"), I();
								continue;
							}
							return S("unexpected closed tag");
						}
						let m$1 = T();
						switch (m$1.name) {
							case "dict":
								1 === d ? k() : 2 === d ? R() : (f = {}, null !== n$1 && (f[n$1] = {
									filename: t$2,
									line: i,
									char: o
								}), b(1, f)), m$1.isClosed && w();
								continue;
							case "array":
								1 === d ? C() : 2 === d ? A() : (f = [], b(2, f)), m$1.isClosed && I();
								continue;
							case "key":
								G = D(m$1), 1 !== d ? S("unexpected <key>") : null !== _ ? S("too many <key>") : _ = G;
								continue;
							case "string":
								P(D(m$1));
								continue;
							case "real":
								v(parseFloat(D(m$1)));
								continue;
							case "integer":
								x(parseInt(D(m$1), 10));
								continue;
							case "date":
								N(new Date(D(m$1)));
								continue;
							case "data":
								E(D(m$1));
								continue;
							case "true":
								D(m$1), F(!0);
								continue;
							case "false":
								D(m$1), F(!1);
								continue;
						}
						if (!/^plist/.test(m$1.name)) return S("unexpected opened tag " + m$1.name);
					}
					var G;
					return f;
				}
				Object.defineProperty(t$1, "__esModule", { value: !0 }), t$1.parsePLIST = t$1.parseWithLocation = void 0, t$1.parseWithLocation = function(e$2, t$2, s) {
					return n(e$2, t$2, s);
				}, t$1.parsePLIST = function(e$2) {
					return n(e$2, null, null);
				};
			},
			583: (e$1, t$1, n) => {
				Object.defineProperty(t$1, "__esModule", { value: !0 }), t$1.SyncRegistry = void 0;
				const s = n(752);
				t$1.SyncRegistry = class {
					constructor(e$2, t$2) {
						this._onigLibPromise = t$2, this._grammars = /* @__PURE__ */ new Map(), this._rawGrammars = /* @__PURE__ */ new Map(), this._injectionGrammars = /* @__PURE__ */ new Map(), this._theme = e$2;
					}
					dispose() {
						for (const e$2 of this._grammars.values()) e$2.dispose();
					}
					setTheme(e$2) {
						this._theme = e$2;
					}
					getColorMap() {
						return this._theme.getColorMap();
					}
					addGrammar(e$2, t$2) {
						this._rawGrammars.set(e$2.scopeName, e$2), t$2 && this._injectionGrammars.set(e$2.scopeName, t$2);
					}
					lookup(e$2) {
						return this._rawGrammars.get(e$2);
					}
					injections(e$2) {
						return this._injectionGrammars.get(e$2);
					}
					getDefaults() {
						return this._theme.getDefaults();
					}
					themeMatch(e$2) {
						return this._theme.match(e$2);
					}
					async grammarForScopeName(e$2, t$2, n$1, r, i) {
						if (!this._grammars.has(e$2)) {
							let o = this._rawGrammars.get(e$2);
							if (!o) return null;
							this._grammars.set(e$2, s.createGrammar(e$2, o, t$2, n$1, r, i, this, await this._onigLibPromise));
						}
						return this._grammars.get(e$2);
					}
				};
			},
			666: (e$1, t$1, n) => {
				Object.defineProperty(t$1, "__esModule", { value: !0 }), t$1.CompiledRule = t$1.RegExpSourceList = t$1.RegExpSource = t$1.RuleFactory = t$1.BeginWhileRule = t$1.BeginEndRule = t$1.IncludeOnlyRule = t$1.MatchRule = t$1.CaptureRule = t$1.Rule = t$1.ruleIdToNumber = t$1.ruleIdFromNumber = t$1.whileRuleId = t$1.endRuleId = void 0;
				const s = n(807), r = n(784), i = /\\(\d+)/, o = /\\(\d+)/g;
				t$1.endRuleId = -1, t$1.whileRuleId = -2, t$1.ruleIdFromNumber = function(e$2) {
					return e$2;
				}, t$1.ruleIdToNumber = function(e$2) {
					return e$2;
				};
				class a {
					constructor(e$2, t$2, n$1, r$1) {
						this.$location = e$2, this.id = t$2, this._name = n$1 || null, this._nameIsCapturing = s.RegexSource.hasCaptures(this._name), this._contentName = r$1 || null, this._contentNameIsCapturing = s.RegexSource.hasCaptures(this._contentName);
					}
					get debugName() {
						const e$2 = this.$location ? `${s.basename(this.$location.filename)}:${this.$location.line}` : "unknown";
						return `${this.constructor.name}#${this.id} @ ${e$2}`;
					}
					getName(e$2, t$2) {
						return this._nameIsCapturing && null !== this._name && null !== e$2 && null !== t$2 ? s.RegexSource.replaceCaptures(this._name, e$2, t$2) : this._name;
					}
					getContentName(e$2, t$2) {
						return this._contentNameIsCapturing && null !== this._contentName ? s.RegexSource.replaceCaptures(this._contentName, e$2, t$2) : this._contentName;
					}
				}
				t$1.Rule = a;
				class c extends a {
					constructor(e$2, t$2, n$1, s$1, r$1) {
						super(e$2, t$2, n$1, s$1), this.retokenizeCapturedWithRuleId = r$1;
					}
					dispose() {}
					collectPatterns(e$2, t$2) {
						throw new Error("Not supported!");
					}
					compile(e$2, t$2) {
						throw new Error("Not supported!");
					}
					compileAG(e$2, t$2, n$1, s$1) {
						throw new Error("Not supported!");
					}
				}
				t$1.CaptureRule = c;
				class l extends a {
					constructor(e$2, t$2, n$1, s$1, r$1) {
						super(e$2, t$2, n$1, null), this._match = new f(s$1, this.id), this.captures = r$1, this._cachedCompiledPatterns = null;
					}
					dispose() {
						this._cachedCompiledPatterns && (this._cachedCompiledPatterns.dispose(), this._cachedCompiledPatterns = null);
					}
					get debugMatchRegExp() {
						return `${this._match.source}`;
					}
					collectPatterns(e$2, t$2) {
						t$2.push(this._match);
					}
					compile(e$2, t$2) {
						return this._getCachedCompiledPatterns(e$2).compile(e$2);
					}
					compileAG(e$2, t$2, n$1, s$1) {
						return this._getCachedCompiledPatterns(e$2).compileAG(e$2, n$1, s$1);
					}
					_getCachedCompiledPatterns(e$2) {
						return this._cachedCompiledPatterns || (this._cachedCompiledPatterns = new m(), this.collectPatterns(e$2, this._cachedCompiledPatterns)), this._cachedCompiledPatterns;
					}
				}
				t$1.MatchRule = l;
				class u extends a {
					constructor(e$2, t$2, n$1, s$1, r$1) {
						super(e$2, t$2, n$1, s$1), this.patterns = r$1.patterns, this.hasMissingPatterns = r$1.hasMissingPatterns, this._cachedCompiledPatterns = null;
					}
					dispose() {
						this._cachedCompiledPatterns && (this._cachedCompiledPatterns.dispose(), this._cachedCompiledPatterns = null);
					}
					collectPatterns(e$2, t$2) {
						for (const n$1 of this.patterns) e$2.getRule(n$1).collectPatterns(e$2, t$2);
					}
					compile(e$2, t$2) {
						return this._getCachedCompiledPatterns(e$2).compile(e$2);
					}
					compileAG(e$2, t$2, n$1, s$1) {
						return this._getCachedCompiledPatterns(e$2).compileAG(e$2, n$1, s$1);
					}
					_getCachedCompiledPatterns(e$2) {
						return this._cachedCompiledPatterns || (this._cachedCompiledPatterns = new m(), this.collectPatterns(e$2, this._cachedCompiledPatterns)), this._cachedCompiledPatterns;
					}
				}
				t$1.IncludeOnlyRule = u;
				class h extends a {
					constructor(e$2, t$2, n$1, s$1, r$1, i$1, o$1, a$1, c$1, l$1) {
						super(e$2, t$2, n$1, s$1), this._begin = new f(r$1, this.id), this.beginCaptures = i$1, this._end = new f(o$1 || "￿", -1), this.endHasBackReferences = this._end.hasBackReferences, this.endCaptures = a$1, this.applyEndPatternLast = c$1 || !1, this.patterns = l$1.patterns, this.hasMissingPatterns = l$1.hasMissingPatterns, this._cachedCompiledPatterns = null;
					}
					dispose() {
						this._cachedCompiledPatterns && (this._cachedCompiledPatterns.dispose(), this._cachedCompiledPatterns = null);
					}
					get debugBeginRegExp() {
						return `${this._begin.source}`;
					}
					get debugEndRegExp() {
						return `${this._end.source}`;
					}
					getEndWithResolvedBackReferences(e$2, t$2) {
						return this._end.resolveBackReferences(e$2, t$2);
					}
					collectPatterns(e$2, t$2) {
						t$2.push(this._begin);
					}
					compile(e$2, t$2) {
						return this._getCachedCompiledPatterns(e$2, t$2).compile(e$2);
					}
					compileAG(e$2, t$2, n$1, s$1) {
						return this._getCachedCompiledPatterns(e$2, t$2).compileAG(e$2, n$1, s$1);
					}
					_getCachedCompiledPatterns(e$2, t$2) {
						if (!this._cachedCompiledPatterns) {
							this._cachedCompiledPatterns = new m();
							for (const t$3 of this.patterns) e$2.getRule(t$3).collectPatterns(e$2, this._cachedCompiledPatterns);
							this.applyEndPatternLast ? this._cachedCompiledPatterns.push(this._end.hasBackReferences ? this._end.clone() : this._end) : this._cachedCompiledPatterns.unshift(this._end.hasBackReferences ? this._end.clone() : this._end);
						}
						return this._end.hasBackReferences && (this.applyEndPatternLast ? this._cachedCompiledPatterns.setSource(this._cachedCompiledPatterns.length() - 1, t$2) : this._cachedCompiledPatterns.setSource(0, t$2)), this._cachedCompiledPatterns;
					}
				}
				t$1.BeginEndRule = h;
				class p extends a {
					constructor(e$2, n$1, s$1, r$1, i$1, o$1, a$1, c$1, l$1) {
						super(e$2, n$1, s$1, r$1), this._begin = new f(i$1, this.id), this.beginCaptures = o$1, this.whileCaptures = c$1, this._while = new f(a$1, t$1.whileRuleId), this.whileHasBackReferences = this._while.hasBackReferences, this.patterns = l$1.patterns, this.hasMissingPatterns = l$1.hasMissingPatterns, this._cachedCompiledPatterns = null, this._cachedCompiledWhilePatterns = null;
					}
					dispose() {
						this._cachedCompiledPatterns && (this._cachedCompiledPatterns.dispose(), this._cachedCompiledPatterns = null), this._cachedCompiledWhilePatterns && (this._cachedCompiledWhilePatterns.dispose(), this._cachedCompiledWhilePatterns = null);
					}
					get debugBeginRegExp() {
						return `${this._begin.source}`;
					}
					get debugWhileRegExp() {
						return `${this._while.source}`;
					}
					getWhileWithResolvedBackReferences(e$2, t$2) {
						return this._while.resolveBackReferences(e$2, t$2);
					}
					collectPatterns(e$2, t$2) {
						t$2.push(this._begin);
					}
					compile(e$2, t$2) {
						return this._getCachedCompiledPatterns(e$2).compile(e$2);
					}
					compileAG(e$2, t$2, n$1, s$1) {
						return this._getCachedCompiledPatterns(e$2).compileAG(e$2, n$1, s$1);
					}
					_getCachedCompiledPatterns(e$2) {
						if (!this._cachedCompiledPatterns) {
							this._cachedCompiledPatterns = new m();
							for (const t$2 of this.patterns) e$2.getRule(t$2).collectPatterns(e$2, this._cachedCompiledPatterns);
						}
						return this._cachedCompiledPatterns;
					}
					compileWhile(e$2, t$2) {
						return this._getCachedCompiledWhilePatterns(e$2, t$2).compile(e$2);
					}
					compileWhileAG(e$2, t$2, n$1, s$1) {
						return this._getCachedCompiledWhilePatterns(e$2, t$2).compileAG(e$2, n$1, s$1);
					}
					_getCachedCompiledWhilePatterns(e$2, t$2) {
						return this._cachedCompiledWhilePatterns || (this._cachedCompiledWhilePatterns = new m(), this._cachedCompiledWhilePatterns.push(this._while.hasBackReferences ? this._while.clone() : this._while)), this._while.hasBackReferences && this._cachedCompiledWhilePatterns.setSource(0, t$2 || "￿"), this._cachedCompiledWhilePatterns;
					}
				}
				t$1.BeginWhileRule = p;
				class d {
					static createCaptureRule(e$2, t$2, n$1, s$1, r$1) {
						return e$2.registerRule(((e$3) => new c(t$2, e$3, n$1, s$1, r$1)));
					}
					static getCompiledRuleId(e$2, t$2, n$1) {
						return e$2.id || t$2.registerRule(((r$1) => {
							if (e$2.id = r$1, e$2.match) return new l(e$2.$vscodeTextmateLocation, e$2.id, e$2.name, e$2.match, d._compileCaptures(e$2.captures, t$2, n$1));
							if (void 0 === e$2.begin) {
								e$2.repository && (n$1 = s.mergeObjects({}, n$1, e$2.repository));
								let r$2 = e$2.patterns;
								return void 0 === r$2 && e$2.include && (r$2 = [{ include: e$2.include }]), new u(e$2.$vscodeTextmateLocation, e$2.id, e$2.name, e$2.contentName, d._compilePatterns(r$2, t$2, n$1));
							}
							return e$2.while ? new p(e$2.$vscodeTextmateLocation, e$2.id, e$2.name, e$2.contentName, e$2.begin, d._compileCaptures(e$2.beginCaptures || e$2.captures, t$2, n$1), e$2.while, d._compileCaptures(e$2.whileCaptures || e$2.captures, t$2, n$1), d._compilePatterns(e$2.patterns, t$2, n$1)) : new h(e$2.$vscodeTextmateLocation, e$2.id, e$2.name, e$2.contentName, e$2.begin, d._compileCaptures(e$2.beginCaptures || e$2.captures, t$2, n$1), e$2.end, d._compileCaptures(e$2.endCaptures || e$2.captures, t$2, n$1), e$2.applyEndPatternLast, d._compilePatterns(e$2.patterns, t$2, n$1));
						})), e$2.id;
					}
					static _compileCaptures(e$2, t$2, n$1) {
						let s$1 = [];
						if (e$2) {
							let r$1 = 0;
							for (const t$3 in e$2) {
								if ("$vscodeTextmateLocation" === t$3) continue;
								const e$3 = parseInt(t$3, 10);
								e$3 > r$1 && (r$1 = e$3);
							}
							for (let e$3 = 0; e$3 <= r$1; e$3++) s$1[e$3] = null;
							for (const r$2 in e$2) {
								if ("$vscodeTextmateLocation" === r$2) continue;
								const i$1 = parseInt(r$2, 10);
								let o$1 = 0;
								e$2[r$2].patterns && (o$1 = d.getCompiledRuleId(e$2[r$2], t$2, n$1)), s$1[i$1] = d.createCaptureRule(t$2, e$2[r$2].$vscodeTextmateLocation, e$2[r$2].name, e$2[r$2].contentName, o$1);
							}
						}
						return s$1;
					}
					static _compilePatterns(e$2, t$2, n$1) {
						let s$1 = [];
						if (e$2) for (let i$1 = 0, o$1 = e$2.length; i$1 < o$1; i$1++) {
							const o$2 = e$2[i$1];
							let a$1 = -1;
							if (o$2.include) {
								const e$3 = r.parseInclude(o$2.include);
								switch (e$3.kind) {
									case 0:
									case 1:
										a$1 = d.getCompiledRuleId(n$1[o$2.include], t$2, n$1);
										break;
									case 2:
										let s$2 = n$1[e$3.ruleName];
										s$2 && (a$1 = d.getCompiledRuleId(s$2, t$2, n$1));
										break;
									case 3:
									case 4:
										const r$1 = e$3.scopeName, i$2 = 4 === e$3.kind ? e$3.ruleName : null, c$1 = t$2.getExternalGrammar(r$1, n$1);
										if (c$1) if (i$2) {
											let e$4 = c$1.repository[i$2];
											e$4 && (a$1 = d.getCompiledRuleId(e$4, t$2, c$1.repository));
										} else a$1 = d.getCompiledRuleId(c$1.repository.$self, t$2, c$1.repository);
								}
							} else a$1 = d.getCompiledRuleId(o$2, t$2, n$1);
							if (-1 !== a$1) {
								const e$3 = t$2.getRule(a$1);
								let n$2 = !1;
								if ((e$3 instanceof u || e$3 instanceof h || e$3 instanceof p) && e$3.hasMissingPatterns && 0 === e$3.patterns.length && (n$2 = !0), n$2) continue;
								s$1.push(a$1);
							}
						}
						return {
							patterns: s$1,
							hasMissingPatterns: (e$2 ? e$2.length : 0) !== s$1.length
						};
					}
				}
				t$1.RuleFactory = d;
				class f {
					constructor(e$2, t$2) {
						if (e$2) {
							const t$3 = e$2.length;
							let n$1 = 0, s$1 = [], r$1 = !1;
							for (let i$1 = 0; i$1 < t$3; i$1++) if ("\\" === e$2.charAt(i$1) && i$1 + 1 < t$3) {
								const t$4 = e$2.charAt(i$1 + 1);
								"z" === t$4 ? (s$1.push(e$2.substring(n$1, i$1)), s$1.push("$(?!\\n)(?<!\\n)"), n$1 = i$1 + 2) : "A" !== t$4 && "G" !== t$4 || (r$1 = !0), i$1++;
							}
							this.hasAnchor = r$1, 0 === n$1 ? this.source = e$2 : (s$1.push(e$2.substring(n$1, t$3)), this.source = s$1.join(""));
						} else this.hasAnchor = !1, this.source = e$2;
						this.hasAnchor ? this._anchorCache = this._buildAnchorCache() : this._anchorCache = null, this.ruleId = t$2, this.hasBackReferences = i.test(this.source);
					}
					clone() {
						return new f(this.source, this.ruleId);
					}
					setSource(e$2) {
						this.source !== e$2 && (this.source = e$2, this.hasAnchor && (this._anchorCache = this._buildAnchorCache()));
					}
					resolveBackReferences(e$2, t$2) {
						let n$1 = t$2.map(((t$3) => e$2.substring(t$3.start, t$3.end)));
						return o.lastIndex = 0, this.source.replace(o, ((e$3, t$3) => s.escapeRegExpCharacters(n$1[parseInt(t$3, 10)] || "")));
					}
					_buildAnchorCache() {
						let e$2, t$2, n$1, s$1, r$1 = [], i$1 = [], o$1 = [], a$1 = [];
						for (e$2 = 0, t$2 = this.source.length; e$2 < t$2; e$2++) n$1 = this.source.charAt(e$2), r$1[e$2] = n$1, i$1[e$2] = n$1, o$1[e$2] = n$1, a$1[e$2] = n$1, "\\" === n$1 && e$2 + 1 < t$2 && (s$1 = this.source.charAt(e$2 + 1), "A" === s$1 ? (r$1[e$2 + 1] = "￿", i$1[e$2 + 1] = "￿", o$1[e$2 + 1] = "A", a$1[e$2 + 1] = "A") : "G" === s$1 ? (r$1[e$2 + 1] = "￿", i$1[e$2 + 1] = "G", o$1[e$2 + 1] = "￿", a$1[e$2 + 1] = "G") : (r$1[e$2 + 1] = s$1, i$1[e$2 + 1] = s$1, o$1[e$2 + 1] = s$1, a$1[e$2 + 1] = s$1), e$2++);
						return {
							A0_G0: r$1.join(""),
							A0_G1: i$1.join(""),
							A1_G0: o$1.join(""),
							A1_G1: a$1.join("")
						};
					}
					resolveAnchors(e$2, t$2) {
						return this.hasAnchor && this._anchorCache ? e$2 ? t$2 ? this._anchorCache.A1_G1 : this._anchorCache.A1_G0 : t$2 ? this._anchorCache.A0_G1 : this._anchorCache.A0_G0 : this.source;
					}
				}
				t$1.RegExpSource = f;
				class m {
					constructor() {
						this._items = [], this._hasAnchors = !1, this._cached = null, this._anchorCache = {
							A0_G0: null,
							A0_G1: null,
							A1_G0: null,
							A1_G1: null
						};
					}
					dispose() {
						this._disposeCaches();
					}
					_disposeCaches() {
						this._cached && (this._cached.dispose(), this._cached = null), this._anchorCache.A0_G0 && (this._anchorCache.A0_G0.dispose(), this._anchorCache.A0_G0 = null), this._anchorCache.A0_G1 && (this._anchorCache.A0_G1.dispose(), this._anchorCache.A0_G1 = null), this._anchorCache.A1_G0 && (this._anchorCache.A1_G0.dispose(), this._anchorCache.A1_G0 = null), this._anchorCache.A1_G1 && (this._anchorCache.A1_G1.dispose(), this._anchorCache.A1_G1 = null);
					}
					push(e$2) {
						this._items.push(e$2), this._hasAnchors = this._hasAnchors || e$2.hasAnchor;
					}
					unshift(e$2) {
						this._items.unshift(e$2), this._hasAnchors = this._hasAnchors || e$2.hasAnchor;
					}
					length() {
						return this._items.length;
					}
					setSource(e$2, t$2) {
						this._items[e$2].source !== t$2 && (this._disposeCaches(), this._items[e$2].setSource(t$2));
					}
					compile(e$2) {
						if (!this._cached) this._cached = new g(e$2, this._items.map(((e$3) => e$3.source)), this._items.map(((e$3) => e$3.ruleId)));
						return this._cached;
					}
					compileAG(e$2, t$2, n$1) {
						return this._hasAnchors ? t$2 ? n$1 ? (this._anchorCache.A1_G1 || (this._anchorCache.A1_G1 = this._resolveAnchors(e$2, t$2, n$1)), this._anchorCache.A1_G1) : (this._anchorCache.A1_G0 || (this._anchorCache.A1_G0 = this._resolveAnchors(e$2, t$2, n$1)), this._anchorCache.A1_G0) : n$1 ? (this._anchorCache.A0_G1 || (this._anchorCache.A0_G1 = this._resolveAnchors(e$2, t$2, n$1)), this._anchorCache.A0_G1) : (this._anchorCache.A0_G0 || (this._anchorCache.A0_G0 = this._resolveAnchors(e$2, t$2, n$1)), this._anchorCache.A0_G0) : this.compile(e$2);
					}
					_resolveAnchors(e$2, t$2, n$1) {
						return new g(e$2, this._items.map(((e$3) => e$3.resolveAnchors(t$2, n$1))), this._items.map(((e$3) => e$3.ruleId)));
					}
				}
				t$1.RegExpSourceList = m;
				class g {
					constructor(e$2, t$2, n$1) {
						this.regExps = t$2, this.rules = n$1, this.scanner = e$2.createOnigScanner(t$2);
					}
					dispose() {
						"function" == typeof this.scanner.dispose && this.scanner.dispose();
					}
					toString() {
						const e$2 = [];
						for (let t$2 = 0, n$1 = this.rules.length; t$2 < n$1; t$2++) e$2.push("   - " + this.rules[t$2] + ": " + this.regExps[t$2]);
						return e$2.join("\n");
					}
					findNextMatchSync(e$2, t$2, n$1) {
						const s$1 = this.scanner.findNextMatchSync(e$2, t$2, n$1);
						return s$1 ? {
							ruleId: this.rules[s$1.index],
							captureIndices: s$1.captureIndices
						} : null;
					}
				}
				t$1.CompiledRule = g;
			},
			63: (e$1, t$1, n) => {
				Object.defineProperty(t$1, "__esModule", { value: !0 }), t$1.ThemeTrieElement = t$1.ThemeTrieElementRule = t$1.ColorMap = t$1.fontStyleToString = t$1.ParsedThemeRule = t$1.parseTheme = t$1.StyleAttributes = t$1.ScopeStack = t$1.Theme = void 0;
				const s = n(807);
				class r {
					constructor(e$2, t$2, n$1) {
						this._colorMap = e$2, this._defaults = t$2, this._root = n$1, this._cachedMatchRoot = new s.CachedFn(((e$3) => this._root.match(e$3)));
					}
					static createFromRawTheme(e$2, t$2) {
						return this.createFromParsedTheme(c(e$2), t$2);
					}
					static createFromParsedTheme(e$2, t$2) {
						return function(e$3, t$3) {
							e$3.sort(((e$4, t$4) => {
								let n$2 = s.strcmp(e$4.scope, t$4.scope);
								return 0 !== n$2 ? n$2 : (n$2 = s.strArrCmp(e$4.parentScopes, t$4.parentScopes), 0 !== n$2 ? n$2 : e$4.index - t$4.index);
							}));
							let n$1 = 0, i$1 = "#000000", o$1 = "#ffffff", c$1 = "", l$1 = 0, h$1 = 0;
							for (; e$3.length >= 1 && "" === e$3[0].scope;) {
								let t$4 = e$3.shift();
								-1 !== t$4.fontStyle && (n$1 = t$4.fontStyle), null !== t$4.foreground && (i$1 = t$4.foreground), null !== t$4.background && (o$1 = t$4.background), null !== t$4.fontFamily && (c$1 = t$4.fontFamily), null !== t$4.fontSize && (l$1 = t$4.fontSize), null !== t$4.lineHeight && (h$1 = t$4.lineHeight);
							}
							let f = new u(t$3), m = new a(n$1, f.getId(i$1), f.getId(o$1), c$1, l$1, h$1), g = new d(new p(0, null, -1, 0, 0, c$1, l$1, h$1), []);
							for (let t$4 = 0, n$2 = e$3.length; t$4 < n$2; t$4++) {
								let n$3 = e$3[t$4];
								g.insert(0, n$3.scope, n$3.parentScopes, n$3.fontStyle, f.getId(n$3.foreground), f.getId(n$3.background), n$3.fontFamily, n$3.fontSize, n$3.lineHeight);
							}
							return new r(f, m, g);
						}(e$2, t$2);
					}
					getColorMap() {
						return this._colorMap.getColorMap();
					}
					getDefaults() {
						return this._defaults;
					}
					match(e$2) {
						if (null === e$2) return this._defaults;
						const t$2 = e$2.scopeName, n$1 = this._cachedMatchRoot.get(t$2).find(((t$3) => function(e$3, t$4) {
							if (0 === t$4.length) return !0;
							for (let n$2 = 0; n$2 < t$4.length; n$2++) {
								let s$1 = t$4[n$2], r$1 = !1;
								if (">" === s$1) {
									if (n$2 === t$4.length - 1) return !1;
									s$1 = t$4[++n$2], r$1 = !0;
								}
								for (; e$3 && !o(e$3.scopeName, s$1);) {
									if (r$1) return !1;
									e$3 = e$3.parent;
								}
								if (!e$3) return !1;
								e$3 = e$3.parent;
							}
							return !0;
						}(e$2.parent, t$3.parentScopes)));
						return n$1 ? new a(n$1.fontStyle, n$1.foreground, n$1.background, n$1.fontFamily, n$1.fontSize, n$1.lineHeight) : null;
					}
				}
				t$1.Theme = r;
				class i {
					constructor(e$2, t$2) {
						this.parent = e$2, this.scopeName = t$2;
					}
					static push(e$2, t$2) {
						for (const n$1 of t$2) e$2 = new i(e$2, n$1);
						return e$2;
					}
					static from(...e$2) {
						let t$2 = null;
						for (let n$1 = 0; n$1 < e$2.length; n$1++) t$2 = new i(t$2, e$2[n$1]);
						return t$2;
					}
					push(e$2) {
						return new i(this, e$2);
					}
					getSegments() {
						let e$2 = this;
						const t$2 = [];
						for (; e$2;) t$2.push(e$2.scopeName), e$2 = e$2.parent;
						return t$2.reverse(), t$2;
					}
					toString() {
						return this.getSegments().join(" ");
					}
					extends(e$2) {
						return this === e$2 || null !== this.parent && this.parent.extends(e$2);
					}
					getExtensionIfDefined(e$2) {
						const t$2 = [];
						let n$1 = this;
						for (; n$1 && n$1 !== e$2;) t$2.push(n$1.scopeName), n$1 = n$1.parent;
						return n$1 === e$2 ? t$2.reverse() : void 0;
					}
				}
				function o(e$2, t$2) {
					return t$2 === e$2 || e$2.startsWith(t$2) && "." === e$2[t$2.length];
				}
				t$1.ScopeStack = i;
				class a {
					constructor(e$2, t$2, n$1, s$1, r$1, i$1) {
						this.fontStyle = e$2, this.foregroundId = t$2, this.backgroundId = n$1, this.fontFamily = s$1, this.fontSize = r$1, this.lineHeight = i$1;
					}
				}
				function c(e$2) {
					if (!e$2) return [];
					if (!e$2.settings || !Array.isArray(e$2.settings)) return [];
					let t$2 = e$2.settings, n$1 = [], r$1 = 0;
					for (let e$3 = 0, i$1 = t$2.length; e$3 < i$1; e$3++) {
						let i$2, o$1 = t$2[e$3];
						if (!o$1.settings) continue;
						if ("string" == typeof o$1.scope) {
							let e$4 = o$1.scope;
							e$4 = e$4.replace(/^[,]+/, ""), e$4 = e$4.replace(/[,]+$/, ""), i$2 = e$4.split(",");
						} else i$2 = Array.isArray(o$1.scope) ? o$1.scope : [""];
						let a$1 = -1;
						if ("string" == typeof o$1.settings.fontStyle) {
							a$1 = 0;
							let e$4 = o$1.settings.fontStyle.split(" ");
							for (let t$3 = 0, n$2 = e$4.length; t$3 < n$2; t$3++) switch (e$4[t$3]) {
								case "italic":
									a$1 |= 1;
									break;
								case "bold":
									a$1 |= 2;
									break;
								case "underline":
									a$1 |= 4;
									break;
								case "strikethrough": a$1 |= 8;
							}
						}
						let c$1 = null;
						"string" == typeof o$1.settings.foreground && s.isValidHexColor(o$1.settings.foreground) && (c$1 = o$1.settings.foreground);
						let u$1 = null;
						"string" == typeof o$1.settings.background && s.isValidHexColor(o$1.settings.background) && (u$1 = o$1.settings.background);
						let h$1 = "";
						"string" == typeof o$1.settings.fontFamily && (h$1 = o$1.settings.fontFamily);
						let p$1 = 0;
						"number" == typeof o$1.settings.fontSize && (p$1 = o$1.settings.fontSize);
						let d$1 = 0;
						"number" == typeof o$1.settings.lineHeight && (d$1 = o$1.settings.lineHeight);
						for (let t$3 = 0, s$1 = i$2.length; t$3 < s$1; t$3++) {
							let s$2 = i$2[t$3].trim().split(" "), o$2 = s$2[s$2.length - 1], f = null;
							s$2.length > 1 && (f = s$2.slice(0, s$2.length - 1), f.reverse()), n$1[r$1++] = new l(o$2, f, e$3, a$1, c$1, u$1, h$1, p$1, d$1);
						}
					}
					return n$1;
				}
				t$1.StyleAttributes = a, t$1.parseTheme = c;
				class l {
					constructor(e$2, t$2, n$1, s$1, r$1, i$1, o$1, a$1, c$1) {
						this.scope = e$2, this.parentScopes = t$2, this.index = n$1, this.fontStyle = s$1, this.foreground = r$1, this.background = i$1, this.fontFamily = o$1, this.fontSize = a$1, this.lineHeight = c$1;
					}
				}
				t$1.ParsedThemeRule = l, t$1.fontStyleToString = function(e$2) {
					if (-1 === e$2) return "not set";
					let t$2 = "";
					return 1 & e$2 && (t$2 += "italic "), 2 & e$2 && (t$2 += "bold "), 4 & e$2 && (t$2 += "underline "), 8 & e$2 && (t$2 += "strikethrough "), "" === t$2 && (t$2 = "none"), t$2.trim();
				};
				class u {
					constructor(e$2) {
						if (this._lastColorId = 0, this._id2color = [], this._color2id = Object.create(null), Array.isArray(e$2)) {
							this._isFrozen = !0;
							for (let t$2 = 0, n$1 = e$2.length; t$2 < n$1; t$2++) this._color2id[e$2[t$2]] = t$2, this._id2color[t$2] = e$2[t$2];
						} else this._isFrozen = !1;
					}
					getId(e$2) {
						if (null === e$2) return 0;
						e$2 = e$2.toUpperCase();
						let t$2 = this._color2id[e$2];
						if (t$2) return t$2;
						if (this._isFrozen) throw new Error(`Missing color in color map - ${e$2}`);
						return t$2 = ++this._lastColorId, this._color2id[e$2] = t$2, this._id2color[t$2] = e$2, t$2;
					}
					getColorMap() {
						return this._id2color.slice(0);
					}
				}
				t$1.ColorMap = u;
				const h = Object.freeze([]);
				class p {
					constructor(e$2, t$2, n$1, s$1, r$1, i$1, o$1, a$1) {
						this.scopeDepth = e$2, this.parentScopes = t$2 || h, this.fontStyle = n$1, this.foreground = s$1, this.background = r$1, this.fontFamily = i$1, this.fontSize = o$1, this.lineHeight = a$1;
					}
					clone() {
						return new p(this.scopeDepth, this.parentScopes, this.fontStyle, this.foreground, this.background, this.fontFamily, this.fontSize, this.lineHeight);
					}
					static cloneArr(e$2) {
						let t$2 = [];
						for (let n$1 = 0, s$1 = e$2.length; n$1 < s$1; n$1++) t$2[n$1] = e$2[n$1].clone();
						return t$2;
					}
					acceptOverwrite(e$2, t$2, n$1, s$1, r$1, i$1, o$1) {
						this.scopeDepth > e$2 ? console.log("how did this happen?") : this.scopeDepth = e$2, -1 !== t$2 && (this.fontStyle = t$2), 0 !== n$1 && (this.foreground = n$1), 0 !== s$1 && (this.background = s$1), "" !== r$1 && (this.fontFamily = r$1), 0 !== i$1 && (this.fontSize = i$1), 0 !== o$1 && (this.lineHeight = o$1);
					}
				}
				t$1.ThemeTrieElementRule = p;
				class d {
					constructor(e$2, t$2 = [], n$1 = {}) {
						this._mainRule = e$2, this._children = n$1, this._rulesWithParentScopes = t$2;
					}
					static _cmpBySpecificity(e$2, t$2) {
						if (e$2.scopeDepth !== t$2.scopeDepth) return t$2.scopeDepth - e$2.scopeDepth;
						let n$1 = 0, s$1 = 0;
						for (; ">" === e$2.parentScopes[n$1] && n$1++, ">" === t$2.parentScopes[s$1] && s$1++, !(n$1 >= e$2.parentScopes.length || s$1 >= t$2.parentScopes.length);) {
							const r$1 = t$2.parentScopes[s$1].length - e$2.parentScopes[n$1].length;
							if (0 !== r$1) return r$1;
							n$1++, s$1++;
						}
						return t$2.parentScopes.length - e$2.parentScopes.length;
					}
					match(e$2) {
						if ("" !== e$2) {
							let t$3, n$1, s$1 = e$2.indexOf(".");
							if (-1 === s$1 ? (t$3 = e$2, n$1 = "") : (t$3 = e$2.substring(0, s$1), n$1 = e$2.substring(s$1 + 1)), this._children.hasOwnProperty(t$3)) return this._children[t$3].match(n$1);
						}
						const t$2 = this._rulesWithParentScopes.concat(this._mainRule);
						return t$2.sort(d._cmpBySpecificity), t$2;
					}
					insert(e$2, t$2, n$1, s$1, r$1, i$1, o$1, a$1, c$1) {
						if ("" === t$2) return void this._doInsertHere(e$2, n$1, s$1, r$1, i$1, o$1, a$1, c$1);
						let l$1, u$1, h$1, f = t$2.indexOf(".");
						-1 === f ? (l$1 = t$2, u$1 = "") : (l$1 = t$2.substring(0, f), u$1 = t$2.substring(f + 1)), this._children.hasOwnProperty(l$1) ? h$1 = this._children[l$1] : (h$1 = new d(this._mainRule.clone(), p.cloneArr(this._rulesWithParentScopes)), this._children[l$1] = h$1), h$1.insert(e$2 + 1, u$1, n$1, s$1, r$1, i$1, o$1, a$1, c$1);
					}
					_doInsertHere(e$2, t$2, n$1, r$1, i$1, o$1, a$1, c$1) {
						if (null !== t$2) {
							for (let l$1 = 0, u$1 = this._rulesWithParentScopes.length; l$1 < u$1; l$1++) {
								let u$2 = this._rulesWithParentScopes[l$1];
								if (0 === s.strArrCmp(u$2.parentScopes, t$2)) return void u$2.acceptOverwrite(e$2, n$1, r$1, i$1, o$1, a$1, c$1);
							}
							-1 === n$1 && (n$1 = this._mainRule.fontStyle), 0 === r$1 && (r$1 = this._mainRule.foreground), 0 === i$1 && (i$1 = this._mainRule.background), "" === o$1 && (o$1 = this._mainRule.fontFamily), 0 === a$1 && (a$1 = this._mainRule.fontSize), 0 === c$1 && (c$1 = this._mainRule.lineHeight), this._rulesWithParentScopes.push(new p(e$2, t$2, n$1, r$1, i$1, o$1, a$1, c$1));
						} else this._mainRule.acceptOverwrite(e$2, n$1, r$1, i$1, o$1, a$1, c$1);
					}
				}
				t$1.ThemeTrieElement = d;
			},
			807: (e$1, t$1) => {
				function n(e$2) {
					return Array.isArray(e$2) ? function(e$3) {
						let t$2 = [];
						for (let s$1 = 0, r$1 = e$3.length; s$1 < r$1; s$1++) t$2[s$1] = n(e$3[s$1]);
						return t$2;
					}(e$2) : "object" == typeof e$2 ? function(e$3) {
						let t$2 = {};
						for (let s$1 in e$3) t$2[s$1] = n(e$3[s$1]);
						return t$2;
					}(e$2) : e$2;
				}
				Object.defineProperty(t$1, "__esModule", { value: !0 }), t$1.containsRTL = t$1.performanceNow = t$1.CachedFn = t$1.escapeRegExpCharacters = t$1.isValidHexColor = t$1.strArrCmp = t$1.strcmp = t$1.RegexSource = t$1.basename = t$1.mergeObjects = t$1.clone = void 0, t$1.clone = function(e$2) {
					return n(e$2);
				}, t$1.mergeObjects = function(e$2, ...t$2) {
					return t$2.forEach(((t$3) => {
						for (let n$1 in t$3) e$2[n$1] = t$3[n$1];
					})), e$2;
				}, t$1.basename = function e$2(t$2) {
					const n$1 = ~t$2.lastIndexOf("/") || ~t$2.lastIndexOf("\\");
					return 0 === n$1 ? t$2 : ~n$1 == t$2.length - 1 ? e$2(t$2.substring(0, t$2.length - 1)) : t$2.substr(1 + ~n$1);
				};
				let s, r = /\$(\d+)|\${(\d+):\/(downcase|upcase)}/g;
				function i(e$2, t$2) {
					return e$2 < t$2 ? -1 : e$2 > t$2 ? 1 : 0;
				}
				t$1.RegexSource = class {
					static hasCaptures(e$2) {
						return null !== e$2 && (r.lastIndex = 0, r.test(e$2));
					}
					static replaceCaptures(e$2, t$2, n$1) {
						return e$2.replace(r, ((e$3, s$1, r$1, i$1) => {
							let o = n$1[parseInt(s$1 || r$1, 10)];
							if (!o) return e$3;
							{
								let e$4 = t$2.substring(o.start, o.end);
								for (; "." === e$4[0];) e$4 = e$4.substring(1);
								switch (i$1) {
									case "downcase": return e$4.toLowerCase();
									case "upcase": return e$4.toUpperCase();
									default: return e$4;
								}
							}
						}));
					}
				}, t$1.strcmp = i, t$1.strArrCmp = function(e$2, t$2) {
					if (null === e$2 && null === t$2) return 0;
					if (!e$2) return -1;
					if (!t$2) return 1;
					let n$1 = e$2.length, s$1 = t$2.length;
					if (n$1 === s$1) {
						for (let s$2 = 0; s$2 < n$1; s$2++) {
							let n$2 = i(e$2[s$2], t$2[s$2]);
							if (0 !== n$2) return n$2;
						}
						return 0;
					}
					return n$1 - s$1;
				}, t$1.isValidHexColor = function(e$2) {
					return !!(/^#[0-9a-f]{6}$/i.test(e$2) || /^#[0-9a-f]{8}$/i.test(e$2) || /^#[0-9a-f]{3}$/i.test(e$2) || /^#[0-9a-f]{4}$/i.test(e$2));
				}, t$1.escapeRegExpCharacters = function(e$2) {
					return e$2.replace(/[\-\\\{\}\*\+\?\|\^\$\.\,\[\]\(\)\#\s]/g, "\\$&");
				}, t$1.CachedFn = class {
					constructor(e$2) {
						this.fn = e$2, this.cache = /* @__PURE__ */ new Map();
					}
					get(e$2) {
						if (this.cache.has(e$2)) return this.cache.get(e$2);
						const t$2 = this.fn(e$2);
						return this.cache.set(e$2, t$2), t$2;
					}
				}, t$1.performanceNow = "undefined" == typeof performance ? function() {
					return Date.now();
				} : function() {
					return performance.now();
				}, t$1.containsRTL = function(e$2) {
					return s || (s = /(?:[\u05BE\u05C0\u05C3\u05C6\u05D0-\u05F4\u0608\u060B\u060D\u061B-\u064A\u066D-\u066F\u0671-\u06D5\u06E5\u06E6\u06EE\u06EF\u06FA-\u0710\u0712-\u072F\u074D-\u07A5\u07B1-\u07EA\u07F4\u07F5\u07FA\u07FE-\u0815\u081A\u0824\u0828\u0830-\u0858\u085E-\u088E\u08A0-\u08C9\u200F\uFB1D\uFB1F-\uFB28\uFB2A-\uFD3D\uFD50-\uFDC7\uFDF0-\uFDFC\uFE70-\uFEFC]|\uD802[\uDC00-\uDD1B\uDD20-\uDE00\uDE10-\uDE35\uDE40-\uDEE4\uDEEB-\uDF35\uDF40-\uDFFF]|\uD803[\uDC00-\uDD23\uDE80-\uDEA9\uDEAD-\uDF45\uDF51-\uDF81\uDF86-\uDFF6]|\uD83A[\uDC00-\uDCCF\uDD00-\uDD43\uDD4B-\uDFFF]|\uD83B[\uDC00-\uDEBB])/), s.test(e$2);
				};
			}
		}, t = {};
		return function n(s) {
			var r = t[s];
			if (void 0 !== r) return r.exports;
			var i = t[s] = { exports: {} };
			return e[s].call(i.exports, i, i.exports, n), i.exports;
		}(625);
	})()));
})))();
"" + new URL("onig-CwjCXqnP.wasm", import.meta.url).href;
var browserOnigurumaPromise;
async function loadBrowserOniguruma() {
	browserOnigurumaPromise ?? (browserOnigurumaPromise = (async () => {
		const oniguruma = await __vitePreload(() => import("./main-BIUnxdI8.js").then(__toDynamicImportESM()), __vite__mapDeps([0,1]), import.meta.url);
		const response = await fetch("" + new URL("onig-CwjCXqnP.wasm", import.meta.url).href);
		if (!response.ok) throw new Error(`Failed to load TextMate regex engine from ${"" + new URL("onig-CwjCXqnP.wasm", import.meta.url).href}`);
		await oniguruma.loadWASM(response);
		return {
			createOnigScanner: oniguruma.createOnigScanner,
			createOnigString: oniguruma.createOnigString
		};
	})());
	return browserOnigurumaPromise;
}
var TextMateTokenizerState = class TextMateTokenizerState {
	constructor(ruleStack) {
		this.ruleStack = ruleStack;
	}
	clone() {
		return new TextMateTokenizerState(this.ruleStack.clone());
	}
	equals(other) {
		return other instanceof TextMateTokenizerState && this.ruleStack.equals(other.ruleStack);
	}
};
function createTokensProvider(grammar, fallbackScopeName) {
	return {
		getInitialState() {
			return new TextMateTokenizerState(import_main.INITIAL);
		},
		tokenize(line, state) {
			const textMateState = state instanceof TextMateTokenizerState ? state : new TextMateTokenizerState(import_main.INITIAL);
			const result = grammar.tokenizeLine(line, textMateState.ruleStack);
			return {
				endState: new TextMateTokenizerState(result.ruleStack),
				tokens: result.tokens.map((token) => ({
					startIndex: token.startIndex,
					scopes: token.scopes.at(-1) ?? fallbackScopeName
				}))
			};
		}
	};
}
async function createTextMateTokensProvider(options) {
	const registry = new import_main.Registry({
		onigLib: (options.loadOniguruma ?? loadBrowserOniguruma)(),
		loadGrammar: options.loadGrammar
	});
	let grammar;
	try {
		grammar = await registry.loadGrammar(options.scopeName);
	} catch (error) {
		if (error instanceof Error && error.message.includes(`No grammar provided for <${options.scopeName}>`)) throw new Error(`No TextMate grammar registered for scope ${options.scopeName}`);
		throw error;
	}
	if (!grammar) throw new Error(`No TextMate grammar registered for scope ${options.scopeName}`);
	return createTokensProvider(grammar, options.scopeName);
}
export { createTextMateTokensProvider };
