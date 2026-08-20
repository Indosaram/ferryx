import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { Cd as notifyInstalledAgentSkillsChanged } from "./store-CgXrfmaH.js";
import { i as subscribeSkillFreshnessUpdateDialog, n as getSkillFreshnessUpdateDialogRequest } from "./skill-freshness-update-dialog-Bw6DvImT.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var run = { state: "idle" };
var listeners = /* @__PURE__ */ new Set();
var subscribed = false;
var successTimer = null;
const SKILL_UPDATE_SUCCESS_LINGER_MS = 4e3;
function emit() {
	for (const listener of listeners) listener();
}
function clearSuccessTimer() {
	if (successTimer) {
		clearTimeout(successTimer);
		successTimer = null;
	}
}
function scheduleSuccessLinger() {
	clearSuccessTimer();
	if (run.state !== "success" || getSkillFreshnessUpdateDialogRequest()) return;
	successTimer = setTimeout(() => {
		successTimer = null;
		acknowledgeSkillUpdateRun();
	}, SKILL_UPDATE_SUCCESS_LINGER_MS);
}
subscribeSkillFreshnessUpdateDialog(scheduleSuccessLinger);
function setRun(next) {
	const wasRunning = run.state === "running";
	run = next;
	scheduleSuccessLinger();
	if (next.state === "success" || next.state === "error" || wasRunning && next.state === "idle") notifyInstalledAgentSkillsChanged();
	emit();
}
function ensureSubscribed() {
	if (subscribed) return;
	subscribed = true;
	window.api.skills.onUpdateRun(setRun);
	window.api.skills.getUpdateRun().then((current) => {
		if (run.state === "idle") setRun(current);
	});
}
function subscribeSkillUpdateRun(listener) {
	ensureSubscribed();
	listeners.add(listener);
	return () => listeners.delete(listener);
}
function getSkillUpdateRun() {
	return run;
}
function useSkillUpdateRun() {
	return (0, import_react.useSyncExternalStore)(subscribeSkillUpdateRun, getSkillUpdateRun, getSkillUpdateRun);
}
async function startSkillUpdateRun(names) {
	ensureSubscribed();
	try {
		await window.api.skills.startUpdateRun([...names]);
	} catch (error) {
		console.error("Failed to start skill update run", error);
	}
}
async function cancelSkillUpdateRun() {
	try {
		await window.api.skills.cancelUpdateRun();
	} catch (error) {
		console.error("Failed to cancel skill update run", error);
	}
}
async function acknowledgeSkillUpdateRun() {
	try {
		await window.api.skills.acknowledgeUpdateRun();
	} catch (error) {
		console.error("Failed to acknowledge skill update run", error);
	}
}
export { useSkillUpdateRun as i, cancelSkillUpdateRun as n, startSkillUpdateRun as r, acknowledgeSkillUpdateRun as t };
