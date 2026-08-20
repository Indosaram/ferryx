import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { j as create } from "./plugin-manifest-Bs-50M_g.js";
var TRANSLATABLE_PLUGIN_CHROME = new Set([
	"auto.components.settings.PluginsSettingsSection.title",
	"auto.components.settings.PluginsSettingsSection.systemLabel",
	"auto.components.settings.PluginsSettingsSection.install",
	"auto.components.settings.PluginsSettingsSection.loading",
	"auto.components.settings.PluginsSettingsSection.empty",
	"auto.components.settings.PluginsSettingsSection.emptyTitle",
	"auto.components.settings.PluginsSettingsSection.noInstalledResults",
	"auto.components.settings.PluginsSettingsSection.noInstalledResultsTitle",
	"auto.components.settings.PluginMarketplaceBrowser.manageSources",
	"auto.components.settings.PluginMarketplaceBrowser.addSource",
	"auto.components.settings.PluginMarketplaceBrowser.refresh",
	"auto.components.settings.PluginMarketplaceBrowser.refreshing",
	"auto.components.settings.PluginMarketplaceBrowser.loading",
	"auto.components.settings.PluginMarketplaceBrowser.tryAgain",
	"auto.components.settings.PluginMarketplaceBrowser.clearSearch",
	"auto.components.settings.PluginMarketplaceBrowser.empty",
	"auto.components.settings.PluginMarketplaceBrowser.emptyTitle",
	"auto.components.settings.PluginMarketplaceBrowser.noInstalled",
	"auto.components.settings.PluginMarketplaceBrowser.noInstalledTitle",
	"auto.components.settings.PluginMarketplaceBrowser.noResults",
	"auto.components.settings.PluginMarketplaceBrowser.noResultsTitle",
	"auto.components.settings.PluginMarketplaceBrowser.noSourcesTitle",
	"auto.components.settings.PluginDevelopmentSection.title",
	"auto.components.settings.PluginDevelopmentSection.add",
	"auto.components.settings.PluginDevelopmentSection.remove",
	"auto.components.settings.PluginDevelopmentSection.pathLabel",
	"auto.components.settings.PluginDevelopmentSection.pathRequired",
	"auto.components.settings.PluginDevelopmentSection.placeholder",
	"auto.components.settings.plugins.search.title",
	"auto.components.settings.plugins.search.description",
	"auto.components.settings.plugins.search.install",
	"auto.components.settings.plugins.search.permissions",
	"auto.components.settings.plugins.search.logs",
	"auto.components.settings.plugins.search.development"
]);
function translatablePluginChrome(path) {
	return TRANSLATABLE_PLUGIN_CHROME.has(path);
}
function translatablePluginChromeContainer(path) {
	const prefix = `${path}.`;
	for (const exempt of TRANSLATABLE_PLUGIN_CHROME) if (exempt.startsWith(prefix)) return true;
	return false;
}
const PLUGIN_LANGUAGE_CATALOG_MAX_ENTRIES = 2e4;
var DANGEROUS_CATALOG_KEYS = new Set([
	"__proto__",
	"prototype",
	"constructor"
]);
var PROTECTED_TRANSLATION_ROOT = "auto.components.settings.";
var PROTECTED_TRANSLATION_MODULE = /^plugin/i;
function isPluginLanguagePackRegistration(value) {
	if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
	const pack = value;
	return typeof pack.id === "string" && pack.id.startsWith("plugin:") && typeof pack.resourceLanguage === "string" && pack.resourceLanguage === pluginLanguageResourceId(pack.id) && typeof pack.pluginKey === "string" && typeof pack.locale === "string" && validatePluginLanguagePackCatalogShape(pack.catalog).ok;
}
function pluginLanguageResourceId(id) {
	let encoded = "";
	for (let index = 0; index < id.length; index += 1) encoded += id.charCodeAt(index).toString(16).padStart(4, "0");
	return `plugin${encoded}`;
}
function isCatalogObject(value) {
	return typeof value === "object" && value !== null && !Array.isArray(value) && Object.getPrototypeOf(value) === Object.prototype;
}
function protectedTranslation(path) {
	if (!path.startsWith(PROTECTED_TRANSLATION_ROOT)) return false;
	if (translatablePluginChrome(path)) return false;
	return PROTECTED_TRANSLATION_MODULE.test(path.slice(25));
}
function hasUnsafeCatalogKeyCharacter(key) {
	if (key.includes(".")) return true;
	for (let index = 0; index < key.length; index += 1) if (key.charCodeAt(index) <= 31) return true;
	return false;
}
function validatePluginLanguagePackCatalogShape(source) {
	const result = walkPluginLanguagePackCatalog(source, false);
	return result.ok ? {
		ok: true,
		entries: result.entries
	} : result;
}
function walkPluginLanguagePackCatalog(source, copyCatalog) {
	if (!isCatalogObject(source)) return {
		ok: false,
		error: "language pack root must be an object"
	};
	const catalog = copyCatalog ? {} : null;
	const stack = [{
		source,
		target: catalog,
		path: "",
		depth: 0
	}];
	const seen = new WeakSet([source]);
	let entries = 0;
	while (stack.length > 0) {
		const frame = stack.pop();
		if (frame.depth > 16) return {
			ok: false,
			error: `catalog exceeds depth 16`
		};
		for (const key of Object.keys(frame.source)) {
			const value = frame.source[key];
			entries += 1;
			if (entries > 2e4) return {
				ok: false,
				error: `catalog exceeds ${PLUGIN_LANGUAGE_CATALOG_MAX_ENTRIES} entries`
			};
			if (key.length === 0 || key.length > 128 || DANGEROUS_CATALOG_KEYS.has(key) || hasUnsafeCatalogKeyCharacter(key)) return {
				ok: false,
				error: `catalog key ${key || "(empty)"} is not safe`
			};
			const path = frame.path ? `${frame.path}.${key}` : key;
			if (protectedTranslation(path) && !(isCatalogObject(value) && translatablePluginChromeContainer(path))) return {
				ok: false,
				error: `catalog cannot replace protected security copy at ${path}`
			};
			if (typeof value === "string") {
				if (value.length > 8192) return {
					ok: false,
					error: `translation at ${path} exceeds 8192 characters`
				};
				if (frame.target) frame.target[key] = value;
				continue;
			}
			if (!isCatalogObject(value)) return {
				ok: false,
				error: `translation at ${path} must be a string or object`
			};
			if (seen.has(value)) return {
				ok: false,
				error: `catalog contains a repeated or cyclic object at ${path}`
			};
			seen.add(value);
			const child = frame.target ? {} : null;
			if (frame.target && child) frame.target[key] = child;
			stack.push({
				source: value,
				target: child,
				path,
				depth: frame.depth + 1
			});
		}
	}
	return {
		ok: true,
		catalog,
		entries
	};
}
var import_react = /* @__PURE__ */ __toESM(require_react());
var requestGeneration = 0;
var changeSubscriptionStarted = false;
const usePluginLanguagePackStore = create()((set) => ({
	packs: [],
	loaded: false,
	fetchPacks: async () => {
		const generation = ++requestGeneration;
		const api = window.api?.plugins;
		if (!api?.listLanguagePacks) {
			if (generation === requestGeneration) set({
				packs: [],
				loaded: true
			});
			return;
		}
		try {
			const response = await api.listLanguagePacks();
			const packs = Array.isArray(response) ? response.filter(isPluginLanguagePackRegistration) : [];
			if (!Array.isArray(response)) console.warn(`[plugins] Ignoring non-array language-pack list (${typeof response})`);
			else if (packs.length !== response.length) console.warn(`[plugins] Ignoring ${response.length - packs.length} of ${response.length} malformed language packs`);
			if (generation === requestGeneration) set({
				packs,
				loaded: true
			});
		} catch {
			if (generation === requestGeneration) set({
				packs: [],
				loaded: true
			});
		}
	}
}));
function ensurePluginLanguagePacksLoaded() {
	const state = usePluginLanguagePackStore.getState();
	if (!state.loaded) state.fetchPacks();
	if (!changeSubscriptionStarted && window.api?.plugins?.onChanged) {
		changeSubscriptionStarted = true;
		window.api.plugins.onChanged((event) => {
			if (event?.contentPacksChanged ?? true) usePluginLanguagePackStore.getState().fetchPacks();
		});
	}
}
function usePluginLanguagePacks() {
	const packs = usePluginLanguagePackStore((state) => state.packs);
	(0, import_react.useEffect)(() => ensurePluginLanguagePacksLoaded(), []);
	return packs;
}
export { usePluginLanguagePacks as n, usePluginLanguagePackStore as t };
