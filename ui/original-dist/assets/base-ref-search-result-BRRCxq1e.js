var LEGACY_REMOTE_REF_PREFIXES = ["origin/", "upstream/"];
function deriveLegacyLocalBranchName(refName) {
	for (const prefix of LEGACY_REMOTE_REF_PREFIXES) if (refName.startsWith(prefix) && refName.length > prefix.length) return refName.slice(prefix.length);
	return refName;
}
function legacyBaseRefSearchResult(refName) {
	return {
		refName,
		localBranchName: deriveLegacyLocalBranchName(refName)
	};
}
export { legacyBaseRefSearchResult as t };
