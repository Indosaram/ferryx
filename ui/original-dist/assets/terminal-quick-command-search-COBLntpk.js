import { $a as isClipboardTextByteLengthOverLimit, Ag as isTerminalAgentQuickCommand, Og as getTerminalQuickCommandBody } from "./store-CgXrfmaH.js";
var NO_MATCH = Number.POSITIVE_INFINITY;
const TERMINAL_QUICK_COMMAND_SEARCH_QUERY_MAX_BYTES = 2 * 1024;
function isTerminalQuickCommandSearchQueryTooLarge(query, maxBytes = TERMINAL_QUICK_COMMAND_SEARCH_QUERY_MAX_BYTES) {
	return isClipboardTextByteLengthOverLimit(query, maxBytes);
}
function searchTerminalQuickCommands(commands, rawQuery) {
	if (isTerminalQuickCommandSearchQueryTooLarge(rawQuery)) return [];
	const query = normalizeSearchText(rawQuery);
	if (!query) return [...commands];
	const matches = [];
	commands.forEach((command, index) => {
		const score = scoreQuickCommand(command, query);
		if (score !== NO_MATCH) matches.push({
			command,
			score,
			index
		});
	});
	matches.sort((a, b) => a.score - b.score || a.index - b.index);
	return matches.map((match) => match.command);
}
function scoreQuickCommand(command, query) {
	const body = getTerminalQuickCommandBody(command);
	const scores = [scoreCandidate(query, command.label, 0), scoreCandidate(query, body, 400)];
	if (isTerminalAgentQuickCommand(command)) scores.push(scoreCandidate(query, command.agent, 200));
	return Math.min(...scores);
}
function scoreCandidate(query, rawCandidate, baseScore) {
	const candidate = normalizeSearchText(rawCandidate);
	if (!candidate) return NO_MATCH;
	if (candidate === query) return baseScore;
	if (candidate.startsWith(query)) return baseScore + 50;
	const wordIndex = candidate.indexOf(` ${query}`);
	if (wordIndex !== -1) return baseScore + 100 + wordIndex;
	const index = candidate.indexOf(query);
	if (index !== -1) return baseScore + 200 + index;
	return NO_MATCH;
}
function normalizeSearchText(value) {
	let normalized = "";
	let pendingWhitespace = false;
	for (let index = 0; index < value.length; index += 1) {
		if (isTerminalQuickCommandSearchWhitespace(value.charCodeAt(index))) {
			pendingWhitespace = normalized.length > 0;
			continue;
		}
		if (pendingWhitespace) {
			normalized += " ";
			pendingWhitespace = false;
		}
		normalized += value.charAt(index).toLowerCase();
	}
	return normalized;
}
function isTerminalQuickCommandSearchWhitespace(code) {
	return code === 32 || code >= 9 && code <= 13 || code === 160 || code === 5760 || code >= 8192 && code <= 8202 || code === 8232 || code === 8233 || code === 8239 || code === 8287 || code === 12288 || code === 65279;
}
export { searchTerminalQuickCommands as t };
