import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { wd as INSTALLED_AGENT_SKILLS_CHANGED_EVENT } from "./store-CgXrfmaH.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var FOCUS_RESCAN_COOLDOWN_MS = 15e3;
var cachedInventory = null;
var pendingInventory = null;
var invalidationRevision = 0;
var completedRevision = -1;
var lastCompletedScanAt = 0;
var refreshSequence = 0;
var scheduledFocusRescan = null;
var snapshot = {
	inventory: null,
	loading: false,
	error: null
};
var DISABLED_SNAPSHOT = Object.freeze({
	inventory: null,
	loading: false,
	error: null
});
var REENABLING_SNAPSHOT = Object.freeze({
	inventory: null,
	loading: true,
	error: null
});
var subscribers = /* @__PURE__ */ new Set();
var pendingReenableRefresh = null;
function publishSnapshot(next) {
	if (snapshot.inventory === next.inventory && snapshot.loading === next.loading && snapshot.error === next.error) return;
	snapshot = next;
	for (const subscriber of subscribers) subscriber();
}
async function loadInventory(force) {
	if (force) invalidationRevision += 1;
	const targetRevision = invalidationRevision;
	for (;;) {
		if (cachedInventory && completedRevision >= targetRevision) return cachedInventory;
		if (!pendingInventory) {
			const requestRevision = invalidationRevision;
			const request = window.api.skills.freshnessInventory().then((inventory) => {
				cachedInventory = inventory;
				completedRevision = Math.max(completedRevision, requestRevision);
				lastCompletedScanAt = Date.now();
				return inventory;
			}).finally(() => {
				if (pendingInventory === request) pendingInventory = null;
			});
			pendingInventory = request;
		}
		await pendingInventory;
	}
}
async function refreshSkillFreshness(force = true) {
	if (scheduledFocusRescan !== null) {
		window.clearTimeout(scheduledFocusRescan);
		scheduledFocusRescan = null;
	}
	const sequence = ++refreshSequence;
	publishSnapshot({
		inventory: null,
		loading: true,
		error: null
	});
	try {
		const inventory = await loadInventory(force);
		if (sequence === refreshSequence) publishSnapshot({
			inventory,
			loading: false,
			error: null
		});
	} catch (cause) {
		if (sequence === refreshSequence) publishSnapshot({
			inventory: null,
			loading: false,
			error: cause instanceof Error ? cause.message : "Could not inspect Orca skills."
		});
	}
}
function onWindowFocus() {
	const cooldownRemaining = FOCUS_RESCAN_COOLDOWN_MS - (Date.now() - lastCompletedScanAt);
	if (cooldownRemaining <= 0) {
		refreshSkillFreshness(true);
		return;
	}
	if (!snapshot.inventory?.eligibleUpdateNames.length || scheduledFocusRescan !== null) return;
	publishSnapshot({
		inventory: null,
		loading: true,
		error: null
	});
	scheduledFocusRescan = window.setTimeout(() => {
		scheduledFocusRescan = null;
		refreshSkillFreshness(true);
	}, Math.min(cooldownRemaining, FOCUS_RESCAN_COOLDOWN_MS));
}
function onInstalledSkillsChanged() {
	refreshSkillFreshness(true);
}
function subscribe(subscriber) {
	subscribers.add(subscriber);
	if (subscribers.size === 1) {
		window.addEventListener("focus", onWindowFocus);
		window.addEventListener(INSTALLED_AGENT_SKILLS_CHANGED_EVENT, onInstalledSkillsChanged);
	}
	return () => {
		subscribers.delete(subscriber);
		if (subscribers.size === 0) {
			window.removeEventListener("focus", onWindowFocus);
			window.removeEventListener(INSTALLED_AGENT_SKILLS_CHANGED_EVENT, onInstalledSkillsChanged);
			if (scheduledFocusRescan !== null) {
				window.clearTimeout(scheduledFocusRescan);
				scheduledFocusRescan = null;
			}
		}
	};
}
function subscribeDisabled() {
	return () => {};
}
function getSnapshot() {
	return snapshot;
}
function getDisabledSnapshot() {
	return DISABLED_SNAPSHOT;
}
function ensureInventoryLoaded() {
	if (!snapshot.inventory && !snapshot.loading) refreshSkillFreshness(false);
}
async function skipSkillFreshnessRefresh() {}
function refreshSkillFreshnessAfterReenable() {
	pendingReenableRefresh ?? (pendingReenableRefresh = refreshSkillFreshness(true).finally(() => {
		pendingReenableRefresh = null;
	}));
	return pendingReenableRefresh;
}
function useSkillFreshness(enabled = true) {
	const previousEnabledRef = (0, import_react.useRef)(enabled);
	const reenabled = enabled && !previousEnabledRef.current;
	const current = (0, import_react.useSyncExternalStore)(enabled ? subscribe : subscribeDisabled, enabled ? getSnapshot : getDisabledSnapshot, enabled ? getSnapshot : getDisabledSnapshot);
	(0, import_react.useEffect)(() => {
		const wasEnabled = previousEnabledRef.current;
		previousEnabledRef.current = enabled;
		if (!enabled) return;
		if (!wasEnabled) {
			refreshSkillFreshnessAfterReenable();
			return;
		}
		ensureInventoryLoaded();
	}, [enabled]);
	return {
		...reenabled ? REENABLING_SNAPSHOT : current,
		refresh: enabled ? refreshSkillFreshness : skipSkillFreshnessRefresh
	};
}
const SUPPORTED_GLOBAL_SKILL_TOPOLOGIES = new Set(["canonical-copy", "provider-alias"]);
var OWNER_MANAGED_SKILL_SCOPES = new Set(["repo-scope", "plugin-cache"]);
function isOwnerManagedSkillScope(topology) {
	return OWNER_MANAGED_SKILL_SCOPES.has(topology);
}
function skillPlacementParticipatesInGlobalFreshness(installation) {
	return installation.topology !== "repo-scope";
}
function isSkillCopyNeedingAttention(installation) {
	return skillPlacementParticipatesInGlobalFreshness(installation) && installation.status !== "current" && installation.status !== "newer-known" && !(installation.status === "unrecognized" && installation.topology === "plugin-cache") && !(SUPPORTED_GLOBAL_SKILL_TOPOLOGIES.has(installation.topology) && installation.status === "outdated");
}
var SKILL_SCAN_ATTENTION_REASONS = new Set(["io-error"]);
function isSkillScanAttentionReason(reason) {
	return SKILL_SCAN_ATTENTION_REASONS.has(reason);
}
function isSkillScanIssueNeedingAttention(issue) {
	return isSkillScanAttentionReason(issue.reason);
}
var SKILL_SCAN_TRUNCATING_REASONS = new Set(["entry-limit", "candidate-limit"]);
function isTruncatingSkillScanReason(reason) {
	return SKILL_SCAN_TRUNCATING_REASONS.has(reason);
}
function isSkillScanIssueTruncatingScan(issue) {
	return isTruncatingSkillScanReason(issue.reason);
}
function canonicalizeSkillUpdateNames(names) {
	const canonicalNames = [...new Set(names)].sort((left, right) => left.localeCompare(right, "en"));
	if (canonicalNames.some((name) => !/^[a-z0-9][a-z0-9._-]*$/.test(name))) return null;
	return canonicalNames.length > 0 ? canonicalNames : null;
}
function buildTargetedSkillUpdateCommand(names) {
	const canonicalNames = canonicalizeSkillUpdateNames(names);
	return canonicalNames ? `npx skills update ${canonicalNames.join(" ")} --global` : null;
}
export { isSkillScanIssueTruncatingScan as a, useSkillFreshness as c, isSkillScanIssueNeedingAttention as i, isOwnerManagedSkillScope as n, skillPlacementParticipatesInGlobalFreshness as o, isSkillCopyNeedingAttention as r, refreshSkillFreshness as s, buildTargetedSkillUpdateCommand as t };
