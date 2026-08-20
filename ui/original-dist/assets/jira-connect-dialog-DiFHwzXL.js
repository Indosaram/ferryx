import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn, t as Button } from "./button-DszXJEV6.js";
import { t as LoaderCircle } from "./loader-circle-CRZpWdsi.js";
import { t as Lock } from "./lock-Che1sB2g.js";
import { id as hasRemoteProviderRuntime, t as useAppStore } from "./store-CgXrfmaH.js";
import { t as Label } from "./label-D-n9s_wS.js";
import { n as ToggleGroupItem, t as ToggleGroup } from "./toggle-group-CZlhA2tW.js";
import { t as useMountedRef } from "./useMountedRef-1omUd-IV.js";
import { a as DialogFooter, i as DialogDescription, o as DialogHeader, r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
import { t as Input } from "./input-DV5rpysh.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function JiraConnectDialog({ open, onOpenChange, onConnected, overlayClassName, contentClassName }) {
	const connectJira = useAppStore((s) => s.connectJira);
	const settings = useAppStore((s) => s.settings);
	const mountedRef = useMountedRef();
	const siteUrlId = (0, import_react.useId)();
	const emailId = (0, import_react.useId)();
	const tokenId = (0, import_react.useId)();
	const errorId = (0, import_react.useId)();
	const [instanceType, setInstanceType] = (0, import_react.useState)("cloud");
	const [serverAuthMethod, setServerAuthMethod] = (0, import_react.useState)("pat");
	const [siteUrl, setSiteUrl] = (0, import_react.useState)("");
	const [email, setEmail] = (0, import_react.useState)("");
	const [apiToken, setApiToken] = (0, import_react.useState)("");
	const [connectState, setConnectState] = (0, import_react.useState)("idle");
	const [connectError, setConnectError] = (0, import_react.useState)(null);
	(0, import_react.useLayoutEffect)(() => {
		if (!open) return;
		setInstanceType("cloud");
		setServerAuthMethod("pat");
		setSiteUrl("");
		setEmail("");
		setApiToken("");
		setConnectState("idle");
		setConnectError(null);
	}, [open]);
	const isServer = instanceType === "server";
	const isServerBasic = isServer && serverAuthMethod === "basic";
	const needsIdentity = !isServer || isServerBasic;
	const canSubmit = Boolean(siteUrl.trim()) && (!needsIdentity || Boolean(email.trim())) && Boolean(apiToken.trim()) && connectState !== "connecting";
	const credentialStorageCopy = hasRemoteProviderRuntime(settings) ? "Your token is sent to the selected remote runtime and stored there with runtime-supported encryption." : "Your token is stored locally and encrypted when local runtime storage supports it.";
	const clearErrorOnEdit = () => {
		if (connectState === "error") {
			setConnectState("idle");
			setConnectError(null);
		}
	};
	const clearCredentialsOnModeSwitch = () => {
		setEmail("");
		setApiToken("");
		clearErrorOnEdit();
	};
	const handleOpenChange = (nextOpen) => {
		if (connectState !== "connecting") onOpenChange(nextOpen);
	};
	const handleConnect = async () => {
		const trimmedSite = siteUrl.trim();
		const trimmedEmail = email.trim();
		const trimmedToken = apiToken.trim();
		if (!trimmedSite || needsIdentity && !trimmedEmail || !trimmedToken || connectState === "connecting") return;
		setConnectState("connecting");
		setConnectError(null);
		try {
			const result = await connectJira({
				siteUrl: trimmedSite,
				email: needsIdentity ? trimmedEmail : "",
				apiToken: trimmedToken,
				authType: instanceType
			});
			if (!mountedRef.current) return;
			if (result.ok) {
				setSiteUrl("");
				setEmail("");
				setApiToken("");
				setInstanceType("cloud");
				setServerAuthMethod("pat");
				setConnectState("idle");
				onOpenChange(false);
				onConnected?.();
				return;
			}
			setConnectState("error");
			setConnectError(result.error);
		} catch (error) {
			if (mountedRef.current) {
				setConnectState("error");
				setConnectError(error instanceof Error ? error.message : "Connection failed");
			}
		}
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
		open,
		onOpenChange: handleOpenChange,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogContent, {
			overlayClassName,
			className: cn("sm:max-w-md", contentClassName),
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, {
				className: "gap-3",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, {
					className: "leading-tight",
					children: translate("auto.components.jira.connect.dialog.8388bdea2b", "Connect Jira site")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, { children: !isServer ? translate("auto.components.jira.connect.dialog.d785c42b8b", "Use a Jira Cloud site URL, Atlassian email, and API token to browse issues.") : isServerBasic ? translate("auto.components.jira.connect.dialog.1d947a07ab", "Use a self-hosted Jira base URL, username, and password to browse issues.") : translate("auto.components.jira.connect.dialog.2e2b69e48e", "Use a self-hosted Jira base URL and a personal access token to browse issues.") })]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("form", {
				className: "flex flex-col gap-4",
				noValidate: true,
				onSubmit: (event) => {
					event.preventDefault();
					handleConnect();
				},
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex flex-col gap-3",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ToggleGroup, {
							type: "single",
							variant: "outline",
							value: instanceType,
							disabled: connectState === "connecting",
							onValueChange: (value) => {
								if (!value || connectState === "connecting") return;
								setInstanceType(value);
								clearCredentialsOnModeSwitch();
							},
							"aria-label": translate("auto.components.jira.connect.dialog.b67e919bd5", "Jira instance type"),
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ToggleGroupItem, {
								value: "cloud",
								className: "h-8 px-3 text-xs",
								children: translate("auto.components.jira.connect.dialog.17787d6e4b", "Atlassian Cloud")
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ToggleGroupItem, {
								value: "server",
								className: "h-8 px-3 text-xs",
								children: translate("auto.components.jira.connect.dialog.bc7a831773", "Self-hosted")
							})]
						}),
						isServer ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ToggleGroup, {
							type: "single",
							variant: "outline",
							value: serverAuthMethod,
							disabled: connectState === "connecting",
							onValueChange: (value) => {
								if (!value || connectState === "connecting") return;
								setServerAuthMethod(value);
								clearCredentialsOnModeSwitch();
							},
							"aria-label": translate("auto.components.jira.connect.dialog.f49708c369", "Jira authentication method"),
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ToggleGroupItem, {
								value: "pat",
								className: "h-8 px-3 text-xs",
								children: translate("auto.components.jira.connect.dialog.730d973bae", "Personal access token")
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ToggleGroupItem, {
								value: "basic",
								className: "h-8 px-3 text-xs",
								children: translate("auto.components.jira.connect.dialog.84a810dd0e", "Username & password")
							})]
						}) : null,
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "space-y-2",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
								htmlFor: siteUrlId,
								className: "text-xs",
								children: isServer ? translate("auto.components.jira.connect.dialog.3489e186d6", "Jira site URL") : translate("auto.components.jira.connect.dialog.e176f9d0c5", "Jira Cloud site URL")
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
								id: siteUrlId,
								autoFocus: true,
								placeholder: isServer ? translate("auto.components.jira.connect.dialog.cbc27fa599", "https://jira.example.com") : translate("auto.components.jira.connect.dialog.70fcd360c4", "https://example.atlassian.net"),
								value: siteUrl,
								onChange: (event) => {
									setSiteUrl(event.target.value);
									clearErrorOnEdit();
								},
								disabled: connectState === "connecting"
							})]
						}),
						needsIdentity ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "space-y-2",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
								htmlFor: emailId,
								className: "text-xs",
								children: isServerBasic ? translate("auto.components.jira.connect.dialog.8d1223fa5c", "Username") : translate("auto.components.jira.connect.dialog.2849ddb295", "Atlassian email")
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
								id: emailId,
								type: isServerBasic ? "text" : "email",
								placeholder: isServerBasic ? translate("auto.components.jira.connect.dialog.be9eba0a1b", "username") : translate("auto.components.jira.connect.dialog.e91b9a4073", "you@example.com"),
								value: email,
								onChange: (event) => {
									setEmail(event.target.value);
									clearErrorOnEdit();
								},
								disabled: connectState === "connecting"
							})]
						}) : null,
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "space-y-2",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
								htmlFor: tokenId,
								className: "text-xs",
								children: isServerBasic ? translate("auto.components.jira.connect.dialog.70035652d7", "Password") : isServer ? translate("auto.components.jira.connect.dialog.730d973bae", "Personal access token") : translate("auto.components.jira.connect.dialog.3d81bf3ab3", "API token")
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
								id: tokenId,
								type: "password",
								placeholder: isServerBasic ? translate("auto.components.jira.connect.dialog.c50abbf340", "Jira account password") : isServer ? translate("auto.components.jira.connect.dialog.8b9c7b9e7b", "Jira personal access token") : translate("auto.components.jira.connect.dialog.7b3967c12f", "Atlassian API token"),
								value: apiToken,
								onChange: (event) => {
									setApiToken(event.target.value);
									clearErrorOnEdit();
								},
								disabled: connectState === "connecting",
								"aria-invalid": connectState === "error",
								"aria-describedby": connectState === "error" ? errorId : void 0
							})]
						}),
						connectState === "error" && connectError ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							id: errorId,
							className: "text-xs text-destructive",
							children: connectError
						}) : null,
						isServerBasic ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "text-xs text-muted-foreground",
							children: translate("auto.components.jira.connect.dialog.d8737db691", "Use your Jira Server or Data Center account username and password.")
						}) : isServer ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "text-xs text-muted-foreground",
							children: translate("auto.components.jira.connect.dialog.ccfb086d3e", "Create a personal access token in your Jira profile under Personal Access Tokens.")
						}) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
							className: "text-xs text-muted-foreground",
							children: [
								translate("auto.components.jira.connect.dialog.8090504a3e", "Create a token in"),
								" ",
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
									type: "button",
									className: "text-primary underline-offset-2 hover:underline",
									onClick: () => window.api.shell.openUrl("https://id.atlassian.com/manage-profile/security/api-tokens"),
									children: translate("auto.components.jira.connect.dialog.fdd26d81cc", "Atlassian account settings")
								}),
								"."
							]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
							className: "flex items-center gap-1.5 text-[11px] text-muted-foreground/70",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Lock, { className: "size-3 shrink-0" }), credentialStorageCopy]
						})
					]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogFooter, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					type: "button",
					variant: "ghost",
					onClick: () => onOpenChange(false),
					disabled: connectState === "connecting",
					children: translate("auto.components.jira.connect.dialog.79e7aaed39", "Cancel")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					type: "submit",
					disabled: !canSubmit,
					children: connectState === "connecting" ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-4 animate-spin" }), translate("auto.components.jira.connect.dialog.4a2ab52781", "Verifying…")] }) : translate("auto.components.jira.connect.dialog.63ce735809", "Connect")
				})] })]
			})]
		})
	});
}
export { JiraConnectDialog as t };
