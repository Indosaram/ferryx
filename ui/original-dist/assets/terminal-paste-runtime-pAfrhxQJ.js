import { Hp as parseWslUncPath } from "./store-CgXrfmaH.js";
var REMOTE_PTY_ID_PREFIX = "remote:";
function resolveTerminalPasteRuntime({ platform, ptyId, connectionId, remotePlatform, transport, isWindowsConpty }) {
	const windowsConpty = isWindowsConpty === void 0 ? {} : { isWindowsConpty };
	if (isRemoteRuntimePastePtyId(ptyId)) return {
		platform,
		runtimeKey: `remote:${ptyId}`,
		kind: "remote-runtime",
		...windowsConpty
	};
	const transportConnectionId = transport?.getConnectionId?.();
	const effectiveConnectionId = transportConnectionId === void 0 ? connectionId ?? null : transportConnectionId;
	if (effectiveConnectionId) return {
		platform: transport?.getRemotePlatform?.() ?? remotePlatform ?? platform,
		runtimeKey: `ssh:${effectiveConnectionId}`,
		kind: "ssh",
		...windowsConpty
	};
	const wslRuntimeKey = resolveWslRuntimeKey(transport?.getLocalSessionMetadata?.());
	if (wslRuntimeKey) return {
		platform,
		runtimeKey: wslRuntimeKey,
		kind: "wsl",
		...windowsConpty
	};
	return {
		platform,
		runtimeKey: `local:${platform}`,
		kind: "local",
		...windowsConpty
	};
}
function isRemoteRuntimePastePtyId(ptyId) {
	return typeof ptyId === "string" && ptyId.startsWith(REMOTE_PTY_ID_PREFIX);
}
function resolveWslRuntimeKey(metadata) {
	const parsedCwd = metadata?.cwd ? parseWslUncPath(metadata.cwd) : null;
	if (parsedCwd?.distro) return `wsl:${parsedCwd.distro}`;
	if (isWslShellOverride(metadata?.shellOverride)) return "wsl:default";
	return null;
}
function isWslShellOverride(shellOverride) {
	const executable = getShellOverrideExecutableToken(shellOverride);
	const segmentStart = getShellOverridePathSegmentStart(executable);
	const name = executable.slice(segmentStart).toLowerCase();
	return name === "wsl" || name === "wsl.exe";
}
function getShellOverrideExecutableToken(shellOverride) {
	const value = shellOverride ?? "";
	let index = 0;
	while (index < value.length && isShellOverrideWhitespace(value.charCodeAt(index))) index += 1;
	if (index >= value.length) return "";
	const quote = value[index];
	if (quote === "\"" || quote === "'") {
		const tokenStart$1 = index + 1;
		for (let end = tokenStart$1; end < value.length; end += 1) if (value[end] === quote) return value.slice(tokenStart$1, end);
		return value.slice(tokenStart$1);
	}
	const tokenStart = index;
	while (index < value.length && !isShellOverrideWhitespace(value.charCodeAt(index))) index += 1;
	return value.slice(tokenStart, index);
}
function getShellOverridePathSegmentStart(token) {
	for (let index = token.length - 1; index >= 0; index -= 1) {
		const code = token.charCodeAt(index);
		if (code === 47 || code === 92) return index + 1;
	}
	return 0;
}
function isShellOverrideWhitespace(code) {
	return code === 32 || code >= 9 && code <= 13;
}
export { resolveTerminalPasteRuntime as n, isWslShellOverride as t };
