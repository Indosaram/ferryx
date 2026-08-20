import { $ as _arrayMap_default, F as _baseUnary_default, G as _arrayEach_default, H as eq_default, I as isBuffer_default, M as _arrayLikeKeys_default, P as _nodeUtil_default, Q as isArray_default, R as _isPrototype_default, S as _Stack_default, Z as isObject_default, _ as _getTag_default, b as _getSymbols_default, f as _baseIteratee_default, g as _Uint8Array_default, it as _root_default, j as _overArg_default, k as keys_default, l as _baseEach_default, q as _defineProperty_default, rt as _Symbol_default, tt as isObjectLike_default, v as _getAllKeys_default, w as _arrayPush_default, x as stubArray_default, y as _baseGetAllKeys_default, z as isArrayLike_default } from "./graphlib-DH3qXvB3.js";
var objectCreate = Object.create;
var _baseCreate_default = function() {
	function object() {}
	return function(proto) {
		if (!isObject_default(proto)) return {};
		if (objectCreate) return objectCreate(proto);
		object.prototype = proto;
		var result = new object();
		object.prototype = void 0;
		return result;
	};
}();
function copyArray(source, array) {
	var index = -1, length = source.length;
	array || (array = Array(length));
	while (++index < length) array[index] = source[index];
	return array;
}
var _copyArray_default = copyArray;
function baseAssignValue(object, key, value) {
	if (key == "__proto__" && _defineProperty_default) _defineProperty_default(object, key, {
		"configurable": true,
		"enumerable": true,
		"value": value,
		"writable": true
	});
	else object[key] = value;
}
var _baseAssignValue_default = baseAssignValue;
var hasOwnProperty$2 = Object.prototype.hasOwnProperty;
function assignValue(object, key, value) {
	var objValue = object[key];
	if (!(hasOwnProperty$2.call(object, key) && eq_default(objValue, value)) || value === void 0 && !(key in object)) _baseAssignValue_default(object, key, value);
}
var _assignValue_default = assignValue;
function copyObject(source, props, object, customizer) {
	var isNew = !object;
	object || (object = {});
	var index = -1, length = props.length;
	while (++index < length) {
		var key = props[index];
		var newValue = customizer ? customizer(object[key], source[key], key, object, source) : void 0;
		if (newValue === void 0) newValue = source[key];
		if (isNew) _baseAssignValue_default(object, key, newValue);
		else _assignValue_default(object, key, newValue);
	}
	return object;
}
var _copyObject_default = copyObject;
function nativeKeysIn(object) {
	var result = [];
	if (object != null) for (var key in Object(object)) result.push(key);
	return result;
}
var _nativeKeysIn_default = nativeKeysIn;
var hasOwnProperty$1 = Object.prototype.hasOwnProperty;
function baseKeysIn(object) {
	if (!isObject_default(object)) return _nativeKeysIn_default(object);
	var isProto = _isPrototype_default(object), result = [];
	for (var key in object) if (!(key == "constructor" && (isProto || !hasOwnProperty$1.call(object, key)))) result.push(key);
	return result;
}
var _baseKeysIn_default = baseKeysIn;
function keysIn(object) {
	return isArrayLike_default(object) ? _arrayLikeKeys_default(object, true) : _baseKeysIn_default(object);
}
var keysIn_default = keysIn;
var _getPrototype_default = _overArg_default(Object.getPrototypeOf, Object);
function baseAssign(object, source) {
	return object && _copyObject_default(source, keys_default(source), object);
}
var _baseAssign_default = baseAssign;
function baseAssignIn(object, source) {
	return object && _copyObject_default(source, keysIn_default(source), object);
}
var _baseAssignIn_default = baseAssignIn;
var freeExports = typeof exports == "object" && exports && !exports.nodeType && exports;
var freeModule = freeExports && typeof module == "object" && module && !module.nodeType && module;
var Buffer = freeModule && freeModule.exports === freeExports ? _root_default.Buffer : void 0, allocUnsafe = Buffer ? Buffer.allocUnsafe : void 0;
function cloneBuffer(buffer, isDeep) {
	if (isDeep) return buffer.slice();
	var length = buffer.length, result = allocUnsafe ? allocUnsafe(length) : new buffer.constructor(length);
	buffer.copy(result);
	return result;
}
var _cloneBuffer_default = cloneBuffer;
function copySymbols(source, object) {
	return _copyObject_default(source, _getSymbols_default(source), object);
}
var _copySymbols_default = copySymbols;
var _getSymbolsIn_default = !Object.getOwnPropertySymbols ? stubArray_default : function(object) {
	var result = [];
	while (object) {
		_arrayPush_default(result, _getSymbols_default(object));
		object = _getPrototype_default(object);
	}
	return result;
};
function copySymbolsIn(source, object) {
	return _copyObject_default(source, _getSymbolsIn_default(source), object);
}
var _copySymbolsIn_default = copySymbolsIn;
function getAllKeysIn(object) {
	return _baseGetAllKeys_default(object, keysIn_default, _getSymbolsIn_default);
}
var _getAllKeysIn_default = getAllKeysIn;
var hasOwnProperty = Object.prototype.hasOwnProperty;
function initCloneArray(array) {
	var length = array.length, result = new array.constructor(length);
	if (length && typeof array[0] == "string" && hasOwnProperty.call(array, "index")) {
		result.index = array.index;
		result.input = array.input;
	}
	return result;
}
var _initCloneArray_default = initCloneArray;
function cloneArrayBuffer(arrayBuffer) {
	var result = new arrayBuffer.constructor(arrayBuffer.byteLength);
	new _Uint8Array_default(result).set(new _Uint8Array_default(arrayBuffer));
	return result;
}
var _cloneArrayBuffer_default = cloneArrayBuffer;
function cloneDataView(dataView, isDeep) {
	var buffer = isDeep ? _cloneArrayBuffer_default(dataView.buffer) : dataView.buffer;
	return new dataView.constructor(buffer, dataView.byteOffset, dataView.byteLength);
}
var _cloneDataView_default = cloneDataView;
var reFlags = /\w*$/;
function cloneRegExp(regexp) {
	var result = new regexp.constructor(regexp.source, reFlags.exec(regexp));
	result.lastIndex = regexp.lastIndex;
	return result;
}
var _cloneRegExp_default = cloneRegExp;
var symbolProto = _Symbol_default ? _Symbol_default.prototype : void 0, symbolValueOf = symbolProto ? symbolProto.valueOf : void 0;
function cloneSymbol(symbol) {
	return symbolValueOf ? Object(symbolValueOf.call(symbol)) : {};
}
var _cloneSymbol_default = cloneSymbol;
function cloneTypedArray(typedArray, isDeep) {
	var buffer = isDeep ? _cloneArrayBuffer_default(typedArray.buffer) : typedArray.buffer;
	return new typedArray.constructor(buffer, typedArray.byteOffset, typedArray.length);
}
var _cloneTypedArray_default = cloneTypedArray;
var boolTag$1 = "[object Boolean]", dateTag$1 = "[object Date]", mapTag$2 = "[object Map]", numberTag$1 = "[object Number]", regexpTag$1 = "[object RegExp]", setTag$2 = "[object Set]", stringTag$1 = "[object String]", symbolTag$1 = "[object Symbol]";
var arrayBufferTag$1 = "[object ArrayBuffer]", dataViewTag$1 = "[object DataView]", float32Tag$1 = "[object Float32Array]", float64Tag$1 = "[object Float64Array]", int8Tag$1 = "[object Int8Array]", int16Tag$1 = "[object Int16Array]", int32Tag$1 = "[object Int32Array]", uint8Tag$1 = "[object Uint8Array]", uint8ClampedTag$1 = "[object Uint8ClampedArray]", uint16Tag$1 = "[object Uint16Array]", uint32Tag$1 = "[object Uint32Array]";
function initCloneByTag(object, tag, isDeep) {
	var Ctor = object.constructor;
	switch (tag) {
		case arrayBufferTag$1: return _cloneArrayBuffer_default(object);
		case boolTag$1:
		case dateTag$1: return new Ctor(+object);
		case dataViewTag$1: return _cloneDataView_default(object, isDeep);
		case float32Tag$1:
		case float64Tag$1:
		case int8Tag$1:
		case int16Tag$1:
		case int32Tag$1:
		case uint8Tag$1:
		case uint8ClampedTag$1:
		case uint16Tag$1:
		case uint32Tag$1: return _cloneTypedArray_default(object, isDeep);
		case mapTag$2: return new Ctor();
		case numberTag$1:
		case stringTag$1: return new Ctor(object);
		case regexpTag$1: return _cloneRegExp_default(object);
		case setTag$2: return new Ctor();
		case symbolTag$1: return _cloneSymbol_default(object);
	}
}
var _initCloneByTag_default = initCloneByTag;
function initCloneObject(object) {
	return typeof object.constructor == "function" && !_isPrototype_default(object) ? _baseCreate_default(_getPrototype_default(object)) : {};
}
var _initCloneObject_default = initCloneObject;
var mapTag$1 = "[object Map]";
function baseIsMap(value) {
	return isObjectLike_default(value) && _getTag_default(value) == mapTag$1;
}
var _baseIsMap_default = baseIsMap;
var nodeIsMap = _nodeUtil_default && _nodeUtil_default.isMap;
var isMap_default = nodeIsMap ? _baseUnary_default(nodeIsMap) : _baseIsMap_default;
var setTag$1 = "[object Set]";
function baseIsSet(value) {
	return isObjectLike_default(value) && _getTag_default(value) == setTag$1;
}
var _baseIsSet_default = baseIsSet;
var nodeIsSet = _nodeUtil_default && _nodeUtil_default.isSet;
var isSet_default = nodeIsSet ? _baseUnary_default(nodeIsSet) : _baseIsSet_default;
var CLONE_DEEP_FLAG = 1, CLONE_FLAT_FLAG = 2, CLONE_SYMBOLS_FLAG = 4;
var argsTag = "[object Arguments]", arrayTag = "[object Array]", boolTag = "[object Boolean]", dateTag = "[object Date]", errorTag = "[object Error]", funcTag = "[object Function]", genTag = "[object GeneratorFunction]", mapTag = "[object Map]", numberTag = "[object Number]", objectTag = "[object Object]", regexpTag = "[object RegExp]", setTag = "[object Set]", stringTag = "[object String]", symbolTag = "[object Symbol]", weakMapTag = "[object WeakMap]";
var arrayBufferTag = "[object ArrayBuffer]", dataViewTag = "[object DataView]", float32Tag = "[object Float32Array]", float64Tag = "[object Float64Array]", int8Tag = "[object Int8Array]", int16Tag = "[object Int16Array]", int32Tag = "[object Int32Array]", uint8Tag = "[object Uint8Array]", uint8ClampedTag = "[object Uint8ClampedArray]", uint16Tag = "[object Uint16Array]", uint32Tag = "[object Uint32Array]";
var cloneableTags = {};
cloneableTags[argsTag] = cloneableTags[arrayTag] = cloneableTags[arrayBufferTag] = cloneableTags[dataViewTag] = cloneableTags[boolTag] = cloneableTags[dateTag] = cloneableTags[float32Tag] = cloneableTags[float64Tag] = cloneableTags[int8Tag] = cloneableTags[int16Tag] = cloneableTags[int32Tag] = cloneableTags[mapTag] = cloneableTags[numberTag] = cloneableTags[objectTag] = cloneableTags[regexpTag] = cloneableTags[setTag] = cloneableTags[stringTag] = cloneableTags[symbolTag] = cloneableTags[uint8Tag] = cloneableTags[uint8ClampedTag] = cloneableTags[uint16Tag] = cloneableTags[uint32Tag] = true;
cloneableTags[errorTag] = cloneableTags[funcTag] = cloneableTags[weakMapTag] = false;
function baseClone(value, bitmask, customizer, key, object, stack) {
	var result, isDeep = bitmask & CLONE_DEEP_FLAG, isFlat = bitmask & CLONE_FLAT_FLAG, isFull = bitmask & CLONE_SYMBOLS_FLAG;
	if (customizer) result = object ? customizer(value, key, object, stack) : customizer(value);
	if (result !== void 0) return result;
	if (!isObject_default(value)) return value;
	var isArr = isArray_default(value);
	if (isArr) {
		result = _initCloneArray_default(value);
		if (!isDeep) return _copyArray_default(value, result);
	} else {
		var tag = _getTag_default(value), isFunc = tag == funcTag || tag == genTag;
		if (isBuffer_default(value)) return _cloneBuffer_default(value, isDeep);
		if (tag == objectTag || tag == argsTag || isFunc && !object) {
			result = isFlat || isFunc ? {} : _initCloneObject_default(value);
			if (!isDeep) return isFlat ? _copySymbolsIn_default(value, _baseAssignIn_default(result, value)) : _copySymbols_default(value, _baseAssign_default(result, value));
		} else {
			if (!cloneableTags[tag]) return object ? value : {};
			result = _initCloneByTag_default(value, tag, isDeep);
		}
	}
	stack || (stack = new _Stack_default());
	var stacked = stack.get(value);
	if (stacked) return stacked;
	stack.set(value, result);
	if (isSet_default(value)) value.forEach(function(subValue) {
		result.add(baseClone(subValue, bitmask, customizer, subValue, value, stack));
	});
	else if (isMap_default(value)) value.forEach(function(subValue, key$1) {
		result.set(key$1, baseClone(subValue, bitmask, customizer, key$1, value, stack));
	});
	var props = isArr ? void 0 : (isFull ? isFlat ? _getAllKeysIn_default : _getAllKeys_default : isFlat ? keysIn_default : keys_default)(value);
	_arrayEach_default(props || value, function(subValue, key$1) {
		if (props) {
			key$1 = subValue;
			subValue = value[key$1];
		}
		_assignValue_default(result, key$1, baseClone(subValue, bitmask, customizer, key$1, value, stack));
	});
	return result;
}
var _baseClone_default = baseClone;
function baseMap(collection, iteratee) {
	var index = -1, result = isArrayLike_default(collection) ? Array(collection.length) : [];
	_baseEach_default(collection, function(value, key, collection$1) {
		result[++index] = iteratee(value, key, collection$1);
	});
	return result;
}
var _baseMap_default = baseMap;
function map(collection, iteratee) {
	return (isArray_default(collection) ? _arrayMap_default : _baseMap_default)(collection, _baseIteratee_default(iteratee, 3));
}
var map_default = map;
export { _cloneTypedArray_default as a, keysIn_default as c, _baseAssignValue_default as d, _copyArray_default as f, _initCloneObject_default as i, _copyObject_default as l, _baseMap_default as n, _cloneBuffer_default as o, _baseClone_default as r, _getPrototype_default as s, map_default as t, _assignValue_default as u };
