import { st as parseExecutionHostId } from "./agent-status-3vUKbY6l.js";
function isDesktopOwnedHost(hostId) {
	return parseExecutionHostId(hostId)?.kind !== "runtime";
}
function getRepoBackedProviderToolStatus(provider, preflightStatus) {
	if (!preflightStatus) return null;
	if (provider === "github") return preflightStatus.gh;
	return Object.hasOwn(preflightStatus, "glab") ? preflightStatus.glab ?? {
		installed: false,
		authenticated: false
	} : "unsupported";
}
function getProviderReason(status) {
	if (status === "unsupported") return "unsupported-provider";
	if (!status.installed) return "unavailable-source-tool";
	if (!status.authenticated) return "missing-provider-auth";
	return null;
}
function getRepoBackedProviderAvailability(args) {
	return args.contexts.flatMap((context) => {
		const hostPreflight = isDesktopOwnedHost(context.hostId) ? {
			checked: args.preflightReady,
			status: args.preflightStatus
		} : args.runtimePreflightStatusByHostId?.get(context.hostId);
		if (!hostPreflight?.checked) return [];
		const status = getRepoBackedProviderToolStatus(args.provider, hostPreflight.status);
		const reason = status ? getProviderReason(status) : null;
		return reason ? [{
			hostId: context.hostId,
			reason
		}] : [];
	});
}
export { getRepoBackedProviderAvailability as t };
