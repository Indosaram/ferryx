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
var SCRIPT_KIND_LABEL = {
	setup: "setup script",
	archive: "archive script",
	issueCommand: "issue command",
	vmRecipe: "VM recipe"
};
var SCRIPT_KIND_TRIGGER = {
	setup: "when this workspace is created",
	archive: "when this workspace is removed",
	issueCommand: "when this workspace launches with a linked issue",
	vmRecipe: "before provisioning a VM"
};
var OrcaYamlTrustDialog_default = import_react.memo(function OrcaYamlTrustDialog$1() {
	const activeModal = useAppStore((s) => s.activeModal);
	const modalData = useAppStore((s) => s.modalData);
	const closeModal = useAppStore((s) => s.closeModal);
	const markOrcaHookScriptConfirmed = useAppStore((s) => s.markOrcaHookScriptConfirmed);
	const markOrcaHookRepoAlwaysTrusted = useAppStore((s) => s.markOrcaHookRepoAlwaysTrusted);
	const isOpen = activeModal === "confirm-orca-yaml-hooks";
	const [alwaysTrustState, setAlwaysTrustState] = (0, import_react.useState)(() => ({
		isOpen,
		value: false
	}));
	if (alwaysTrustState.isOpen !== isOpen) setAlwaysTrustState({
		isOpen,
		value: false
	});
	const alwaysTrust = alwaysTrustState.isOpen === isOpen ? alwaysTrustState.value : false;
	const setAlwaysTrust = (value) => {
		setAlwaysTrustState({
			isOpen,
			value
		});
	};
	const repoId = typeof modalData.repoId === "string" ? modalData.repoId : "";
	const repoName = typeof modalData.repoName === "string" ? modalData.repoName : "this repository";
	const scriptKind = modalData.scriptKind === "archive" ? "archive" : modalData.scriptKind === "issueCommand" ? "issueCommand" : modalData.scriptKind === "vmRecipe" ? "vmRecipe" : "setup";
	const scriptContent = typeof modalData.scriptContent === "string" ? modalData.scriptContent : "";
	const contentHash = typeof modalData.contentHash === "string" ? modalData.contentHash : "";
	const previouslyApproved = modalData.previouslyApproved === true;
	const onResolve = typeof modalData.onResolve === "function" ? modalData.onResolve : null;
	const resolveAndClose = (0, import_react.useCallback)((decision) => {
		if (decision === "run" && repoId) {
			if (alwaysTrust) markOrcaHookRepoAlwaysTrusted(repoId);
			else if (contentHash) markOrcaHookScriptConfirmed(repoId, scriptKind, contentHash);
		}
		onResolve?.(decision);
		closeModal();
	}, [
		alwaysTrust,
		closeModal,
		contentHash,
		markOrcaHookRepoAlwaysTrusted,
		markOrcaHookScriptConfirmed,
		onResolve,
		repoId,
		scriptKind
	]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
		open: isOpen,
		onOpenChange: (0, import_react.useCallback)((open) => {
			if (!open) resolveAndClose("skip");
		}, [resolveAndClose]),
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogContent, {
			className: "max-w-md sm:max-w-md",
			showCloseButton: false,
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, {
					className: "text-sm",
					children: previouslyApproved ? translate("auto.components.sidebar.OrcaYamlTrustDialog.02b0ede5ad", "{{value0}}'s {{value1}} changed — run the new version?", {
						value0: repoName,
						value1: SCRIPT_KIND_LABEL[scriptKind]
					}) : translate("auto.components.sidebar.OrcaYamlTrustDialog.e4a51dc4b3", "Run {{value0}} from {{value1}}?", {
						value0: SCRIPT_KIND_LABEL[scriptKind],
						value1: repoName
					})
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, {
					className: "text-xs",
					children: previouslyApproved ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("code", { children: translate("auto.components.sidebar.OrcaYamlTrustDialog.79afc6772b", "orca.yaml") }),
						" ",
						translate("auto.components.sidebar.OrcaYamlTrustDialog.c55beddbf8", "changed since you last approved. Re-review before it runs"),
						" ",
						SCRIPT_KIND_TRIGGER[scriptKind],
						"."
					] }) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
						translate("auto.components.sidebar.OrcaYamlTrustDialog.aa3ffb33fb", "This repository's"),
						" ",
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("code", { children: translate("auto.components.sidebar.OrcaYamlTrustDialog.79afc6772b", "orca.yaml") }),
						" ",
						translate("auto.components.sidebar.OrcaYamlTrustDialog.831f2cd9f0", "runs on your machine"),
						" ",
						SCRIPT_KIND_TRIGGER[scriptKind],
						translate("auto.components.sidebar.OrcaYamlTrustDialog.bf800b7e04", ". Only run if you trust"),
						" ",
						repoName,
						"."
					] })
				})] }),
				scriptContent && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "rounded-md border border-border/70 bg-muted/35 px-3 py-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "mb-1 text-[11px] font-medium uppercase tracking-wide text-muted-foreground",
						children: previouslyApproved ? translate("auto.components.sidebar.OrcaYamlTrustDialog.9e52effffd", "New {{value0}} script", { value0: scriptKind }) : translate("auto.components.sidebar.OrcaYamlTrustDialog.95bf974a1a", "{{value0}} script", { value0: scriptKind })
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("pre", {
						className: "max-h-48 overflow-auto whitespace-pre-wrap break-all font-mono text-xs text-foreground scrollbar-sleek",
						children: scriptContent
					})]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("label", {
					className: `flex cursor-pointer items-center gap-2.5 rounded-md border px-3 py-2 transition-colors ${alwaysTrust ? "border-primary/60 bg-primary/5" : "border-border/70 bg-muted/25 hover:border-border hover:bg-muted/40"}`,
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("input", {
						type: "checkbox",
						className: "h-4 w-4 accent-primary",
						checked: alwaysTrust,
						onChange: (event) => setAlwaysTrust(event.target.checked)
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
						className: "text-xs font-medium text-foreground",
						children: [
							translate("auto.components.sidebar.OrcaYamlTrustDialog.531689199b", "Always trust"),
							" ",
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("code", { children: translate("auto.components.sidebar.OrcaYamlTrustDialog.79afc6772b", "orca.yaml") }),
							" ",
							translate("auto.components.sidebar.OrcaYamlTrustDialog.c494b3ccb1", "in"),
							" ",
							repoName
						]
					})]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogFooter, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					variant: "outline",
					onClick: () => resolveAndClose("skip"),
					children: translate("auto.components.sidebar.OrcaYamlTrustDialog.43b7bec4cd", "Don't run")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					onClick: () => resolveAndClose("run"),
					children: translate("auto.components.sidebar.OrcaYamlTrustDialog.f3e2b868fb", "Run hooks")
				})] })
			]
		})
	});
});
export { OrcaYamlTrustDialog_default as default };
