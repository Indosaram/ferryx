import { a as translate } from "./jsx-runtime-Cv_nyRjc.js";
import { n as toast } from "./dist-DgqligFk.js";
async function copyArtifactLink(shareUrl, options = {}) {
	try {
		await window.api.ui.writeClipboardText(shareUrl);
		if (options.showSuccessToast !== false) toast.success(translate("auto.components.artifacts.copySuccess", "Artifact link copied"));
		return true;
	} catch {
		toast.error(translate("auto.components.artifacts.copyFailed", "Could not copy artifact link"));
		return false;
	}
}
function openArtifactInBrowser(shareUrl) {
	window.api.shell.openUrl(shareUrl);
}
export { openArtifactInBrowser as n, copyArtifactLink as t };
