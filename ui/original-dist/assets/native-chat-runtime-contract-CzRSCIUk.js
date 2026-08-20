const RUNTIME_NATIVE_CHAT_READ_ERROR = "Couldn't read agent chat from the remote runtime.";
function parseRuntimeNativeChatTurnLifecycle(value) {
	if (typeof value !== "object" || value === null) return;
	const record = value;
	if (record.state !== "working" && record.state !== "completed" && record.state !== "interrupted" || typeof record.turnId !== "string" || record.turnId.trim().length === 0 || record.timestamp !== null && record.timestamp !== void 0 && (typeof record.timestamp !== "number" || !Number.isFinite(record.timestamp) || record.timestamp <= 0)) return;
	return {
		state: record.state,
		turnId: record.turnId.trim(),
		timestamp: record.timestamp ?? null
	};
}
function parseRuntimeNativeChatReadSessionResult(value) {
	if (typeof value !== "object" || value === null) return { error: RUNTIME_NATIVE_CHAT_READ_ERROR };
	const record = value;
	if (Array.isArray(record.messages)) {
		const lifecycle = parseRuntimeNativeChatTurnLifecycle(record.lifecycle);
		return {
			messages: record.messages,
			...lifecycle ? { lifecycle } : {}
		};
	}
	if (typeof record.error === "string") return {
		error: record.error,
		...record.notFound === true ? { notFound: true } : {}
	};
	return { error: RUNTIME_NATIVE_CHAT_READ_ERROR };
}
export { parseRuntimeNativeChatReadSessionResult as n, parseRuntimeNativeChatTurnLifecycle as r, RUNTIME_NATIVE_CHAT_READ_ERROR as t };
