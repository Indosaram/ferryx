import { a as translate } from "./jsx-runtime-Cv_nyRjc.js";
import { t as searchKeywords } from "./settings-search-keywords-Cutqc_5t.js";
var AGENT_AWAKE_TITLE_KEY = "auto.components.settings.agent-awake-copy.modeTitle";
var AGENT_AWAKE_DESCRIPTION_WINDOWS_KEY = "auto.components.settings.agent-awake-copy.modeDescriptionWindows";
var AGENT_AWAKE_DESCRIPTION_DEFAULT_KEY = "auto.components.settings.agent-awake-copy.modeDescriptionDefault";
function getAgentAwakeTitle() {
	return translate(AGENT_AWAKE_TITLE_KEY, "Keep computer awake");
}
function getAgentAwakeDescription(userAgent = typeof navigator === "undefined" ? "" : navigator.userAgent) {
	if (userAgent.includes("Windows")) return translate(AGENT_AWAKE_DESCRIPTION_WINDOWS_KEY, "Choose On, Agent, or Off. Agent mode stays awake while agents are working; lid-close behavior follows this device's power settings.");
	return translate(AGENT_AWAKE_DESCRIPTION_DEFAULT_KEY, "Choose On, Agent, or Off. Agent mode stays awake while agents are working. Orca also asks this device to stay awake when the lid is closed, subject to its power policy.");
}
function getAgentAwakeSearchKeywords(userAgent = typeof navigator === "undefined" ? "" : navigator.userAgent) {
	const keywords = searchKeywords([
		{
			key: "auto.components.settings.agents.search.66b6b82eb4",
			fallback: "awake"
		},
		{
			key: "auto.components.settings.agents.search.dbc8aca6b0",
			fallback: "sleep"
		},
		{
			key: "auto.components.settings.agents.search.845ad9128a",
			fallback: "power"
		},
		{
			key: "auto.components.settings.agents.search.96ba2373b6",
			fallback: "agent"
		},
		{
			key: "auto.components.settings.agents.search.48f84d10f1",
			fallback: "running"
		},
		{
			key: "auto.components.settings.agents.search.affbf130f6",
			fallback: "working"
		},
		{
			key: "auto.components.settings.agents.search.0d1c334987",
			fallback: "lid"
		},
		{
			key: "auto.components.settings.agents.search.ff8de8a2ad",
			fallback: "display"
		}
	]);
	return userAgent.includes("Linux") ? [...keywords, ...searchKeywords([{
		key: "auto.components.settings.agents.search.f622b8eb2a",
		fallback: "linux"
	}])] : keywords;
}
export { getAgentAwakeSearchKeywords as n, getAgentAwakeTitle as r, getAgentAwakeDescription as t };
