var TUI_AGENT_KIND_BY_AGENT = {
	claude: "claude-code",
	"claude-agent-teams": "claude-agent-teams",
	openclaude: "openclaude",
	codex: "codex",
	autohand: "autohand",
	opencode: "opencode",
	"mimo-code": "mimo-code",
	pi: "pi",
	omp: "omp",
	"prime-agent": "prime-agent",
	gemini: "gemini",
	antigravity: "antigravity",
	aider: "aider",
	goose: "goose",
	amp: "amp",
	kilo: "kilo",
	kiro: "kiro",
	crush: "crush",
	aug: "aug",
	cline: "cline",
	codebuff: "codebuff",
	"command-code": "command-code",
	continue: "continue",
	cursor: "cursor",
	droid: "droid",
	kimi: "kimi",
	"mistral-vibe": "mistral-vibe",
	"qwen-code": "qwen-code",
	rovo: "rovo",
	hermes: "hermes",
	openclaw: "openclaw",
	copilot: "copilot",
	grok: "grok",
	devin: "devin",
	ante: "ante",
	trae: "trae"
};
function tuiAgentToAgentKind(agent) {
	return TUI_AGENT_KIND_BY_AGENT[agent] ?? "other";
}
var AGENT_BY_TUI_AGENT_KIND = Object.fromEntries(Object.entries(TUI_AGENT_KIND_BY_AGENT).map(([agent, kind]) => [kind, agent]));
function agentKindToTuiAgent(kind) {
	if (!kind) return null;
	return AGENT_BY_TUI_AGENT_KIND[kind] ?? null;
}
export { tuiAgentToAgentKind as n, agentKindToTuiAgent as t };
