import { lr as hostedReviewInfoFromGitHubPRInfo } from "./store-CgXrfmaH.js";
function gitHubPRToChecksPanelReview(pr) {
	return hostedReviewInfoFromGitHubPRInfo(pr);
}
function selectChecksPanelReview({ hostedReview, pr, linkedGitLabMR, linkedBitbucketPR, linkedAzureDevOpsPR, linkedGiteaPR }) {
	const gitLabHostedReview = hostedReview?.provider === "gitlab" ? hostedReview : null;
	if (gitLabHostedReview) return gitLabHostedReview;
	if (linkedGitLabMR !== null || linkedBitbucketPR !== null || linkedAzureDevOpsPR !== null || linkedGiteaPR !== null) return null;
	return pr ? gitHubPRToChecksPanelReview(pr) : null;
}
export { selectChecksPanelReview as n, gitHubPRToChecksPanelReview as t };
