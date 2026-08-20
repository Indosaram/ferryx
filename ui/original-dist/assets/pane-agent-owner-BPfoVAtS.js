import { M as getAgentLabel, U as isLegacyPiCompatibleTitle, k as getWrapperTitleSegments, w as detectAgentStatusFromTitle } from "./agent-status-3vUKbY6l.js";
const SYNTHETIC_AGENT_TITLE_PROFILES = {
	codex: {
		workingLabel: "Codex",
		permissionLabel: "Codex - action required",
		idleLabel: "Codex ready",
		synthesizeWorkingTitle: false
	},
	cursor: {
		workingLabel: "Cursor Agent",
		permissionLabel: "Cursor - action required",
		idleLabel: "Cursor ready"
	},
	opencode: {
		workingLabel: "OpenCode",
		permissionLabel: "OpenCode - action required",
		idleLabel: "OpenCode ready",
		synthesizeTerminalTitle: false
	},
	pi: {
		workingLabel: "Pi",
		permissionLabel: "Pi - action required",
		idleLabel: "Pi ready",
		titleIdentityGroup: "pi-compatible"
	},
	omp: {
		workingLabel: "OMP",
		permissionLabel: "OMP - action required",
		idleLabel: "OMP ready",
		titleIdentityGroup: "pi-compatible"
	},
	droid: {
		workingLabel: "Droid",
		permissionLabel: "Droid - action required",
		idleLabel: "Droid ready"
	},
	hermes: {
		workingLabel: "Hermes",
		permissionLabel: "Hermes - action required",
		idleLabel: "Hermes ready"
	},
	devin: {
		workingLabel: "Devin",
		permissionLabel: "Devin - action required",
		idleLabel: "Devin ready"
	}
};
function getSyntheticAgentTitleProfile(agentType) {
	if (!agentType) return null;
	return SYNTHETIC_AGENT_TITLE_PROFILES[agentType] ?? null;
}
function getSyntheticAgentTerminalTitle(agentType, state) {
	const profile = getSyntheticAgentTitleProfile(agentType);
	if (!profile || profile.synthesizeTerminalTitle === false || state === "working") return null;
	return state === "blocked" || state === "waiting" ? profile.permissionLabel : profile.idleLabel;
}
var COMPATIBLE_IDLE_TITLE_RE = /(?<![\w./\\-])(?:ready|idle|done)(?![\w-])/i;
function getProfileForTitleLabel(label) {
	if (!label) return null;
	const normalizedLabel = label.trim().toLowerCase();
	for (const profile of Object.values(SYNTHETIC_AGENT_TITLE_PROFILES)) if (profile.workingLabel.toLowerCase() === normalizedLabel) return { profile };
	return null;
}
function getProfileForTitle(title) {
	const candidates = getWrapperTitleSegments(title);
	let fallback = null;
	for (const candidate of candidates) {
		const labelProfile = getProfileForTitleLabel(getAgentLabel(candidate));
		const legacyProfile = isLegacyPiCompatibleTitle(candidate) ? getProfileForTitleLabel("Pi") : null;
		const candidateProfile = labelProfile ?? legacyProfile;
		if (!candidateProfile) continue;
		const match = {
			...candidateProfile,
			sourceTitle: candidate
		};
		if (candidateProfile.profile.titleIdentityGroup) return match;
		fallback ?? (fallback = match);
	}
	return fallback;
}
function getSourceTitleStatus(title) {
	const detectedStatus = detectAgentStatusFromTitle(title);
	if (detectedStatus) return detectedStatus;
	if (isLegacyPiCompatibleTitle(title)) return "idle";
	return null;
}
function hasPermissionSuffix(title, sourceProfile) {
	const normalizedTitle = title.trim().toLowerCase();
	return normalizedTitle === sourceProfile.permissionLabel.toLowerCase() || normalizedTitle.includes("action required") || normalizedTitle.includes("permission") || normalizedTitle.includes("waiting");
}
function hasIdleSuffix(title, sourceProfile) {
	return title.trim().toLowerCase() === sourceProfile.idleLabel.toLowerCase() || COMPATIBLE_IDLE_TITLE_RE.test(title);
}
function resolveCompatibleAgentTypeForOwner(incomingAgentType, ownerAgentType) {
	if (!incomingAgentType) return;
	const incomingProfile = getSyntheticAgentTitleProfile(incomingAgentType);
	const ownerProfile = getSyntheticAgentTitleProfile(ownerAgentType);
	if (!incomingProfile?.titleIdentityGroup || !ownerProfile?.titleIdentityGroup || incomingProfile.titleIdentityGroup !== ownerProfile.titleIdentityGroup) return incomingAgentType;
	return ownerAgentType;
}
function normalizeCompatibleAgentTitleForOwner(title, ownerAgentType) {
	const ownerProfile = getSyntheticAgentTitleProfile(ownerAgentType);
	if (!ownerProfile?.titleIdentityGroup) return title;
	const source = getProfileForTitle(title);
	if (!source?.profile.titleIdentityGroup || source.profile.titleIdentityGroup !== ownerProfile.titleIdentityGroup) return title;
	const sourceStatus = getSourceTitleStatus(source.sourceTitle);
	if (sourceStatus === "working") return `\u280b ${ownerProfile.workingLabel}`;
	if (sourceStatus === "permission") return ownerProfile.permissionLabel;
	if (sourceStatus === "idle") return ownerProfile.idleLabel;
	if (hasPermissionSuffix(source.sourceTitle, source.profile)) return ownerProfile.permissionLabel;
	if (hasIdleSuffix(source.sourceTitle, source.profile)) return ownerProfile.idleLabel;
	return ownerProfile.workingLabel;
}
function normalizeCompatibleAgentStatusEntryForOwner(entry, ownerAgentType) {
	const agentType = resolveCompatibleAgentTypeForOwner(entry.agentType, ownerAgentType);
	const terminalTitle = entry.terminalTitle ? normalizeCompatibleAgentTitleForOwner(entry.terminalTitle, agentType ?? ownerAgentType) : entry.terminalTitle;
	if (agentType === entry.agentType && terminalTitle === entry.terminalTitle) return entry;
	return {
		...entry,
		...agentType ? { agentType } : {},
		...terminalTitle ? { terminalTitle } : {}
	};
}
function resolvePaneAgentOwner(signals) {
	return signals.launchAgent ?? signals.startupLaunchAgent ?? signals.initialStatusAgent ?? signals.commandInferredAgent ?? signals.hookAgent ?? signals.siblingHookAgent ?? signals.completedHookAgent ?? signals.siblingCompletedHookAgent ?? signals.sleepingSessionAgent ?? null;
}
export { SYNTHETIC_AGENT_TITLE_PROFILES as a, resolveCompatibleAgentTypeForOwner as i, normalizeCompatibleAgentStatusEntryForOwner as n, getSyntheticAgentTerminalTitle as o, normalizeCompatibleAgentTitleForOwner as r, getSyntheticAgentTitleProfile as s, resolvePaneAgentOwner as t };
