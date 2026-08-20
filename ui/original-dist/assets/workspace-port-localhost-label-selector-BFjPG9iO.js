import { r as activateAndRevealWorktree } from "./worktree-activation-BDsaiyMf.js";
import { $f as toRuntimeWorktreeSelector, T as RUNTIME_BROWSER_UNAVAILABLE_MESSAGE, lm as BROWSER_SCREENCAST_RUNTIME_CAPABILITY, lp as RuntimeRpcCallError, np as assertRuntimeEnvironmentCapability, rp as callRuntimeRpc, t as useAppStore } from "./store-CgXrfmaH.js";
import { st as parseExecutionHostId } from "./agent-status-3vUKbY6l.js";
var WORKSPACE_PORT_PLATFORMS = new Set([
	"aix",
	"android",
	"cygwin",
	"darwin",
	"freebsd",
	"haiku",
	"linux",
	"netbsd",
	"openbsd",
	"sunos",
	"unknown",
	"win32"
]);
var WORKSPACE_PORT_PROTOCOLS = new Set([
	"http",
	"https",
	"unknown"
]);
function isRecord(value) {
	return value !== null && typeof value === "object";
}
function isOptionalString(value) {
	return value === void 0 || typeof value === "string";
}
function isOptionalFiniteNumber(value) {
	return value === void 0 || typeof value === "number" && Number.isFinite(value);
}
function isWorkspacePortOwner(value) {
	if (!isRecord(value)) return false;
	return typeof value.worktreeId === "string" && typeof value.repoId === "string" && typeof value.displayName === "string" && typeof value.path === "string" && (value.confidence === "cwd" || value.confidence === "command" || value.confidence === "none");
}
function isWorkspacePort(value) {
	if (!isRecord(value) || typeof value.id !== "string" || typeof value.bindHost !== "string" || typeof value.connectHost !== "string" || typeof value.port !== "number" || !Number.isFinite(value.port) || !isOptionalFiniteNumber(value.pid) || !isOptionalString(value.processName) || !WORKSPACE_PORT_PROTOCOLS.has(value.protocol)) return false;
	if (value.kind === "workspace") return isWorkspacePortOwner(value.owner) && isOptionalString(value.advertisedUrl);
	return value.kind === "container" || value.kind === "external";
}
function requireWorkspacePortScanResult(value) {
	if (!isRecord(value) || !Array.isArray(value.ports) || !value.ports.every(isWorkspacePort) || !WORKSPACE_PORT_PLATFORMS.has(value.platform) || typeof value.scannedAt !== "number" || !Number.isFinite(value.scannedAt) || "unavailableReason" in value && value.unavailableReason !== void 0 && typeof value.unavailableReason !== "string") throw new Error("Workspace port scan returned an invalid response.");
	return value;
}
async function runWorkspacePortScanForTarget(target, repoId) {
	const params = repoId ? { repoId } : {};
	if (target.kind === "local") return requireWorkspacePortScanResult(await window.api.workspacePorts.scan(params));
	try {
		return requireWorkspacePortScanResult(await callRuntimeRpc(target, "workspacePorts.scan", params, { timeoutMs: 15e3 }));
	} catch (error) {
		if (error instanceof RuntimeRpcCallError && error.code === "method_not_found") return {
			platform: "unknown",
			scannedAt: Date.now(),
			ports: [],
			unavailableReason: "The connected runtime does not support workspace port management yet."
		};
		throw error;
	}
}
var HTTPS_PORTS = new Set([443, 8443]);
var LOOPBACK_HOSTS = new Set([
	"localhost",
	"127.0.0.1",
	"::1",
	"0.0.0.0",
	"::"
]);
function hostForLocalAction(host) {
	if (!host) return "localhost";
	return host.includes(":") ? `[${host}]` : host;
}
function addressForPort(port) {
	if (port.kind === "workspace" && port.advertisedUrl) try {
		return new URL(port.advertisedUrl).host || `${hostForLocalAction(port.connectHost)}:${port.port}`;
	} catch {}
	return `${hostForLocalAction(port.connectHost)}:${port.port}`;
}
function browserUrlForPort(port) {
	if (port.kind === "workspace" && port.advertisedUrl) return port.advertisedUrl;
	return `${port.protocol === "https" ? "https" : "http"}://${hostForLocalAction(port.connectHost)}:${port.port}`;
}
function customHostFromAdvertised(advertisedUrl) {
	if (!advertisedUrl) return null;
	try {
		const url = new URL(advertisedUrl);
		const hostname = url.hostname.toLowerCase().replace(/^\[|\]$/g, "");
		if (LOOPBACK_HOSTS.has(hostname)) return null;
		if (/^[0-9.]+$/.test(hostname) || hostname.includes(":")) return null;
		return url.hostname;
	} catch {
		return null;
	}
}
function advertisedProtocolForPort(port) {
	if (port.advertisedProtocol) return port.advertisedProtocol;
	if (port.advertisedUrl) try {
		const protocol = new URL(port.advertisedUrl).protocol.replace(/:$/, "");
		if (protocol === "http" || protocol === "https") return protocol;
	} catch {}
	return HTTPS_PORTS.has(port.remotePort) ? "https" : "http";
}
function browserUrlForPortForwardEntry(entry) {
	return `${advertisedProtocolForPort(entry)}://${customHostFromAdvertised(entry.advertisedUrl) ?? "127.0.0.1"}:${entry.localPort}`;
}
function addressForPortForwardEntry(entry) {
	return new URL(browserUrlForPortForwardEntry(entry)).host;
}
function advertisedBrowserUrlForForwardedRow(entry) {
	if (!customHostFromAdvertised(entry.advertisedUrl)) return null;
	return browserUrlForPortForwardEntry(entry);
}
function advertisedBrowserUrlForDetectedPort(port) {
	const host = customHostFromAdvertised(port.advertisedUrl);
	if (!host) return null;
	return `${advertisedProtocolForPort({
		advertisedProtocol: port.advertisedProtocol,
		advertisedUrl: port.advertisedUrl,
		remotePort: port.port
	})}://${host}:${port.port}`;
}
var WORKSPACE_PORT_STOP_SETTLE_MS = 500;
function canStopWorkspacePort(port) {
	return port.kind === "workspace" && Boolean(port.pid) && port.processName !== "Electron";
}
function delay(ms) {
	return new Promise((resolve) => window.setTimeout(resolve, ms));
}
function shouldOpenWorkspacePortInOrcaBrowser(settings) {
	return settings?.openLinksInApp === true;
}
function isMacShortcutPlatform() {
	return typeof navigator !== "undefined" && navigator.userAgent.includes("Mac");
}
function getPortSystemBrowserHint(isMac = isMacShortcutPlatform()) {
	return isMac ? "⇧⌘+click for system browser" : "Shift+Ctrl+click for system browser";
}
function getPortOpenBrowserTooltipLabel(openLabel, isMac) {
	return `${openLabel}. ${getPortSystemBrowserHint(isMac)}`;
}
function resolvePortOpenInOrcaBrowser({ settings, event, isMac }) {
	if (event?.shiftKey && (isMac ? event.metaKey : event.ctrlKey)) return false;
	return shouldOpenWorkspacePortInOrcaBrowser(settings);
}
function workspacePortOwnerWorktreeId(port) {
	return port.kind === "workspace" ? port.owner.worktreeId : null;
}
function goToWorkspacePortOwner(port) {
	const worktreeId = workspacePortOwnerWorktreeId(port);
	return Boolean(worktreeId && activateAndRevealWorktree(worktreeId));
}
async function openWorkspacePortInBrowser(args) {
	const rawUrl = browserUrlForPort(args.port);
	let url = rawUrl;
	if (args.runtimeTarget.kind === "local" && args.localhostLabelRoute) try {
		url = (await window.api.localhostWorktreeLabels.register(args.localhostLabelRoute)).url;
	} catch {
		url = rawUrl;
	}
	if (args.openInOrcaBrowser === false && args.runtimeTarget.kind === "local") try {
		await window.api.shell.openUrl(url);
		return { ok: true };
	} catch (error) {
		return {
			ok: false,
			reason: (error instanceof Error ? error.message : String(error)) || "Failed to open system browser."
		};
	}
	const worktreeId = args.port.kind === "workspace" ? args.port.owner.worktreeId : args.activeWorktreeId;
	if (!worktreeId) return {
		ok: false,
		reason: "No workspace selected for the browser."
	};
	activateAndRevealWorktree(worktreeId);
	if (args.runtimeTarget.kind === "environment") try {
		await assertRuntimeEnvironmentCapability(args.runtimeTarget.environmentId, BROWSER_SCREENCAST_RUNTIME_CAPABILITY, RUNTIME_BROWSER_UNAVAILABLE_MESSAGE);
		const remotePage = await callRuntimeRpc(args.runtimeTarget, "browser.tabCreate", {
			worktree: toRuntimeWorktreeSelector(worktreeId),
			url
		}, { timeoutMs: 3e4 });
		const tab = args.createBrowserTab(worktreeId, url, {
			activate: true,
			browserRuntimeEnvironmentId: args.runtimeTarget.environmentId
		});
		if (!tab.activePageId) return {
			ok: false,
			reason: "Failed to create a browser page."
		};
		args.setRemoteBrowserPageHandle(tab.activePageId, {
			environmentId: args.runtimeTarget.environmentId,
			remotePageId: remotePage.browserPageId
		});
		return { ok: true };
	} catch (error) {
		return {
			ok: false,
			reason: (error instanceof Error ? error.message : String(error)) || "Failed to open remote browser."
		};
	}
	try {
		args.createBrowserTab(worktreeId, url, { activate: true });
		return { ok: true };
	} catch (error) {
		return {
			ok: false,
			reason: (error instanceof Error ? error.message : String(error)) || "Failed to open browser."
		};
	}
}
async function refreshWorkspacePortScanAfterStop(args) {
	const scanKey = workspacePortScanKeyForTarget(args.runtimeTarget);
	const publishScan = (scan) => {
		args.setWorkspacePortScanForKey?.(scanKey, scan);
		const currentScans = args.getWorkspacePortScansByKey?.() ?? {};
		const merged = mergeWorkspacePortScans({
			...currentScans,
			[scanKey]: scan
		});
		args.setWorkspacePortScan({
			key: merged && Object.keys(currentScans).length > 0 ? "all-hosts:all" : scanKey,
			result: merged ?? scan
		});
	};
	args.setWorkspacePortScanRefreshing(true);
	try {
		let firstScan;
		try {
			firstScan = await scanWorkspacePortsForTarget(args.runtimeTarget);
		} catch (error) {
			return {
				ok: false,
				reason: (error instanceof Error ? error.message : String(error)) || "Workspace port scan failed."
			};
		}
		publishScan(firstScan);
		await delay(WORKSPACE_PORT_STOP_SETTLE_MS);
		try {
			publishScan(await scanWorkspacePortsForTarget(args.runtimeTarget));
		} catch {}
		return { ok: true };
	} finally {
		args.setWorkspacePortScanRefreshing(false);
	}
}
function workspacePortRuntimeTargetKey(target) {
	return target.kind === "local" ? "local" : `environment:${target.environmentId}`;
}
function runtimeTargetForExecutionHostId(hostId) {
	const parsed = parseExecutionHostId(hostId);
	if (parsed?.kind === "local") return { kind: "local" };
	if (parsed?.kind === "runtime") return {
		kind: "environment",
		environmentId: parsed.environmentId
	};
	return null;
}
function workspacePortScanKeyForTarget(target) {
	return `${workspacePortRuntimeTargetKey(target)}:all`;
}
function mergeWorkspacePortScans(scansByKey) {
	const entries = Object.entries(scansByKey).filter(([, scan]) => scan).sort(([a], [b]) => a.localeCompare(b));
	if (entries.length === 0) return null;
	if (entries.length === 1) return entries[0][1];
	const ports = entries.flatMap(([key, scan]) => scan.ports.map((port) => ({
		...port,
		id: `${key}:${port.id}`
	})));
	const unavailable = entries.map(([key, scan]) => scan.unavailableReason ? `${key}: ${scan.unavailableReason}` : null).filter((entry) => entry !== null);
	return {
		platform: "unknown",
		scannedAt: Math.max(...entries.map(([, scan]) => scan.scannedAt)),
		ports,
		...unavailable.length === entries.length && unavailable.length > 0 ? { unavailableReason: unavailable.join("; ") } : {}
	};
}
var inFlightWorkspacePortScans = /* @__PURE__ */ new Map();
function workspacePortScanRequestKey(target, repoId) {
	return JSON.stringify([workspacePortRuntimeTargetKey(target), repoId ?? null]);
}
async function scanWorkspacePortsForTarget(target, repoId) {
	const key = workspacePortScanRequestKey(target, repoId);
	const existing = inFlightWorkspacePortScans.get(key);
	if (existing) return existing;
	const promise = runWorkspacePortScanForTarget(target, repoId).finally(() => {
		if (inFlightWorkspacePortScans.get(key) === promise) inFlightWorkspacePortScans.delete(key);
	});
	inFlightWorkspacePortScans.set(key, promise);
	return promise;
}
async function killWorkspacePortForTarget(target, args) {
	if (target.kind === "local") return window.api.workspacePorts.kill(args);
	try {
		return await callRuntimeRpc(target, "workspacePorts.kill", args, { timeoutMs: 15e3 });
	} catch (error) {
		if (error instanceof RuntimeRpcCallError && error.code === "method_not_found") return {
			ok: false,
			reason: "The connected runtime does not support workspace port management yet."
		};
		throw error;
	}
}
function localhostWorktreeLabelRouteForPort({ port, repo, project, settings }) {
	if (settings?.localhostWorktreeLabelsEnabled !== true || port.kind !== "workspace" || !repo) return null;
	const projectSource = project ?? repo;
	return {
		targetUrl: browserUrlForPort(port),
		projectName: projectSource.displayName,
		worktreeName: port.owner.displayName,
		worktreePath: port.owner.path,
		repoId: repo.id,
		worktreeId: port.owner.worktreeId
	};
}
function resolveLocalhostLabelRouteForPort(state, port) {
	if (port.kind !== "workspace") return null;
	const repo = (state.repos ?? []).find((entry) => entry.id === port.owner.repoId) ?? null;
	const worktree = state.getKnownWorktreeById?.(port.owner.worktreeId) ?? null;
	return localhostWorktreeLabelRouteForPort({
		port,
		repo,
		project: worktree?.projectId ? (state.projects ?? []).find((entry) => entry.id === worktree.projectId) ?? null : null,
		settings: state.settings
	});
}
function useLocalhostLabelRouteForPort(port) {
	const settings = useAppStore((s) => s.settings);
	const portWorktreeId = port.kind === "workspace" ? port.owner.worktreeId : null;
	const portRepoId = port.kind === "workspace" ? port.owner.repoId : null;
	const repo = useAppStore((s) => portRepoId ? (s.repos ?? []).find((entry) => entry.id === portRepoId) ?? null : null);
	const worktree = useAppStore((s) => portWorktreeId ? s.getKnownWorktreeById?.(portWorktreeId) ?? null : null);
	return localhostWorktreeLabelRouteForPort({
		port,
		repo,
		project: useAppStore((s) => worktree?.projectId ? (s.projects ?? []).find((entry) => entry.id === worktree.projectId) ?? null : null),
		settings
	});
}
export { advertisedBrowserUrlForDetectedPort as _, goToWorkspacePortOwner as a, openWorkspacePortInBrowser as c, runtimeTargetForExecutionHostId as d, scanWorkspacePortsForTarget as f, addressForPortForwardEntry as g, addressForPort as h, getPortOpenBrowserTooltipLabel as i, refreshWorkspacePortScanAfterStop as l, workspacePortScanKeyForTarget as m, useLocalhostLabelRouteForPort as n, killWorkspacePortForTarget as o, workspacePortRuntimeTargetKey as p, canStopWorkspacePort as r, mergeWorkspacePortScans as s, resolveLocalhostLabelRouteForPort as t, resolvePortOpenInOrcaBrowser as u, advertisedBrowserUrlForForwardedRow as v, browserUrlForPortForwardEntry as y };
