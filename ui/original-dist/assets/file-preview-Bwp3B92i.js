import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { Cu as findSiblingGroupId, Ht as getConnectionIdForFileFromState, O as getClientCreationActionPolicy, Ou as getRuntimeEnvironmentIdForWorktree, Zt as absolutePathToFileUri, t as useAppStore } from "./store-CgXrfmaH.js";
import { n as toast } from "./dist-DgqligFk.js";
import { t as useShallow } from "./shallow-BpOhx1Gc.js";
import { l as createWebRuntimeSessionBrowserTab } from "./web-runtime-session-CN2syA39.js";
import { n as getConnectionIdForFile } from "./connection-context-BUPsamzR.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
const REMOTE_FILE_BROWSER_UNSUPPORTED_MESSAGE = "Open in Orca Browser is only available for local files.";
var FILE_BROWSER_OPEN_FAILED_MESSAGE = "Unable to open this file in Orca Browser.";
function getWorkspaceFileBrowserActionMode(state, worktreeId) {
	const availability = getClientCreationActionPolicy(state, worktreeId)["managed-browser"];
	if (availability.state !== "enabled") return null;
	return getRuntimeEnvironmentIdForWorktree(state, worktreeId) ? availability.provider === "paired-runtime" ? "paired-runtime" : null : "local-client";
}
function canShowWorkspaceFileBrowserAction(state, worktreeId, filePath) {
	return getWorkspaceFileBrowserActionMode(state, worktreeId) !== null && getConnectionIdForFileFromState(state, worktreeId, filePath) === null;
}
function useWorkspaceFileBrowserActionPredicate(worktreeId) {
	const inputs = useAppStore(useShallow((state) => ({
		mode: worktreeId ? getWorkspaceFileBrowserActionMode(state, worktreeId) : null,
		folderWorkspaces: state.folderWorkspaces,
		projectGroups: state.projectGroups,
		repos: state.repos,
		worktreesByRepo: state.worktreesByRepo
	})));
	return (0, import_react.useCallback)((filePath) => inputs.mode !== null && getConnectionIdForFileFromState(inputs, worktreeId, filePath) === null, [inputs, worktreeId]);
}
function reportRemoteFileBrowserOpen(result) {
	result.then((created) => {
		if (!created) toast.error(FILE_BROWSER_OPEN_FAILED_MESSAGE);
	}).catch(() => {
		toast.error(FILE_BROWSER_OPEN_FAILED_MESSAGE);
	});
}
function getWorkspaceFileBrowserOpenTarget(params) {
	if (getConnectionIdForFile(params.worktreeId, params.filePath) !== null) return {
		status: "unsupported",
		reason: "remote-worktree",
		message: REMOTE_FILE_BROWSER_UNSUPPORTED_MESSAGE
	};
	return {
		status: "ready",
		url: absolutePathToFileUri(params.filePath),
		title: params.filePath.split(/[/\\]/).pop() ?? params.filePath
	};
}
function openFileInBrowserTab(params) {
	const target = getWorkspaceFileBrowserOpenTarget(params);
	if (target.status === "unsupported") return target;
	const state = useAppStore.getState();
	const browserAvailability = getClientCreationActionPolicy(state, params.worktreeId)["managed-browser"];
	if (browserAvailability.state !== "enabled") {
		toast.error(browserAvailability.reason);
		return target;
	}
	const environmentId = getRuntimeEnvironmentIdForWorktree(state, params.worktreeId);
	if (environmentId) {
		if (browserAvailability.provider !== "paired-runtime") {
			toast.error(FILE_BROWSER_OPEN_FAILED_MESSAGE);
			return target;
		}
		reportRemoteFileBrowserOpen(createWebRuntimeSessionBrowserTab({
			worktreeId: params.worktreeId,
			environmentId,
			url: target.url,
			stagedTitle: target.title,
			stagedFocusAddressBar: false
		}));
		return target;
	}
	state.createBrowserTab(params.worktreeId, target.url, {
		title: target.title,
		activate: true
	});
	return target;
}
function canPreviewLanguage(language) {
	return language === "html";
}
function openFilePreviewToSide(params) {
	if (!canPreviewLanguage(params.language)) return;
	const state = useAppStore.getState();
	const worktreeId = params.worktreeId;
	const target = getWorkspaceFileBrowserOpenTarget({
		filePath: params.filePath,
		worktreeId
	});
	if (target.status === "unsupported") {
		toast.error(target.message);
		return;
	}
	const browserAvailability = getClientCreationActionPolicy(state, worktreeId)["managed-browser"];
	if (browserAvailability.state !== "enabled") {
		toast.error(browserAvailability.reason);
		return;
	}
	const environmentId = getRuntimeEnvironmentIdForWorktree(state, worktreeId);
	if (environmentId && browserAvailability.provider !== "paired-runtime") {
		toast.error(FILE_BROWSER_OPEN_FAILED_MESSAGE);
		return;
	}
	const sourceGroupId = params.sourceGroupId ?? state.activeGroupIdByWorktree[worktreeId] ?? state.groupsByWorktree[worktreeId]?.[0]?.id ?? null;
	if (!sourceGroupId) return;
	const layout = state.layoutByWorktree[worktreeId] ?? null;
	const existingSibling = layout ? findSiblingGroupId(layout, sourceGroupId) : null;
	let targetGroupId = existingSibling;
	if (!targetGroupId) targetGroupId = state.createEmptySplitGroup(worktreeId, sourceGroupId, "right");
	if (!targetGroupId) return;
	if (environmentId) {
		reportRemoteFileBrowserOpen(createWebRuntimeSessionBrowserTab({
			worktreeId,
			environmentId,
			url: target.url,
			clientTargetGroupId: targetGroupId,
			clientTargetGroupCreated: !existingSibling,
			focusOnCreate: false,
			stagedTitle: target.title,
			stagedFocusAddressBar: false
		}));
		return;
	}
	state.createBrowserTab(worktreeId, target.url, {
		title: target.title,
		targetGroupId,
		activate: true
	});
}
export { openFilePreviewToSide as a, openFileInBrowserTab as i, canShowWorkspaceFileBrowserAction as n, useWorkspaceFileBrowserActionPredicate as o, getWorkspaceFileBrowserOpenTarget as r, canPreviewLanguage as t };
