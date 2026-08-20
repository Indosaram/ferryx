import { Vp as isWslUncPath } from "./store-CgXrfmaH.js";
import { X as LOCAL_EXECUTION_HOST_ID } from "./agent-status-3vUKbY6l.js";
function isWindowsUserAgent(userAgent) {
	return userAgent?.includes("Windows") ?? false;
}
function isWslCwd(cwd) {
	return isWslUncPath(cwd ?? "");
}
function isWslShellOverride(shellOverride) {
	return /(?:^|[/\\])wsl(?:\.exe)?$/i.test(shellOverride ?? "");
}
function parseWindowsBuildNumber(osRelease) {
	const build = osRelease?.split(".")[2];
	if (!build) return;
	const parsed = Number.parseInt(build, 10);
	return Number.isFinite(parsed) && parsed > 0 ? parsed : void 0;
}
function buildXtermWindowsPtyOptions(buildNumber) {
	if (buildNumber === void 0 || buildNumber < 21376) return { backend: "conpty" };
	return {
		backend: "conpty",
		buildNumber
	};
}
function buildWindowsPtyCompatibilityOptions(context) {
	if (!isLocalNativeWindowsConpty(context)) return {};
	return buildLocalConptyTerminalOptions(context.osRelease);
}
function buildLocalConptyTerminalOptions(osRelease) {
	return { windowsPty: buildXtermWindowsPtyOptions(parseWindowsBuildNumber(osRelease)) };
}
function resolveWindowsShellOverride(tabShellOverride, globalWindowsShell) {
	return tabShellOverride ?? globalWindowsShell ?? void 0;
}
function isLocalNativeWindowsPty(context) {
	return isWindowsUserAgent(context.userAgent) && context.connectionId === null && !isWslCwd(context.cwd) && !isWslShellOverride(context.shellOverride);
}
function isLocalNativeWindowsConpty(context) {
	return context.executionHostId === "local" && isLocalNativeWindowsPty(context);
}
export { resolveWindowsShellOverride as i, buildWindowsPtyCompatibilityOptions as n, isLocalNativeWindowsConpty as r, buildLocalConptyTerminalOptions as t };
