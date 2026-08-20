import { o as dashboardCardDisplayState } from "./dashboard-snapshot-B9IiTV8p.js";
function agentMapState(card) {
	const state = dashboardCardDisplayState(card);
	if (state === "blocked" || state === "waiting") return "attention";
	return state;
}
function filterAgentMapCards({ cards, enabledStates, hostFilter }) {
	return cards.filter((card) => (hostFilter === "all" || (card.hostKind ?? "local") === hostFilter) && enabledStates.has(agentMapState(card)));
}
function countAgentMapCards(cards) {
	const counts = {
		attention: 0,
		working: 0,
		done: 0,
		idle: 0
	};
	for (const card of cards) counts[agentMapState(card)] += 1;
	return counts;
}
export { filterAgentMapCards as n, countAgentMapCards as t };
