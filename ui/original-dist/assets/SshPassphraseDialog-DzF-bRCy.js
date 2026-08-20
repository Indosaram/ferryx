import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as Button } from "./button-DszXJEV6.js";
import { t as useAppStore } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import { n as toast } from "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import "./useMountedRef-1omUd-IV.js";
import { a as DialogFooter, i as DialogDescription, o as DialogHeader, r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
import { t as Input } from "./input-DV5rpysh.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function SshPassphraseDialog() {
	const request = useAppStore((s) => s.sshCredentialQueue[0] ?? null);
	const targetLabels = useAppStore((s) => s.sshTargetLabels);
	const removeRequest = useAppStore((s) => s.removeSshCredentialRequest);
	const [value, setValue] = (0, import_react.useState)("");
	const [submitting, setSubmitting] = (0, import_react.useState)(false);
	const inputRef = (0, import_react.useRef)(null);
	const focusFrameRef = (0, import_react.useRef)(null);
	const open = request !== null;
	const requestId = request?.requestId;
	const [prevRequestId, setPrevRequestId] = (0, import_react.useState)(requestId);
	if (requestId !== prevRequestId) {
		setPrevRequestId(requestId);
		if (requestId) {
			setValue("");
			setSubmitting(false);
		}
	}
	const setInputRef = (0, import_react.useCallback)((input) => {
		inputRef.current = input;
		if (focusFrameRef.current !== null) {
			cancelAnimationFrame(focusFrameRef.current);
			focusFrameRef.current = null;
		}
		if (!input || !requestId) return;
		focusFrameRef.current = requestAnimationFrame(() => {
			focusFrameRef.current = null;
			if (inputRef.current === input) input.focus();
		});
	}, [requestId]);
	const handleSubmit = (0, import_react.useCallback)(async () => {
		if (!request || !value) return;
		setSubmitting(true);
		try {
			await window.api.ssh.submitCredential({
				requestId: request.requestId,
				value
			});
			removeRequest(request.requestId);
		} catch (err) {
			toast.error(err instanceof Error ? err.message : translate("auto.components.settings.SshPassphraseDialog.b8e88fd0de", "Failed to submit SSH credential"));
			setSubmitting(false);
		}
	}, [
		request,
		value,
		removeRequest
	]);
	const handleCancel = (0, import_react.useCallback)(async () => {
		if (request) {
			setSubmitting(true);
			try {
				await window.api.ssh.submitCredential({
					requestId: request.requestId,
					value: null
				});
				removeRequest(request.requestId);
			} catch (err) {
				toast.error(err instanceof Error ? err.message : translate("auto.components.settings.SshPassphraseDialog.c55f105262", "Failed to cancel SSH credential request"));
				setSubmitting(false);
			}
		}
	}, [request, removeRequest]);
	if (!request) return null;
	const label = targetLabels.get(request.targetId) ?? request.targetId;
	const isPassword = request.kind === "password";
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
		open,
		onOpenChange: (isOpen) => !isOpen && void handleCancel(),
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogContent, {
			showCloseButton: false,
			overlayClassName: "!z-[140]",
			className: "!z-[150] max-w-[360px]",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, {
					className: "text-sm",
					children: isPassword ? translate("auto.components.settings.SshPassphraseDialog.106bd57f4a", "SSH Password") : translate("auto.components.settings.SshPassphraseDialog.1f3dde805d", "SSH Key Passphrase")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, {
					className: "text-xs",
					children: isPassword ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
						translate("auto.components.settings.SshPassphraseDialog.dbf9b6f2d0", "Enter the password for"),
						" ",
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "font-medium",
							children: label
						})
					] }) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
						translate("auto.components.settings.SshPassphraseDialog.ce4fdf7914", "Enter the passphrase for"),
						" ",
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "font-medium",
							children: label
						})
					] })
				})] }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("label", {
					htmlFor: "ssh-credential-input",
					className: "text-[11px] font-medium text-muted-foreground mb-1 block",
					children: isPassword ? translate("auto.components.settings.SshPassphraseDialog.cab3d5f5a5", "Password for {{value0}}", { value0: request.detail }) : translate("auto.components.settings.SshPassphraseDialog.8a349e3fac", "Passphrase for {{value0}}", { value0: request.detail })
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
					id: "ssh-credential-input",
					ref: setInputRef,
					type: "password",
					value,
					onChange: (e) => setValue(e.target.value),
					onKeyDown: (e) => {
						if (e.key === "Enter") {
							e.preventDefault();
							handleSubmit();
						}
					},
					placeholder: isPassword ? translate("auto.components.settings.SshPassphraseDialog.abaa0dc653", "Enter password") : translate("auto.components.settings.SshPassphraseDialog.c3ce71aad6", "Enter passphrase"),
					className: "h-8 text-sm",
					disabled: submitting
				})] }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogFooter, {
					className: "mt-1",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						variant: "outline",
						size: "sm",
						onClick: () => void handleCancel(),
						disabled: submitting,
						children: translate("auto.components.settings.SshPassphraseDialog.d5a234456f", "Cancel")
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						size: "sm",
						onClick: () => void handleSubmit(),
						disabled: !value || submitting,
						children: isPassword ? translate("auto.components.settings.SshPassphraseDialog.bec2c1318f", "Connect") : translate("auto.components.settings.SshPassphraseDialog.405066423c", "Unlock")
					})]
				})
			]
		})
	});
}
export { SshPassphraseDialog };
