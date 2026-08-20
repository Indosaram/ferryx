import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as Button } from "./button-DszXJEV6.js";
import "./workspace-status-wl52y3xd.js";
import { r as activateAndRevealWorktree } from "./worktree-activation-BDsaiyMf.js";
import { Fd as isNativeChatTranscriptLocalReadable, Id as buildDismissedOnboardingFolderAgentStartup, Pd as markOnboardingProjectAdded, t as useAppStore } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import { n as toast } from "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import "./useMountedRef-1omUd-IV.js";
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
import { n as upsertAddedRepoWithProjectHostSetup, t as worktreeRefreshOptions } from "./add-repo-runtime-owner-CMgoZO3u.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var NonGitFolderDialog_default = import_react.memo(function NonGitFolderDialog$1() {
	const activeModal = useAppStore((s) => s.activeModal);
	const modalData = useAppStore((s) => s.modalData);
	const closeModal = useAppStore((s) => s.closeModal);
	const addNonGitFolder = useAppStore((s) => s.addNonGitFolder);
	const runtimeEnvironments = useAppStore((s) => s.runtimeEnvironments);
	const isOpen = activeModal === "confirm-non-git-folder";
	const folderPath = typeof modalData.folderPath === "string" ? modalData.folderPath : "";
	const connectionId = typeof modalData.connectionId === "string" ? modalData.connectionId : "";
	const runtimeEnvironmentId = typeof modalData.runtimeEnvironmentId === "string" ? modalData.runtimeEnvironmentId : "";
	const runtimeEnvironmentName = runtimeEnvironmentId && (runtimeEnvironments.find((environment) => environment.id === runtimeEnvironmentId)?.name || runtimeEnvironmentId);
	const checkedHostDescription = connectionId ? translate("auto.components.sidebar.NonGitFolderDialog.9a766f33ac", "This path was checked on the SSH host.") : runtimeEnvironmentName ? translate("auto.components.sidebar.NonGitFolderDialog.79fd02cf5f", "This path was checked on {{hostName}}.", { hostName: runtimeEnvironmentName }) : translate("auto.components.sidebar.NonGitFolderDialog.8851b77327", "This path was checked locally.");
	const handleConfirm = (0, import_react.useCallback)(() => {
		if (connectionId && folderPath) (async () => {
			try {
				const stateBeforeAdd = useAppStore.getState();
				const result = await window.api.repos.addRemote({
					connectionId,
					remotePath: folderPath,
					kind: "folder"
				});
				if ("error" in result) throw new Error(result.error);
				const { repo } = upsertAddedRepoWithProjectHostSetup(result.repo, { sshConnectionId: connectionId });
				const state = useAppStore.getState();
				const hadProjectBeforeAdd = stateBeforeAdd.repos.length > 0;
				await markOnboardingProjectAdded("addedFolder");
				const ownerOptions = worktreeRefreshOptions(void 0, connectionId);
				await state.fetchWorktrees(repo.id, ownerOptions);
				const folderWorktree = useAppStore.getState().worktreesByRepo[repo.id]?.find((worktree) => worktree.hostId === ownerOptions.executionHostId);
				if (folderWorktree) {
					const onboarding = await window.api.onboarding.get().catch(() => null);
					const startup = buildDismissedOnboardingFolderAgentStartup(useAppStore.getState().settings, onboarding, hadProjectBeforeAdd, isNativeChatTranscriptLocalReadable(connectionId));
					activateAndRevealWorktree(folderWorktree.id, {
						sidebarRevealBehavior: "auto",
						executionHostId: ownerOptions.executionHostId,
						...startup ? { startup } : {}
					});
				}
			} catch (err) {
				toast.error(err instanceof Error ? err.message : translate("auto.components.sidebar.NonGitFolderDialog.c49fb13492", "Failed to add folder on this host"));
			}
		})();
		else if (folderPath) addNonGitFolder(folderPath, { runtimeEnvironmentId: runtimeEnvironmentId || null });
		closeModal();
	}, [
		addNonGitFolder,
		closeModal,
		folderPath,
		connectionId,
		runtimeEnvironmentId
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
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, {
					className: "text-sm",
					children: translate("auto.components.sidebar.NonGitFolderDialog.e52454b7f6", "Open as Folder")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogDescription, {
					className: "text-xs",
					children: [translate("auto.components.sidebar.NonGitFolderDialog.8fba4b8cbb", "This folder isn't a Git repository. You'll have the editor, terminal, and search, but Git-based features won't be available."), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "mt-2 block",
						children: checkedHostDescription
					})]
				})] }),
				folderPath && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "rounded-md border border-border/70 bg-muted/35 px-3 py-2 text-xs",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "break-all text-muted-foreground",
						children: folderPath
					})
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogFooter, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					variant: "outline",
					onClick: () => handleOpenChange(false),
					children: translate("auto.components.sidebar.NonGitFolderDialog.05b33a17a9", "Cancel")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					onClick: handleConfirm,
					children: translate("auto.components.sidebar.NonGitFolderDialog.e52454b7f6", "Open as Folder")
				})] })
			]
		})
	});
});
export { NonGitFolderDialog_default as default };
