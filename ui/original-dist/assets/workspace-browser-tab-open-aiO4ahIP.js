import { a as translate } from "./jsx-runtime-Cv_nyRjc.js";
import { Mu as resolveWorktreeOperationRoute, O as getClientCreationActionPolicy, Pm as FLOATING_TERMINAL_WORKTREE_ID, ei as SEARCH_ENGINE_LABELS, lm as BROWSER_SCREENCAST_RUNTIME_CAPABILITY, t as useAppStore } from "./store-CgXrfmaH.js";
import { X as LOCAL_EXECUTION_HOST_ID, st as parseExecutionHostId } from "./agent-status-3vUKbY6l.js";
import { l as createWebRuntimeSessionBrowserTab } from "./web-runtime-session-CN2syA39.js";
function isExpectedRuntimeBrowserRoute(state, availability, route, workspaceId, expectedRuntimeEnvironmentId) {
	if (availability.state !== "enabled" || workspaceId === "global-floating-terminal" || !route) return false;
	const expectedEnvironmentId = expectedRuntimeEnvironmentId.trim();
	const environmentId = route.runtimeEnvironmentId?.trim() || null;
	const capabilities = state.runtimeStatusByEnvironmentId?.get(expectedEnvironmentId)?.status?.capabilities;
	if (environmentId !== expectedEnvironmentId || !capabilities?.includes("browser.screencast.v1")) return false;
	const host = parseExecutionHostId(route.executionHostId);
	return !route.executionHostId || Boolean(host && (host.kind !== "runtime" || host.environmentId === environmentId));
}
function canOpenWorkspaceBrowserTabOnRuntime(state, workspaceId, expectedRuntimeEnvironmentId) {
	const availability = getClientCreationActionPolicy(state, workspaceId)["managed-browser"];
	return isExpectedRuntimeBrowserRoute(state, availability, resolveWorktreeOperationRoute(state, workspaceId), workspaceId, expectedRuntimeEnvironmentId);
}
function urlTabTitle(url) {
	try {
		const parsed = new URL(url);
		return `${parsed.host}${parsed.pathname === "/" ? "" : parsed.pathname}`;
	} catch {
		return null;
	}
}
function intentPresentation(intent, url) {
	if (intent.kind === "url") return {
		error: translate("auto.lib.workspace.browser.tab.open.urlFailed", "Unable to open URL."),
		title: urlTabTitle(url) ?? translate("auto.components.tab.bar.TabBarCreateEntry.7cdf8ee0c8", "Open URL")
	};
	const engine = SEARCH_ENGINE_LABELS[intent.engine];
	return {
		error: translate("auto.lib.workspace.browser.tab.open.searchFailed", "Unable to search with {{value0}}.", { value0: engine }),
		title: translate("auto.components.tab.bar.TabBarCreateEntry.searchProvider", "Search {{value0}}", { value0: engine })
	};
}
function openFailure(message, reason, cause) {
	console.warn(`[workspace-browser-tab-open] ${reason}`);
	return new Error(message, { cause: new Error(reason, cause === void 0 ? void 0 : { cause }) });
}
function validateTarget(url) {
	try {
		const parsed = new URL(url);
		return (parsed.protocol === "http:" || parsed.protocol === "https:") && !!parsed.hostname;
	} catch {
		return false;
	}
}
function createClientBrowserTab(state, request, hostId, presentation) {
	try {
		state.createBrowserTab(request.workspaceId, request.url, {
			activate: true,
			browserRuntimeEnvironmentId: null,
			focusAddressBar: false,
			sessionProfileId: state.defaultBrowserSessionProfileIdByHostId[hostId] ?? state.defaultBrowserSessionProfileId,
			targetGroupId: request.targetGroupId,
			title: presentation.title
		});
	} catch (error) {
		throw openFailure(presentation.error, "client tab creation rejected", error);
	}
}
function assertManagedBrowserEnabled(availability, presentation) {
	if (availability.state !== "enabled") throw openFailure(presentation.error, availability.reason);
}
async function openWorkspaceBrowserTab(request) {
	const presentation = intentPresentation(request.intent, request.url);
	if (!validateTarget(request.url)) throw openFailure(presentation.error, "target is not an http(s) URL");
	const state = useAppStore.getState();
	const availability = getClientCreationActionPolicy(state, request.workspaceId)["managed-browser"];
	assertManagedBrowserEnabled(availability, presentation);
	const route = resolveWorktreeOperationRoute(state, request.workspaceId);
	if (!route) throw openFailure(presentation.error, "no active worktree route");
	const environmentId = route.runtimeEnvironmentId?.trim() || null;
	const expectedEnvironmentId = request.expectedRuntimeEnvironmentId === void 0 ? null : request.expectedRuntimeEnvironmentId.trim();
	if (expectedEnvironmentId !== null && !isExpectedRuntimeBrowserRoute(state, availability, route, request.workspaceId, expectedEnvironmentId)) throw openFailure(presentation.error, "asserted runtime cannot provide this managed browser");
	const host = parseExecutionHostId(route.executionHostId);
	if (!environmentId) {
		if (!host || host.kind === "runtime") throw openFailure(presentation.error, `unresolved client host: ${route.executionHostId}`);
		createClientBrowserTab(state, request, host.id, presentation);
		return;
	}
	if (route.executionHostId && (!host || host.kind === "runtime" && host.environmentId !== environmentId)) throw openFailure(presentation.error, `host ${route.executionHostId} does not own runtime ${environmentId}`);
	if (availability.provider === "local-client") {
		createClientBrowserTab(state, request, host && host.kind !== "runtime" ? host.id : LOCAL_EXECUTION_HOST_ID, presentation);
		return;
	}
	let created = false;
	try {
		created = await createWebRuntimeSessionBrowserTab({
			worktreeId: request.workspaceId,
			environmentId,
			url: request.url,
			targetGroupId: request.targetGroupId,
			...expectedEnvironmentId !== null ? { waitForRegistration: true } : {},
			selectWorktree: true,
			stagedTitle: presentation.title,
			stagedFocusAddressBar: false,
			failureLogMode: "operation-only"
		});
	} catch (error) {
		throw openFailure(presentation.error, "runtime browser tab creation failed", error);
	}
	if (!created) throw openFailure(presentation.error, "runtime browser tab creation was unavailable");
}
export { openWorkspaceBrowserTab as n, canOpenWorkspaceBrowserTabOnRuntime as t };
