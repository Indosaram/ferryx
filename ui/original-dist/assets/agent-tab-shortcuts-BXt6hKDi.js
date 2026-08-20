import { ng as pickTuiAgent, tg as normalizeDisabledTuiAgents } from "./store-CgXrfmaH.js";
import { A as ALL_TUI_AGENTS, s as agentTabActionId } from "./plugin-manifest-Bs-50M_g.js";
function listBoundAgentTabActions(keybindings, disabledTuiAgents) {
	if (!keybindings) return [];
	const disabled = new Set(normalizeDisabledTuiAgents(disabledTuiAgents));
	const bound = [];
	for (const agent of ALL_TUI_AGENTS) {
		if (disabled.has(agent)) continue;
		const actionId = agentTabActionId(agent);
		if ((keybindings[actionId] ?? []).length > 0) bound.push({
			agent,
			actionId
		});
	}
	return bound;
}
function resolveDefaultAgentForNewTab(args) {
	return pickTuiAgent(args.defaultTuiAgent === "blank" ? null : args.defaultTuiAgent, args.detectedAgentIds ?? [], args.disabledTuiAgents);
}
export { resolveDefaultAgentForNewTab as n, listBoundAgentTabActions as t };
