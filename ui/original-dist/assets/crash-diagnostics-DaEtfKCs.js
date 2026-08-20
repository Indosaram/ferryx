import { ml as getBrowserWebviewMemoryProfile, pc as recordRendererCrashBreadcrumb, ps as collectRendererMemoryProfileCounts } from "./store-CgXrfmaH.js";
var RENDERER_MEMORY_SAMPLE_INTERVAL_MS = 6e4;
var BYTES_PER_MEGABYTE = 1024 * 1024;
var BYTES_PER_KILOBYTE = 1024;
var RENDERER_MEMORY_HIGHWATER_RATIOS = [.6, .8];
var rendererCrashDiagnosticsInstalled = false;
var rendererSurface = "main";
var emittedHighwaterRatios = /* @__PURE__ */ new Set();
function installRendererCrashDiagnostics(surface = "main") {
	if (rendererCrashDiagnosticsInstalled || typeof window === "undefined") return;
	rendererCrashDiagnosticsInstalled = true;
	rendererSurface = surface;
	window.addEventListener("error", recordRendererError);
	window.addEventListener("unhandledrejection", recordRendererUnhandledRejection);
	if (readHeapMetrics()) {
		recordRendererMemory("startup");
		window.setInterval(() => recordRendererMemory("interval"), RENDERER_MEMORY_SAMPLE_INTERVAL_MS);
	}
}
function recordRendererError(event) {
	if (/^ResizeObserver loop (?:limit exceeded|completed with undelivered notifications)\.?$/i.test(event.message)) {
		event.preventDefault();
		return;
	}
	recordRendererCrashBreadcrumb("renderer_error", compactBreadcrumbData({
		message: event.message,
		filename: event.filename,
		lineno: event.lineno,
		colno: event.colno,
		...describeUnknownValue("error", event.error)
	}));
}
function recordRendererUnhandledRejection(event) {
	recordRendererCrashBreadcrumb("renderer_unhandled_rejection", compactBreadcrumbData(describeUnknownValue("reason", event.reason)));
}
function recordRendererMemory(reason) {
	const memory = readHeapMetrics();
	if (!memory) return;
	const browserWebviews = getBrowserWebviewMemoryProfile();
	recordRendererCrashBreadcrumb("renderer_memory", compactBreadcrumbData({
		reason,
		usedHeapMB: toMegabytes(memory.usedJSHeapSize),
		totalHeapMB: toMegabytes(memory.totalJSHeapSize),
		heapLimitMB: toMegabytes(memory.jsHeapSizeLimit),
		heapSource: memory.exact ? "v8" : "quantized",
		mallocedMB: toMegabytes(memory.mallocedBytes),
		blinkAllocatedMB: toMegabytes(memory.blinkAllocatedBytes),
		browserWebviews: browserWebviews.browserWebviewCount,
		registeredBrowserGuests: browserWebviews.registeredBrowserGuestCount
	}));
	recordRendererMemoryHighwater(memory, browserWebviews);
}
function recordRendererMemoryHighwater(memory, browserWebviews) {
	const used = memory.usedJSHeapSize;
	const limit = memory.jsHeapSizeLimit;
	if (!isFiniteHeapBytes(used) || !isFiniteHeapBytes(limit) || limit <= 0) return;
	const ratio = used / limit;
	let crossedThreshold = false;
	for (const threshold of RENDERER_MEMORY_HIGHWATER_RATIOS) if (ratio >= threshold && !emittedHighwaterRatios.has(threshold)) {
		crossedThreshold = true;
		break;
	}
	if (!crossedThreshold) return;
	const profile = compactBreadcrumbData({
		rendererSurface,
		usedHeapMB: toMegabytes(used),
		totalHeapMB: toMegabytes(memory.totalJSHeapSize),
		heapLimitMB: toMegabytes(limit),
		heapSource: memory.exact ? "v8" : "quantized",
		mallocedMB: toMegabytes(memory.mallocedBytes),
		blinkAllocatedMB: toMegabytes(memory.blinkAllocatedBytes),
		domNodes: document.getElementsByTagName("*").length,
		terminalElements: document.querySelectorAll(".xterm").length,
		browserWebviews: browserWebviews.browserWebviewCount,
		registeredBrowserGuests: browserWebviews.registeredBrowserGuestCount,
		...collectRendererMemoryProfileCounts()
	});
	for (const threshold of RENDERER_MEMORY_HIGHWATER_RATIOS) {
		if (ratio < threshold || emittedHighwaterRatios.has(threshold)) continue;
		emittedHighwaterRatios.add(threshold);
		recordRendererCrashBreadcrumb("renderer_memory_highwater", {
			...profile,
			thresholdPct: Math.round(threshold * 100)
		});
	}
}
function isFiniteHeapBytes(value) {
	return typeof value === "number" && Number.isFinite(value);
}
function getPerformanceMemory() {
	if (typeof window === "undefined") return;
	return window.performance.memory;
}
function readHeapMetrics() {
	if (typeof window === "undefined") return;
	const exact = window.api?.crashReports?.readHeapStatistics?.();
	if (exact) return {
		usedJSHeapSize: exact.usedHeapKB * BYTES_PER_KILOBYTE,
		totalJSHeapSize: exact.totalHeapKB * BYTES_PER_KILOBYTE,
		jsHeapSizeLimit: exact.heapLimitKB * BYTES_PER_KILOBYTE,
		mallocedBytes: exact.mallocedKB * BYTES_PER_KILOBYTE,
		blinkAllocatedBytes: exact.blinkAllocatedKB === void 0 ? void 0 : exact.blinkAllocatedKB * BYTES_PER_KILOBYTE,
		exact: true
	};
	const fallback = getPerformanceMemory();
	return fallback ? {
		...fallback,
		exact: false
	} : void 0;
}
function describeUnknownValue(prefix, value) {
	if (value === null) return { [`${prefix}Type`]: "null" };
	if (value === void 0) return { [`${prefix}Type`]: "undefined" };
	if (typeof value === "object" || typeof value === "function") {
		const candidate = value;
		return {
			[`${prefix}Type`]: typeof value === "function" ? "function" : candidate.constructor?.name,
			[`${prefix}Name`]: typeof candidate.name === "string" ? candidate.name : void 0,
			[`${prefix}Message`]: typeof candidate.message === "string" ? candidate.message : void 0,
			[`${prefix}Stack`]: typeof candidate.stack === "string" ? candidate.stack : void 0
		};
	}
	return {
		[`${prefix}Type`]: typeof value,
		[`${prefix}Message`]: stringifyUnknown(value)
	};
}
function stringifyUnknown(value) {
	try {
		return String(value);
	} catch {
		return "[unstringifiable]";
	}
}
function compactBreadcrumbData(data) {
	const compacted = {};
	for (const [key, value] of Object.entries(data)) if (typeof value === "string" || typeof value === "boolean" || value === null) compacted[key] = value;
	else if (typeof value === "number" && Number.isFinite(value)) compacted[key] = value;
	return compacted;
}
function toMegabytes(value) {
	return typeof value === "number" && Number.isFinite(value) ? Math.round(value / BYTES_PER_MEGABYTE) : void 0;
}
export { installRendererCrashDiagnostics as t };
