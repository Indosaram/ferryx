const COMPUTER_AWAKE_MODES = [
	"on",
	"off",
	"auto"
];
function normalizeComputerAwakeMode(mode, legacyAutoEnabled) {
	const explicitMode = COMPUTER_AWAKE_MODES.includes(mode) ? mode : null;
	if (!explicitMode) return legacyAutoEnabled === true ? "auto" : "off";
	if (typeof legacyAutoEnabled === "boolean" && legacyAutoEnabled !== (explicitMode !== "off")) return legacyAutoEnabled ? "auto" : "off";
	return explicitMode;
}
function computerAwakeSettingsForMode(mode) {
	return {
		computerAwakeMode: mode,
		keepComputerAwakeWhileAgentsRun: mode !== "off"
	};
}
export { normalizeComputerAwakeMode as n, computerAwakeSettingsForMode as t };
