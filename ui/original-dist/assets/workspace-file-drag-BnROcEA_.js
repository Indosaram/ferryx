import { Lp as normalizeRuntimePathForComparison, to as measureClipboardTextByteLength } from "./store-CgXrfmaH.js";
const NATIVE_FILE_DROP_MAX_PATHS = 256;
const NATIVE_FILE_DROP_TARGET = {
	editor: "editor",
	terminal: "terminal",
	composer: "composer",
	fileExplorer: "file-explorer",
	projectSidebar: "project-sidebar"
};
function getDataTransferTypes(types) {
	return types ? Array.from(types) : [];
}
function hasNativeFileDragTypes(types) {
	const values = getDataTransferTypes(types);
	return values.includes("Files") && !values.includes("text/x-orca-file-path");
}
function validateNativeFileDropPaths(paths, options = {}) {
	const pathCount = paths.length;
	if (pathCount > (options.maxPaths ?? 256)) return {
		byteLength: 0,
		pathCount,
		reason: "too-many-paths",
		status: "rejected"
	};
	const maxPathBytes = options.maxPathBytes ?? 262144;
	let byteLength = 0;
	for (const path of paths) {
		const measurement = measureClipboardTextByteLength(path, { stopAfterBytes: maxPathBytes - byteLength });
		byteLength += measurement.byteLength;
		if (byteLength > maxPathBytes) return {
			byteLength,
			pathCount,
			reason: "paths-too-large",
			status: "rejected"
		};
	}
	return {
		byteLength,
		pathCount,
		status: "accepted"
	};
}
const WORKSPACE_FILE_PATH_MIME = "text/x-orca-file-path";
const WORKSPACE_FILE_PATHS_MIME = "text/x-orca-file-paths";
function encodeWorkspaceFilePaths(paths) {
	return paths.length === 1 ? paths[0] : JSON.stringify(paths);
}
function decodeWorkspaceFilePathPayload(data, options = {}) {
	if (!data) return {
		pathCount: 0,
		paths: [],
		status: "accepted"
	};
	try {
		const parsed = JSON.parse(data);
		if (Array.isArray(parsed)) return collectDecodedWorkspaceFilePaths(parsed, options.maxPaths);
	} catch {}
	if (options.maxPaths !== void 0 && options.maxPaths < 1) return {
		pathCount: 1,
		reason: "too-many-paths",
		status: "rejected"
	};
	return {
		pathCount: 1,
		paths: [data],
		status: "accepted"
	};
}
function collectDecodedWorkspaceFilePaths(values, maxPaths) {
	const paths = [];
	let pathCount = 0;
	for (const value of values) {
		if (typeof value !== "string") continue;
		pathCount += 1;
		if (maxPaths === void 0 || pathCount <= maxPaths) paths.push(value);
	}
	if (maxPaths !== void 0 && pathCount > maxPaths) return {
		pathCount,
		reason: "too-many-paths",
		status: "rejected"
	};
	return {
		pathCount,
		paths,
		status: "accepted"
	};
}
function isNormalizedRuntimePathInsideOrEqual(rootPath, candidatePath) {
	if (candidatePath === rootPath) return true;
	const rootWithBoundary = rootPath === "/" || /^[a-z]:\/$/i.test(rootPath) ? rootPath : `${rootPath.replace(/\/+$/, "")}/`;
	return candidatePath.startsWith(rootWithBoundary);
}
function getUniqueWorkspaceFilePathEntries(paths) {
	const uniquePaths = [];
	const seenNormalizedPaths = /* @__PURE__ */ new Set();
	for (const path of paths) {
		if (!path) continue;
		const normalizedPath = normalizeRuntimePathForComparison(path);
		if (!seenNormalizedPaths.has(normalizedPath)) {
			seenNormalizedPaths.add(normalizedPath);
			uniquePaths.push({
				normalizedPath,
				path
			});
		}
	}
	return uniquePaths;
}
function getTopLevelWorkspaceFilePaths(paths) {
	const uniquePaths = getUniqueWorkspaceFilePathEntries(paths);
	return uniquePaths.filter((pathEntry) => !uniquePaths.some((candidateRoot) => candidateRoot.normalizedPath !== pathEntry.normalizedPath && isNormalizedRuntimePathInsideOrEqual(candidateRoot.normalizedPath, pathEntry.normalizedPath))).map((pathEntry) => pathEntry.path);
}
function readWorkspaceFileDragPaths(dataTransfer, options = {}) {
	const maxPathBytes = options.maxPathBytes ?? 262144;
	const maxPaths = options.maxPaths ?? 256;
	const data = dataTransfer.getData("text/x-orca-file-paths") || dataTransfer.getData("text/x-orca-file-path");
	if (!data) return {
		byteLength: 0,
		pathCount: 0,
		paths: [],
		status: "accepted"
	};
	const rawMeasurement = measureClipboardTextByteLength(data, { stopAfterBytes: maxPathBytes });
	if (rawMeasurement.exceededLimit) return {
		byteLength: rawMeasurement.byteLength,
		pathCount: 0,
		reason: "paths-too-large",
		status: "rejected"
	};
	const decodedPathResult = decodeWorkspaceFilePathPayload(data, { maxPaths });
	if (decodedPathResult.status === "rejected") return {
		byteLength: 0,
		pathCount: decodedPathResult.pathCount,
		reason: decodedPathResult.reason,
		status: "rejected"
	};
	const decodedPaths = decodedPathResult.paths;
	const validation = validateNativeFileDropPaths(decodedPaths, {
		maxPathBytes,
		maxPaths
	});
	if (validation.status === "rejected") return {
		byteLength: validation.byteLength,
		pathCount: validation.pathCount,
		reason: validation.reason,
		status: "rejected"
	};
	const paths = getTopLevelWorkspaceFilePaths(decodedPaths);
	return {
		byteLength: validation.byteLength,
		pathCount: paths.length,
		paths,
		status: "accepted"
	};
}
function getWorkspaceFileDragRejectionMessage(reason) {
	if (reason === "too-many-paths") return "Drop contains too many paths.";
	return "Drop path list is too large.";
}
export { readWorkspaceFileDragPaths as a, hasNativeFileDragTypes as c, getWorkspaceFileDragRejectionMessage as i, WORKSPACE_FILE_PATH_MIME as n, NATIVE_FILE_DROP_MAX_PATHS as o, encodeWorkspaceFilePaths as r, NATIVE_FILE_DROP_TARGET as s, WORKSPACE_FILE_PATHS_MIME as t };
