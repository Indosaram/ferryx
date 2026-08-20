import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { Ip as isWindowsAbsolutePathLike, Vu as createBrowserUuid, nt as cancelRuntimeFileList, t as useAppStore, ut as listRuntimeFiles } from "./store-CgXrfmaH.js";
import { t as useShallow } from "./shallow-BpOhx1Gc.js";
import { _ as useWorktreesForRepo } from "./selectors-XOBeaOSb.js";
import { a as getFileExplorerOwnerUnresolvedMessage, i as getFileExplorerOperationRoute, r as getFileExplorerOperationOwnerFromState } from "./file-explorer-operation-owner-C4AAHFB5.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
function cleanRuntimeFileListError(error) {
	return (error instanceof Error ? error.message : String(error)).replace(/^Error invoking remote method '[^']+':\s*Error:\s*/, "");
}
function isNestedWorktreePath(parentPath, childPath) {
	const windowsPath = isWindowsAbsolutePathLike(parentPath);
	const parent = parentPath.replace(/[\\/]+$/, "").replace(/\\/g, "/");
	const child = childPath.replace(/\\/g, "/");
	const comparableParent = windowsPath ? parent.toLowerCase() : parent;
	return (windowsPath ? child.toLowerCase() : child).startsWith(`${comparableParent}/`);
}
function getNestedWorktreeExcludePaths(worktreeId, worktreePath, repoWorktrees) {
	return repoWorktrees.filter((worktree) => worktree.id !== worktreeId && isNestedWorktreePath(worktreePath, worktree.path)).map((worktree) => worktree.path).sort();
}
function getRuntimeFileListTarget(worktreeId, worktreePath, repoWorktrees) {
	const resolvedWorktreePath = worktreePath ?? null;
	if (!worktreeId || !resolvedWorktreePath) return {
		canList: false,
		excludeRequest: {
			paths: [],
			key: "[]"
		},
		worktreePath: null
	};
	return {
		canList: true,
		excludeRequest: getNestedWorktreeExcludeRequest(worktreeId, resolvedWorktreePath, repoWorktrees),
		worktreePath: resolvedWorktreePath
	};
}
function getNestedWorktreeExcludeRequest(worktreeId, worktreePath, repoWorktrees) {
	if (!worktreeId || !worktreePath || repoWorktrees.length === 0) return {
		paths: [],
		key: "[]"
	};
	const paths = getNestedWorktreeExcludePaths(worktreeId, worktreePath, repoWorktrees);
	return {
		paths,
		key: JSON.stringify(paths)
	};
}
function useRuntimeFileListForWorktree({ enabled, worktreeId }) {
	const worktree = useAppStore((state) => worktreeId ? state.getKnownWorktreeById(worktreeId) ?? null : null);
	const worktreePath = worktree?.path ?? null;
	const repoWorktrees = useWorktreesForRepo(worktree?.repoId ?? null);
	const [files, setFiles] = (0, import_react.useState)([]);
	const [loading, setLoading] = (0, import_react.useState)(false);
	const [loadError, setLoadError] = (0, import_react.useState)(null);
	const [listedOperationOwner, setListedOperationOwner] = (0, import_react.useState)({ kind: "unresolved" });
	const lastRequestKeyRef = (0, import_react.useRef)("");
	const target = (0, import_react.useMemo)(() => getRuntimeFileListTarget(worktreeId, worktreePath, repoWorktrees), [
		repoWorktrees,
		worktreeId,
		worktreePath
	]);
	const { excludeRequest } = target;
	const operationOwnerState = useAppStore(useShallow((state) => ({
		settings: state.settings,
		repos: state.repos,
		worktreesByRepo: state.worktreesByRepo,
		detectedWorktreesByRepo: state.detectedWorktreesByRepo,
		folderWorkspaces: state.folderWorkspaces,
		projectGroups: state.projectGroups,
		restoredRuntimeHostIdByWorkspaceSessionKey: state.restoredRuntimeHostIdByWorkspaceSessionKey
	})));
	const operationOwner = (0, import_react.useMemo)(() => getFileExplorerOperationOwnerFromState(operationOwnerState, worktreeId), [operationOwnerState, worktreeId]);
	const operationOwnerKey = JSON.stringify(operationOwner);
	const operationOwnerRef = (0, import_react.useRef)(operationOwner);
	operationOwnerRef.current = operationOwner;
	const operationRoute = getFileExplorerOperationRoute(operationOwner);
	const operationRouteAvailable = operationRoute !== null;
	const connectionId = operationRoute?.connectionId;
	const runtimeEnvironmentId = operationRoute?.settings.activeRuntimeEnvironmentId ?? null;
	const activeTargetStatus = useAppStore((state) => connectionId ? state.sshConnectionStates.get(connectionId)?.status : void 0);
	const connectionPending = activeTargetStatus === "connecting" || activeTargetStatus === "deploying-relay" || activeTargetStatus === "reconnecting";
	const requestKey = (0, import_react.useMemo)(() => `${worktreePath ?? ""}\n${operationOwnerKey}\n${excludeRequest.key}\n${activeTargetStatus ?? ""}`, [
		activeTargetStatus,
		excludeRequest.key,
		operationOwnerKey,
		worktreePath
	]);
	(0, import_react.useEffect)(() => {
		if (!enabled) {
			setLoading(false);
			setListedOperationOwner({ kind: "unresolved" });
			return;
		}
		if (!target.canList || !worktreeId || !worktreePath || !operationRouteAvailable) {
			setFiles([]);
			setListedOperationOwner({ kind: "unresolved" });
			setLoadError(operationRouteAvailable ? null : getFileExplorerOwnerUnresolvedMessage());
			setLoading(false);
			return;
		}
		let cancelled = false;
		if (lastRequestKeyRef.current !== requestKey) setFiles([]);
		lastRequestKeyRef.current = requestKey;
		setLoadError(null);
		setLoading(true);
		const excludePaths = excludeRequest.paths.length > 0 ? excludeRequest.paths : void 0;
		const requestToken = createBrowserUuid();
		const requestOperationOwner = operationOwnerRef.current;
		const requestContext = {
			settings: { activeRuntimeEnvironmentId: runtimeEnvironmentId },
			worktreeId,
			worktreePath,
			connectionId
		};
		listRuntimeFiles(requestContext, {
			rootPath: worktreePath,
			excludePaths,
			requestToken
		}).then((result) => {
			if (!cancelled) {
				setFiles(result);
				setListedOperationOwner(requestOperationOwner);
			}
		}).catch((error) => {
			if (!cancelled) {
				setFiles([]);
				setLoadError(cleanRuntimeFileListError(error));
			}
		}).finally(() => {
			if (!cancelled) setLoading(false);
		});
		return () => {
			cancelled = true;
			cancelRuntimeFileList(requestContext, requestToken);
		};
	}, [
		enabled,
		excludeRequest,
		connectionId,
		operationOwnerKey,
		operationRouteAvailable,
		requestKey,
		runtimeEnvironmentId,
		target.canList,
		worktreeId,
		worktreePath
	]);
	return {
		files,
		loading: loading || connectionPending,
		loadError,
		operationOwner: listedOperationOwner
	};
}
export { useRuntimeFileListForWorktree as t };
