import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import "./jsx-runtime-Cv_nyRjc.js";
import "./button-DszXJEV6.js";
import { t as useAppStore } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./useMountedRef-1omUd-IV.js";
import "./web-runtime-session-CN2syA39.js";
import "./agent-paste-draft-C2PA7vXu.js";
import "./agent-process-recognition-BB0O3DaN.js";
import "./terminal-pty-input-transaction-2UskR-Bm.js";
import "./pane-agent-owner-BPfoVAtS.js";
import "./native-chat-session-option-cache-DGE3h47U.js";
import "./github-links-C1M8w9wX.js";
import "./connection-context-BUPsamzR.js";
import { t as runSleepWorktree } from "./sleep-worktree-flow-83wDDapJ.js";
import { t as activateTabAndFocusPane } from "./activate-tab-and-focus-pane-dvS5VCkm.js";
import "./agent-status-connection-ownership-D5nXPHBo.js";
import "./worktree-agent-rows-C1pW_DbE.js";
import "./worktree-title-derived-agent-rows-xbcpjeY8.js";
import "./agent-row-conversation-name-DXwI1NP0.js";
import "./launch-agent-in-new-tab-44JGNfKl.js";
import "./terminal-keyboard-protocol-De0UZ6qG.js";
import { t as buildDashboardSnapshot } from "./build-dashboard-snapshot-B3HmQPph.js";
import { t as launchDashboardAgent } from "./launch-dashboard-agent-B64ro2Tz.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var PUBLISH_THROTTLE_MS = 250;
function repoIconsUnchanged(next, previous) {
	if (!previous) return false;
	const nextIds = Object.keys(next);
	if (nextIds.length !== Object.keys(previous).length) return false;
	return nextIds.every((id) => id in previous && next[id] === previous[id]);
}
function dashboardSnapshotInputsChanged(state, previousState) {
	return state.repos !== previousState.repos || state.worktreesByRepo !== previousState.worktreesByRepo || state.tabsByWorktree !== previousState.tabsByWorktree || state.retainedAgentsByPaneKey !== previousState.retainedAgentsByPaneKey || state.migrationUnsupportedByPtyId !== previousState.migrationUnsupportedByPtyId || state.runtimeAgentOrchestrationByPaneKey !== previousState.runtimeAgentOrchestrationByPaneKey || state.terminalLayoutsByTabId !== previousState.terminalLayoutsByTabId || state.ptyIdsByTabId !== previousState.ptyIdsByTabId || state.runtimePaneTitlesByTabId !== previousState.runtimePaneTitlesByTabId || state.acknowledgedAgentsByPaneKey !== previousState.acknowledgedAgentsByPaneKey || state.hostedReviewCache !== previousState.hostedReviewCache || state.prCache !== previousState.prCache || state.settings !== previousState.settings || state.workspaceStatuses !== previousState.workspaceStatuses || state.detectedAgentIds !== previousState.detectedAgentIds || state.remoteDetectedAgentIds !== previousState.remoteDetectedAgentIds || state.runtimeDetectedAgentIds !== previousState.runtimeDetectedAgentIds || state.sshConnectionStates !== previousState.sshConnectionStates || state.sshStateByEnvironment !== previousState.sshStateByEnvironment || state.runtimeStatusByEnvironmentId !== previousState.runtimeStatusByEnvironmentId || state.paneForegroundAgentByPaneKey !== previousState.paneForegroundAgentByPaneKey || state.detectedWorktreesByRepo !== previousState.detectedWorktreesByRepo || state.folderWorkspaces !== previousState.folderWorkspaces || state.projectGroups !== previousState.projectGroups || state.sshTargetLabels !== previousState.sshTargetLabels || state.restoredRuntimeHostIdByWorkspaceSessionKey !== previousState.restoredRuntimeHostIdByWorkspaceSessionKey || state.runtimeEnvironments !== previousState.runtimeEnvironments || state.runtimeEnvironmentCatalogHydrated !== previousState.runtimeEnvironmentCatalogHydrated || state.removedRuntimeEnvironmentIds !== previousState.removedRuntimeEnvironmentIds;
}
function watchSnapshotInputs(onChanged) {
	return useAppStore.subscribe((state, previousState) => {
		if (dashboardSnapshotInputsChanged(state, previousState)) onChanged();
	});
}
function useDashboardPopoutBridge(enabled) {
	(0, import_react.useEffect)(() => {
		if (!enabled) return;
		return window.api.dashboard.onSpawnAgent?.(launchDashboardAgent);
	}, [enabled]);
	(0, import_react.useEffect)(() => {
		if (!enabled) return;
		return window.api.dashboard.onSleepWorkspace?.(({ worktreeId }) => {
			runSleepWorktree(worktreeId);
		});
	}, [enabled]);
	(0, import_react.useEffect)(() => {
		if (!enabled) return;
		return window.api.dashboard.onRevealAgent((args) => {
			useAppStore.getState().setActiveWorktree(args.worktreeId, args.executionHostId);
			activateTabAndFocusPane(args.tabId, args.leafId, { flashFocusedPane: true });
		});
	}, [enabled]);
	(0, import_react.useEffect)(() => {
		if (!enabled) return;
		return window.api.dashboard.onAckAgent?.((paneKey) => {
			useAppStore.getState().acknowledgeAgents([paneKey]);
		});
	}, [enabled]);
	(0, import_react.useEffect)(() => {
		if (!enabled) return;
		let open = false;
		let disposed = false;
		let unsubscribeStore = null;
		let trailingTimer = null;
		let lastPublishAt = 0;
		let lastPublishedRepoIcons = null;
		const publishNow = (withIcons) => {
			lastPublishAt = Date.now();
			const snapshot = buildDashboardSnapshot(useAppStore.getState(), lastPublishAt);
			const icons = snapshot.repoIconsByRepoId ?? {};
			if (!withIcons && repoIconsUnchanged(icons, lastPublishedRepoIcons)) {
				const { repoIconsByRepoId: _omitted, ...withoutIcons } = snapshot;
				window.api.dashboard.publishSnapshot(withoutIcons);
				return;
			}
			lastPublishedRepoIcons = icons;
			window.api.dashboard.publishSnapshot(snapshot);
		};
		const publishThrottled = () => {
			if (!open || disposed) return;
			const elapsed = Date.now() - lastPublishAt;
			if (elapsed >= PUBLISH_THROTTLE_MS) {
				if (trailingTimer) {
					clearTimeout(trailingTimer);
					trailingTimer = null;
				}
				publishNow(false);
				return;
			}
			if (!trailingTimer) trailingTimer = setTimeout(() => {
				trailingTimer = null;
				if (open && !disposed) publishNow(false);
			}, PUBLISH_THROTTLE_MS - elapsed);
		};
		const setOpen = (next) => {
			if (next === open || disposed) return;
			open = next;
			if (open) {
				if (!unsubscribeStore) unsubscribeStore = watchSnapshotInputs(publishThrottled);
				publishNow(true);
			} else {
				unsubscribeStore?.();
				unsubscribeStore = null;
				if (trailingTimer) {
					clearTimeout(trailingTimer);
					trailingTimer = null;
				}
			}
		};
		const offOpenChanged = window.api.dashboard.onPopoutOpenChanged((next) => setOpen(next));
		const offRequested = window.api.dashboard.onSnapshotRequested(() => {
			if (open) publishNow(true);
		});
		window.api.dashboard.getPopoutOpen().then((isOpen) => {
			if (!disposed && isOpen) setOpen(true);
		});
		return () => {
			disposed = true;
			offOpenChanged?.();
			offRequested?.();
			unsubscribeStore?.();
			if (trailingTimer) clearTimeout(trailingTimer);
		};
	}, [enabled]);
}
function DashboardPopoutBridge() {
	useDashboardPopoutBridge(true);
	return null;
}
export { DashboardPopoutBridge as default };
