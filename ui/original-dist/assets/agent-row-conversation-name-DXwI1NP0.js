import { L as isClaudeManagementTitle, P as isMeaningfulOpenCodeTerminalTitle, j as stripLeadingAgentTitleDecorationOrEmpty, r as formatAgentTypeLabel } from "./agent-status-3vUKbY6l.js";
import { a as SYNTHETIC_AGENT_TITLE_PROFILES } from "./pane-agent-owner-BPfoVAtS.js";
var SYNTHETIC_STATUS_TITLES_LOWER = new Set(Object.values(SYNTHETIC_AGENT_TITLE_PROFILES).flatMap((profile) => [
	profile.workingLabel.toLowerCase(),
	profile.permissionLabel.toLowerCase(),
	profile.idleLabel.toLowerCase()
]));
var FALLBACK_TAB_TITLE_LOWER = "agent";
var AGENT_IDENTITY_ALIASES_LOWER = {
	claude: ["claude code"],
	gemini: ["gemini cli"]
};
var STATUS_WITH_CONTEXT_RE = /^(?:ready|idle|done)(?:\s+\([^)]*\))?$/i;
var DEFAULT_TERMINAL_TITLE_RE = /^terminal \d+$/i;
function isIdentityStatusTitle(titleLower, identityLower) {
	return titleLower === identityLower || titleLower === `${identityLower} ready` || titleLower === `${identityLower} idle` || titleLower === `${identityLower} done` || titleLower === `${identityLower} working` || titleLower === `${identityLower} thinking` || titleLower === `${identityLower} running` || titleLower === `${identityLower} - action required`;
}
function isAgentIdentityStatusTitle(titleLower, agentType, agentTypeLabelLower) {
	if (isIdentityStatusTitle(titleLower, agentTypeLabelLower)) return true;
	return AGENT_IDENTITY_ALIASES_LOWER[agentType ?? ""]?.some((identity) => isIdentityStatusTitle(titleLower, identity)) ?? false;
}
function isCwdLikeTitle(title) {
	if (/^(?:~|[\\/]|[A-Za-z]:[\\/])/.test(title)) return true;
	return !/\s/.test(title) && /[\\/]/.test(title);
}
function conversationNameFromLiveTitle(liveTitle, agentType, agentTypeLabelLower, defaultTitle) {
	const stripped = stripLeadingAgentTitleDecorationOrEmpty(liveTitle.trim()).trim();
	if (!stripped) return null;
	const lower = stripped.toLowerCase();
	if (SYNTHETIC_STATUS_TITLES_LOWER.has(lower) || lower === FALLBACK_TAB_TITLE_LOWER || isAgentIdentityStatusTitle(lower, agentType, agentTypeLabelLower) || STATUS_WITH_CONTEXT_RE.test(stripped) || DEFAULT_TERMINAL_TITLE_RE.test(stripped) || isClaudeManagementTitle(stripped) || isCwdLikeTitle(stripped)) return null;
	if (defaultTitle && stripped === defaultTitle.trim()) return null;
	return stripped;
}
function getAgentRowConversationName(tab, agentType, generatedTitlesEnabled) {
	const customTitle = tab.customTitle?.trim();
	if (customTitle) return customTitle;
	const quickCommandLabel = tab.quickCommandLabel?.trim();
	if (quickCommandLabel) return quickCommandLabel;
	const liveTitle = tab.title?.trim() ?? "";
	if (isMeaningfulOpenCodeTerminalTitle(liveTitle)) return liveTitle;
	const generatedTitle = generatedTitlesEnabled ? tab.generatedTitle?.trim() : "";
	if (generatedTitle) return generatedTitle;
	if (!liveTitle) return null;
	return conversationNameFromLiveTitle(liveTitle, agentType, formatAgentTypeLabel(agentType).toLowerCase(), tab.defaultTitle);
}
export { getAgentRowConversationName as t };
