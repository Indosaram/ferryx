var cachedMigrationUnsupportedEntries = /* @__PURE__ */ new WeakMap();
function migrationUnsupportedToAgentStatusEntry(entry) {
	const cached = cachedMigrationUnsupportedEntries.get(entry);
	if (cached !== void 0) return cached;
	const converted = !entry.paneKey ? null : {
		state: "blocked",
		prompt: "Agent unavailable after pane identity migration",
		updatedAt: Number.MAX_SAFE_INTEGER,
		stateStartedAt: entry.updatedAt,
		agentType: "unknown",
		paneKey: entry.paneKey,
		terminalTitle: "Migration unsupported",
		stateHistory: [],
		lastAssistantMessage: "Restart this terminal so Orca can attach a stable UUID pane key to agent hooks."
	};
	cachedMigrationUnsupportedEntries.set(entry, converted);
	return converted;
}
export { migrationUnsupportedToAgentStatusEntry as t };
