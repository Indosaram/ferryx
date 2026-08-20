function blocksCodexPaneInput(notice) {
	return Boolean(notice) && !notice?.dismissed;
}
function awaitsCodexRestartAnswer(notice) {
	return Boolean(notice) && !notice?.dismissed && !notice?.restartRequested;
}
export { blocksCodexPaneInput as n, awaitsCodexRestartAnswer as t };
