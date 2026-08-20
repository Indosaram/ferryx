import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as isLazyChunkLoadError } from "./lazy-with-retry-pSZJrSfN.js";
import { n as init_defineProperty, t as _defineProperty } from "./defineProperty-BAtR-r70.js";
import { n as cn, t as Button } from "./button-DszXJEV6.js";
import { t as RotateCw } from "./rotate-cw-DBuQrdY8.js";
import { t as TriangleAlert } from "./triangle-alert-HrLt1y9s.js";
import { n as reportReactErrorBoundaryCrash } from "./react-error-boundary-reporting-CkObujra.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
init_defineProperty();
var RecoverableRenderErrorBoundary = class extends import_react.Component {
	constructor(..._args) {
		super(..._args);
		_defineProperty(this, "state", {
			error: null,
			resetKey: this.props.resetKey
		});
		_defineProperty(this, "handleReset", () => {
			this.setState({ error: null });
		});
	}
	static getDerivedStateFromProps(props, state) {
		if (props.resetKey !== state.resetKey) return {
			error: null,
			resetKey: props.resetKey
		};
		return null;
	}
	static getDerivedStateFromError(error) {
		return { error };
	}
	componentDidCatch(error, errorInfo) {
		console.error(`[${this.props.boundaryId}] render crash contained by boundary`, error, errorInfo);
		if (this.props.reportAsCrash === false) return;
		if (isLazyChunkLoadError(error)) return;
		reportReactErrorBoundaryCrash({
			boundaryId: this.props.boundaryId,
			surface: this.props.surface,
			error,
			errorInfo
		});
	}
	render() {
		if (!this.state.error) return this.props.children;
		if (this.props.fallback) return this.props.fallback({
			error: this.state.error,
			reset: this.handleReset
		});
		return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: cn("flex flex-col items-center justify-center gap-3 px-6 text-center text-sm text-muted-foreground", this.props.compact ? "min-h-9 py-2" : "h-full min-h-0 py-8", this.props.className),
			role: "alert",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "flex size-8 items-center justify-center rounded-full border border-destructive/25 bg-destructive/10 text-destructive",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "size-4" })
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "space-y-1",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "font-medium text-foreground",
						children: this.props.title ?? translate("auto.components.error.boundaries.RecoverableRenderErrorBoundary.ab855c11f4", "This part of Orca hit an error.")
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "max-w-md text-xs",
						children: this.props.description ?? translate("auto.components.error.boundaries.RecoverableRenderErrorBoundary.34a189ae0f", "The rest of the app is still running. Retry this surface or switch away and come back.")
					})]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
					type: "button",
					variant: "outline",
					size: "sm",
					onClick: this.handleReset,
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RotateCw, { className: "size-3.5" }), translate("auto.components.error.boundaries.RecoverableRenderErrorBoundary.55001880db", "Retry")]
				})
			]
		});
	}
};
export { RecoverableRenderErrorBoundary as t };
