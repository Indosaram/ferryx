import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as Button } from "./button-DszXJEV6.js";
import { t as useAppStore } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import "./useMountedRef-1omUd-IV.js";
import { a as DialogFooter, i as DialogDescription, o as DialogHeader, r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var NAME_TOKEN = "\0";
var RemoveFolderDialog_default = import_react.memo(function RemoveFolderDialog$1() {
	const activeModal = useAppStore((s) => s.activeModal);
	const modalData = useAppStore((s) => s.modalData);
	const closeModal = useAppStore((s) => s.closeModal);
	const removeProject = useAppStore((s) => s.removeProject);
	const isOpen = activeModal === "confirm-remove-folder";
	const repoId = typeof modalData.repoId === "string" ? modalData.repoId : "";
	const displayName = typeof modalData.displayName === "string" ? modalData.displayName : "";
	const sshHostLabel = useAppStore((s) => {
		const connectionId = s.repos.find((r) => r.id === repoId)?.connectionId?.trim();
		if (!connectionId) return null;
		return s.sshTargetLabels.get(connectionId) ?? s.removedSshTargetLabels.get(connectionId) ?? connectionId;
	});
	const [descriptionBeforeName, descriptionAfterName] = (sshHostLabel ? translate("auto.components.sidebar.RemoveFolderDialog.removeDescriptionSsh", "This only removes {{name}} from Orca. Its files stay on {{host}} — re-add that SSH host to recover it.", {
		name: NAME_TOKEN,
		host: sshHostLabel
	}) : translate("auto.components.sidebar.RemoveFolderDialog.removeDescriptionLocal", "This only removes {{name}} from Orca. It is still on your disk.", { name: NAME_TOKEN })).split(NAME_TOKEN);
	const handleConfirm = (0, import_react.useCallback)(() => {
		if (repoId) removeProject(repoId, { errorFeedback: "toast" });
		closeModal();
	}, [
		closeModal,
		removeProject,
		repoId
	]);
	const handleOpenChange = (0, import_react.useCallback)((open) => {
		if (!open) closeModal();
	}, [closeModal]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
		open: isOpen,
		onOpenChange: handleOpenChange,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogContent, {
			className: "max-w-sm sm:max-w-sm",
			showCloseButton: false,
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, {
				className: "text-sm",
				children: translate("auto.components.sidebar.RemoveFolderDialog.b79b39d865", "Remove Project")
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogDescription, {
				className: "text-xs",
				children: [
					descriptionBeforeName,
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "break-all font-medium text-foreground",
						children: displayName
					}),
					descriptionAfterName
				]
			})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogFooter, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				variant: "outline",
				onClick: () => handleOpenChange(false),
				children: translate("auto.components.sidebar.RemoveFolderDialog.d36883e046", "Cancel")
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				variant: "destructive",
				onClick: handleConfirm,
				children: translate("auto.components.sidebar.RemoveFolderDialog.4dc5b5065b", "Remove")
			})] })]
		})
	});
});
export { RemoveFolderDialog_default as default };
