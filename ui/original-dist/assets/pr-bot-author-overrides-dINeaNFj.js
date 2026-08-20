const MAX_PR_BOT_AUTHOR_OVERRIDES = 500;
function normalizePRCommentAuthorLogin(author) {
	if (author.length > 255) return "";
	return author.trim().toLowerCase();
}
function createBotAuthorOverrideSet(logins) {
	const set = /* @__PURE__ */ new Set();
	const iterator = (logins ?? [])[Symbol.iterator]();
	let inspected = 0;
	while (inspected < 500) {
		const next = iterator.next();
		if (next.done) break;
		inspected += 1;
		const login = next.value;
		if (typeof login !== "string") continue;
		const normalized = normalizePRCommentAuthorLogin(login);
		if (normalized) set.add(normalized);
	}
	return set;
}
function normalizePRBotAuthorOverrides(value) {
	if (!Array.isArray(value)) return [];
	return [...createBotAuthorOverrideSet(value)].sort();
}
function applyPRBotAuthorOverride(current, author, isBot) {
	const overrides = new Set(createBotAuthorOverrideSet(current));
	const normalized = normalizePRCommentAuthorLogin(author);
	if (!normalized || overrides.has(normalized) === isBot) return [...overrides].sort();
	if (isBot) {
		if (overrides.size >= 500) return [...overrides].sort();
		overrides.add(normalized);
	} else overrides.delete(normalized);
	return [...overrides].sort();
}
export { normalizePRCommentAuthorLogin as a, normalizePRBotAuthorOverrides as i, applyPRBotAuthorOverride as n, createBotAuthorOverrideSet as r, MAX_PR_BOT_AUTHOR_OVERRIDES as t };
