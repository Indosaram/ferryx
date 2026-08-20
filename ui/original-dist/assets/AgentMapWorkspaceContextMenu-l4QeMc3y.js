const __vite__mapDeps=(i,m=__vite__mapDeps,d=(m.f||(m.f=["./WorktreeContextMenu-BlxsA9vu.js","./button-DszXJEV6.js","./jsx-runtime-Cv_nyRjc.js","./preload-helper-Cgw39-ka.js","./chunk-Dhmk_5SA.js","./react-Da2TLWQy.js","./dropdown-menu-Dth6LPK-.js","./dist-CUdeCwrc.js","./dist-Ca8cIakR.js","./classPrivateFieldGet2-CvaeS1Sp.js","./dist-BsNIAh1s.js","./floating-ui.dom-i2UEqmZo.js","./dist-BvH-oDES.js","./dist-DGfr86jh.js","./dist-DW1EJH6e.js","./react-dom-Da8MQai-.js","./dist-B1f0G6s_.js","./dist-G_cmV6EA.js","./es2015-B5WZ-7WO.js","./check-Lb2n4tDb.js","./chevron-right-CZtMe6Ev.js","./circle-DumnR8X3.js","./label-D-n9s_wS.js","./popover-CgR1mzy7.js","./tooltip-DPmd1AoJ.js","./dist-DS2hPbHS.js","./command-D8Tw17HJ.js","./dist-CN60QqbN.js","./search-DK1nVA6d.js","./open-in-app-catalog-DpQuLNDD.js","./localized-catalog-DubKHKUR.js","./workspace-status-wl52y3xd.js","./circle-alert-keRTpMg-.js","./circle-dashed-Df2rqHLJ.js","./store-CgXrfmaH.js","./defineProperty-BAtR-r70.js","./dist-DgqligFk.js","./plugin-manifest-Bs-50M_g.js","./useMountedRef-1omUd-IV.js","./agent-status-3vUKbY6l.js","./agent-kind-Dfx6MnkP.js","./telemetry-ZyUPyKMD.js","./WorktreeContextMenu-BmbFRbIZ.js","./esm-DQfOTgcy.js","./bell-D5DYvUTg.js","./circle-x-Cl9fp3Vy.js","./code-xml-Q_QLKUSg.js","./copy-jk2iqVkp.js","./folder-plus-09lX5Kg7.js","./worktree-activation-BDsaiyMf.js","./list-BWhHuDP9.js","./pin-K26SGNXp.js","./native-chat-session-option-cache-DGE3h47U.js","./agent-paste-draft-C2PA7vXu.js","./terminal-pty-input-transaction-2UskR-Bm.js","./agent-process-recognition-BB0O3DaN.js","./github-links-C1M8w9wX.js","./worktree-lineage-projection-CS7n_mKq.js","./web-session-tabs-sync-CYKZbAxS.js","./shallow-BpOhx1Gc.js","./web-runtime-session-CN2syA39.js","./window-visibility-interval-CtnbYoau.js","./web-agent-session-handoff-D4ZdXDx4.js","./pane-agent-owner-BPfoVAtS.js","./connection-context-BUPsamzR.js","./migration-unsupported-agent-entry-BJ_0rXR-.js","./windows-pty-compatibility-XujC9UTf.js","./worktree-git-identity-display-B29QxW_l.js","./selectors-XOBeaOSb.js","./git-branch-CnBuDEti.js","./moon-Cw6GyiDZ.js","./pencil-CLc9a5do.js","./pin-off-CP-untcQ.js","./server-DYdwnXME.js","./unlink-B9rPq8CF.js","./workflow-DL6naYZy.js","./RepoBadgeLabel-BMcVlWTu.js","./StatusIndicator-CJ9TRLK4.js","./message-circle-question-mark-CFrAq4X1.js","./AgentWorkingSpinner-BpnTWNKF.js","./worktree-status-DR0Zr8Ht.js","./worktree-title-derived-agent-rows-xbcpjeY8.js","./WorktreeCardHelpers-Detnezco.js","./WorktreeOpenInMenu-DH_mb2Hm.js","./external-link-BrcDtGAn.js","./folder-open-B2ZB-rfY.js","./delete-worktree-flow-RxB6NScm.js","./preserved-branch-batch-toast-DHxeGO1o.js","./sleep-worktree-flow-83wDDapJ.js","./worktree-card-status-inputs-DozvjAa5.js","./dialog-BbelfMSB.js","./x-BrGKE4uz.js","./input-DV5rpysh.js","./ime-composition-keyboard-event-HdRxQ6x2.js","./manual-terminal-worktree-parking-DpGjzl0b.js"])))=>i.map(i=>d[i]);
import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as lazyWithRetry } from "./lazy-with-retry-pSZJrSfN.js";
import "./button-DszXJEV6.js";
import { t as useAppStore, vd as parseWorkspaceKey } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import { $ as getRepoExecutionHostId, lt as toSshExecutionHostId, rt as normalizeExecutionHostId } from "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import { t as __vitePreload } from "./preload-helper-Cgw39-ka.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./useMountedRef-1omUd-IV.js";
import { t as useShallow } from "./shallow-BpOhx1Gc.js";
import { h as useWorktreeById } from "./selectors-XOBeaOSb.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var WorktreeContextMenu = lazyWithRetry(() => __vitePreload(() => import("./WorktreeContextMenu-BlxsA9vu.js"), __vite__mapDeps([0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47,48,49,50,51,52,53,54,55,56,57,58,59,60,61,62,63,64,65,66,67,68,69,70,71,72,73,74,75,76,77,78,79,80,81,82,83,84,85,86,87,88,89,90,91,92,93,94]), import.meta.url), { reloadKey: "agent-map-worktree-context-menu" });
function countWorkspaceOwners(worktreeId, state) {
	if (!worktreeId) return 0;
	const scope = parseWorkspaceKey(worktreeId);
	if (scope?.type === "folder") return new Set(state.folderWorkspaces.filter((workspace) => workspace.id === scope.folderWorkspaceId).map((workspace) => normalizeExecutionHostId(workspace.executionHostId) ?? (workspace.connectionId ? toSshExecutionHostId(workspace.connectionId) : "local"))).size;
	const repoOwnerIdsByRepoId = /* @__PURE__ */ new Map();
	for (const repo of state.repos) {
		const ownerId = getRepoExecutionHostId(repo);
		const owners = repoOwnerIdsByRepoId.get(repo.id);
		if (owners) owners.add(ownerId);
		else repoOwnerIdsByRepoId.set(repo.id, new Set([ownerId]));
	}
	const ownerIds = /* @__PURE__ */ new Set();
	const addOwner = (worktree) => {
		const directOwner = normalizeExecutionHostId(worktree.hostId);
		if (directOwner) {
			ownerIds.add(directOwner);
			return;
		}
		const repoOwnerIds = repoOwnerIdsByRepoId.get(worktree.repoId);
		if (!repoOwnerIds) {
			ownerIds.add("local");
			return;
		}
		for (const ownerId of repoOwnerIds) ownerIds.add(ownerId);
	};
	for (const worktrees of Object.values(state.worktreesByRepo)) for (const worktree of worktrees) if (worktree.id === worktreeId) addOwner(worktree);
	for (const result of Object.values(state.detectedWorktreesByRepo)) for (const worktree of result.worktrees) if (worktree.id === worktreeId) addOwner(worktree);
	return ownerIds.size;
}
function ContextMenuTrigger({ request }) {
	const triggerRef = (0, import_react.useRef)(null);
	(0, import_react.useEffect)(() => {
		triggerRef.current?.dispatchEvent(new MouseEvent("contextmenu", {
			bubbles: true,
			cancelable: true,
			clientX: request.clientX,
			clientY: request.clientY,
			altKey: request.altKey,
			button: 2
		}));
	}, [request]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		ref: triggerRef,
		"aria-hidden": true
	});
}
function AgentMapWorkspaceContextMenu({ request, onOpenChange, onLifecycleComplete }) {
	const { worktreesByRepo, detectedWorktreesByRepo, folderWorkspaces, repos } = useAppStore(useShallow((state) => ({
		worktreesByRepo: state.worktreesByRepo,
		detectedWorktreesByRepo: state.detectedWorktreesByRepo,
		folderWorkspaces: state.folderWorkspaces,
		repos: state.repos
	})));
	const worktree = useWorktreeById(request?.worktreeId ?? null, request?.executionHostId);
	const ownerCount = (0, import_react.useMemo)(() => countWorkspaceOwners(request?.worktreeId ?? null, {
		worktreesByRepo,
		detectedWorktreesByRepo,
		folderWorkspaces,
		repos
	}), [
		detectedWorktreesByRepo,
		folderWorkspaces,
		repos,
		request?.worktreeId,
		worktreesByRepo
	]);
	const unavailable = request !== null && (!worktree || ownerCount !== 1);
	(0, import_react.useEffect)(() => {
		if (unavailable) onLifecycleComplete?.();
	}, [onLifecycleComplete, unavailable]);
	if (!request || unavailable || !worktree) return null;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "pointer-events-none absolute inset-0",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(import_react.Suspense, {
			fallback: null,
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorktreeContextMenu, {
				worktree,
				onOpenChange,
				onLifecycleComplete,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ContextMenuTrigger, { request })
			})
		})
	});
}
export { AgentMapWorkspaceContextMenu };
