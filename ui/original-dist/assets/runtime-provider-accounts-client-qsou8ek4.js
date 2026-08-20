import { Ca as normalizeGlobalWindowsRuntimeDefault, lp as RuntimeRpcCallError, op as getActiveRuntimeTarget, rp as callRuntimeRpc } from "./store-CgXrfmaH.js";
var CLI_GATED_ITEMS = new Set([
	"claude",
	"codex",
	"gemini",
	"kimi",
	"antigravity",
	"grok"
]);
function isStatusBarItemAvailable(id, detectedAgentIds) {
	if (!CLI_GATED_ITEMS.has(id)) return true;
	if (detectedAgentIds === null) return true;
	return detectedAgentIds.includes(id);
}
function resolveLocalAccountRuntimeTarget(settings, platform = process.platform) {
	if (settings.localAccountRuntime === "host") return {
		runtime: "host",
		wslDistro: null
	};
	if (settings.localAccountRuntime === "wsl") return {
		runtime: "wsl",
		wslDistro: normalizeDistro(settings.localAccountWslDistro)
	};
	if (platform !== "win32") return {
		runtime: "host",
		wslDistro: null
	};
	const runtimeDefault = normalizeGlobalWindowsRuntimeDefault(settings.localWindowsRuntimeDefault);
	if (runtimeDefault.kind === "wsl") return {
		runtime: "wsl",
		wslDistro: runtimeDefault.distro
	};
	return {
		runtime: "host",
		wslDistro: null
	};
}
function normalizeDistro(value) {
	const trimmed = value?.trim();
	return trimmed ? trimmed : null;
}
var REMOTE_ACCOUNTS_FIRST_SNAPSHOT_TIMEOUT_MS = 15e3;
var REMOTE_ACCOUNT_MUTATION_TIMEOUT_MS = 3e4;
var pendingProviderAccountsSnapshots = /* @__PURE__ */ new Map();
function getProviderAccountsOwnerKey(settings) {
	const target = getActiveRuntimeTarget(settings);
	return target.kind === "local" ? "local" : `environment:${target.environmentId}`;
}
function hasRemoteProviderAccountOwner(settings) {
	return getActiveRuntimeTarget(settings).kind === "environment";
}
function emptyClaudeAccountsState() {
	return {
		accounts: [],
		activeAccountId: null,
		activeAccountIdsByRuntime: {
			host: null,
			wsl: {}
		}
	};
}
function emptyCodexAccountsState() {
	return {
		accounts: [],
		activeAccountId: null,
		activeAccountIdsByRuntime: {
			host: null,
			wsl: {}
		}
	};
}
function providerAccountsLoadError(provider, cause) {
	const message = String(cause?.message ?? cause);
	return /* @__PURE__ */ new Error(`Could not load ${provider} accounts: ${message}`);
}
function watchProviderAccounts(settings, handlers) {
	const target = getActiveRuntimeTarget(settings);
	if (target.kind === "local") {
		let closed$1 = false;
		Promise.allSettled([window.api.claudeAccounts.list(), window.api.codexAccounts.list()]).then(([claudeResult, codexResult]) => {
			if (closed$1) return;
			const claudeError = claudeResult.status === "rejected" ? providerAccountsLoadError("Claude", claudeResult.reason) : null;
			const codexError = codexResult.status === "rejected" ? providerAccountsLoadError("Codex", codexResult.reason) : null;
			if (claudeError && codexError) {
				const errors = [claudeError, codexError];
				handlers.onError(new AggregateError(errors, errors.map((error) => error.message).join(" ")));
				return;
			}
			const failedProviders = [];
			if (claudeError) failedProviders.push("claude");
			if (codexError) failedProviders.push("codex");
			handlers.onSnapshot({
				claude: claudeResult.status === "fulfilled" ? claudeResult.value : emptyClaudeAccountsState(),
				codex: codexResult.status === "fulfilled" ? codexResult.value : emptyCodexAccountsState(),
				rateLimits: null,
				...failedProviders.length > 0 ? { failedProviders } : {}
			});
			for (const error of [claudeError, codexError]) if (error && !closed$1) handlers.onError(error);
		});
		return { close: () => {
			closed$1 = true;
		} };
	}
	let closed = false;
	let unsubscribe = null;
	let receivedSnapshot = false;
	const firstSnapshotTimer = window.setTimeout(() => {
		if (!closed && !receivedSnapshot) handlers.onError(/* @__PURE__ */ new Error("Timed out waiting for remote provider accounts."));
	}, REMOTE_ACCOUNTS_FIRST_SNAPSHOT_TIMEOUT_MS);
	window.api.runtimeEnvironments.subscribe({
		selector: target.environmentId,
		method: "accounts.subscribe",
		timeoutMs: REMOTE_ACCOUNTS_FIRST_SNAPSHOT_TIMEOUT_MS
	}, {
		onResponse: (response) => {
			if (closed) return;
			const typed = response;
			if (typed.ok === false) {
				handlers.onError(new RuntimeRpcCallError(typed));
				return;
			}
			const message = typed.result;
			if ((message.type === "ready" || message.type === "snapshot") && message.snapshot) {
				receivedSnapshot = true;
				handlers.onSnapshot(message.snapshot);
			}
		},
		onError: (error) => {
			if (!closed) handlers.onError(new Error(error.message));
		},
		onClose: () => {
			if (!closed && !receivedSnapshot) handlers.onError(/* @__PURE__ */ new Error("Remote provider account subscription closed."));
		}
	}).then((handle) => {
		unsubscribe = handle.unsubscribe;
		if (closed) unsubscribe();
	}).catch((error) => {
		if (!closed) handlers.onError(error);
	});
	return { close: () => {
		closed = true;
		window.clearTimeout(firstSnapshotTimer);
		unsubscribe?.();
	} };
}
function fetchProviderAccountsSnapshot(settings) {
	const ownerKey = getProviderAccountsOwnerKey(settings);
	const pending = pendingProviderAccountsSnapshots.get(ownerKey);
	if (pending) return pending;
	const request = new Promise((resolve, reject) => {
		const watcher = watchProviderAccounts(settings, {
			onSnapshot: (snapshot) => {
				watcher.close();
				resolve(snapshot);
			},
			onError: (error) => {
				watcher.close();
				reject(error instanceof Error ? error : new Error(String(error)));
			}
		});
	});
	pendingProviderAccountsSnapshots.set(ownerKey, request);
	const clearPending = () => {
		if (pendingProviderAccountsSnapshots.get(ownerKey) === request) pendingProviderAccountsSnapshots.delete(ownerKey);
	};
	request.then(clearPending, clearPending);
	return request;
}
async function selectClaudeProviderAccount(settings, selection) {
	const target = getActiveRuntimeTarget(settings);
	if (target.kind === "environment") return callRuntimeRpc(target, "accounts.selectClaude", { accountId: selection.accountId }, { timeoutMs: REMOTE_ACCOUNT_MUTATION_TIMEOUT_MS });
	return window.api.claudeAccounts.select(selection);
}
async function selectCodexProviderAccount(settings, selection) {
	const target = getActiveRuntimeTarget(settings);
	if (target.kind === "environment") return callRuntimeRpc(target, "accounts.selectCodex", { accountId: selection.accountId }, { timeoutMs: REMOTE_ACCOUNT_MUTATION_TIMEOUT_MS });
	return window.api.codexAccounts.select(selection);
}
async function removeClaudeProviderAccount(settings, accountId) {
	const target = getActiveRuntimeTarget(settings);
	if (target.kind === "environment") return callRuntimeRpc(target, "accounts.removeClaude", { accountId }, { timeoutMs: REMOTE_ACCOUNT_MUTATION_TIMEOUT_MS });
	return window.api.claudeAccounts.remove({ accountId });
}
async function removeCodexProviderAccount(settings, accountId) {
	const target = getActiveRuntimeTarget(settings);
	if (target.kind === "environment") return callRuntimeRpc(target, "accounts.removeCodex", { accountId }, { timeoutMs: REMOTE_ACCOUNT_MUTATION_TIMEOUT_MS });
	return window.api.codexAccounts.remove({ accountId });
}
export { removeClaudeProviderAccount as a, selectCodexProviderAccount as c, isStatusBarItemAvailable as d, hasRemoteProviderAccountOwner as i, watchProviderAccounts as l, emptyCodexAccountsState as n, removeCodexProviderAccount as o, fetchProviderAccountsSnapshot as r, selectClaudeProviderAccount as s, emptyClaudeAccountsState as t, resolveLocalAccountRuntimeTarget as u };
