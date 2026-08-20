import { rp as callRuntimeRpc } from "./store-CgXrfmaH.js";
import { st as parseExecutionHostId } from "./agent-status-3vUKbY6l.js";
function getAutomationHostTargetKey(target) {
	return target.kind === "environment" ? `environment:${target.environmentId}` : "local";
}
function getAutomationHostTargetFromKey(key) {
	if (!key) return null;
	if (key.startsWith("environment:")) return {
		kind: "environment",
		environmentId: key.slice(12)
	};
	return { kind: "local" };
}
function getAutomationTargetFromHostId(hostId) {
	const parsed = parseExecutionHostId(hostId);
	return parsed?.kind === "runtime" ? {
		kind: "environment",
		environmentId: parsed.environmentId
	} : { kind: "local" };
}
function getAutomationListTarget(settings) {
	const environmentId = settings?.activeRuntimeEnvironmentId?.trim();
	return environmentId ? {
		kind: "environment",
		environmentId
	} : { kind: "local" };
}
function getAutomationOwnerTarget(automation, sourceTarget) {
	if (sourceTarget?.kind === "environment") return sourceTarget;
	return getAutomationTargetFromHostId(automation.runContext?.hostId);
}
function getAutomationCreateTarget(input) {
	return getAutomationTargetFromHostId(input.runContext?.hostId);
}
function toRuntimeAutomationCreateInput(input) {
	const { projectId, workspaceId, ...rest } = input;
	return {
		...rest,
		repo: projectId,
		workspace: input.workspaceMode === "existing" ? workspaceId ?? void 0 : void 0
	};
}
function toRuntimeAutomationUpdateInput(input) {
	const { projectId, workspaceId, ...rest } = input;
	return {
		...rest,
		...projectId !== void 0 ? { repo: projectId } : {},
		...workspaceId !== void 0 ? { workspace: workspaceId ?? void 0 } : {}
	};
}
async function listAutomationsForTarget(target) {
	if (target.kind === "local") return await window.api.automations.list();
	return (await callRuntimeRpc(target, "automation.list", void 0, { timeoutMs: 15e3 })).automations;
}
async function listAutomationRunsForTarget(target, automationId) {
	if (target.kind === "local") return await window.api.automations.listRuns(automationId ? { automationId } : void 0);
	return (await callRuntimeRpc(target, "automation.runs", automationId ? { automationId } : {}, { timeoutMs: 15e3 })).runs;
}
async function createAutomationForTarget(input) {
	const target = getAutomationCreateTarget(input);
	if (target.kind === "local") return await window.api.automations.create(input);
	return (await callRuntimeRpc(target, "automation.create", toRuntimeAutomationCreateInput(input), { timeoutMs: 15e3 })).automation;
}
async function updateAutomationForTarget(automation, updates, sourceTarget) {
	const target = getAutomationOwnerTarget(automation, sourceTarget);
	if (target.kind === "local") return await window.api.automations.update({
		id: automation.id,
		updates
	});
	return (await callRuntimeRpc(target, "automation.update", {
		id: automation.id,
		updates: toRuntimeAutomationUpdateInput(updates)
	}, { timeoutMs: 15e3 })).automation;
}
async function deleteAutomationForTarget(automation, sourceTarget) {
	const target = getAutomationOwnerTarget(automation, sourceTarget);
	if (target.kind === "local") {
		await window.api.automations.delete({ id: automation.id });
		return;
	}
	await callRuntimeRpc(target, "automation.delete", { id: automation.id }, { timeoutMs: 15e3 });
}
async function runAutomationNowForTarget(automation, sourceTarget) {
	const target = getAutomationOwnerTarget(automation, sourceTarget);
	if (target.kind === "local") return await window.api.automations.runNow({ id: automation.id });
	return (await callRuntimeRpc(target, "automation.runNow", { id: automation.id }, { timeoutMs: 15e3 })).run;
}
export { getAutomationListTarget as a, listAutomationRunsForTarget as c, updateAutomationForTarget as d, getAutomationHostTargetKey as i, listAutomationsForTarget as l, deleteAutomationForTarget as n, getAutomationOwnerTarget as o, getAutomationHostTargetFromKey as r, getAutomationTargetFromHostId as s, createAutomationForTarget as t, runAutomationNowForTarget as u };
