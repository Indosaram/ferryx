import { a as translate } from "./jsx-runtime-Cv_nyRjc.js";
import { Na as clearWorktreeSleepIntent, Pa as markWorktreeSleepIntent, al as VIRTUALIZED_SCROLL_ANCHOR_RECORD_EVENT, t as useAppStore } from "./store-CgXrfmaH.js";
import { n as toast } from "./dist-DgqligFk.js";
async function runSleepWorktree(worktreeId) {
	await runSleepWorktrees([worktreeId]);
}
function getSidebarWorktreeOptions(worktreeId) {
	return Array.from(document.querySelectorAll("[data-worktree-id]")).filter((element) => element.dataset.worktreeId === worktreeId);
}
function isPinnedSidebarWorktreeOption(element) {
	return element.dataset.worktreeRowKey?.startsWith("pinned:") === true;
}
function findPrimarySidebarWorktreeOption(worktreeId) {
	const options = getSidebarWorktreeOptions(worktreeId);
	return options.find((element) => element.querySelector("[data-worktree-card-active=\"primary\"]")) ?? options.find((element) => !isPinnedSidebarWorktreeOption(element)) ?? options[0] ?? null;
}
function findSidebarWorktreeRow(worktreeId, rowKey) {
	const options = getSidebarWorktreeOptions(worktreeId);
	return (rowKey ? options.find((element) => element.dataset.worktreeRowKey === rowKey) ?? null : findPrimarySidebarWorktreeOption(worktreeId) ?? null)?.closest("[data-worktree-virtual-row]") ?? null;
}
function preserveSidebarWorktreePosition(worktreeId) {
	if (typeof document === "undefined") return () => {};
	const getScroller = () => document.querySelector("[data-worktree-sidebar]");
	const scroller = getScroller();
	const activeOption = findPrimarySidebarWorktreeOption(worktreeId);
	const activeRowKey = activeOption?.dataset.worktreeRowKey;
	const row = activeOption?.closest("[data-worktree-virtual-row]") ?? null;
	if (!scroller || !row) return () => {};
	scroller.dispatchEvent(new Event(VIRTUALIZED_SCROLL_ANCHOR_RECORD_EVENT));
	const previousScrollTop = scroller.scrollTop;
	const previousScrollHeight = scroller.scrollHeight;
	const previousTop = row.getBoundingClientRect().top;
	return () => {
		let attempts = 0;
		const restore = () => {
			const currentScroller = getScroller();
			if (!currentScroller) {
				attempts += 1;
				if (attempts < 12) window.requestAnimationFrame(restore);
				return;
			}
			const nextRow = findSidebarWorktreeRow(worktreeId, activeRowKey);
			if (!nextRow) currentScroller.scrollTop = Math.max(0, previousScrollTop + currentScroller.scrollHeight - previousScrollHeight);
			else {
				const delta = nextRow.getBoundingClientRect().top - previousTop;
				if (Math.abs(delta) > 1) currentScroller.scrollTop += delta;
			}
			attempts += 1;
			if (attempts < 12) window.requestAnimationFrame(restore);
		};
		window.requestAnimationFrame(restore);
	};
}
function describeSleepFailure(error) {
	const detail = error instanceof Error ? error.message : String(error);
	if (detail.includes("legacy")) return translate("auto.components.sidebar.sleep.worktree.flow.legacy.unverified", "The older host runtime could not confirm terminal shutdown. The workspace was kept open; update the host and try again.");
	if (detail.includes("terminal_") || detail.includes("runtime") || detail.includes("connection") || detail.includes("Daemon")) return translate("auto.components.sidebar.sleep.worktree.flow.host.unverified", "The host could not confirm terminal shutdown. The workspace was kept open; check the connection and try again.");
	return translate("auto.components.sidebar.sleep.worktree.flow.retry", "The workspace was kept open. Try again; if the problem continues, check the host connection.");
}
async function runSleepWorktrees(worktreeIds) {
	if (worktreeIds.length === 0) return;
	const { activeWorktreeId, setActiveWorktree, shutdownWorktreeBrowsers, shutdownWorktreeTerminals } = useAppStore.getState();
	let activeSleepIntentWorktreeId = null;
	if (activeWorktreeId && worktreeIds.includes(activeWorktreeId)) {
		const restoreSidebarPosition = preserveSidebarWorktreePosition(activeWorktreeId);
		markWorktreeSleepIntent(activeWorktreeId);
		activeSleepIntentWorktreeId = activeWorktreeId;
		setActiveWorktree(null);
		restoreSidebarPosition();
	}
	const errors = [];
	const failedWorktreeIds = /* @__PURE__ */ new Set();
	try {
		for (const worktreeId of worktreeIds) {
			try {
				await shutdownWorktreeBrowsers(worktreeId);
			} catch (err) {
				console.error("[sleep-worktree] browser shutdown failed", {
					worktreeId,
					error: err
				});
				failedWorktreeIds.add(worktreeId);
				errors.push(describeSleepFailure(err));
				continue;
			}
			try {
				await shutdownWorktreeTerminals(worktreeId, { keepIdentifiers: true });
				if (typeof window !== "undefined" && window.api?.ephemeralVm?.suspendWorkspace) await window.api.ephemeralVm.suspendWorkspace({ workspaceId: worktreeId });
			} catch (err) {
				console.error("[sleep-worktree] terminal or host suspension failed", {
					worktreeId,
					error: err
				});
				failedWorktreeIds.add(worktreeId);
				errors.push(describeSleepFailure(err));
			}
		}
	} finally {
		if (activeSleepIntentWorktreeId) {
			clearWorktreeSleepIntent(activeSleepIntentWorktreeId);
			if (failedWorktreeIds.has(activeSleepIntentWorktreeId)) setActiveWorktree(activeSleepIntentWorktreeId);
		}
	}
	if (errors.length > 0) toast.error(worktreeIds.length === 1 ? translate("auto.components.sidebar.sleep.worktree.flow.8bc3fc0671", "Failed to sleep workspace") : translate("auto.components.sidebar.sleep.worktree.flow.c460fecc4a", "Failed to sleep some workspaces"), { description: errors.join("\n") });
}
export { runSleepWorktrees as n, runSleepWorktree as t };
