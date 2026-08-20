import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { d as isPluginUiLanguage, i as setRendererPluginLanguagePacks, l as resolveUiLocale, r as i18n, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { r as I18nContext } from "./useTranslation-DX5IRIhk.js";
import { t as useAppStore } from "./store-CgXrfmaH.js";
import { n as usePluginLanguagePacks } from "./plugin-language-packs-dIIzfC0p.js";
(function polyfill() {
	const relList = document.createElement("link").relList;
	if (relList && relList.supports && relList.supports("modulepreload")) return;
	for (const link of document.querySelectorAll("link[rel=\"modulepreload\"]")) processPreload(link);
	new MutationObserver((mutations) => {
		for (const mutation of mutations) {
			if (mutation.type !== "childList") continue;
			for (const node of mutation.addedNodes) if (node.tagName === "LINK" && node.rel === "modulepreload") processPreload(node);
		}
	}).observe(document, {
		childList: true,
		subtree: true
	});
	function getFetchOpts(link) {
		const fetchOpts = {};
		if (link.integrity) fetchOpts.integrity = link.integrity;
		if (link.referrerPolicy) fetchOpts.referrerPolicy = link.referrerPolicy;
		if (link.crossOrigin === "use-credentials") fetchOpts.credentials = "include";
		else if (link.crossOrigin === "anonymous") fetchOpts.credentials = "omit";
		else fetchOpts.credentials = "same-origin";
		return fetchOpts;
	}
	function processPreload(link) {
		if (link.ep) return;
		link.ep = true;
		const fetchOpts = getFetchOpts(link);
		fetch(link.href, fetchOpts);
	}
})();
var import_react = /* @__PURE__ */ __toESM(require_react());
function I18nextProvider({ i18n: i18n$1, defaultNS, children }) {
	const value = (0, import_react.useMemo)(() => ({
		i18n: i18n$1,
		defaultNS
	}), [i18n$1, defaultNS]);
	return (0, import_react.createElement)(I18nContext.Provider, { value }, children);
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function I18nProvider({ children }) {
	const uiLanguage = useAppStore((state) => state.settings?.uiLanguage ?? null);
	const pluginLanguagePacks = usePluginLanguagePacks();
	const selectedPluginLanguage = pluginLanguagePacks.find((pack) => pack.id === uiLanguage);
	const locale = uiLanguage === null ? null : selectedPluginLanguage?.resourceLanguage ?? (isPluginUiLanguage(uiLanguage) ? "en" : resolveUiLocale(uiLanguage));
	const requestedLocale = (0, import_react.useRef)(null);
	(0, import_react.useEffect)(() => {
		setRendererPluginLanguagePacks(pluginLanguagePacks);
		requestedLocale.current = null;
	}, [pluginLanguagePacks]);
	(0, import_react.useEffect)(() => {
		if (locale === null || requestedLocale.current === locale) return;
		requestedLocale.current = locale;
		i18n.changeLanguage(locale);
	}, [locale, pluginLanguagePacks]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(I18nextProvider, {
		i18n,
		children
	});
}
export { I18nProvider as t };
