import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as Button } from "./button-DszXJEV6.js";
import { Cp as getRepoIdFromWorktreeId, m_ as Trash2, t as useAppStore, vc as preservedBranchCleanupKey } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import { t as Checkbox } from "./checkbox-PAbetBh2.js";
import { t as Label } from "./label-D-n9s_wS.js";
import "./useMountedRef-1omUd-IV.js";
import { t as forceDeletePreservedBranchBatch } from "./preserved-branch-batch-toast-DHxeGO1o.js";
import { a as DialogFooter, i as DialogDescription, o as DialogHeader, r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function selectionKey(branch) {
	return preservedBranchCleanupKey(branch);
}
function getRepositoryLabel(branch) {
	const repoId = getRepoIdFromWorktreeId(branch.worktreeId);
	return useAppStore.getState().repos?.find((repo) => repo.id === repoId)?.displayName || repoId;
}
function PreservedBranchBatchReviewDialog({ branches, open, onOpenChange, onForceDelete }) {
	const actionableBranches = (0, import_react.useMemo)(() => branches.filter((branch) => Boolean(branch.expectedHead)), [branches]);
	const actionableKeys = (0, import_react.useMemo)(() => actionableBranches.map((branch) => selectionKey(branch)), [actionableBranches]);
	const [selectedKeys, setSelectedKeys] = (0, import_react.useState)(() => new Set(actionableKeys));
	const selectedBranches = actionableBranches.filter((branch) => selectedKeys.has(selectionKey(branch)));
	const allSelected = selectedBranches.length === actionableBranches.length;
	const someSelected = selectedBranches.length > 0 && !allSelected;
	const setBranchSelected = (branch, selected) => {
		setSelectedKeys((current) => {
			const next = new Set(current);
			if (selected) next.add(selectionKey(branch));
			else next.delete(selectionKey(branch));
			return next;
		});
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
		open,
		onOpenChange,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogContent, {
			className: "sm:max-w-lg",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, {
					className: "text-sm",
					children: translate("auto.components.sidebar.PreservedBranchBatchReviewDialog.c4bf8e7eaf", "Review kept branches")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, { children: translate("auto.components.sidebar.PreservedBranchBatchReviewDialog.f21976c9a8", "Select the local branches you want to force delete. Unselected branches stay in their repositories.") })] }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "max-h-72 overflow-y-auto scrollbar-sleek rounded-md border border-border/70 bg-muted/35 text-xs",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "sticky top-0 z-10 flex min-h-9 items-center justify-between gap-3 border-b border-border/70 bg-background/95 px-3 backdrop-blur-sm",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Label, {
							htmlFor: "preserved-branch-select-all",
							className: "gap-2 text-xs",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Checkbox, {
								id: "preserved-branch-select-all",
								checked: someSelected ? "indeterminate" : allSelected,
								onCheckedChange: (checked) => setSelectedKeys(checked === true ? new Set(actionableKeys) : /* @__PURE__ */ new Set())
							}), translate("auto.components.sidebar.PreservedBranchBatchReviewDialog.38c947f7c5", "Select all")]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-[11px] tabular-nums text-muted-foreground",
							children: translate("auto.components.sidebar.PreservedBranchBatchReviewDialog.9602129d38", "{{value0}} of {{value1}} selected", {
								value0: selectedBranches.length,
								value1: actionableBranches.length
							})
						})]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("ul", {
						className: "px-3",
						children: branches.map((branch, index) => {
							const actionableBranch = branch.expectedHead ? branch : null;
							const branchKey = selectionKey(branch);
							const checkboxId = `preserved-branch-${index}`;
							return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("li", {
								className: "grid min-h-11 grid-cols-[1rem_minmax(0,1fr)_auto] items-center gap-2.5 border-b border-border/50 py-1.5 last:border-0",
								children: [
									/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Checkbox, {
										id: checkboxId,
										checked: actionableBranch ? selectedKeys.has(branchKey) : false,
										disabled: !actionableBranch,
										onCheckedChange: (checked) => {
											if (actionableBranch) setBranchSelected(actionableBranch, checked === true);
										}
									}),
									/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Label, {
										htmlFor: checkboxId,
										className: "block min-w-0 cursor-pointer leading-snug",
										children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
											className: "block break-all font-mono font-medium text-foreground",
											children: branch.branchName
										}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
											className: "mt-0.5 block break-all text-[11px] text-muted-foreground",
											children: [getRepositoryLabel(branch), branch.expectedHead ? ` · ${branch.expectedHead.slice(0, 7)}` : ""]
										})]
									}),
									/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
										className: "text-[11px] whitespace-nowrap text-muted-foreground",
										children: actionableBranch ? translate("auto.components.sidebar.PreservedBranchBatchReviewDialog.ee39e872d5", "May be unmerged") : translate("auto.components.sidebar.PreservedBranchBatchReviewDialog.676db406fd", "Head unavailable")
									})
								]
							}, branchKey);
						})
					})]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogFooter, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					type: "button",
					variant: "outline",
					onClick: () => onOpenChange(false),
					children: translate("auto.components.sidebar.PreservedBranchBatchReviewDialog.285e1e4882", "Cancel")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
					type: "button",
					variant: "destructive",
					disabled: selectedBranches.length === 0,
					onClick: () => onForceDelete(selectedBranches),
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, {}), translate("auto.components.sidebar.PreservedBranchBatchReviewDialog.a0f9863597", "Force Delete {{count}} Branches", { count: selectedBranches.length })]
				})] })
			]
		})
	});
}
function isPreservedBranchCleanup(value) {
	if (!value || typeof value !== "object") return false;
	const branch = value;
	return typeof branch.worktreeId === "string" && typeof branch.branchName === "string" && (branch.expectedHead === void 0 || typeof branch.expectedHead === "string") && (branch.hostId === void 0 || typeof branch.hostId === "string") && (branch.runtimeEnvironmentId === void 0 || typeof branch.runtimeEnvironmentId === "string");
}
function getModalBranches(value) {
	return Array.isArray(value) ? value.filter(isPreservedBranchCleanup) : [];
}
function PreservedBranchBatchReviewModal() {
	const activeModal = useAppStore((state) => state.activeModal);
	const modalData = useAppStore((state) => state.modalData);
	const closeModal = useAppStore((state) => state.closeModal);
	const branches = (0, import_react.useMemo)(() => getModalBranches(modalData.branches), [modalData.branches]);
	const open = activeModal === "preserved-branch-review" && branches.length > 0;
	const handleForceDelete = (selectedBranches) => {
		closeModal();
		forceDeletePreservedBranchBatch(selectedBranches);
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PreservedBranchBatchReviewDialog, {
		branches,
		open,
		onOpenChange: (nextOpen) => {
			if (!nextOpen) closeModal();
		},
		onForceDelete: handleForceDelete
	});
}
export { PreservedBranchBatchReviewModal as default };
