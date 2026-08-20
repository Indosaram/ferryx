import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn, t as Button } from "./button-DszXJEV6.js";
import { t as ExternalLink } from "./external-link-BrcDtGAn.js";
import { t as LoaderCircle } from "./loader-circle-CRZpWdsi.js";
import { t as Lock } from "./lock-Che1sB2g.js";
import { op as getActiveRuntimeTarget, t as useAppStore } from "./store-CgXrfmaH.js";
import { t as Label } from "./label-D-n9s_wS.js";
import { t as useMountedRef } from "./useMountedRef-1omUd-IV.js";
import { d as buildLinearWorkspaceApiSettingsUrl, l as buildLinearPersonalApiKeySettingsUrl } from "./github-links-C1M8w9wX.js";
import { a as DialogFooter, i as DialogDescription, o as DialogHeader, r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
import { t as Input } from "./input-DV5rpysh.js";
const CLOSED_LINEAR_API_KEY_DIALOG_STATE = Object.freeze({
	apiKeyDraft: "",
	connectState: "idle",
	connectError: null
});
function createLinearApiKeyDialogState() {
	return CLOSED_LINEAR_API_KEY_DIALOG_STATE;
}
function resolveLinearApiKeyDialogState(state, open) {
	if (open) return state;
	if (state.apiKeyDraft === "" && state.connectState === "idle" && state.connectError === null) return state;
	return CLOSED_LINEAR_API_KEY_DIALOG_STATE;
}
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function LinearApiKeyDialog({ open, onOpenChange, workspace, title, description, connectLabel, onConnected, overlayClassName, contentClassName }) {
	const settings = useAppStore((s) => s.settings);
	const connectLinear = useAppStore((s) => s.connectLinear);
	const mountedRef = useMountedRef();
	const apiKeyInputId = (0, import_react.useId)();
	const apiKeyErrorId = (0, import_react.useId)();
	const [dialogState, setDialogState] = (0, import_react.useState)(createLinearApiKeyDialogState);
	const runtimeTarget = (0, import_react.useMemo)(() => getActiveRuntimeTarget(settings), [settings]);
	const personalKeyUrl = buildLinearPersonalApiKeySettingsUrl(workspace?.organizationUrlKey);
	const workspaceApiUrl = buildLinearWorkspaceApiSettingsUrl(workspace?.organizationUrlKey);
	const submitLabel = connectLabel ?? (workspace ? "Update access" : "Connect");
	const resolvedDialogState = resolveLinearApiKeyDialogState(dialogState, open);
	if (resolvedDialogState !== dialogState) setDialogState(resolvedDialogState);
	const { apiKeyDraft, connectState, connectError } = resolvedDialogState;
	const handleOpenChange = (nextOpen) => {
		if (connectState !== "connecting") onOpenChange(nextOpen);
	};
	const handleConnect = async () => {
		const apiKey = apiKeyDraft.trim();
		if (!apiKey || connectState === "connecting") return;
		setDialogState((current) => ({
			...current,
			connectState: "connecting",
			connectError: null
		}));
		try {
			const result = await connectLinear(apiKey);
			if (!mountedRef.current) return;
			if (result.ok) {
				setDialogState(createLinearApiKeyDialogState());
				onOpenChange(false);
				onConnected?.();
				return;
			}
			setDialogState((current) => ({
				...current,
				connectState: "error",
				connectError: result.error
			}));
		} catch (error) {
			if (mountedRef.current) setDialogState((current) => ({
				...current,
				connectState: "error",
				connectError: error instanceof Error ? error.message : "Connection failed"
			}));
		}
	};
	const resolvedTitle = title ?? (workspace ? `Update Linear access for ${workspace.organizationName}` : "Add Linear access");
	const resolvedDescription = description ?? (workspace ? `Paste a Personal API key for ${workspace.organizationName}. If this workspace is already connected, Orca replaces its stored key.` : "Paste a Personal API key for the Linear workspace you want Orca to use. If that workspace is already connected, Orca replaces its stored key.");
	const storageCopy = runtimeTarget.kind === "environment" ? "This key is stored by the active remote runtime." : "Local runtime keys are stored on this device using Electron encrypted storage when available.";
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
		open,
		onOpenChange: handleOpenChange,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogContent, {
			overlayClassName,
			className: cn("sm:max-w-lg", contentClassName),
			onKeyDown: (event) => {
				if (event.key === "Enter" && apiKeyDraft.trim() && connectState !== "connecting") {
					event.preventDefault();
					handleConnect();
				}
			},
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, {
					className: "gap-3",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, {
						className: "leading-tight",
						children: resolvedTitle
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, { children: resolvedDescription })]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "space-y-3",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "space-y-2",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
								htmlFor: apiKeyInputId,
								className: "text-xs",
								children: translate("auto.components.linear.api.key.dialog.7d498f653c", "Personal API key")
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
								id: apiKeyInputId,
								autoFocus: true,
								type: "password",
								placeholder: translate("auto.components.linear.api.key.dialog.edec49dfae", "lin_api_..."),
								value: apiKeyDraft,
								onChange: (event) => {
									const nextDraft = event.target.value;
									setDialogState((current) => ({
										apiKeyDraft: nextDraft,
										connectState: current.connectState === "error" ? "idle" : current.connectState,
										connectError: current.connectState === "error" ? null : current.connectError
									}));
								},
								disabled: connectState === "connecting",
								"aria-invalid": connectState === "error",
								"aria-describedby": connectState === "error" ? apiKeyErrorId : void 0
							})]
						}),
						connectState === "error" && connectError ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							id: apiKeyErrorId,
							className: "text-xs text-destructive",
							children: connectError
						}) : null,
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "space-y-2 text-xs leading-relaxed text-muted-foreground",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", { children: [
									translate("auto.components.linear.api.key.dialog.af52a6227f", "Create a Personal API key from Account > Security & Access."),
									" ",
									!workspace ? translate("auto.components.linear.api.key.dialog.c9889a09f8", "Use Linear to choose the intended workspace before creating the key.") : null
								] }),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", { children: translate("auto.components.linear.api.key.dialog.d56d3629f4", "Prefer full access when Orca should show every team the account can access in that workspace. Restricted keys only expose permitted teams, and private teams require the key owner to have access.") }),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", { children: translate("auto.components.linear.api.key.dialog.e3100b36b9", "If member API keys are blocked, ask a workspace admin to allow them from workspace API settings.") }),
								/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
									className: "flex flex-wrap items-center gap-2 pt-1",
									children: [
										/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
											type: "button",
											className: "inline-flex items-center gap-1 text-primary underline-offset-2 hover:underline",
											onClick: () => window.api.shell.openUrl(personalKeyUrl),
											children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExternalLink, { className: "size-3" }), translate("auto.components.linear.api.key.dialog.dc7ccb0f7c", "Personal API keys")]
										}),
										/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
											className: "text-muted-foreground/60",
											children: "|"
										}),
										/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
											type: "button",
											className: "inline-flex items-center gap-1 text-primary underline-offset-2 hover:underline",
											onClick: () => window.api.shell.openUrl(workspaceApiUrl),
											children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExternalLink, { className: "size-3" }), translate("auto.components.linear.api.key.dialog.e603ee9156", "Workspace API settings")]
										})
									]
								})
							]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
							className: "flex items-center gap-1.5 text-[11px] text-muted-foreground/70",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Lock, { className: "size-3 shrink-0" }), storageCopy]
						})
					]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogFooter, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					variant: "ghost",
					onClick: () => onOpenChange(false),
					disabled: connectState === "connecting",
					children: translate("auto.components.linear.api.key.dialog.f8f704a019", "Cancel")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					onClick: () => void handleConnect(),
					disabled: !apiKeyDraft.trim() || connectState === "connecting",
					children: connectState === "connecting" ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-4 animate-spin" }), translate("auto.components.linear.api.key.dialog.834a52c084", "Verifying...")] }) : submitLabel
				})] })
			]
		})
	});
}
export { LinearApiKeyDialog as t };
