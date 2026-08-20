import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import "./jsx-runtime-Cv_nyRjc.js";
import "./button-DszXJEV6.js";
import "./workspace-status-wl52y3xd.js";
import { r as activateAndRevealWorktree } from "./worktree-activation-BDsaiyMf.js";
import { Kg as isFolderRepo, t as useAppStore } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
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
import { t as finishProjectAddWithDefaultCheckout } from "./project-added-default-checkout-D1bj4zkk.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
function ProjectAddedDialog() {
	const activeModal = useAppStore((s) => s.activeModal);
	const modalData = useAppStore((s) => s.modalData);
	const closeModal = useAppStore((s) => s.closeModal);
	const repos = useAppStore((s) => s.repos);
	const fetchRepos = useAppStore((s) => s.fetchRepos);
	const fetchWorktrees = useAppStore((s) => s.fetchWorktrees);
	const setHideDefaultBranchWorkspace = useAppStore((s) => s.setHideDefaultBranchWorkspace);
	const handoffRunRef = (0, import_react.useRef)(0);
	const pendingRepoHydrationRef = (0, import_react.useRef)(null);
	const repoId = typeof modalData?.repoId === "string" ? modalData.repoId : typeof modalData?.projectId === "string" ? modalData.projectId : "";
	const repo = repos.find((candidate) => candidate.id === repoId) ?? null;
	(0, import_react.useEffect)(() => {
		if (activeModal !== "project-added") {
			handoffRunRef.current++;
			pendingRepoHydrationRef.current = null;
			return;
		}
		if (!repoId) {
			closeModal();
			return;
		}
		if (!repo) {
			if (pendingRepoHydrationRef.current === repoId) return;
			pendingRepoHydrationRef.current = repoId;
			let cancelled$1 = false;
			(async () => {
				await fetchRepos();
				if (cancelled$1) return;
				if (!useAppStore.getState().repos.find((candidate) => candidate.id === repoId)) closeModal();
				pendingRepoHydrationRef.current = null;
			})();
			return () => {
				cancelled$1 = true;
				pendingRepoHydrationRef.current = null;
			};
		}
		pendingRepoHydrationRef.current = null;
		const runId = ++handoffRunRef.current;
		let cancelled = false;
		if (isFolderRepo(repo)) {
			(async () => {
				try {
					await fetchWorktrees(repoId);
				} catch {}
				if (cancelled) return;
				const folderWorktree = useAppStore.getState().worktreesByRepo[repoId]?.[0];
				if (folderWorktree) activateAndRevealWorktree(folderWorktree.id, { sidebarRevealBehavior: "auto" });
				closeModal();
			})();
			return () => {
				cancelled = true;
			};
		}
		(async () => {
			try {
				await fetchWorktrees(repoId);
			} catch {}
			if (!cancelled && handoffRunRef.current === runId) await finishProjectAddWithDefaultCheckout({
				repoId,
				source: "project_added_compat",
				closeModal,
				setHideDefaultBranchWorkspace
			});
		})();
		return () => {
			cancelled = true;
		};
	}, [
		activeModal,
		closeModal,
		fetchRepos,
		fetchWorktrees,
		repo,
		repoId,
		setHideDefaultBranchWorkspace
	]);
	return null;
}
export { ProjectAddedDialog as default };
