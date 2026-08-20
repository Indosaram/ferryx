import { Ft as getCachedAutomaticPushTargetUpstreamStatus, Gf as getRuntimeGitStatus, It as invalidateAutomaticPushTargetUpstreamStatusCache, Lt as storeCachedAutomaticPushTargetUpstreamStatus, Pt as clearAutomaticPushTargetUpstreamStatusCache, qf as getRuntimeGitUpstreamStatus } from "./store-CgXrfmaH.js";
var branchLineTotalMergeBaseByWorktree = /* @__PURE__ */ new Map();
function setBranchLineTotalMergeBase(worktreeId, mergeBase) {
	if (!mergeBase) {
		branchLineTotalMergeBaseByWorktree.delete(worktreeId);
		return;
	}
	branchLineTotalMergeBaseByWorktree.set(worktreeId, mergeBase);
}
function getBranchLineTotalMergeBase(worktreeId) {
	return branchLineTotalMergeBaseByWorktree.get(worktreeId);
}
var MAX_REFRESH_ORDERING_WORKTREES = 1024;
var strictUpstreamRefreshGenerationByWorktree = /* @__PURE__ */ new Map();
var automaticRefreshGenerationByWorktree = /* @__PURE__ */ new Map();
var lastAppliedAutomaticGenerationByWorktree = /* @__PURE__ */ new Map();
var automaticUpstreamRefreshInFlightByWorktree = /* @__PURE__ */ new Map();
function trimRefreshOrderingState() {
	for (const worktreeId of strictUpstreamRefreshGenerationByWorktree.keys()) {
		if (strictUpstreamRefreshGenerationByWorktree.size <= MAX_REFRESH_ORDERING_WORKTREES) break;
		if (automaticUpstreamRefreshInFlightByWorktree.has(worktreeId)) continue;
		strictUpstreamRefreshGenerationByWorktree.delete(worktreeId);
	}
}
function beginAutomaticUpstreamRefresh(worktreeId) {
	automaticUpstreamRefreshInFlightByWorktree.set(worktreeId, (automaticUpstreamRefreshInFlightByWorktree.get(worktreeId) ?? 0) + 1);
	const automaticGeneration = (automaticRefreshGenerationByWorktree.get(worktreeId) ?? 0) + 1;
	automaticRefreshGenerationByWorktree.set(worktreeId, automaticGeneration);
	return {
		strictGeneration: strictUpstreamRefreshGenerationByWorktree.get(worktreeId) ?? 0,
		automaticGeneration
	};
}
function finishAutomaticUpstreamRefresh(worktreeId) {
	const count = automaticUpstreamRefreshInFlightByWorktree.get(worktreeId) ?? 0;
	if (count <= 1) {
		automaticUpstreamRefreshInFlightByWorktree.delete(worktreeId);
		automaticRefreshGenerationByWorktree.delete(worktreeId);
		lastAppliedAutomaticGenerationByWorktree.delete(worktreeId);
	} else automaticUpstreamRefreshInFlightByWorktree.set(worktreeId, count - 1);
	trimRefreshOrderingState();
}
function shouldApplyAutomaticUpstreamRefresh(worktreeId, order, shouldApply) {
	return (strictUpstreamRefreshGenerationByWorktree.get(worktreeId) ?? 0) === order.strictGeneration && order.automaticGeneration >= (lastAppliedAutomaticGenerationByWorktree.get(worktreeId) ?? 0) && (shouldApply?.() ?? true);
}
function claimAutomaticUpstreamRefreshApply(worktreeId, order, shouldApply) {
	if (!shouldApplyAutomaticUpstreamRefresh(worktreeId, order, shouldApply)) return false;
	lastAppliedAutomaticGenerationByWorktree.set(worktreeId, Math.max(lastAppliedAutomaticGenerationByWorktree.get(worktreeId) ?? 0, order.automaticGeneration));
	return true;
}
function beginStrictUpstreamRefresh(worktreeId) {
	strictUpstreamRefreshGenerationByWorktree.set(worktreeId, (strictUpstreamRefreshGenerationByWorktree.get(worktreeId) ?? 0) + 1);
	trimRefreshOrderingState();
}
async function fetchAndApplyAutomaticUpstreamStatus({ settings, worktreeId, worktreePath, connectionId, pushTarget, deps, order, shouldApply }) {
	if (!shouldApplyAutomaticUpstreamRefresh(worktreeId, order, shouldApply)) return null;
	const upstreamStatus = await deps.fetchUpstreamStatus(worktreeId, worktreePath, connectionId, pushTarget, {
		runtimeTargetSettings: settings,
		applyUpstreamStatus: false
	});
	if (!upstreamStatus) {
		if (pushTarget) invalidateAutomaticPushTargetUpstreamStatusCache({
			settings,
			worktreeId,
			worktreePath,
			connectionId,
			pushTarget
		});
		return null;
	}
	if (!claimAutomaticUpstreamRefreshApply(worktreeId, order, shouldApply)) return null;
	deps.setUpstreamStatus(worktreeId, upstreamStatus);
	return upstreamStatus;
}
async function refreshGitStatusForWorktree({ settings, worktreeId, worktreePath, connectionId, pushTarget, deps, request }) {
	const refreshOrder = beginAutomaticUpstreamRefresh(worktreeId);
	const branchLineTotalMergeBase = getBranchLineTotalMergeBase(worktreeId);
	try {
		const status = await getRuntimeGitStatus({
			settings,
			worktreeId,
			worktreePath,
			connectionId
		}, {
			...request?.reuseLineStats === true ? { reuseLineStats: true } : {},
			...request?.signal ? { signal: request.signal } : {},
			...branchLineTotalMergeBase ? { branchLineTotalMergeBase } : {}
		});
		if (!claimAutomaticUpstreamRefreshApply(worktreeId, refreshOrder, request?.shouldApply)) return;
		deps.setGitStatus(worktreeId, status);
		deps.updateWorktreeGitIdentity(worktreeId, {
			head: status.head,
			branch: status.branch ?? (status.head ? null : void 0)
		});
		request?.onStatusAccepted?.(status);
		if (pushTarget) {
			if (getCachedAutomaticPushTargetUpstreamStatus({
				settings,
				worktreeId,
				worktreePath,
				connectionId,
				pushTarget,
				status
			})) return;
			const upstreamStatus = await fetchAndApplyAutomaticUpstreamStatus({
				settings,
				worktreeId,
				worktreePath,
				connectionId,
				pushTarget,
				deps,
				order: refreshOrder,
				shouldApply: request?.shouldApply
			});
			if (upstreamStatus) storeCachedAutomaticPushTargetUpstreamStatus({
				settings,
				worktreeId,
				worktreePath,
				connectionId,
				pushTarget,
				status
			}, upstreamStatus);
			return;
		}
		if (status.upstreamStatus) {
			if (status.upstreamStatus.ahead > 0 && status.upstreamStatus.behind > 0 && status.upstreamStatus.behindCommitsArePatchEquivalent === void 0) {
				await fetchAndApplyAutomaticUpstreamStatus({
					settings,
					worktreeId,
					worktreePath,
					connectionId,
					deps,
					order: refreshOrder,
					shouldApply: request?.shouldApply
				});
				return;
			}
			if (claimAutomaticUpstreamRefreshApply(worktreeId, refreshOrder, request?.shouldApply)) deps.setUpstreamStatus(worktreeId, status.upstreamStatus);
			return;
		}
		await fetchAndApplyAutomaticUpstreamStatus({
			settings,
			worktreeId,
			worktreePath,
			connectionId,
			pushTarget,
			deps,
			order: refreshOrder,
			shouldApply: request?.shouldApply
		});
	} finally {
		finishAutomaticUpstreamRefresh(worktreeId);
	}
}
async function refreshGitStatusForWorktreeStrict({ settings, worktreeId, worktreePath, connectionId, pushTarget, deps }) {
	beginStrictUpstreamRefresh(worktreeId);
	clearAutomaticPushTargetUpstreamStatusCache();
	const strictBranchLineTotalMergeBase = getBranchLineTotalMergeBase(worktreeId);
	const status = await getRuntimeGitStatus({
		settings,
		worktreeId,
		worktreePath,
		connectionId
	}, {
		bypassEffectiveUpstreamNegativeCache: true,
		...strictBranchLineTotalMergeBase ? { branchLineTotalMergeBase: strictBranchLineTotalMergeBase } : {}
	});
	deps.setGitStatus(worktreeId, status);
	deps.updateWorktreeGitIdentity(worktreeId, {
		head: status.head,
		branch: status.branch ?? (status.head ? null : void 0)
	});
	if (pushTarget) {
		const upstreamStatus$1 = await getRuntimeGitUpstreamStatus({
			settings,
			worktreeId,
			worktreePath,
			connectionId
		}, pushTarget);
		deps.setUpstreamStatus(worktreeId, upstreamStatus$1);
		return {
			status,
			upstreamStatus: upstreamStatus$1
		};
	}
	if (status.upstreamStatus) {
		if (status.upstreamStatus.ahead > 0 && status.upstreamStatus.behind > 0 && status.upstreamStatus.behindCommitsArePatchEquivalent === void 0) {
			const upstreamStatus$1 = await getRuntimeGitUpstreamStatus({
				settings,
				worktreeId,
				worktreePath,
				connectionId
			}, void 0);
			deps.setUpstreamStatus(worktreeId, upstreamStatus$1);
			return {
				status,
				upstreamStatus: upstreamStatus$1
			};
		}
		deps.setUpstreamStatus(worktreeId, status.upstreamStatus);
		return {
			status,
			upstreamStatus: status.upstreamStatus
		};
	}
	const upstreamStatus = await getRuntimeGitUpstreamStatus({
		settings,
		worktreeId,
		worktreePath,
		connectionId
	}, void 0);
	deps.setUpstreamStatus(worktreeId, upstreamStatus);
	return {
		status,
		upstreamStatus
	};
}
export { refreshGitStatusForWorktreeStrict as n, setBranchLineTotalMergeBase as r, refreshGitStatusForWorktree as t };
