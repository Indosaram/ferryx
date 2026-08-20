import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { sc as WINDOWS_GIT_BASH_SHELL } from "./store-CgXrfmaH.js";
var gwindows_logo_default = "" + new URL("gwindows_logo-HIMo1C3u.svg", import.meta.url).href;
require_react();
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function PowerShellIcon({ size = 14 }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", {
		width: size,
		height: size,
		viewBox: "0 0 24 24",
		xmlns: "http://www.w3.org/2000/svg",
		"aria-hidden": true,
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", {
				x: "1.5",
				y: "3",
				width: "21",
				height: "18",
				rx: "2.5",
				fill: "#2E74B5"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", {
				d: "M6.5 7.3l6.2 4.7-6.2 4.7-1.2-1.2 4.6-3.5-4.6-3.5z",
				fill: "#ffffff",
				fillRule: "nonzero"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", {
				x: "12.5",
				y: "15.3",
				width: "5",
				height: "1.4",
				rx: "0.4",
				fill: "#ffffff"
			})
		]
	});
}
function CmdIcon({ size = 14 }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", {
		width: size,
		height: size,
		viewBox: "0 0 24 24",
		xmlns: "http://www.w3.org/2000/svg",
		"aria-hidden": true,
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", {
				x: "1.5",
				y: "3",
				width: "21",
				height: "18",
				rx: "2.5",
				fill: "#1F1F1F"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", {
				d: "M5.8 8l4 4-4 4-1.1-1.1L7.7 12 4.7 9.1z",
				fill: "#ffffff"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", {
				x: "10.5",
				y: "15",
				width: "8",
				height: "1.4",
				rx: "0.4",
				fill: "#ffffff"
			})
		]
	});
}
function WslIcon({ size = 14 }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", {
		width: size,
		height: size,
		viewBox: "0 0 24 24",
		xmlns: "http://www.w3.org/2000/svg",
		"aria-hidden": true,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", {
			x: "1.5",
			y: "3",
			width: "21",
			height: "18",
			rx: "2.5",
			fill: "#F4B400"
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("text", {
			x: "12",
			y: "15.2",
			textAnchor: "middle",
			fontSize: "7",
			fontWeight: "800",
			fill: "#1F1F1F",
			fontFamily: "system-ui, -apple-system, sans-serif",
			children: translate("auto.components.tab.bar.shell.icons.e9b2e70613", "WSL")
		})]
	});
}
function GitBashIcon({ size = 14 }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("img", {
		src: gwindows_logo_default,
		alt: "",
		"aria-hidden": true,
		width: size,
		height: size,
		className: "block",
		style: {
			width: size,
			height: size
		}
	});
}
function GenericTerminalIcon({ size = 14 }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", {
		width: size,
		height: size,
		viewBox: "0 0 24 24",
		xmlns: "http://www.w3.org/2000/svg",
		"aria-hidden": true,
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", {
				x: "1.5",
				y: "3",
				width: "21",
				height: "18",
				rx: "2.5",
				fill: "#000000"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", {
				d: "M6 7.5 L11.5 12 L6 16.5",
				stroke: "#ffffff",
				strokeWidth: "3.2",
				strokeLinecap: "round",
				strokeLinejoin: "round",
				fill: "none"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", {
				d: "M12.5 16.5 L18 16.5",
				stroke: "#ffffff",
				strokeWidth: "2.6",
				strokeLinecap: "round",
				fill: "none"
			})
		]
	});
}
function ShellIcon({ shell, size = 14 }) {
	const normalized = (shell ?? "").toLowerCase();
	const normalizedName = normalized.replaceAll("\\", "/").split("/").pop();
	if (normalized === "powershell.exe" || normalized === "pwsh.exe") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PowerShellIcon, { size });
	if (normalized === "cmd.exe") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CmdIcon, { size });
	if (normalized === "wsl.exe" || normalized.startsWith("wsl")) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(WslIcon, { size });
	if (normalized === "git-bash" || normalizedName === "bash.exe") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(GitBashIcon, { size });
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(GenericTerminalIcon, { size });
}
export { ShellIcon as t };
