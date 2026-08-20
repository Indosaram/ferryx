import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { n as init_defineProperty, t as _defineProperty } from "./defineProperty-BAtR-r70.js";
import { Ac as normalizeRelativePath, Bg as isLocalWindowsDesktopClient, C as forEachWithConcurrency, Dc as dirname, Ec as basename, Ip as isWindowsAbsolutePathLike, La as getFolderWorkspaceConnectionId, Lp as normalizeRuntimePathForComparison, Ou as getRuntimeEnvironmentIdForWorktree, Pp as isPathInsideOrEqual, Rp as normalizeRuntimePathSeparators, jp as getLocalWindowsWslPathIdentity, kc as joinPath, pt as readRuntimeFileContent, qg as isGitRepoKind, qu as findWorktreeById, t as useAppStore, vd as parseWorkspaceKey, yp as findRepoForHost, yt as subscribeRuntimeFileChanges, zp as relativePathInsideRoot } from "./store-CgXrfmaH.js";
import { st as parseExecutionHostId } from "./agent-status-3vUKbY6l.js";
import { a as track } from "./telemetry-ZyUPyKMD.js";
import { n as getConnectionIdForFile } from "./connection-context-BUPsamzR.js";
import { c as canAutoSaveOpenFile, d as isExternalReloadableEditorTab, l as getOpenFilesForExternalFileChange, p as notifyEditorExternalFileChange } from "./editor-autosave-C_Vljs6z.js";
import { n as getFileExplorerOperationOwner, r as getFileExplorerOperationOwnerFromState } from "./file-explorer-operation-owner-C4AAHFB5.js";
import { t as splitPathSegments } from "./path-tree-C_1ToHfK.js";
const ORCA_WORKTREE_FILE_CHANGE_EVENT = "orca:worktree-file-change";
function fileExplorerRefreshConcurrency(owner$1) {
	switch (owner$1.kind) {
		case "local": return 16;
		case "ssh": return 4;
		case "runtime": return parseExecutionHostId(owner$1.executionHostId)?.kind === "ssh" ? 4 : 8;
		case "unresolved": return 4;
	}
}
function createFileExplorerWatchRefreshScheduler({ refreshTree, refreshDir, isCoveredByFullRefresh, dirConcurrency, trailingMs = 150, maxWaitMs = 500, schedule = setTimeout, clear = clearTimeout }) {
	let pendingFull = false;
	const pendingDirs = /* @__PURE__ */ new Map();
	let firstRequestAt = null;
	let timer = null;
	let inFlight = null;
	let disposed = false;
	function clearTimer() {
		if (timer !== null) {
			clear(timer);
			timer = null;
		}
	}
	function arm(delayMs) {
		clearTimer();
		timer = schedule(() => {
			timer = null;
			if (inFlight === null) run();
		}, delayMs);
	}
	function armForRequest() {
		firstRequestAt ?? (firstRequestAt = Date.now());
		arm(Math.max(0, Math.min(trailingMs, firstRequestAt + maxWaitMs - Date.now())));
	}
	function run() {
		const full = pendingFull;
		const dirs = Array.from(pendingDirs.values());
		pendingFull = false;
		pendingDirs.clear();
		firstRequestAt = null;
		const started = (async () => {
			const covered = full ? new Set(dirs.filter(isCoveredByFullRefresh)) : null;
			const outcome = full && !disposed ? await refreshTree() : null;
			await forEachWithConcurrency(outcome === "root-unreadable" ? [] : covered && outcome === "refreshed" ? dirs.filter((dirPath) => !covered.has(dirPath)) : dirs, dirConcurrency, (dirPath) => disposed ? Promise.resolve() : refreshDir(dirPath));
		})();
		inFlight = started;
		started.catch(() => {}).finally(() => {
			if (inFlight === started) inFlight = null;
			if (!disposed && (pendingFull || pendingDirs.size > 0)) arm(trailingMs);
		});
	}
	return {
		requestFullRefresh: () => {
			if (disposed) return;
			pendingFull = true;
			armForRequest();
		},
		requestDirRefresh: (dirPath) => {
			if (disposed) return;
			pendingDirs.set(normalizeRuntimePathForComparison(dirPath), dirPath);
			armForRequest();
		},
		cancel: () => {
			const discardedWork = pendingFull || pendingDirs.size > 0 || timer !== null || inFlight !== null;
			disposed = true;
			clearTimer();
			pendingFull = false;
			pendingDirs.clear();
			firstRequestAt = null;
			return discardedWork;
		}
	};
}
function normalizeAbsolutePath(path) {
	const normalizedPath = normalizeRuntimePathSeparators(path);
	if (normalizedPath === "/") return normalizedPath;
	if (/^[A-Za-z]:\/$/.test(normalizedPath)) return normalizedPath;
	return normalizedPath.replace(/\/+$/, "");
}
function normalizeAbsolutePathForComparison(path) {
	return normalizeRuntimePathForComparison(path);
}
function isPathEqualOrDescendant(candidatePath, targetPath) {
	return isPathInsideOrEqual(targetPath, candidatePath);
}
function getRevealAncestorDirs(worktreePath, filePath) {
	const relativePath = relativePathInsideRoot(worktreePath, filePath);
	if (relativePath === null) return null;
	const segments = splitPathSegments(normalizeRelativePath(relativePath));
	const ancestorDirs = [];
	let currentPath = worktreePath;
	for (const segment of segments.slice(0, -1)) {
		currentPath = joinPath(currentPath, segment);
		ancestorDirs.push(currentPath);
	}
	return ancestorDirs;
}
function createSubtreeMatcher(paths) {
	const normalizedRoots = new Set([...paths].map(normalizeRuntimePathForComparison));
	return (candidatePath) => {
		const candidate = normalizeRuntimePathForComparison(candidatePath);
		if (normalizedRoots.has(candidate)) return true;
		if (candidate.startsWith("/") && normalizedRoots.has("/")) return true;
		for (let index = candidate.indexOf("/"); index >= 0; index = candidate.indexOf("/", index + 1)) {
			if (index === 2 && /^[a-z]:\//.test(candidate) && normalizedRoots.has(candidate.slice(0, 3))) return true;
			if (index > 0 && normalizedRoots.has(candidate.slice(0, index))) return true;
		}
		return false;
	};
}
function purgeDirCacheSubtrees(setDirCache, deletedPaths) {
	if (deletedPaths.size === 0) return;
	const shouldPurge = createSubtreeMatcher(deletedPaths);
	setDirCache((prev) => {
		let changed = false;
		const next = {};
		for (const key of Object.keys(prev)) if (shouldPurge(key)) changed = true;
		else next[key] = prev[key];
		return changed ? next : prev;
	});
}
function purgeExpandedDirsSubtrees(worktreeId, deletedPaths) {
	if (deletedPaths.size === 0) return;
	const shouldPurge = createSubtreeMatcher(deletedPaths);
	useAppStore.setState((state) => {
		const current = state.expandedDirs[worktreeId];
		if (!current) return state;
		const next = /* @__PURE__ */ new Set();
		let changed = false;
		for (const dirPath of current) if (shouldPurge(dirPath)) changed = true;
		else next.add(dirPath);
		if (!changed) return state;
		return { expandedDirs: {
			...state.expandedDirs,
			[worktreeId]: next
		} };
	});
}
function clearStalePendingReveal(deletedPath) {
	const normalized = normalizeAbsolutePath(deletedPath);
	useAppStore.setState((state) => {
		if (state.pendingExplorerReveal && isPathEqualOrDescendant(normalizeAbsolutePath(state.pendingExplorerReveal.filePath), normalized)) return { pendingExplorerReveal: null };
		return state;
	});
}
function normalizeExplorerAbsolutePath(path) {
	return path === "/" || /^[A-Za-z]:[\\/]$/.test(path) ? path : path.replace(/[\\/]+$/, "");
}
function getExternalFileChangeRelativePath(worktreePath, absolutePath, isDirectory) {
	if (isDirectory === true) return null;
	const relativePath = relativePathInsideRoot(worktreePath, absolutePath);
	if (relativePath === null || relativePath === "") return null;
	return normalizeRelativePath(relativePath);
}
function canonicalizeFileExplorerWatchPath(worktreePath, absolutePath) {
	const relativePath = relativePathInsideRoot(worktreePath, absolutePath);
	if (relativePath === null) return null;
	const rootPath = normalizeExplorerAbsolutePath(worktreePath);
	return relativePath === "" ? rootPath : joinPath(rootPath, relativePath);
}
function createCachedDirPathIndex(cache) {
	const index = /* @__PURE__ */ new Map();
	for (const key of Object.keys(cache)) {
		const normalizedKey = normalizeRuntimePathForComparison(key);
		if (!index.has(normalizedKey)) index.set(normalizedKey, key);
	}
	return index;
}
function resolveCachedDirPath(cache, dirPath, worktreePath, cachePathIndex) {
	if (dirPath in cache) return dirPath;
	const target = normalizeRuntimePathForComparison(dirPath);
	const indexedPath = cachePathIndex?.get(target);
	if (indexedPath) return indexedPath;
	if (!cachePathIndex) {
		for (const key of Object.keys(cache)) if (normalizeRuntimePathForComparison(key) === target) return key;
	}
	if (worktreePath && normalizeRuntimePathForComparison(worktreePath) === target) return normalizeExplorerAbsolutePath(worktreePath);
	return null;
}
function parentDirForWatchPath(normalizedPath) {
	const parentPath = dirname(normalizedPath);
	if (/^[A-Za-z]:$/.test(parentPath)) return `${parentPath}${normalizedPath.includes("\\") ? "\\" : "/"}`;
	return normalizeExplorerAbsolutePath(parentPath);
}
function cachedDirectoryContainsPath(cache, cachedDirPath, childPath, childPathIndexes) {
	let childPaths = childPathIndexes.get(cachedDirPath);
	if (!childPaths) {
		childPaths = new Set(cache[cachedDirPath]?.children.map((child) => normalizeRuntimePathForComparison(child.path)) ?? []);
		childPathIndexes.set(cachedDirPath, childPaths);
	}
	return childPaths.has(normalizeRuntimePathForComparison(childPath));
}
function processFileExplorerFsPayload(args) {
	const { payload, currentWorktreePath, worktreeId, cache, setDirCache, setSelectedPath, refreshDir, refreshTree } = args;
	if (normalizeRuntimePathForComparison(payload.worktreePath) !== normalizeRuntimePathForComparison(currentWorktreePath)) return;
	const dirsToRefresh = /* @__PURE__ */ new Set();
	const childPathIndexes = /* @__PURE__ */ new Map();
	const cachePathIndex = createCachedDirPathIndex(cache);
	const cachedDirsToPurge = /* @__PURE__ */ new Set();
	const reconciledRenameSources = /* @__PURE__ */ new Set();
	let needsFullRefresh = false;
	const queueCachedDirPurge = (cachedDir) => {
		if (cachedDir) cachedDirsToPurge.add(cachedDir);
	};
	for (const evt of payload.events) {
		if (evt.kind === "overflow") {
			needsFullRefresh = true;
			break;
		}
		const normalizedPath = canonicalizeFileExplorerWatchPath(currentWorktreePath, evt.absolutePath);
		if (!normalizedPath) continue;
		if (evt.kind === "delete") {
			const cachedDir = resolveCachedDirPath(cache, normalizedPath, currentWorktreePath, cachePathIndex);
			const wasDirectory = cachedDir !== null;
			if (wasDirectory && cachedDir) queueCachedDirPurge(cachedDir);
			clearStalePendingReveal(normalizedPath);
			setSelectedPath((prev) => {
				if (prev && normalizeRuntimePathForComparison(prev) === normalizeRuntimePathForComparison(normalizedPath)) return null;
				if (prev && wasDirectory && isPathInsideOrEqual(normalizedPath, prev)) return null;
				return prev;
			});
			const cachedParent = resolveCachedDirPath(cache, parentDirForWatchPath(normalizedPath), currentWorktreePath, cachePathIndex);
			if (cachedParent) dirsToRefresh.add(cachedParent);
		} else if (evt.kind === "create" || evt.kind === "rename") {
			const cachedParent = resolveCachedDirPath(cache, parentDirForWatchPath(normalizedPath), currentWorktreePath, cachePathIndex);
			if (cachedParent) dirsToRefresh.add(cachedParent);
			if (evt.kind === "rename") {
				const oldPath = evt.oldAbsolutePath ? canonicalizeFileExplorerWatchPath(currentWorktreePath, evt.oldAbsolutePath) : null;
				const cachedOldDir = oldPath ? resolveCachedDirPath(cache, oldPath, currentWorktreePath, cachePathIndex) : null;
				if (oldPath) {
					const cachedOldParent = resolveCachedDirPath(cache, parentDirForWatchPath(oldPath), currentWorktreePath, cachePathIndex);
					if (cachedOldParent) dirsToRefresh.add(cachedOldParent);
					const sourceKey = normalizeRuntimePathForComparison(oldPath);
					if (!reconciledRenameSources.has(sourceKey)) {
						reconciledRenameSources.add(sourceKey);
						clearStalePendingReveal(oldPath);
						setSelectedPath((prev) => {
							if (!prev) return prev;
							if (normalizeRuntimePathForComparison(prev) === sourceKey) return null;
							return cachedOldDir && isPathInsideOrEqual(oldPath, prev) ? null : prev;
						});
					}
				}
				const cachedNewDir = resolveCachedDirPath(cache, normalizedPath, currentWorktreePath, cachePathIndex);
				queueCachedDirPurge(cachedOldDir);
				queueCachedDirPurge(cachedNewDir);
			}
		} else if (evt.kind === "update") {
			const cachedDir = resolveCachedDirPath(cache, normalizedPath, currentWorktreePath, cachePathIndex);
			if (evt.isDirectory === true && cachedDir) {
				dirsToRefresh.add(cachedDir);
				continue;
			}
			const cachedParent = resolveCachedDirPath(cache, parentDirForWatchPath(normalizedPath), currentWorktreePath, cachePathIndex);
			if (cachedParent && !dirsToRefresh.has(cachedParent) && !cachedDirectoryContainsPath(cache, cachedParent, normalizedPath, childPathIndexes)) dirsToRefresh.add(cachedParent);
		}
	}
	purgeDirCacheSubtrees(setDirCache, cachedDirsToPurge);
	purgeExpandedDirsSubtrees(worktreeId, cachedDirsToPurge);
	if (needsFullRefresh) {
		refreshTree();
		return;
	}
	const rootPath = normalizeExplorerAbsolutePath(currentWorktreePath);
	for (const dirPath of dirsToRefresh) if (normalizeRuntimePathForComparison(dirPath) === normalizeRuntimePathForComparison(rootPath) || dirPath in cache) refreshDir(dirPath);
}
var import_react = /* @__PURE__ */ __toESM(require_react());
function getFileExplorerWatchRuntimeEnvironmentId(state, activeWorktreeId, expectedOwner) {
	const owner$1 = getFileExplorerOperationOwnerFromState({
		settings: state.settings,
		repos: state.repos,
		worktreesByRepo: state.worktreesByRepo,
		detectedWorktreesByRepo: state.detectedWorktreesByRepo ?? {},
		folderWorkspaces: state.folderWorkspaces ?? [],
		projectGroups: state.projectGroups ?? [],
		restoredRuntimeHostIdByWorkspaceSessionKey: state.restoredRuntimeHostIdByWorkspaceSessionKey ?? {}
	}, activeWorktreeId);
	if (expectedOwner && JSON.stringify(owner$1) !== JSON.stringify(expectedOwner)) return;
	return owner$1.kind === "runtime" ? owner$1.environmentId : owner$1.kind === "unresolved" ? void 0 : null;
}
function useFileExplorerWatch({ worktreePath, activeWorktreeId, dirCache, setDirCache, expanded, setSelectedPath, refreshDir, refreshTree, inlineInput, dragSourcePath, isNativeDragOver, operationOwner }) {
	const activeRuntimeEnvironmentId = useAppStore((s) => getFileExplorerWatchRuntimeEnvironmentId(s, activeWorktreeId, operationOwner));
	const dirCacheRef = (0, import_react.useRef)(dirCache);
	dirCacheRef.current = dirCache;
	const expandedRef = (0, import_react.useRef)(expanded);
	expandedRef.current = expanded;
	const inlineInputRef = (0, import_react.useRef)(inlineInput);
	inlineInputRef.current = inlineInput;
	const dragSourceRef = (0, import_react.useRef)(dragSourcePath);
	dragSourceRef.current = dragSourcePath;
	const isNativeDragOverRef = (0, import_react.useRef)(isNativeDragOver);
	isNativeDragOverRef.current = isNativeDragOver;
	const refreshDirRef = (0, import_react.useRef)(refreshDir);
	refreshDirRef.current = refreshDir;
	const refreshTreeRef = (0, import_react.useRef)(refreshTree);
	refreshTreeRef.current = refreshTree;
	const deferredRef = (0, import_react.useRef)([]);
	const resyncWatchKeysRef = (0, import_react.useRef)(/* @__PURE__ */ new Set());
	const activeResyncByWatchKeyRef = (0, import_react.useRef)(/* @__PURE__ */ new Map());
	const processPayloadRef = (0, import_react.useRef)(null);
	(0, import_react.useEffect)(() => {
		if (!worktreePath || !activeWorktreeId || activeRuntimeEnvironmentId === void 0) return;
		const currentWorktreePath = worktreePath;
		const currentWorktreeId = activeWorktreeId;
		const resyncWatchKeys = resyncWatchKeysRef.current;
		const activeResyncByWatchKey = activeResyncByWatchKeyRef.current;
		const currentWatchKey = JSON.stringify([currentWorktreeId, normalizeRuntimePathForComparison(currentWorktreePath)]);
		const scheduler = createFileExplorerWatchRefreshScheduler({
			refreshTree: () => refreshTreeRef.current(),
			refreshDir: (dirPath) => refreshDirRef.current(dirPath),
			isCoveredByFullRefresh: (dirPath) => normalizeRuntimePathForComparison(dirPath) === normalizeRuntimePathForComparison(currentWorktreePath) || expandedRef.current.has(dirPath),
			dirConcurrency: fileExplorerRefreshConcurrency(getFileExplorerOperationOwner(currentWorktreeId)),
			trailingMs: 0,
			maxWaitMs: 0
		});
		activeResyncByWatchKey.set(currentWatchKey, scheduler.requestFullRefresh);
		if (resyncWatchKeys.delete(currentWatchKey)) scheduler.requestFullRefresh();
		function processPayload(payload) {
			processFileExplorerFsPayload({
				payload,
				currentWorktreePath,
				worktreeId: currentWorktreeId,
				cache: dirCacheRef.current,
				expanded: expandedRef.current,
				setDirCache,
				setSelectedPath,
				refreshDir: scheduler.requestDirRefresh,
				refreshTree: scheduler.requestFullRefresh
			});
		}
		processPayloadRef.current = processPayload;
		let disposed = false;
		const handleFsChanged = (payload) => {
			if (disposed) {
				if (normalizeRuntimePathForComparison(payload.worktreePath) === normalizeRuntimePathForComparison(currentWorktreePath)) {
					const requestActiveResync = activeResyncByWatchKey.get(currentWatchKey);
					if (requestActiveResync) requestActiveResync();
					else resyncWatchKeys.add(currentWatchKey);
				}
				return;
			}
			if (inlineInputRef.current !== null || dragSourceRef.current !== null || isNativeDragOverRef.current) {
				deferredRef.current.push(payload);
				return;
			}
			processPayload(payload);
		};
		let unsubscribeListener = null;
		if (activeRuntimeEnvironmentId?.trim() && activeWorktreeId) subscribeRuntimeFileChanges({
			settings: { activeRuntimeEnvironmentId },
			worktreeId: activeWorktreeId,
			worktreePath,
			connectionId: void 0
		}, handleFsChanged, (err) => {
			console.warn("[filesystem-watch] failed to subscribe to runtime file changes", {
				worktreeId: activeWorktreeId,
				worktreePath,
				error: err.message
			});
		}).then((unsubscribe) => {
			if (disposed) {
				unsubscribe();
				return;
			}
			unsubscribeListener = unsubscribe;
		}).catch((err) => {
			console.warn("[filesystem-watch] failed to subscribe to runtime file changes", {
				worktreeId: activeWorktreeId,
				worktreePath,
				error: err instanceof Error ? err.message : String(err)
			});
		});
		else unsubscribeListener = window.api.fs.onFsChanged(handleFsChanged);
		return () => {
			disposed = true;
			unsubscribeListener?.();
			if (activeResyncByWatchKey.get(currentWatchKey) === scheduler.requestFullRefresh) activeResyncByWatchKey.delete(currentWatchKey);
			const hadDeferredEvents = deferredRef.current.length > 0;
			if (scheduler.cancel() || hadDeferredEvents) resyncWatchKeys.add(currentWatchKey);
			deferredRef.current = [];
			processPayloadRef.current = null;
		};
	}, [
		worktreePath,
		activeWorktreeId,
		activeRuntimeEnvironmentId,
		setDirCache,
		setSelectedPath
	]);
	(0, import_react.useEffect)(() => {
		if (inlineInput === null && dragSourcePath === null && !isNativeDragOver && deferredRef.current.length > 0) {
			const deferred = deferredRef.current.splice(0);
			if (processPayloadRef.current) for (const payload of deferred) processPayloadRef.current(payload);
		}
	}, [
		inlineInput,
		dragSourcePath,
		isNativeDragOver
	]);
}
init_defineProperty();
function openFileRuntimeOwner$1(file) {
	return file.runtimeEnvironmentId?.trim() || null;
}
function addToListMap(map, key, value) {
	const existing = map.get(key);
	if (existing) existing.push(value);
	else map.set(key, [value]);
}
var IndexedPathLookup = class {
	constructor(allowAliases) {
		_defineProperty(this, "direct", /* @__PURE__ */ new Map());
		_defineProperty(this, "aliases", /* @__PURE__ */ new Map());
		_defineProperty(this, "wslAliases", /* @__PURE__ */ new Map());
		this.allowAliases = allowAliases;
	}
	add(path, value) {
		this.direct.set(path.identity.normalizedPath, value);
		if (!this.allowAliases) return;
		this.aliases.set(path.identity.aliasComparisonPath, value);
		if (path.identity.isWslUnc) this.wslAliases.set(path.identity.aliasComparisonPath, value);
	}
	get(identity) {
		const direct = this.direct.get(identity.normalizedPath);
		if (direct !== void 0 || !this.allowAliases) return direct;
		return identity.isWslUnc ? this.aliases.get(identity.aliasComparisonPath) : this.wslAliases.get(identity.aliasComparisonPath);
	}
};
function pathIdentity(value, allowAliases) {
	if (allowAliases) return getLocalWindowsWslPathIdentity(value);
	const normalizedPath = normalizeRuntimePathForComparison(value);
	return {
		normalizedPath,
		aliasComparisonPath: normalizedPath,
		isWslUnc: false
	};
}
function collectMatchingFiles(direct, aliases, diffs) {
	const byIndex = /* @__PURE__ */ new Map();
	for (const entry of [
		...direct,
		...aliases,
		...diffs
	]) byIndex.set(entry.index, entry.file);
	return [...byIndex.entries()].sort(([left], [right]) => left - right).map(([, file]) => file);
}
var IndexedOpenFileLookup = class {
	constructor(openFiles, scope, allowAliases) {
		_defineProperty(this, "directEditors", /* @__PURE__ */ new Map());
		_defineProperty(this, "aliasEditors", /* @__PURE__ */ new Map());
		_defineProperty(this, "wslAliasEditors", /* @__PURE__ */ new Map());
		_defineProperty(this, "diffsByRelativePath", /* @__PURE__ */ new Map());
		_defineProperty(this, "indexedOpenFiles", /* @__PURE__ */ new Map());
		_defineProperty(this, "hasCombinedDiffConsumer", void 0);
		this.allowAliases = allowAliases;
		let hasCombinedDiffConsumer = false;
		for (const [index, file] of openFiles.entries()) {
			if (file.worktreeId !== scope.worktreeId || openFileRuntimeOwner$1(file) !== scope.runtimeEnvironmentId) continue;
			if (file.mode === "diff" && (file.diffSource === "combined-uncommitted" || file.diffSource === "combined-all")) {
				hasCombinedDiffConsumer = true;
				continue;
			}
			if (file.mode === "diff") {
				if (file.diffSource === "unstaged" || file.diffSource === "staged") addToListMap(this.diffsByRelativePath, file.relativePath, {
					file,
					index,
					identity: null
				});
				continue;
			}
			if (file.mode !== "edit" && file.mode !== "markdown-preview") continue;
			const identity = pathIdentity(file.filePath, allowAliases);
			const indexedFile = {
				file,
				index,
				identity
			};
			this.indexedOpenFiles.set(file.id, indexedFile);
			addToListMap(this.directEditors, file.filePath, indexedFile);
			if (allowAliases) {
				addToListMap(this.aliasEditors, identity.aliasComparisonPath, indexedFile);
				if (identity.isWslUnc) addToListMap(this.wslAliasEditors, identity.aliasComparisonPath, indexedFile);
			}
		}
		this.hasCombinedDiffConsumer = hasCombinedDiffConsumer;
	}
	matchingOpenFiles(change) {
		const aliases = !this.allowAliases ? [] : change.identity.isWslUnc ? this.aliasEditors.get(change.identity.aliasComparisonPath) ?? [] : this.wslAliasEditors.get(change.identity.aliasComparisonPath) ?? [];
		return collectMatchingFiles(this.directEditors.get(change.absolutePath) ?? [], aliases, this.diffsByRelativePath.get(change.relativePath) ?? []);
	}
};
function indexEditorExternalWatchBatchPaths(payload, openFiles, scope) {
	const allowAliases = scope.allowLocalWindowsWslAliases === true;
	const createOrUpdateLookup = new IndexedPathLookup(allowAliases);
	const deleteLookup = new IndexedPathLookup(allowAliases);
	const createOrUpdatePaths = /* @__PURE__ */ new Map();
	const changesByRelativePath = /* @__PURE__ */ new Map();
	for (const event of payload.events) {
		if (event.kind === "overflow") continue;
		const eventPath = {
			absolutePath: event.absolutePath,
			identity: pathIdentity(event.absolutePath, allowAliases)
		};
		if (event.kind === "delete") {
			deleteLookup.add(eventPath, eventPath);
			continue;
		}
		if (event.isDirectory !== true) {
			createOrUpdatePaths.set(eventPath.identity.normalizedPath, event.absolutePath);
			createOrUpdateLookup.add(eventPath, eventPath);
		}
		const relativePath = getExternalFileChangeRelativePath(scope.worktreePath, event.absolutePath, event.isDirectory);
		if (relativePath && !changesByRelativePath.has(relativePath)) {
			const absolutePath = joinPath(scope.worktreePath, relativePath);
			changesByRelativePath.set(relativePath, {
				relativePath,
				absolutePath,
				identity: eventPath.identity
			});
		}
	}
	const initialOpenFileLookup = new IndexedOpenFileLookup(openFiles, scope, allowAliases);
	const openFileLookups = /* @__PURE__ */ new WeakMap();
	openFileLookups.set(openFiles, initialOpenFileLookup);
	const getOpenFileLookup = (currentOpenFiles) => {
		const existing = openFileLookups.get(currentOpenFiles);
		if (existing) return existing;
		const indexed = new IndexedOpenFileLookup(currentOpenFiles, scope, allowAliases);
		openFileLookups.set(currentOpenFiles, indexed);
		return indexed;
	};
	const matchesCreateOrUpdate = (file) => {
		const identity = initialOpenFileLookup.indexedOpenFiles.get(file.id)?.identity ?? pathIdentity(file.filePath, allowAliases);
		return createOrUpdateLookup.get(identity) !== void 0;
	};
	const deletedOpenEditors = [];
	for (const indexedFile of initialOpenFileLookup.indexedOpenFiles.values()) {
		const deletedPath = deleteLookup.get(indexedFile.identity);
		if (deletedPath) deletedOpenEditors.push({
			file: indexedFile.file,
			normalizedDeletePath: deletedPath.identity.normalizedPath
		});
	}
	return {
		createOrUpdatePaths,
		changes: [...changesByRelativePath.values()],
		deletedOpenEditors,
		hasCombinedDiffConsumer: initialOpenFileLookup.hasCombinedDiffConsumer,
		matchesCreateOrUpdate,
		matchingOpenFiles: (change, currentOpenFiles = openFiles) => getOpenFileLookup(currentOpenFiles).matchingOpenFiles(change)
	};
}
var SELF_WRITE_TTL_MS = 750;
const SELF_WRITE_REMOTE_TTL_MS = 3e3;
var SELF_WRITE_MAX_STAMPS = 256;
var stamps = /* @__PURE__ */ new Map();
function selfWriteKey(absolutePath, runtimeEnvironmentId) {
	return `${runtimeEnvironmentId?.trim() || "client"}::${normalizeAbsolutePathForComparison(absolutePath)}`;
}
function pruneExpiredSelfWrites(now = Date.now()) {
	for (const [key, stamp] of stamps) if (now > stamp.expiresAt) stamps.delete(key);
}
function enforceSelfWriteStampLimit() {
	while (stamps.size > SELF_WRITE_MAX_STAMPS) {
		const oldest = stamps.keys().next().value;
		if (oldest === void 0) break;
		stamps.delete(oldest);
	}
}
function recordSelfWrite(absolutePath, content, runtimeEnvironmentId, ttlMs = SELF_WRITE_TTL_MS) {
	const now = Date.now();
	pruneExpiredSelfWrites(now);
	const key = selfWriteKey(absolutePath, runtimeEnvironmentId);
	stamps.delete(key);
	stamps.set(key, {
		content: content ?? null,
		expiresAt: now + ttlMs
	});
	enforceSelfWriteStampLimit();
}
function clearSelfWrite(absolutePath, runtimeEnvironmentId) {
	stamps.delete(selfWriteKey(absolutePath, runtimeEnvironmentId));
}
function getRecentSelfWrite(absolutePath, runtimeEnvironmentId) {
	const key = selfWriteKey(absolutePath, runtimeEnvironmentId);
	const stamp = stamps.get(key);
	if (!stamp) return null;
	if (Date.now() > stamp.expiresAt) {
		stamps.delete(key);
		return null;
	}
	return { content: stamp.content };
}
function hasRecentSelfWrite(absolutePath, runtimeEnvironmentId) {
	return getRecentSelfWrite(absolutePath, runtimeEnvironmentId) !== null;
}
var operations = /* @__PURE__ */ new Map();
function owner(runtimeEnvironmentId) {
	return runtimeEnvironmentId?.trim() || null;
}
function normalize(absolutePath) {
	return normalizeAbsolutePathForComparison(absolutePath);
}
function isInsideOrEqual(root, candidate) {
	return candidate === root || candidate.startsWith(`${root}/`);
}
function beginEditorPathMove(args) {
	operations.set(args.operationId, {
		worktreeId: args.worktreeId,
		runtimeEnvironmentId: owner(args.runtimeEnvironmentId),
		sourceRoots: args.sourcePaths.map(normalize)
	});
}
function settleEditorPathMove(operationId) {
	operations.delete(operationId);
}
function hasActiveEditorPathMoves() {
	return operations.size > 0;
}
function isActiveMoveSourcePath(worktreeId, runtimeEnvironmentId, absolutePath) {
	if (operations.size === 0) return false;
	const normalizedPath = normalize(absolutePath);
	const scopedOwner = owner(runtimeEnvironmentId);
	for (const operation of operations.values()) {
		if (operation.worktreeId !== worktreeId || operation.runtimeEnvironmentId !== scopedOwner) continue;
		if (operation.sourceRoots.some((root) => isInsideOrEqual(root, normalizedPath))) return true;
	}
	return false;
}
function conflictSurface(file) {
	return file.mode === "edit" ? "edit" : "unstaged-diff";
}
function conflictTransport(connectionId, runtimeEnvironmentId) {
	if (connectionId) return "ssh";
	if (runtimeEnvironmentId?.trim()) return "runtime";
	return "local";
}
function trackExternalChangeConflictShown(file, options) {
	track("editor_external_change_conflict_shown", {
		surface: conflictSurface(file),
		transport: conflictTransport(options.connectionId, file.runtimeEnvironmentId),
		origin: options.origin
	});
}
function trackExternalChangeConflictAction(file, action) {
	track("editor_external_change_conflict_action", {
		action,
		surface: conflictSurface(file),
		transport: conflictTransport(getConnectionIdForFile(file.worktreeId, file.filePath) ?? void 0, file.runtimeEnvironmentId)
	});
}
function markFileChangedOnDisk(state, file, options) {
	if (!file.isDirty || !canAutoSaveOpenFile(file)) return;
	if (file.externalMutation !== "changed") trackExternalChangeConflictShown(file, options);
	state.setExternalMutation(file.id, "changed");
}
function getDiffContentSignature(content) {
	let hash = 2166136261;
	for (let i = 0; i < content.length; i += 1) {
		hash ^= content.charCodeAt(i);
		hash = Math.imul(hash, 16777619);
	}
	return (hash >>> 0).toString(16);
}
function getDiskBaselineSignature(content) {
	let hashA = 2166136261;
	let hashB = 84696351;
	for (let i = 0; i < content.length; i += 1) {
		const code = content.charCodeAt(i);
		hashA ^= code;
		hashA = Math.imul(hashA, 16777619);
		hashB = Math.imul(hashB ^ code, 1099511627);
	}
	return `${(hashA >>> 0).toString(16)}-${(hashB >>> 0).toString(16)}-${content.length.toString(16)}`;
}
var EXTERNAL_RELOAD_DEBOUNCE_MS = 75;
var pendingExternalReloadTimers = /* @__PURE__ */ new Map();
function warnExternalWatchFailure(target, err) {
	console.warn("[filesystem-watch] failed to watch worktree", {
		worktreeId: target.worktreeId,
		worktreePath: target.worktreePath,
		connectionId: target.connectionId,
		error: err instanceof Error ? err.message : String(err)
	});
}
function scheduleDebouncedExternalReload(notification) {
	const key = `${notification.worktreeId}::${notification.runtimeEnvironmentId ?? "client"}::${notification.relativePath}`;
	const existing = pendingExternalReloadTimers.get(key);
	if (existing !== void 0) globalThis.clearTimeout(existing);
	const handle = globalThis.setTimeout(() => {
		pendingExternalReloadTimers.delete(key);
		notifyEditorExternalFileChange(notification);
	}, EXTERNAL_RELOAD_DEBOUNCE_MS);
	pendingExternalReloadTimers.set(key, handle);
}
function localWslAliasOption(target) {
	return isLocalWindowsDesktopClient() && target.allowLocalWindowsWslAliases === true ? { allowLocalWindowsWslAliases: true } : {};
}
function isLocalHostStamp(value) {
	return parseExecutionHostId(value)?.kind === "local";
}
function canWatchLocalWindowsWslAliases(args) {
	if (args.runtimeEnvironmentId !== null || args.connectionId !== null || !isWindowsAbsolutePathLike(args.worktreePath)) return false;
	if (args.worktree) return !!args.repo && !args.worktree.runtimeOwnerEnvironmentId?.trim() && isLocalHostStamp(args.worktree.hostId) && isLocalHostStamp(args.repo.executionHostId);
	return !!args.folderWorkspace && isLocalHostStamp(args.folderWorkspace.executionHostId) && isLocalHostStamp(args.projectGroup?.executionHostId);
}
var cachedOpenFiles = null;
var cachedWorktreesByRepo = null;
var cachedRepos = null;
var cachedActiveWorktreeId = null;
var cachedRuntimeEnvironmentId;
var cachedRightSidebarOpen = null;
var cachedRightSidebarTab = null;
var cachedRightSidebarExplorerView = null;
var cachedGitStatusHugeByWorktree = null;
var cachedSshConnectionStates = null;
var cachedFolderWorkspaces = null;
var cachedProjectGroups = null;
var cachedWatchedTargetsSnapshot = {
	targets: [],
	targetsKey: ""
};
function getWatchedTargetKey(target) {
	return `${target.worktreeId}::${target.worktreePath}::${target.connectionId ?? "local"}::${target.runtimeEnvironmentId ?? "client"}::${target.allowLocalWindowsWslAliases === true ? "wsl-aliases" : "literal"}`;
}
function openFileRuntimeOwner(file) {
	return file.runtimeEnvironmentId?.trim() || null;
}
function getEditorExternalWatchTargets(state) {
	const runtimeEnvironmentId = state.settings?.activeRuntimeEnvironmentId?.trim() || void 0;
	if (cachedOpenFiles === state.openFiles && cachedWorktreesByRepo === state.worktreesByRepo && cachedRepos === state.repos && cachedActiveWorktreeId === state.activeWorktreeId && cachedRuntimeEnvironmentId === runtimeEnvironmentId && cachedRightSidebarOpen === state.rightSidebarOpen && cachedRightSidebarTab === state.rightSidebarTab && cachedRightSidebarExplorerView === state.rightSidebarExplorerView && cachedGitStatusHugeByWorktree === state.gitStatusHugeByWorktree && cachedSshConnectionStates === state.sshConnectionStates && cachedFolderWorkspaces === state.folderWorkspaces && cachedProjectGroups === state.projectGroups) return cachedWatchedTargetsSnapshot;
	const targetOwnersByWorktreeId = /* @__PURE__ */ new Map();
	for (const f of state.openFiles) {
		let owners = targetOwnersByWorktreeId.get(f.worktreeId);
		if (!owners) {
			owners = /* @__PURE__ */ new Set();
			targetOwnersByWorktreeId.set(f.worktreeId, owners);
		}
		owners.add(openFileRuntimeOwner(f));
	}
	const activeWorktreeId = state.activeWorktreeId;
	const activeWorktree = activeWorktreeId ? findWorktreeById(state.worktreesByRepo, activeWorktreeId) : void 0;
	const activeWorktreeHost = parseExecutionHostId(activeWorktree?.hostId);
	const activeRepo = activeWorktree ? activeWorktreeHost?.kind === "local" ? findRepoForHost(state.repos, activeWorktree.repoId, { hostId: activeWorktreeHost.id }) ?? void 0 : state.repos.find((repo) => repo.id === activeWorktree.repoId) : void 0;
	const sourceControlCanConsumeWatch = !!activeWorktreeId && !!activeRepo && isGitRepoKind(activeRepo) && !state.gitStatusHugeByWorktree[activeWorktreeId] && (!activeRepo.connectionId || state.sshConnectionStates.get(activeRepo.connectionId)?.status === "connected");
	if (activeWorktreeId !== null && state.rightSidebarOpen && (state.rightSidebarTab === "explorer" && state.rightSidebarExplorerView === "files" || state.rightSidebarTab === "source-control" && sourceControlCanConsumeWatch)) {
		let owners = targetOwnersByWorktreeId.get(activeWorktreeId);
		if (!owners) {
			owners = /* @__PURE__ */ new Set();
			targetOwnersByWorktreeId.set(activeWorktreeId, owners);
		}
		owners.add(getRuntimeEnvironmentIdForWorktree(state, activeWorktreeId));
	}
	const nextTargets = [];
	const parts = [];
	const sortedWorktreeIds = Array.from(targetOwnersByWorktreeId.keys()).sort();
	for (const id of sortedWorktreeIds) {
		const wt = findWorktreeById(state.worktreesByRepo, id);
		const workspaceScope = parseWorkspaceKey(id);
		const folderWorkspace = workspaceScope?.type === "folder" ? state.folderWorkspaces.find((workspace) => workspace.id === workspaceScope.folderWorkspaceId) : void 0;
		if (!wt && !folderWorkspace) continue;
		const worktreeHost = parseExecutionHostId(wt?.hostId);
		const repo = wt ? worktreeHost?.kind === "local" ? findRepoForHost(state.repos, wt.repoId, { hostId: worktreeHost.id }) ?? void 0 : state.repos.find((r) => r.id === wt.repoId) : void 0;
		const folderHostId = parseExecutionHostId(folderWorkspace?.executionHostId)?.id;
		const projectGroup = folderWorkspace ? state.projectGroups.find((group) => group.id === folderWorkspace.projectGroupId && parseExecutionHostId(group.executionHostId)?.id === folderHostId) : void 0;
		const connectionId = folderWorkspace ? getFolderWorkspaceConnectionId(state, folderWorkspace.id) : repo ? repo.connectionId ?? null : void 0;
		if (connectionId === void 0 && folderWorkspace) continue;
		const owners = Array.from(targetOwnersByWorktreeId.get(id) ?? []).sort((a, b) => (a ?? "").localeCompare(b ?? ""));
		for (const owner$1 of owners) {
			const target = {
				worktreeId: id,
				worktreePath: wt?.path ?? folderWorkspace.folderPath,
				connectionId: connectionId ?? void 0,
				runtimeEnvironmentId: owner$1,
				...canWatchLocalWindowsWslAliases({
					worktreePath: wt?.path ?? folderWorkspace.folderPath,
					runtimeEnvironmentId: owner$1,
					connectionId,
					worktree: wt,
					repo,
					folderWorkspace,
					projectGroup
				}) ? { allowLocalWindowsWslAliases: true } : {}
			};
			nextTargets.push(target);
			parts.push(getWatchedTargetKey(target));
		}
	}
	const targetsKey = parts.join("|");
	cachedOpenFiles = state.openFiles;
	cachedWorktreesByRepo = state.worktreesByRepo;
	cachedRepos = state.repos;
	cachedActiveWorktreeId = state.activeWorktreeId;
	cachedRuntimeEnvironmentId = runtimeEnvironmentId;
	cachedRightSidebarOpen = state.rightSidebarOpen;
	cachedRightSidebarTab = state.rightSidebarTab;
	cachedRightSidebarExplorerView = state.rightSidebarExplorerView;
	cachedGitStatusHugeByWorktree = state.gitStatusHugeByWorktree;
	cachedSshConnectionStates = state.sshConnectionStates;
	cachedFolderWorkspaces = state.folderWorkspaces;
	cachedProjectGroups = state.projectGroups;
	if (targetsKey === cachedWatchedTargetsSnapshot.targetsKey) return cachedWatchedTargetsSnapshot;
	cachedWatchedTargetsSnapshot = {
		targets: nextTargets,
		targetsKey
	};
	return cachedWatchedTargetsSnapshot;
}
var EXTERNAL_MUTATION_DEBOUNCE_MS = 75;
function useEditorExternalWatch() {
	const { targets, targetsKey } = useAppStore(getEditorExternalWatchTargets);
	const targetsRef = (0, import_react.useRef)([]);
	const latestTargetsRef = (0, import_react.useRef)(targets);
	latestTargetsRef.current = targets;
	const remoteWatchUnsubsRef = (0, import_react.useRef)(/* @__PURE__ */ new Map());
	const fsChangedHandlerRef = (0, import_react.useRef)(null);
	(0, import_react.useEffect)(() => {
		const nextTargets = latestTargetsRef.current;
		const prev = targetsRef.current;
		const prevKeys = new Set(prev.map(getWatchedTargetKey));
		const nextKeys = new Set(nextTargets.map(getWatchedTargetKey));
		const removed = prev.filter((t) => !nextKeys.has(getWatchedTargetKey(t)));
		const added = nextTargets.filter((t) => !prevKeys.has(getWatchedTargetKey(t)));
		for (const target of removed) {
			const key = getWatchedTargetKey(target);
			const remoteUnsubscribe = remoteWatchUnsubsRef.current.get(key);
			if (remoteUnsubscribe) {
				remoteUnsubscribe();
				remoteWatchUnsubsRef.current.delete(key);
			} else window.api.fs.unwatchWorktree({
				worktreePath: target.worktreePath,
				connectionId: target.connectionId
			});
		}
		for (const target of added) {
			if (target.runtimeEnvironmentId) {
				const key = getWatchedTargetKey(target);
				let cancelled = false;
				const pendingUnsubscribe = () => {
					cancelled = true;
				};
				remoteWatchUnsubsRef.current.set(key, pendingUnsubscribe);
				subscribeRuntimeFileChanges({
					settings: { activeRuntimeEnvironmentId: target.runtimeEnvironmentId },
					worktreeId: target.worktreeId,
					worktreePath: target.worktreePath,
					connectionId: target.connectionId
				}, (payload) => fsChangedHandlerRef.current?.(payload, target.runtimeEnvironmentId), (err) => warnExternalWatchFailure(target, err)).then((unsubscribe) => {
					if (cancelled) {
						unsubscribe();
						return;
					}
					if (remoteWatchUnsubsRef.current.get(key) === pendingUnsubscribe) remoteWatchUnsubsRef.current.set(key, unsubscribe);
					else unsubscribe();
				}).catch((err) => {
					if (remoteWatchUnsubsRef.current.get(key) === pendingUnsubscribe) remoteWatchUnsubsRef.current.delete(key);
					warnExternalWatchFailure(target, err);
				});
				continue;
			}
			window.api.fs.watchWorktree({
				worktreePath: target.worktreePath,
				connectionId: target.connectionId
			}).catch((err) => {
				warnExternalWatchFailure(target, err);
			});
		}
		targetsRef.current = nextTargets;
	}, [targetsKey]);
	(0, import_react.useEffect)(() => {
		const remoteWatchUnsubs = remoteWatchUnsubsRef.current;
		const { handleFsChanged, dispose } = createExternalWatchEventHandler((worktreePath, runtimeEnvironmentId) => targetsRef.current.find((t) => normalizeRuntimePathForComparison(t.worktreePath) === normalizeRuntimePathForComparison(worktreePath) && t.runtimeEnvironmentId === runtimeEnvironmentId));
		const unsubscribe = window.api.fs.onFsChanged((payload) => handleFsChanged(payload, null));
		fsChangedHandlerRef.current = handleFsChanged;
		return () => {
			unsubscribe();
			dispose();
			fsChangedHandlerRef.current = null;
			for (const target of targetsRef.current) {
				const key = getWatchedTargetKey(target);
				const remoteUnsubscribe = remoteWatchUnsubs.get(key);
				if (remoteUnsubscribe) remoteUnsubscribe();
				else window.api.fs.unwatchWorktree({
					worktreePath: target.worktreePath,
					connectionId: target.connectionId
				});
			}
			remoteWatchUnsubs.clear();
			targetsRef.current = [];
		};
	}, []);
}
function createExternalWatchEventHandler(findTarget) {
	const pendingDeletes = /* @__PURE__ */ new Map();
	const pendingKey = (worktreeId, runtimeEnvironmentId, absolutePath) => `${worktreeId}::${runtimeEnvironmentId ?? "client"}::${absolutePath}`;
	const handleFsChanged = (payload, runtimeEnvironmentId = null) => {
		const target = findTarget(payload.worktreePath, runtimeEnvironmentId);
		if (!target) return;
		if (typeof window !== "undefined" && typeof window.dispatchEvent === "function") window.dispatchEvent(new CustomEvent(ORCA_WORKTREE_FILE_CHANGE_EVENT, { detail: {
			payload,
			runtimeEnvironmentId: target.runtimeEnvironmentId
		} }));
		const openFilesAtStart = useAppStore.getState().openFiles;
		const batchPaths = indexEditorExternalWatchBatchPaths(payload, openFilesAtStart, {
			worktreeId: target.worktreeId,
			worktreePath: target.worktreePath,
			runtimeEnvironmentId: target.runtimeEnvironmentId,
			...localWslAliasOption(target)
		});
		const createOrUpdatePaths = batchPaths.createOrUpdatePaths;
		for (const createdPath of createOrUpdatePaths.keys()) {
			const key = pendingKey(target.worktreeId, target.runtimeEnvironmentId, createdPath);
			const existing = pendingDeletes.get(key);
			if (existing) {
				clearTimeout(existing.timer);
				pendingDeletes.delete(key);
			}
		}
		const deletedOpenEditorsRaw = batchPaths.deletedOpenEditors;
		const deletedOpenEditors = hasActiveEditorPathMoves() ? deletedOpenEditorsRaw.filter(({ file }) => !isActiveMoveSourcePath(target.worktreeId, target.runtimeEnvironmentId, file.filePath)) : deletedOpenEditorsRaw;
		const deletedOpenEditorIds = deletedOpenEditors.map(({ file }) => file.id);
		const hasPairedCreate = deletedOpenEditorIds.length > 0 && hasRenameCorrelatedCreate(payload, target.worktreeId, deletedOpenEditorIds, openFilesAtStart);
		if (deletedOpenEditorIds.length > 0) if (hasPairedCreate) {
			const setExternalMutation = useAppStore.getState().setExternalMutation;
			for (const fileId of deletedOpenEditorIds) setExternalMutation(fileId, "renamed");
		} else for (const { file, normalizedDeletePath } of deletedOpenEditors) {
			const fileId = file.id;
			const absolutePath = normalizedDeletePath;
			const key = pendingKey(target.worktreeId, target.runtimeEnvironmentId, absolutePath);
			const existing = pendingDeletes.get(key);
			if (existing) {
				clearTimeout(existing.timer);
				pendingDeletes.delete(key);
			}
			const timer = setTimeout(() => {
				pendingDeletes.delete(key);
				const state = useAppStore.getState();
				if (state.openFiles.some((f) => f.id === fileId && f.mode === "edit")) state.setExternalMutation(fileId, "deleted");
			}, EXTERNAL_MUTATION_DEBOUNCE_MS);
			pendingDeletes.set(key, {
				fileId,
				timer
			});
		}
		if (createOrUpdatePaths.size > 0) {
			const state = useAppStore.getState();
			for (const file of state.openFiles) if (file.worktreeId === target.worktreeId && openFileRuntimeOwner(file) === target.runtimeEnvironmentId && (file.mode === "edit" || file.mode === "markdown-preview") && (file.externalMutation === "deleted" || file.externalMutation === "renamed") && batchPaths.matchesCreateOrUpdate(file)) state.setExternalMutation(file.id, null);
		}
		let overflowed = false;
		for (const evt of payload.events) if (evt.kind === "overflow") {
			for (const notification of getOverflowExternalReloadTargets(target)) scheduleDebouncedExternalReload(notification);
			overflowed = true;
			break;
		}
		if (overflowed || batchPaths.changes.length === 0) return;
		for (const change of batchPaths.changes) {
			const relativePath = change.relativePath;
			const matching = batchPaths.matchingOpenFiles(change);
			const notification = {
				worktreeId: target.worktreeId,
				worktreePath: target.worktreePath,
				relativePath,
				runtimeEnvironmentId: target.runtimeEnvironmentId,
				...localWslAliasOption(target)
			};
			Object.defineProperty(notification, "indexedOpenFiles", { value: { matches: (openFiles) => batchPaths.matchingOpenFiles(change, openFiles) } });
			const absolutePath = change.absolutePath;
			if (matching.length === 0) {
				if (batchPaths.hasCombinedDiffConsumer) scheduleDebouncedExternalReload(notification);
				continue;
			}
			const dirtyMatches = matching.filter((f) => f.isDirty);
			if (dirtyMatches.length > 0) {
				const dirtyIds = dirtyMatches.filter((f) => canAutoSaveOpenFile(f)).map((f) => f.id);
				let isSelfMoveEcho = false;
				if (dirtyMatches.some((f) => f.pendingSelfMoveEcho)) {
					const normalizedAbsolutePath = normalizeRuntimePathForComparison(absolutePath);
					isSelfMoveEcho = dirtyMatches.some((f) => f.pendingSelfMoveEcho && normalizeRuntimePathForComparison(f.pendingSelfMoveEcho.targetPath) === normalizedAbsolutePath);
				}
				if (isSelfMoveEcho) scheduleSelfMoveEchoVerification(target, dirtyIds, true);
				else scheduleChangedOnDiskMark(target, notification, dirtyIds);
				if (dirtyMatches.length === matching.length) {
					if (batchPaths.hasCombinedDiffConsumer) scheduleDebouncedExternalReload(notification);
					continue;
				}
			}
			const recentSelfWrite = getRecentSelfWrite(absolutePath, target.runtimeEnvironmentId);
			if (recentSelfWrite) {
				scheduleSelfWriteAwareExternalReload(target, notification, matching[0], recentSelfWrite);
				continue;
			}
			scheduleDebouncedExternalReload(notification);
		}
	};
	const dispose = () => {
		for (const pending of pendingDeletes.values()) clearTimeout(pending.timer);
		pendingDeletes.clear();
	};
	return {
		handleFsChanged,
		dispose
	};
}
var inFlightEchoVerificationReads = /* @__PURE__ */ new Map();
function readFileForEchoVerification(args) {
	const key = [
		args.runtimeEnvironmentId ?? "",
		args.connectionId ?? "",
		args.expectedExternalSshTargetId ?? "",
		args.filePath
	].join("::");
	let pending = inFlightEchoVerificationReads.get(key);
	if (!pending) {
		pending = readRuntimeFileContent({
			settings: args.runtimeEnvironmentId ? { activeRuntimeEnvironmentId: args.runtimeEnvironmentId } : null,
			filePath: args.filePath,
			relativePath: args.relativePath,
			worktreeId: args.worktreeId ?? void 0,
			connectionId: args.connectionId,
			expectedExternalSshTargetId: args.expectedExternalSshTargetId
		});
		inFlightEchoVerificationReads.set(key, pending);
		const release = () => {
			if (inFlightEchoVerificationReads.get(key) === pending) inFlightEchoVerificationReads.delete(key);
		};
		pending.then(release, release);
	}
	return pending;
}
function markTabsChangedOnDisk(fileIds, connectionId) {
	const state = useAppStore.getState();
	for (const fileId of fileIds) {
		const file = state.openFiles.find((f) => f.id === fileId);
		if (file) markFileChangedOnDisk(state, file, {
			connectionId,
			origin: "live"
		});
	}
}
function scheduleChangedOnDiskMark(target, notification, fileIds) {
	if (fileIds.length === 0) return;
	const absolutePath = joinPath(notification.worktreePath, notification.relativePath);
	const recentSelfWrite = getRecentSelfWrite(absolutePath, target.runtimeEnvironmentId);
	if (!recentSelfWrite || recentSelfWrite.content === null) {
		markTabsChangedOnDisk(fileIds, target.connectionId);
		return;
	}
	readFileForEchoVerification({
		runtimeEnvironmentId: target.runtimeEnvironmentId,
		filePath: absolutePath,
		relativePath: notification.relativePath,
		worktreeId: notification.worktreeId,
		connectionId: target.connectionId
	}).then((result) => {
		if (result.isBinary || result.content !== recentSelfWrite.content) markTabsChangedOnDisk(fileIds, target.connectionId);
	}).catch(() => {
		markTabsChangedOnDisk(fileIds, target.connectionId);
	});
}
var liveMoveVerifyGeneration = /* @__PURE__ */ new Map();
var liveMoveVerifyCounter = 0;
function resolveLiveMoveVerification(candidate, diskSignature, connectionId, consumeProvenance) {
	const { fileId, baseline, generation, operationId } = candidate;
	if (liveMoveVerifyGeneration.get(fileId) !== generation) return;
	liveMoveVerifyGeneration.delete(fileId);
	const state = useAppStore.getState();
	state.setPendingLiveDiskVerification(fileId, false);
	const file = state.openFiles.find((f) => f.id === fileId);
	if (!file || !file.isDirty || file.externalMutation === "changed" || file.lastKnownDiskSignature !== baseline || operationId !== void 0 && file.pendingSelfMoveEcho?.operationId !== operationId) return;
	if (consumeProvenance) state.clearSelfMoveEcho(fileId);
	if (!(baseline !== void 0 && diskSignature === baseline)) markFileChangedOnDisk(state, file, {
		connectionId,
		origin: "live"
	});
}
function verifyLatchedMoveDestinations(worktreePath, connectionId, fileIds) {
	const state = useAppStore.getState();
	const gated = fileIds.filter((id) => state.openFiles.find((f) => f.id === id)?.pendingSelfMoveEcho);
	if (gated.length === 0) return;
	scheduleSelfMoveEchoVerification({
		worktreeId: "",
		worktreePath,
		connectionId,
		runtimeEnvironmentId: null
	}, gated, false);
}
function scheduleSelfMoveEchoVerification(target, fileIds, consumeProvenance) {
	if (fileIds.length === 0) return;
	const state = useAppStore.getState();
	for (const fileId of fileIds) {
		const file = state.openFiles.find((f) => f.id === fileId);
		if (!file || !file.isDirty || file.externalMutation === "changed") continue;
		const generation = ++liveMoveVerifyCounter;
		liveMoveVerifyGeneration.set(fileId, generation);
		state.setPendingLiveDiskVerification(fileId, true);
		const candidate = {
			fileId,
			baseline: file.lastKnownDiskSignature,
			generation,
			operationId: file.pendingSelfMoveEcho?.operationId
		};
		readFileForEchoVerification({
			runtimeEnvironmentId: file.runtimeEnvironmentId?.trim() || target.runtimeEnvironmentId,
			filePath: file.filePath,
			relativePath: file.relativePath,
			worktreeId: file.worktreeId,
			connectionId: target.connectionId,
			expectedExternalSshTargetId: file.externalSshTargetId
		}).then((result) => {
			resolveLiveMoveVerification(candidate, result.isBinary ? null : getDiskBaselineSignature(result.content), target.connectionId, consumeProvenance);
		}).catch(() => resolveLiveMoveVerification(candidate, null, target.connectionId, consumeProvenance));
	}
}
function scheduleSelfWriteAwareExternalReload(target, notification, file, recentSelfWrite) {
	if (recentSelfWrite.content === null) {
		scheduleDebouncedExternalReload(notification);
		return;
	}
	const runtimeEnvironmentId = file.runtimeEnvironmentId ?? target.runtimeEnvironmentId;
	readFileForEchoVerification({
		runtimeEnvironmentId,
		filePath: file.filePath,
		relativePath: file.relativePath,
		worktreeId: file.worktreeId,
		connectionId: target.connectionId,
		expectedExternalSshTargetId: file.externalSshTargetId
	}).then((result) => {
		if ((result.isBinary || result.content !== recentSelfWrite.content) && hasCleanExternalReloadTarget(notification)) {
			clearSelfWrite(file.filePath, runtimeEnvironmentId);
			scheduleDebouncedExternalReload(notification);
		}
	}).catch(() => {
		if (hasCleanExternalReloadTarget(notification)) {
			clearSelfWrite(file.filePath, runtimeEnvironmentId);
			scheduleDebouncedExternalReload(notification);
		}
	});
}
function hasCleanExternalReloadTarget(notification) {
	return getOpenFilesForExternalFileChange(useAppStore.getState().openFiles, notification).some((file) => !file.isDirty);
}
function getOverflowExternalReloadTargets(target) {
	const state = useAppStore.getState();
	const notifications = [];
	for (const file of state.openFiles) {
		if (file.worktreeId !== target.worktreeId || openFileRuntimeOwner(file) !== (target.runtimeEnvironmentId ?? null) || !isExternalReloadableEditorTab(file) || file.isDirty) continue;
		if (file.externalMutation) state.setExternalMutation(file.id, null);
		notifications.push({
			worktreeId: target.worktreeId,
			worktreePath: target.worktreePath,
			relativePath: file.relativePath,
			runtimeEnvironmentId: target.runtimeEnvironmentId ?? null,
			...localWslAliasOption({ allowLocalWindowsWslAliases: target.allowLocalWindowsWslAliases })
		});
	}
	return notifications;
}
function hasRenameCorrelatedCreate(payload, worktreeId, deletedOpenEditorIds, openFiles) {
	if (deletedOpenEditorIds.length === 0) return false;
	const deletedIdSet = new Set(deletedOpenEditorIds);
	const deletedBasenames = /* @__PURE__ */ new Set();
	for (const file of openFiles) {
		if (file.worktreeId !== worktreeId || file.mode !== "edit" && file.mode !== "markdown-preview") continue;
		if (!deletedIdSet.has(file.id)) continue;
		deletedBasenames.add(basename(file.filePath));
	}
	if (deletedBasenames.size === 0) return false;
	for (const evt of payload.events) {
		if (evt.kind !== "create" || evt.isDirectory === true) continue;
		if (deletedBasenames.has(basename(evt.absolutePath))) return true;
	}
	return false;
}
export { ORCA_WORKTREE_FILE_CHANGE_EVENT as _, markFileChangedOnDisk as a, settleEditorPathMove as c, hasRecentSelfWrite as d, recordSelfWrite as f, fileExplorerRefreshConcurrency as g, isPathEqualOrDescendant as h, getDiskBaselineSignature as i, SELF_WRITE_REMOTE_TTL_MS as l, getRevealAncestorDirs as m, verifyLatchedMoveDestinations as n, trackExternalChangeConflictAction as o, useFileExplorerWatch as p, getDiffContentSignature as r, beginEditorPathMove as s, useEditorExternalWatch as t, clearSelfWrite as u };
