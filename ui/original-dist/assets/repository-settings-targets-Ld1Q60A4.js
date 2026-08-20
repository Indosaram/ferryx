function getRepositoryLocalCommandsSectionId(repoId) {
	return `repo-${repoId}-local-commands`;
}
function getRepositoryIconSectionId(repoId) {
	return `repo-${repoId}-icon`;
}
function getRepositorySourceControlAiSectionId(repoId) {
	return `repo-${repoId}-source-control-ai`;
}
function getRepositorySourceControlAiActionRecipeSectionId(repoId, actionId) {
	return `repo-${repoId}-source-control-ai-${actionId}`;
}
export { getRepositorySourceControlAiSectionId as i, getRepositoryLocalCommandsSectionId as n, getRepositorySourceControlAiActionRecipeSectionId as r, getRepositoryIconSectionId as t };
