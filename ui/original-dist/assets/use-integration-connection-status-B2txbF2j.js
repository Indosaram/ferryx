import { ma as localPreflightContextKey, rd as getProviderRuntimeContextKey, t as useAppStore, ua as getLocalPreflightContext } from "./store-CgXrfmaH.js";
function deriveIntegrationStepStates(input) {
	const review = input.reviewConnected ? "done" : "active";
	const taskResolved = input.trackerConnected || input.reviewConnected && input.codeHostTaskConnected;
	return {
		review,
		task: taskResolved ? "done" : input.reviewConnected ? "active" : "upcoming",
		complete: input.reviewConnected && taskResolved
	};
}
function deriveIntegrationFlowState(input) {
	const trackerConnected = input.trackerProviderName !== null;
	const codeHostTaskReady = input.codeHostTaskProviderName !== null && !input.trackerChecking;
	const stepStates = deriveIntegrationStepStates({
		reviewConnected: input.reviewConnected,
		trackerConnected,
		codeHostTaskConnected: codeHostTaskReady
	});
	return {
		...stepStates,
		taskResolved: stepStates.task === "done"
	};
}
function isBitbucketReviewConnected(status) {
	return status?.configured === true && status.authenticated === true;
}
function isAzureDevOpsReviewConfigured(status) {
	if (status?.configured !== true) return false;
	if (status.tokenConfigured === true && status.baseUrl && status.authenticated !== true) return false;
	return true;
}
function isGiteaReviewConfigured(status) {
	if (status?.configured !== true) return false;
	if (status.tokenConfigured === true && status.authenticated !== true) return false;
	return true;
}
function deriveIntegrationConnectionStatus(facts) {
	const preflightCurrent = facts.preflightStatusContextKey === facts.expectedPreflightContextKey;
	const reviewChecking = facts.preflightStatusLoading || !facts.preflightStatusChecked || !preflightCurrent;
	const reviewReadyForConnection = !reviewChecking && facts.preflightStatusError === null;
	const githubConnected = reviewReadyForConnection && facts.preflightStatus?.gh?.installed === true && facts.preflightStatus.gh.authenticated === true;
	const gitlabConnected = reviewReadyForConnection && facts.preflightStatus?.glab?.installed === true && facts.preflightStatus.glab.authenticated === true;
	const bitbucketConnected = reviewReadyForConnection && isBitbucketReviewConnected(facts.preflightStatus?.bitbucket);
	const azureDevOpsConnected = reviewReadyForConnection && isAzureDevOpsReviewConfigured(facts.preflightStatus?.azureDevOps);
	const giteaConnected = reviewReadyForConnection && isGiteaReviewConfigured(facts.preflightStatus?.gitea);
	const linearStatusCurrent = facts.linearStatusContextKey === facts.providerRuntimeContextKey;
	const jiraStatusCurrent = facts.jiraStatusContextKey === facts.providerRuntimeContextKey;
	const linearChecking = !linearStatusCurrent || !facts.linearStatusChecked;
	const jiraChecking = !jiraStatusCurrent || !facts.jiraStatusChecked;
	const linearConnected = !linearChecking && linearStatusCurrent && facts.linearStatus.connected === true;
	const jiraConnected = !jiraChecking && jiraStatusCurrent && facts.jiraStatus.connected === true;
	const reviewProviderName = githubConnected ? "GitHub" : gitlabConnected ? "GitLab" : bitbucketConnected ? "Bitbucket" : azureDevOpsConnected ? "Azure DevOps" : giteaConnected ? "Gitea" : null;
	const codeHostTaskProviderName = githubConnected ? "GitHub" : gitlabConnected ? "GitLab" : null;
	const trackerProviderName = linearConnected ? "Linear" : jiraConnected ? "Jira" : null;
	const taskSourceNames = [
		...linearConnected ? ["Linear"] : [],
		...jiraConnected ? ["Jira"] : [],
		...githubConnected ? ["GitHub"] : [],
		...gitlabConnected ? ["GitLab"] : []
	];
	const hasUsableTaskSource = taskSourceNames.length > 0;
	const trackerChecking = trackerProviderName === null && (linearChecking || jiraChecking);
	return {
		reviewConnected: githubConnected || gitlabConnected || bitbucketConnected || azureDevOpsConnected || giteaConnected,
		reviewProviderName,
		codeHostTaskProviderName,
		trackerConnected: hasUsableTaskSource,
		trackerProviderName,
		taskSourceNames,
		reviewChecking,
		trackerChecking,
		checking: !hasUsableTaskSource && (reviewChecking || trackerChecking)
	};
}
function useIntegrationConnectionStatus() {
	const preflightStatus = useAppStore((s) => s.preflightStatus);
	const preflightStatusChecked = useAppStore((s) => s.preflightStatusChecked);
	const preflightStatusContextKey = useAppStore((s) => s.preflightStatusContextKey);
	const preflightStatusError = useAppStore((s) => s.preflightStatusError);
	const preflightStatusLoading = useAppStore((s) => s.preflightStatusLoading);
	const linearStatus = useAppStore((s) => s.linearStatus);
	const linearStatusChecked = useAppStore((s) => s.linearStatusChecked);
	const linearStatusContextKey = useAppStore((s) => s.linearStatusContextKey);
	const jiraStatus = useAppStore((s) => s.jiraStatus);
	const jiraStatusChecked = useAppStore((s) => s.jiraStatusChecked);
	const jiraStatusContextKey = useAppStore((s) => s.jiraStatusContextKey);
	const settings = useAppStore((s) => s.settings);
	return deriveIntegrationConnectionStatus({
		preflightStatus,
		preflightStatusChecked,
		preflightStatusContextKey,
		preflightStatusError,
		preflightStatusLoading,
		expectedPreflightContextKey: useAppStore((s) => localPreflightContextKey(getLocalPreflightContext(s))),
		linearStatus,
		linearStatusChecked,
		linearStatusContextKey,
		jiraStatus,
		jiraStatusChecked,
		jiraStatusContextKey,
		providerRuntimeContextKey: getProviderRuntimeContextKey(settings)
	});
}
export { deriveIntegrationFlowState as n, useIntegrationConnectionStatus as r, deriveIntegrationConnectionStatus as t };
