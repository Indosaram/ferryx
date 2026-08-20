import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { l as createLucideIcon, n as cn } from "./button-DszXJEV6.js";
import { t as createLocalizedCatalog } from "./localized-catalog-DubKHKUR.js";
var AppWindow = createLucideIcon("app-window", [
	["rect", {
		x: "2",
		y: "4",
		width: "20",
		height: "16",
		rx: "2",
		key: "izxlao"
	}],
	["path", {
		d: "M10 4v4",
		key: "pp8u80"
	}],
	["path", {
		d: "M2 8h20",
		key: "d11cs7"
	}],
	["path", {
		d: "M6 4v4",
		key: "1svtjw"
	}]
]);
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
const getOpenInAppPresets = createLocalizedCatalog(() => [
	{
		id: "vscode",
		label: translate("auto.lib.open.in.app.catalog.173553f73a", "VS Code"),
		command: "code",
		faviconDomain: "code.visualstudio.com"
	},
	{
		id: "cursor",
		label: translate("auto.lib.open.in.app.catalog.d62b12e98a", "Cursor"),
		command: "cursor",
		faviconDomain: "cursor.com"
	},
	{
		id: "zed",
		label: translate("auto.lib.open.in.app.catalog.f8b8ca2711", "Zed"),
		command: "zed",
		faviconDomain: "zed.dev",
		iconClassName: "dark:invert"
	}
]);
function getOpenInAppPreset(application) {
	const command = application.command.trim().toLowerCase();
	return getOpenInAppPresets().find((preset) => preset.command === command) ?? null;
}
function isOpenInAppPresetAdded(applications, preset) {
	return applications.some((application) => application.command.trim().toLowerCase() === preset.command);
}
function OpenInApplicationIcon({ application, size = 14 }) {
	const preset = getOpenInAppPreset(application);
	if (preset) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("img", {
		src: `https://www.google.com/s2/favicons?domain=${preset.faviconDomain}&sz=64`,
		width: size,
		height: size,
		alt: "",
		"aria-hidden": true,
		className: cn("shrink-0", preset.iconClassName),
		style: { borderRadius: 2 }
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AppWindow, {
		width: size,
		height: size
	});
}
export { AppWindow as a, isOpenInAppPresetAdded as i, getOpenInAppPreset as n, getOpenInAppPresets as r, OpenInApplicationIcon as t };
