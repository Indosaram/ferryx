import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
var _a$1;
const NEVER = /* @__PURE__ */ Object.freeze({ status: "aborted" });
function $constructor(name, initializer$2, params) {
	function init(inst, def) {
		if (!inst._zod) Object.defineProperty(inst, "_zod", {
			value: {
				def,
				constr: _,
				traits: /* @__PURE__ */ new Set()
			},
			enumerable: false
		});
		if (inst._zod.traits.has(name)) return;
		inst._zod.traits.add(name);
		initializer$2(inst, def);
		const proto = _.prototype;
		const keys = Object.keys(proto);
		for (let i = 0; i < keys.length; i++) {
			const k = keys[i];
			if (!(k in inst)) inst[k] = proto[k].bind(inst);
		}
	}
	const Parent = params?.Parent ?? Object;
	class Definition extends Parent {}
	Object.defineProperty(Definition, "name", { value: name });
	function _(def) {
		var _a$2;
		const inst = params?.Parent ? new Definition() : this;
		init(inst, def);
		(_a$2 = inst._zod).deferred ?? (_a$2.deferred = []);
		for (const fn of inst._zod.deferred) fn();
		return inst;
	}
	Object.defineProperty(_, "init", { value: init });
	Object.defineProperty(_, Symbol.hasInstance, { value: (inst) => {
		if (params?.Parent && inst instanceof params.Parent) return true;
		return inst?._zod?.traits?.has(name);
	} });
	Object.defineProperty(_, "name", { value: name });
	return _;
}
var $ZodAsyncError = class extends Error {
	constructor() {
		super(`Encountered Promise during synchronous parse. Use .parseAsync() instead.`);
	}
};
var $ZodEncodeError = class extends Error {
	constructor(name) {
		super(`Encountered unidirectional transform during encode: ${name}`);
		this.name = "ZodEncodeError";
	}
};
(_a$1 = globalThis).__zod_globalConfig ?? (_a$1.__zod_globalConfig = {});
const globalConfig = globalThis.__zod_globalConfig;
function config(newConfig) {
	if (newConfig) Object.assign(globalConfig, newConfig);
	return globalConfig;
}
function getEnumValues(entries) {
	const numericValues = Object.values(entries).filter((v) => typeof v === "number");
	return Object.entries(entries).filter(([k, _]) => numericValues.indexOf(+k) === -1).map(([_, v]) => v);
}
function jsonStringifyReplacer(_, value) {
	if (typeof value === "bigint") return value.toString();
	return value;
}
function cached(getter) {
	return { get value() {
		{
			const value = getter();
			Object.defineProperty(this, "value", { value });
			return value;
		}
		throw new Error("cached value already set");
	} };
}
function nullish(input) {
	return input === null || input === void 0;
}
function cleanRegex(source) {
	const start = source.startsWith("^") ? 1 : 0;
	const end = source.endsWith("$") ? source.length - 1 : source.length;
	return source.slice(start, end);
}
function floatSafeRemainder(val, step) {
	const ratio = val / step;
	const roundedRatio = Math.round(ratio);
	const tolerance = Number.EPSILON * Math.max(Math.abs(ratio), 1);
	if (Math.abs(ratio - roundedRatio) < tolerance) return 0;
	return ratio - roundedRatio;
}
var EVALUATING = /* @__PURE__ */ Symbol("evaluating");
function defineLazy(object$1, key, getter) {
	let value = void 0;
	Object.defineProperty(object$1, key, {
		get() {
			if (value === EVALUATING) return;
			if (value === void 0) {
				value = EVALUATING;
				value = getter();
			}
			return value;
		},
		set(v) {
			Object.defineProperty(object$1, key, { value: v });
		},
		configurable: true
	});
}
function assignProp(target, prop, value) {
	Object.defineProperty(target, prop, {
		value,
		writable: true,
		enumerable: true,
		configurable: true
	});
}
function mergeDefs(...defs) {
	const mergedDescriptors = {};
	for (const def of defs) {
		const descriptors = Object.getOwnPropertyDescriptors(def);
		Object.assign(mergedDescriptors, descriptors);
	}
	return Object.defineProperties({}, mergedDescriptors);
}
function esc(str) {
	return JSON.stringify(str);
}
function slugify(input) {
	return input.toLowerCase().trim().replace(/[^\w\s-]/g, "").replace(/[\s_-]+/g, "-").replace(/^-+|-+$/g, "");
}
const captureStackTrace = "captureStackTrace" in Error ? Error.captureStackTrace : (..._args) => {};
function isObject(data) {
	return typeof data === "object" && data !== null && !Array.isArray(data);
}
const allowsEval = /* @__PURE__ */ cached(() => {
	if (globalConfig.jitless) return false;
	if (typeof navigator !== "undefined" && navigator?.userAgent?.includes("Cloudflare")) return false;
	try {
		new Function("");
		return true;
	} catch (_) {
		return false;
	}
});
function isPlainObject(o) {
	if (isObject(o) === false) return false;
	const ctor = o.constructor;
	if (ctor === void 0) return true;
	if (typeof ctor !== "function") return true;
	const prot = ctor.prototype;
	if (isObject(prot) === false) return false;
	if (Object.prototype.hasOwnProperty.call(prot, "isPrototypeOf") === false) return false;
	return true;
}
function shallowClone(o) {
	if (isPlainObject(o)) return { ...o };
	if (Array.isArray(o)) return [...o];
	if (o instanceof Map) return new Map(o);
	if (o instanceof Set) return new Set(o);
	return o;
}
const propertyKeyTypes = /* @__PURE__ */ new Set([
	"string",
	"number",
	"symbol"
]);
function escapeRegex(str) {
	return str.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
function clone(inst, def, params) {
	const cl = new inst._zod.constr(def ?? inst._zod.def);
	if (!def || params?.parent) cl._zod.parent = inst;
	return cl;
}
function normalizeParams(_params) {
	const params = _params;
	if (!params) return {};
	if (typeof params === "string") return { error: () => params };
	if (params?.message !== void 0) {
		if (params?.error !== void 0) throw new Error("Cannot specify both `message` and `error` params");
		params.error = params.message;
	}
	delete params.message;
	if (typeof params.error === "string") return {
		...params,
		error: () => params.error
	};
	return params;
}
function optionalKeys(shape) {
	return Object.keys(shape).filter((k) => {
		return shape[k]._zod.optin === "optional" && shape[k]._zod.optout === "optional";
	});
}
const NUMBER_FORMAT_RANGES = {
	safeint: [Number.MIN_SAFE_INTEGER, Number.MAX_SAFE_INTEGER],
	int32: [-2147483648, 2147483647],
	uint32: [0, 4294967295],
	float32: [-34028234663852886e22, 34028234663852886e22],
	float64: [-Number.MAX_VALUE, Number.MAX_VALUE]
};
function pick(schema, mask) {
	const currDef = schema._zod.def;
	const checks = currDef.checks;
	if (checks && checks.length > 0) throw new Error(".pick() cannot be used on object schemas containing refinements");
	return clone(schema, mergeDefs(schema._zod.def, {
		get shape() {
			const newShape = {};
			for (const key in mask) {
				if (!(key in currDef.shape)) throw new Error(`Unrecognized key: "${key}"`);
				if (!mask[key]) continue;
				newShape[key] = currDef.shape[key];
			}
			assignProp(this, "shape", newShape);
			return newShape;
		},
		checks: []
	}));
}
function omit(schema, mask) {
	const currDef = schema._zod.def;
	const checks = currDef.checks;
	if (checks && checks.length > 0) throw new Error(".omit() cannot be used on object schemas containing refinements");
	return clone(schema, mergeDefs(schema._zod.def, {
		get shape() {
			const newShape = { ...schema._zod.def.shape };
			for (const key in mask) {
				if (!(key in currDef.shape)) throw new Error(`Unrecognized key: "${key}"`);
				if (!mask[key]) continue;
				delete newShape[key];
			}
			assignProp(this, "shape", newShape);
			return newShape;
		},
		checks: []
	}));
}
function extend(schema, shape) {
	if (!isPlainObject(shape)) throw new Error("Invalid input to extend: expected a plain object");
	const checks = schema._zod.def.checks;
	if (checks && checks.length > 0) {
		const existingShape = schema._zod.def.shape;
		for (const key in shape) if (Object.getOwnPropertyDescriptor(existingShape, key) !== void 0) throw new Error("Cannot overwrite keys on object schemas containing refinements. Use `.safeExtend()` instead.");
	}
	return clone(schema, mergeDefs(schema._zod.def, { get shape() {
		const _shape = {
			...schema._zod.def.shape,
			...shape
		};
		assignProp(this, "shape", _shape);
		return _shape;
	} }));
}
function safeExtend(schema, shape) {
	if (!isPlainObject(shape)) throw new Error("Invalid input to safeExtend: expected a plain object");
	return clone(schema, mergeDefs(schema._zod.def, { get shape() {
		const _shape = {
			...schema._zod.def.shape,
			...shape
		};
		assignProp(this, "shape", _shape);
		return _shape;
	} }));
}
function merge(a, b) {
	if (a._zod.def.checks?.length) throw new Error(".merge() cannot be used on object schemas containing refinements. Use .safeExtend() instead.");
	return clone(a, mergeDefs(a._zod.def, {
		get shape() {
			const _shape = {
				...a._zod.def.shape,
				...b._zod.def.shape
			};
			assignProp(this, "shape", _shape);
			return _shape;
		},
		get catchall() {
			return b._zod.def.catchall;
		},
		checks: b._zod.def.checks ?? []
	}));
}
function partial(Class, schema, mask) {
	const checks = schema._zod.def.checks;
	if (checks && checks.length > 0) throw new Error(".partial() cannot be used on object schemas containing refinements");
	return clone(schema, mergeDefs(schema._zod.def, {
		get shape() {
			const oldShape = schema._zod.def.shape;
			const shape = { ...oldShape };
			if (mask) for (const key in mask) {
				if (!(key in oldShape)) throw new Error(`Unrecognized key: "${key}"`);
				if (!mask[key]) continue;
				shape[key] = Class ? new Class({
					type: "optional",
					innerType: oldShape[key]
				}) : oldShape[key];
			}
			else for (const key in oldShape) shape[key] = Class ? new Class({
				type: "optional",
				innerType: oldShape[key]
			}) : oldShape[key];
			assignProp(this, "shape", shape);
			return shape;
		},
		checks: []
	}));
}
function required(Class, schema, mask) {
	return clone(schema, mergeDefs(schema._zod.def, { get shape() {
		const oldShape = schema._zod.def.shape;
		const shape = { ...oldShape };
		if (mask) for (const key in mask) {
			if (!(key in shape)) throw new Error(`Unrecognized key: "${key}"`);
			if (!mask[key]) continue;
			shape[key] = new Class({
				type: "nonoptional",
				innerType: oldShape[key]
			});
		}
		else for (const key in oldShape) shape[key] = new Class({
			type: "nonoptional",
			innerType: oldShape[key]
		});
		assignProp(this, "shape", shape);
		return shape;
	} }));
}
function aborted(x, startIndex = 0) {
	if (x.aborted === true) return true;
	for (let i = startIndex; i < x.issues.length; i++) if (x.issues[i]?.continue !== true) return true;
	return false;
}
function explicitlyAborted(x, startIndex = 0) {
	if (x.aborted === true) return true;
	for (let i = startIndex; i < x.issues.length; i++) if (x.issues[i]?.continue === false) return true;
	return false;
}
function prefixIssues(path, issues) {
	return issues.map((iss) => {
		var _a$2;
		(_a$2 = iss).path ?? (_a$2.path = []);
		iss.path.unshift(path);
		return iss;
	});
}
function unwrapMessage(message) {
	return typeof message === "string" ? message : message?.message;
}
function finalizeIssue(iss, ctx, config$1) {
	const message = iss.message ? iss.message : unwrapMessage(iss.inst?._zod.def?.error?.(iss)) ?? unwrapMessage(ctx?.error?.(iss)) ?? unwrapMessage(config$1.customError?.(iss)) ?? unwrapMessage(config$1.localeError?.(iss)) ?? "Invalid input";
	const { inst: _inst, continue: _continue, input: _input, ...rest } = iss;
	rest.path ?? (rest.path = []);
	rest.message = message;
	if (ctx?.reportInput) rest.input = _input;
	return rest;
}
function getLengthableOrigin(input) {
	if (Array.isArray(input)) return "array";
	if (typeof input === "string") return "string";
	return "unknown";
}
function issue(...args) {
	const [iss, input, inst] = args;
	if (typeof iss === "string") return {
		message: iss,
		code: "custom",
		input,
		inst
	};
	return { ...iss };
}
var initializer$1 = (inst, def) => {
	inst.name = "$ZodError";
	Object.defineProperty(inst, "_zod", {
		value: inst._zod,
		enumerable: false
	});
	Object.defineProperty(inst, "issues", {
		value: def,
		enumerable: false
	});
	inst.message = JSON.stringify(def, jsonStringifyReplacer, 2);
	Object.defineProperty(inst, "toString", {
		value: () => inst.message,
		enumerable: false
	});
};
const $ZodError = $constructor("$ZodError", initializer$1);
const $ZodRealError = $constructor("$ZodError", initializer$1, { Parent: Error });
function flattenError(error, mapper = (issue$1) => issue$1.message) {
	const fieldErrors = {};
	const formErrors = [];
	for (const sub of error.issues) if (sub.path.length > 0) {
		fieldErrors[sub.path[0]] = fieldErrors[sub.path[0]] || [];
		fieldErrors[sub.path[0]].push(mapper(sub));
	} else formErrors.push(mapper(sub));
	return {
		formErrors,
		fieldErrors
	};
}
function formatError(error, mapper = (issue$1) => issue$1.message) {
	const fieldErrors = { _errors: [] };
	const processError = (error$1, path = []) => {
		for (const issue$1 of error$1.issues) if (issue$1.code === "invalid_union" && issue$1.errors.length) issue$1.errors.map((issues) => processError({ issues }, [...path, ...issue$1.path]));
		else if (issue$1.code === "invalid_key") processError({ issues: issue$1.issues }, [...path, ...issue$1.path]);
		else if (issue$1.code === "invalid_element") processError({ issues: issue$1.issues }, [...path, ...issue$1.path]);
		else {
			const fullpath = [...path, ...issue$1.path];
			if (fullpath.length === 0) fieldErrors._errors.push(mapper(issue$1));
			else {
				let curr = fieldErrors;
				let i = 0;
				while (i < fullpath.length) {
					const el = fullpath[i];
					if (!(i === fullpath.length - 1)) curr[el] = curr[el] || { _errors: [] };
					else {
						curr[el] = curr[el] || { _errors: [] };
						curr[el]._errors.push(mapper(issue$1));
					}
					curr = curr[el];
					i++;
				}
			}
		}
	};
	processError(error);
	return fieldErrors;
}
const _parse = (_Err) => (schema, value, _ctx, _params) => {
	const ctx = _ctx ? {
		..._ctx,
		async: false
	} : { async: false };
	const result = schema._zod.run({
		value,
		issues: []
	}, ctx);
	if (result instanceof Promise) throw new $ZodAsyncError();
	if (result.issues.length) {
		const e = new (_params?.Err ?? _Err)(result.issues.map((iss) => finalizeIssue(iss, ctx, config())));
		captureStackTrace(e, _params?.callee);
		throw e;
	}
	return result.value;
};
const _parseAsync = (_Err) => async (schema, value, _ctx, params) => {
	const ctx = _ctx ? {
		..._ctx,
		async: true
	} : { async: true };
	let result = schema._zod.run({
		value,
		issues: []
	}, ctx);
	if (result instanceof Promise) result = await result;
	if (result.issues.length) {
		const e = new (params?.Err ?? _Err)(result.issues.map((iss) => finalizeIssue(iss, ctx, config())));
		captureStackTrace(e, params?.callee);
		throw e;
	}
	return result.value;
};
const _safeParse = (_Err) => (schema, value, _ctx) => {
	const ctx = _ctx ? {
		..._ctx,
		async: false
	} : { async: false };
	const result = schema._zod.run({
		value,
		issues: []
	}, ctx);
	if (result instanceof Promise) throw new $ZodAsyncError();
	return result.issues.length ? {
		success: false,
		error: new (_Err ?? $ZodError)(result.issues.map((iss) => finalizeIssue(iss, ctx, config())))
	} : {
		success: true,
		data: result.value
	};
};
const safeParse$1 = /* @__PURE__ */ _safeParse($ZodRealError);
const _safeParseAsync = (_Err) => async (schema, value, _ctx) => {
	const ctx = _ctx ? {
		..._ctx,
		async: true
	} : { async: true };
	let result = schema._zod.run({
		value,
		issues: []
	}, ctx);
	if (result instanceof Promise) result = await result;
	return result.issues.length ? {
		success: false,
		error: new _Err(result.issues.map((iss) => finalizeIssue(iss, ctx, config())))
	} : {
		success: true,
		data: result.value
	};
};
const safeParseAsync$1 = /* @__PURE__ */ _safeParseAsync($ZodRealError);
const _encode = (_Err) => (schema, value, _ctx) => {
	const ctx = _ctx ? {
		..._ctx,
		direction: "backward"
	} : { direction: "backward" };
	return _parse(_Err)(schema, value, ctx);
};
const _decode = (_Err) => (schema, value, _ctx) => {
	return _parse(_Err)(schema, value, _ctx);
};
const _encodeAsync = (_Err) => async (schema, value, _ctx) => {
	const ctx = _ctx ? {
		..._ctx,
		direction: "backward"
	} : { direction: "backward" };
	return _parseAsync(_Err)(schema, value, ctx);
};
const _decodeAsync = (_Err) => async (schema, value, _ctx) => {
	return _parseAsync(_Err)(schema, value, _ctx);
};
const _safeEncode = (_Err) => (schema, value, _ctx) => {
	const ctx = _ctx ? {
		..._ctx,
		direction: "backward"
	} : { direction: "backward" };
	return _safeParse(_Err)(schema, value, ctx);
};
const _safeDecode = (_Err) => (schema, value, _ctx) => {
	return _safeParse(_Err)(schema, value, _ctx);
};
const _safeEncodeAsync = (_Err) => async (schema, value, _ctx) => {
	const ctx = _ctx ? {
		..._ctx,
		direction: "backward"
	} : { direction: "backward" };
	return _safeParseAsync(_Err)(schema, value, ctx);
};
const _safeDecodeAsync = (_Err) => async (schema, value, _ctx) => {
	return _safeParseAsync(_Err)(schema, value, _ctx);
};
const cuid = /^[cC][0-9a-z]{6,}$/;
const cuid2 = /^[0-9a-z]+$/;
const ulid = /^[0-9A-HJKMNP-TV-Za-hjkmnp-tv-z]{26}$/;
const xid = /^[0-9a-vA-V]{20}$/;
const ksuid = /^[A-Za-z0-9]{27}$/;
const nanoid = /^[a-zA-Z0-9_-]{21}$/;
const duration$1 = /^P(?:(\d+W)|(?!.*W)(?=\d|T\d)(\d+Y)?(\d+M)?(\d+D)?(T(?=\d)(\d+H)?(\d+M)?(\d+([.,]\d+)?S)?)?)$/;
const guid = /^([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})$/;
const uuid$1 = (version$1) => {
	if (!version$1) return /^([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}|00000000-0000-0000-0000-000000000000|ffffffff-ffff-ffff-ffff-ffffffffffff)$/;
	return /* @__PURE__ */ new RegExp(`^([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-${version$1}[0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12})$`);
};
const email = /^(?!\.)(?!.*\.\.)([A-Za-z0-9_'+\-\.]*)[A-Za-z0-9_+-]@([A-Za-z0-9][A-Za-z0-9\-]*\.)+[A-Za-z]{2,}$/;
var _emoji$1 = `^(\\p{Extended_Pictographic}|\\p{Emoji_Component})+$`;
function emoji() {
	return new RegExp(_emoji$1, "u");
}
const ipv4 = /^(?:(?:25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9][0-9]|[0-9])\.){3}(?:25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9][0-9]|[0-9])$/;
const ipv6 = /^(([0-9a-fA-F]{1,4}:){7}[0-9a-fA-F]{1,4}|([0-9a-fA-F]{1,4}:){1,7}:|([0-9a-fA-F]{1,4}:){1,6}:[0-9a-fA-F]{1,4}|([0-9a-fA-F]{1,4}:){1,5}(:[0-9a-fA-F]{1,4}){1,2}|([0-9a-fA-F]{1,4}:){1,4}(:[0-9a-fA-F]{1,4}){1,3}|([0-9a-fA-F]{1,4}:){1,3}(:[0-9a-fA-F]{1,4}){1,4}|([0-9a-fA-F]{1,4}:){1,2}(:[0-9a-fA-F]{1,4}){1,5}|[0-9a-fA-F]{1,4}:((:[0-9a-fA-F]{1,4}){1,6})|:((:[0-9a-fA-F]{1,4}){1,7}|:))$/;
const cidrv4 = /^((25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9][0-9]|[0-9])\.){3}(25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9][0-9]|[0-9])\/([0-9]|[1-2][0-9]|3[0-2])$/;
const cidrv6 = /^(([0-9a-fA-F]{1,4}:){7}[0-9a-fA-F]{1,4}|::|([0-9a-fA-F]{1,4})?::([0-9a-fA-F]{1,4}:?){0,6})\/(12[0-8]|1[01][0-9]|[1-9]?[0-9])$/;
const base64 = /^$|^(?:[0-9a-zA-Z+/]{4})*(?:(?:[0-9a-zA-Z+/]{2}==)|(?:[0-9a-zA-Z+/]{3}=))?$/;
const base64url = /^[A-Za-z0-9_-]*$/;
const httpProtocol = /^https?$/;
const e164 = /^\+[1-9]\d{6,14}$/;
var dateSource = `(?:(?:\\d\\d[2468][048]|\\d\\d[13579][26]|\\d\\d0[48]|[02468][048]00|[13579][26]00)-02-29|\\d{4}-(?:(?:0[13578]|1[02])-(?:0[1-9]|[12]\\d|3[01])|(?:0[469]|11)-(?:0[1-9]|[12]\\d|30)|(?:02)-(?:0[1-9]|1\\d|2[0-8])))`;
const date$1 = /* @__PURE__ */ new RegExp(`^${dateSource}$`);
function timeSource(args) {
	const hhmm = `(?:[01]\\d|2[0-3]):[0-5]\\d`;
	return typeof args.precision === "number" ? args.precision === -1 ? `${hhmm}` : args.precision === 0 ? `${hhmm}:[0-5]\\d` : `${hhmm}:[0-5]\\d\\.\\d{${args.precision}}` : `${hhmm}(?::[0-5]\\d(?:\\.\\d+)?)?`;
}
function time$1(args) {
	return /* @__PURE__ */ new RegExp(`^${timeSource(args)}$`);
}
function datetime$1(args) {
	const time$2 = timeSource({ precision: args.precision });
	const opts = ["Z"];
	if (args.local) opts.push("");
	if (args.offset) opts.push(`([+-](?:[01]\\d|2[0-3]):[0-5]\\d)`);
	const timeRegex = `${time$2}(?:${opts.join("|")})`;
	return /* @__PURE__ */ new RegExp(`^${dateSource}T(?:${timeRegex})$`);
}
const string$1 = (params) => {
	const regex = params ? `[\\s\\S]{${params?.minimum ?? 0},${params?.maximum ?? ""}}` : `[\\s\\S]*`;
	return /* @__PURE__ */ new RegExp(`^${regex}$`);
};
const integer = /^-?\d+$/;
const number$1 = /^-?\d+(?:\.\d+)?$/;
const boolean$1 = /^(?:true|false)$/i;
var _null$2 = /^null$/i;
const lowercase = /^[^A-Z]*$/;
const uppercase = /^[^a-z]*$/;
const $ZodCheck = /* @__PURE__ */ $constructor("$ZodCheck", (inst, def) => {
	var _a$2;
	inst._zod ?? (inst._zod = {});
	inst._zod.def = def;
	(_a$2 = inst._zod).onattach ?? (_a$2.onattach = []);
});
var numericOriginMap = {
	number: "number",
	bigint: "bigint",
	object: "date"
};
const $ZodCheckLessThan = /* @__PURE__ */ $constructor("$ZodCheckLessThan", (inst, def) => {
	$ZodCheck.init(inst, def);
	const origin = numericOriginMap[typeof def.value];
	inst._zod.onattach.push((inst$1) => {
		const bag = inst$1._zod.bag;
		const curr = (def.inclusive ? bag.maximum : bag.exclusiveMaximum) ?? Number.POSITIVE_INFINITY;
		if (def.value < curr) if (def.inclusive) bag.maximum = def.value;
		else bag.exclusiveMaximum = def.value;
	});
	inst._zod.check = (payload) => {
		if (def.inclusive ? payload.value <= def.value : payload.value < def.value) return;
		payload.issues.push({
			origin,
			code: "too_big",
			maximum: typeof def.value === "object" ? def.value.getTime() : def.value,
			input: payload.value,
			inclusive: def.inclusive,
			inst,
			continue: !def.abort
		});
	};
});
const $ZodCheckGreaterThan = /* @__PURE__ */ $constructor("$ZodCheckGreaterThan", (inst, def) => {
	$ZodCheck.init(inst, def);
	const origin = numericOriginMap[typeof def.value];
	inst._zod.onattach.push((inst$1) => {
		const bag = inst$1._zod.bag;
		const curr = (def.inclusive ? bag.minimum : bag.exclusiveMinimum) ?? Number.NEGATIVE_INFINITY;
		if (def.value > curr) if (def.inclusive) bag.minimum = def.value;
		else bag.exclusiveMinimum = def.value;
	});
	inst._zod.check = (payload) => {
		if (def.inclusive ? payload.value >= def.value : payload.value > def.value) return;
		payload.issues.push({
			origin,
			code: "too_small",
			minimum: typeof def.value === "object" ? def.value.getTime() : def.value,
			input: payload.value,
			inclusive: def.inclusive,
			inst,
			continue: !def.abort
		});
	};
});
const $ZodCheckMultipleOf = /* @__PURE__ */ $constructor("$ZodCheckMultipleOf", (inst, def) => {
	$ZodCheck.init(inst, def);
	inst._zod.onattach.push((inst$1) => {
		var _a$2;
		(_a$2 = inst$1._zod.bag).multipleOf ?? (_a$2.multipleOf = def.value);
	});
	inst._zod.check = (payload) => {
		if (typeof payload.value !== typeof def.value) throw new Error("Cannot mix number and bigint in multiple_of check.");
		if (typeof payload.value === "bigint" ? payload.value % def.value === BigInt(0) : floatSafeRemainder(payload.value, def.value) === 0) return;
		payload.issues.push({
			origin: typeof payload.value,
			code: "not_multiple_of",
			divisor: def.value,
			input: payload.value,
			inst,
			continue: !def.abort
		});
	};
});
const $ZodCheckNumberFormat = /* @__PURE__ */ $constructor("$ZodCheckNumberFormat", (inst, def) => {
	$ZodCheck.init(inst, def);
	def.format = def.format || "float64";
	const isInt = def.format?.includes("int");
	const origin = isInt ? "int" : "number";
	const [minimum, maximum] = NUMBER_FORMAT_RANGES[def.format];
	inst._zod.onattach.push((inst$1) => {
		const bag = inst$1._zod.bag;
		bag.format = def.format;
		bag.minimum = minimum;
		bag.maximum = maximum;
		if (isInt) bag.pattern = integer;
	});
	inst._zod.check = (payload) => {
		const input = payload.value;
		if (isInt) {
			if (!Number.isInteger(input)) {
				payload.issues.push({
					expected: origin,
					format: def.format,
					code: "invalid_type",
					continue: false,
					input,
					inst
				});
				return;
			}
			if (!Number.isSafeInteger(input)) {
				if (input > 0) payload.issues.push({
					input,
					code: "too_big",
					maximum: Number.MAX_SAFE_INTEGER,
					note: "Integers must be within the safe integer range.",
					inst,
					origin,
					inclusive: true,
					continue: !def.abort
				});
				else payload.issues.push({
					input,
					code: "too_small",
					minimum: Number.MIN_SAFE_INTEGER,
					note: "Integers must be within the safe integer range.",
					inst,
					origin,
					inclusive: true,
					continue: !def.abort
				});
				return;
			}
		}
		if (input < minimum) payload.issues.push({
			origin: "number",
			input,
			code: "too_small",
			minimum,
			inclusive: true,
			inst,
			continue: !def.abort
		});
		if (input > maximum) payload.issues.push({
			origin: "number",
			input,
			code: "too_big",
			maximum,
			inclusive: true,
			inst,
			continue: !def.abort
		});
	};
});
const $ZodCheckMaxLength = /* @__PURE__ */ $constructor("$ZodCheckMaxLength", (inst, def) => {
	var _a$2;
	$ZodCheck.init(inst, def);
	(_a$2 = inst._zod.def).when ?? (_a$2.when = (payload) => {
		const val = payload.value;
		return !nullish(val) && val.length !== void 0;
	});
	inst._zod.onattach.push((inst$1) => {
		const curr = inst$1._zod.bag.maximum ?? Number.POSITIVE_INFINITY;
		if (def.maximum < curr) inst$1._zod.bag.maximum = def.maximum;
	});
	inst._zod.check = (payload) => {
		const input = payload.value;
		if (input.length <= def.maximum) return;
		const origin = getLengthableOrigin(input);
		payload.issues.push({
			origin,
			code: "too_big",
			maximum: def.maximum,
			inclusive: true,
			input,
			inst,
			continue: !def.abort
		});
	};
});
const $ZodCheckMinLength = /* @__PURE__ */ $constructor("$ZodCheckMinLength", (inst, def) => {
	var _a$2;
	$ZodCheck.init(inst, def);
	(_a$2 = inst._zod.def).when ?? (_a$2.when = (payload) => {
		const val = payload.value;
		return !nullish(val) && val.length !== void 0;
	});
	inst._zod.onattach.push((inst$1) => {
		const curr = inst$1._zod.bag.minimum ?? Number.NEGATIVE_INFINITY;
		if (def.minimum > curr) inst$1._zod.bag.minimum = def.minimum;
	});
	inst._zod.check = (payload) => {
		const input = payload.value;
		if (input.length >= def.minimum) return;
		const origin = getLengthableOrigin(input);
		payload.issues.push({
			origin,
			code: "too_small",
			minimum: def.minimum,
			inclusive: true,
			input,
			inst,
			continue: !def.abort
		});
	};
});
const $ZodCheckLengthEquals = /* @__PURE__ */ $constructor("$ZodCheckLengthEquals", (inst, def) => {
	var _a$2;
	$ZodCheck.init(inst, def);
	(_a$2 = inst._zod.def).when ?? (_a$2.when = (payload) => {
		const val = payload.value;
		return !nullish(val) && val.length !== void 0;
	});
	inst._zod.onattach.push((inst$1) => {
		const bag = inst$1._zod.bag;
		bag.minimum = def.length;
		bag.maximum = def.length;
		bag.length = def.length;
	});
	inst._zod.check = (payload) => {
		const input = payload.value;
		const length = input.length;
		if (length === def.length) return;
		const origin = getLengthableOrigin(input);
		const tooBig = length > def.length;
		payload.issues.push({
			origin,
			...tooBig ? {
				code: "too_big",
				maximum: def.length
			} : {
				code: "too_small",
				minimum: def.length
			},
			inclusive: true,
			exact: true,
			input: payload.value,
			inst,
			continue: !def.abort
		});
	};
});
const $ZodCheckStringFormat = /* @__PURE__ */ $constructor("$ZodCheckStringFormat", (inst, def) => {
	var _a$2, _b;
	$ZodCheck.init(inst, def);
	inst._zod.onattach.push((inst$1) => {
		const bag = inst$1._zod.bag;
		bag.format = def.format;
		if (def.pattern) {
			bag.patterns ?? (bag.patterns = /* @__PURE__ */ new Set());
			bag.patterns.add(def.pattern);
		}
	});
	if (def.pattern) (_a$2 = inst._zod).check ?? (_a$2.check = (payload) => {
		def.pattern.lastIndex = 0;
		if (def.pattern.test(payload.value)) return;
		payload.issues.push({
			origin: "string",
			code: "invalid_format",
			format: def.format,
			input: payload.value,
			...def.pattern ? { pattern: def.pattern.toString() } : {},
			inst,
			continue: !def.abort
		});
	});
	else (_b = inst._zod).check ?? (_b.check = () => {});
});
const $ZodCheckRegex = /* @__PURE__ */ $constructor("$ZodCheckRegex", (inst, def) => {
	$ZodCheckStringFormat.init(inst, def);
	inst._zod.check = (payload) => {
		def.pattern.lastIndex = 0;
		if (def.pattern.test(payload.value)) return;
		payload.issues.push({
			origin: "string",
			code: "invalid_format",
			format: "regex",
			input: payload.value,
			pattern: def.pattern.toString(),
			inst,
			continue: !def.abort
		});
	};
});
const $ZodCheckLowerCase = /* @__PURE__ */ $constructor("$ZodCheckLowerCase", (inst, def) => {
	def.pattern ?? (def.pattern = lowercase);
	$ZodCheckStringFormat.init(inst, def);
});
const $ZodCheckUpperCase = /* @__PURE__ */ $constructor("$ZodCheckUpperCase", (inst, def) => {
	def.pattern ?? (def.pattern = uppercase);
	$ZodCheckStringFormat.init(inst, def);
});
const $ZodCheckIncludes = /* @__PURE__ */ $constructor("$ZodCheckIncludes", (inst, def) => {
	$ZodCheck.init(inst, def);
	const escapedRegex = escapeRegex(def.includes);
	const pattern = new RegExp(typeof def.position === "number" ? `^.{${def.position}}${escapedRegex}` : escapedRegex);
	def.pattern = pattern;
	inst._zod.onattach.push((inst$1) => {
		const bag = inst$1._zod.bag;
		bag.patterns ?? (bag.patterns = /* @__PURE__ */ new Set());
		bag.patterns.add(pattern);
	});
	inst._zod.check = (payload) => {
		if (payload.value.includes(def.includes, def.position)) return;
		payload.issues.push({
			origin: "string",
			code: "invalid_format",
			format: "includes",
			includes: def.includes,
			input: payload.value,
			inst,
			continue: !def.abort
		});
	};
});
const $ZodCheckStartsWith = /* @__PURE__ */ $constructor("$ZodCheckStartsWith", (inst, def) => {
	$ZodCheck.init(inst, def);
	const pattern = /* @__PURE__ */ new RegExp(`^${escapeRegex(def.prefix)}.*`);
	def.pattern ?? (def.pattern = pattern);
	inst._zod.onattach.push((inst$1) => {
		const bag = inst$1._zod.bag;
		bag.patterns ?? (bag.patterns = /* @__PURE__ */ new Set());
		bag.patterns.add(pattern);
	});
	inst._zod.check = (payload) => {
		if (payload.value.startsWith(def.prefix)) return;
		payload.issues.push({
			origin: "string",
			code: "invalid_format",
			format: "starts_with",
			prefix: def.prefix,
			input: payload.value,
			inst,
			continue: !def.abort
		});
	};
});
const $ZodCheckEndsWith = /* @__PURE__ */ $constructor("$ZodCheckEndsWith", (inst, def) => {
	$ZodCheck.init(inst, def);
	const pattern = /* @__PURE__ */ new RegExp(`.*${escapeRegex(def.suffix)}$`);
	def.pattern ?? (def.pattern = pattern);
	inst._zod.onattach.push((inst$1) => {
		const bag = inst$1._zod.bag;
		bag.patterns ?? (bag.patterns = /* @__PURE__ */ new Set());
		bag.patterns.add(pattern);
	});
	inst._zod.check = (payload) => {
		if (payload.value.endsWith(def.suffix)) return;
		payload.issues.push({
			origin: "string",
			code: "invalid_format",
			format: "ends_with",
			suffix: def.suffix,
			input: payload.value,
			inst,
			continue: !def.abort
		});
	};
});
const $ZodCheckOverwrite = /* @__PURE__ */ $constructor("$ZodCheckOverwrite", (inst, def) => {
	$ZodCheck.init(inst, def);
	inst._zod.check = (payload) => {
		payload.value = def.tx(payload.value);
	};
});
var Doc = class {
	constructor(args = []) {
		this.content = [];
		this.indent = 0;
		if (this) this.args = args;
	}
	indented(fn) {
		this.indent += 1;
		fn(this);
		this.indent -= 1;
	}
	write(arg) {
		if (typeof arg === "function") {
			arg(this, { execution: "sync" });
			arg(this, { execution: "async" });
			return;
		}
		const lines = arg.split("\n").filter((x) => x);
		const minIndent = Math.min(...lines.map((x) => x.length - x.trimStart().length));
		const dedented = lines.map((x) => x.slice(minIndent)).map((x) => " ".repeat(this.indent * 2) + x);
		for (const line of dedented) this.content.push(line);
	}
	compile() {
		const F = Function;
		const args = this?.args;
		const lines = [...(this?.content ?? [``]).map((x) => `  ${x}`)];
		return new F(...args, lines.join("\n"));
	}
};
const version = {
	major: 4,
	minor: 4,
	patch: 3
};
const $ZodType = /* @__PURE__ */ $constructor("$ZodType", (inst, def) => {
	var _a$2;
	inst ?? (inst = {});
	inst._zod.def = def;
	inst._zod.bag = inst._zod.bag || {};
	inst._zod.version = version;
	const checks = [...inst._zod.def.checks ?? []];
	if (inst._zod.traits.has("$ZodCheck")) checks.unshift(inst);
	for (const ch of checks) for (const fn of ch._zod.onattach) fn(inst);
	if (checks.length === 0) {
		(_a$2 = inst._zod).deferred ?? (_a$2.deferred = []);
		inst._zod.deferred?.push(() => {
			inst._zod.run = inst._zod.parse;
		});
	} else {
		const runChecks = (payload, checks$1, ctx) => {
			let isAborted = aborted(payload);
			let asyncResult;
			for (const ch of checks$1) {
				if (ch._zod.def.when) {
					if (explicitlyAborted(payload)) continue;
					if (!ch._zod.def.when(payload)) continue;
				} else if (isAborted) continue;
				const currLen = payload.issues.length;
				const _ = ch._zod.check(payload);
				if (_ instanceof Promise && ctx?.async === false) throw new $ZodAsyncError();
				if (asyncResult || _ instanceof Promise) asyncResult = (asyncResult ?? Promise.resolve()).then(async () => {
					await _;
					if (payload.issues.length === currLen) return;
					if (!isAborted) isAborted = aborted(payload, currLen);
				});
				else {
					if (payload.issues.length === currLen) continue;
					if (!isAborted) isAborted = aborted(payload, currLen);
				}
			}
			if (asyncResult) return asyncResult.then(() => {
				return payload;
			});
			return payload;
		};
		const handleCanaryResult = (canary, payload, ctx) => {
			if (aborted(canary)) {
				canary.aborted = true;
				return canary;
			}
			const checkResult = runChecks(payload, checks, ctx);
			if (checkResult instanceof Promise) {
				if (ctx.async === false) throw new $ZodAsyncError();
				return checkResult.then((checkResult$1) => inst._zod.parse(checkResult$1, ctx));
			}
			return inst._zod.parse(checkResult, ctx);
		};
		inst._zod.run = (payload, ctx) => {
			if (ctx.skipChecks) return inst._zod.parse(payload, ctx);
			if (ctx.direction === "backward") {
				const canary = inst._zod.parse({
					value: payload.value,
					issues: []
				}, {
					...ctx,
					skipChecks: true
				});
				if (canary instanceof Promise) return canary.then((canary$1) => {
					return handleCanaryResult(canary$1, payload, ctx);
				});
				return handleCanaryResult(canary, payload, ctx);
			}
			const result = inst._zod.parse(payload, ctx);
			if (result instanceof Promise) {
				if (ctx.async === false) throw new $ZodAsyncError();
				return result.then((result$1) => runChecks(result$1, checks, ctx));
			}
			return runChecks(result, checks, ctx);
		};
	}
	defineLazy(inst, "~standard", () => ({
		validate: (value) => {
			try {
				const r = safeParse$1(inst, value);
				return r.success ? { value: r.data } : { issues: r.error?.issues };
			} catch (_) {
				return safeParseAsync$1(inst, value).then((r) => r.success ? { value: r.data } : { issues: r.error?.issues });
			}
		},
		vendor: "zod",
		version: 1
	}));
});
const $ZodString = /* @__PURE__ */ $constructor("$ZodString", (inst, def) => {
	$ZodType.init(inst, def);
	inst._zod.pattern = [...inst?._zod.bag?.patterns ?? []].pop() ?? string$1(inst._zod.bag);
	inst._zod.parse = (payload, _) => {
		if (def.coerce) try {
			payload.value = String(payload.value);
		} catch (_$1) {}
		if (typeof payload.value === "string") return payload;
		payload.issues.push({
			expected: "string",
			code: "invalid_type",
			input: payload.value,
			inst
		});
		return payload;
	};
});
const $ZodStringFormat = /* @__PURE__ */ $constructor("$ZodStringFormat", (inst, def) => {
	$ZodCheckStringFormat.init(inst, def);
	$ZodString.init(inst, def);
});
const $ZodGUID = /* @__PURE__ */ $constructor("$ZodGUID", (inst, def) => {
	def.pattern ?? (def.pattern = guid);
	$ZodStringFormat.init(inst, def);
});
const $ZodUUID = /* @__PURE__ */ $constructor("$ZodUUID", (inst, def) => {
	if (def.version) {
		const v = {
			v1: 1,
			v2: 2,
			v3: 3,
			v4: 4,
			v5: 5,
			v6: 6,
			v7: 7,
			v8: 8
		}[def.version];
		if (v === void 0) throw new Error(`Invalid UUID version: "${def.version}"`);
		def.pattern ?? (def.pattern = uuid$1(v));
	} else def.pattern ?? (def.pattern = uuid$1());
	$ZodStringFormat.init(inst, def);
});
const $ZodEmail = /* @__PURE__ */ $constructor("$ZodEmail", (inst, def) => {
	def.pattern ?? (def.pattern = email);
	$ZodStringFormat.init(inst, def);
});
const $ZodURL = /* @__PURE__ */ $constructor("$ZodURL", (inst, def) => {
	$ZodStringFormat.init(inst, def);
	inst._zod.check = (payload) => {
		try {
			const trimmed = payload.value.trim();
			if (!def.normalize && def.protocol?.source === httpProtocol.source) {
				if (!/^https?:\/\//i.test(trimmed)) {
					payload.issues.push({
						code: "invalid_format",
						format: "url",
						note: "Invalid URL format",
						input: payload.value,
						inst,
						continue: !def.abort
					});
					return;
				}
			}
			const url = new URL(trimmed);
			if (def.hostname) {
				def.hostname.lastIndex = 0;
				if (!def.hostname.test(url.hostname)) payload.issues.push({
					code: "invalid_format",
					format: "url",
					note: "Invalid hostname",
					pattern: def.hostname.source,
					input: payload.value,
					inst,
					continue: !def.abort
				});
			}
			if (def.protocol) {
				def.protocol.lastIndex = 0;
				if (!def.protocol.test(url.protocol.endsWith(":") ? url.protocol.slice(0, -1) : url.protocol)) payload.issues.push({
					code: "invalid_format",
					format: "url",
					note: "Invalid protocol",
					pattern: def.protocol.source,
					input: payload.value,
					inst,
					continue: !def.abort
				});
			}
			if (def.normalize) payload.value = url.href;
			else payload.value = trimmed;
			return;
		} catch (_) {
			payload.issues.push({
				code: "invalid_format",
				format: "url",
				input: payload.value,
				inst,
				continue: !def.abort
			});
		}
	};
});
const $ZodEmoji = /* @__PURE__ */ $constructor("$ZodEmoji", (inst, def) => {
	def.pattern ?? (def.pattern = emoji());
	$ZodStringFormat.init(inst, def);
});
const $ZodNanoID = /* @__PURE__ */ $constructor("$ZodNanoID", (inst, def) => {
	def.pattern ?? (def.pattern = nanoid);
	$ZodStringFormat.init(inst, def);
});
const $ZodCUID = /* @__PURE__ */ $constructor("$ZodCUID", (inst, def) => {
	def.pattern ?? (def.pattern = cuid);
	$ZodStringFormat.init(inst, def);
});
const $ZodCUID2 = /* @__PURE__ */ $constructor("$ZodCUID2", (inst, def) => {
	def.pattern ?? (def.pattern = cuid2);
	$ZodStringFormat.init(inst, def);
});
const $ZodULID = /* @__PURE__ */ $constructor("$ZodULID", (inst, def) => {
	def.pattern ?? (def.pattern = ulid);
	$ZodStringFormat.init(inst, def);
});
const $ZodXID = /* @__PURE__ */ $constructor("$ZodXID", (inst, def) => {
	def.pattern ?? (def.pattern = xid);
	$ZodStringFormat.init(inst, def);
});
const $ZodKSUID = /* @__PURE__ */ $constructor("$ZodKSUID", (inst, def) => {
	def.pattern ?? (def.pattern = ksuid);
	$ZodStringFormat.init(inst, def);
});
const $ZodISODateTime = /* @__PURE__ */ $constructor("$ZodISODateTime", (inst, def) => {
	def.pattern ?? (def.pattern = datetime$1(def));
	$ZodStringFormat.init(inst, def);
});
const $ZodISODate = /* @__PURE__ */ $constructor("$ZodISODate", (inst, def) => {
	def.pattern ?? (def.pattern = date$1);
	$ZodStringFormat.init(inst, def);
});
const $ZodISOTime = /* @__PURE__ */ $constructor("$ZodISOTime", (inst, def) => {
	def.pattern ?? (def.pattern = time$1(def));
	$ZodStringFormat.init(inst, def);
});
const $ZodISODuration = /* @__PURE__ */ $constructor("$ZodISODuration", (inst, def) => {
	def.pattern ?? (def.pattern = duration$1);
	$ZodStringFormat.init(inst, def);
});
const $ZodIPv4 = /* @__PURE__ */ $constructor("$ZodIPv4", (inst, def) => {
	def.pattern ?? (def.pattern = ipv4);
	$ZodStringFormat.init(inst, def);
	inst._zod.bag.format = `ipv4`;
});
const $ZodIPv6 = /* @__PURE__ */ $constructor("$ZodIPv6", (inst, def) => {
	def.pattern ?? (def.pattern = ipv6);
	$ZodStringFormat.init(inst, def);
	inst._zod.bag.format = `ipv6`;
	inst._zod.check = (payload) => {
		try {
			new URL(`http://[${payload.value}]`);
		} catch {
			payload.issues.push({
				code: "invalid_format",
				format: "ipv6",
				input: payload.value,
				inst,
				continue: !def.abort
			});
		}
	};
});
const $ZodCIDRv4 = /* @__PURE__ */ $constructor("$ZodCIDRv4", (inst, def) => {
	def.pattern ?? (def.pattern = cidrv4);
	$ZodStringFormat.init(inst, def);
});
const $ZodCIDRv6 = /* @__PURE__ */ $constructor("$ZodCIDRv6", (inst, def) => {
	def.pattern ?? (def.pattern = cidrv6);
	$ZodStringFormat.init(inst, def);
	inst._zod.check = (payload) => {
		const parts = payload.value.split("/");
		try {
			if (parts.length !== 2) throw new Error();
			const [address, prefix] = parts;
			if (!prefix) throw new Error();
			const prefixNum = Number(prefix);
			if (`${prefixNum}` !== prefix) throw new Error();
			if (prefixNum < 0 || prefixNum > 128) throw new Error();
			new URL(`http://[${address}]`);
		} catch {
			payload.issues.push({
				code: "invalid_format",
				format: "cidrv6",
				input: payload.value,
				inst,
				continue: !def.abort
			});
		}
	};
});
function isValidBase64(data) {
	if (data === "") return true;
	if (/\s/.test(data)) return false;
	if (data.length % 4 !== 0) return false;
	try {
		atob(data);
		return true;
	} catch {
		return false;
	}
}
const $ZodBase64 = /* @__PURE__ */ $constructor("$ZodBase64", (inst, def) => {
	def.pattern ?? (def.pattern = base64);
	$ZodStringFormat.init(inst, def);
	inst._zod.bag.contentEncoding = "base64";
	inst._zod.check = (payload) => {
		if (isValidBase64(payload.value)) return;
		payload.issues.push({
			code: "invalid_format",
			format: "base64",
			input: payload.value,
			inst,
			continue: !def.abort
		});
	};
});
function isValidBase64URL(data) {
	if (!base64url.test(data)) return false;
	const base64$1 = data.replace(/[-_]/g, (c) => c === "-" ? "+" : "/");
	return isValidBase64(base64$1.padEnd(Math.ceil(base64$1.length / 4) * 4, "="));
}
const $ZodBase64URL = /* @__PURE__ */ $constructor("$ZodBase64URL", (inst, def) => {
	def.pattern ?? (def.pattern = base64url);
	$ZodStringFormat.init(inst, def);
	inst._zod.bag.contentEncoding = "base64url";
	inst._zod.check = (payload) => {
		if (isValidBase64URL(payload.value)) return;
		payload.issues.push({
			code: "invalid_format",
			format: "base64url",
			input: payload.value,
			inst,
			continue: !def.abort
		});
	};
});
const $ZodE164 = /* @__PURE__ */ $constructor("$ZodE164", (inst, def) => {
	def.pattern ?? (def.pattern = e164);
	$ZodStringFormat.init(inst, def);
});
function isValidJWT(token, algorithm = null) {
	try {
		const tokensParts = token.split(".");
		if (tokensParts.length !== 3) return false;
		const [header] = tokensParts;
		if (!header) return false;
		const parsedHeader = JSON.parse(atob(header));
		if ("typ" in parsedHeader && parsedHeader?.typ !== "JWT") return false;
		if (!parsedHeader.alg) return false;
		if (algorithm && (!("alg" in parsedHeader) || parsedHeader.alg !== algorithm)) return false;
		return true;
	} catch {
		return false;
	}
}
const $ZodJWT = /* @__PURE__ */ $constructor("$ZodJWT", (inst, def) => {
	$ZodStringFormat.init(inst, def);
	inst._zod.check = (payload) => {
		if (isValidJWT(payload.value, def.alg)) return;
		payload.issues.push({
			code: "invalid_format",
			format: "jwt",
			input: payload.value,
			inst,
			continue: !def.abort
		});
	};
});
const $ZodNumber = /* @__PURE__ */ $constructor("$ZodNumber", (inst, def) => {
	$ZodType.init(inst, def);
	inst._zod.pattern = inst._zod.bag.pattern ?? number$1;
	inst._zod.parse = (payload, _ctx) => {
		if (def.coerce) try {
			payload.value = Number(payload.value);
		} catch (_) {}
		const input = payload.value;
		if (typeof input === "number" && !Number.isNaN(input) && Number.isFinite(input)) return payload;
		const received = typeof input === "number" ? Number.isNaN(input) ? "NaN" : !Number.isFinite(input) ? "Infinity" : void 0 : void 0;
		payload.issues.push({
			expected: "number",
			code: "invalid_type",
			input,
			inst,
			...received ? { received } : {}
		});
		return payload;
	};
});
const $ZodNumberFormat = /* @__PURE__ */ $constructor("$ZodNumberFormat", (inst, def) => {
	$ZodCheckNumberFormat.init(inst, def);
	$ZodNumber.init(inst, def);
});
const $ZodBoolean = /* @__PURE__ */ $constructor("$ZodBoolean", (inst, def) => {
	$ZodType.init(inst, def);
	inst._zod.pattern = boolean$1;
	inst._zod.parse = (payload, _ctx) => {
		if (def.coerce) try {
			payload.value = Boolean(payload.value);
		} catch (_) {}
		const input = payload.value;
		if (typeof input === "boolean") return payload;
		payload.issues.push({
			expected: "boolean",
			code: "invalid_type",
			input,
			inst
		});
		return payload;
	};
});
const $ZodNull = /* @__PURE__ */ $constructor("$ZodNull", (inst, def) => {
	$ZodType.init(inst, def);
	inst._zod.pattern = _null$2;
	inst._zod.values = new Set([null]);
	inst._zod.parse = (payload, _ctx) => {
		const input = payload.value;
		if (input === null) return payload;
		payload.issues.push({
			expected: "null",
			code: "invalid_type",
			input,
			inst
		});
		return payload;
	};
});
const $ZodUnknown = /* @__PURE__ */ $constructor("$ZodUnknown", (inst, def) => {
	$ZodType.init(inst, def);
	inst._zod.parse = (payload) => payload;
});
const $ZodNever = /* @__PURE__ */ $constructor("$ZodNever", (inst, def) => {
	$ZodType.init(inst, def);
	inst._zod.parse = (payload, _ctx) => {
		payload.issues.push({
			expected: "never",
			code: "invalid_type",
			input: payload.value,
			inst
		});
		return payload;
	};
});
function handleArrayResult(result, final, index) {
	if (result.issues.length) final.issues.push(...prefixIssues(index, result.issues));
	final.value[index] = result.value;
}
const $ZodArray = /* @__PURE__ */ $constructor("$ZodArray", (inst, def) => {
	$ZodType.init(inst, def);
	inst._zod.parse = (payload, ctx) => {
		const input = payload.value;
		if (!Array.isArray(input)) {
			payload.issues.push({
				expected: "array",
				code: "invalid_type",
				input,
				inst
			});
			return payload;
		}
		payload.value = Array(input.length);
		const proms = [];
		for (let i = 0; i < input.length; i++) {
			const item = input[i];
			const result = def.element._zod.run({
				value: item,
				issues: []
			}, ctx);
			if (result instanceof Promise) proms.push(result.then((result$1) => handleArrayResult(result$1, payload, i)));
			else handleArrayResult(result, payload, i);
		}
		if (proms.length) return Promise.all(proms).then(() => payload);
		return payload;
	};
});
function handlePropertyResult(result, final, key, input, isOptionalIn, isOptionalOut) {
	const isPresent = key in input;
	if (result.issues.length) {
		if (isOptionalIn && isOptionalOut && !isPresent) return;
		final.issues.push(...prefixIssues(key, result.issues));
	}
	if (!isPresent && !isOptionalIn) {
		if (!result.issues.length) final.issues.push({
			code: "invalid_type",
			expected: "nonoptional",
			input: void 0,
			path: [key]
		});
		return;
	}
	if (result.value === void 0) {
		if (isPresent) final.value[key] = void 0;
	} else final.value[key] = result.value;
}
function normalizeDef(def) {
	const keys = Object.keys(def.shape);
	for (const k of keys) if (!def.shape?.[k]?._zod?.traits?.has("$ZodType")) throw new Error(`Invalid element at key "${k}": expected a Zod schema`);
	const okeys = optionalKeys(def.shape);
	return {
		...def,
		keys,
		keySet: new Set(keys),
		numKeys: keys.length,
		optionalKeys: new Set(okeys)
	};
}
function handleCatchall(proms, input, payload, ctx, def, inst) {
	const unrecognized = [];
	const keySet = def.keySet;
	const _catchall = def.catchall._zod;
	const t = _catchall.def.type;
	const isOptionalIn = _catchall.optin === "optional";
	const isOptionalOut = _catchall.optout === "optional";
	for (const key in input) {
		if (key === "__proto__") continue;
		if (keySet.has(key)) continue;
		if (t === "never") {
			unrecognized.push(key);
			continue;
		}
		const r = _catchall.run({
			value: input[key],
			issues: []
		}, ctx);
		if (r instanceof Promise) proms.push(r.then((r$1) => handlePropertyResult(r$1, payload, key, input, isOptionalIn, isOptionalOut)));
		else handlePropertyResult(r, payload, key, input, isOptionalIn, isOptionalOut);
	}
	if (unrecognized.length) payload.issues.push({
		code: "unrecognized_keys",
		keys: unrecognized,
		input,
		inst
	});
	if (!proms.length) return payload;
	return Promise.all(proms).then(() => {
		return payload;
	});
}
const $ZodObject = /* @__PURE__ */ $constructor("$ZodObject", (inst, def) => {
	$ZodType.init(inst, def);
	if (!Object.getOwnPropertyDescriptor(def, "shape")?.get) {
		const sh = def.shape;
		Object.defineProperty(def, "shape", { get: () => {
			const newSh = { ...sh };
			Object.defineProperty(def, "shape", { value: newSh });
			return newSh;
		} });
	}
	const _normalized = cached(() => normalizeDef(def));
	defineLazy(inst._zod, "propValues", () => {
		const shape = def.shape;
		const propValues = {};
		for (const key in shape) {
			const field = shape[key]._zod;
			if (field.values) {
				propValues[key] ?? (propValues[key] = /* @__PURE__ */ new Set());
				for (const v of field.values) propValues[key].add(v);
			}
		}
		return propValues;
	});
	const isObject$1 = isObject;
	const catchall = def.catchall;
	let value;
	inst._zod.parse = (payload, ctx) => {
		value ?? (value = _normalized.value);
		const input = payload.value;
		if (!isObject$1(input)) {
			payload.issues.push({
				expected: "object",
				code: "invalid_type",
				input,
				inst
			});
			return payload;
		}
		payload.value = {};
		const proms = [];
		const shape = value.shape;
		for (const key of value.keys) {
			const el = shape[key];
			const isOptionalIn = el._zod.optin === "optional";
			const isOptionalOut = el._zod.optout === "optional";
			const r = el._zod.run({
				value: input[key],
				issues: []
			}, ctx);
			if (r instanceof Promise) proms.push(r.then((r$1) => handlePropertyResult(r$1, payload, key, input, isOptionalIn, isOptionalOut)));
			else handlePropertyResult(r, payload, key, input, isOptionalIn, isOptionalOut);
		}
		if (!catchall) return proms.length ? Promise.all(proms).then(() => payload) : payload;
		return handleCatchall(proms, input, payload, ctx, _normalized.value, inst);
	};
});
const $ZodObjectJIT = /* @__PURE__ */ $constructor("$ZodObjectJIT", (inst, def) => {
	$ZodObject.init(inst, def);
	const superParse = inst._zod.parse;
	const _normalized = cached(() => normalizeDef(def));
	const generateFastpass = (shape) => {
		const doc = new Doc([
			"shape",
			"payload",
			"ctx"
		]);
		const normalized = _normalized.value;
		const parseStr = (key) => {
			const k = esc(key);
			return `shape[${k}]._zod.run({ value: input[${k}], issues: [] }, ctx)`;
		};
		doc.write(`const input = payload.value;`);
		const ids = Object.create(null);
		let counter = 0;
		for (const key of normalized.keys) ids[key] = `key_${counter++}`;
		doc.write(`const newResult = {};`);
		for (const key of normalized.keys) {
			const id = ids[key];
			const k = esc(key);
			const schema = shape[key];
			const isOptionalIn = schema?._zod?.optin === "optional";
			const isOptionalOut = schema?._zod?.optout === "optional";
			doc.write(`const ${id} = ${parseStr(key)};`);
			if (isOptionalIn && isOptionalOut) doc.write(`
        if (${id}.issues.length) {
          if (${k} in input) {
            payload.issues = payload.issues.concat(${id}.issues.map(iss => ({
              ...iss,
              path: iss.path ? [${k}, ...iss.path] : [${k}]
            })));
          }
        }
        
        if (${id}.value === undefined) {
          if (${k} in input) {
            newResult[${k}] = undefined;
          }
        } else {
          newResult[${k}] = ${id}.value;
        }
        
      `);
			else if (!isOptionalIn) doc.write(`
        const ${id}_present = ${k} in input;
        if (${id}.issues.length) {
          payload.issues = payload.issues.concat(${id}.issues.map(iss => ({
            ...iss,
            path: iss.path ? [${k}, ...iss.path] : [${k}]
          })));
        }
        if (!${id}_present && !${id}.issues.length) {
          payload.issues.push({
            code: "invalid_type",
            expected: "nonoptional",
            input: undefined,
            path: [${k}]
          });
        }

        if (${id}_present) {
          if (${id}.value === undefined) {
            newResult[${k}] = undefined;
          } else {
            newResult[${k}] = ${id}.value;
          }
        }

      `);
			else doc.write(`
        if (${id}.issues.length) {
          payload.issues = payload.issues.concat(${id}.issues.map(iss => ({
            ...iss,
            path: iss.path ? [${k}, ...iss.path] : [${k}]
          })));
        }
        
        if (${id}.value === undefined) {
          if (${k} in input) {
            newResult[${k}] = undefined;
          }
        } else {
          newResult[${k}] = ${id}.value;
        }
        
      `);
		}
		doc.write(`payload.value = newResult;`);
		doc.write(`return payload;`);
		const fn = doc.compile();
		return (payload, ctx) => fn(shape, payload, ctx);
	};
	let fastpass;
	const isObject$1 = isObject;
	const jit = !globalConfig.jitless;
	const fastEnabled = jit && allowsEval.value;
	const catchall = def.catchall;
	let value;
	inst._zod.parse = (payload, ctx) => {
		value ?? (value = _normalized.value);
		const input = payload.value;
		if (!isObject$1(input)) {
			payload.issues.push({
				expected: "object",
				code: "invalid_type",
				input,
				inst
			});
			return payload;
		}
		if (jit && fastEnabled && ctx?.async === false && ctx.jitless !== true) {
			if (!fastpass) fastpass = generateFastpass(def.shape);
			payload = fastpass(payload, ctx);
			if (!catchall) return payload;
			return handleCatchall([], input, payload, ctx, value, inst);
		}
		return superParse(payload, ctx);
	};
});
function handleUnionResults(results, final, inst, ctx) {
	for (const result of results) if (result.issues.length === 0) {
		final.value = result.value;
		return final;
	}
	const nonaborted = results.filter((r) => !aborted(r));
	if (nonaborted.length === 1) {
		final.value = nonaborted[0].value;
		return nonaborted[0];
	}
	final.issues.push({
		code: "invalid_union",
		input: final.value,
		inst,
		errors: results.map((result) => result.issues.map((iss) => finalizeIssue(iss, ctx, config())))
	});
	return final;
}
const $ZodUnion = /* @__PURE__ */ $constructor("$ZodUnion", (inst, def) => {
	$ZodType.init(inst, def);
	defineLazy(inst._zod, "optin", () => def.options.some((o) => o._zod.optin === "optional") ? "optional" : void 0);
	defineLazy(inst._zod, "optout", () => def.options.some((o) => o._zod.optout === "optional") ? "optional" : void 0);
	defineLazy(inst._zod, "values", () => {
		if (def.options.every((o) => o._zod.values)) return new Set(def.options.flatMap((option) => Array.from(option._zod.values)));
	});
	defineLazy(inst._zod, "pattern", () => {
		if (def.options.every((o) => o._zod.pattern)) {
			const patterns = def.options.map((o) => o._zod.pattern);
			return /* @__PURE__ */ new RegExp(`^(${patterns.map((p) => cleanRegex(p.source)).join("|")})$`);
		}
	});
	const first = def.options.length === 1 ? def.options[0]._zod.run : null;
	inst._zod.parse = (payload, ctx) => {
		if (first) return first(payload, ctx);
		let async = false;
		const results = [];
		for (const option of def.options) {
			const result = option._zod.run({
				value: payload.value,
				issues: []
			}, ctx);
			if (result instanceof Promise) {
				results.push(result);
				async = true;
			} else {
				if (result.issues.length === 0) return result;
				results.push(result);
			}
		}
		if (!async) return handleUnionResults(results, payload, inst, ctx);
		return Promise.all(results).then((results$1) => {
			return handleUnionResults(results$1, payload, inst, ctx);
		});
	};
});
const $ZodDiscriminatedUnion = /* @__PURE__ */ $constructor("$ZodDiscriminatedUnion", (inst, def) => {
	def.inclusive = false;
	$ZodUnion.init(inst, def);
	const _super = inst._zod.parse;
	defineLazy(inst._zod, "propValues", () => {
		const propValues = {};
		for (const option of def.options) {
			const pv = option._zod.propValues;
			if (!pv || Object.keys(pv).length === 0) throw new Error(`Invalid discriminated union option at index "${def.options.indexOf(option)}"`);
			for (const [k, v] of Object.entries(pv)) {
				if (!propValues[k]) propValues[k] = /* @__PURE__ */ new Set();
				for (const val of v) propValues[k].add(val);
			}
		}
		return propValues;
	});
	const disc = cached(() => {
		const opts = def.options;
		const map = /* @__PURE__ */ new Map();
		for (const o of opts) {
			const values = o._zod.propValues?.[def.discriminator];
			if (!values || values.size === 0) throw new Error(`Invalid discriminated union option at index "${def.options.indexOf(o)}"`);
			for (const v of values) {
				if (map.has(v)) throw new Error(`Duplicate discriminator value "${String(v)}"`);
				map.set(v, o);
			}
		}
		return map;
	});
	inst._zod.parse = (payload, ctx) => {
		const input = payload.value;
		if (!isObject(input)) {
			payload.issues.push({
				code: "invalid_type",
				expected: "object",
				input,
				inst
			});
			return payload;
		}
		const opt = disc.value.get(input?.[def.discriminator]);
		if (opt) return opt._zod.run(payload, ctx);
		if (def.unionFallback || ctx.direction === "backward") return _super(payload, ctx);
		payload.issues.push({
			code: "invalid_union",
			errors: [],
			note: "No matching discriminator",
			discriminator: def.discriminator,
			options: Array.from(disc.value.keys()),
			input,
			path: [def.discriminator],
			inst
		});
		return payload;
	};
});
const $ZodIntersection = /* @__PURE__ */ $constructor("$ZodIntersection", (inst, def) => {
	$ZodType.init(inst, def);
	inst._zod.parse = (payload, ctx) => {
		const input = payload.value;
		const left = def.left._zod.run({
			value: input,
			issues: []
		}, ctx);
		const right = def.right._zod.run({
			value: input,
			issues: []
		}, ctx);
		if (left instanceof Promise || right instanceof Promise) return Promise.all([left, right]).then(([left$1, right$1]) => {
			return handleIntersectionResults(payload, left$1, right$1);
		});
		return handleIntersectionResults(payload, left, right);
	};
});
function mergeValues(a, b) {
	if (a === b) return {
		valid: true,
		data: a
	};
	if (a instanceof Date && b instanceof Date && +a === +b) return {
		valid: true,
		data: a
	};
	if (isPlainObject(a) && isPlainObject(b)) {
		const bKeys = Object.keys(b);
		const sharedKeys = Object.keys(a).filter((key) => bKeys.indexOf(key) !== -1);
		const newObj = {
			...a,
			...b
		};
		for (const key of sharedKeys) {
			const sharedValue = mergeValues(a[key], b[key]);
			if (!sharedValue.valid) return {
				valid: false,
				mergeErrorPath: [key, ...sharedValue.mergeErrorPath]
			};
			newObj[key] = sharedValue.data;
		}
		return {
			valid: true,
			data: newObj
		};
	}
	if (Array.isArray(a) && Array.isArray(b)) {
		if (a.length !== b.length) return {
			valid: false,
			mergeErrorPath: []
		};
		const newArray = [];
		for (let index = 0; index < a.length; index++) {
			const itemA = a[index];
			const itemB = b[index];
			const sharedValue = mergeValues(itemA, itemB);
			if (!sharedValue.valid) return {
				valid: false,
				mergeErrorPath: [index, ...sharedValue.mergeErrorPath]
			};
			newArray.push(sharedValue.data);
		}
		return {
			valid: true,
			data: newArray
		};
	}
	return {
		valid: false,
		mergeErrorPath: []
	};
}
function handleIntersectionResults(result, left, right) {
	const unrecKeys = /* @__PURE__ */ new Map();
	let unrecIssue;
	for (const iss of left.issues) if (iss.code === "unrecognized_keys") {
		unrecIssue ?? (unrecIssue = iss);
		for (const k of iss.keys) {
			if (!unrecKeys.has(k)) unrecKeys.set(k, {});
			unrecKeys.get(k).l = true;
		}
	} else result.issues.push(iss);
	for (const iss of right.issues) if (iss.code === "unrecognized_keys") for (const k of iss.keys) {
		if (!unrecKeys.has(k)) unrecKeys.set(k, {});
		unrecKeys.get(k).r = true;
	}
	else result.issues.push(iss);
	const bothKeys = [...unrecKeys].filter(([, f]) => f.l && f.r).map(([k]) => k);
	if (bothKeys.length && unrecIssue) result.issues.push({
		...unrecIssue,
		keys: bothKeys
	});
	if (aborted(result)) return result;
	const merged = mergeValues(left.value, right.value);
	if (!merged.valid) throw new Error(`Unmergable intersection. Error path: ${JSON.stringify(merged.mergeErrorPath)}`);
	result.value = merged.data;
	return result;
}
const $ZodRecord = /* @__PURE__ */ $constructor("$ZodRecord", (inst, def) => {
	$ZodType.init(inst, def);
	inst._zod.parse = (payload, ctx) => {
		const input = payload.value;
		if (!isPlainObject(input)) {
			payload.issues.push({
				expected: "record",
				code: "invalid_type",
				input,
				inst
			});
			return payload;
		}
		const proms = [];
		const values = def.keyType._zod.values;
		if (values) {
			payload.value = {};
			const recordKeys = /* @__PURE__ */ new Set();
			for (const key of values) if (typeof key === "string" || typeof key === "number" || typeof key === "symbol") {
				recordKeys.add(typeof key === "number" ? key.toString() : key);
				const keyResult = def.keyType._zod.run({
					value: key,
					issues: []
				}, ctx);
				if (keyResult instanceof Promise) throw new Error("Async schemas not supported in object keys currently");
				if (keyResult.issues.length) {
					payload.issues.push({
						code: "invalid_key",
						origin: "record",
						issues: keyResult.issues.map((iss) => finalizeIssue(iss, ctx, config())),
						input: key,
						path: [key],
						inst
					});
					continue;
				}
				const outKey = keyResult.value;
				const result = def.valueType._zod.run({
					value: input[key],
					issues: []
				}, ctx);
				if (result instanceof Promise) proms.push(result.then((result$1) => {
					if (result$1.issues.length) payload.issues.push(...prefixIssues(key, result$1.issues));
					payload.value[outKey] = result$1.value;
				}));
				else {
					if (result.issues.length) payload.issues.push(...prefixIssues(key, result.issues));
					payload.value[outKey] = result.value;
				}
			}
			let unrecognized;
			for (const key in input) if (!recordKeys.has(key)) {
				unrecognized = unrecognized ?? [];
				unrecognized.push(key);
			}
			if (unrecognized && unrecognized.length > 0) payload.issues.push({
				code: "unrecognized_keys",
				input,
				inst,
				keys: unrecognized
			});
		} else {
			payload.value = {};
			for (const key of Reflect.ownKeys(input)) {
				if (key === "__proto__") continue;
				if (!Object.prototype.propertyIsEnumerable.call(input, key)) continue;
				let keyResult = def.keyType._zod.run({
					value: key,
					issues: []
				}, ctx);
				if (keyResult instanceof Promise) throw new Error("Async schemas not supported in object keys currently");
				if (typeof key === "string" && number$1.test(key) && keyResult.issues.length) {
					const retryResult = def.keyType._zod.run({
						value: Number(key),
						issues: []
					}, ctx);
					if (retryResult instanceof Promise) throw new Error("Async schemas not supported in object keys currently");
					if (retryResult.issues.length === 0) keyResult = retryResult;
				}
				if (keyResult.issues.length) {
					if (def.mode === "loose") payload.value[key] = input[key];
					else payload.issues.push({
						code: "invalid_key",
						origin: "record",
						issues: keyResult.issues.map((iss) => finalizeIssue(iss, ctx, config())),
						input: key,
						path: [key],
						inst
					});
					continue;
				}
				const result = def.valueType._zod.run({
					value: input[key],
					issues: []
				}, ctx);
				if (result instanceof Promise) proms.push(result.then((result$1) => {
					if (result$1.issues.length) payload.issues.push(...prefixIssues(key, result$1.issues));
					payload.value[keyResult.value] = result$1.value;
				}));
				else {
					if (result.issues.length) payload.issues.push(...prefixIssues(key, result.issues));
					payload.value[keyResult.value] = result.value;
				}
			}
		}
		if (proms.length) return Promise.all(proms).then(() => payload);
		return payload;
	};
});
const $ZodEnum = /* @__PURE__ */ $constructor("$ZodEnum", (inst, def) => {
	$ZodType.init(inst, def);
	const values = getEnumValues(def.entries);
	const valuesSet = new Set(values);
	inst._zod.values = valuesSet;
	inst._zod.pattern = /* @__PURE__ */ new RegExp(`^(${values.filter((k) => propertyKeyTypes.has(typeof k)).map((o) => typeof o === "string" ? escapeRegex(o) : o.toString()).join("|")})$`);
	inst._zod.parse = (payload, _ctx) => {
		const input = payload.value;
		if (valuesSet.has(input)) return payload;
		payload.issues.push({
			code: "invalid_value",
			values,
			input,
			inst
		});
		return payload;
	};
});
const $ZodLiteral = /* @__PURE__ */ $constructor("$ZodLiteral", (inst, def) => {
	$ZodType.init(inst, def);
	if (def.values.length === 0) throw new Error("Cannot create literal schema with no valid values");
	const values = new Set(def.values);
	inst._zod.values = values;
	inst._zod.pattern = /* @__PURE__ */ new RegExp(`^(${def.values.map((o) => typeof o === "string" ? escapeRegex(o) : o ? escapeRegex(o.toString()) : String(o)).join("|")})$`);
	inst._zod.parse = (payload, _ctx) => {
		const input = payload.value;
		if (values.has(input)) return payload;
		payload.issues.push({
			code: "invalid_value",
			values: def.values,
			input,
			inst
		});
		return payload;
	};
});
const $ZodTransform = /* @__PURE__ */ $constructor("$ZodTransform", (inst, def) => {
	$ZodType.init(inst, def);
	inst._zod.optin = "optional";
	inst._zod.parse = (payload, ctx) => {
		if (ctx.direction === "backward") throw new $ZodEncodeError(inst.constructor.name);
		const _out = def.transform(payload.value, payload);
		if (ctx.async) return (_out instanceof Promise ? _out : Promise.resolve(_out)).then((output) => {
			payload.value = output;
			payload.fallback = true;
			return payload;
		});
		if (_out instanceof Promise) throw new $ZodAsyncError();
		payload.value = _out;
		payload.fallback = true;
		return payload;
	};
});
function handleOptionalResult(result, input) {
	if (input === void 0 && (result.issues.length || result.fallback)) return {
		issues: [],
		value: void 0
	};
	return result;
}
const $ZodOptional = /* @__PURE__ */ $constructor("$ZodOptional", (inst, def) => {
	$ZodType.init(inst, def);
	inst._zod.optin = "optional";
	inst._zod.optout = "optional";
	defineLazy(inst._zod, "values", () => {
		return def.innerType._zod.values ? new Set([...def.innerType._zod.values, void 0]) : void 0;
	});
	defineLazy(inst._zod, "pattern", () => {
		const pattern = def.innerType._zod.pattern;
		return pattern ? /* @__PURE__ */ new RegExp(`^(${cleanRegex(pattern.source)})?$`) : void 0;
	});
	inst._zod.parse = (payload, ctx) => {
		if (def.innerType._zod.optin === "optional") {
			const input = payload.value;
			const result = def.innerType._zod.run(payload, ctx);
			if (result instanceof Promise) return result.then((r) => handleOptionalResult(r, input));
			return handleOptionalResult(result, input);
		}
		if (payload.value === void 0) return payload;
		return def.innerType._zod.run(payload, ctx);
	};
});
const $ZodExactOptional = /* @__PURE__ */ $constructor("$ZodExactOptional", (inst, def) => {
	$ZodOptional.init(inst, def);
	defineLazy(inst._zod, "values", () => def.innerType._zod.values);
	defineLazy(inst._zod, "pattern", () => def.innerType._zod.pattern);
	inst._zod.parse = (payload, ctx) => {
		return def.innerType._zod.run(payload, ctx);
	};
});
const $ZodNullable = /* @__PURE__ */ $constructor("$ZodNullable", (inst, def) => {
	$ZodType.init(inst, def);
	defineLazy(inst._zod, "optin", () => def.innerType._zod.optin);
	defineLazy(inst._zod, "optout", () => def.innerType._zod.optout);
	defineLazy(inst._zod, "pattern", () => {
		const pattern = def.innerType._zod.pattern;
		return pattern ? /* @__PURE__ */ new RegExp(`^(${cleanRegex(pattern.source)}|null)$`) : void 0;
	});
	defineLazy(inst._zod, "values", () => {
		return def.innerType._zod.values ? new Set([...def.innerType._zod.values, null]) : void 0;
	});
	inst._zod.parse = (payload, ctx) => {
		if (payload.value === null) return payload;
		return def.innerType._zod.run(payload, ctx);
	};
});
const $ZodDefault = /* @__PURE__ */ $constructor("$ZodDefault", (inst, def) => {
	$ZodType.init(inst, def);
	inst._zod.optin = "optional";
	defineLazy(inst._zod, "values", () => def.innerType._zod.values);
	inst._zod.parse = (payload, ctx) => {
		if (ctx.direction === "backward") return def.innerType._zod.run(payload, ctx);
		if (payload.value === void 0) {
			payload.value = def.defaultValue;
			return payload;
		}
		const result = def.innerType._zod.run(payload, ctx);
		if (result instanceof Promise) return result.then((result$1) => handleDefaultResult(result$1, def));
		return handleDefaultResult(result, def);
	};
});
function handleDefaultResult(payload, def) {
	if (payload.value === void 0) payload.value = def.defaultValue;
	return payload;
}
const $ZodPrefault = /* @__PURE__ */ $constructor("$ZodPrefault", (inst, def) => {
	$ZodType.init(inst, def);
	inst._zod.optin = "optional";
	defineLazy(inst._zod, "values", () => def.innerType._zod.values);
	inst._zod.parse = (payload, ctx) => {
		if (ctx.direction === "backward") return def.innerType._zod.run(payload, ctx);
		if (payload.value === void 0) payload.value = def.defaultValue;
		return def.innerType._zod.run(payload, ctx);
	};
});
const $ZodNonOptional = /* @__PURE__ */ $constructor("$ZodNonOptional", (inst, def) => {
	$ZodType.init(inst, def);
	defineLazy(inst._zod, "values", () => {
		const v = def.innerType._zod.values;
		return v ? new Set([...v].filter((x) => x !== void 0)) : void 0;
	});
	inst._zod.parse = (payload, ctx) => {
		const result = def.innerType._zod.run(payload, ctx);
		if (result instanceof Promise) return result.then((result$1) => handleNonOptionalResult(result$1, inst));
		return handleNonOptionalResult(result, inst);
	};
});
function handleNonOptionalResult(payload, inst) {
	if (!payload.issues.length && payload.value === void 0) payload.issues.push({
		code: "invalid_type",
		expected: "nonoptional",
		input: payload.value,
		inst
	});
	return payload;
}
const $ZodCatch = /* @__PURE__ */ $constructor("$ZodCatch", (inst, def) => {
	$ZodType.init(inst, def);
	inst._zod.optin = "optional";
	defineLazy(inst._zod, "optout", () => def.innerType._zod.optout);
	defineLazy(inst._zod, "values", () => def.innerType._zod.values);
	inst._zod.parse = (payload, ctx) => {
		if (ctx.direction === "backward") return def.innerType._zod.run(payload, ctx);
		const result = def.innerType._zod.run(payload, ctx);
		if (result instanceof Promise) return result.then((result$1) => {
			payload.value = result$1.value;
			if (result$1.issues.length) {
				payload.value = def.catchValue({
					...payload,
					error: { issues: result$1.issues.map((iss) => finalizeIssue(iss, ctx, config())) },
					input: payload.value
				});
				payload.issues = [];
				payload.fallback = true;
			}
			return payload;
		});
		payload.value = result.value;
		if (result.issues.length) {
			payload.value = def.catchValue({
				...payload,
				error: { issues: result.issues.map((iss) => finalizeIssue(iss, ctx, config())) },
				input: payload.value
			});
			payload.issues = [];
			payload.fallback = true;
		}
		return payload;
	};
});
const $ZodPipe = /* @__PURE__ */ $constructor("$ZodPipe", (inst, def) => {
	$ZodType.init(inst, def);
	defineLazy(inst._zod, "values", () => def.in._zod.values);
	defineLazy(inst._zod, "optin", () => def.in._zod.optin);
	defineLazy(inst._zod, "optout", () => def.out._zod.optout);
	defineLazy(inst._zod, "propValues", () => def.in._zod.propValues);
	inst._zod.parse = (payload, ctx) => {
		if (ctx.direction === "backward") {
			const right = def.out._zod.run(payload, ctx);
			if (right instanceof Promise) return right.then((right$1) => handlePipeResult(right$1, def.in, ctx));
			return handlePipeResult(right, def.in, ctx);
		}
		const left = def.in._zod.run(payload, ctx);
		if (left instanceof Promise) return left.then((left$1) => handlePipeResult(left$1, def.out, ctx));
		return handlePipeResult(left, def.out, ctx);
	};
});
function handlePipeResult(left, next, ctx) {
	if (left.issues.length) {
		left.aborted = true;
		return left;
	}
	return next._zod.run({
		value: left.value,
		issues: left.issues,
		fallback: left.fallback
	}, ctx);
}
const $ZodReadonly = /* @__PURE__ */ $constructor("$ZodReadonly", (inst, def) => {
	$ZodType.init(inst, def);
	defineLazy(inst._zod, "propValues", () => def.innerType._zod.propValues);
	defineLazy(inst._zod, "values", () => def.innerType._zod.values);
	defineLazy(inst._zod, "optin", () => def.innerType?._zod?.optin);
	defineLazy(inst._zod, "optout", () => def.innerType?._zod?.optout);
	inst._zod.parse = (payload, ctx) => {
		if (ctx.direction === "backward") return def.innerType._zod.run(payload, ctx);
		const result = def.innerType._zod.run(payload, ctx);
		if (result instanceof Promise) return result.then(handleReadonlyResult);
		return handleReadonlyResult(result);
	};
});
function handleReadonlyResult(payload) {
	payload.value = Object.freeze(payload.value);
	return payload;
}
const $ZodLazy = /* @__PURE__ */ $constructor("$ZodLazy", (inst, def) => {
	$ZodType.init(inst, def);
	defineLazy(inst._zod, "innerType", () => {
		const d = def;
		if (!d._cachedInner) d._cachedInner = def.getter();
		return d._cachedInner;
	});
	defineLazy(inst._zod, "pattern", () => inst._zod.innerType?._zod?.pattern);
	defineLazy(inst._zod, "propValues", () => inst._zod.innerType?._zod?.propValues);
	defineLazy(inst._zod, "optin", () => inst._zod.innerType?._zod?.optin ?? void 0);
	defineLazy(inst._zod, "optout", () => inst._zod.innerType?._zod?.optout ?? void 0);
	inst._zod.parse = (payload, ctx) => {
		return inst._zod.innerType._zod.run(payload, ctx);
	};
});
const $ZodCustom = /* @__PURE__ */ $constructor("$ZodCustom", (inst, def) => {
	$ZodCheck.init(inst, def);
	$ZodType.init(inst, def);
	inst._zod.parse = (payload, _) => {
		return payload;
	};
	inst._zod.check = (payload) => {
		const input = payload.value;
		const r = def.fn(input);
		if (r instanceof Promise) return r.then((r$1) => handleRefineResult(r$1, payload, input, inst));
		handleRefineResult(r, payload, input, inst);
	};
});
function handleRefineResult(result, payload, input, inst) {
	if (!result) {
		const _iss = {
			code: "custom",
			input,
			inst,
			path: [...inst._zod.def.path ?? []],
			continue: !inst._zod.def.abort
		};
		if (inst._zod.def.params) _iss.params = inst._zod.def.params;
		payload.issues.push(issue(_iss));
	}
}
var _a;
var $ZodRegistry = class {
	constructor() {
		this._map = /* @__PURE__ */ new WeakMap();
		this._idmap = /* @__PURE__ */ new Map();
	}
	add(schema, ..._meta) {
		const meta$2 = _meta[0];
		this._map.set(schema, meta$2);
		if (meta$2 && typeof meta$2 === "object" && "id" in meta$2) this._idmap.set(meta$2.id, schema);
		return this;
	}
	clear() {
		this._map = /* @__PURE__ */ new WeakMap();
		this._idmap = /* @__PURE__ */ new Map();
		return this;
	}
	remove(schema) {
		const meta$2 = this._map.get(schema);
		if (meta$2 && typeof meta$2 === "object" && "id" in meta$2) this._idmap.delete(meta$2.id);
		this._map.delete(schema);
		return this;
	}
	get(schema) {
		const p = schema._zod.parent;
		if (p) {
			const pm = { ...this.get(p) ?? {} };
			delete pm.id;
			const f = {
				...pm,
				...this._map.get(schema)
			};
			return Object.keys(f).length ? f : void 0;
		}
		return this._map.get(schema);
	}
	has(schema) {
		return this._map.has(schema);
	}
};
function registry() {
	return new $ZodRegistry();
}
(_a = globalThis).__zod_globalRegistry ?? (_a.__zod_globalRegistry = registry());
const globalRegistry = globalThis.__zod_globalRegistry;
/* @__NO_SIDE_EFFECTS__ */
function _string(Class, params) {
	return new Class({
		type: "string",
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _email(Class, params) {
	return new Class({
		type: "string",
		format: "email",
		check: "string_format",
		abort: false,
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _guid(Class, params) {
	return new Class({
		type: "string",
		format: "guid",
		check: "string_format",
		abort: false,
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _uuid(Class, params) {
	return new Class({
		type: "string",
		format: "uuid",
		check: "string_format",
		abort: false,
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _uuidv4(Class, params) {
	return new Class({
		type: "string",
		format: "uuid",
		check: "string_format",
		abort: false,
		version: "v4",
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _uuidv6(Class, params) {
	return new Class({
		type: "string",
		format: "uuid",
		check: "string_format",
		abort: false,
		version: "v6",
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _uuidv7(Class, params) {
	return new Class({
		type: "string",
		format: "uuid",
		check: "string_format",
		abort: false,
		version: "v7",
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _url(Class, params) {
	return new Class({
		type: "string",
		format: "url",
		check: "string_format",
		abort: false,
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _emoji(Class, params) {
	return new Class({
		type: "string",
		format: "emoji",
		check: "string_format",
		abort: false,
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _nanoid(Class, params) {
	return new Class({
		type: "string",
		format: "nanoid",
		check: "string_format",
		abort: false,
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _cuid(Class, params) {
	return new Class({
		type: "string",
		format: "cuid",
		check: "string_format",
		abort: false,
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _cuid2(Class, params) {
	return new Class({
		type: "string",
		format: "cuid2",
		check: "string_format",
		abort: false,
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _ulid(Class, params) {
	return new Class({
		type: "string",
		format: "ulid",
		check: "string_format",
		abort: false,
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _xid(Class, params) {
	return new Class({
		type: "string",
		format: "xid",
		check: "string_format",
		abort: false,
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _ksuid(Class, params) {
	return new Class({
		type: "string",
		format: "ksuid",
		check: "string_format",
		abort: false,
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _ipv4(Class, params) {
	return new Class({
		type: "string",
		format: "ipv4",
		check: "string_format",
		abort: false,
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _ipv6(Class, params) {
	return new Class({
		type: "string",
		format: "ipv6",
		check: "string_format",
		abort: false,
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _cidrv4(Class, params) {
	return new Class({
		type: "string",
		format: "cidrv4",
		check: "string_format",
		abort: false,
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _cidrv6(Class, params) {
	return new Class({
		type: "string",
		format: "cidrv6",
		check: "string_format",
		abort: false,
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _base64(Class, params) {
	return new Class({
		type: "string",
		format: "base64",
		check: "string_format",
		abort: false,
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _base64url(Class, params) {
	return new Class({
		type: "string",
		format: "base64url",
		check: "string_format",
		abort: false,
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _e164(Class, params) {
	return new Class({
		type: "string",
		format: "e164",
		check: "string_format",
		abort: false,
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _jwt(Class, params) {
	return new Class({
		type: "string",
		format: "jwt",
		check: "string_format",
		abort: false,
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _isoDateTime(Class, params) {
	return new Class({
		type: "string",
		format: "datetime",
		check: "string_format",
		offset: false,
		local: false,
		precision: null,
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _isoDate(Class, params) {
	return new Class({
		type: "string",
		format: "date",
		check: "string_format",
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _isoTime(Class, params) {
	return new Class({
		type: "string",
		format: "time",
		check: "string_format",
		precision: null,
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _isoDuration(Class, params) {
	return new Class({
		type: "string",
		format: "duration",
		check: "string_format",
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _number(Class, params) {
	return new Class({
		type: "number",
		checks: [],
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _int(Class, params) {
	return new Class({
		type: "number",
		check: "number_format",
		abort: false,
		format: "safeint",
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _boolean(Class, params) {
	return new Class({
		type: "boolean",
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _null$1(Class, params) {
	return new Class({
		type: "null",
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _unknown(Class) {
	return new Class({ type: "unknown" });
}
/* @__NO_SIDE_EFFECTS__ */
function _never(Class, params) {
	return new Class({
		type: "never",
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _lt(value, params) {
	return new $ZodCheckLessThan({
		check: "less_than",
		...normalizeParams(params),
		value,
		inclusive: false
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _lte(value, params) {
	return new $ZodCheckLessThan({
		check: "less_than",
		...normalizeParams(params),
		value,
		inclusive: true
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _gt(value, params) {
	return new $ZodCheckGreaterThan({
		check: "greater_than",
		...normalizeParams(params),
		value,
		inclusive: false
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _gte(value, params) {
	return new $ZodCheckGreaterThan({
		check: "greater_than",
		...normalizeParams(params),
		value,
		inclusive: true
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _multipleOf(value, params) {
	return new $ZodCheckMultipleOf({
		check: "multiple_of",
		...normalizeParams(params),
		value
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _maxLength(maximum, params) {
	return new $ZodCheckMaxLength({
		check: "max_length",
		...normalizeParams(params),
		maximum
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _minLength(minimum, params) {
	return new $ZodCheckMinLength({
		check: "min_length",
		...normalizeParams(params),
		minimum
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _length(length, params) {
	return new $ZodCheckLengthEquals({
		check: "length_equals",
		...normalizeParams(params),
		length
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _regex(pattern, params) {
	return new $ZodCheckRegex({
		check: "string_format",
		format: "regex",
		...normalizeParams(params),
		pattern
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _lowercase(params) {
	return new $ZodCheckLowerCase({
		check: "string_format",
		format: "lowercase",
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _uppercase(params) {
	return new $ZodCheckUpperCase({
		check: "string_format",
		format: "uppercase",
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _includes(includes, params) {
	return new $ZodCheckIncludes({
		check: "string_format",
		format: "includes",
		...normalizeParams(params),
		includes
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _startsWith(prefix, params) {
	return new $ZodCheckStartsWith({
		check: "string_format",
		format: "starts_with",
		...normalizeParams(params),
		prefix
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _endsWith(suffix, params) {
	return new $ZodCheckEndsWith({
		check: "string_format",
		format: "ends_with",
		...normalizeParams(params),
		suffix
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _overwrite(tx) {
	return new $ZodCheckOverwrite({
		check: "overwrite",
		tx
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _normalize(form) {
	return /* @__PURE__ */ _overwrite((input) => input.normalize(form));
}
/* @__NO_SIDE_EFFECTS__ */
function _trim() {
	return /* @__PURE__ */ _overwrite((input) => input.trim());
}
/* @__NO_SIDE_EFFECTS__ */
function _toLowerCase() {
	return /* @__PURE__ */ _overwrite((input) => input.toLowerCase());
}
/* @__NO_SIDE_EFFECTS__ */
function _toUpperCase() {
	return /* @__PURE__ */ _overwrite((input) => input.toUpperCase());
}
/* @__NO_SIDE_EFFECTS__ */
function _slugify() {
	return /* @__PURE__ */ _overwrite((input) => slugify(input));
}
/* @__NO_SIDE_EFFECTS__ */
function _array(Class, element, params) {
	return new Class({
		type: "array",
		element,
		...normalizeParams(params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _refine(Class, fn, _params) {
	return new Class({
		type: "custom",
		check: "custom",
		fn,
		...normalizeParams(_params)
	});
}
/* @__NO_SIDE_EFFECTS__ */
function _superRefine(fn, params) {
	const ch = /* @__PURE__ */ _check((payload) => {
		payload.addIssue = (issue$1) => {
			if (typeof issue$1 === "string") payload.issues.push(issue(issue$1, payload.value, ch._zod.def));
			else {
				const _issue = issue$1;
				if (_issue.fatal) _issue.continue = false;
				_issue.code ?? (_issue.code = "custom");
				_issue.input ?? (_issue.input = payload.value);
				_issue.inst ?? (_issue.inst = ch);
				_issue.continue ?? (_issue.continue = !ch._zod.def.abort);
				payload.issues.push(issue(_issue));
			}
		};
		return fn(payload.value, payload);
	}, params);
	return ch;
}
/* @__NO_SIDE_EFFECTS__ */
function _check(fn, params) {
	const ch = new $ZodCheck({
		check: "custom",
		...normalizeParams(params)
	});
	ch._zod.check = fn;
	return ch;
}
function initializeContext(params) {
	let target = params?.target ?? "draft-2020-12";
	if (target === "draft-4") target = "draft-04";
	if (target === "draft-7") target = "draft-07";
	return {
		processors: params.processors ?? {},
		metadataRegistry: params?.metadata ?? globalRegistry,
		target,
		unrepresentable: params?.unrepresentable ?? "throw",
		override: params?.override ?? (() => {}),
		io: params?.io ?? "output",
		counter: 0,
		seen: /* @__PURE__ */ new Map(),
		cycles: params?.cycles ?? "ref",
		reused: params?.reused ?? "inline",
		external: params?.external ?? void 0
	};
}
function process(schema, ctx, _params = {
	path: [],
	schemaPath: []
}) {
	var _a$2;
	const def = schema._zod.def;
	const seen = ctx.seen.get(schema);
	if (seen) {
		seen.count++;
		if (_params.schemaPath.includes(schema)) seen.cycle = _params.path;
		return seen.schema;
	}
	const result = {
		schema: {},
		count: 1,
		cycle: void 0,
		path: _params.path
	};
	ctx.seen.set(schema, result);
	const overrideSchema = schema._zod.toJSONSchema?.();
	if (overrideSchema) result.schema = overrideSchema;
	else {
		const params = {
			..._params,
			schemaPath: [..._params.schemaPath, schema],
			path: _params.path
		};
		if (schema._zod.processJSONSchema) schema._zod.processJSONSchema(ctx, result.schema, params);
		else {
			const _json = result.schema;
			const processor = ctx.processors[def.type];
			if (!processor) throw new Error(`[toJSONSchema]: Non-representable type encountered: ${def.type}`);
			processor(schema, ctx, _json, params);
		}
		const parent = schema._zod.parent;
		if (parent) {
			if (!result.ref) result.ref = parent;
			process(parent, ctx, params);
			ctx.seen.get(parent).isParent = true;
		}
	}
	const meta$2 = ctx.metadataRegistry.get(schema);
	if (meta$2) Object.assign(result.schema, meta$2);
	if (ctx.io === "input" && isTransforming(schema)) {
		delete result.schema.examples;
		delete result.schema.default;
	}
	if (ctx.io === "input" && "_prefault" in result.schema) (_a$2 = result.schema).default ?? (_a$2.default = result.schema._prefault);
	delete result.schema._prefault;
	return ctx.seen.get(schema).schema;
}
function extractDefs(ctx, schema) {
	const root = ctx.seen.get(schema);
	if (!root) throw new Error("Unprocessed schema. This is a bug in Zod.");
	const idToSchema = /* @__PURE__ */ new Map();
	for (const entry of ctx.seen.entries()) {
		const id = ctx.metadataRegistry.get(entry[0])?.id;
		if (id) {
			const existing = idToSchema.get(id);
			if (existing && existing !== entry[0]) throw new Error(`Duplicate schema id "${id}" detected during JSON Schema conversion. Two different schemas cannot share the same id when converted together.`);
			idToSchema.set(id, entry[0]);
		}
	}
	const makeURI = (entry) => {
		const defsSegment = ctx.target === "draft-2020-12" ? "$defs" : "definitions";
		if (ctx.external) {
			const externalId = ctx.external.registry.get(entry[0])?.id;
			const uriGenerator = ctx.external.uri ?? ((id$1) => id$1);
			if (externalId) return { ref: uriGenerator(externalId) };
			const id = entry[1].defId ?? entry[1].schema.id ?? `schema${ctx.counter++}`;
			entry[1].defId = id;
			return {
				defId: id,
				ref: `${uriGenerator("__shared")}#/${defsSegment}/${id}`
			};
		}
		if (entry[1] === root) return { ref: "#" };
		const defUriPrefix = `#/${defsSegment}/`;
		const defId = entry[1].schema.id ?? `__schema${ctx.counter++}`;
		return {
			defId,
			ref: defUriPrefix + defId
		};
	};
	const extractToDef = (entry) => {
		if (entry[1].schema.$ref) return;
		const seen = entry[1];
		const { ref, defId } = makeURI(entry);
		seen.def = { ...seen.schema };
		if (defId) seen.defId = defId;
		const schema$1 = seen.schema;
		for (const key in schema$1) delete schema$1[key];
		schema$1.$ref = ref;
	};
	if (ctx.cycles === "throw") for (const entry of ctx.seen.entries()) {
		const seen = entry[1];
		if (seen.cycle) throw new Error(`Cycle detected: #/${seen.cycle?.join("/")}/<root>

Set the \`cycles\` parameter to \`"ref"\` to resolve cyclical schemas with defs.`);
	}
	for (const entry of ctx.seen.entries()) {
		const seen = entry[1];
		if (schema === entry[0]) {
			extractToDef(entry);
			continue;
		}
		if (ctx.external) {
			const ext = ctx.external.registry.get(entry[0])?.id;
			if (schema !== entry[0] && ext) {
				extractToDef(entry);
				continue;
			}
		}
		if (ctx.metadataRegistry.get(entry[0])?.id) {
			extractToDef(entry);
			continue;
		}
		if (seen.cycle) {
			extractToDef(entry);
			continue;
		}
		if (seen.count > 1) {
			if (ctx.reused === "ref") {
				extractToDef(entry);
				continue;
			}
		}
	}
}
function finalize(ctx, schema) {
	const root = ctx.seen.get(schema);
	if (!root) throw new Error("Unprocessed schema. This is a bug in Zod.");
	const flattenRef = (zodSchema) => {
		const seen = ctx.seen.get(zodSchema);
		if (seen.ref === null) return;
		const schema$1 = seen.def ?? seen.schema;
		const _cached = { ...schema$1 };
		const ref = seen.ref;
		seen.ref = null;
		if (ref) {
			flattenRef(ref);
			const refSeen = ctx.seen.get(ref);
			const refSchema = refSeen.schema;
			if (refSchema.$ref && (ctx.target === "draft-07" || ctx.target === "draft-04" || ctx.target === "openapi-3.0")) {
				schema$1.allOf = schema$1.allOf ?? [];
				schema$1.allOf.push(refSchema);
			} else Object.assign(schema$1, refSchema);
			Object.assign(schema$1, _cached);
			if (zodSchema._zod.parent === ref) for (const key in schema$1) {
				if (key === "$ref" || key === "allOf") continue;
				if (!(key in _cached)) delete schema$1[key];
			}
			if (refSchema.$ref && refSeen.def) for (const key in schema$1) {
				if (key === "$ref" || key === "allOf") continue;
				if (key in refSeen.def && JSON.stringify(schema$1[key]) === JSON.stringify(refSeen.def[key])) delete schema$1[key];
			}
		}
		const parent = zodSchema._zod.parent;
		if (parent && parent !== ref) {
			flattenRef(parent);
			const parentSeen = ctx.seen.get(parent);
			if (parentSeen?.schema.$ref) {
				schema$1.$ref = parentSeen.schema.$ref;
				if (parentSeen.def) for (const key in schema$1) {
					if (key === "$ref" || key === "allOf") continue;
					if (key in parentSeen.def && JSON.stringify(schema$1[key]) === JSON.stringify(parentSeen.def[key])) delete schema$1[key];
				}
			}
		}
		ctx.override({
			zodSchema,
			jsonSchema: schema$1,
			path: seen.path ?? []
		});
	};
	for (const entry of [...ctx.seen.entries()].reverse()) flattenRef(entry[0]);
	const result = {};
	if (ctx.target === "draft-2020-12") result.$schema = "https://json-schema.org/draft/2020-12/schema";
	else if (ctx.target === "draft-07") result.$schema = "http://json-schema.org/draft-07/schema#";
	else if (ctx.target === "draft-04") result.$schema = "http://json-schema.org/draft-04/schema#";
	else if (ctx.target === "openapi-3.0") {}
	if (ctx.external?.uri) {
		const id = ctx.external.registry.get(schema)?.id;
		if (!id) throw new Error("Schema is missing an `id` property");
		result.$id = ctx.external.uri(id);
	}
	Object.assign(result, root.def ?? root.schema);
	const rootMetaId = ctx.metadataRegistry.get(schema)?.id;
	if (rootMetaId !== void 0 && result.id === rootMetaId) delete result.id;
	const defs = ctx.external?.defs ?? {};
	for (const entry of ctx.seen.entries()) {
		const seen = entry[1];
		if (seen.def && seen.defId) {
			if (seen.def.id === seen.defId) delete seen.def.id;
			defs[seen.defId] = seen.def;
		}
	}
	if (ctx.external) {} else if (Object.keys(defs).length > 0) if (ctx.target === "draft-2020-12") result.$defs = defs;
	else result.definitions = defs;
	try {
		const finalized = JSON.parse(JSON.stringify(result));
		Object.defineProperty(finalized, "~standard", {
			value: {
				...schema["~standard"],
				jsonSchema: {
					input: createStandardJSONSchemaMethod(schema, "input", ctx.processors),
					output: createStandardJSONSchemaMethod(schema, "output", ctx.processors)
				}
			},
			enumerable: false,
			writable: false
		});
		return finalized;
	} catch (_err) {
		throw new Error("Error converting schema to JSON.");
	}
}
function isTransforming(_schema, _ctx) {
	const ctx = _ctx ?? { seen: /* @__PURE__ */ new Set() };
	if (ctx.seen.has(_schema)) return false;
	ctx.seen.add(_schema);
	const def = _schema._zod.def;
	if (def.type === "transform") return true;
	if (def.type === "array") return isTransforming(def.element, ctx);
	if (def.type === "set") return isTransforming(def.valueType, ctx);
	if (def.type === "lazy") return isTransforming(def.getter(), ctx);
	if (def.type === "promise" || def.type === "optional" || def.type === "nonoptional" || def.type === "nullable" || def.type === "readonly" || def.type === "default" || def.type === "prefault") return isTransforming(def.innerType, ctx);
	if (def.type === "intersection") return isTransforming(def.left, ctx) || isTransforming(def.right, ctx);
	if (def.type === "record" || def.type === "map") return isTransforming(def.keyType, ctx) || isTransforming(def.valueType, ctx);
	if (def.type === "pipe") {
		if (_schema._zod.traits.has("$ZodCodec")) return true;
		return isTransforming(def.in, ctx) || isTransforming(def.out, ctx);
	}
	if (def.type === "object") {
		for (const key in def.shape) if (isTransforming(def.shape[key], ctx)) return true;
		return false;
	}
	if (def.type === "union") {
		for (const option of def.options) if (isTransforming(option, ctx)) return true;
		return false;
	}
	if (def.type === "tuple") {
		for (const item of def.items) if (isTransforming(item, ctx)) return true;
		if (def.rest && isTransforming(def.rest, ctx)) return true;
		return false;
	}
	return false;
}
const createToJSONSchemaMethod = (schema, processors = {}) => (params) => {
	const ctx = initializeContext({
		...params,
		processors
	});
	process(schema, ctx);
	extractDefs(ctx, schema);
	return finalize(ctx, schema);
};
const createStandardJSONSchemaMethod = (schema, io, processors = {}) => (params) => {
	const { libraryOptions, target } = params ?? {};
	const ctx = initializeContext({
		...libraryOptions ?? {},
		target,
		io,
		processors
	});
	process(schema, ctx);
	extractDefs(ctx, schema);
	return finalize(ctx, schema);
};
var formatMap = {
	guid: "uuid",
	url: "uri",
	datetime: "date-time",
	json_string: "json-string",
	regex: ""
};
const stringProcessor = (schema, ctx, _json, _params) => {
	const json$1 = _json;
	json$1.type = "string";
	const { minimum, maximum, format, patterns, contentEncoding } = schema._zod.bag;
	if (typeof minimum === "number") json$1.minLength = minimum;
	if (typeof maximum === "number") json$1.maxLength = maximum;
	if (format) {
		json$1.format = formatMap[format] ?? format;
		if (json$1.format === "") delete json$1.format;
		if (format === "time") delete json$1.format;
	}
	if (contentEncoding) json$1.contentEncoding = contentEncoding;
	if (patterns && patterns.size > 0) {
		const regexes = [...patterns];
		if (regexes.length === 1) json$1.pattern = regexes[0].source;
		else if (regexes.length > 1) json$1.allOf = [...regexes.map((regex) => ({
			...ctx.target === "draft-07" || ctx.target === "draft-04" || ctx.target === "openapi-3.0" ? { type: "string" } : {},
			pattern: regex.source
		}))];
	}
};
const numberProcessor = (schema, ctx, _json, _params) => {
	const json$1 = _json;
	const { minimum, maximum, format, multipleOf, exclusiveMaximum, exclusiveMinimum } = schema._zod.bag;
	if (typeof format === "string" && format.includes("int")) json$1.type = "integer";
	else json$1.type = "number";
	const exMin = typeof exclusiveMinimum === "number" && exclusiveMinimum >= (minimum ?? Number.NEGATIVE_INFINITY);
	const exMax = typeof exclusiveMaximum === "number" && exclusiveMaximum <= (maximum ?? Number.POSITIVE_INFINITY);
	const legacy = ctx.target === "draft-04" || ctx.target === "openapi-3.0";
	if (exMin) if (legacy) {
		json$1.minimum = exclusiveMinimum;
		json$1.exclusiveMinimum = true;
	} else json$1.exclusiveMinimum = exclusiveMinimum;
	else if (typeof minimum === "number") json$1.minimum = minimum;
	if (exMax) if (legacy) {
		json$1.maximum = exclusiveMaximum;
		json$1.exclusiveMaximum = true;
	} else json$1.exclusiveMaximum = exclusiveMaximum;
	else if (typeof maximum === "number") json$1.maximum = maximum;
	if (typeof multipleOf === "number") json$1.multipleOf = multipleOf;
};
const booleanProcessor = (_schema, _ctx, json$1, _params) => {
	json$1.type = "boolean";
};
const nullProcessor = (_schema, ctx, json$1, _params) => {
	if (ctx.target === "openapi-3.0") {
		json$1.type = "string";
		json$1.nullable = true;
		json$1.enum = [null];
	} else json$1.type = "null";
};
const neverProcessor = (_schema, _ctx, json$1, _params) => {
	json$1.not = {};
};
const unknownProcessor = (_schema, _ctx, _json, _params) => {};
const enumProcessor = (schema, _ctx, json$1, _params) => {
	const def = schema._zod.def;
	const values = getEnumValues(def.entries);
	if (values.every((v) => typeof v === "number")) json$1.type = "number";
	if (values.every((v) => typeof v === "string")) json$1.type = "string";
	json$1.enum = values;
};
const literalProcessor = (schema, ctx, json$1, _params) => {
	const def = schema._zod.def;
	const vals = [];
	for (const val of def.values) if (val === void 0) {
		if (ctx.unrepresentable === "throw") throw new Error("Literal `undefined` cannot be represented in JSON Schema");
	} else if (typeof val === "bigint") if (ctx.unrepresentable === "throw") throw new Error("BigInt literals cannot be represented in JSON Schema");
	else vals.push(Number(val));
	else vals.push(val);
	if (vals.length === 0) {} else if (vals.length === 1) {
		const val = vals[0];
		json$1.type = val === null ? "null" : typeof val;
		if (ctx.target === "draft-04" || ctx.target === "openapi-3.0") json$1.enum = [val];
		else json$1.const = val;
	} else {
		if (vals.every((v) => typeof v === "number")) json$1.type = "number";
		if (vals.every((v) => typeof v === "string")) json$1.type = "string";
		if (vals.every((v) => typeof v === "boolean")) json$1.type = "boolean";
		if (vals.every((v) => v === null)) json$1.type = "null";
		json$1.enum = vals;
	}
};
const customProcessor = (_schema, ctx, _json, _params) => {
	if (ctx.unrepresentable === "throw") throw new Error("Custom types cannot be represented in JSON Schema");
};
const transformProcessor = (_schema, ctx, _json, _params) => {
	if (ctx.unrepresentable === "throw") throw new Error("Transforms cannot be represented in JSON Schema");
};
const arrayProcessor = (schema, ctx, _json, params) => {
	const json$1 = _json;
	const def = schema._zod.def;
	const { minimum, maximum } = schema._zod.bag;
	if (typeof minimum === "number") json$1.minItems = minimum;
	if (typeof maximum === "number") json$1.maxItems = maximum;
	json$1.type = "array";
	json$1.items = process(def.element, ctx, {
		...params,
		path: [...params.path, "items"]
	});
};
const objectProcessor = (schema, ctx, _json, params) => {
	const json$1 = _json;
	const def = schema._zod.def;
	json$1.type = "object";
	json$1.properties = {};
	const shape = def.shape;
	for (const key in shape) json$1.properties[key] = process(shape[key], ctx, {
		...params,
		path: [
			...params.path,
			"properties",
			key
		]
	});
	const allKeys = new Set(Object.keys(shape));
	const requiredKeys = new Set([...allKeys].filter((key) => {
		const v = def.shape[key]._zod;
		if (ctx.io === "input") return v.optin === void 0;
		else return v.optout === void 0;
	}));
	if (requiredKeys.size > 0) json$1.required = Array.from(requiredKeys);
	if (def.catchall?._zod.def.type === "never") json$1.additionalProperties = false;
	else if (!def.catchall) {
		if (ctx.io === "output") json$1.additionalProperties = false;
	} else if (def.catchall) json$1.additionalProperties = process(def.catchall, ctx, {
		...params,
		path: [...params.path, "additionalProperties"]
	});
};
const unionProcessor = (schema, ctx, json$1, params) => {
	const def = schema._zod.def;
	const isExclusive = def.inclusive === false;
	const options = def.options.map((x, i) => process(x, ctx, {
		...params,
		path: [
			...params.path,
			isExclusive ? "oneOf" : "anyOf",
			i
		]
	}));
	if (isExclusive) json$1.oneOf = options;
	else json$1.anyOf = options;
};
const intersectionProcessor = (schema, ctx, json$1, params) => {
	const def = schema._zod.def;
	const a = process(def.left, ctx, {
		...params,
		path: [
			...params.path,
			"allOf",
			0
		]
	});
	const b = process(def.right, ctx, {
		...params,
		path: [
			...params.path,
			"allOf",
			1
		]
	});
	const isSimpleIntersection = (val) => "allOf" in val && Object.keys(val).length === 1;
	json$1.allOf = [...isSimpleIntersection(a) ? a.allOf : [a], ...isSimpleIntersection(b) ? b.allOf : [b]];
};
const recordProcessor = (schema, ctx, _json, params) => {
	const json$1 = _json;
	const def = schema._zod.def;
	json$1.type = "object";
	const keyType = def.keyType;
	const patterns = keyType._zod.bag?.patterns;
	if (def.mode === "loose" && patterns && patterns.size > 0) {
		const valueSchema = process(def.valueType, ctx, {
			...params,
			path: [
				...params.path,
				"patternProperties",
				"*"
			]
		});
		json$1.patternProperties = {};
		for (const pattern of patterns) json$1.patternProperties[pattern.source] = valueSchema;
	} else {
		if (ctx.target === "draft-07" || ctx.target === "draft-2020-12") json$1.propertyNames = process(def.keyType, ctx, {
			...params,
			path: [...params.path, "propertyNames"]
		});
		json$1.additionalProperties = process(def.valueType, ctx, {
			...params,
			path: [...params.path, "additionalProperties"]
		});
	}
	const keyValues = keyType._zod.values;
	if (keyValues) {
		const validKeyValues = [...keyValues].filter((v) => typeof v === "string" || typeof v === "number");
		if (validKeyValues.length > 0) json$1.required = validKeyValues;
	}
};
const nullableProcessor = (schema, ctx, json$1, params) => {
	const def = schema._zod.def;
	const inner = process(def.innerType, ctx, params);
	const seen = ctx.seen.get(schema);
	if (ctx.target === "openapi-3.0") {
		seen.ref = def.innerType;
		json$1.nullable = true;
	} else json$1.anyOf = [inner, { type: "null" }];
};
const nonoptionalProcessor = (schema, ctx, _json, params) => {
	const def = schema._zod.def;
	process(def.innerType, ctx, params);
	const seen = ctx.seen.get(schema);
	seen.ref = def.innerType;
};
const defaultProcessor = (schema, ctx, json$1, params) => {
	const def = schema._zod.def;
	process(def.innerType, ctx, params);
	const seen = ctx.seen.get(schema);
	seen.ref = def.innerType;
	json$1.default = JSON.parse(JSON.stringify(def.defaultValue));
};
const prefaultProcessor = (schema, ctx, json$1, params) => {
	const def = schema._zod.def;
	process(def.innerType, ctx, params);
	const seen = ctx.seen.get(schema);
	seen.ref = def.innerType;
	if (ctx.io === "input") json$1._prefault = JSON.parse(JSON.stringify(def.defaultValue));
};
const catchProcessor = (schema, ctx, json$1, params) => {
	const def = schema._zod.def;
	process(def.innerType, ctx, params);
	const seen = ctx.seen.get(schema);
	seen.ref = def.innerType;
	let catchValue;
	try {
		catchValue = def.catchValue(void 0);
	} catch {
		throw new Error("Dynamic catch values are not supported in JSON Schema");
	}
	json$1.default = catchValue;
};
const pipeProcessor = (schema, ctx, _json, params) => {
	const def = schema._zod.def;
	const inIsTransform = def.in._zod.traits.has("$ZodTransform");
	const innerType = ctx.io === "input" ? inIsTransform ? def.out : def.in : def.out;
	process(innerType, ctx, params);
	const seen = ctx.seen.get(schema);
	seen.ref = innerType;
};
const readonlyProcessor = (schema, ctx, json$1, params) => {
	const def = schema._zod.def;
	process(def.innerType, ctx, params);
	const seen = ctx.seen.get(schema);
	seen.ref = def.innerType;
	json$1.readOnly = true;
};
const optionalProcessor = (schema, ctx, _json, params) => {
	const def = schema._zod.def;
	process(def.innerType, ctx, params);
	const seen = ctx.seen.get(schema);
	seen.ref = def.innerType;
};
const lazyProcessor = (schema, ctx, _json, params) => {
	const innerType = schema._zod.innerType;
	process(innerType, ctx, params);
	const seen = ctx.seen.get(schema);
	seen.ref = innerType;
};
const ZodISODateTime = /* @__PURE__ */ $constructor("ZodISODateTime", (inst, def) => {
	$ZodISODateTime.init(inst, def);
	ZodStringFormat.init(inst, def);
});
function datetime(params) {
	return /* @__PURE__ */ _isoDateTime(ZodISODateTime, params);
}
const ZodISODate = /* @__PURE__ */ $constructor("ZodISODate", (inst, def) => {
	$ZodISODate.init(inst, def);
	ZodStringFormat.init(inst, def);
});
function date(params) {
	return /* @__PURE__ */ _isoDate(ZodISODate, params);
}
const ZodISOTime = /* @__PURE__ */ $constructor("ZodISOTime", (inst, def) => {
	$ZodISOTime.init(inst, def);
	ZodStringFormat.init(inst, def);
});
function time(params) {
	return /* @__PURE__ */ _isoTime(ZodISOTime, params);
}
const ZodISODuration = /* @__PURE__ */ $constructor("ZodISODuration", (inst, def) => {
	$ZodISODuration.init(inst, def);
	ZodStringFormat.init(inst, def);
});
function duration(params) {
	return /* @__PURE__ */ _isoDuration(ZodISODuration, params);
}
var initializer = (inst, issues) => {
	$ZodError.init(inst, issues);
	inst.name = "ZodError";
	Object.defineProperties(inst, {
		format: { value: (mapper) => formatError(inst, mapper) },
		flatten: { value: (mapper) => flattenError(inst, mapper) },
		addIssue: { value: (issue$1) => {
			inst.issues.push(issue$1);
			inst.message = JSON.stringify(inst.issues, jsonStringifyReplacer, 2);
		} },
		addIssues: { value: (issues$1) => {
			inst.issues.push(...issues$1);
			inst.message = JSON.stringify(inst.issues, jsonStringifyReplacer, 2);
		} },
		isEmpty: { get() {
			return inst.issues.length === 0;
		} }
	});
};
const ZodRealError = /* @__PURE__ */ $constructor("ZodError", initializer, { Parent: Error });
const parse = /* @__PURE__ */ _parse(ZodRealError);
const parseAsync = /* @__PURE__ */ _parseAsync(ZodRealError);
const safeParse = /* @__PURE__ */ _safeParse(ZodRealError);
const safeParseAsync = /* @__PURE__ */ _safeParseAsync(ZodRealError);
const encode = /* @__PURE__ */ _encode(ZodRealError);
const decode = /* @__PURE__ */ _decode(ZodRealError);
const encodeAsync = /* @__PURE__ */ _encodeAsync(ZodRealError);
const decodeAsync = /* @__PURE__ */ _decodeAsync(ZodRealError);
const safeEncode = /* @__PURE__ */ _safeEncode(ZodRealError);
const safeDecode = /* @__PURE__ */ _safeDecode(ZodRealError);
const safeEncodeAsync = /* @__PURE__ */ _safeEncodeAsync(ZodRealError);
const safeDecodeAsync = /* @__PURE__ */ _safeDecodeAsync(ZodRealError);
var _installedGroups = /* @__PURE__ */ new WeakMap();
function _installLazyMethods(inst, group, methods) {
	const proto = Object.getPrototypeOf(inst);
	let installed = _installedGroups.get(proto);
	if (!installed) {
		installed = /* @__PURE__ */ new Set();
		_installedGroups.set(proto, installed);
	}
	if (installed.has(group)) return;
	installed.add(group);
	for (const key in methods) {
		const fn = methods[key];
		Object.defineProperty(proto, key, {
			configurable: true,
			enumerable: false,
			get() {
				const bound = fn.bind(this);
				Object.defineProperty(this, key, {
					configurable: true,
					writable: true,
					enumerable: true,
					value: bound
				});
				return bound;
			},
			set(v) {
				Object.defineProperty(this, key, {
					configurable: true,
					writable: true,
					enumerable: true,
					value: v
				});
			}
		});
	}
}
const ZodType = /* @__PURE__ */ $constructor("ZodType", (inst, def) => {
	$ZodType.init(inst, def);
	Object.assign(inst["~standard"], { jsonSchema: {
		input: createStandardJSONSchemaMethod(inst, "input"),
		output: createStandardJSONSchemaMethod(inst, "output")
	} });
	inst.toJSONSchema = createToJSONSchemaMethod(inst, {});
	inst.def = def;
	inst.type = def.type;
	Object.defineProperty(inst, "_def", { value: def });
	inst.parse = (data, params) => parse(inst, data, params, { callee: inst.parse });
	inst.safeParse = (data, params) => safeParse(inst, data, params);
	inst.parseAsync = async (data, params) => parseAsync(inst, data, params, { callee: inst.parseAsync });
	inst.safeParseAsync = async (data, params) => safeParseAsync(inst, data, params);
	inst.spa = inst.safeParseAsync;
	inst.encode = (data, params) => encode(inst, data, params);
	inst.decode = (data, params) => decode(inst, data, params);
	inst.encodeAsync = async (data, params) => encodeAsync(inst, data, params);
	inst.decodeAsync = async (data, params) => decodeAsync(inst, data, params);
	inst.safeEncode = (data, params) => safeEncode(inst, data, params);
	inst.safeDecode = (data, params) => safeDecode(inst, data, params);
	inst.safeEncodeAsync = async (data, params) => safeEncodeAsync(inst, data, params);
	inst.safeDecodeAsync = async (data, params) => safeDecodeAsync(inst, data, params);
	_installLazyMethods(inst, "ZodType", {
		check(...chks) {
			const def$1 = this.def;
			return this.clone(mergeDefs(def$1, { checks: [...def$1.checks ?? [], ...chks.map((ch) => typeof ch === "function" ? { _zod: {
				check: ch,
				def: { check: "custom" },
				onattach: []
			} } : ch)] }), { parent: true });
		},
		with(...chks) {
			return this.check(...chks);
		},
		clone(def$1, params) {
			return clone(this, def$1, params);
		},
		brand() {
			return this;
		},
		register(reg, meta$2) {
			reg.add(this, meta$2);
			return this;
		},
		refine(check, params) {
			return this.check(refine(check, params));
		},
		superRefine(refinement, params) {
			return this.check(superRefine(refinement, params));
		},
		overwrite(fn) {
			return this.check(/* @__PURE__ */ _overwrite(fn));
		},
		optional() {
			return optional(this);
		},
		exactOptional() {
			return exactOptional(this);
		},
		nullable() {
			return nullable(this);
		},
		nullish() {
			return optional(nullable(this));
		},
		nonoptional(params) {
			return nonoptional(this, params);
		},
		array() {
			return array(this);
		},
		or(arg) {
			return union([this, arg]);
		},
		and(arg) {
			return intersection(this, arg);
		},
		transform(tx) {
			return pipe(this, transform(tx));
		},
		default(d) {
			return _default(this, d);
		},
		prefault(d) {
			return prefault(this, d);
		},
		catch(params) {
			return _catch(this, params);
		},
		pipe(target) {
			return pipe(this, target);
		},
		readonly() {
			return readonly(this);
		},
		describe(description) {
			const cl = this.clone();
			globalRegistry.add(cl, { description });
			return cl;
		},
		meta(...args) {
			if (args.length === 0) return globalRegistry.get(this);
			const cl = this.clone();
			globalRegistry.add(cl, args[0]);
			return cl;
		},
		isOptional() {
			return this.safeParse(void 0).success;
		},
		isNullable() {
			return this.safeParse(null).success;
		},
		apply(fn) {
			return fn(this);
		}
	});
	Object.defineProperty(inst, "description", {
		get() {
			return globalRegistry.get(inst)?.description;
		},
		configurable: true
	});
	return inst;
});
const _ZodString = /* @__PURE__ */ $constructor("_ZodString", (inst, def) => {
	$ZodString.init(inst, def);
	ZodType.init(inst, def);
	inst._zod.processJSONSchema = (ctx, json$1, params) => stringProcessor(inst, ctx, json$1, params);
	const bag = inst._zod.bag;
	inst.format = bag.format ?? null;
	inst.minLength = bag.minimum ?? null;
	inst.maxLength = bag.maximum ?? null;
	_installLazyMethods(inst, "_ZodString", {
		regex(...args) {
			return this.check(/* @__PURE__ */ _regex(...args));
		},
		includes(...args) {
			return this.check(/* @__PURE__ */ _includes(...args));
		},
		startsWith(...args) {
			return this.check(/* @__PURE__ */ _startsWith(...args));
		},
		endsWith(...args) {
			return this.check(/* @__PURE__ */ _endsWith(...args));
		},
		min(...args) {
			return this.check(/* @__PURE__ */ _minLength(...args));
		},
		max(...args) {
			return this.check(/* @__PURE__ */ _maxLength(...args));
		},
		length(...args) {
			return this.check(/* @__PURE__ */ _length(...args));
		},
		nonempty(...args) {
			return this.check(/* @__PURE__ */ _minLength(1, ...args));
		},
		lowercase(params) {
			return this.check(/* @__PURE__ */ _lowercase(params));
		},
		uppercase(params) {
			return this.check(/* @__PURE__ */ _uppercase(params));
		},
		trim() {
			return this.check(/* @__PURE__ */ _trim());
		},
		normalize(...args) {
			return this.check(/* @__PURE__ */ _normalize(...args));
		},
		toLowerCase() {
			return this.check(/* @__PURE__ */ _toLowerCase());
		},
		toUpperCase() {
			return this.check(/* @__PURE__ */ _toUpperCase());
		},
		slugify() {
			return this.check(/* @__PURE__ */ _slugify());
		}
	});
});
const ZodString = /* @__PURE__ */ $constructor("ZodString", (inst, def) => {
	$ZodString.init(inst, def);
	_ZodString.init(inst, def);
	inst.email = (params) => inst.check(/* @__PURE__ */ _email(ZodEmail, params));
	inst.url = (params) => inst.check(/* @__PURE__ */ _url(ZodURL, params));
	inst.jwt = (params) => inst.check(/* @__PURE__ */ _jwt(ZodJWT, params));
	inst.emoji = (params) => inst.check(/* @__PURE__ */ _emoji(ZodEmoji, params));
	inst.guid = (params) => inst.check(/* @__PURE__ */ _guid(ZodGUID, params));
	inst.uuid = (params) => inst.check(/* @__PURE__ */ _uuid(ZodUUID, params));
	inst.uuidv4 = (params) => inst.check(/* @__PURE__ */ _uuidv4(ZodUUID, params));
	inst.uuidv6 = (params) => inst.check(/* @__PURE__ */ _uuidv6(ZodUUID, params));
	inst.uuidv7 = (params) => inst.check(/* @__PURE__ */ _uuidv7(ZodUUID, params));
	inst.nanoid = (params) => inst.check(/* @__PURE__ */ _nanoid(ZodNanoID, params));
	inst.guid = (params) => inst.check(/* @__PURE__ */ _guid(ZodGUID, params));
	inst.cuid = (params) => inst.check(/* @__PURE__ */ _cuid(ZodCUID, params));
	inst.cuid2 = (params) => inst.check(/* @__PURE__ */ _cuid2(ZodCUID2, params));
	inst.ulid = (params) => inst.check(/* @__PURE__ */ _ulid(ZodULID, params));
	inst.base64 = (params) => inst.check(/* @__PURE__ */ _base64(ZodBase64, params));
	inst.base64url = (params) => inst.check(/* @__PURE__ */ _base64url(ZodBase64URL, params));
	inst.xid = (params) => inst.check(/* @__PURE__ */ _xid(ZodXID, params));
	inst.ksuid = (params) => inst.check(/* @__PURE__ */ _ksuid(ZodKSUID, params));
	inst.ipv4 = (params) => inst.check(/* @__PURE__ */ _ipv4(ZodIPv4, params));
	inst.ipv6 = (params) => inst.check(/* @__PURE__ */ _ipv6(ZodIPv6, params));
	inst.cidrv4 = (params) => inst.check(/* @__PURE__ */ _cidrv4(ZodCIDRv4, params));
	inst.cidrv6 = (params) => inst.check(/* @__PURE__ */ _cidrv6(ZodCIDRv6, params));
	inst.e164 = (params) => inst.check(/* @__PURE__ */ _e164(ZodE164, params));
	inst.datetime = (params) => inst.check(datetime(params));
	inst.date = (params) => inst.check(date(params));
	inst.time = (params) => inst.check(time(params));
	inst.duration = (params) => inst.check(duration(params));
});
function string(params) {
	return /* @__PURE__ */ _string(ZodString, params);
}
const ZodStringFormat = /* @__PURE__ */ $constructor("ZodStringFormat", (inst, def) => {
	$ZodStringFormat.init(inst, def);
	_ZodString.init(inst, def);
});
const ZodEmail = /* @__PURE__ */ $constructor("ZodEmail", (inst, def) => {
	$ZodEmail.init(inst, def);
	ZodStringFormat.init(inst, def);
});
const ZodGUID = /* @__PURE__ */ $constructor("ZodGUID", (inst, def) => {
	$ZodGUID.init(inst, def);
	ZodStringFormat.init(inst, def);
});
const ZodUUID = /* @__PURE__ */ $constructor("ZodUUID", (inst, def) => {
	$ZodUUID.init(inst, def);
	ZodStringFormat.init(inst, def);
});
function uuid(params) {
	return /* @__PURE__ */ _uuid(ZodUUID, params);
}
const ZodURL = /* @__PURE__ */ $constructor("ZodURL", (inst, def) => {
	$ZodURL.init(inst, def);
	ZodStringFormat.init(inst, def);
});
const ZodEmoji = /* @__PURE__ */ $constructor("ZodEmoji", (inst, def) => {
	$ZodEmoji.init(inst, def);
	ZodStringFormat.init(inst, def);
});
const ZodNanoID = /* @__PURE__ */ $constructor("ZodNanoID", (inst, def) => {
	$ZodNanoID.init(inst, def);
	ZodStringFormat.init(inst, def);
});
const ZodCUID = /* @__PURE__ */ $constructor("ZodCUID", (inst, def) => {
	$ZodCUID.init(inst, def);
	ZodStringFormat.init(inst, def);
});
const ZodCUID2 = /* @__PURE__ */ $constructor("ZodCUID2", (inst, def) => {
	$ZodCUID2.init(inst, def);
	ZodStringFormat.init(inst, def);
});
const ZodULID = /* @__PURE__ */ $constructor("ZodULID", (inst, def) => {
	$ZodULID.init(inst, def);
	ZodStringFormat.init(inst, def);
});
const ZodXID = /* @__PURE__ */ $constructor("ZodXID", (inst, def) => {
	$ZodXID.init(inst, def);
	ZodStringFormat.init(inst, def);
});
const ZodKSUID = /* @__PURE__ */ $constructor("ZodKSUID", (inst, def) => {
	$ZodKSUID.init(inst, def);
	ZodStringFormat.init(inst, def);
});
const ZodIPv4 = /* @__PURE__ */ $constructor("ZodIPv4", (inst, def) => {
	$ZodIPv4.init(inst, def);
	ZodStringFormat.init(inst, def);
});
const ZodIPv6 = /* @__PURE__ */ $constructor("ZodIPv6", (inst, def) => {
	$ZodIPv6.init(inst, def);
	ZodStringFormat.init(inst, def);
});
const ZodCIDRv4 = /* @__PURE__ */ $constructor("ZodCIDRv4", (inst, def) => {
	$ZodCIDRv4.init(inst, def);
	ZodStringFormat.init(inst, def);
});
const ZodCIDRv6 = /* @__PURE__ */ $constructor("ZodCIDRv6", (inst, def) => {
	$ZodCIDRv6.init(inst, def);
	ZodStringFormat.init(inst, def);
});
const ZodBase64 = /* @__PURE__ */ $constructor("ZodBase64", (inst, def) => {
	$ZodBase64.init(inst, def);
	ZodStringFormat.init(inst, def);
});
const ZodBase64URL = /* @__PURE__ */ $constructor("ZodBase64URL", (inst, def) => {
	$ZodBase64URL.init(inst, def);
	ZodStringFormat.init(inst, def);
});
const ZodE164 = /* @__PURE__ */ $constructor("ZodE164", (inst, def) => {
	$ZodE164.init(inst, def);
	ZodStringFormat.init(inst, def);
});
const ZodJWT = /* @__PURE__ */ $constructor("ZodJWT", (inst, def) => {
	$ZodJWT.init(inst, def);
	ZodStringFormat.init(inst, def);
});
const ZodNumber = /* @__PURE__ */ $constructor("ZodNumber", (inst, def) => {
	$ZodNumber.init(inst, def);
	ZodType.init(inst, def);
	inst._zod.processJSONSchema = (ctx, json$1, params) => numberProcessor(inst, ctx, json$1, params);
	_installLazyMethods(inst, "ZodNumber", {
		gt(value, params) {
			return this.check(/* @__PURE__ */ _gt(value, params));
		},
		gte(value, params) {
			return this.check(/* @__PURE__ */ _gte(value, params));
		},
		min(value, params) {
			return this.check(/* @__PURE__ */ _gte(value, params));
		},
		lt(value, params) {
			return this.check(/* @__PURE__ */ _lt(value, params));
		},
		lte(value, params) {
			return this.check(/* @__PURE__ */ _lte(value, params));
		},
		max(value, params) {
			return this.check(/* @__PURE__ */ _lte(value, params));
		},
		int(params) {
			return this.check(int(params));
		},
		safe(params) {
			return this.check(int(params));
		},
		positive(params) {
			return this.check(/* @__PURE__ */ _gt(0, params));
		},
		nonnegative(params) {
			return this.check(/* @__PURE__ */ _gte(0, params));
		},
		negative(params) {
			return this.check(/* @__PURE__ */ _lt(0, params));
		},
		nonpositive(params) {
			return this.check(/* @__PURE__ */ _lte(0, params));
		},
		multipleOf(value, params) {
			return this.check(/* @__PURE__ */ _multipleOf(value, params));
		},
		step(value, params) {
			return this.check(/* @__PURE__ */ _multipleOf(value, params));
		},
		finite() {
			return this;
		}
	});
	const bag = inst._zod.bag;
	inst.minValue = Math.max(bag.minimum ?? Number.NEGATIVE_INFINITY, bag.exclusiveMinimum ?? Number.NEGATIVE_INFINITY) ?? null;
	inst.maxValue = Math.min(bag.maximum ?? Number.POSITIVE_INFINITY, bag.exclusiveMaximum ?? Number.POSITIVE_INFINITY) ?? null;
	inst.isInt = (bag.format ?? "").includes("int") || Number.isSafeInteger(bag.multipleOf ?? .5);
	inst.isFinite = true;
	inst.format = bag.format ?? null;
});
function number(params) {
	return /* @__PURE__ */ _number(ZodNumber, params);
}
const ZodNumberFormat = /* @__PURE__ */ $constructor("ZodNumberFormat", (inst, def) => {
	$ZodNumberFormat.init(inst, def);
	ZodNumber.init(inst, def);
});
function int(params) {
	return /* @__PURE__ */ _int(ZodNumberFormat, params);
}
const ZodBoolean = /* @__PURE__ */ $constructor("ZodBoolean", (inst, def) => {
	$ZodBoolean.init(inst, def);
	ZodType.init(inst, def);
	inst._zod.processJSONSchema = (ctx, json$1, params) => booleanProcessor(inst, ctx, json$1, params);
});
function boolean(params) {
	return /* @__PURE__ */ _boolean(ZodBoolean, params);
}
const ZodNull = /* @__PURE__ */ $constructor("ZodNull", (inst, def) => {
	$ZodNull.init(inst, def);
	ZodType.init(inst, def);
	inst._zod.processJSONSchema = (ctx, json$1, params) => nullProcessor(inst, ctx, json$1, params);
});
function _null(params) {
	return /* @__PURE__ */ _null$1(ZodNull, params);
}
const ZodUnknown = /* @__PURE__ */ $constructor("ZodUnknown", (inst, def) => {
	$ZodUnknown.init(inst, def);
	ZodType.init(inst, def);
	inst._zod.processJSONSchema = (ctx, json$1, params) => unknownProcessor(inst, ctx, json$1, params);
});
function unknown() {
	return /* @__PURE__ */ _unknown(ZodUnknown);
}
const ZodNever = /* @__PURE__ */ $constructor("ZodNever", (inst, def) => {
	$ZodNever.init(inst, def);
	ZodType.init(inst, def);
	inst._zod.processJSONSchema = (ctx, json$1, params) => neverProcessor(inst, ctx, json$1, params);
});
function never(params) {
	return /* @__PURE__ */ _never(ZodNever, params);
}
const ZodArray = /* @__PURE__ */ $constructor("ZodArray", (inst, def) => {
	$ZodArray.init(inst, def);
	ZodType.init(inst, def);
	inst._zod.processJSONSchema = (ctx, json$1, params) => arrayProcessor(inst, ctx, json$1, params);
	inst.element = def.element;
	_installLazyMethods(inst, "ZodArray", {
		min(n, params) {
			return this.check(/* @__PURE__ */ _minLength(n, params));
		},
		nonempty(params) {
			return this.check(/* @__PURE__ */ _minLength(1, params));
		},
		max(n, params) {
			return this.check(/* @__PURE__ */ _maxLength(n, params));
		},
		length(n, params) {
			return this.check(/* @__PURE__ */ _length(n, params));
		},
		unwrap() {
			return this.element;
		}
	});
});
function array(element, params) {
	return /* @__PURE__ */ _array(ZodArray, element, params);
}
const ZodObject = /* @__PURE__ */ $constructor("ZodObject", (inst, def) => {
	$ZodObjectJIT.init(inst, def);
	ZodType.init(inst, def);
	inst._zod.processJSONSchema = (ctx, json$1, params) => objectProcessor(inst, ctx, json$1, params);
	defineLazy(inst, "shape", () => {
		return def.shape;
	});
	_installLazyMethods(inst, "ZodObject", {
		keyof() {
			return _enum(Object.keys(this._zod.def.shape));
		},
		catchall(catchall) {
			return this.clone({
				...this._zod.def,
				catchall
			});
		},
		passthrough() {
			return this.clone({
				...this._zod.def,
				catchall: unknown()
			});
		},
		loose() {
			return this.clone({
				...this._zod.def,
				catchall: unknown()
			});
		},
		strict() {
			return this.clone({
				...this._zod.def,
				catchall: never()
			});
		},
		strip() {
			return this.clone({
				...this._zod.def,
				catchall: void 0
			});
		},
		extend(incoming) {
			return extend(this, incoming);
		},
		safeExtend(incoming) {
			return safeExtend(this, incoming);
		},
		merge(other) {
			return merge(this, other);
		},
		pick(mask) {
			return pick(this, mask);
		},
		omit(mask) {
			return omit(this, mask);
		},
		partial(...args) {
			return partial(ZodOptional, this, args[0]);
		},
		required(...args) {
			return required(ZodNonOptional, this, args[0]);
		}
	});
});
function object(shape, params) {
	return new ZodObject({
		type: "object",
		shape: shape ?? {},
		...normalizeParams(params)
	});
}
const ZodUnion = /* @__PURE__ */ $constructor("ZodUnion", (inst, def) => {
	$ZodUnion.init(inst, def);
	ZodType.init(inst, def);
	inst._zod.processJSONSchema = (ctx, json$1, params) => unionProcessor(inst, ctx, json$1, params);
	inst.options = def.options;
});
function union(options, params) {
	return new ZodUnion({
		type: "union",
		options,
		...normalizeParams(params)
	});
}
const ZodDiscriminatedUnion = /* @__PURE__ */ $constructor("ZodDiscriminatedUnion", (inst, def) => {
	ZodUnion.init(inst, def);
	$ZodDiscriminatedUnion.init(inst, def);
});
function discriminatedUnion(discriminator, options, params) {
	return new ZodDiscriminatedUnion({
		type: "union",
		options,
		discriminator,
		...normalizeParams(params)
	});
}
const ZodIntersection = /* @__PURE__ */ $constructor("ZodIntersection", (inst, def) => {
	$ZodIntersection.init(inst, def);
	ZodType.init(inst, def);
	inst._zod.processJSONSchema = (ctx, json$1, params) => intersectionProcessor(inst, ctx, json$1, params);
});
function intersection(left, right) {
	return new ZodIntersection({
		type: "intersection",
		left,
		right
	});
}
const ZodRecord = /* @__PURE__ */ $constructor("ZodRecord", (inst, def) => {
	$ZodRecord.init(inst, def);
	ZodType.init(inst, def);
	inst._zod.processJSONSchema = (ctx, json$1, params) => recordProcessor(inst, ctx, json$1, params);
	inst.keyType = def.keyType;
	inst.valueType = def.valueType;
});
function record(keyType, valueType, params) {
	if (!valueType || !valueType._zod) return new ZodRecord({
		type: "record",
		keyType: string(),
		valueType: keyType,
		...normalizeParams(valueType)
	});
	return new ZodRecord({
		type: "record",
		keyType,
		valueType,
		...normalizeParams(params)
	});
}
const ZodEnum = /* @__PURE__ */ $constructor("ZodEnum", (inst, def) => {
	$ZodEnum.init(inst, def);
	ZodType.init(inst, def);
	inst._zod.processJSONSchema = (ctx, json$1, params) => enumProcessor(inst, ctx, json$1, params);
	inst.enum = def.entries;
	inst.options = Object.values(def.entries);
	const keys = new Set(Object.keys(def.entries));
	inst.extract = (values, params) => {
		const newEntries = {};
		for (const value of values) if (keys.has(value)) newEntries[value] = def.entries[value];
		else throw new Error(`Key ${value} not found in enum`);
		return new ZodEnum({
			...def,
			checks: [],
			...normalizeParams(params),
			entries: newEntries
		});
	};
	inst.exclude = (values, params) => {
		const newEntries = { ...def.entries };
		for (const value of values) if (keys.has(value)) delete newEntries[value];
		else throw new Error(`Key ${value} not found in enum`);
		return new ZodEnum({
			...def,
			checks: [],
			...normalizeParams(params),
			entries: newEntries
		});
	};
});
function _enum(values, params) {
	return new ZodEnum({
		type: "enum",
		entries: Array.isArray(values) ? Object.fromEntries(values.map((v) => [v, v])) : values,
		...normalizeParams(params)
	});
}
const ZodLiteral = /* @__PURE__ */ $constructor("ZodLiteral", (inst, def) => {
	$ZodLiteral.init(inst, def);
	ZodType.init(inst, def);
	inst._zod.processJSONSchema = (ctx, json$1, params) => literalProcessor(inst, ctx, json$1, params);
	inst.values = new Set(def.values);
	Object.defineProperty(inst, "value", { get() {
		if (def.values.length > 1) throw new Error("This schema contains multiple valid literal values. Use `.values` instead.");
		return def.values[0];
	} });
});
function literal(value, params) {
	return new ZodLiteral({
		type: "literal",
		values: Array.isArray(value) ? value : [value],
		...normalizeParams(params)
	});
}
const ZodTransform = /* @__PURE__ */ $constructor("ZodTransform", (inst, def) => {
	$ZodTransform.init(inst, def);
	ZodType.init(inst, def);
	inst._zod.processJSONSchema = (ctx, json$1, params) => transformProcessor(inst, ctx, json$1, params);
	inst._zod.parse = (payload, _ctx) => {
		if (_ctx.direction === "backward") throw new $ZodEncodeError(inst.constructor.name);
		payload.addIssue = (issue$1) => {
			if (typeof issue$1 === "string") payload.issues.push(issue(issue$1, payload.value, def));
			else {
				const _issue = issue$1;
				if (_issue.fatal) _issue.continue = false;
				_issue.code ?? (_issue.code = "custom");
				_issue.input ?? (_issue.input = payload.value);
				_issue.inst ?? (_issue.inst = inst);
				payload.issues.push(issue(_issue));
			}
		};
		const output = def.transform(payload.value, payload);
		if (output instanceof Promise) return output.then((output$1) => {
			payload.value = output$1;
			payload.fallback = true;
			return payload;
		});
		payload.value = output;
		payload.fallback = true;
		return payload;
	};
});
function transform(fn) {
	return new ZodTransform({
		type: "transform",
		transform: fn
	});
}
const ZodOptional = /* @__PURE__ */ $constructor("ZodOptional", (inst, def) => {
	$ZodOptional.init(inst, def);
	ZodType.init(inst, def);
	inst._zod.processJSONSchema = (ctx, json$1, params) => optionalProcessor(inst, ctx, json$1, params);
	inst.unwrap = () => inst._zod.def.innerType;
});
function optional(innerType) {
	return new ZodOptional({
		type: "optional",
		innerType
	});
}
const ZodExactOptional = /* @__PURE__ */ $constructor("ZodExactOptional", (inst, def) => {
	$ZodExactOptional.init(inst, def);
	ZodType.init(inst, def);
	inst._zod.processJSONSchema = (ctx, json$1, params) => optionalProcessor(inst, ctx, json$1, params);
	inst.unwrap = () => inst._zod.def.innerType;
});
function exactOptional(innerType) {
	return new ZodExactOptional({
		type: "optional",
		innerType
	});
}
const ZodNullable = /* @__PURE__ */ $constructor("ZodNullable", (inst, def) => {
	$ZodNullable.init(inst, def);
	ZodType.init(inst, def);
	inst._zod.processJSONSchema = (ctx, json$1, params) => nullableProcessor(inst, ctx, json$1, params);
	inst.unwrap = () => inst._zod.def.innerType;
});
function nullable(innerType) {
	return new ZodNullable({
		type: "nullable",
		innerType
	});
}
const ZodDefault = /* @__PURE__ */ $constructor("ZodDefault", (inst, def) => {
	$ZodDefault.init(inst, def);
	ZodType.init(inst, def);
	inst._zod.processJSONSchema = (ctx, json$1, params) => defaultProcessor(inst, ctx, json$1, params);
	inst.unwrap = () => inst._zod.def.innerType;
	inst.removeDefault = inst.unwrap;
});
function _default(innerType, defaultValue) {
	return new ZodDefault({
		type: "default",
		innerType,
		get defaultValue() {
			return typeof defaultValue === "function" ? defaultValue() : shallowClone(defaultValue);
		}
	});
}
const ZodPrefault = /* @__PURE__ */ $constructor("ZodPrefault", (inst, def) => {
	$ZodPrefault.init(inst, def);
	ZodType.init(inst, def);
	inst._zod.processJSONSchema = (ctx, json$1, params) => prefaultProcessor(inst, ctx, json$1, params);
	inst.unwrap = () => inst._zod.def.innerType;
});
function prefault(innerType, defaultValue) {
	return new ZodPrefault({
		type: "prefault",
		innerType,
		get defaultValue() {
			return typeof defaultValue === "function" ? defaultValue() : shallowClone(defaultValue);
		}
	});
}
const ZodNonOptional = /* @__PURE__ */ $constructor("ZodNonOptional", (inst, def) => {
	$ZodNonOptional.init(inst, def);
	ZodType.init(inst, def);
	inst._zod.processJSONSchema = (ctx, json$1, params) => nonoptionalProcessor(inst, ctx, json$1, params);
	inst.unwrap = () => inst._zod.def.innerType;
});
function nonoptional(innerType, params) {
	return new ZodNonOptional({
		type: "nonoptional",
		innerType,
		...normalizeParams(params)
	});
}
const ZodCatch = /* @__PURE__ */ $constructor("ZodCatch", (inst, def) => {
	$ZodCatch.init(inst, def);
	ZodType.init(inst, def);
	inst._zod.processJSONSchema = (ctx, json$1, params) => catchProcessor(inst, ctx, json$1, params);
	inst.unwrap = () => inst._zod.def.innerType;
	inst.removeCatch = inst.unwrap;
});
function _catch(innerType, catchValue) {
	return new ZodCatch({
		type: "catch",
		innerType,
		catchValue: typeof catchValue === "function" ? catchValue : () => catchValue
	});
}
const ZodPipe = /* @__PURE__ */ $constructor("ZodPipe", (inst, def) => {
	$ZodPipe.init(inst, def);
	ZodType.init(inst, def);
	inst._zod.processJSONSchema = (ctx, json$1, params) => pipeProcessor(inst, ctx, json$1, params);
	inst.in = def.in;
	inst.out = def.out;
});
function pipe(in_, out) {
	return new ZodPipe({
		type: "pipe",
		in: in_,
		out
	});
}
const ZodReadonly = /* @__PURE__ */ $constructor("ZodReadonly", (inst, def) => {
	$ZodReadonly.init(inst, def);
	ZodType.init(inst, def);
	inst._zod.processJSONSchema = (ctx, json$1, params) => readonlyProcessor(inst, ctx, json$1, params);
	inst.unwrap = () => inst._zod.def.innerType;
});
function readonly(innerType) {
	return new ZodReadonly({
		type: "readonly",
		innerType
	});
}
const ZodLazy = /* @__PURE__ */ $constructor("ZodLazy", (inst, def) => {
	$ZodLazy.init(inst, def);
	ZodType.init(inst, def);
	inst._zod.processJSONSchema = (ctx, json$1, params) => lazyProcessor(inst, ctx, json$1, params);
	inst.unwrap = () => inst._zod.def.getter();
});
function lazy(getter) {
	return new ZodLazy({
		type: "lazy",
		getter
	});
}
const ZodCustom = /* @__PURE__ */ $constructor("ZodCustom", (inst, def) => {
	$ZodCustom.init(inst, def);
	ZodType.init(inst, def);
	inst._zod.processJSONSchema = (ctx, json$1, params) => customProcessor(inst, ctx, json$1, params);
});
function refine(fn, _params = {}) {
	return /* @__PURE__ */ _refine(ZodCustom, fn, _params);
}
function superRefine(fn, params) {
	return /* @__PURE__ */ _superRefine(fn, params);
}
function json(params) {
	const jsonSchema = lazy(() => {
		return union([
			string(params),
			number(),
			boolean(),
			_null(),
			array(jsonSchema),
			record(string(), jsonSchema)
		]);
	});
	return jsonSchema;
}
var createStoreImpl = (createState) => {
	let state;
	const listeners = /* @__PURE__ */ new Set();
	const setState = (partial$1, replace) => {
		const nextState = typeof partial$1 === "function" ? partial$1(state) : partial$1;
		if (!Object.is(nextState, state)) {
			const previousState = state;
			state = (replace != null ? replace : typeof nextState !== "object" || nextState === null) ? nextState : Object.assign({}, state, nextState);
			listeners.forEach((listener) => listener(state, previousState));
		}
	};
	const getState = () => state;
	const getInitialState = () => initialState;
	const subscribe = (listener) => {
		listeners.add(listener);
		return () => listeners.delete(listener);
	};
	const api = {
		setState,
		getState,
		getInitialState,
		subscribe
	};
	const initialState = state = createState(setState, getState, api);
	return api;
};
var createStore = ((createState) => createState ? createStoreImpl(createState) : createStoreImpl);
var import_react = /* @__PURE__ */ __toESM(require_react(), 1);
var identity = (arg) => arg;
function useStore(api, selector = identity) {
	const slice = import_react.useSyncExternalStore(api.subscribe, import_react.useCallback(() => selector(api.getState()), [api, selector]), import_react.useCallback(() => selector(api.getInitialState()), [api, selector]));
	import_react.useDebugValue(slice);
	return slice;
}
var createImpl = (createState) => {
	const api = createStore(createState);
	const useBoundStore = (selector) => useStore(api, selector);
	Object.assign(useBoundStore, api);
	return useBoundStore;
};
var create = ((createState) => createState ? createImpl(createState) : createImpl);
const pluginCapabilitySchema = object({ kind: _enum([
	"workspace:read",
	"terminal:send",
	"notifications:show",
	"storage",
	"secrets",
	"events:subscribe",
	"settings:own"
]) }).strict();
var WINDOWS_DEVICE_NAME_RE = /^(?:con|prn|aux|nul|clock\$|conin\$|conout\$|com[1-9¹²³]|lpt[1-9¹²³])(?:\..*)?$/i;
var WINDOWS_FORBIDDEN_CHAR_RE = /[<>:"|?*]/;
function pluginPathSegmentError(segment) {
	if (segment.length === 0 || segment === "." || segment === "..") return "empty and dot path segments are not allowed";
	if (segment.endsWith(".") || segment.endsWith(" ")) return "path segments may not end with a dot or space";
	if (WINDOWS_FORBIDDEN_CHAR_RE.test(segment) || [...segment].some((character) => character.charCodeAt(0) <= 31)) return "path segment contains a Windows-forbidden character or alternate-data-stream colon";
	if (WINDOWS_DEVICE_NAME_RE.test(segment)) return "path segment is a Windows reserved device name";
	return null;
}
function pluginRelativePathError(value) {
	if (value.length === 0 || value.startsWith("/") || value.startsWith("\\")) return "must be a non-empty relative path";
	const segments = value.split(/[\\/]/);
	for (const segment of segments) {
		const error = pluginPathSegmentError(segment);
		if (error) return error;
	}
	return null;
}
function isSafePluginRelativePath(value) {
	return pluginRelativePathError(value) === null;
}
var PLUGIN_ID_RE = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
var DANGEROUS_PLUGIN_NAMES = new Set([
	"__proto__",
	"prototype",
	"constructor"
]);
function isSafePluginId(id) {
	return typeof id === "string" && id.length <= 64 && PLUGIN_ID_RE.test(id) && !DANGEROUS_PLUGIN_NAMES.has(id);
}
const pluginIdSchema = string().refine(isSafePluginId, "must be kebab-case (a-z, 0-9, dashes) and not a reserved name");
const pluginRelativePathSchema = string().min(1).max(1024).refine(isSafePluginRelativePath, "must be a portable relative path inside the plugin directory");
string().min(1).max(1024).transform((value) => value.replace(/[\\/]+$/, "")).refine(isSafePluginRelativePath, "must be a portable relative path inside the plugin directory");
const pluginCommandIdSchema = string().min(1).max(256).regex(/^[A-Za-z0-9]+(?:[._-][A-Za-z0-9]+)*$/, "must be a portable command id");
function isPluginManifestId(value) {
	return PLUGIN_ID_RE.test(value);
}
const TUI_AGENT_DISPLAY_NAMES = {
	claude: "Claude",
	"claude-agent-teams": "Claude Agent Teams",
	openclaude: "OpenClaude",
	codex: "Codex",
	devin: "Devin",
	ante: "Ante",
	trae: "Trae",
	autohand: "Autohand Code",
	opencode: "OpenCode",
	"mimo-code": "MiMo Code",
	pi: "Pi",
	omp: "OMP",
	"prime-agent": "Prime Agent",
	gemini: "Gemini",
	antigravity: "Antigravity",
	aider: "Aider",
	goose: "Goose",
	amp: "Amp",
	kilo: "Kilocode",
	kiro: "Kiro",
	crush: "Charm",
	aug: "Auggie",
	cline: "Cline",
	codebuff: "Codebuff",
	"command-code": "Command Code",
	continue: "Continue",
	cursor: "Cursor",
	droid: "Droid",
	kimi: "Kimi",
	"mistral-vibe": "Mistral Vibe",
	"qwen-code": "Qwen Code",
	rovo: "Rovo Dev",
	hermes: "Hermes",
	openclaw: "OpenClaw",
	copilot: "GitHub Copilot",
	grok: "Grok"
};
const ALL_TUI_AGENTS = Object.keys(TUI_AGENT_DISPLAY_NAMES);
const KEYBINDING_DEFINITIONS = [
	{
		id: "worktree.quickOpen",
		title: "Go to File",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"global",
			"file",
			"quick open"
		],
		defaultBindings: platformBindings(["Mod+P"])
	},
	{
		id: "app.settings",
		title: "Open Settings",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"settings",
			"preferences"
		],
		defaultBindings: platformBindings(["Mod+Comma"]),
		conflictGroup: "menu"
	},
	{
		id: "app.forceReload",
		title: "Force Reload",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"reload",
			"refresh",
			"force"
		],
		defaultBindings: platformBindings(["Mod+Shift+R"]),
		conflictGroup: "menu"
	},
	{
		id: "worktree.palette",
		title: "Switch worktree",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"global",
			"worktree",
			"switch",
			"jump"
		],
		defaultBindings: {
			darwin: ["Mod+J"],
			linux: ["Mod+Shift+J"],
			win32: ["Mod+Shift+J"]
		}
	},
	{
		id: "worktree.navigateUp",
		title: "Previous worktree",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"global",
			"worktree",
			"previous",
			"up"
		],
		defaultBindings: platformBindings(["Mod+Shift+ArrowUp"])
	},
	{
		id: "worktree.navigateDown",
		title: "Next worktree",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"global",
			"worktree",
			"next",
			"down"
		],
		defaultBindings: platformBindings(["Mod+Shift+ArrowDown"])
	},
	{
		id: "workspace.create",
		title: "Create worktree",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"global",
			"worktree",
			"create",
			"new workspace"
		],
		defaultBindings: platformBindings(["Mod+N", "Mod+Shift+N"])
	},
	{
		id: "workspace.rename",
		title: "Rename worktree",
		group: "Global",
		scope: "global",
		conflictGroup: "workspace-shell",
		searchKeywords: [
			"shortcut",
			"global",
			"worktree",
			"rename",
			"workspace",
			"title"
		],
		defaultBindings: {
			darwin: ["Mod+Alt+R"],
			linux: [],
			win32: []
		}
	},
	{
		id: "workspace.delete",
		title: "Delete Workspace",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"global",
			"workspace",
			"current workspace",
			"worktree",
			"delete",
			"remove",
			"trash"
		],
		defaultBindings: platformBindings([]),
		allowInTerminal: true
	},
	{
		id: "workspace.openBoard",
		title: "Toggle Workspace Board",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"global",
			"workspace",
			"board",
			"kanban",
			"worktree",
			"toggle",
			"open",
			"close"
		],
		defaultBindings: platformBindings([]),
		allowInTerminal: true
	},
	{
		id: "workspace.selectByIndex",
		title: "Select Workspace 1–9",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"global",
			"workspace",
			"worktree",
			"select",
			"switch",
			"number",
			"digit",
			"1-9",
			"index"
		],
		defaultBindings: platformBindings(["Mod+1"])
	},
	{
		id: "voice.dictation",
		title: "Dictation",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"dictation",
			"voice",
			"speech",
			"microphone"
		],
		defaultBindings: platformBindings(["Mod+E"])
	},
	{
		id: "view.tasks",
		title: "Open Tasks",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"tasks",
			"github issues",
			"linear"
		],
		defaultBindings: platformBindings([])
	},
	{
		id: "sidebar.left.toggle",
		title: "Toggle Sidebar",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"sidebar",
			"left"
		],
		defaultBindings: platformBindings(["Mod+B"])
	},
	{
		id: "sidebar.right.toggle",
		title: "Toggle Right Sidebar",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"sidebar",
			"right"
		],
		defaultBindings: platformBindings(["Mod+L"])
	},
	{
		id: "sidebar.explorer.toggle",
		title: "Show Explorer",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"sidebar",
			"explorer",
			"files"
		],
		defaultBindings: platformBindings(["Mod+Shift+E"])
	},
	{
		id: "sidebar.search.toggle",
		title: "Show Search",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"sidebar",
			"search"
		],
		defaultBindings: platformBindings(["Mod+Shift+F"])
	},
	{
		id: "sidebar.sourceControl.toggle",
		title: "Show Source Control",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"sidebar",
			"source control",
			"git"
		],
		defaultBindings: platformBindings(["Mod+Shift+G"])
	},
	{
		id: "sidebar.checks.toggle",
		title: "Show Checks",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"sidebar",
			"checks",
			"ci"
		],
		defaultBindings: platformBindings([])
	},
	{
		id: "sidebar.ports.toggle",
		title: "Show Ports",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"sidebar",
			"ports"
		],
		defaultBindings: {
			darwin: ["Mod+Shift+I"],
			linux: [],
			win32: []
		}
	},
	{
		id: "sidebar.sleepingWorkspaces.toggle",
		title: "Toggle Sleeping Workspaces",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"sidebar",
			"sleeping",
			"asleep",
			"workspaces",
			"worktree",
			"filter",
			"show",
			"hide"
		],
		defaultBindings: platformBindings([])
	},
	{
		id: "sidebar.focusWorktreeList",
		title: "Focus worktree list",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"sidebar",
			"worktree",
			"focus"
		],
		defaultBindings: platformBindings(["Mod+Shift+0"])
	},
	{
		id: "floatingTerminal.toggle",
		title: "Toggle Floating Terminal",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"floating terminal",
			"terminal"
		],
		defaultBindings: platformBindings(["Mod+Alt+A"]),
		allowInTerminal: true
	},
	{
		id: "floatingWorkspace.maximize",
		title: "Maximize Floating Workspace Panel",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"floating",
			"workspace",
			"panel",
			"floating workspace",
			"workspace panel",
			"maximize",
			"expand"
		],
		defaultBindings: {
			darwin: ["Mod+Alt+Shift+A"],
			linux: [],
			win32: []
		},
		allowInTerminal: true
	},
	{
		id: "floatingWorkspace.minimize",
		title: "Minimize Floating Workspace Panel",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"floating",
			"workspace",
			"panel",
			"floating workspace",
			"workspace panel",
			"minimize",
			"hide"
		],
		defaultBindings: {
			darwin: [],
			linux: [],
			win32: []
		},
		allowInTerminal: true
	},
	{
		id: "zoom.in",
		title: "Zoom In",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"zoom",
			"in",
			"scale"
		],
		defaultBindings: platformBindings([
			"Mod+Equal",
			"Mod+Shift+Plus",
			"Mod+NumpadAdd"
		])
	},
	{
		id: "zoom.out",
		title: "Zoom Out",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"zoom",
			"out",
			"scale"
		],
		defaultBindings: platformBindings(["Mod+Minus", "Mod+NumpadSubtract"])
	},
	{
		id: "zoom.reset",
		title: "Reset Size",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"zoom",
			"reset",
			"size",
			"actual"
		],
		defaultBindings: platformBindings(["Mod+0"])
	},
	{
		id: "worktree.history.back",
		title: "Worktree History Back",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"worktree",
			"history",
			"back"
		],
		defaultBindings: platformBindings(["Mod+Alt+ArrowLeft"]),
		allowInTerminal: true
	},
	{
		id: "worktree.history.forward",
		title: "Worktree History Forward",
		group: "Global",
		scope: "global",
		searchKeywords: [
			"shortcut",
			"worktree",
			"history",
			"forward"
		],
		defaultBindings: platformBindings(["Mod+Alt+ArrowRight"]),
		allowInTerminal: true
	},
	{
		id: "tab.newTerminal",
		title: "New terminal tab",
		group: "Tabs",
		scope: "tabs",
		searchKeywords: [
			"shortcut",
			"tab",
			"terminal",
			"new"
		],
		defaultBindings: platformBindings(["Mod+T"])
	},
	{
		id: "tab.newAgent",
		title: "New agent tab (default agent)",
		group: "Tabs",
		scope: "tabs",
		searchKeywords: [
			"shortcut",
			"tab",
			"agent",
			"new",
			"default",
			"launch"
		],
		defaultBindings: {
			darwin: ["Mod+Alt+T"],
			linux: [],
			win32: []
		}
	},
	{
		id: "tab.newBrowser",
		title: "New browser tab",
		group: "Tabs",
		scope: "tabs",
		searchKeywords: [
			"shortcut",
			"tab",
			"browser",
			"new"
		],
		defaultBindings: platformBindings(["Mod+Shift+B"])
	},
	{
		id: "tab.newSimulator",
		title: "New mobile emulator tab",
		group: "Tabs",
		scope: "tabs",
		searchKeywords: [
			"shortcut",
			"tab",
			"simulator",
			"emulator",
			"mobile",
			"ios",
			"new"
		],
		defaultBindings: {
			darwin: ["Mod+Alt+Shift+E"],
			linux: [],
			win32: []
		}
	},
	{
		id: "tab.newMarkdown",
		title: "New markdown tab",
		group: "Tabs",
		scope: "tabs",
		searchKeywords: [
			"shortcut",
			"tab",
			"markdown",
			"file",
			"new"
		],
		defaultBindings: platformBindings(["Mod+Shift+M"])
	},
	{
		id: "tab.openMarkdown",
		title: "Open markdown tab",
		group: "Tabs",
		scope: "tabs",
		searchKeywords: [
			"shortcut",
			"tab",
			"markdown",
			"file",
			"open"
		],
		defaultBindings: platformBindings(["Mod+Shift+O"])
	},
	{
		id: "tab.close",
		title: "Close active tab",
		group: "Tabs",
		scope: "tabs",
		searchKeywords: [
			"shortcut",
			"close",
			"tab",
			"pane"
		],
		defaultBindings: platformBindings(["Mod+W"])
	},
	{
		id: "tab.closeAll",
		title: "Close all editor tabs",
		group: "Tabs",
		scope: "tabs",
		searchKeywords: [
			"shortcut",
			"close",
			"all",
			"tabs",
			"files",
			"editors"
		],
		defaultBindings: platformBindings(["Mod+Alt+W"])
	},
	{
		id: "tab.rename",
		title: "Rename active tab",
		group: "Tabs",
		scope: "tabs",
		conflictGroup: "workspace-shell",
		searchKeywords: [
			"shortcut",
			"tab",
			"rename",
			"title",
			"label"
		],
		defaultBindings: {
			darwin: ["Mod+R"],
			linux: [],
			win32: []
		}
	},
	{
		id: "tab.reopenClosed",
		title: "Reopen closed tab",
		group: "Tabs",
		scope: "tabs",
		searchKeywords: [
			"shortcut",
			"tab",
			"reopen",
			"restore",
			"closed"
		],
		defaultBindings: platformBindings(["Mod+Shift+T"])
	},
	{
		id: "tab.nextSameType",
		title: "Next tab (same type)",
		group: "Tab Navigation",
		scope: "tabs",
		searchKeywords: [
			"shortcut",
			"tab",
			"next",
			"switch",
			"cycle"
		],
		defaultBindings: platformBindings(["Mod+Alt+BracketRight"])
	},
	{
		id: "tab.previousSameType",
		title: "Previous tab (same type)",
		group: "Tab Navigation",
		scope: "tabs",
		searchKeywords: [
			"shortcut",
			"tab",
			"previous",
			"switch",
			"cycle"
		],
		defaultBindings: platformBindings(["Mod+Alt+BracketLeft"])
	},
	{
		id: "tab.nextAllTypes",
		title: "Next tab (all types)",
		group: "Tab Navigation",
		scope: "tabs",
		searchKeywords: [
			"shortcut",
			"tab",
			"next",
			"switch",
			"cycle",
			"all",
			"any"
		],
		defaultBindings: platformBindings(["Mod+Shift+BracketRight"])
	},
	{
		id: "tab.previousAllTypes",
		title: "Previous tab (all types)",
		group: "Tab Navigation",
		scope: "tabs",
		searchKeywords: [
			"shortcut",
			"tab",
			"previous",
			"switch",
			"cycle",
			"all",
			"any"
		],
		defaultBindings: platformBindings(["Mod+Shift+BracketLeft"])
	},
	{
		id: "tab.previousRecent",
		title: "Previous recent tab",
		group: "Tab Navigation",
		scope: "tabs",
		searchKeywords: [
			"shortcut",
			"tab",
			"recent",
			"mru",
			"switch",
			"last used"
		],
		defaultBindings: platformBindings(["Ctrl+Tab"]),
		allowInTerminal: true
	},
	{
		id: "tab.nextTerminal",
		title: "Next terminal tab",
		group: "Tab Navigation",
		scope: "tabs",
		searchKeywords: [
			"shortcut",
			"tab",
			"terminal",
			"next",
			"switch"
		],
		defaultBindings: platformBindings(["Ctrl+PageDown"]),
		allowInTerminal: true
	},
	{
		id: "tab.previousTerminal",
		title: "Previous terminal tab",
		group: "Tab Navigation",
		scope: "tabs",
		searchKeywords: [
			"shortcut",
			"tab",
			"terminal",
			"previous",
			"switch"
		],
		defaultBindings: platformBindings(["Ctrl+PageUp"]),
		allowInTerminal: true
	},
	{
		id: "tab.selectByIndex",
		title: "Select Tab 1–9",
		group: "Tab Navigation",
		scope: "tabs",
		searchKeywords: [
			"shortcut",
			"tab",
			"select",
			"switch",
			"number",
			"digit",
			"1-9",
			"index"
		],
		defaultBindings: {
			darwin: ["Ctrl+1"],
			linux: ["Alt+1"],
			win32: ["Alt+1"]
		}
	},
	{
		id: "tab.openQuickCommandsMenu",
		title: "Toggle Quick Commands menu",
		group: "Quick Commands",
		scope: "tabs",
		conflictGroup: "global",
		searchKeywords: [
			"shortcut",
			"quick",
			"command",
			"menu",
			"tab",
			"group",
			"toggle"
		],
		defaultBindings: platformBindings([])
	},
	{
		id: "browser.find",
		title: "Find in Browser",
		group: "Browser",
		scope: "browser",
		searchKeywords: [
			"shortcut",
			"browser",
			"find",
			"search"
		],
		defaultBindings: platformBindings(["Mod+F"])
	},
	{
		id: "browser.back",
		title: "Go Back in Browser",
		group: "Browser",
		scope: "browser",
		searchKeywords: [
			"shortcut",
			"browser",
			"history",
			"back",
			"previous"
		],
		defaultBindings: {
			darwin: ["Mod+BracketLeft"],
			linux: ["Alt+ArrowLeft"],
			win32: ["Alt+ArrowLeft"]
		}
	},
	{
		id: "browser.forward",
		title: "Go Forward in Browser",
		group: "Browser",
		scope: "browser",
		searchKeywords: [
			"shortcut",
			"browser",
			"history",
			"forward",
			"next"
		],
		defaultBindings: {
			darwin: ["Mod+BracketRight"],
			linux: ["Alt+ArrowRight"],
			win32: ["Alt+ArrowRight"]
		}
	},
	{
		id: "browser.reload",
		title: "Reload Browser Page",
		group: "Browser",
		scope: "browser",
		searchKeywords: [
			"shortcut",
			"browser",
			"reload",
			"refresh"
		],
		defaultBindings: platformBindings(["Mod+R"])
	},
	{
		id: "browser.hardReload",
		title: "Hard Reload Browser Page",
		group: "Browser",
		scope: "browser",
		searchKeywords: [
			"shortcut",
			"browser",
			"reload",
			"refresh",
			"cache"
		],
		defaultBindings: platformBindings(["Mod+Shift+R"])
	},
	{
		id: "browser.focusAddressBar",
		title: "Focus Browser Address Bar",
		group: "Browser",
		scope: "browser",
		searchKeywords: [
			"shortcut",
			"browser",
			"address",
			"url",
			"location"
		],
		defaultBindings: platformBindings(["Mod+L"])
	},
	{
		id: "browser.grabElement",
		title: "Grab Page Element",
		group: "Browser",
		scope: "browser",
		searchKeywords: [
			"shortcut",
			"browser",
			"grab",
			"copy",
			"element"
		],
		defaultBindings: platformBindings(["Mod+C"])
	},
	{
		id: "editor.find",
		title: "Find in editor",
		group: "Editors",
		scope: "editor",
		searchKeywords: [
			"shortcut",
			"editor",
			"find",
			"search"
		],
		defaultBindings: platformBindings(["Mod+F"])
	},
	{
		id: "editor.replace",
		title: "Replace in editor",
		group: "Editors",
		scope: "editor",
		searchKeywords: [
			"shortcut",
			"editor",
			"replace",
			"find",
			"search"
		],
		defaultBindings: {
			darwin: ["Mod+Alt+F"],
			linux: ["Mod+H"],
			win32: ["Mod+H"]
		}
	},
	{
		id: "editor.save",
		title: "Save File",
		group: "Editors",
		scope: "editor",
		searchKeywords: [
			"shortcut",
			"editor",
			"save"
		],
		defaultBindings: platformBindings(["Mod+S"])
	},
	{
		id: "editor.markdownPreview",
		title: "Show Markdown Preview",
		group: "Editors",
		scope: "editor",
		searchKeywords: [
			"shortcut",
			"editor",
			"markdown",
			"preview"
		],
		defaultBindings: platformBindings(["Mod+Shift+V"])
	},
	{
		id: "editor.toggleWordWrap",
		title: "Toggle Word Wrap",
		group: "Editors",
		scope: "editor",
		searchKeywords: [
			"shortcut",
			"editor",
			"word wrap",
			"wrap",
			"long lines",
			"soft wrap"
		],
		defaultBindings: platformBindings(["Alt+Z"])
	},
	{
		id: "editor.copyContext",
		title: "Copy Context",
		group: "Editors",
		scope: "editor",
		searchKeywords: [
			"shortcut",
			"editor",
			"copy",
			"context"
		],
		defaultBindings: platformBindings(["Mod+Alt+C"])
	},
	{
		id: "editor.previousChange",
		title: "Go to Previous Change",
		group: "Editors",
		scope: "editor",
		searchKeywords: [
			"shortcut",
			"editor",
			"diff",
			"change",
			"hunk",
			"previous"
		],
		defaultBindings: platformBindings(["Shift+F7"]),
		allowBareKeybindings: true
	},
	{
		id: "editor.nextChange",
		title: "Go to Next Change",
		group: "Editors",
		scope: "editor",
		searchKeywords: [
			"shortcut",
			"editor",
			"diff",
			"change",
			"hunk",
			"next"
		],
		defaultBindings: platformBindings(["F7"]),
		allowBareKeybindings: true
	},
	{
		id: "editor.addReviewNote",
		title: "Add Review Note",
		group: "Editors",
		scope: "editor",
		searchKeywords: [
			"shortcut",
			"editor",
			"markdown",
			"note",
			"comment",
			"annotation",
			"review"
		],
		defaultBindings: platformBindings(["Mod+Shift+A"])
	},
	{
		id: "sourceControl.sendReviewNotes",
		title: "Send Review Notes to Agent",
		group: "Global",
		scope: "global",
		conflictGroup: "editor",
		searchKeywords: [
			"shortcut",
			"source control",
			"diff",
			"notes",
			"send",
			"agent",
			"review",
			"annotate"
		],
		defaultBindings: platformBindings([])
	},
	{
		id: "fileExplorer.undo",
		title: "Undo file operation",
		group: "File Explorer",
		scope: "fileExplorer",
		searchKeywords: [
			"shortcut",
			"file explorer",
			"undo"
		],
		defaultBindings: platformBindings(["Mod+Z"])
	},
	{
		id: "fileExplorer.redo",
		title: "Redo file operation",
		group: "File Explorer",
		scope: "fileExplorer",
		searchKeywords: [
			"shortcut",
			"file explorer",
			"redo"
		],
		defaultBindings: {
			darwin: ["Mod+Shift+Z"],
			linux: ["Mod+Shift+Z", "Ctrl+Y"],
			win32: ["Mod+Shift+Z", "Ctrl+Y"]
		}
	},
	{
		id: "fileExplorer.copyPath",
		title: "Copy file path",
		group: "File Explorer",
		scope: "fileExplorer",
		searchKeywords: [
			"shortcut",
			"file explorer",
			"copy",
			"path"
		],
		defaultBindings: {
			darwin: ["Mod+Alt+C"],
			linux: ["Alt+Shift+C"],
			win32: ["Alt+Shift+C"]
		}
	},
	{
		id: "fileExplorer.copyRelativePath",
		title: "Copy relative file path",
		group: "File Explorer",
		scope: "fileExplorer",
		searchKeywords: [
			"shortcut",
			"file explorer",
			"copy",
			"relative",
			"path"
		],
		defaultBindings: platformBindings(["Mod+Alt+Shift+C"])
	},
	{
		id: "fileExplorer.delete",
		title: "Delete file",
		group: "File Explorer",
		scope: "fileExplorer",
		searchKeywords: [
			"shortcut",
			"file explorer",
			"delete",
			"remove",
			"trash"
		],
		defaultBindings: {
			darwin: ["Mod+Backspace", "Delete"],
			linux: ["Delete"],
			win32: ["Delete"]
		},
		allowBareKeybindings: true
	},
	{
		id: "settings.search",
		title: "Search Settings",
		group: "Settings",
		scope: "settings",
		searchKeywords: [
			"shortcut",
			"settings",
			"search",
			"find"
		],
		defaultBindings: platformBindings(["Mod+F"])
	},
	{
		id: "terminal.copySelection",
		title: "Copy terminal selection",
		group: "Terminal Panes",
		scope: "terminal",
		searchKeywords: [
			"shortcut",
			"terminal",
			"copy",
			"selection"
		],
		defaultBindings: {
			darwin: ["Mod+C"],
			linux: ["Ctrl+Shift+C", "Ctrl+C"],
			win32: ["Ctrl+Shift+C", "Ctrl+C"]
		}
	},
	{
		id: "terminal.selectAll",
		title: "Select all terminal text",
		group: "Terminal Panes",
		scope: "terminal",
		searchKeywords: [
			"shortcut",
			"terminal",
			"select",
			"all"
		],
		defaultBindings: {
			darwin: ["Mod+A"],
			linux: ["Ctrl+Shift+A"],
			win32: ["Ctrl+Shift+A"]
		}
	},
	{
		id: "terminal.paste",
		title: "Paste into terminal",
		group: "Terminal Panes",
		scope: "terminal",
		searchKeywords: [
			"shortcut",
			"terminal",
			"paste",
			"clipboard"
		],
		defaultBindings: {
			darwin: ["Mod+V"],
			linux: [
				"Ctrl+V",
				"Ctrl+Shift+V",
				"Shift+Insert"
			],
			win32: [
				"Ctrl+V",
				"Ctrl+Shift+V",
				"Shift+Insert"
			]
		}
	},
	{
		id: "terminal.search",
		title: "Search active pane",
		group: "Terminal Panes",
		scope: "terminal",
		searchKeywords: [
			"shortcut",
			"terminal",
			"search",
			"find"
		],
		defaultBindings: platformBindings(["Mod+F"])
	},
	{
		id: "terminal.clear",
		title: "Clear active pane",
		group: "Terminal Panes",
		scope: "terminal",
		searchKeywords: [
			"shortcut",
			"pane",
			"clear"
		],
		defaultBindings: platformBindings(["Mod+K"])
	},
	{
		id: "terminal.focusNextPane",
		title: "Focus next pane",
		group: "Terminal Panes",
		scope: "terminal",
		searchKeywords: [
			"shortcut",
			"pane",
			"focus",
			"next"
		],
		defaultBindings: platformBindings(["Mod+BracketRight"])
	},
	{
		id: "terminal.focusPreviousPane",
		title: "Focus previous pane",
		group: "Terminal Panes",
		scope: "terminal",
		searchKeywords: [
			"shortcut",
			"pane",
			"focus",
			"previous"
		],
		defaultBindings: platformBindings(["Mod+BracketLeft"])
	},
	{
		id: "terminal.equalizePaneSizes",
		title: "Equalize pane sizes",
		group: "Terminal Panes",
		scope: "terminal",
		searchKeywords: [
			"shortcut",
			"pane",
			"split",
			"equalize",
			"resize",
			"balance",
			"size"
		],
		defaultBindings: platformBindings([])
	},
	{
		id: "terminal.expandPane",
		title: "Expand / collapse pane",
		group: "Terminal Panes",
		scope: "terminal",
		searchKeywords: [
			"shortcut",
			"pane",
			"expand",
			"collapse"
		],
		defaultBindings: platformBindings(["Mod+Shift+Enter"])
	},
	{
		id: "terminal.setTitle",
		title: "Set Title…",
		group: "Terminal Panes",
		scope: "terminal",
		searchKeywords: [
			"shortcut",
			"terminal",
			"pane",
			"set title",
			"title",
			"rename"
		],
		defaultBindings: platformBindings([])
	},
	{
		id: "terminal.clearPaneTitle",
		title: "Clear Pane Title",
		group: "Terminal Panes",
		scope: "terminal",
		searchKeywords: [
			"shortcut",
			"terminal",
			"pane",
			"clear title",
			"remove title",
			"title"
		],
		defaultBindings: platformBindings([])
	},
	{
		id: "terminal.closePane",
		title: "Close active pane",
		group: "Terminal Panes",
		scope: "terminal",
		searchKeywords: [
			"shortcut",
			"pane",
			"close"
		],
		defaultBindings: platformBindings(["Mod+W"])
	},
	{
		id: "terminal.splitRight",
		title: "Split terminal right",
		group: "Terminal Panes",
		scope: "terminal",
		searchKeywords: [
			"shortcut",
			"pane",
			"split",
			"right"
		],
		defaultBindings: {
			darwin: ["Mod+D"],
			linux: ["Mod+Shift+D"],
			win32: ["Mod+Shift+D"]
		}
	},
	{
		id: "terminal.splitDown",
		title: "Split terminal down",
		group: "Terminal Panes",
		scope: "terminal",
		searchKeywords: [
			"shortcut",
			"pane",
			"split",
			"down"
		],
		defaultBindings: {
			darwin: ["Mod+Shift+D"],
			linux: ["Alt+Shift+D"],
			win32: ["Alt+Shift+D"]
		}
	},
	{
		id: "terminal.switchInputSource",
		title: "Switch input source / language (native)",
		group: "Terminal Panes",
		scope: "terminal",
		searchKeywords: [
			"shortcut",
			"input",
			"source",
			"language",
			"korean",
			"english",
			"ime",
			"switch",
			"hangul",
			"layout"
		],
		defaultBindings: {
			darwin: [],
			linux: [],
			win32: []
		},
		allowShiftOnlyKeybindings: true
	},
	...buildAgentTabKeybindingDefinitions()
];
function agentTabActionId(agent) {
	return `tab.newAgent.${agent}`;
}
function buildAgentTabKeybindingDefinitions() {
	return ALL_TUI_AGENTS.map((agent) => ({
		id: agentTabActionId(agent),
		title: `New ${TUI_AGENT_DISPLAY_NAMES[agent]} tab`,
		group: "Agents",
		scope: "tabs",
		searchKeywords: [
			"shortcut",
			"tab",
			"agent",
			"new",
			"launch",
			agent,
			TUI_AGENT_DISPLAY_NAMES[agent].toLowerCase()
		],
		defaultBindings: platformBindings([])
	}));
}
var DEFINITIONS_BY_ID = new Map(KEYBINDING_DEFINITIONS.map((definition) => [definition.id, definition]));
var DEFINITION_IDS = new Set(KEYBINDING_DEFINITIONS.map((definition) => definition.id));
var DIGIT_INDEX_ACTION_ID_SET = new Set(["tab.selectByIndex", "workspace.selectByIndex"]);
var DIGIT_INDEX_KEY_PATTERN = /^[1-9]$/;
function isDigitIndexActionId(actionId) {
	return DIGIT_INDEX_ACTION_ID_SET.has(actionId);
}
function platformBindings(bindings) {
	return {
		darwin: bindings,
		linux: bindings,
		win32: bindings
	};
}
function getKeybindingPlatform(platform) {
	return platform === "darwin" ? "darwin" : platform === "win32" ? "win32" : "linux";
}
function isKeybindingActionId(value) {
	return DEFINITION_IDS.has(value) || isPluginKeybindingActionId(value);
}
function isPluginKeybindingActionId(value) {
	return value.length <= 400 && /^plugin:[a-z0-9]+(?:-[a-z0-9]+)*\.[a-z0-9]+(?:-[a-z0-9]+)*\/[A-Za-z0-9]+(?:[._-][A-Za-z0-9]+)*$/.test(value);
}
function hasModifier(input, modifier) {
	if (modifier === "alt") return Boolean(input.alt ?? input.altKey);
	if (modifier === "meta") return Boolean(input.meta ?? input.metaKey);
	if (modifier === "control") return Boolean(input.control ?? input.ctrlKey);
	return Boolean(input.shift ?? input.shiftKey);
}
function isFunctionKeyToken(key) {
	return /^F([1-9]|1[0-9]|2[0-4])$/.test(key);
}
function normalizeKeyToken(token) {
	if (token === " ") return "Space";
	const trimmed = token.trim();
	if (!trimmed) return null;
	const upper = trimmed.toUpperCase();
	if (upper.length === 1 && upper >= "A" && upper <= "Z") return upper;
	if (upper.length === 1 && upper >= "0" && upper <= "9") return upper;
	if (isFunctionKeyToken(upper)) return upper;
	return {
		"[": "BracketLeft",
		"]": "BracketRight",
		"{": "BracketLeft",
		"}": "BracketRight",
		"-": "Minus",
		_: "Underscore",
		"=": "Equal",
		"+": "Plus",
		",": "Comma",
		".": "Period",
		"/": "Slash",
		"\\": "Backslash",
		";": "Semicolon",
		"'": "Quote",
		"`": "Backquote",
		RETURN: "Enter",
		ESC: "Escape",
		SPACEBAR: "Space",
		PGUP: "PageUp",
		PGDN: "PageDown",
		PLUS: "Plus",
		MINUS: "Minus",
		EQUAL: "Equal",
		UNDERSCORE: "Underscore",
		ARROWLEFT: "ArrowLeft",
		LEFT: "ArrowLeft",
		ARROWRIGHT: "ArrowRight",
		RIGHT: "ArrowRight",
		ARROWUP: "ArrowUp",
		UP: "ArrowUp",
		ARROWDOWN: "ArrowDown",
		DOWN: "ArrowDown",
		PAGEUP: "PageUp",
		PAGEDOWN: "PageDown",
		BACKSPACE: "Backspace",
		DELETE: "Delete",
		DEL: "Delete",
		INSERT: "Insert",
		INS: "Insert",
		ENTER: "Enter",
		TAB: "Tab",
		ESCAPE: "Escape",
		SPACE: "Space",
		BRACKETLEFT: "BracketLeft",
		BRACKETRIGHT: "BracketRight",
		NUMPADADD: "NumpadAdd",
		NUMPADSUBTRACT: "NumpadSubtract",
		ADD: "NumpadAdd",
		SUBTRACT: "NumpadSubtract",
		COMMA: "Comma",
		PERIOD: "Period",
		SLASH: "Slash",
		BACKSLASH: "Backslash",
		SEMICOLON: "Semicolon",
		QUOTE: "Quote",
		BACKQUOTE: "Backquote"
	}[upper] ?? null;
}
function parseModifierToken(rawPart) {
	const part = rawPart.toLowerCase();
	if (part === "mod" || part === "cmdorctrl" || part === "commandorcontrol") return "Mod";
	if (part === "cmd" || part === "command" || part === "meta" || rawPart === "⌘") return "Cmd";
	if (part === "ctrl" || part === "control" || rawPart === "⌃") return "Ctrl";
	if (part === "alt" || part === "option" || part === "opt" || rawPart === "⌥") return "Alt";
	if (part === "shift" || rawPart === "⇧") return "Shift";
	return null;
}
function applyModifierToken(parsed, modifier) {
	if (modifier === "Mod") parsed.mod = true;
	else if (modifier === "Cmd") parsed.meta = true;
	else if (modifier === "Ctrl") parsed.control = true;
	else if (modifier === "Alt") parsed.alt = true;
	else parsed.shift = true;
}
function emptyParsedKeybinding() {
	return {
		mod: false,
		meta: false,
		control: false,
		alt: false,
		shift: false,
		key: ""
	};
}
function parseDoubleTapKeybinding(rawParts) {
	const modifiers = [];
	let sawDoubleTap = false;
	for (const rawPart of rawParts) {
		if (rawPart.toLowerCase() === "doubletap") {
			if (sawDoubleTap) return null;
			sawDoubleTap = true;
			continue;
		}
		const modifier = parseModifierToken(rawPart);
		if (!modifier) return null;
		modifiers.push(modifier);
	}
	if (modifiers.length === 0) return null;
	const parsed = emptyParsedKeybinding();
	for (const modifier of modifiers) applyModifierToken(parsed, modifier);
	if (parsed.mod && (parsed.meta || parsed.control)) {
		parsed.doubleTapModifier = "Mod";
		return parsed;
	}
	if (modifiers.length > 1) return null;
	parsed.doubleTapModifier = modifiers[0];
	return parsed;
}
function parseKeybinding(binding) {
	const rawParts = binding.split("+").map((part) => part.trim()).filter(Boolean);
	if (rawParts.length === 0) return null;
	if (rawParts.some((part) => part.toLowerCase() === "doubletap")) return parseDoubleTapKeybinding(rawParts);
	const parsed = emptyParsedKeybinding();
	for (const rawPart of rawParts) {
		const modifier = parseModifierToken(rawPart);
		if (modifier) {
			applyModifierToken(parsed, modifier);
			continue;
		}
		if (parsed.key) return null;
		const key = normalizeKeyToken(rawPart);
		if (!key) return null;
		parsed.key = key;
	}
	return parsed.key ? parsed : null;
}
function canonicalizeParsedKeybinding(parsed) {
	if (parsed.doubleTapModifier) return `DoubleTap+${parsed.doubleTapModifier}`;
	const parts = [];
	if (parsed.mod) parts.push("Mod");
	if (parsed.meta) parts.push("Cmd");
	if (parsed.control) parts.push("Ctrl");
	if (parsed.alt) parts.push("Alt");
	if (parsed.shift) parts.push("Shift");
	parts.push(parsed.key);
	return parts.join("+");
}
function isSafeBareKey(parsed) {
	if (parsed.mod || parsed.meta || parsed.control || parsed.alt) return false;
	if (parsed.shift) return isFunctionKeyToken(parsed.key);
	return isFunctionKeyToken(parsed.key) || [
		"Backspace",
		"Delete",
		"Enter",
		"Escape",
		"Tab",
		"ArrowLeft",
		"ArrowRight",
		"ArrowUp",
		"ArrowDown",
		"PageUp",
		"PageDown"
	].includes(parsed.key);
}
function normalizeKeybindingWithOptions(binding, options = {}) {
	const parsed = parseKeybinding(binding);
	if (!parsed) return {
		ok: false,
		error: "Use a shortcut like Ctrl+Shift+P or Cmd+K."
	};
	if (parsed.mod && (parsed.meta || parsed.control)) return {
		ok: false,
		error: "Use either Mod or a platform-specific modifier, not both."
	};
	if (parsed.doubleTapModifier) return {
		ok: true,
		value: canonicalizeParsedKeybinding(parsed)
	};
	const isShiftInsert = parsed.shift && parsed.key === "Insert";
	const isBareAllowed = options.allowBareKeybindings === true && isSafeBareKey(parsed);
	const isShiftOnlyAllowed = options.allowShiftOnlyKeybindings === true && parsed.shift && !parsed.mod && !parsed.meta && !parsed.control && !parsed.alt;
	if (!parsed.mod && !parsed.meta && !parsed.control && !parsed.alt && !isShiftInsert && !isBareAllowed && !isShiftOnlyAllowed) return {
		ok: false,
		error: "Include at least one modifier key."
	};
	return {
		ok: true,
		value: canonicalizeParsedKeybinding(parsed)
	};
}
function normalizeKeybinding(binding) {
	return normalizeKeybindingWithOptions(binding);
}
function isDoubleTapBinding(binding) {
	return Boolean(parseKeybinding(binding)?.doubleTapModifier);
}
function normalizeKeybindingListWithOptions(input, options = {}) {
	const trimmed = input.trim();
	if (!trimmed) return [];
	const normalized = [];
	for (const piece of trimmed.split(",")) {
		const result = normalizeKeybindingWithOptions(piece, options);
		if (!result.ok) return result;
		if (!normalized.includes(result.value)) normalized.push(result.value);
	}
	return normalized;
}
function normalizeKeybindingArrayWithOptions(input, options = {}) {
	const normalized = [];
	for (const binding of input) {
		const piece = normalizeKeybindingListWithOptions(binding, options);
		if (!Array.isArray(piece)) return piece;
		for (const normalizedBinding of piece) if (!normalized.includes(normalizedBinding)) normalized.push(normalizedBinding);
	}
	return normalized;
}
function normalizeOptionsForAction(actionId) {
	const definition = DEFINITIONS_BY_ID.get(actionId);
	return {
		allowBareKeybindings: definition?.allowBareKeybindings === true,
		allowShiftOnlyKeybindings: definition?.allowShiftOnlyKeybindings === true
	};
}
function canonicalizeDigitIndexBinding(binding) {
	const parsed = parseKeybinding(binding);
	if (!parsed || parsed.doubleTapModifier || !DIGIT_INDEX_KEY_PATTERN.test(parsed.key)) return {
		ok: false,
		error: "Pick a number key 1–9 with a modifier, like Cmd+1 or Ctrl+1."
	};
	return {
		ok: true,
		value: canonicalizeParsedKeybinding({
			...parsed,
			key: "1"
		})
	};
}
function finalizeDigitIndexBindings(actionId, result) {
	if (!isDigitIndexActionId(actionId) || !Array.isArray(result)) return result;
	const canonical = [];
	for (const binding of result) {
		const normalized = canonicalizeDigitIndexBinding(binding);
		if (!normalized.ok) return normalized;
		if (!canonical.includes(normalized.value)) canonical.push(normalized.value);
	}
	return canonical;
}
function normalizeKeybindingListForAction(actionId, input) {
	return finalizeDigitIndexBindings(actionId, normalizeKeybindingListWithOptions(input, normalizeOptionsForAction(actionId)));
}
function normalizeKeybindingArrayForAction(actionId, input) {
	return finalizeDigitIndexBindings(actionId, normalizeKeybindingArrayWithOptions(input, normalizeOptionsForAction(actionId)));
}
var MODIFIER_KEYS = new Set([
	"Alt",
	"AltGraph",
	"Control",
	"Meta",
	"Shift",
	"OS",
	"Fn",
	"FnLock",
	"Hyper",
	"Super",
	"Symbol",
	"SymbolLock"
]);
var PUNCTUATION_KEY_TOKENS = new Set([
	"BracketLeft",
	"BracketRight",
	"Minus",
	"Underscore",
	"Equal",
	"Plus",
	"Comma",
	"Period",
	"Slash",
	"Backslash",
	"Semicolon",
	"Quote",
	"Backquote"
]);
var PHYSICAL_CODE_FALLBACK_KEYS = new Set([
	"",
	"Dead",
	"Unidentified"
]);
var SHIFTED_PUNCTUATION_KEY_TOKENS = {
	"<": "Comma",
	">": "Period",
	"?": "Slash",
	"|": "Backslash",
	":": "Semicolon",
	"\"": "Quote",
	"~": "Backquote"
};
function logicalKeyTokenFromInput(input) {
	const key = input.key ?? "";
	if (MODIFIER_KEYS.has(key)) return null;
	const normalizedKey = normalizeKeyToken(key);
	if (normalizedKey) return normalizedKey;
	if (hasModifier(input, "shift")) return SHIFTED_PUNCTUATION_KEY_TOKENS[key] ?? null;
	return null;
}
function canUsePhysicalCodeFallback(input) {
	return PHYSICAL_CODE_FALLBACK_KEYS.has(input.key ?? "");
}
function isLatinShortcutKey(key) {
	if (key.length !== 1) return false;
	const upper = key.toUpperCase();
	return upper >= "A" && upper <= "Z" || key >= "0" && key <= "9";
}
function shouldUseNonLatinShortcutPhysicalFallback(input, platform) {
	if (getKeybindingPlatform(platform) === "darwin") return false;
	if (!(hasModifier(input, "control") || hasModifier(input, "meta"))) return false;
	if (hasModifier(input, "control") && hasModifier(input, "alt")) return false;
	if (logicalKeyTokenFromInput(input) !== null) return false;
	const key = input.key ?? "";
	return key !== "" && !MODIFIER_KEYS.has(key) && !isLatinShortcutKey(key);
}
function canFallBackToPhysicalCode(input, platform) {
	return canUsePhysicalCodeFallback(input) || shouldUseNonLatinShortcutPhysicalFallback(input, platform);
}
function physicalCodeKeyTokenFromInput(input) {
	const code = input.code ?? "";
	if (code.startsWith("Key") && code.length === 4) return code.slice(3).toUpperCase();
	if (code.startsWith("Digit") && code.length === 6) return code.slice(5);
	return normalizeKeyToken(code);
}
function numpadCodeKeyTokenFromInput(input) {
	const code = input.code ?? "";
	return code === "NumpadAdd" || code === "NumpadSubtract" ? normalizeKeyToken(code) : null;
}
function shouldUseMacOptionComposedCaptureFallback(input, platform) {
	if (getKeybindingPlatform(platform) !== "darwin" || !hasModifier(input, "alt") || MODIFIER_KEYS.has(input.key ?? "")) return false;
	const physicalToken = physicalCodeKeyTokenFromInput(input);
	if (!physicalToken) return false;
	return physicalToken.length === 1 && physicalToken >= "A" && physicalToken <= "Z" || isPunctuationKeyToken(physicalToken);
}
function keyTokenFromInput(input, platform) {
	const numpadKey = numpadCodeKeyTokenFromInput(input);
	if (numpadKey) return numpadKey;
	const logicalKey = logicalKeyTokenFromInput(input);
	if (logicalKey) return logicalKey;
	if (!canUsePhysicalCodeFallback(input) && !shouldUseMacOptionComposedCaptureFallback(input, platform) && !shouldUseNonLatinShortcutPhysicalFallback(input, platform)) return null;
	return physicalCodeKeyTokenFromInput(input);
}
function canonicalDoubleTapToken(modifier, platform) {
	const isMac = platform === "darwin";
	if (modifier === "Cmd" && isMac) return "Mod";
	if (modifier === "Ctrl" && !isMac) return "Mod";
	return modifier;
}
function keybindingFromInputWithOptions(input, platform, options = {}) {
	if (input.doubleTapModifier) return normalizeKeybindingWithOptions(`DoubleTap+${canonicalDoubleTapToken(input.doubleTapModifier, platform)}`, options);
	const key = keyTokenFromInput(input, platform);
	if (!key) return {
		ok: false,
		error: "Press a key, not only a modifier."
	};
	const isMac = getKeybindingPlatform(platform) === "darwin";
	const parts = [];
	if (isMac ? hasModifier(input, "meta") : hasModifier(input, "control")) parts.push("Mod");
	if (isMac && hasModifier(input, "control")) parts.push("Ctrl");
	if (!isMac && hasModifier(input, "meta")) parts.push("Cmd");
	if (hasModifier(input, "alt")) parts.push("Alt");
	if (hasModifier(input, "shift")) parts.push("Shift");
	parts.push(key);
	return normalizeKeybindingWithOptions(parts.join("+"), options);
}
function keybindingFromInputForAction(actionId, input, platform) {
	const result = keybindingFromInputWithOptions(input, platform, normalizeOptionsForAction(actionId));
	if (!result.ok || !isDigitIndexActionId(actionId)) return result;
	return canonicalizeDigitIndexBinding(result.value);
}
function getDefaultBindings(definition, platform) {
	return definition.defaultBindings[getKeybindingPlatform(platform)].map((binding) => {
		const normalized = normalizeKeybindingWithOptions(binding, {
			allowBareKeybindings: definition.allowBareKeybindings === true,
			allowShiftOnlyKeybindings: definition.allowShiftOnlyKeybindings === true
		});
		return normalized.ok ? normalized.value : binding;
	});
}
function getEffectiveKeybindingsForAction(actionId, platform, overrides) {
	const definition = DEFINITIONS_BY_ID.get(actionId);
	const override = overrides?.[actionId];
	if (Array.isArray(override)) {
		if (isDigitIndexActionId(actionId)) {
			const canonical = [];
			for (const binding of override) {
				const normalized = canonicalizeDigitIndexBinding(binding);
				if (normalized.ok && !canonical.includes(normalized.value)) canonical.push(normalized.value);
			}
			return canonical;
		}
		return override.flatMap((binding) => {
			const normalized = normalizeKeybindingWithOptions(binding, normalizeOptionsForAction(actionId));
			return normalized.ok ? [normalized.value] : [];
		});
	}
	return definition ? getDefaultBindings(definition, platform) : [];
}
function getEffectiveKeybindingsForDefinition(definition, platform, overrides) {
	const override = overrides?.[definition.id];
	if (Array.isArray(override)) return getEffectiveKeybindingsForAction(definition.id, platform, overrides);
	return getDefaultBindings(definition, platform);
}
function getKeybindingDefinition(actionId) {
	return DEFINITIONS_BY_ID.get(actionId) ?? null;
}
function normalizeTerminalShortcutPolicy(policy) {
	return policy === "terminal-first" ? "terminal-first" : "orca-first";
}
function isKeybindingAllowedInTerminal(definition) {
	return definition.scope === "terminal" || definition.allowInTerminal === true;
}
function isKeybindingPotentialTerminalConflict(definition) {
	return definition.scope !== "terminal" && definition.allowInTerminal !== true;
}
function keybindingIsActiveInContext(definition, options = {}) {
	if (options.context !== "terminal") return true;
	if (normalizeTerminalShortcutPolicy(options.terminalShortcutPolicy) === "orca-first") return true;
	return isKeybindingAllowedInTerminal(definition);
}
function platformModifiers(parsed, platform) {
	const isMac = platform === "darwin";
	return {
		meta: parsed.meta || parsed.mod && isMac,
		control: parsed.control || parsed.mod && !isMac,
		alt: parsed.alt,
		shift: parsed.shift
	};
}
function modifierStateMatches(parsed, input, platform) {
	const expected = platformModifiers(parsed, platform);
	return hasModifier(input, "meta") === expected.meta && hasModifier(input, "control") === expected.control && hasModifier(input, "alt") === expected.alt && hasModifier(input, "shift") === expected.shift;
}
function shouldUseMacOptionLetterPhysicalFallback(parsed, input, platform) {
	return getKeybindingPlatform(platform) === "darwin" && parsed.alt && hasModifier(input, "alt") && logicalKeyTokenFromInput(input) === null;
}
function shouldUseMacOptionPunctuationPhysicalFallback(parsed, input, platform) {
	return getKeybindingPlatform(platform) === "darwin" && parsed.alt && hasModifier(input, "alt") && logicalKeyTokenFromInput(input) === null;
}
function letterKeyMatches(input, letter, parsed, platform) {
	const logicalKey = logicalKeyTokenFromInput(input);
	if (logicalKey && logicalKey.length === 1 && logicalKey >= "A" && logicalKey <= "Z") return logicalKey === letter.toUpperCase();
	return (canFallBackToPhysicalCode(input, platform) || shouldUseMacOptionLetterPhysicalFallback(parsed, input, platform)) && input.code === `Key${letter.toUpperCase()}`;
}
function digitKeyMatches(input, digit, platform) {
	const logicalKey = logicalKeyTokenFromInput(input);
	if (logicalKey && logicalKey.length === 1 && logicalKey >= "0" && logicalKey <= "9") return logicalKey === digit;
	return canFallBackToPhysicalCode(input, platform) && input.code === `Digit${digit}`;
}
function isPunctuationKeyToken(token) {
	return token !== null && PUNCTUATION_KEY_TOKENS.has(token);
}
function semanticPunctuationKey(input) {
	const logicalKey = logicalKeyTokenFromInput(input);
	return isPunctuationKeyToken(logicalKey) ? logicalKey : null;
}
function physicalPunctuationKey(input) {
	const physicalKey = physicalCodeKeyTokenFromInput(input);
	return isPunctuationKeyToken(physicalKey) ? physicalKey : null;
}
function shouldUseSemanticPunctuation(parsed, input, platform) {
	if (getKeybindingPlatform(platform) !== "darwin" && parsed.mod && parsed.alt && hasModifier(input, "control") && hasModifier(input, "alt") && !hasModifier(input, "meta") && physicalPunctuationKey(input) === null) return false;
	return true;
}
function keyMatches(parsedKey, input, parsed, platform) {
	if (parsedKey.length === 1 && parsedKey >= "A" && parsedKey <= "Z") return letterKeyMatches(input, parsedKey, parsed, platform);
	if (parsedKey.length === 1 && parsedKey >= "0" && parsedKey <= "9") return digitKeyMatches(input, parsedKey, platform);
	if (parsedKey === "NumpadAdd" || parsedKey === "NumpadSubtract") return numpadCodeKeyTokenFromInput(input) === parsedKey || logicalKeyTokenFromInput(input) === parsedKey;
	if (isPunctuationKeyToken(parsedKey)) {
		const semanticKey = semanticPunctuationKey(input);
		if (semanticKey !== null) {
			if (!shouldUseSemanticPunctuation(parsed, input, platform)) return false;
			return semanticKey === parsedKey;
		}
		return (canFallBackToPhysicalCode(input, platform) || shouldUseMacOptionPunctuationPhysicalFallback(parsed, input, platform)) && physicalPunctuationKey(input) === parsedKey;
	}
	const logicalKey = logicalKeyTokenFromInput(input);
	if (logicalKey !== null) return logicalKey === parsedKey;
	return canFallBackToPhysicalCode(input, platform) && physicalCodeKeyTokenFromInput(input) === parsedKey;
}
function resolveModifierToken(modifier, platform) {
	switch (modifier) {
		case "Mod": return platform === "darwin" ? "meta" : "control";
		case "Cmd": return "meta";
		case "Ctrl": return "control";
		case "Alt": return "alt";
		case "Shift": return "shift";
	}
}
function keybindingMatchesInput(binding, input, platform) {
	const parsed = parseKeybinding(binding);
	if (!parsed) return false;
	if (parsed.doubleTapModifier) return input.doubleTapModifier !== void 0 && resolveModifierToken(parsed.doubleTapModifier, platform) === resolveModifierToken(input.doubleTapModifier, platform);
	if (input.doubleTapModifier !== void 0) return false;
	return modifierStateMatches(parsed, input, platform) && keyMatches(parsed.key, input, parsed, platform);
}
function keybindingConflictIdentityForParsed(parsed, platform) {
	if (parsed.doubleTapModifier) return `DoubleTap:${resolveModifierToken(parsed.doubleTapModifier, platform)}`;
	const modifiers = platformModifiers(parsed, platform);
	return [
		modifiers.meta ? "Meta" : "",
		modifiers.control ? "Control" : "",
		modifiers.alt ? "Alt" : "",
		modifiers.shift ? "Shift" : "",
		parsed.key
	].join("+");
}
function getKeybindingConflictIdentity(binding, platform) {
	const parsed = parseKeybinding(binding);
	return parsed ? keybindingConflictIdentityForParsed(parsed, platform) : binding;
}
function keybindingConflictIdentities(actionId, binding, platform) {
	const exact = getKeybindingConflictIdentity(binding, platform);
	if (!isDigitIndexActionId(actionId)) return [exact];
	const parsed = parseKeybinding(binding);
	if (!parsed || parsed.doubleTapModifier || !DIGIT_INDEX_KEY_PATTERN.test(parsed.key)) return [exact];
	return Array.from({ length: 9 }, (_, index) => keybindingConflictIdentityForParsed({
		...parsed,
		key: String(index + 1)
	}, platform));
}
function keybindingMatchesAction(actionId, input, platform, overrides, options = {}) {
	const definition = DEFINITIONS_BY_ID.get(actionId);
	if (!definition) return false;
	if (!keybindingIsActiveInContext(definition, options)) return false;
	return getEffectiveKeybindingsForAction(actionId, platform, overrides).some((binding) => keybindingMatchesInput(binding, input, platform));
}
function digitFromInput(input, platform) {
	for (let value = 1; value <= 9; value++) {
		const digit = String(value);
		if (digitKeyMatches(input, digit, platform)) return digit;
	}
	return null;
}
function matchKeybindingDigitIndex(actionId, input, platform, overrides, options = {}) {
	const definition = DEFINITIONS_BY_ID.get(actionId);
	if (!definition || !keybindingIsActiveInContext(definition, options)) return null;
	const digit = digitFromInput(input, platform);
	if (!digit) return null;
	for (const binding of getEffectiveKeybindingsForAction(actionId, platform, overrides)) {
		const parsed = parseKeybinding(binding);
		if (!parsed || parsed.doubleTapModifier || !DIGIT_INDEX_KEY_PATTERN.test(parsed.key)) continue;
		if (keybindingMatchesInput(canonicalizeParsedKeybinding({
			...parsed,
			key: digit
		}), input, platform)) return Number(digit) - 1;
	}
	return null;
}
function formatModifierGlyph(modifier, isMac) {
	switch (modifier) {
		case "Mod": return isMac ? "⌘" : "Ctrl";
		case "Cmd": return isMac ? "⌘" : "Cmd";
		case "Ctrl": return isMac ? "⌃" : "Ctrl";
		case "Alt": return isMac ? "⌥" : "Alt";
		case "Shift": return isMac ? "⇧" : "Shift";
	}
}
function formatKeybinding(binding, platform) {
	const parsed = parseKeybinding(binding);
	if (!parsed) return [binding];
	const isMac = platform === "darwin";
	if (parsed.doubleTapModifier) {
		const glyph = formatModifierGlyph(parsed.doubleTapModifier, isMac);
		return [glyph, glyph];
	}
	const parts = [];
	if (parsed.mod) parts.push(isMac ? "⌘" : "Ctrl");
	if (parsed.meta) parts.push(isMac ? "⌘" : "Cmd");
	if (parsed.control) parts.push(isMac ? "⌃" : "Ctrl");
	if (parsed.alt) parts.push(isMac ? "⌥" : "Alt");
	if (parsed.shift) parts.push(isMac ? "⇧" : "Shift");
	parts.push(formatKeyToken(parsed.key));
	return parts;
}
function formatKeybindingList(bindings, platform) {
	if (bindings.length === 0) return "Unassigned";
	return bindings.map((binding) => {
		const separator = isDoubleTapBinding(binding) ? " " : platform === "darwin" ? "" : "+";
		return formatKeybinding(binding, platform).join(separator);
	}).join(", ");
}
function findKeybindingActionsForBinding(binding, platform, overrides, scopes = ["global", "tabs"]) {
	const identity$1 = getKeybindingConflictIdentity(binding, platform);
	const allowedScopes = new Set(scopes);
	return KEYBINDING_DEFINITIONS.filter((definition) => allowedScopes.has(definition.scope) && getEffectiveKeybindingsForAction(definition.id, platform, overrides).some((candidate) => keybindingConflictIdentities(definition.id, candidate, platform).includes(identity$1))).map((definition) => definition.id);
}
function formatKeyToken(token) {
	return {
		BracketLeft: "[",
		BracketRight: "]",
		Minus: "-",
		Underscore: "_",
		Equal: "=",
		Plus: "+",
		ArrowLeft: "←",
		ArrowRight: "→",
		ArrowUp: "↑",
		ArrowDown: "↓",
		PageUp: "PageUp",
		PageDown: "PageDown",
		NumpadAdd: "Numpad +",
		NumpadSubtract: "Numpad -",
		Comma: ",",
		Period: ".",
		Slash: "/",
		Backslash: "\\",
		Semicolon: ";",
		Quote: "'",
		Backquote: "`",
		Enter: "Enter",
		Backspace: "Backspace",
		Delete: "Delete",
		Insert: "Insert",
		Tab: "Tab",
		Escape: "Esc",
		Space: "Space"
	}[token] ?? token;
}
function findKeybindingConflicts(platform, overrides, options = {}) {
	return findKeybindingConflictsForDefinitions(KEYBINDING_DEFINITIONS, platform, overrides, options);
}
function findKeybindingConflictsForDefinitions(definitions, platform, overrides, options = {}) {
	const owners = /* @__PURE__ */ new Map();
	const ignoredActionIds = new Set(options.ignoredActionIds ?? []);
	const customizedActions = new Set(Object.keys(overrides ?? {}).filter((actionId) => isKeybindingActionId(actionId) && !ignoredActionIds.has(actionId)));
	for (const actionId of options.relevantActionIds ?? []) if (!ignoredActionIds.has(actionId)) customizedActions.add(actionId);
	for (const definition of definitions) {
		if (ignoredActionIds.has(definition.id)) continue;
		for (const binding of getEffectiveKeybindingsForDefinition(definition, platform, overrides)) {
			const groups = new Set([definition.conflictGroup ?? definition.scope]);
			if (definition.conflictGroup) groups.add(definition.scope);
			for (const group of groups) for (const identity$1 of keybindingConflictIdentities(definition.id, binding, platform)) {
				const conflictKey = `${group}\u0000${identity$1}`;
				const current = owners.get(conflictKey) ?? {
					binding,
					actionIds: /* @__PURE__ */ new Set()
				};
				if (!isDigitIndexActionId(definition.id) && Array.from(current.actionIds).some((actionId) => isDigitIndexActionId(actionId))) current.binding = binding;
				current.actionIds.add(definition.id);
				owners.set(conflictKey, current);
			}
		}
	}
	const seenConflictKeys = /* @__PURE__ */ new Set();
	return Array.from(owners.values()).filter(({ actionIds }) => actionIds.size > 1 && setIntersects(actionIds, customizedActions)).map(({ binding, actionIds }) => ({
		binding,
		actionIds: Array.from(actionIds)
	})).filter((conflict) => {
		const key = `${conflict.binding}\u0000${conflict.actionIds.join("\0")}`;
		if (seenConflictKeys.has(key)) return false;
		seenConflictKeys.add(key);
		return true;
	});
}
function setIntersects(left, right) {
	for (const value of left) if (right.has(value)) return true;
	return false;
}
const pluginLanguagePackContributionSchema = object({
	locale: string().min(2).max(35).regex(/^[A-Za-z]{2,3}(?:-[A-Za-z0-9]{2,8})*$/, "must be a portable locale identifier"),
	path: pluginRelativePathSchema
}).strict();
const pluginKeybindingContributionSchema = object({
	command: pluginCommandIdSchema,
	key: string().min(1).max(128).transform((value, ctx) => {
		const normalized = normalizeKeybinding(value);
		if (!normalized.ok) {
			ctx.addIssue({
				code: "custom",
				message: normalized.error
			});
			return NEVER;
		}
		return normalized.value;
	}),
	when: _enum(["global", "worktree"]).optional()
}).strict();
const pluginVmRecipeContributionSchema = object({ path: pluginRelativePathSchema }).strict();
const pluginAgentProfileContributionSchema = object({ path: pluginRelativePathSchema }).strict();
const PLUGIN_COMMAND_ALIAS_ACTION_IDS = [
	"worktree.history.back",
	"worktree.history.forward",
	"sidebar.left.toggle",
	"sidebar.sleepingWorkspaces.toggle",
	"floatingWorkspace.maximize",
	"tab.rename",
	"workspace.rename",
	"workspace.openBoard",
	"view.tasks",
	"sidebar.right.toggle",
	"sidebar.explorer.toggle",
	"sidebar.search.toggle",
	"sidebar.sourceControl.toggle",
	"sidebar.checks.toggle",
	"sidebar.ports.toggle"
];
var PLUGIN_COMMAND_ALIAS_ACTION_ID_SET = new Set(PLUGIN_COMMAND_ALIAS_ACTION_IDS);
function isPluginCommandAliasActionId(value) {
	return PLUGIN_COMMAND_ALIAS_ACTION_ID_SET.has(value);
}
function pluginCommandKeybindingActionId(pluginKey, commandId) {
	return `plugin:${pluginKey}/${commandId}`;
}
function rejectDuplicateValues(entries, valueOf, path, label, ctx) {
	const seen = /* @__PURE__ */ new Set();
	for (const [index, entry] of entries.entries()) {
		const value = valueOf(entry);
		if (seen.has(value)) ctx.addIssue({
			code: "custom",
			path: [
				"contributes",
				path,
				index
			],
			message: `duplicate ${label}: ${value}`
		});
		seen.add(value);
	}
}
function validatePluginManifestContributions(manifest, ctx) {
	for (const path of ["panels", "commands"]) rejectDuplicateValues(manifest.contributes[path], (entry) => entry.id, path, `${path} id`, ctx);
	rejectDuplicateValues(manifest.contributes.languagePacks, (entry) => entry.locale.toLowerCase(), "languagePacks", "language pack locale", ctx);
	for (const path of ["vmRecipes", "agents"]) rejectDuplicateValues(manifest.contributes[path], (entry) => entry.path, path, `${path} path`, ctx);
	const keybindingIdentities = /* @__PURE__ */ new Set();
	for (const [index, keybinding] of manifest.contributes.keybindings.entries()) {
		const identities = [
			"darwin",
			"linux",
			"win32"
		].map((platform) => getKeybindingConflictIdentity(keybinding.key, platform));
		if (identities.some((identity$1) => keybindingIdentities.has(identity$1))) ctx.addIssue({
			code: "custom",
			path: [
				"contributes",
				"keybindings",
				index
			],
			message: `duplicate keybinding: ${keybinding.key.toLowerCase()}`
		});
		identities.forEach((identity$1) => keybindingIdentities.add(identity$1));
	}
	const commands = new Map(manifest.contributes.commands.map((command) => [command.id, command]));
	for (const [index, command] of manifest.contributes.commands.entries()) if (command.action !== void 0 && !isPluginCommandAliasActionId(command.action)) ctx.addIssue({
		code: "custom",
		path: [
			"contributes",
			"commands",
			index,
			"action"
		],
		message: `unknown built-in action: ${command.action}`
	});
	for (const [index, keybinding] of manifest.contributes.keybindings.entries()) {
		const command = commands.get(keybinding.command);
		if (!command) {
			ctx.addIssue({
				code: "custom",
				path: [
					"contributes",
					"keybindings",
					index,
					"command"
				],
				message: `unknown contributed command: ${keybinding.command}`
			});
			continue;
		}
		const commandContext = command.context ?? "global";
		if (keybinding.when !== void 0 && keybinding.when !== commandContext) ctx.addIssue({
			code: "custom",
			path: [
				"contributes",
				"keybindings",
				index,
				"when"
			],
			message: "keybinding context must match its command context"
		});
	}
	if (!manifest.main && manifest.contributes.commands.some((command) => command.action === void 0)) ctx.addIssue({
		code: "custom",
		path: ["main"],
		message: "required when contributes.commands contains a worker command"
	});
	if (!manifest.main && manifest.contributes.events.length > 0) ctx.addIssue({
		code: "custom",
		path: ["main"],
		message: "required when contributes.events is non-empty"
	});
	if (manifest.contributes.events.length > 0 && !manifest.capabilities.some((capability) => capability.kind === "events:subscribe")) ctx.addIssue({
		code: "custom",
		path: ["capabilities"],
		message: "events:subscribe capability required when contributes.events is non-empty"
	});
}
var SEMVER_RE = /^(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)(?:-(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)(?:\.(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*))*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;
var orcaEngineRangeSchema = string().max(64).regex(/^>=\d+\.\d+\.\d+$/, "must be a \">=x.y.z\" version range");
var panelContributionSchema = object({
	id: pluginIdSchema,
	title: string().min(1).max(256),
	icon: string().min(1).max(64).optional(),
	entry: pluginRelativePathSchema
});
var commandContributionSchema = object({
	id: pluginCommandIdSchema,
	title: string().min(1).max(256),
	context: _enum(["global", "worktree"]).optional(),
	action: pluginCommandIdSchema.optional()
});
const PLUGIN_EVENT_NAMES = [
	"worktree.created",
	"worktree.removed",
	"agent.status.changed"
];
const PLUGIN_EVENT_SUBSCRIPTION_LIMIT = PLUGIN_EVENT_NAMES.length;
var eventContributionSchema = object({ on: _enum(PLUGIN_EVENT_NAMES) });
object({
	manifestVersion: literal(1),
	id: pluginIdSchema,
	publisher: pluginIdSchema,
	name: string().min(1).max(256),
	version: string().regex(SEMVER_RE, "must be semver"),
	description: string().max(4096).optional(),
	author: object({
		name: string().min(1).max(256),
		url: string().max(2048).optional()
	}).optional(),
	repository: string().max(2048).optional(),
	icon: pluginRelativePathSchema.optional(),
	engines: object({ orca: orcaEngineRangeSchema }),
	pluginApi: literal(1),
	main: pluginRelativePathSchema.optional(),
	contributes: object({
		panels: array(panelContributionSchema).max(64).default([]),
		commands: array(commandContributionSchema).max(256).default([]),
		events: array(eventContributionSchema).max(PLUGIN_EVENT_SUBSCRIPTION_LIMIT).default([]),
		languagePacks: array(pluginLanguagePackContributionSchema).max(16).default([]),
		keybindings: array(pluginKeybindingContributionSchema).max(256).default([]),
		vmRecipes: array(pluginVmRecipeContributionSchema).max(64).default([]),
		agents: array(pluginAgentProfileContributionSchema).max(64).default([])
	}).strict().default(() => ({
		panels: [],
		commands: [],
		events: [],
		languagePacks: [],
		keybindings: [],
		vmRecipes: [],
		agents: []
	})),
	capabilities: array(pluginCapabilitySchema).max(32).default([])
}).superRefine(validatePluginManifestContributions);
function isQualifiedPluginKey(value) {
	const parts = value.split(".");
	if (parts.length !== 2) return false;
	return isSafePluginId(parts[0]) && isSafePluginId(parts[1]);
}
function isPluginPanelTabKey(tab) {
	if (!tab.startsWith("plugin:")) return false;
	const [qualifiedKey, panelId, ...extra] = tab.slice(7).split("/");
	return extra.length === 0 && !!qualifiedKey && !!panelId && isQualifiedPluginKey(qualifiedKey) && isPluginManifestId(panelId);
}
export { ALL_TUI_AGENTS as A, number as B, keybindingIsActiveInContext as C, normalizeKeybindingArrayForAction as D, matchKeybindingDigitIndex as E, boolean as F, unknown as G, record as H, discriminatedUnion as I, uuid as K, json as L, _enum as M, _null as N, normalizeKeybindingListForAction as O, array as P, lazy as R, keybindingFromInputForAction as S, keybindingMatchesInput as T, string as U, object as V, union as W, isDigitIndexActionId as _, pluginCommandKeybindingActionId as a, isKeybindingAllowedInTerminal as b, findKeybindingActionsForBinding as c, formatKeybinding as d, formatKeybindingList as f, getKeybindingPlatform as g, getKeybindingDefinition as h, PLUGIN_COMMAND_ALIAS_ACTION_IDS as i, create as j, normalizeTerminalShortcutPolicy as k, findKeybindingConflicts as l, getEffectiveKeybindingsForDefinition as m, isPluginPanelTabKey as n, KEYBINDING_DEFINITIONS as o, getEffectiveKeybindingsForAction as p, isQualifiedPluginKey as r, agentTabActionId as s, PLUGIN_EVENT_NAMES as t, findKeybindingConflictsForDefinitions as u, isDoubleTapBinding as v, keybindingMatchesAction as w, isKeybindingPotentialTerminalConflict as x, isKeybindingActionId as y, literal as z };
