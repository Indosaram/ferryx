function getTaskPresetQuery(presetId) {
	switch (presetId) {
		case "all":
		case "issues": return "is:issue is:open";
		case "my-issues": return "assignee:@me is:issue is:open";
		case "prs": return "is:pr is:open";
		case "my-prs": return "author:@me is:pr is:open";
		case "review": return "review-requested:@me is:pr is:open";
		case null: return "is:issue is:open";
	}
}
function shouldSuppressEnterSubmit(event, isTextarea) {
	return event.isComposing || isTextarea && event.shiftKey;
}
function shouldAllowComposerEnterSubmitTarget(target, composer) {
	if (!(target instanceof HTMLElement)) return false;
	if (composer?.contains(target)) return true;
	return composer ? target.contains(composer) : false;
}
export { shouldSuppressEnterSubmit as n, getTaskPresetQuery as r, shouldAllowComposerEnterSubmitTarget as t };
