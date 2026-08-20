var _freeGlobal_default = typeof global == "object" && global && global.Object === Object && global;
var freeSelf = typeof self == "object" && self && self.Object === Object && self;
var _root_default = _freeGlobal_default || freeSelf || Function("return this")();
var _Symbol_default = _root_default.Symbol;
var objectProto$3 = Object.prototype;
var hasOwnProperty$9 = objectProto$3.hasOwnProperty;
var nativeObjectToString$1 = objectProto$3.toString;
var symToStringTag$1 = _Symbol_default ? _Symbol_default.toStringTag : void 0;
function getRawTag(value) {
	var isOwn = hasOwnProperty$9.call(value, symToStringTag$1), tag = value[symToStringTag$1];
	try {
		value[symToStringTag$1] = void 0;
		var unmasked = true;
	} catch (e) {}
	var result = nativeObjectToString$1.call(value);
	if (unmasked) if (isOwn) value[symToStringTag$1] = tag;
	else delete value[symToStringTag$1];
	return result;
}
var _getRawTag_default = getRawTag;
var nativeObjectToString = Object.prototype.toString;
function objectToString(value) {
	return nativeObjectToString.call(value);
}
var _objectToString_default = objectToString;
var nullTag = "[object Null]", undefinedTag = "[object Undefined]";
var symToStringTag = _Symbol_default ? _Symbol_default.toStringTag : void 0;
function baseGetTag(value) {
	if (value == null) return value === void 0 ? undefinedTag : nullTag;
	return symToStringTag && symToStringTag in Object(value) ? _getRawTag_default(value) : _objectToString_default(value);
}
var _baseGetTag_default = baseGetTag;
function isObjectLike(value) {
	return value != null && typeof value == "object";
}
var isObjectLike_default = isObjectLike;
var symbolTag$1 = "[object Symbol]";
function isSymbol(value) {
	return typeof value == "symbol" || isObjectLike_default(value) && _baseGetTag_default(value) == symbolTag$1;
}
var isSymbol_default = isSymbol;
function arrayMap(array, iteratee) {
	var index = -1, length = array == null ? 0 : array.length, result = Array(length);
	while (++index < length) result[index] = iteratee(array[index], index, array);
	return result;
}
var _arrayMap_default = arrayMap;
var isArray_default = Array.isArray;
var INFINITY$1 = Infinity;
var symbolProto$1 = _Symbol_default ? _Symbol_default.prototype : void 0, symbolToString = symbolProto$1 ? symbolProto$1.toString : void 0;
function baseToString(value) {
	if (typeof value == "string") return value;
	if (isArray_default(value)) return _arrayMap_default(value, baseToString) + "";
	if (isSymbol_default(value)) return symbolToString ? symbolToString.call(value) : "";
	var result = value + "";
	return result == "0" && 1 / value == -INFINITY$1 ? "-0" : result;
}
var _baseToString_default = baseToString;
function isObject(value) {
	var type = typeof value;
	return value != null && (type == "object" || type == "function");
}
var isObject_default = isObject;
function identity(value) {
	return value;
}
var identity_default = identity;
var asyncTag = "[object AsyncFunction]", funcTag$1 = "[object Function]", genTag = "[object GeneratorFunction]", proxyTag = "[object Proxy]";
function isFunction(value) {
	if (!isObject_default(value)) return false;
	var tag = _baseGetTag_default(value);
	return tag == funcTag$1 || tag == genTag || tag == asyncTag || tag == proxyTag;
}
var isFunction_default = isFunction;
var _coreJsData_default = _root_default["__core-js_shared__"];
var maskSrcKey = function() {
	var uid = /[^.]+$/.exec(_coreJsData_default && _coreJsData_default.keys && _coreJsData_default.keys.IE_PROTO || "");
	return uid ? "Symbol(src)_1." + uid : "";
}();
function isMasked(func) {
	return !!maskSrcKey && maskSrcKey in func;
}
var _isMasked_default = isMasked;
var funcToString$1 = Function.prototype.toString;
function toSource(func) {
	if (func != null) {
		try {
			return funcToString$1.call(func);
		} catch (e) {}
		try {
			return func + "";
		} catch (e) {}
	}
	return "";
}
var _toSource_default = toSource;
var reRegExpChar = /[\\^$.*+?()[\]{}|]/g;
var reIsHostCtor = /^\[object .+?Constructor\]$/;
var funcProto = Function.prototype, objectProto$2 = Object.prototype;
var funcToString = funcProto.toString;
var hasOwnProperty$8 = objectProto$2.hasOwnProperty;
var reIsNative = RegExp("^" + funcToString.call(hasOwnProperty$8).replace(reRegExpChar, "\\$&").replace(/hasOwnProperty|(function).*?(?=\\\()| for .+?(?=\\\])/g, "$1.*?") + "$");
function baseIsNative(value) {
	if (!isObject_default(value) || _isMasked_default(value)) return false;
	return (isFunction_default(value) ? reIsNative : reIsHostCtor).test(_toSource_default(value));
}
var _baseIsNative_default = baseIsNative;
function getValue(object, key) {
	return object == null ? void 0 : object[key];
}
var _getValue_default = getValue;
function getNative(object, key) {
	var value = _getValue_default(object, key);
	return _baseIsNative_default(value) ? value : void 0;
}
var _getNative_default = getNative;
var _WeakMap_default = _getNative_default(_root_default, "WeakMap");
function apply(func, thisArg, args) {
	switch (args.length) {
		case 0: return func.call(thisArg);
		case 1: return func.call(thisArg, args[0]);
		case 2: return func.call(thisArg, args[0], args[1]);
		case 3: return func.call(thisArg, args[0], args[1], args[2]);
	}
	return func.apply(thisArg, args);
}
var _apply_default = apply;
function noop() {}
var noop_default = noop;
var HOT_COUNT = 800, HOT_SPAN = 16;
var nativeNow = Date.now;
function shortOut(func) {
	var count = 0, lastCalled = 0;
	return function() {
		var stamp = nativeNow(), remaining = HOT_SPAN - (stamp - lastCalled);
		lastCalled = stamp;
		if (remaining > 0) {
			if (++count >= HOT_COUNT) return arguments[0];
		} else count = 0;
		return func.apply(void 0, arguments);
	};
}
var _shortOut_default = shortOut;
function constant(value) {
	return function() {
		return value;
	};
}
var constant_default = constant;
var _defineProperty_default = function() {
	try {
		var func = _getNative_default(Object, "defineProperty");
		func({}, "", {});
		return func;
	} catch (e) {}
}();
var _setToString_default = _shortOut_default(!_defineProperty_default ? identity_default : function(func, string) {
	return _defineProperty_default(func, "toString", {
		"configurable": true,
		"enumerable": false,
		"value": constant_default(string),
		"writable": true
	});
});
function arrayEach(array, iteratee) {
	var index = -1, length = array == null ? 0 : array.length;
	while (++index < length) if (iteratee(array[index], index, array) === false) break;
	return array;
}
var _arrayEach_default = arrayEach;
function baseFindIndex(array, predicate, fromIndex, fromRight) {
	var length = array.length, index = fromIndex + (fromRight ? 1 : -1);
	while (fromRight ? index-- : ++index < length) if (predicate(array[index], index, array)) return index;
	return -1;
}
var _baseFindIndex_default = baseFindIndex;
function baseIsNaN(value) {
	return value !== value;
}
var _baseIsNaN_default = baseIsNaN;
function strictIndexOf(array, value, fromIndex) {
	var index = fromIndex - 1, length = array.length;
	while (++index < length) if (array[index] === value) return index;
	return -1;
}
var _strictIndexOf_default = strictIndexOf;
function baseIndexOf(array, value, fromIndex) {
	return value === value ? _strictIndexOf_default(array, value, fromIndex) : _baseFindIndex_default(array, _baseIsNaN_default, fromIndex);
}
var _baseIndexOf_default = baseIndexOf;
function arrayIncludes(array, value) {
	return !!(array == null ? 0 : array.length) && _baseIndexOf_default(array, value, 0) > -1;
}
var _arrayIncludes_default = arrayIncludes;
var MAX_SAFE_INTEGER$1 = 9007199254740991;
var reIsUint = /^(?:0|[1-9]\d*)$/;
function isIndex(value, length) {
	var type = typeof value;
	length = length == null ? MAX_SAFE_INTEGER$1 : length;
	return !!length && (type == "number" || type != "symbol" && reIsUint.test(value)) && value > -1 && value % 1 == 0 && value < length;
}
var _isIndex_default = isIndex;
function eq(value, other) {
	return value === other || value !== value && other !== other;
}
var eq_default = eq;
var nativeMax = Math.max;
function overRest(func, start, transform) {
	start = nativeMax(start === void 0 ? func.length - 1 : start, 0);
	return function() {
		var args = arguments, index = -1, length = nativeMax(args.length - start, 0), array = Array(length);
		while (++index < length) array[index] = args[start + index];
		index = -1;
		var otherArgs = Array(start + 1);
		while (++index < start) otherArgs[index] = args[index];
		otherArgs[start] = transform(array);
		return _apply_default(func, this, otherArgs);
	};
}
var _overRest_default = overRest;
function baseRest(func, start) {
	return _setToString_default(_overRest_default(func, start, identity_default), func + "");
}
var _baseRest_default = baseRest;
var MAX_SAFE_INTEGER = 9007199254740991;
function isLength(value) {
	return typeof value == "number" && value > -1 && value % 1 == 0 && value <= MAX_SAFE_INTEGER;
}
var isLength_default = isLength;
function isArrayLike(value) {
	return value != null && isLength_default(value.length) && !isFunction_default(value);
}
var isArrayLike_default = isArrayLike;
var objectProto$1 = Object.prototype;
function isPrototype(value) {
	var Ctor = value && value.constructor;
	return value === (typeof Ctor == "function" && Ctor.prototype || objectProto$1);
}
var _isPrototype_default = isPrototype;
function baseTimes(n, iteratee) {
	var index = -1, result = Array(n);
	while (++index < n) result[index] = iteratee(index);
	return result;
}
var _baseTimes_default = baseTimes;
var argsTag$2 = "[object Arguments]";
function baseIsArguments(value) {
	return isObjectLike_default(value) && _baseGetTag_default(value) == argsTag$2;
}
var _baseIsArguments_default = baseIsArguments;
var objectProto = Object.prototype;
var hasOwnProperty$7 = objectProto.hasOwnProperty;
var propertyIsEnumerable$1 = objectProto.propertyIsEnumerable;
var isArguments_default = _baseIsArguments_default(function() {
	return arguments;
}()) ? _baseIsArguments_default : function(value) {
	return isObjectLike_default(value) && hasOwnProperty$7.call(value, "callee") && !propertyIsEnumerable$1.call(value, "callee");
};
function stubFalse() {
	return false;
}
var stubFalse_default = stubFalse;
var freeExports$1 = typeof exports == "object" && exports && !exports.nodeType && exports;
var freeModule$1 = freeExports$1 && typeof module == "object" && module && !module.nodeType && module;
var Buffer = freeModule$1 && freeModule$1.exports === freeExports$1 ? _root_default.Buffer : void 0;
var isBuffer_default = (Buffer ? Buffer.isBuffer : void 0) || stubFalse_default;
var argsTag$1 = "[object Arguments]", arrayTag$1 = "[object Array]", boolTag$1 = "[object Boolean]", dateTag$1 = "[object Date]", errorTag$1 = "[object Error]", funcTag = "[object Function]", mapTag$3 = "[object Map]", numberTag$1 = "[object Number]", objectTag$2 = "[object Object]", regexpTag$1 = "[object RegExp]", setTag$3 = "[object Set]", stringTag$1 = "[object String]", weakMapTag$1 = "[object WeakMap]";
var arrayBufferTag$1 = "[object ArrayBuffer]", dataViewTag$2 = "[object DataView]", float32Tag = "[object Float32Array]", float64Tag = "[object Float64Array]", int8Tag = "[object Int8Array]", int16Tag = "[object Int16Array]", int32Tag = "[object Int32Array]", uint8Tag = "[object Uint8Array]", uint8ClampedTag = "[object Uint8ClampedArray]", uint16Tag = "[object Uint16Array]", uint32Tag = "[object Uint32Array]";
var typedArrayTags = {};
typedArrayTags[float32Tag] = typedArrayTags[float64Tag] = typedArrayTags[int8Tag] = typedArrayTags[int16Tag] = typedArrayTags[int32Tag] = typedArrayTags[uint8Tag] = typedArrayTags[uint8ClampedTag] = typedArrayTags[uint16Tag] = typedArrayTags[uint32Tag] = true;
typedArrayTags[argsTag$1] = typedArrayTags[arrayTag$1] = typedArrayTags[arrayBufferTag$1] = typedArrayTags[boolTag$1] = typedArrayTags[dataViewTag$2] = typedArrayTags[dateTag$1] = typedArrayTags[errorTag$1] = typedArrayTags[funcTag] = typedArrayTags[mapTag$3] = typedArrayTags[numberTag$1] = typedArrayTags[objectTag$2] = typedArrayTags[regexpTag$1] = typedArrayTags[setTag$3] = typedArrayTags[stringTag$1] = typedArrayTags[weakMapTag$1] = false;
function baseIsTypedArray(value) {
	return isObjectLike_default(value) && isLength_default(value.length) && !!typedArrayTags[_baseGetTag_default(value)];
}
var _baseIsTypedArray_default = baseIsTypedArray;
function baseUnary(func) {
	return function(value) {
		return func(value);
	};
}
var _baseUnary_default = baseUnary;
var freeExports = typeof exports == "object" && exports && !exports.nodeType && exports;
var freeModule = freeExports && typeof module == "object" && module && !module.nodeType && module;
var freeProcess = freeModule && freeModule.exports === freeExports && _freeGlobal_default.process;
var _nodeUtil_default = function() {
	try {
		var types = freeModule && freeModule.require && freeModule.require("util").types;
		if (types) return types;
		return freeProcess && freeProcess.binding && freeProcess.binding("util");
	} catch (e) {}
}();
var nodeIsTypedArray = _nodeUtil_default && _nodeUtil_default.isTypedArray;
var isTypedArray_default = nodeIsTypedArray ? _baseUnary_default(nodeIsTypedArray) : _baseIsTypedArray_default;
var hasOwnProperty$6 = Object.prototype.hasOwnProperty;
function arrayLikeKeys(value, inherited) {
	var isArr = isArray_default(value), isArg = !isArr && isArguments_default(value), isBuff = !isArr && !isArg && isBuffer_default(value), isType = !isArr && !isArg && !isBuff && isTypedArray_default(value), skipIndexes = isArr || isArg || isBuff || isType, result = skipIndexes ? _baseTimes_default(value.length, String) : [], length = result.length;
	for (var key in value) if ((inherited || hasOwnProperty$6.call(value, key)) && !(skipIndexes && (key == "length" || isBuff && (key == "offset" || key == "parent") || isType && (key == "buffer" || key == "byteLength" || key == "byteOffset") || _isIndex_default(key, length)))) result.push(key);
	return result;
}
var _arrayLikeKeys_default = arrayLikeKeys;
function overArg(func, transform) {
	return function(arg) {
		return func(transform(arg));
	};
}
var _overArg_default = overArg;
var _nativeKeys_default = _overArg_default(Object.keys, Object);
var hasOwnProperty$5 = Object.prototype.hasOwnProperty;
function baseKeys(object) {
	if (!_isPrototype_default(object)) return _nativeKeys_default(object);
	var result = [];
	for (var key in Object(object)) if (hasOwnProperty$5.call(object, key) && key != "constructor") result.push(key);
	return result;
}
var _baseKeys_default = baseKeys;
function keys(object) {
	return isArrayLike_default(object) ? _arrayLikeKeys_default(object) : _baseKeys_default(object);
}
var keys_default = keys;
var reIsDeepProp = /\.|\[(?:[^[\]]*|(["'])(?:(?!\1)[^\\]|\\.)*?\1)\]/, reIsPlainProp = /^\w*$/;
function isKey(value, object) {
	if (isArray_default(value)) return false;
	var type = typeof value;
	if (type == "number" || type == "symbol" || type == "boolean" || value == null || isSymbol_default(value)) return true;
	return reIsPlainProp.test(value) || !reIsDeepProp.test(value) || object != null && value in Object(object);
}
var _isKey_default = isKey;
var _nativeCreate_default = _getNative_default(Object, "create");
function hashClear() {
	this.__data__ = _nativeCreate_default ? _nativeCreate_default(null) : {};
	this.size = 0;
}
var _hashClear_default = hashClear;
function hashDelete(key) {
	var result = this.has(key) && delete this.__data__[key];
	this.size -= result ? 1 : 0;
	return result;
}
var _hashDelete_default = hashDelete;
var HASH_UNDEFINED$2 = "__lodash_hash_undefined__";
var hasOwnProperty$4 = Object.prototype.hasOwnProperty;
function hashGet(key) {
	var data = this.__data__;
	if (_nativeCreate_default) {
		var result = data[key];
		return result === HASH_UNDEFINED$2 ? void 0 : result;
	}
	return hasOwnProperty$4.call(data, key) ? data[key] : void 0;
}
var _hashGet_default = hashGet;
var hasOwnProperty$3 = Object.prototype.hasOwnProperty;
function hashHas(key) {
	var data = this.__data__;
	return _nativeCreate_default ? data[key] !== void 0 : hasOwnProperty$3.call(data, key);
}
var _hashHas_default = hashHas;
var HASH_UNDEFINED$1 = "__lodash_hash_undefined__";
function hashSet(key, value) {
	var data = this.__data__;
	this.size += this.has(key) ? 0 : 1;
	data[key] = _nativeCreate_default && value === void 0 ? HASH_UNDEFINED$1 : value;
	return this;
}
var _hashSet_default = hashSet;
function Hash(entries) {
	var index = -1, length = entries == null ? 0 : entries.length;
	this.clear();
	while (++index < length) {
		var entry = entries[index];
		this.set(entry[0], entry[1]);
	}
}
Hash.prototype.clear = _hashClear_default;
Hash.prototype["delete"] = _hashDelete_default;
Hash.prototype.get = _hashGet_default;
Hash.prototype.has = _hashHas_default;
Hash.prototype.set = _hashSet_default;
var _Hash_default = Hash;
function listCacheClear() {
	this.__data__ = [];
	this.size = 0;
}
var _listCacheClear_default = listCacheClear;
function assocIndexOf(array, key) {
	var length = array.length;
	while (length--) if (eq_default(array[length][0], key)) return length;
	return -1;
}
var _assocIndexOf_default = assocIndexOf;
var splice = Array.prototype.splice;
function listCacheDelete(key) {
	var data = this.__data__, index = _assocIndexOf_default(data, key);
	if (index < 0) return false;
	if (index == data.length - 1) data.pop();
	else splice.call(data, index, 1);
	--this.size;
	return true;
}
var _listCacheDelete_default = listCacheDelete;
function listCacheGet(key) {
	var data = this.__data__, index = _assocIndexOf_default(data, key);
	return index < 0 ? void 0 : data[index][1];
}
var _listCacheGet_default = listCacheGet;
function listCacheHas(key) {
	return _assocIndexOf_default(this.__data__, key) > -1;
}
var _listCacheHas_default = listCacheHas;
function listCacheSet(key, value) {
	var data = this.__data__, index = _assocIndexOf_default(data, key);
	if (index < 0) {
		++this.size;
		data.push([key, value]);
	} else data[index][1] = value;
	return this;
}
var _listCacheSet_default = listCacheSet;
function ListCache(entries) {
	var index = -1, length = entries == null ? 0 : entries.length;
	this.clear();
	while (++index < length) {
		var entry = entries[index];
		this.set(entry[0], entry[1]);
	}
}
ListCache.prototype.clear = _listCacheClear_default;
ListCache.prototype["delete"] = _listCacheDelete_default;
ListCache.prototype.get = _listCacheGet_default;
ListCache.prototype.has = _listCacheHas_default;
ListCache.prototype.set = _listCacheSet_default;
var _ListCache_default = ListCache;
var _Map_default = _getNative_default(_root_default, "Map");
function mapCacheClear() {
	this.size = 0;
	this.__data__ = {
		"hash": new _Hash_default(),
		"map": new (_Map_default || _ListCache_default)(),
		"string": new _Hash_default()
	};
}
var _mapCacheClear_default = mapCacheClear;
function isKeyable(value) {
	var type = typeof value;
	return type == "string" || type == "number" || type == "symbol" || type == "boolean" ? value !== "__proto__" : value === null;
}
var _isKeyable_default = isKeyable;
function getMapData(map, key) {
	var data = map.__data__;
	return _isKeyable_default(key) ? data[typeof key == "string" ? "string" : "hash"] : data.map;
}
var _getMapData_default = getMapData;
function mapCacheDelete(key) {
	var result = _getMapData_default(this, key)["delete"](key);
	this.size -= result ? 1 : 0;
	return result;
}
var _mapCacheDelete_default = mapCacheDelete;
function mapCacheGet(key) {
	return _getMapData_default(this, key).get(key);
}
var _mapCacheGet_default = mapCacheGet;
function mapCacheHas(key) {
	return _getMapData_default(this, key).has(key);
}
var _mapCacheHas_default = mapCacheHas;
function mapCacheSet(key, value) {
	var data = _getMapData_default(this, key), size = data.size;
	data.set(key, value);
	this.size += data.size == size ? 0 : 1;
	return this;
}
var _mapCacheSet_default = mapCacheSet;
function MapCache(entries) {
	var index = -1, length = entries == null ? 0 : entries.length;
	this.clear();
	while (++index < length) {
		var entry = entries[index];
		this.set(entry[0], entry[1]);
	}
}
MapCache.prototype.clear = _mapCacheClear_default;
MapCache.prototype["delete"] = _mapCacheDelete_default;
MapCache.prototype.get = _mapCacheGet_default;
MapCache.prototype.has = _mapCacheHas_default;
MapCache.prototype.set = _mapCacheSet_default;
var _MapCache_default = MapCache;
var FUNC_ERROR_TEXT = "Expected a function";
function memoize(func, resolver) {
	if (typeof func != "function" || resolver != null && typeof resolver != "function") throw new TypeError(FUNC_ERROR_TEXT);
	var memoized = function() {
		var args = arguments, key = resolver ? resolver.apply(this, args) : args[0], cache = memoized.cache;
		if (cache.has(key)) return cache.get(key);
		var result = func.apply(this, args);
		memoized.cache = cache.set(key, result) || cache;
		return result;
	};
	memoized.cache = new (memoize.Cache || _MapCache_default)();
	return memoized;
}
memoize.Cache = _MapCache_default;
var memoize_default = memoize;
var MAX_MEMOIZE_SIZE = 500;
function memoizeCapped(func) {
	var result = memoize_default(func, function(key) {
		if (cache.size === MAX_MEMOIZE_SIZE) cache.clear();
		return key;
	});
	var cache = result.cache;
	return result;
}
var _memoizeCapped_default = memoizeCapped;
var rePropName = /[^.[\]]+|\[(?:(-?\d+(?:\.\d+)?)|(["'])((?:(?!\2)[^\\]|\\.)*?)\2)\]|(?=(?:\.|\[\])(?:\.|\[\]|$))/g;
var reEscapeChar = /\\(\\)?/g;
var _stringToPath_default = _memoizeCapped_default(function(string) {
	var result = [];
	if (string.charCodeAt(0) === 46) result.push("");
	string.replace(rePropName, function(match, number, quote, subString) {
		result.push(quote ? subString.replace(reEscapeChar, "$1") : number || match);
	});
	return result;
});
function toString(value) {
	return value == null ? "" : _baseToString_default(value);
}
var toString_default = toString;
function castPath(value, object) {
	if (isArray_default(value)) return value;
	return _isKey_default(value, object) ? [value] : _stringToPath_default(toString_default(value));
}
var _castPath_default = castPath;
var INFINITY = Infinity;
function toKey(value) {
	if (typeof value == "string" || isSymbol_default(value)) return value;
	var result = value + "";
	return result == "0" && 1 / value == -INFINITY ? "-0" : result;
}
var _toKey_default = toKey;
function baseGet(object, path) {
	path = _castPath_default(path, object);
	var index = 0, length = path.length;
	while (object != null && index < length) object = object[_toKey_default(path[index++])];
	return index && index == length ? object : void 0;
}
var _baseGet_default = baseGet;
function get(object, path, defaultValue) {
	var result = object == null ? void 0 : _baseGet_default(object, path);
	return result === void 0 ? defaultValue : result;
}
var get_default = get;
function arrayPush(array, values$1) {
	var index = -1, length = values$1.length, offset = array.length;
	while (++index < length) array[offset + index] = values$1[index];
	return array;
}
var _arrayPush_default = arrayPush;
var spreadableSymbol = _Symbol_default ? _Symbol_default.isConcatSpreadable : void 0;
function isFlattenable(value) {
	return isArray_default(value) || isArguments_default(value) || !!(spreadableSymbol && value && value[spreadableSymbol]);
}
var _isFlattenable_default = isFlattenable;
function baseFlatten(array, depth, predicate, isStrict, result) {
	var index = -1, length = array.length;
	predicate || (predicate = _isFlattenable_default);
	result || (result = []);
	while (++index < length) {
		var value = array[index];
		if (depth > 0 && predicate(value)) if (depth > 1) baseFlatten(value, depth - 1, predicate, isStrict, result);
		else _arrayPush_default(result, value);
		else if (!isStrict) result[result.length] = value;
	}
	return result;
}
var _baseFlatten_default = baseFlatten;
function arrayReduce(array, iteratee, accumulator, initAccum) {
	var index = -1, length = array == null ? 0 : array.length;
	if (initAccum && length) accumulator = array[++index];
	while (++index < length) accumulator = iteratee(accumulator, array[index], index, array);
	return accumulator;
}
var _arrayReduce_default = arrayReduce;
function stackClear() {
	this.__data__ = new _ListCache_default();
	this.size = 0;
}
var _stackClear_default = stackClear;
function stackDelete(key) {
	var data = this.__data__, result = data["delete"](key);
	this.size = data.size;
	return result;
}
var _stackDelete_default = stackDelete;
function stackGet(key) {
	return this.__data__.get(key);
}
var _stackGet_default = stackGet;
function stackHas(key) {
	return this.__data__.has(key);
}
var _stackHas_default = stackHas;
var LARGE_ARRAY_SIZE$1 = 200;
function stackSet(key, value) {
	var data = this.__data__;
	if (data instanceof _ListCache_default) {
		var pairs = data.__data__;
		if (!_Map_default || pairs.length < LARGE_ARRAY_SIZE$1 - 1) {
			pairs.push([key, value]);
			this.size = ++data.size;
			return this;
		}
		data = this.__data__ = new _MapCache_default(pairs);
	}
	data.set(key, value);
	this.size = data.size;
	return this;
}
var _stackSet_default = stackSet;
function Stack(entries) {
	this.size = (this.__data__ = new _ListCache_default(entries)).size;
}
Stack.prototype.clear = _stackClear_default;
Stack.prototype["delete"] = _stackDelete_default;
Stack.prototype.get = _stackGet_default;
Stack.prototype.has = _stackHas_default;
Stack.prototype.set = _stackSet_default;
var _Stack_default = Stack;
function arrayFilter(array, predicate) {
	var index = -1, length = array == null ? 0 : array.length, resIndex = 0, result = [];
	while (++index < length) {
		var value = array[index];
		if (predicate(value, index, array)) result[resIndex++] = value;
	}
	return result;
}
var _arrayFilter_default = arrayFilter;
function stubArray() {
	return [];
}
var stubArray_default = stubArray;
var propertyIsEnumerable = Object.prototype.propertyIsEnumerable;
var nativeGetSymbols = Object.getOwnPropertySymbols;
var _getSymbols_default = !nativeGetSymbols ? stubArray_default : function(object) {
	if (object == null) return [];
	object = Object(object);
	return _arrayFilter_default(nativeGetSymbols(object), function(symbol) {
		return propertyIsEnumerable.call(object, symbol);
	});
};
function baseGetAllKeys(object, keysFunc, symbolsFunc) {
	var result = keysFunc(object);
	return isArray_default(object) ? result : _arrayPush_default(result, symbolsFunc(object));
}
var _baseGetAllKeys_default = baseGetAllKeys;
function getAllKeys(object) {
	return _baseGetAllKeys_default(object, keys_default, _getSymbols_default);
}
var _getAllKeys_default = getAllKeys;
var _DataView_default = _getNative_default(_root_default, "DataView");
var _Promise_default = _getNative_default(_root_default, "Promise");
var _Set_default = _getNative_default(_root_default, "Set");
var mapTag$2 = "[object Map]", objectTag$1 = "[object Object]", promiseTag = "[object Promise]", setTag$2 = "[object Set]", weakMapTag = "[object WeakMap]";
var dataViewTag$1 = "[object DataView]";
var dataViewCtorString = _toSource_default(_DataView_default), mapCtorString = _toSource_default(_Map_default), promiseCtorString = _toSource_default(_Promise_default), setCtorString = _toSource_default(_Set_default), weakMapCtorString = _toSource_default(_WeakMap_default);
var getTag = _baseGetTag_default;
if (_DataView_default && getTag(new _DataView_default(/* @__PURE__ */ new ArrayBuffer(1))) != dataViewTag$1 || _Map_default && getTag(new _Map_default()) != mapTag$2 || _Promise_default && getTag(_Promise_default.resolve()) != promiseTag || _Set_default && getTag(new _Set_default()) != setTag$2 || _WeakMap_default && getTag(new _WeakMap_default()) != weakMapTag) getTag = function(value) {
	var result = _baseGetTag_default(value), Ctor = result == objectTag$1 ? value.constructor : void 0, ctorString = Ctor ? _toSource_default(Ctor) : "";
	if (ctorString) switch (ctorString) {
		case dataViewCtorString: return dataViewTag$1;
		case mapCtorString: return mapTag$2;
		case promiseCtorString: return promiseTag;
		case setCtorString: return setTag$2;
		case weakMapCtorString: return weakMapTag;
	}
	return result;
};
var _getTag_default = getTag;
var _Uint8Array_default = _root_default.Uint8Array;
var HASH_UNDEFINED = "__lodash_hash_undefined__";
function setCacheAdd(value) {
	this.__data__.set(value, HASH_UNDEFINED);
	return this;
}
var _setCacheAdd_default = setCacheAdd;
function setCacheHas(value) {
	return this.__data__.has(value);
}
var _setCacheHas_default = setCacheHas;
function SetCache(values$1) {
	var index = -1, length = values$1 == null ? 0 : values$1.length;
	this.__data__ = new _MapCache_default();
	while (++index < length) this.add(values$1[index]);
}
SetCache.prototype.add = SetCache.prototype.push = _setCacheAdd_default;
SetCache.prototype.has = _setCacheHas_default;
var _SetCache_default = SetCache;
function arraySome(array, predicate) {
	var index = -1, length = array == null ? 0 : array.length;
	while (++index < length) if (predicate(array[index], index, array)) return true;
	return false;
}
var _arraySome_default = arraySome;
function cacheHas(cache, key) {
	return cache.has(key);
}
var _cacheHas_default = cacheHas;
var COMPARE_PARTIAL_FLAG$5 = 1, COMPARE_UNORDERED_FLAG$3 = 2;
function equalArrays(array, other, bitmask, customizer, equalFunc, stack) {
	var isPartial = bitmask & COMPARE_PARTIAL_FLAG$5, arrLength = array.length, othLength = other.length;
	if (arrLength != othLength && !(isPartial && othLength > arrLength)) return false;
	var arrStacked = stack.get(array);
	var othStacked = stack.get(other);
	if (arrStacked && othStacked) return arrStacked == other && othStacked == array;
	var index = -1, result = true, seen = bitmask & COMPARE_UNORDERED_FLAG$3 ? new _SetCache_default() : void 0;
	stack.set(array, other);
	stack.set(other, array);
	while (++index < arrLength) {
		var arrValue = array[index], othValue = other[index];
		if (customizer) var compared = isPartial ? customizer(othValue, arrValue, index, other, array, stack) : customizer(arrValue, othValue, index, array, other, stack);
		if (compared !== void 0) {
			if (compared) continue;
			result = false;
			break;
		}
		if (seen) {
			if (!_arraySome_default(other, function(othValue$1, othIndex) {
				if (!_cacheHas_default(seen, othIndex) && (arrValue === othValue$1 || equalFunc(arrValue, othValue$1, bitmask, customizer, stack))) return seen.push(othIndex);
			})) {
				result = false;
				break;
			}
		} else if (!(arrValue === othValue || equalFunc(arrValue, othValue, bitmask, customizer, stack))) {
			result = false;
			break;
		}
	}
	stack["delete"](array);
	stack["delete"](other);
	return result;
}
var _equalArrays_default = equalArrays;
function mapToArray(map) {
	var index = -1, result = Array(map.size);
	map.forEach(function(value, key) {
		result[++index] = [key, value];
	});
	return result;
}
var _mapToArray_default = mapToArray;
function setToArray(set) {
	var index = -1, result = Array(set.size);
	set.forEach(function(value) {
		result[++index] = value;
	});
	return result;
}
var _setToArray_default = setToArray;
var COMPARE_PARTIAL_FLAG$4 = 1, COMPARE_UNORDERED_FLAG$2 = 2;
var boolTag = "[object Boolean]", dateTag = "[object Date]", errorTag = "[object Error]", mapTag$1 = "[object Map]", numberTag = "[object Number]", regexpTag = "[object RegExp]", setTag$1 = "[object Set]", stringTag = "[object String]", symbolTag = "[object Symbol]";
var arrayBufferTag = "[object ArrayBuffer]", dataViewTag = "[object DataView]";
var symbolProto = _Symbol_default ? _Symbol_default.prototype : void 0, symbolValueOf = symbolProto ? symbolProto.valueOf : void 0;
function equalByTag(object, other, tag, bitmask, customizer, equalFunc, stack) {
	switch (tag) {
		case dataViewTag:
			if (object.byteLength != other.byteLength || object.byteOffset != other.byteOffset) return false;
			object = object.buffer;
			other = other.buffer;
		case arrayBufferTag:
			if (object.byteLength != other.byteLength || !equalFunc(new _Uint8Array_default(object), new _Uint8Array_default(other))) return false;
			return true;
		case boolTag:
		case dateTag:
		case numberTag: return eq_default(+object, +other);
		case errorTag: return object.name == other.name && object.message == other.message;
		case regexpTag:
		case stringTag: return object == other + "";
		case mapTag$1: var convert = _mapToArray_default;
		case setTag$1:
			var isPartial = bitmask & COMPARE_PARTIAL_FLAG$4;
			convert || (convert = _setToArray_default);
			if (object.size != other.size && !isPartial) return false;
			var stacked = stack.get(object);
			if (stacked) return stacked == other;
			bitmask |= COMPARE_UNORDERED_FLAG$2;
			stack.set(object, other);
			var result = _equalArrays_default(convert(object), convert(other), bitmask, customizer, equalFunc, stack);
			stack["delete"](object);
			return result;
		case symbolTag: if (symbolValueOf) return symbolValueOf.call(object) == symbolValueOf.call(other);
	}
	return false;
}
var _equalByTag_default = equalByTag;
var COMPARE_PARTIAL_FLAG$3 = 1;
var hasOwnProperty$2 = Object.prototype.hasOwnProperty;
function equalObjects(object, other, bitmask, customizer, equalFunc, stack) {
	var isPartial = bitmask & COMPARE_PARTIAL_FLAG$3, objProps = _getAllKeys_default(object), objLength = objProps.length;
	if (objLength != _getAllKeys_default(other).length && !isPartial) return false;
	var index = objLength;
	while (index--) {
		var key = objProps[index];
		if (!(isPartial ? key in other : hasOwnProperty$2.call(other, key))) return false;
	}
	var objStacked = stack.get(object);
	var othStacked = stack.get(other);
	if (objStacked && othStacked) return objStacked == other && othStacked == object;
	var result = true;
	stack.set(object, other);
	stack.set(other, object);
	var skipCtor = isPartial;
	while (++index < objLength) {
		key = objProps[index];
		var objValue = object[key], othValue = other[key];
		if (customizer) var compared = isPartial ? customizer(othValue, objValue, key, other, object, stack) : customizer(objValue, othValue, key, object, other, stack);
		if (!(compared === void 0 ? objValue === othValue || equalFunc(objValue, othValue, bitmask, customizer, stack) : compared)) {
			result = false;
			break;
		}
		skipCtor || (skipCtor = key == "constructor");
	}
	if (result && !skipCtor) {
		var objCtor = object.constructor, othCtor = other.constructor;
		if (objCtor != othCtor && "constructor" in object && "constructor" in other && !(typeof objCtor == "function" && objCtor instanceof objCtor && typeof othCtor == "function" && othCtor instanceof othCtor)) result = false;
	}
	stack["delete"](object);
	stack["delete"](other);
	return result;
}
var _equalObjects_default = equalObjects;
var COMPARE_PARTIAL_FLAG$2 = 1;
var argsTag = "[object Arguments]", arrayTag = "[object Array]", objectTag = "[object Object]";
var hasOwnProperty$1 = Object.prototype.hasOwnProperty;
function baseIsEqualDeep(object, other, bitmask, customizer, equalFunc, stack) {
	var objIsArr = isArray_default(object), othIsArr = isArray_default(other), objTag = objIsArr ? arrayTag : _getTag_default(object), othTag = othIsArr ? arrayTag : _getTag_default(other);
	objTag = objTag == argsTag ? objectTag : objTag;
	othTag = othTag == argsTag ? objectTag : othTag;
	var objIsObj = objTag == objectTag, othIsObj = othTag == objectTag, isSameTag = objTag == othTag;
	if (isSameTag && isBuffer_default(object)) {
		if (!isBuffer_default(other)) return false;
		objIsArr = true;
		objIsObj = false;
	}
	if (isSameTag && !objIsObj) {
		stack || (stack = new _Stack_default());
		return objIsArr || isTypedArray_default(object) ? _equalArrays_default(object, other, bitmask, customizer, equalFunc, stack) : _equalByTag_default(object, other, objTag, bitmask, customizer, equalFunc, stack);
	}
	if (!(bitmask & COMPARE_PARTIAL_FLAG$2)) {
		var objIsWrapped = objIsObj && hasOwnProperty$1.call(object, "__wrapped__"), othIsWrapped = othIsObj && hasOwnProperty$1.call(other, "__wrapped__");
		if (objIsWrapped || othIsWrapped) {
			var objUnwrapped = objIsWrapped ? object.value() : object, othUnwrapped = othIsWrapped ? other.value() : other;
			stack || (stack = new _Stack_default());
			return equalFunc(objUnwrapped, othUnwrapped, bitmask, customizer, stack);
		}
	}
	if (!isSameTag) return false;
	stack || (stack = new _Stack_default());
	return _equalObjects_default(object, other, bitmask, customizer, equalFunc, stack);
}
var _baseIsEqualDeep_default = baseIsEqualDeep;
function baseIsEqual(value, other, bitmask, customizer, stack) {
	if (value === other) return true;
	if (value == null || other == null || !isObjectLike_default(value) && !isObjectLike_default(other)) return value !== value && other !== other;
	return _baseIsEqualDeep_default(value, other, bitmask, customizer, baseIsEqual, stack);
}
var _baseIsEqual_default = baseIsEqual;
var COMPARE_PARTIAL_FLAG$1 = 1, COMPARE_UNORDERED_FLAG$1 = 2;
function baseIsMatch(object, source, matchData, customizer) {
	var index = matchData.length, length = index, noCustomizer = !customizer;
	if (object == null) return !length;
	object = Object(object);
	while (index--) {
		var data = matchData[index];
		if (noCustomizer && data[2] ? data[1] !== object[data[0]] : !(data[0] in object)) return false;
	}
	while (++index < length) {
		data = matchData[index];
		var key = data[0], objValue = object[key], srcValue = data[1];
		if (noCustomizer && data[2]) {
			if (objValue === void 0 && !(key in object)) return false;
		} else {
			var stack = new _Stack_default();
			if (customizer) var result = customizer(objValue, srcValue, key, object, source, stack);
			if (!(result === void 0 ? _baseIsEqual_default(srcValue, objValue, COMPARE_PARTIAL_FLAG$1 | COMPARE_UNORDERED_FLAG$1, customizer, stack) : result)) return false;
		}
	}
	return true;
}
var _baseIsMatch_default = baseIsMatch;
function isStrictComparable(value) {
	return value === value && !isObject_default(value);
}
var _isStrictComparable_default = isStrictComparable;
function getMatchData(object) {
	var result = keys_default(object), length = result.length;
	while (length--) {
		var key = result[length], value = object[key];
		result[length] = [
			key,
			value,
			_isStrictComparable_default(value)
		];
	}
	return result;
}
var _getMatchData_default = getMatchData;
function matchesStrictComparable(key, srcValue) {
	return function(object) {
		if (object == null) return false;
		return object[key] === srcValue && (srcValue !== void 0 || key in Object(object));
	};
}
var _matchesStrictComparable_default = matchesStrictComparable;
function baseMatches(source) {
	var matchData = _getMatchData_default(source);
	if (matchData.length == 1 && matchData[0][2]) return _matchesStrictComparable_default(matchData[0][0], matchData[0][1]);
	return function(object) {
		return object === source || _baseIsMatch_default(object, source, matchData);
	};
}
var _baseMatches_default = baseMatches;
function baseHasIn(object, key) {
	return object != null && key in Object(object);
}
var _baseHasIn_default = baseHasIn;
function hasPath(object, path, hasFunc) {
	path = _castPath_default(path, object);
	var index = -1, length = path.length, result = false;
	while (++index < length) {
		var key = _toKey_default(path[index]);
		if (!(result = object != null && hasFunc(object, key))) break;
		object = object[key];
	}
	if (result || ++index != length) return result;
	length = object == null ? 0 : object.length;
	return !!length && isLength_default(length) && _isIndex_default(key, length) && (isArray_default(object) || isArguments_default(object));
}
var _hasPath_default = hasPath;
function hasIn(object, path) {
	return object != null && _hasPath_default(object, path, _baseHasIn_default);
}
var hasIn_default = hasIn;
var COMPARE_PARTIAL_FLAG = 1, COMPARE_UNORDERED_FLAG = 2;
function baseMatchesProperty(path, srcValue) {
	if (_isKey_default(path) && _isStrictComparable_default(srcValue)) return _matchesStrictComparable_default(_toKey_default(path), srcValue);
	return function(object) {
		var objValue = get_default(object, path);
		return objValue === void 0 && objValue === srcValue ? hasIn_default(object, path) : _baseIsEqual_default(srcValue, objValue, COMPARE_PARTIAL_FLAG | COMPARE_UNORDERED_FLAG);
	};
}
var _baseMatchesProperty_default = baseMatchesProperty;
function baseProperty(key) {
	return function(object) {
		return object == null ? void 0 : object[key];
	};
}
var _baseProperty_default = baseProperty;
function basePropertyDeep(path) {
	return function(object) {
		return _baseGet_default(object, path);
	};
}
var _basePropertyDeep_default = basePropertyDeep;
function property(path) {
	return _isKey_default(path) ? _baseProperty_default(_toKey_default(path)) : _basePropertyDeep_default(path);
}
var property_default = property;
function baseIteratee(value) {
	if (typeof value == "function") return value;
	if (value == null) return identity_default;
	if (typeof value == "object") return isArray_default(value) ? _baseMatchesProperty_default(value[0], value[1]) : _baseMatches_default(value);
	return property_default(value);
}
var _baseIteratee_default = baseIteratee;
function createBaseFor(fromRight) {
	return function(object, iteratee, keysFunc) {
		var index = -1, iterable = Object(object), props = keysFunc(object), length = props.length;
		while (length--) {
			var key = props[fromRight ? length : ++index];
			if (iteratee(iterable[key], key, iterable) === false) break;
		}
		return object;
	};
}
var _baseFor_default = createBaseFor();
function baseForOwn(object, iteratee) {
	return object && _baseFor_default(object, iteratee, keys_default);
}
var _baseForOwn_default = baseForOwn;
function createBaseEach(eachFunc, fromRight) {
	return function(collection, iteratee) {
		if (collection == null) return collection;
		if (!isArrayLike_default(collection)) return eachFunc(collection, iteratee);
		var length = collection.length, index = fromRight ? length : -1, iterable = Object(collection);
		while (fromRight ? index-- : ++index < length) if (iteratee(iterable[index], index, iterable) === false) break;
		return collection;
	};
}
var _baseEach_default = createBaseEach(_baseForOwn_default);
function isArrayLikeObject(value) {
	return isObjectLike_default(value) && isArrayLike_default(value);
}
var isArrayLikeObject_default = isArrayLikeObject;
function arrayIncludesWith(array, value, comparator) {
	var index = -1, length = array == null ? 0 : array.length;
	while (++index < length) if (comparator(value, array[index])) return true;
	return false;
}
var _arrayIncludesWith_default = arrayIncludesWith;
function castFunction(value) {
	return typeof value == "function" ? value : identity_default;
}
var _castFunction_default = castFunction;
function forEach(collection, iteratee) {
	return (isArray_default(collection) ? _arrayEach_default : _baseEach_default)(collection, _castFunction_default(iteratee));
}
var forEach_default = forEach;
function baseFilter(collection, predicate) {
	var result = [];
	_baseEach_default(collection, function(value, index, collection$1) {
		if (predicate(value, index, collection$1)) result.push(value);
	});
	return result;
}
var _baseFilter_default = baseFilter;
function filter(collection, predicate) {
	return (isArray_default(collection) ? _arrayFilter_default : _baseFilter_default)(collection, _baseIteratee_default(predicate, 3));
}
var filter_default = filter;
function baseValues(object, props) {
	return _arrayMap_default(props, function(key) {
		return object[key];
	});
}
var _baseValues_default = baseValues;
function values(object) {
	return object == null ? [] : _baseValues_default(object, keys_default(object));
}
var values_default = values;
var mapTag = "[object Map]", setTag = "[object Set]";
var hasOwnProperty = Object.prototype.hasOwnProperty;
function isEmpty(value) {
	if (value == null) return true;
	if (isArrayLike_default(value) && (isArray_default(value) || typeof value == "string" || typeof value.splice == "function" || isBuffer_default(value) || isTypedArray_default(value) || isArguments_default(value))) return !value.length;
	var tag = _getTag_default(value);
	if (tag == mapTag || tag == setTag) return !value.size;
	if (_isPrototype_default(value)) return !_baseKeys_default(value).length;
	for (var key in value) if (hasOwnProperty.call(value, key)) return false;
	return true;
}
var isEmpty_default = isEmpty;
function isUndefined(value) {
	return value === void 0;
}
var isUndefined_default = isUndefined;
function baseReduce(collection, iteratee, accumulator, initAccum, eachFunc) {
	eachFunc(collection, function(value, index, collection$1) {
		accumulator = initAccum ? (initAccum = false, value) : iteratee(accumulator, value, index, collection$1);
	});
	return accumulator;
}
var _baseReduce_default = baseReduce;
function reduce(collection, iteratee, accumulator) {
	var func = isArray_default(collection) ? _arrayReduce_default : _baseReduce_default, initAccum = arguments.length < 3;
	return func(collection, _baseIteratee_default(iteratee, 4), accumulator, initAccum, _baseEach_default);
}
var reduce_default = reduce;
var _createSet_default = !(_Set_default && 1 / _setToArray_default(new _Set_default([, -0]))[1] == Infinity) ? noop_default : function(values$1) {
	return new _Set_default(values$1);
};
var LARGE_ARRAY_SIZE = 200;
function baseUniq(array, iteratee, comparator) {
	var index = -1, includes = _arrayIncludes_default, length = array.length, isCommon = true, result = [], seen = result;
	if (comparator) {
		isCommon = false;
		includes = _arrayIncludesWith_default;
	} else if (length >= LARGE_ARRAY_SIZE) {
		var set = iteratee ? null : _createSet_default(array);
		if (set) return _setToArray_default(set);
		isCommon = false;
		includes = _cacheHas_default;
		seen = new _SetCache_default();
	} else seen = iteratee ? [] : result;
	outer: while (++index < length) {
		var value = array[index], computed = iteratee ? iteratee(value) : value;
		value = comparator || value !== 0 ? value : 0;
		if (isCommon && computed === computed) {
			var seenIndex = seen.length;
			while (seenIndex--) if (seen[seenIndex] === computed) continue outer;
			if (iteratee) seen.push(computed);
			result.push(value);
		} else if (!includes(seen, computed, comparator)) {
			if (seen !== result) seen.push(computed);
			result.push(value);
		}
	}
	return result;
}
var _baseUniq_default = baseUniq;
var union_default = _baseRest_default(function(arrays) {
	return _baseUniq_default(_baseFlatten_default(arrays, 1, isArrayLikeObject_default, true));
});
var DEFAULT_EDGE_NAME = "\0";
var GRAPH_NODE = "\0";
var EDGE_KEY_DELIM = "";
var Graph = class {
	constructor(opts = {}) {
		this._isDirected = Object.prototype.hasOwnProperty.call(opts, "directed") ? opts.directed : true;
		this._isMultigraph = Object.prototype.hasOwnProperty.call(opts, "multigraph") ? opts.multigraph : false;
		this._isCompound = Object.prototype.hasOwnProperty.call(opts, "compound") ? opts.compound : false;
		this._label = void 0;
		this._defaultNodeLabelFn = constant_default(void 0);
		this._defaultEdgeLabelFn = constant_default(void 0);
		this._nodes = {};
		if (this._isCompound) {
			this._parent = {};
			this._children = {};
			this._children[GRAPH_NODE] = {};
		}
		this._in = {};
		this._preds = {};
		this._out = {};
		this._sucs = {};
		this._edgeObjs = {};
		this._edgeLabels = {};
	}
	isDirected() {
		return this._isDirected;
	}
	isMultigraph() {
		return this._isMultigraph;
	}
	isCompound() {
		return this._isCompound;
	}
	setGraph(label) {
		this._label = label;
		return this;
	}
	graph() {
		return this._label;
	}
	setDefaultNodeLabel(newDefault) {
		if (!isFunction_default(newDefault)) newDefault = constant_default(newDefault);
		this._defaultNodeLabelFn = newDefault;
		return this;
	}
	nodeCount() {
		return this._nodeCount;
	}
	nodes() {
		return keys_default(this._nodes);
	}
	sources() {
		var self$1 = this;
		return filter_default(this.nodes(), function(v) {
			return isEmpty_default(self$1._in[v]);
		});
	}
	sinks() {
		var self$1 = this;
		return filter_default(this.nodes(), function(v) {
			return isEmpty_default(self$1._out[v]);
		});
	}
	setNodes(vs, value) {
		var args = arguments;
		var self$1 = this;
		forEach_default(vs, function(v) {
			if (args.length > 1) self$1.setNode(v, value);
			else self$1.setNode(v);
		});
		return this;
	}
	setNode(v, value) {
		if (Object.prototype.hasOwnProperty.call(this._nodes, v)) {
			if (arguments.length > 1) this._nodes[v] = value;
			return this;
		}
		this._nodes[v] = arguments.length > 1 ? value : this._defaultNodeLabelFn(v);
		if (this._isCompound) {
			this._parent[v] = GRAPH_NODE;
			this._children[v] = {};
			this._children[GRAPH_NODE][v] = true;
		}
		this._in[v] = {};
		this._preds[v] = {};
		this._out[v] = {};
		this._sucs[v] = {};
		++this._nodeCount;
		return this;
	}
	node(v) {
		return this._nodes[v];
	}
	hasNode(v) {
		return Object.prototype.hasOwnProperty.call(this._nodes, v);
	}
	removeNode(v) {
		if (Object.prototype.hasOwnProperty.call(this._nodes, v)) {
			var removeEdge = (e) => this.removeEdge(this._edgeObjs[e]);
			delete this._nodes[v];
			if (this._isCompound) {
				this._removeFromParentsChildList(v);
				delete this._parent[v];
				forEach_default(this.children(v), (child) => {
					this.setParent(child);
				});
				delete this._children[v];
			}
			forEach_default(keys_default(this._in[v]), removeEdge);
			delete this._in[v];
			delete this._preds[v];
			forEach_default(keys_default(this._out[v]), removeEdge);
			delete this._out[v];
			delete this._sucs[v];
			--this._nodeCount;
		}
		return this;
	}
	setParent(v, parent) {
		if (!this._isCompound) throw new Error("Cannot set parent in a non-compound graph");
		if (isUndefined_default(parent)) parent = GRAPH_NODE;
		else {
			parent += "";
			for (var ancestor = parent; !isUndefined_default(ancestor); ancestor = this.parent(ancestor)) if (ancestor === v) throw new Error("Setting " + parent + " as parent of " + v + " would create a cycle");
			this.setNode(parent);
		}
		this.setNode(v);
		this._removeFromParentsChildList(v);
		this._parent[v] = parent;
		this._children[parent][v] = true;
		return this;
	}
	_removeFromParentsChildList(v) {
		delete this._children[this._parent[v]][v];
	}
	parent(v) {
		if (this._isCompound) {
			var parent = this._parent[v];
			if (parent !== GRAPH_NODE) return parent;
		}
	}
	children(v) {
		if (isUndefined_default(v)) v = GRAPH_NODE;
		if (this._isCompound) {
			var children = this._children[v];
			if (children) return keys_default(children);
		} else if (v === GRAPH_NODE) return this.nodes();
		else if (this.hasNode(v)) return [];
	}
	predecessors(v) {
		var predsV = this._preds[v];
		if (predsV) return keys_default(predsV);
	}
	successors(v) {
		var sucsV = this._sucs[v];
		if (sucsV) return keys_default(sucsV);
	}
	neighbors(v) {
		var preds = this.predecessors(v);
		if (preds) return union_default(preds, this.successors(v));
	}
	isLeaf(v) {
		var neighbors;
		if (this.isDirected()) neighbors = this.successors(v);
		else neighbors = this.neighbors(v);
		return neighbors.length === 0;
	}
	filterNodes(filter$1) {
		var copy = new this.constructor({
			directed: this._isDirected,
			multigraph: this._isMultigraph,
			compound: this._isCompound
		});
		copy.setGraph(this.graph());
		var self$1 = this;
		forEach_default(this._nodes, function(value, v) {
			if (filter$1(v)) copy.setNode(v, value);
		});
		forEach_default(this._edgeObjs, function(e) {
			if (copy.hasNode(e.v) && copy.hasNode(e.w)) copy.setEdge(e, self$1.edge(e));
		});
		var parents = {};
		function findParent(v) {
			var parent = self$1.parent(v);
			if (parent === void 0 || copy.hasNode(parent)) {
				parents[v] = parent;
				return parent;
			} else if (parent in parents) return parents[parent];
			else return findParent(parent);
		}
		if (this._isCompound) forEach_default(copy.nodes(), function(v) {
			copy.setParent(v, findParent(v));
		});
		return copy;
	}
	setDefaultEdgeLabel(newDefault) {
		if (!isFunction_default(newDefault)) newDefault = constant_default(newDefault);
		this._defaultEdgeLabelFn = newDefault;
		return this;
	}
	edgeCount() {
		return this._edgeCount;
	}
	edges() {
		return values_default(this._edgeObjs);
	}
	setPath(vs, value) {
		var self$1 = this;
		var args = arguments;
		reduce_default(vs, function(v, w) {
			if (args.length > 1) self$1.setEdge(v, w, value);
			else self$1.setEdge(v, w);
			return w;
		});
		return this;
	}
	setEdge() {
		var v, w, name, value;
		var valueSpecified = false;
		var arg0 = arguments[0];
		if (typeof arg0 === "object" && arg0 !== null && "v" in arg0) {
			v = arg0.v;
			w = arg0.w;
			name = arg0.name;
			if (arguments.length === 2) {
				value = arguments[1];
				valueSpecified = true;
			}
		} else {
			v = arg0;
			w = arguments[1];
			name = arguments[3];
			if (arguments.length > 2) {
				value = arguments[2];
				valueSpecified = true;
			}
		}
		v = "" + v;
		w = "" + w;
		if (!isUndefined_default(name)) name = "" + name;
		var e = edgeArgsToId(this._isDirected, v, w, name);
		if (Object.prototype.hasOwnProperty.call(this._edgeLabels, e)) {
			if (valueSpecified) this._edgeLabels[e] = value;
			return this;
		}
		if (!isUndefined_default(name) && !this._isMultigraph) throw new Error("Cannot set a named edge when isMultigraph = false");
		this.setNode(v);
		this.setNode(w);
		this._edgeLabels[e] = valueSpecified ? value : this._defaultEdgeLabelFn(v, w, name);
		var edgeObj = edgeArgsToObj(this._isDirected, v, w, name);
		v = edgeObj.v;
		w = edgeObj.w;
		Object.freeze(edgeObj);
		this._edgeObjs[e] = edgeObj;
		incrementOrInitEntry(this._preds[w], v);
		incrementOrInitEntry(this._sucs[v], w);
		this._in[w][e] = edgeObj;
		this._out[v][e] = edgeObj;
		this._edgeCount++;
		return this;
	}
	edge(v, w, name) {
		var e = arguments.length === 1 ? edgeObjToId(this._isDirected, arguments[0]) : edgeArgsToId(this._isDirected, v, w, name);
		return this._edgeLabels[e];
	}
	hasEdge(v, w, name) {
		var e = arguments.length === 1 ? edgeObjToId(this._isDirected, arguments[0]) : edgeArgsToId(this._isDirected, v, w, name);
		return Object.prototype.hasOwnProperty.call(this._edgeLabels, e);
	}
	removeEdge(v, w, name) {
		var e = arguments.length === 1 ? edgeObjToId(this._isDirected, arguments[0]) : edgeArgsToId(this._isDirected, v, w, name);
		var edge = this._edgeObjs[e];
		if (edge) {
			v = edge.v;
			w = edge.w;
			delete this._edgeLabels[e];
			delete this._edgeObjs[e];
			decrementOrRemoveEntry(this._preds[w], v);
			decrementOrRemoveEntry(this._sucs[v], w);
			delete this._in[w][e];
			delete this._out[v][e];
			this._edgeCount--;
		}
		return this;
	}
	inEdges(v, u) {
		var inV = this._in[v];
		if (inV) {
			var edges = values_default(inV);
			if (!u) return edges;
			return filter_default(edges, function(edge) {
				return edge.v === u;
			});
		}
	}
	outEdges(v, w) {
		var outV = this._out[v];
		if (outV) {
			var edges = values_default(outV);
			if (!w) return edges;
			return filter_default(edges, function(edge) {
				return edge.w === w;
			});
		}
	}
	nodeEdges(v, w) {
		var inEdges = this.inEdges(v, w);
		if (inEdges) return inEdges.concat(this.outEdges(v, w));
	}
};
Graph.prototype._nodeCount = 0;
Graph.prototype._edgeCount = 0;
function incrementOrInitEntry(map, k) {
	if (map[k]) map[k]++;
	else map[k] = 1;
}
function decrementOrRemoveEntry(map, k) {
	if (!--map[k]) delete map[k];
}
function edgeArgsToId(isDirected, v_, w_, name) {
	var v = "" + v_;
	var w = "" + w_;
	if (!isDirected && v > w) {
		var tmp = v;
		v = w;
		w = tmp;
	}
	return v + EDGE_KEY_DELIM + w + EDGE_KEY_DELIM + (isUndefined_default(name) ? DEFAULT_EDGE_NAME : name);
}
function edgeArgsToObj(isDirected, v_, w_, name) {
	var v = "" + v_;
	var w = "" + w_;
	if (!isDirected && v > w) {
		var tmp = v;
		v = w;
		w = tmp;
	}
	var edgeObj = {
		v,
		w
	};
	if (name) edgeObj.name = name;
	return edgeObj;
}
function edgeObjToId(isDirected, edgeObj) {
	return edgeArgsToId(isDirected, edgeObj.v, edgeObj.w, edgeObj.name);
}
export { _arrayMap_default as $, _baseKeys_default as A, _baseRest_default as B, _baseFlatten_default as C, _castPath_default as D, _toKey_default as E, _baseUnary_default as F, _arrayEach_default as G, eq_default as H, isBuffer_default as I, constant_default as J, _setToString_default as K, isArguments_default as L, _arrayLikeKeys_default as M, isTypedArray_default as N, toString_default as O, _nodeUtil_default as P, isArray_default as Q, _isPrototype_default as R, _Stack_default as S, _baseGet_default as T, _isIndex_default as U, _overRest_default as V, _baseFindIndex_default as W, identity_default as X, isFunction_default as Y, isObject_default as Z, _getTag_default as _, filter_default as a, _getSymbols_default as b, isArrayLikeObject_default as c, _baseFor_default as d, isSymbol_default as et, _baseIteratee_default as f, _Uint8Array_default as g, _hasPath_default as h, values_default as i, _root_default as it, _overArg_default as j, keys_default as k, _baseEach_default as l, hasIn_default as m, reduce_default as n, _baseGetTag_default as nt, forEach_default as o, _baseProperty_default as p, _defineProperty_default as q, isUndefined_default as r, _Symbol_default as rt, _castFunction_default as s, Graph as t, isObjectLike_default as tt, _baseForOwn_default as u, _getAllKeys_default as v, _arrayPush_default as w, stubArray_default as x, _baseGetAllKeys_default as y, isArrayLike_default as z };
