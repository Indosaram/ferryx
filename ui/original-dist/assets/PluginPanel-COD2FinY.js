import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { B as number, F as boolean, G as unknown, H as record, L as json, M as _enum, P as array, U as string, V as object, n as isPluginPanelTabKey, t as PLUGIN_EVENT_NAMES, z as literal } from "./plugin-manifest-Bs-50M_g.js";
import { a as usePluginPanelsStore, i as usePluginPanels } from "./plugin-panels-DzBD2awO.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
const PANEL_ACTION_TEXT_MAX_LENGTH = 4096;
const PLUGIN_TERMINAL_ID_MAX_LENGTH = 1024;
var workspaceReadContextParams = object({}).strict().optional();
var workspaceReadContextResult = object({
	branch: string().max(512),
	displayName: string().max(512),
	terminals: array(object({ id: string().min(1).max(PLUGIN_TERMINAL_ID_MAX_LENGTH) }).strict()).max(50)
}).strict().nullable();
var terminalSendTextParams = object({
	terminalId: string().min(1).max(PLUGIN_TERMINAL_ID_MAX_LENGTH),
	text: string().min(1).max(PANEL_ACTION_TEXT_MAX_LENGTH),
	enter: boolean().default(false)
});
var terminalSendTextResult = object({ accepted: boolean() });
var notificationsShowParams = object({
	title: string().min(1).max(120),
	body: string().max(1e3).optional()
});
var notificationsShowResult = object({ delivered: boolean() });
var RESERVED_STORAGE_KEYS = new Set([
	"__proto__",
	"prototype",
	"constructor"
]);
var storageKeySchema = string().min(1).max(256).refine((key) => !RESERVED_STORAGE_KEYS.has(key), "reserved storage key");
var pluginJsonValueSchema = json();
const PLUGIN_STORAGE_KEY_LIMIT = 1024;
var storageGetParams = object({ key: storageKeySchema });
var storageGetResult = object({ value: pluginJsonValueSchema });
var storageSetParams = object({
	key: storageKeySchema,
	value: pluginJsonValueSchema
});
var storageSetResult = object({ ok: literal(true) });
var storageDeleteParams = object({ key: storageKeySchema });
var storageDeleteResult = object({ ok: literal(true) });
var storageKeysParams = object({}).strict().optional();
var storageKeysResult = object({ keys: array(string()).max(PLUGIN_STORAGE_KEY_LIMIT) });
var secretsGetParams = object({ key: storageKeySchema });
var secretsGetResult = object({ value: string().nullable() });
var secretsSetParams = object({
	key: storageKeySchema,
	value: string().max(64 * 1024)
});
var secretsSetResult = object({ ok: literal(true) });
var secretsDeleteParams = object({ key: storageKeySchema });
var secretsDeleteResult = object({ ok: literal(true) });
var settingsGetParams = object({}).strict().optional();
var settingsGetResult = object({ settings: record(string(), pluginJsonValueSchema) });
var settingsSetParams = object({
	key: storageKeySchema,
	value: pluginJsonValueSchema
});
var settingsSetResult = object({ ok: literal(true) });
var eventsSubscribeParams = object({ events: array(_enum(PLUGIN_EVENT_NAMES)).min(1).max(PLUGIN_EVENT_NAMES.length) });
var eventsSubscribeResult = object({ subscribed: array(_enum(PLUGIN_EVENT_NAMES)) });
var spec = (entry) => ({
	...entry,
	stability: "experimental"
});
const PLUGIN_HOST_API_V0 = [
	spec({
		name: "workspace.readContext",
		since: "1.0",
		scope: "active-worktree",
		capability: "workspace:read",
		mutation: false,
		panel: true,
		params: workspaceReadContextParams,
		result: workspaceReadContextResult
	}),
	spec({
		name: "terminal.sendText",
		since: "1.0",
		scope: "explicit-terminal",
		capability: "terminal:send",
		mutation: true,
		panel: true,
		params: terminalSendTextParams,
		result: terminalSendTextResult
	}),
	spec({
		name: "notifications.show",
		since: "1.0",
		scope: "desktop",
		capability: "notifications:show",
		mutation: true,
		panel: true,
		params: notificationsShowParams,
		result: notificationsShowResult
	}),
	spec({
		name: "storage.get",
		since: "1.0",
		scope: "plugin-private",
		capability: "storage",
		mutation: false,
		panel: false,
		params: storageGetParams,
		result: storageGetResult
	}),
	spec({
		name: "storage.set",
		since: "1.0",
		scope: "plugin-private",
		capability: "storage",
		mutation: true,
		panel: false,
		params: storageSetParams,
		result: storageSetResult
	}),
	spec({
		name: "storage.delete",
		since: "1.0",
		scope: "plugin-private",
		capability: "storage",
		mutation: true,
		panel: false,
		params: storageDeleteParams,
		result: storageDeleteResult
	}),
	spec({
		name: "storage.keys",
		since: "1.0",
		scope: "plugin-private",
		capability: "storage",
		mutation: false,
		panel: false,
		params: storageKeysParams,
		result: storageKeysResult
	}),
	spec({
		name: "secrets.get",
		since: "1.0",
		scope: "plugin-private",
		capability: "secrets",
		mutation: false,
		panel: false,
		params: secretsGetParams,
		result: secretsGetResult
	}),
	spec({
		name: "secrets.set",
		since: "1.0",
		scope: "plugin-private",
		capability: "secrets",
		mutation: true,
		panel: false,
		params: secretsSetParams,
		result: secretsSetResult
	}),
	spec({
		name: "secrets.delete",
		since: "1.0",
		scope: "plugin-private",
		capability: "secrets",
		mutation: true,
		panel: false,
		params: secretsDeleteParams,
		result: secretsDeleteResult
	}),
	spec({
		name: "settings.get",
		since: "1.0",
		scope: "plugin-private",
		capability: "settings:own",
		mutation: false,
		panel: false,
		params: settingsGetParams,
		result: settingsGetResult
	}),
	spec({
		name: "settings.set",
		since: "1.0",
		scope: "plugin-private",
		capability: "settings:own",
		mutation: true,
		panel: false,
		params: settingsSetParams,
		result: settingsSetResult
	}),
	spec({
		name: "events.subscribe",
		since: "1.0",
		scope: "host-events",
		capability: "events:subscribe",
		mutation: false,
		panel: false,
		params: eventsSubscribeParams,
		result: eventsSubscribeResult
	})
];
new Map(PLUGIN_HOST_API_V0.map((entry) => [entry.name, entry]));
const PLUGIN_PANEL_ACTIONS = PLUGIN_HOST_API_V0.filter((entry) => entry.panel).map((entry) => entry.name);
function isPluginPanelAction(action) {
	return PLUGIN_PANEL_ACTIONS.includes(action);
}
const PANEL_ACTION_REQUEST_TYPE = "orca-panel-action";
const PANEL_ACTION_RESULT_TYPE = "orca-panel-action-result";
const PANEL_PING_TYPE = "orca-panel-ping";
const PANEL_PONG_TYPE = "orca-panel-pong";
const PLUGIN_PANEL_FRAME_NAME_PREFIX = "orca-plugin-panel:";
const PANEL_MESSAGE_MAX_BYTES = 64 * 1024;
const PANEL_MESSAGE_RATE_LIMIT = {
	maxMessages: 30,
	perMs: 1e4
};
const PANEL_CONTROL_MESSAGE_MAX_BYTES = 1024;
const panelActionRequestSchema = object({
	type: literal(PANEL_ACTION_REQUEST_TYPE),
	requestId: string().min(1).max(128),
	action: string().min(1).refine(isPluginPanelAction, "not a panel-callable action"),
	params: unknown().optional()
});
object({
	type: literal(PANEL_PONG_TYPE),
	pingId: number().int().nonnegative()
});
object({
	sessionToken: string().min(32).max(128),
	action: string().min(1),
	params: unknown().optional()
}).strict();
function parsePanelActionRequest(data) {
	const parsed = panelActionRequestSchema.safeParse(data);
	if (parsed.success) return {
		ok: true,
		request: parsed.data
	};
	let requestId = null;
	if (typeof data === "object" && data !== null && "requestId" in data) {
		const raw = data.requestId;
		if (typeof raw === "string" && raw.length > 0 && raw.length <= 128) requestId = raw;
	}
	const issue = parsed.error.issues[0];
	const path = issue?.path.join(".") || "(root)";
	return {
		ok: false,
		requestId,
		error: `${path}: ${issue?.message ?? "invalid panel action request"}`
	};
}
function looksLikePanelActionRequest(data) {
	return typeof data === "object" && data !== null && data.type === "orca-panel-action";
}
function readPanelPongId(data) {
	if (typeof data !== "object" || data === null) return null;
	const frame = data;
	if (frame.type !== "orca-panel-pong" || typeof frame.pingId !== "number") return null;
	return Number.isSafeInteger(frame.pingId) && frame.pingId >= 0 ? frame.pingId : null;
}
const PANEL_SHELL_TOKENS_PLACEHOLDER = "/*__ORCA_PANEL_TOKENS__*/";
const PANEL_SHELL_COLOR_SCHEME_PLACEHOLDER = "__ORCA_COLOR_SCHEME__";
const PANEL_DESIGN_TOKEN_ALLOWLIST = [
	"--background",
	"--foreground",
	"--card",
	"--card-foreground",
	"--popover",
	"--popover-foreground",
	"--primary",
	"--primary-foreground",
	"--secondary",
	"--secondary-foreground",
	"--muted",
	"--muted-foreground",
	"--accent",
	"--accent-foreground",
	"--destructive",
	"--destructive-foreground",
	"--border",
	"--input",
	"--ring",
	"--radius"
];
function createPanelMessageBudget(limits = {}) {
	const maxBytes = limits.maxBytes ?? 65536;
	const maxMessages = limits.maxMessages ?? PANEL_MESSAGE_RATE_LIMIT.maxMessages;
	const perMs = limits.perMs ?? PANEL_MESSAGE_RATE_LIMIT.perMs;
	const timestamps = [];
	return {
		maxBytes,
		admit(now, messageBytes) {
			while (timestamps.length > 0 && timestamps[0] <= now - perMs) timestamps.shift();
			if (timestamps.length >= maxMessages) return "rate_limited";
			timestamps.push(now);
			if (messageBytes > maxBytes) return "oversized";
			return null;
		}
	};
}
function createPanelControlMessageBudget() {
	return {
		maxBytes: PANEL_CONTROL_MESSAGE_MAX_BYTES,
		admit: (_now, messageBytes) => messageBytes > 1024 ? "oversized" : null
	};
}
var textEncoder = new TextEncoder();
function utf8Bytes(value, stopAfter) {
	if (value.length > stopAfter) return stopAfter + 1;
	return textEncoder.encode(value).byteLength;
}
function structuredCloneMessageBytes(data, stopAfter = PANEL_MESSAGE_MAX_BYTES) {
	const seen = /* @__PURE__ */ new WeakSet();
	let total = 0;
	let visitedNodes = 0;
	const add = (bytes) => {
		total = Math.min(stopAfter + 1, total + bytes);
	};
	const visit = (value, depth) => {
		if (total > stopAfter) return;
		if (value === null) {
			add(1);
			return;
		}
		switch (typeof value) {
			case "undefined":
			case "boolean":
				add(1);
				return;
			case "number":
				add(8);
				return;
			case "bigint":
				add(utf8Bytes(value.toString(), stopAfter - total));
				return;
			case "string":
				add(utf8Bytes(value, stopAfter - total));
				return;
			case "symbol":
			case "function":
				total = stopAfter + 1;
				return;
			case "object": break;
		}
		const object$1 = value;
		if (seen.has(object$1)) {
			add(8);
			return;
		}
		seen.add(object$1);
		visitedNodes += 1;
		if (visitedNodes > 1e4 || depth > 100) {
			total = stopAfter + 1;
			return;
		}
		if (object$1 instanceof ArrayBuffer) {
			add(object$1.byteLength);
			return;
		}
		if (typeof SharedArrayBuffer !== "undefined" && object$1 instanceof SharedArrayBuffer) {
			add(object$1.byteLength);
			return;
		}
		if (ArrayBuffer.isView(object$1)) {
			add(16);
			visit(object$1.buffer, depth + 1);
			return;
		}
		if (typeof Blob !== "undefined" && object$1 instanceof Blob) {
			add(object$1.size);
			return;
		}
		if (object$1 instanceof Date) {
			add(8);
			return;
		}
		if (object$1 instanceof RegExp) {
			visit(object$1.source, depth + 1);
			visit(object$1.flags, depth + 1);
			return;
		}
		if (object$1 instanceof Map) {
			add(8);
			for (const [key, entry] of object$1) {
				add(4);
				visit(key, depth + 1);
				visit(entry, depth + 1);
			}
			return;
		}
		if (object$1 instanceof Set) {
			add(8);
			for (const entry of object$1) {
				add(4);
				visit(entry, depth + 1);
			}
			return;
		}
		if (Array.isArray(object$1)) {
			add(8);
			for (const entry of object$1) {
				add(4);
				visit(entry, depth + 1);
			}
			return;
		}
		try {
			const prototype = Object.getPrototypeOf(object$1);
			if (prototype !== Object.prototype && prototype !== null) {
				total = stopAfter + 1;
				return;
			}
			for (const key of Object.keys(object$1)) {
				add(4);
				add(utf8Bytes(key, stopAfter - total));
				visit(object$1[key], depth + 1);
			}
		} catch {
			total = stopAfter + 1;
		}
	};
	visit(data, 0);
	return total;
}
function callPanelActionViaPreload(call) {
	const panelAction = window.api?.plugins?.panelAction;
	if (!panelAction) return Promise.resolve({
		ok: false,
		code: "unavailable",
		error: translate("auto.components.rightSidebar.pluginPanelBridgeHost.actionsUnavailable", "Plugin actions are not available in this client.")
	});
	return panelAction(call);
}
function createPanelBridgeMessageHandler(options) {
	const budget = options.budget ?? createPanelMessageBudget();
	const controlBudget = options.controlBudget ?? createPanelControlMessageBudget();
	const now = options.now ?? (() => Date.now());
	return (event) => {
		const panelWindow = options.getPanelWindow();
		if (!panelWindow || event.source !== panelWindow) return;
		const requestingWindow = panelWindow;
		const respond = (message) => {
			if (options.isActive?.() === false || options.getPanelWindow() !== requestingWindow) return;
			requestingWindow.postMessage(message, "*");
		};
		const pongId = readPanelPongId(event.data);
		if (pongId !== null) {
			const timestamp = now();
			const pongBytes = structuredCloneMessageBytes(event.data, controlBudget.maxBytes ?? 1024);
			budget.admit(timestamp, pongBytes);
			if (!controlBudget.admit(timestamp, pongBytes)) options.onPong?.(pongId);
			return;
		}
		const refusal = budget.admit(now(), structuredCloneMessageBytes(event.data, budget.maxBytes));
		if (refusal) {
			const requestId$1 = typeof event.data === "object" && event.data !== null ? event.data.requestId : void 0;
			if (typeof requestId$1 === "string" && requestId$1.length > 0 && requestId$1.length <= 128) respond({
				type: PANEL_ACTION_RESULT_TYPE,
				requestId: requestId$1,
				ok: false,
				errorCode: refusal === "oversized" ? "invalid_request" : "rate_limited",
				error: refusal === "oversized" ? translate("auto.components.rightSidebar.pluginPanelBridgeHost.messageTooLarge", "Message exceeds the size limit.") : translate("auto.components.rightSidebar.pluginPanelBridgeHost.tooManyRequests", "Too many requests.")
			});
			return;
		}
		if (!looksLikePanelActionRequest(event.data)) return;
		const parsed = parsePanelActionRequest(event.data);
		if (!parsed.ok) {
			if (parsed.requestId) respond({
				type: PANEL_ACTION_RESULT_TYPE,
				requestId: parsed.requestId,
				ok: false,
				errorCode: "invalid_request",
				error: parsed.error
			});
			return;
		}
		const { requestId, action, params } = parsed.request;
		options.callPanelAction({
			sessionToken: options.sessionToken,
			action,
			params
		}).then((outcome) => {
			respond(outcome.ok ? {
				type: PANEL_ACTION_RESULT_TYPE,
				requestId,
				ok: true,
				value: outcome.value
			} : {
				type: PANEL_ACTION_RESULT_TYPE,
				requestId,
				ok: false,
				errorCode: outcome.code,
				error: outcome.error
			});
		}).catch((error) => {
			respond({
				type: PANEL_ACTION_RESULT_TYPE,
				requestId,
				ok: false,
				errorCode: "action_failed",
				error: error instanceof Error ? error.message : String(error)
			});
		});
	};
}
function createPanelWatchdog(options) {
	const pingIntervalMs = options.pingIntervalMs ?? 1e4;
	const pongTimeoutMs = options.pongTimeoutMs ?? 5e3;
	let pingTimer = null;
	let deadlineTimer = null;
	let nextPingId = 0;
	let awaitedPingId = null;
	let active = false;
	let generation = 0;
	const clearDeadline = () => {
		if (deadlineTimer) {
			clearTimeout(deadlineTimer);
			deadlineTimer = null;
		}
	};
	const ping = () => {
		if (!active || awaitedPingId !== null) return;
		awaitedPingId = nextPingId++;
		options.sendPing(awaitedPingId);
		const deadlineGeneration = generation;
		deadlineTimer = setTimeout(() => {
			if (active && generation === deadlineGeneration && awaitedPingId !== null) {
				active = false;
				if (pingTimer) {
					clearInterval(pingTimer);
					pingTimer = null;
				}
				deadlineTimer = null;
				awaitedPingId = null;
				options.onUnresponsive();
			}
		}, pongTimeoutMs);
	};
	return {
		start() {
			if (active) return;
			generation += 1;
			active = true;
			awaitedPingId = null;
			clearDeadline();
			pingTimer = setInterval(ping, pingIntervalMs);
			ping();
		},
		stop() {
			active = false;
			generation += 1;
			if (pingTimer) {
				clearInterval(pingTimer);
				pingTimer = null;
			}
			clearDeadline();
			awaitedPingId = null;
		},
		handlePong(pingId) {
			if (active && pingId === awaitedPingId) {
				awaitedPingId = null;
				clearDeadline();
			}
		}
	};
}
function buildPanelDesignTokenCss() {
	const styles = getComputedStyle(document.documentElement);
	const declarations = [];
	for (const token of PANEL_DESIGN_TOKEN_ALLOWLIST) {
		const value = styles.getPropertyValue(token).trim();
		if (value.length > 0) declarations.push(`${token}:${value.replaceAll(/[{}<>;]/g, "")}`);
	}
	return declarations.join(";");
}
function currentPanelColorScheme() {
	return document.documentElement.classList.contains("dark") ? "dark" : "light";
}
function readPanelThemeSnapshot() {
	return `${currentPanelColorScheme()}|${buildPanelDesignTokenCss()}`;
}
function usePluginPanelThemeRevision() {
	const [revision, setRevision] = (0, import_react.useState)(0);
	const snapshotRef = (0, import_react.useRef)(null);
	(0, import_react.useEffect)(() => {
		snapshotRef.current ?? (snapshotRef.current = readPanelThemeSnapshot());
		const observer = new MutationObserver(() => {
			const snapshot = readPanelThemeSnapshot();
			if (snapshot === snapshotRef.current) return;
			snapshotRef.current = snapshot;
			setRevision((current) => current + 1);
		});
		observer.observe(document.documentElement, {
			attributes: true,
			attributeFilter: ["class", "style"]
		});
		return () => observer.disconnect();
	}, []);
	return revision;
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function PluginPanelMessage({ children }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "flex min-h-0 flex-1 items-center justify-center p-6 text-center text-sm text-muted-foreground",
		children
	});
}
function fillPanelShell(html) {
	return html.replace(PANEL_SHELL_COLOR_SCHEME_PLACEHOLDER, currentPanelColorScheme()).replace(PANEL_SHELL_TOKENS_PLACEHOLDER, buildPanelDesignTokenCss());
}
function PluginPanel({ tabKey }) {
	const panels = usePluginPanels();
	const setPanelHealth = usePluginPanelsStore((state) => state.setPanelHealth);
	const panel = isPluginPanelTabKey(tabKey) ? panels.find((entry) => entry.tabKey === tabKey) ?? null : null;
	const [entryState, setEntryState] = (0, import_react.useState)({ status: "loading" });
	const [sessionToken, setSessionToken] = (0, import_react.useState)(null);
	const [loadedFrameKey, setLoadedFrameKey] = (0, import_react.useState)(null);
	const iframeRef = (0, import_react.useRef)(null);
	const themeRevision = usePluginPanelThemeRevision();
	const pluginKey = panel?.pluginKey ?? null;
	const panelId = panel?.id ?? null;
	const panelShell = entryState.status === "ready" ? entryState.shellHtml : null;
	const panelDocument = panelShell ? fillPanelShell(panelShell) : null;
	const panelFrameKey = entryState.status === "ready" ? `${tabKey}:${entryState.documentRevision}:${themeRevision}` : null;
	const watchdog = (0, import_react.useMemo)(() => createPanelWatchdog({
		sendPing: (pingId) => iframeRef.current?.contentWindow?.postMessage({
			type: PANEL_PING_TYPE,
			pingId
		}, "*"),
		onUnresponsive: () => {
			setPanelHealth(tabKey, "error");
			setEntryState({ status: "unresponsive" });
		}
	}), [setPanelHealth, tabKey]);
	(0, import_react.useEffect)(() => {
		if (!sessionToken || !panelDocument) return;
		let active = true;
		const handler = createPanelBridgeMessageHandler({
			sessionToken,
			getPanelWindow: () => iframeRef.current?.contentWindow ?? null,
			callPanelAction: callPanelActionViaPreload,
			isActive: () => active,
			onPong: (pingId) => watchdog.handlePong(pingId)
		});
		window.addEventListener("message", handler);
		return () => {
			active = false;
			window.removeEventListener("message", handler);
		};
	}, [
		panelDocument,
		sessionToken,
		watchdog
	]);
	(0, import_react.useEffect)(() => {
		if (!panelFrameKey || loadedFrameKey !== panelFrameKey) return;
		watchdog.start();
		return () => watchdog.stop();
	}, [
		loadedFrameKey,
		panelFrameKey,
		watchdog
	]);
	(0, import_react.useEffect)(() => {
		if (!pluginKey || !panelId) return;
		let cancelled = false;
		let currentHtml = null;
		let documentRevision = 0;
		setEntryState({ status: "loading" });
		setSessionToken(null);
		const pluginsApi = window.api?.plugins;
		if (!pluginsApi) {
			setPanelHealth(tabKey, "error");
			setEntryState({ status: "error" });
			return;
		}
		let loadGeneration = 0;
		const load = () => {
			const generation = ++loadGeneration;
			pluginsApi.readPanelEntry({
				pluginKey,
				panelId
			}).then((entry) => {
				if (cancelled || generation !== loadGeneration) return;
				if (!entry) {
					currentHtml = null;
					setSessionToken(null);
					setPanelHealth(tabKey, "error");
					setEntryState({ status: "error" });
					return;
				}
				setSessionToken(entry.sessionToken);
				setPanelHealth(tabKey, "healthy");
				if (entry.html !== currentHtml) {
					currentHtml = entry.html;
					documentRevision += 1;
					setPanelHealth(tabKey, "healthy");
					setEntryState({
						status: "ready",
						shellHtml: entry.html,
						documentRevision
					});
				}
			}).catch(() => {
				if (!cancelled && generation === loadGeneration) {
					currentHtml = null;
					setSessionToken(null);
					setPanelHealth(tabKey, "error");
					setEntryState({ status: "error" });
				}
			});
		};
		load();
		const unsubscribe = pluginsApi.onChanged ? pluginsApi.onChanged(load) : null;
		return () => {
			cancelled = true;
			loadGeneration += 1;
			unsubscribe?.();
		};
	}, [
		panelId,
		pluginKey,
		setPanelHealth,
		tabKey
	]);
	if (!panel) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PluginPanelMessage, { children: translate("auto.components.right.sidebar.PluginPanel.unavailable", "This plugin panel is no longer available.") });
	if (entryState.status === "loading") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PluginPanelMessage, { children: translate("auto.components.right.sidebar.PluginPanel.loading", "Loading plugin panel...") });
	if (entryState.status === "unresponsive") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PluginPanelMessage, { children: translate("auto.components.right.sidebar.PluginPanel.unresponsive", "This plugin panel stopped responding and was suspended.") });
	if (entryState.status === "error") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PluginPanelMessage, { children: translate("auto.components.right.sidebar.PluginPanel.loadFailed", "The plugin panel could not be loaded.") });
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("iframe", {
		ref: iframeRef,
		sandbox: "allow-scripts",
		name: `${PLUGIN_PANEL_FRAME_NAME_PREFIX}${tabKey}`,
		srcDoc: panelDocument ?? "",
		onLoad: () => setLoadedFrameKey(panelFrameKey),
		title: panel.title,
		className: "h-full w-full flex-1 border-0 bg-background"
	}, panelFrameKey);
}
var PluginPanel_default = PluginPanel;
export { PluginPanel_default as default };
