import { a as track } from "./telemetry-ZyUPyKMD.js";
function getOrcaCliFeatureTipTelemetrySource(value) {
	return value === "app_open" ? "app_open" : "manual";
}
function trackOrcaCliFeatureTipShown(source) {
	track("orca_cli_feature_tip_shown", { source });
}
function trackOrcaCliFeatureTipSetupClicked(source) {
	track("orca_cli_feature_tip_setup_clicked", { source });
}
function trackOrcaCliFeatureTipSetupResult(source, result) {
	track("orca_cli_feature_tip_setup_result", {
		source,
		result
	});
}
function trackCmdJPaletteFeatureTipShown(source) {
	track("cmd_j_palette_feature_tip_shown", { source });
}
function trackCmdJPaletteFeatureTipAcknowledged(source) {
	track("cmd_j_palette_feature_tip_acknowledged", { source });
}
export { trackOrcaCliFeatureTipSetupResult as a, trackOrcaCliFeatureTipSetupClicked as i, trackCmdJPaletteFeatureTipAcknowledged as n, trackOrcaCliFeatureTipShown as o, trackCmdJPaletteFeatureTipShown as r, getOrcaCliFeatureTipTelemetrySource as t };
