import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as Button } from "./button-DszXJEV6.js";
import "./workspace-status-wl52y3xd.js";
import { t as FolderPlus } from "./folder-plus-09lX5Kg7.js";
import "./worktree-activation-BDsaiyMf.js";
import { t as LoaderCircle } from "./loader-circle-CRZpWdsi.js";
import { qg as isGitRepoKind, t as useAppStore } from "./store-CgXrfmaH.js";
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
import { a as DialogFooter, i as DialogDescription, o as DialogHeader, r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
import { t as finishProjectAddWithDefaultCheckout } from "./project-added-default-checkout-D1bj4zkk.js";
import { n as upsertAddedRepoWithProjectHostSetup, t as worktreeRefreshOptions } from "./add-repo-runtime-owner-CMgoZO3u.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var NON_GIT_REPO_ERROR = "Not a valid git repository";
var AddProjectFromFolderDialog_default = import_react.memo(function AddProjectFromFolderDialog$1() {
	const activeModal = useAppStore((s) => s.activeModal);
	const modalData = useAppStore((s) => s.modalData);
	const closeModal = useAppStore((s) => s.closeModal);
	const openModal = useAppStore((s) => s.openModal);
	const addRepoPath = useAppStore((s) => s.addRepoPath);
	const fetchWorktrees = useAppStore((s) => s.fetchWorktrees);
	const setHideDefaultBranchWorkspace = useAppStore((s) => s.setHideDefaultBranchWorkspace);
	const [isAdding, setIsAdding] = (0, import_react.useState)(false);
	const [error, setError] = (0, import_react.useState)(null);
	const mountedRef = useMountedRef();
	const addGenRef = (0, import_react.useRef)(0);
	const isOpen = activeModal === "confirm-add-project-from-folder";
	const [previousOpen, setPreviousOpen] = (0, import_react.useState)(isOpen);
	const folderPath = typeof modalData.folderPath === "string" ? modalData.folderPath : "";
	const connectionId = typeof modalData.connectionId === "string" ? modalData.connectionId : "";
	const runtimeEnvironmentId = typeof modalData.runtimeEnvironmentId === "string" ? modalData.runtimeEnvironmentId : null;
	if (isOpen !== previousOpen) {
		setPreviousOpen(isOpen);
		if (!isOpen) {
			addGenRef.current++;
			setIsAdding(false);
			setError(null);
		}
	}
	const openNonGitConfirmation = (0, import_react.useCallback)(() => {
		closeModal();
		openModal("confirm-non-git-folder", {
			folderPath,
			...connectionId ? { connectionId } : {},
			...runtimeEnvironmentId ? { runtimeEnvironmentId } : {}
		});
	}, [
		closeModal,
		connectionId,
		folderPath,
		openModal,
		runtimeEnvironmentId
	]);
	const handleConfirm = (0, import_react.useCallback)(async () => {
		if (!folderPath || isAdding) return;
		const gen = ++addGenRef.current;
		setIsAdding(true);
		setError(null);
		try {
			let repo;
			if (connectionId) {
				const result = await window.api.repos.addRemote({
					connectionId,
					remotePath: folderPath
				});
				if ("error" in result) throw new Error(result.error);
				const upserted = upsertAddedRepoWithProjectHostSetup(result.repo, { sshConnectionId: connectionId });
				repo = upserted.repo;
				if (upserted.alreadyPresent) useAppStore.getState().clearOrcaHookTrustForRepo(repo.id);
				if (!mountedRef.current || gen !== addGenRef.current) return;
				toast.success(translate("auto.components.sidebar.AddProjectFromFolderDialog.e643b30398", "Project added on SSH host"), { description: repo.displayName });
			} else repo = await addRepoPath(folderPath, "git", { runtimeEnvironmentId });
			if (!mountedRef.current || gen !== addGenRef.current) return;
			if (!repo) return;
			if (!isGitRepoKind(repo)) {
				openNonGitConfirmation();
				return;
			}
			const ownerOptions = worktreeRefreshOptions(runtimeEnvironmentId, connectionId);
			await fetchWorktrees(repo.id, ownerOptions);
			if (!mountedRef.current || gen !== addGenRef.current) return;
			await finishProjectAddWithDefaultCheckout({
				repoId: repo.id,
				source: connectionId ? "ssh_remote_path" : runtimeEnvironmentId ? "runtime_server_path" : "local_folder_picker",
				selectedPath: folderPath,
				executionHostId: ownerOptions.executionHostId,
				closeModal,
				setHideDefaultBranchWorkspace
			});
		} catch (err) {
			const message = err instanceof Error ? err.message : String(err);
			if (message.includes(NON_GIT_REPO_ERROR)) {
				if (mountedRef.current && gen === addGenRef.current) openNonGitConfirmation();
				return;
			}
			if (mountedRef.current && gen === addGenRef.current) setError(message);
		} finally {
			if (mountedRef.current && gen === addGenRef.current) setIsAdding(false);
		}
	}, [
		addRepoPath,
		closeModal,
		connectionId,
		fetchWorktrees,
		folderPath,
		isAdding,
		mountedRef,
		openNonGitConfirmation,
		runtimeEnvironmentId,
		setHideDefaultBranchWorkspace
	]);
	const handleOpenChange = (0, import_react.useCallback)((open) => {
		if (!open) {
			addGenRef.current++;
			closeModal();
		}
	}, [closeModal]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
		open: isOpen,
		onOpenChange: handleOpenChange,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogContent, {
			className: "sm:max-w-lg",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, { children: translate("auto.components.sidebar.AddProjectFromFolderDialog.7d1f51678c", "Add Project") }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, { children: translate("auto.components.sidebar.AddProjectFromFolderDialog.046751dbfb", "Add this folder as a separate Orca project.") })] }),
				folderPath && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "rounded-md border border-border/70 bg-muted/35 px-3 py-2 text-xs",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "break-all font-mono text-muted-foreground",
						children: folderPath
					})
				}),
				error && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "text-xs text-destructive",
					children: error
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogFooter, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					variant: "outline",
					onClick: () => handleOpenChange(false),
					disabled: isAdding,
					children: translate("auto.components.sidebar.AddProjectFromFolderDialog.7726a16374", "Cancel")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
					onClick: handleConfirm,
					disabled: !folderPath || isAdding,
					children: [isAdding ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-4 animate-spin" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(FolderPlus, { className: "size-4" }), translate("auto.components.sidebar.AddProjectFromFolderDialog.7d1f51678c", "Add Project")]
				})] })
			]
		})
	});
});
export { AddProjectFromFolderDialog_default as default };
