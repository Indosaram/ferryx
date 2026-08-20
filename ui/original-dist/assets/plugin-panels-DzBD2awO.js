import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { j as create } from "./plugin-manifest-Bs-50M_g.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var pluginListGeneration = 0;
var pluginListRetryAttempt = 0;
var pluginListRetryTimer = null;
var PLUGIN_LIST_MAX_RETRIES = 2;
function schedulePluginListRetry(generation) {
	if (pluginListRetryAttempt >= PLUGIN_LIST_MAX_RETRIES) {
		pluginListRetryAttempt = 0;
		return;
	}
	pluginListRetryAttempt += 1;
	const delayMs = 250 * 2 ** (pluginListRetryAttempt - 1);
	pluginListRetryTimer = setTimeout(() => {
		pluginListRetryTimer = null;
		const state = usePluginPanelsStore.getState();
		if (generation === pluginListGeneration && state.fetchStatus === "error") state.fetchPlugins();
	}, delayMs);
}
const usePluginPanelsStore = create()((set) => ({
	plugins: [],
	panelErrors: {},
	fetchStatus: "idle",
	fetchPlugins: async () => {
		const generation = ++pluginListGeneration;
		const pluginsApi = window.api?.plugins;
		if (!pluginsApi) {
			if (generation === pluginListGeneration) {
				pluginListRetryAttempt = 0;
				set({
					fetchStatus: "ready",
					plugins: [],
					panelErrors: {}
				});
			}
			return;
		}
		set({ fetchStatus: "loading" });
		try {
			const plugins = await pluginsApi.list();
			if (generation === pluginListGeneration) {
				pluginListRetryAttempt = 0;
				set((state) => ({
					plugins,
					fetchStatus: "ready",
					panelErrors: retainInstalledPanelErrors(state.panelErrors, plugins)
				}));
			}
		} catch {
			if (generation === pluginListGeneration) {
				set({
					plugins: [],
					panelErrors: {},
					fetchStatus: "error"
				});
				schedulePluginListRetry(generation);
			}
		}
	},
	setPlugins: (plugins) => {
		pluginListGeneration += 1;
		pluginListRetryAttempt = 0;
		if (pluginListRetryTimer) {
			clearTimeout(pluginListRetryTimer);
			pluginListRetryTimer = null;
		}
		set((state) => ({
			plugins,
			fetchStatus: "ready",
			panelErrors: retainInstalledPanelErrors(state.panelErrors, plugins)
		}));
	},
	setPanelHealth: (tabKey, health) => {
		set((state) => {
			const panelErrors = { ...state.panelErrors };
			if (health === "error") panelErrors[tabKey] = true;
			else delete panelErrors[tabKey];
			return { panelErrors };
		});
	}
}));
function retainInstalledPanelErrors(panelErrors, plugins) {
	const installed = collectInstalledPluginTabKeys(plugins);
	return Object.fromEntries(Object.entries(panelErrors).filter(([tabKey]) => installed.has(tabKey)));
}
var changeSubscriptionStarted = false;
function ensurePluginPanelsLoaded() {
	const { fetchStatus, fetchPlugins } = usePluginPanelsStore.getState();
	if (fetchStatus === "idle") fetchPlugins();
	if (!changeSubscriptionStarted && window.api?.plugins?.onChanged) {
		changeSubscriptionStarted = true;
		window.api.plugins.onChanged(() => {
			usePluginPanelsStore.getState().fetchPlugins();
		});
	}
}
function collectActivePluginPanels(plugins) {
	return plugins.filter((plugin) => plugin.status === "running" || plugin.status === "restarting" || plugin.status === "idle").flatMap((plugin) => plugin.panels.map((panel) => ({
		...panel,
		pluginKey: plugin.pluginKey,
		pluginName: plugin.name
	})));
}
function collectInstalledPluginTabKeys(plugins) {
	return new Set(plugins.flatMap((plugin) => plugin.panels.map((panel) => panel.tabKey)));
}
function collectActivePluginCommands(plugins) {
	return plugins.filter((plugin) => plugin.status === "running" || plugin.status === "restarting" || plugin.status === "idle").flatMap((plugin) => plugin.commands.map((command) => ({
		...command,
		pluginKey: plugin.pluginKey,
		pluginName: plugin.name
	})));
}
function collectEditablePluginCommands(plugins) {
	return plugins.filter((plugin) => [
		"running",
		"restarting",
		"idle",
		"errored"
	].includes(plugin.status)).flatMap((plugin) => plugin.commands.map((command) => ({
		...command,
		pluginKey: plugin.pluginKey,
		pluginName: plugin.name
	})));
}
function usePluginPanels() {
	const plugins = usePluginPanelsStore((s) => s.plugins);
	(0, import_react.useEffect)(() => {
		ensurePluginPanelsLoaded();
	}, []);
	return (0, import_react.useMemo)(() => collectActivePluginPanels(plugins), [plugins]);
}
function usePluginCommands() {
	const plugins = usePluginPanelsStore((state) => state.plugins);
	(0, import_react.useEffect)(() => {
		ensurePluginPanelsLoaded();
	}, []);
	return (0, import_react.useMemo)(() => collectActivePluginCommands(plugins), [plugins]);
}
function useEditablePluginCommands() {
	const plugins = usePluginPanelsStore((state) => state.plugins);
	(0, import_react.useEffect)(() => {
		ensurePluginPanelsLoaded();
	}, []);
	return (0, import_react.useMemo)(() => collectEditablePluginCommands(plugins), [plugins]);
}
export { usePluginPanelsStore as a, usePluginPanels as i, useEditablePluginCommands as n, usePluginCommands as r, collectInstalledPluginTabKeys as t };
