import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn } from "./button-DszXJEV6.js";
import "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import "./useMountedRef-1omUd-IV.js";
import "./useShortcutLabel-C-KRYtlB.js";
import "./dialog-BbelfMSB.js";
import "./find-query-bounds-BKNiI6IV.js";
import { t as ImageViewer } from "./ImageViewer-EZPfzHX3.js";
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function ImageDiffPane({ label, content, filePath, mimeType, layout }) {
	const isIntrinsicLayout = layout === "intrinsic";
	if (!content) return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: cn("flex min-h-0 flex-col overflow-hidden rounded-md bg-muted/10", isIntrinsicLayout ? "h-auto" : "h-full"),
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "px-3 py-2 text-xs font-medium text-muted-foreground",
			children: label
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: cn("flex items-center justify-center bg-muted/20 p-6 text-sm text-muted-foreground", isIntrinsicLayout ? "min-h-32" : "flex-1"),
			children: translate("auto.components.editor.ImageDiffViewer.fb0ae4f3c0", "No preview")
		})]
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: cn("flex min-h-0 flex-col overflow-hidden rounded-md bg-muted/10", isIntrinsicLayout ? "h-auto" : "h-full"),
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "px-3 py-2 text-xs font-medium text-muted-foreground",
			children: label
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: cn("min-h-0", isIntrinsicLayout ? "flex-none" : "flex-1"),
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ImageViewer, {
				content,
				filePath,
				mimeType,
				layout
			})
		})]
	});
}
function ImageDiffViewer({ originalContent, modifiedContent, filePath, mimeType, sideBySide, layout = "fill" }) {
	const isIntrinsicLayout = layout === "intrinsic";
	const gridRowStyle = !sideBySide && !isIntrinsicLayout ? { gridTemplateRows: `${originalContent ? "minmax(32rem, 1fr)" : "auto"} ${modifiedContent ? "minmax(32rem, 1fr)" : "auto"}` } : void 0;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: cn("grid min-h-0 gap-3 p-3", isIntrinsicLayout ? "h-auto" : "h-full", sideBySide ? "grid-cols-2" : "grid-cols-1", !sideBySide && !isIntrinsicLayout && "overflow-y-auto scrollbar-editor"),
		style: gridRowStyle,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ImageDiffPane, {
			label: translate("auto.components.editor.ImageDiffViewer.57aac3979a", "Original"),
			content: originalContent,
			filePath,
			mimeType,
			layout
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ImageDiffPane, {
			label: translate("auto.components.editor.ImageDiffViewer.a651be62b0", "Modified"),
			content: modifiedContent,
			filePath,
			mimeType,
			layout
		})]
	});
}
export { ImageDiffViewer as default };
