import { lu as isUtf8ByteLengthWithinLimit, op as getActiveRuntimeTarget, rp as callRuntimeRpc } from "./store-CgXrfmaH.js";
import { t as legacyBaseRefSearchResult } from "./base-ref-search-result-BRRCxq1e.js";
const RUNTIME_REPO_REF_SEARCH_QUERY_MAX_BYTES = 2 * 1024;
function isRuntimeRepoRefSearchQueryWithinLimit(query, maxBytes = RUNTIME_REPO_REF_SEARCH_QUERY_MAX_BYTES) {
	return isUtf8ByteLengthWithinLimit(query, maxBytes);
}
async function getRuntimeRepoBaseRefDefault(settings, repoId, hostId) {
	const target = getActiveRuntimeTarget(settings);
	if (target.kind !== "environment") return window.api.repos.getBaseRefDefault({
		repoId,
		...hostId ? { hostId } : {}
	});
	return callRuntimeRpc(target, "repo.baseRefDefault", { repo: repoId }, { timeoutMs: 15e3 });
}
async function searchRuntimeRepoBaseRefs(settings, repoId, query, limit, hostId) {
	if (!isRuntimeRepoRefSearchQueryWithinLimit(query)) return [];
	const target = getActiveRuntimeTarget(settings);
	if (target.kind !== "environment") return window.api.repos.searchBaseRefs({
		repoId,
		query,
		limit,
		...hostId ? { hostId } : {}
	});
	return (await callRuntimeRpc(target, "repo.searchRefs", {
		repo: repoId,
		query,
		limit
	}, { timeoutMs: 15e3 })).refs;
}
async function searchRuntimeRepoBaseRefDetails(settings, repoId, query, limit, hostId) {
	if (!isRuntimeRepoRefSearchQueryWithinLimit(query)) return [];
	const target = getActiveRuntimeTarget(settings);
	if (target.kind !== "environment") return window.api.repos.searchBaseRefDetails({
		repoId,
		query,
		limit,
		...hostId ? { hostId } : {}
	});
	const result = await callRuntimeRpc(target, "repo.searchRefs", {
		repo: repoId,
		query,
		limit
	}, { timeoutMs: 15e3 });
	return result.refDetails ?? result.refs.map(legacyBaseRefSearchResult);
}
export { isRuntimeRepoRefSearchQueryWithinLimit as i, searchRuntimeRepoBaseRefDetails as n, searchRuntimeRepoBaseRefs as r, getRuntimeRepoBaseRefDefault as t };
