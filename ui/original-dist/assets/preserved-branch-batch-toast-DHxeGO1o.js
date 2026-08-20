import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as Button } from "./button-DszXJEV6.js";
import { Cp as getRepoIdFromWorktreeId, m_ as Trash2, t as useAppStore, w as mapWithConcurrency } from "./store-CgXrfmaH.js";
import { n as toast } from "./dist-DgqligFk.js";
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var BRANCH_REPO_DELETE_CONCURRENCY = 4;
function PreservedBranchBatchToastBody({ branches, onReview }) {
	const actionableCount = branches.filter((branch) => branch.expectedHead).length;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex w-[300px] max-w-[calc(100vw-96px)] flex-col gap-3",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
			className: "min-w-0 break-words text-sm leading-5 text-popover-foreground/80",
			children: translate("auto.components.sidebar.preserved.branch.batch.toast.a3cdd9d9e6", "Git kept {{count}} local branches because they may contain unmerged commits. Kept branches do not retain workspace folders; their commits remain in the repository. Orca may continue freeing workspace disk space in the background.", { count: branches.length })
		}), actionableCount > 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
			type: "button",
			variant: "destructive",
			size: "sm",
			className: "w-full",
			onClick: onReview,
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, { className: "size-3.5" }), translate("auto.components.sidebar.preserved.branch.batch.toast.6310412304", "Review {{count}} Branches", { count: actionableCount })]
		}) : null]
	});
}
function showPreservedBranchBatchToast(workspaceCount, branches) {
	if (branches.length === 0) return;
	const actionableCount = branches.filter((branch) => branch.expectedHead).length;
	const toastId = `preserved-branch-batch:${branches[0].worktreeId}:${branches.length}`;
	const removedWorkspaces = translate("auto.components.sidebar.preserved.branch.batch.toast.cea24c2b7d", "{{count}} workspaces removed", { count: workspaceCount });
	const keptBranches = translate("auto.components.sidebar.preserved.branch.batch.toast.0e0379f24a", "{{count}} branches kept", { count: branches.length });
	const onReview = () => {
		useAppStore.getState().openModal("preserved-branch-review", { branches });
		toast.dismiss(toastId);
	};
	toast.warning(translate("auto.components.sidebar.preserved.branch.batch.toast.4cf75caab7", "{{value0}}, {{value1}}", {
		value0: removedWorkspaces,
		value1: keptBranches
	}), {
		id: toastId,
		description: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PreservedBranchBatchToastBody, {
			branches,
			onReview
		}),
		dismissible: true,
		...actionableCount > 0 ? { duration: Infinity } : {}
	});
}
async function forceDeletePreservedBranchBatch(branches) {
	if (branches.length === 0) return;
	const progressToastId = `force-delete-branch-batch:${branches[0].worktreeId}:${branches.length}`;
	toast.loading(translate("auto.components.sidebar.preserved.branch.batch.toast.e61d78054f", "Deleting local branches: {{value0}}", { value0: branches.length }), { id: progressToastId });
	const branchesByRepo = /* @__PURE__ */ new Map();
	for (const branch of branches) {
		const repoId = getRepoIdFromWorktreeId(branch.worktreeId);
		const repoBranches = branchesByRepo.get(repoId);
		if (repoBranches) repoBranches.push(branch);
		else branchesByRepo.set(repoId, [branch]);
	}
	const failures = (await mapWithConcurrency([...branchesByRepo.values()], BRANCH_REPO_DELETE_CONCURRENCY, async (repoBranches) => {
		const results = [];
		for (const branch of repoBranches) results.push({
			branch,
			result: await useAppStore.getState().forceDeletePreservedBranch(branch.worktreeId, branch.branchName, branch.expectedHead, {
				suppressToast: true,
				...branch.hostId ? { hostId: branch.hostId } : {},
				...branch.runtimeEnvironmentId ? { runtimeEnvironmentId: branch.runtimeEnvironmentId } : {}
			})
		});
		return results;
	})).flat().filter((result) => !result.result.ok);
	if (failures.length === 0) {
		toast.success(translate("auto.components.sidebar.preserved.branch.batch.toast.1e1a5f6763", "Local branches deleted: {{value0}}", { value0: branches.length }), { id: progressToastId });
		return;
	}
	const deletedCount = branches.length - failures.length;
	const description = failures.map(({ branch, result }) => result.ok ? "" : `${branch.branchName}: ${result.error}`).filter(Boolean).join("; ");
	const failedBranches = failures.map(({ branch }) => branch);
	toast.error(translate("auto.components.sidebar.preserved.branch.batch.toast.43d9395605", "{{value0}} deleted, {{value1}} not deleted", {
		value0: deletedCount,
		value1: failures.length
	}), {
		id: progressToastId,
		description: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex w-[300px] max-w-[calc(100vw-96px)] flex-col gap-3",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				className: "min-w-0 break-words text-sm leading-5 text-popover-foreground/80",
				children: description
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
				type: "button",
				variant: "destructive",
				size: "sm",
				className: "w-full",
				onClick: () => {
					forceDeletePreservedBranchBatch(failedBranches);
				},
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, { className: "size-3.5" }), translate("auto.components.sidebar.preserved.branch.batch.toast.d42f1f14e0", "Retry {{count}} Branches", { count: failedBranches.length })]
			})]
		}),
		duration: Infinity,
		dismissible: true
	});
}
export { showPreservedBranchBatchToast as n, forceDeletePreservedBranchBatch as t };
