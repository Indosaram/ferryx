import { $a as isClipboardTextByteLengthOverLimit } from "./store-CgXrfmaH.js";
var PATH_SCORE_OFFSET = 1e3;
const REPO_SEARCH_QUERY_MAX_BYTES = 2 * 1024;
function isRepoSearchQueryTooLarge(query, maxBytes = REPO_SEARCH_QUERY_MAX_BYTES) {
	return isClipboardTextByteLengthOverLimit(query, maxBytes);
}
function matchScore(repo, query) {
	const displayNameIndex = repo.displayName.toLowerCase().indexOf(query);
	if (displayNameIndex !== -1) return displayNameIndex;
	const pathIndex = repo.path.toLowerCase().indexOf(query);
	if (pathIndex !== -1) return PATH_SCORE_OFFSET + pathIndex;
	return null;
}
function searchRepos(repos, rawQuery) {
	if (isRepoSearchQueryTooLarge(rawQuery)) return [];
	const query = rawQuery.trim().toLowerCase();
	if (!query) return repos;
	const matches = [];
	for (const [index, repo] of repos.entries()) {
		const score = matchScore(repo, query);
		if (score !== null) matches.push({
			repo,
			score,
			index
		});
	}
	matches.sort((a, b) => a.score - b.score || a.index - b.index);
	return matches.map((match) => match.repo);
}
export { searchRepos as n, isRepoSearchQueryTooLarge as t };
