import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as Button } from "./button-DszXJEV6.js";
import { t as LoaderCircle } from "./loader-circle-CRZpWdsi.js";
import { Qi as getTerminalPtyOwnershipIdentity, Zi as buildTerminalTabRetirementPlans, t as useAppStore } from "./store-CgXrfmaH.js";
import { n as toast } from "./dist-DgqligFk.js";
import { t as useMountedRef } from "./useMountedRef-1omUd-IV.js";
import { t as closeTerminalTab } from "./terminal-tab-actions-BaU95skQ.js";
import { a as DialogFooter, i as DialogDescription, o as DialogHeader, r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
function reserveTerminalRetirementTeardowns(state, plan, scheduledPtyOwners) {
	const cleanupOnlyPtyIds = new Set(plan.cleanupOnlyPtyIds);
	const newlyScheduledPtyOwners = [];
	const reserve = (ptyId) => {
		const owner = getTerminalPtyOwnershipIdentity(state, ptyId, plan.worktreeId);
		if (scheduledPtyOwners.has(owner)) {
			cleanupOnlyPtyIds.add(ptyId);
			return false;
		}
		scheduledPtyOwners.add(owner);
		newlyScheduledPtyOwners.push(owner);
		return true;
	};
	return {
		plan: {
			...plan,
			localOrSshPtyIds: plan.localOrSshPtyIds.filter(reserve),
			runtimeTerminals: plan.runtimeTerminals.filter((terminal) => reserve(terminal.ptyId)),
			cleanupOnlyPtyIds: [...cleanupOnlyPtyIds]
		},
		newlyScheduledPtyOwners
	};
}
var listeners = /* @__PURE__ */ new Set();
function subscribeDaemonSessionInventoryInvalidated(listener) {
	listeners.add(listener);
	return () => {
		listeners.delete(listener);
	};
}
function notifyDaemonSessionInventoryInvalidated() {
	const snapshot = [...listeners];
	for (const listener of snapshot) listener();
}
var CLOSE_BATCH_SIZE = 2;
function snapshotKillAllTerminalSurfaceIds(state = useAppStore.getState()) {
	const targetIds = /* @__PURE__ */ new Set();
	for (const tabs of Object.values(state.tabsByWorktree)) for (const tab of tabs) targetIds.add(tab.id);
	for (const tabs of Object.values(state.unifiedTabsByWorktree)) for (const tab of tabs) if (tab.contentType === "terminal") targetIds.add(tab.entityId);
	return [...targetIds];
}
function getTargetIndex(state, targetIds) {
	const ownerByTargetId = /* @__PURE__ */ new Map();
	const terminalIdsByWorktree = /* @__PURE__ */ new Map();
	for (const [worktreeId, tabs] of Object.entries(state.tabsByWorktree)) {
		const ids = terminalIdsByWorktree.get(worktreeId) ?? /* @__PURE__ */ new Set();
		for (const tab of tabs) {
			ids.add(tab.id);
			if (targetIds.has(tab.id) && !ownerByTargetId.has(tab.id)) ownerByTargetId.set(tab.id, worktreeId);
		}
		terminalIdsByWorktree.set(worktreeId, ids);
	}
	for (const [worktreeId, tabs] of Object.entries(state.unifiedTabsByWorktree)) {
		const ids = terminalIdsByWorktree.get(worktreeId) ?? /* @__PURE__ */ new Set();
		for (const tab of tabs) {
			if (tab.contentType === "terminal") ids.add(tab.entityId);
			if (tab.contentType === "terminal" && targetIds.has(tab.entityId) && !ownerByTargetId.has(tab.entityId)) ownerByTargetId.set(tab.entityId, worktreeId);
		}
		terminalIdsByWorktree.set(worktreeId, ids);
	}
	return {
		ownerByTargetId,
		terminalIdsByWorktree
	};
}
function getNextTerminalId(ids, closingId) {
	for (const id of ids) if (id !== closingId) return id;
	return null;
}
function createDefaultDependencies() {
	return {
		getState: useAppStore.getState,
		killDaemonSessions: () => window.api.pty.management.killAll(),
		notifyInventoryInvalidated: notifyDaemonSessionInventoryInvalidated,
		closeSurface: closeTerminalTab,
		killPty: (ptyId) => window.api.pty.kill(ptyId),
		now: () => globalThis.performance?.now() ?? Date.now(),
		yieldToRenderer: () => new Promise((resolve) => {
			const channel = new MessageChannel();
			channel.port1.onmessage = () => {
				channel.port1.close();
				channel.port2.close();
				resolve();
			};
			channel.port2.postMessage(void 0);
		}),
		reportSummary: (summary) => console.info("[kill-all-terminal-surfaces]", summary)
	};
}
async function runKillAllTerminalSurfaces(snapshotTargetIds, dependencies = {}) {
	const deps = {
		...createDefaultDependencies(),
		...dependencies
	};
	const targetIds = [...new Set(snapshotTargetIds)];
	let daemon;
	try {
		daemon = {
			status: "fulfilled",
			...await deps.killDaemonSessions()
		};
	} catch {
		daemon = { status: "rejected" };
	}
	try {
		deps.notifyInventoryInvalidated();
	} catch {}
	const cleanupState = deps.getState();
	const remainingTargetIds = new Set(targetIds);
	const createCloseWave = (state) => {
		const { ownerByTargetId, terminalIdsByWorktree } = getTargetIndex(state, new Set(remainingTargetIds));
		const presentTargetIds = [...remainingTargetIds].filter((targetId) => ownerByTargetId.has(targetId));
		for (const targetId of remainingTargetIds) if (!ownerByTargetId.has(targetId)) remainingTargetIds.delete(targetId);
		const activeWorktreeId = state.activeWorktreeId;
		const closeOrder = [...presentTargetIds.filter((targetId) => ownerByTargetId.get(targetId) !== activeWorktreeId), ...presentTargetIds.filter((targetId) => ownerByTargetId.get(targetId) === activeWorktreeId)];
		return {
			state,
			ownerByTargetId,
			terminalIdsByWorktree,
			closeOrder,
			retirementPlans: buildTerminalTabRetirementPlans(state, closeOrder)
		};
	};
	let closeWave = createCloseWave(cleanupState);
	const daemonKilledSessionIds = new Set(daemon.status === "fulfilled" ? daemon.killedSessionIds ?? [] : []);
	const scheduledPtyOwners = /* @__PURE__ */ new Set();
	const exactKillTasks = [];
	const attemptedTargetIds = [];
	const failedCloseTargetIds = /* @__PURE__ */ new Set();
	const closeStartedAt = deps.now();
	let closeBatchStartedAt = closeStartedAt;
	let maxCloseBatchDurationMs = 0;
	let closeYieldCount = 0;
	while (closeWave.closeOrder.length > 0) {
		const batchTargetIds = closeWave.closeOrder.splice(0, CLOSE_BATCH_SIZE);
		let mustReplanAfterYield = false;
		for (const targetId of batchTargetIds) {
			remainingTargetIds.delete(targetId);
			attemptedTargetIds.push(targetId);
			const owningWorktreeId = closeWave.ownerByTargetId.get(targetId);
			const remainingTerminalIds = closeWave.terminalIdsByWorktree.get(owningWorktreeId) ?? /* @__PURE__ */ new Set();
			const nextTerminalTabId = getNextTerminalId(remainingTerminalIds, targetId);
			const { plan: retirementPlan, newlyScheduledPtyOwners } = reserveTerminalRetirementTeardowns(closeWave.state, closeWave.retirementPlans.get(targetId), scheduledPtyOwners);
			let closeFailed = false;
			try {
				deps.closeSurface(targetId, {
					force: true,
					localPtyTeardownOwnedExternally: true,
					precomputedRetirementPlan: retirementPlan,
					precomputedCloseState: {
						owningWorktreeId,
						terminalCountBeforeClose: remainingTerminalIds.size,
						nextTerminalTabId
					}
				});
			} catch {
				closeFailed = true;
				failedCloseTargetIds.add(targetId);
			}
			if (closeFailed && snapshotKillAllTerminalSurfaceIds(deps.getState()).includes(targetId)) {
				for (const owner of newlyScheduledPtyOwners) scheduledPtyOwners.delete(owner);
				mustReplanAfterYield = true;
				break;
			}
			remainingTerminalIds.delete(targetId);
			for (const ptyId of retirementPlan.localOrSshPtyIds) {
				if (daemonKilledSessionIds.has(ptyId)) continue;
				try {
					exactKillTasks.push(deps.killPty(ptyId));
				} catch (error) {
					exactKillTasks.push(Promise.reject(error));
				}
			}
		}
		maxCloseBatchDurationMs = Math.max(maxCloseBatchDurationMs, deps.now() - closeBatchStartedAt);
		if (remainingTargetIds.size > 0) {
			const stateAfterBatch = deps.getState();
			try {
				await deps.yieldToRenderer();
			} catch {}
			closeYieldCount += 1;
			closeBatchStartedAt = deps.now();
			const stateAfterYield = deps.getState();
			if (mustReplanAfterYield || stateAfterYield !== stateAfterBatch) closeWave = createCloseWave(stateAfterYield);
		}
	}
	const closeDurationMs = Math.max(0, deps.now() - closeStartedAt);
	const exactKillResults = await Promise.allSettled(exactKillTasks);
	const exactKillAcceptedCount = exactKillResults.filter((result) => result.status === "fulfilled").length;
	const finalTargetIds = new Set(snapshotKillAllTerminalSurfaceIds(deps.getState()));
	const absentTargetCount = targetIds.filter((targetId) => !finalTargetIds.has(targetId)).length;
	const failedCloseAttemptCount = attemptedTargetIds.filter((targetId) => failedCloseTargetIds.has(targetId) || finalTargetIds.has(targetId)).length;
	const summary = {
		targetCount: targetIds.length,
		closeAttemptCount: attemptedTargetIds.length,
		absentTargetCount,
		failedCloseAttemptCount,
		exactKillAcceptedCount,
		exactKillRejectedCount: exactKillResults.length - exactKillAcceptedCount,
		closeDurationMs: Math.round(closeDurationMs * 100) / 100,
		maxCloseBatchDurationMs: Math.round(maxCloseBatchDurationMs * 100) / 100,
		closeYieldCount,
		closePhaseExceededLongTaskBudget: maxCloseBatchDurationMs > 50,
		daemon
	};
	try {
		deps.reportSummary(summary);
	} catch {}
	return summary;
}
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function showKillAllTerminalSurfacesResult(summary) {
	const description = [summary.targetCount > 0 ? translate("auto.components.shared.useDaemonActions.71a8d342b0", "Terminal tabs absent: {{value0}}/{{value1}}. Failed close attempts: {{value2}}. Exact PTY shutdown requests accepted: {{value3}}; failed: {{value4}}.", {
		value0: summary.absentTargetCount,
		value1: summary.targetCount,
		value2: summary.failedCloseAttemptCount,
		value3: summary.exactKillAcceptedCount,
		value4: summary.exactKillRejectedCount
	}) : null, summary.daemon.status === "rejected" ? translate("auto.components.shared.useDaemonActions.2e57c1a940", "The daemon shutdown result is unverified because its management request failed.") : translate("auto.components.shared.useDaemonActions.993af6052c", "Daemon management reported exited: {{value0}}/{{value1}}; still present before exact cleanup: {{value2}}.", {
		value0: summary.daemon.killedCount,
		value1: summary.daemon.killedCount + summary.daemon.remainingCount,
		value2: summary.daemon.remainingCount
	})].filter(Boolean).join(" ");
	if (summary.daemon.status === "rejected") {
		toast.error(translate("auto.components.shared.useDaemonActions.1f0d8ac762", "Terminal cleanup finished with errors."), { description });
		return;
	}
	if (summary.failedCloseAttemptCount > 0 || summary.exactKillRejectedCount > 0) {
		toast.error(translate("auto.components.shared.useDaemonActions.1f0d8ac762", "Terminal cleanup finished with errors."), { description });
		return;
	}
	if (summary.daemon.remainingCount > 0) {
		toast.warning(translate("auto.components.shared.useDaemonActions.80b6ea14cf", "Terminal cleanup finished with warnings."), { description });
		return;
	}
	if (summary.targetCount === 0 && summary.daemon.killedCount === 0) {
		toast.info(translate("auto.components.shared.useDaemonActions.47cd2a50e9", "No sessions or terminal tabs were reported."));
		return;
	}
	toast.success(summary.targetCount > 0 ? translate("auto.components.shared.useDaemonActions.c34fb1098d", "Terminal tabs closed and shutdown requested.") : translate("auto.components.shared.useDaemonActions.d9657ac204", "Terminal session shutdown requested."), { description });
}
function useDaemonActions(callbacks) {
	const [pending, setPending] = (0, import_react.useState)(null);
	const [busyKind, setBusyKind] = (0, import_react.useState)(null);
	const mountedRef = useMountedRef();
	const clearPendingAction = (0, import_react.useCallback)(() => {
		if (!mountedRef.current) return;
		setBusyKind(null);
		setPending(null);
	}, [mountedRef]);
	const runRestart = (0, import_react.useCallback)(async () => {
		setBusyKind("restart");
		try {
			const { success } = await window.api.pty.management.restart();
			if (success) toast.success(translate("auto.components.shared.useDaemonActions.0e9da1b98e", "Daemon restarted."));
			else toast.error(translate("auto.components.shared.useDaemonActions.b5954e12d3", "Restart failed — check logs."));
		} catch (err) {
			toast.error(translate("auto.components.shared.useDaemonActions.d762b41f41", "Restart failed."), { description: err instanceof Error ? err.message : void 0 });
		} finally {
			clearPendingAction();
			if (mountedRef.current) callbacks?.onRestartSettled?.();
		}
	}, [
		callbacks,
		clearPendingAction,
		mountedRef
	]);
	const runKillAll = (0, import_react.useCallback)(async () => {
		const targetSurfaceIds = snapshotKillAllTerminalSurfaceIds();
		setBusyKind("killAll");
		callbacks?.onKillAllStart?.();
		try {
			const summary = await runKillAllTerminalSurfaces(targetSurfaceIds);
			if (summary.daemon.status === "rejected" && mountedRef.current) callbacks?.onKillAllError?.();
			showKillAllTerminalSurfacesResult(summary);
		} catch (err) {
			if (mountedRef.current) callbacks?.onKillAllError?.();
			toast.error(translate("auto.components.shared.useDaemonActions.e8f25bd903", "Couldn’t finish terminal cleanup."), { description: err instanceof Error ? err.message : void 0 });
		} finally {
			clearPendingAction();
			if (mountedRef.current) callbacks?.onKillAllSettled?.();
		}
	}, [
		callbacks,
		clearPendingAction,
		mountedRef
	]);
	const runConfirmed = (0, import_react.useCallback)(() => {
		if (pending === "restart") runRestart();
		else if (pending === "killAll") runKillAll();
	}, [
		pending,
		runRestart,
		runKillAll
	]);
	return {
		pending,
		setPending,
		busyKind,
		isBusy: busyKind !== null,
		runRestart,
		runKillAll,
		runConfirmed
	};
}
function getCopy(kind) {
	if (kind === "restart") return {
		title: translate("auto.components.shared.useDaemonActions.922548bc66", "Restart the terminal daemon?"),
		description: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(import_jsx_runtime.Fragment, { children: translate("auto.components.shared.useDaemonActions.01d6b7c64e", "Kills every running terminal pane and restarts the daemon process. Panes show \"Process exited\" and can be reopened immediately. Legacy-protocol sessions from a previous app version are preserved. This can't be undone.") }),
		confirmLabel: "Restart daemon",
		busyLabel: "Restarting…"
	};
	return {
		title: translate("auto.components.shared.useDaemonActions.1bbea41a77", "Kill all terminal sessions?"),
		description: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(import_jsx_runtime.Fragment, { children: translate("auto.components.shared.useDaemonActions.a702d4196e", "This closes every terminal tab across all workspaces and requests shutdown for its current terminal sessions. Any unsaved terminal work is lost. The daemon itself keeps running, and new terminals can be opened immediately. This can't be undone.") }),
		confirmLabel: "Kill all sessions",
		busyLabel: "Killing…"
	};
}
function DaemonActionDialog({ api, extraDescription }) {
	const { pending, setPending, busyKind, isBusy, runConfirmed } = api;
	const copy = pending ? getCopy(pending) : null;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
		open: pending !== null,
		onOpenChange: (open) => {
			if (open) return;
			if (isBusy) return;
			setPending(null);
		},
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogContent, {
			className: "max-w-md",
			showCloseButton: !isBusy,
			onPointerDownOutside: (e) => {
				if (isBusy) e.preventDefault();
			},
			onEscapeKeyDown: (e) => {
				if (isBusy) e.preventDefault();
			},
			children: copy ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, {
				className: "text-sm",
				children: copy.title
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogDescription, {
				className: "text-xs",
				children: [copy.description, extraDescription ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "mt-2",
					children: extraDescription
				}) : null]
			})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogFooter, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				variant: "outline",
				onClick: () => setPending(null),
				disabled: isBusy,
				children: translate("auto.components.shared.useDaemonActions.01af244097", "Cancel")
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
				variant: "destructive",
				onClick: runConfirmed,
				disabled: isBusy,
				children: [isBusy ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-4 animate-spin" }) : null, isBusy && busyKind === pending ? copy.busyLabel : copy.confirmLabel]
			})] })] }) : null
		})
	});
}
export { subscribeDaemonSessionInventoryInvalidated as i, useDaemonActions as n, notifyDaemonSessionInventoryInvalidated as r, DaemonActionDialog as t };
