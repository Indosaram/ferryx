import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { Yu as getSingleFocusedRuntimeEnvironmentId, op as getActiveRuntimeTarget, t as useAppStore } from "./store-CgXrfmaH.js";
function isSkillsCliAgentKeyShaped(value) {
	return /^(?:\*|[a-z0-9][a-z0-9.-]*)$/i.test(value);
}
const ORCA_SKILLS_REPOSITORY_URL = "https://github.com/stablyai/orca";
const ORCA_CLI_SKILL_NAME = "orca-cli";
const COMPUTER_USE_SKILL_NAME = "computer-use";
const ORCHESTRATION_SKILL_NAME = "orchestration";
const EPHEMERAL_VMS_SKILL_NAME = "orca-per-workspace-env";
const ORCA_LINEAR_SKILL_NAME = "orca-linear";
const LINEAR_TICKETS_SKILL_NAME = "linear-tickets";
const LINEAR_AGENT_SKILL_NAMES = [ORCA_LINEAR_SKILL_NAME, LINEAR_TICKETS_SKILL_NAME];
function buildAgentFeatureSkillInstallArgs(skillNames, options = {}) {
	if (skillNames.length === 0) throw new Error("At least one skill name is required.");
	const global = options.global ?? true;
	const agents = options.agents ?? [];
	if (options.yes && agents.length === 0) throw new Error("An install target is required when skipping prompts.");
	const unusable = agents.find((agent) => !isSkillsCliAgentKeyShaped(agent));
	if (unusable !== void 0) throw new Error(`"${unusable}" is not a usable install target.`);
	return [
		"skills",
		"add",
		ORCA_SKILLS_REPOSITORY_URL,
		...skillNames.flatMap((name) => ["--skill", name]),
		...global ? ["--global"] : [],
		...agents.flatMap((agent) => ["--agent", agent]),
		...options.yes ? ["-y"] : []
	];
}
function buildAgentFeatureSkillInstallCommand(skillNames, options = {}) {
	return `npx ${buildAgentFeatureSkillInstallArgs(skillNames, options).join(" ")}`;
}
function buildAgentFeatureSkillUpdateArgs(skillNames, options = {}) {
	const names = (typeof skillNames === "string" ? [skillNames] : skillNames).map((name) => name.trim()).filter((name) => name.length > 0);
	if (names.length === 0) throw new Error("A skill name is required.");
	const global = options.global ?? true;
	return [
		"skills",
		"update",
		...names,
		global ? "--global" : "--project",
		...options.yes ? ["-y"] : []
	];
}
function buildAgentFeatureSkillUpdateCommand(skillNames, options = {}) {
	return `npx ${buildAgentFeatureSkillUpdateArgs(skillNames, options).join(" ")}`;
}
const ORCA_CLI_SKILL_INSTALL_COMMAND = buildAgentFeatureSkillInstallCommand([ORCA_CLI_SKILL_NAME]);
const ORCA_CLI_SKILL_UPDATE_COMMAND = buildAgentFeatureSkillUpdateCommand(ORCA_CLI_SKILL_NAME);
const COMPUTER_USE_SKILL_INSTALL_COMMAND = buildAgentFeatureSkillInstallCommand([COMPUTER_USE_SKILL_NAME]);
const COMPUTER_USE_SKILL_UPDATE_COMMAND = buildAgentFeatureSkillUpdateCommand(COMPUTER_USE_SKILL_NAME);
const ORCHESTRATION_SKILL_INSTALL_COMMAND = buildAgentFeatureSkillInstallCommand([ORCHESTRATION_SKILL_NAME]);
const ORCHESTRATION_SKILL_UPDATE_COMMAND = buildAgentFeatureSkillUpdateCommand(ORCHESTRATION_SKILL_NAME);
const EPHEMERAL_VMS_SKILL_INSTALL_COMMAND = buildAgentFeatureSkillInstallCommand([EPHEMERAL_VMS_SKILL_NAME]);
const EPHEMERAL_VMS_SKILL_UPDATE_COMMAND = buildAgentFeatureSkillUpdateCommand(EPHEMERAL_VMS_SKILL_NAME);
const ORCA_CLI_ORCHESTRATION_SKILL_INSTALL_COMMAND = buildAgentFeatureSkillInstallCommand([ORCA_CLI_SKILL_NAME, ORCHESTRATION_SKILL_NAME]);
const ORCA_LINEAR_SKILL_INSTALL_COMMAND = buildAgentFeatureSkillInstallCommand([ORCA_LINEAR_SKILL_NAME]);
const ORCA_LINEAR_SKILL_UPDATE_COMMAND = buildAgentFeatureSkillUpdateCommand(ORCA_LINEAR_SKILL_NAME);
const LINEAR_TICKETS_SKILL_UPDATE_COMMAND = buildAgentFeatureSkillUpdateCommand(LINEAR_TICKETS_SKILL_NAME);
const ORCHESTRATION_SETUP_STATE_EVENT = "orca:orchestration-setup-state";
const ORCHESTRATION_ENABLED_STORAGE_KEY = "orca.orchestration.enabled";
const ORCHESTRATION_SETUP_DISMISSED_STORAGE_KEY = "orca.orchestration.setupDismissed";
function isOrchestrationSetupEnabled() {
	return localStorage.getItem(ORCHESTRATION_ENABLED_STORAGE_KEY) === "1";
}
function hasOrchestrationSetupMarker() {
	return isOrchestrationSetupEnabled();
}
function markOrchestrationSetupComplete() {
	localStorage.setItem(ORCHESTRATION_ENABLED_STORAGE_KEY, "1");
	notifyOrchestrationSetupStateChanged();
}
function isOrchestrationSetupDismissed() {
	return localStorage.getItem(ORCHESTRATION_SETUP_DISMISSED_STORAGE_KEY) === "1";
}
function notifyOrchestrationSetupStateChanged() {
	window.dispatchEvent(new CustomEvent(ORCHESTRATION_SETUP_STATE_EVENT));
}
var import_react = /* @__PURE__ */ __toESM(require_react());
var UNRESOLVED = Symbol("skill-discovery-runtime-unresolved");
function useActiveSkillDiscoveryRuntimeTarget() {
	const environmentId = useAppStore((state) => state.runtimeEnvironmentCatalogSettled ? getSingleFocusedRuntimeEnvironmentId(state) : UNRESOLVED);
	return (0, import_react.useMemo)(() => environmentId === UNRESOLVED ? null : getActiveRuntimeTarget({ activeRuntimeEnvironmentId: environmentId }), [environmentId]);
}
export { ORCA_LINEAR_SKILL_NAME as C, ORCHESTRATION_SKILL_UPDATE_COMMAND as D, ORCHESTRATION_SKILL_NAME as E, buildAgentFeatureSkillInstallCommand as O, ORCA_LINEAR_SKILL_INSTALL_COMMAND as S, ORCHESTRATION_SKILL_INSTALL_COMMAND as T, LINEAR_TICKETS_SKILL_UPDATE_COMMAND as _, hasOrchestrationSetupMarker as a, ORCA_CLI_SKILL_NAME as b, notifyOrchestrationSetupStateChanged as c, COMPUTER_USE_SKILL_UPDATE_COMMAND as d, EPHEMERAL_VMS_SKILL_INSTALL_COMMAND as f, LINEAR_TICKETS_SKILL_NAME as g, LINEAR_AGENT_SKILL_NAMES as h, ORCHESTRATION_SETUP_STATE_EVENT as i, COMPUTER_USE_SKILL_INSTALL_COMMAND as l, EPHEMERAL_VMS_SKILL_UPDATE_COMMAND as m, ORCHESTRATION_ENABLED_STORAGE_KEY as n, isOrchestrationSetupDismissed as o, EPHEMERAL_VMS_SKILL_NAME as p, ORCHESTRATION_SETUP_DISMISSED_STORAGE_KEY as r, markOrchestrationSetupComplete as s, useActiveSkillDiscoveryRuntimeTarget as t, COMPUTER_USE_SKILL_NAME as u, ORCA_CLI_ORCHESTRATION_SKILL_INSTALL_COMMAND as v, ORCA_LINEAR_SKILL_UPDATE_COMMAND as w, ORCA_CLI_SKILL_UPDATE_COMMAND as x, ORCA_CLI_SKILL_INSTALL_COMMAND as y };
