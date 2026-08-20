function getAutomationLegacyRepoId(automation) {
	return automation.projectId;
}
function getAutomationRunRepoId(automation) {
	return automation.runContext?.repoId ?? getAutomationLegacyRepoId(automation);
}
function formatAutomationPrecheckTimeout(seconds) {
	return `${seconds}s`;
}
function didAutomationPrecheckPass(result) {
	return Boolean(result && !result.timedOut && !result.error && result.exitCode === 0);
}
function formatAutomationPrecheckFailure(result) {
	if (result.timedOut) return `Precheck timed out after ${formatAutomationPrecheckTimeout(Math.max(1, Math.round(result.durationMs / 1e3)))}.`;
	if (result.error) return `Precheck failed: ${result.error}`;
	return `Precheck exited with code ${result.exitCode ?? "unknown"}.`;
}
export { getAutomationRunRepoId as i, formatAutomationPrecheckFailure as n, formatAutomationPrecheckTimeout as r, didAutomationPrecheckPass as t };
