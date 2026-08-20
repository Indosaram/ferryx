import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as Button } from "./button-DszXJEV6.js";
import "./workspace-status-wl52y3xd.js";
import "./worktree-activation-BDsaiyMf.js";
import { t as LoaderCircle } from "./loader-circle-CRZpWdsi.js";
import { t as ServerOff } from "./server-off-xGmv1av8.js";
import { t as Server } from "./server-DYdwnXME.js";
import { t as useAppStore } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import { n as toast } from "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import { t as useMountedRef } from "./useMountedRef-1omUd-IV.js";
import "./selectors-XOBeaOSb.js";
import "./web-runtime-session-CN2syA39.js";
import "./agent-paste-draft-C2PA7vXu.js";
import "./agent-process-recognition-BB0O3DaN.js";
import "./terminal-pty-input-transaction-2UskR-Bm.js";
import "./web-session-tabs-sync-CYKZbAxS.js";
import "./pane-agent-owner-BPfoVAtS.js";
import "./native-chat-session-option-cache-DGE3h47U.js";
import "./github-links-C1M8w9wX.js";
import "./connection-context-BUPsamzR.js";
import "./localized-catalog-DubKHKUR.js";
import { r as runWorktreeDeleteWithToast } from "./delete-worktree-flow-RxB6NScm.js";
import "./preserved-branch-batch-toast-DHxeGO1o.js";
import { a as DialogFooter, i as DialogDescription, o as DialogHeader, r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function isForgetModalData(data) {
	if (!data || typeof data !== "object") return false;
	const candidate = data;
	return typeof candidate.worktreeId === "string" && candidate.resolution != null;
}
function ForgetSshWorkspaceDialog() {
	const modalData = useAppStore((s) => s.modalData);
	const closeModal = useAppStore((s) => s.closeModal);
	const hostLabel = useAppStore((s) => {
		const resolution$1 = isForgetModalData(s.modalData) ? s.modalData.resolution : null;
		const targetId = resolution$1 && resolution$1.kind !== "not-ssh" ? resolution$1.targetId : void 0;
		if (!targetId) return "";
		return s.sshTargetLabels.get(targetId) ?? s.removedSshTargetLabels.get(targetId) ?? targetId;
	});
	const [busy, setBusy] = (0, import_react.useState)(null);
	const mountedRef = useMountedRef();
	if (!isForgetModalData(modalData)) return null;
	const { worktreeId, displayName, resolution } = modalData;
	const canReconnect = resolution.kind === "disconnected";
	const done = () => {
		if (mountedRef.current) {
			setBusy(null);
			closeModal();
		}
	};
	const handleReconnectAndDelete = async () => {
		if (resolution.kind !== "disconnected") return;
		setBusy("reconnect");
		try {
			await window.api.ssh.connect({ targetId: resolution.targetId });
		} catch (err) {
			if (mountedRef.current) setBusy(null);
			toast.error(err instanceof Error ? err.message : translate("auto.components.sidebar.ForgetSshWorkspaceDialog.reconnectFailed", "Reconnection failed"));
			return;
		}
		closeModal();
		runWorktreeDeleteWithToast(worktreeId, displayName);
		if (mountedRef.current) setBusy(null);
	};
	const handleForget = async () => {
		setBusy("forget");
		try {
			const result = await useAppStore.getState().removeWorktree(worktreeId, false, { mode: "forget-local" });
			if (!result.ok) {
				toast.error(result.error);
				if (mountedRef.current) setBusy(null);
				return;
			}
			done();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : String(err));
			if (mountedRef.current) setBusy(null);
		}
	};
	const forgetDescription = translate("auto.components.sidebar.ForgetSshWorkspaceDialog.forgetBody", "Removes this workspace from Orca only. Files, the Git worktree, and branches on {{host}} are left untouched.", { host: hostLabel });
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
		open: true,
		onOpenChange: (open) => !open ? closeModal() : void 0,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogContent, {
			className: "sm:max-w-md gap-3 p-5",
			showCloseButton: false,
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, {
					className: "gap-1",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogTitle, {
						className: "flex items-center gap-2 text-sm font-semibold",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ServerOff, { className: "size-4 text-muted-foreground" }), translate("auto.components.sidebar.ForgetSshWorkspaceDialog.title", "Delete “{{name}}”?", { name: displayName })]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, {
						className: "text-xs",
						children: canReconnect ? translate("auto.components.sidebar.ForgetSshWorkspaceDialog.disconnectedBody", "The SSH host for this workspace is not connected. Reconnect to delete it on the remote too, or remove it from Orca only.") : translate("auto.components.sidebar.ForgetSshWorkspaceDialog.ghostBody", "{{host}} is no longer a saved SSH host, so this workspace is no longer connected to a live host. It can only be removed from Orca — files and branches on the remote are left untouched.", { host: hostLabel })
					})]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-center gap-2.5 rounded-md border border-border/50 bg-card/40 px-3 py-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Server, { className: "size-3.5 shrink-0 text-muted-foreground" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "min-w-0 flex-1 truncate text-xs font-medium",
						children: hostLabel
					})]
				}),
				canReconnect ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "text-[11px] leading-snug text-muted-foreground",
					children: forgetDescription
				}) : null,
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogFooter, {
					className: "gap-2 sm:gap-2",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							variant: "outline",
							size: "sm",
							onClick: () => closeModal(),
							disabled: busy != null,
							children: translate("auto.components.sidebar.ForgetSshWorkspaceDialog.cancel", "Cancel")
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
							variant: "outline",
							size: "sm",
							onClick: () => void handleForget(),
							disabled: busy != null,
							children: [busy === "forget" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3.5 animate-spin" }) : null, translate("auto.components.sidebar.ForgetSshWorkspaceDialog.forget", "Remove from Orca")]
						}),
						canReconnect ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
							size: "sm",
							onClick: () => void handleReconnectAndDelete(),
							disabled: busy != null,
							children: [busy === "reconnect" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3.5 animate-spin" }) : null, translate("auto.components.sidebar.ForgetSshWorkspaceDialog.reconnectAndDelete", "Reconnect & Delete")]
						}) : null
					]
				})
			]
		})
	});
}
var ForgetSshWorkspaceDialog_default = ForgetSshWorkspaceDialog;
export { ForgetSshWorkspaceDialog, ForgetSshWorkspaceDialog_default as default };
