import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import "./button-DszXJEV6.js";
import "./workspace-status-wl52y3xd.js";
import "./worktree-activation-BDsaiyMf.js";
import { t as GitBranch } from "./git-branch-CnBuDEti.js";
import { t as LoaderCircle } from "./loader-circle-CRZpWdsi.js";
import { t as RotateCcw } from "./rotate-ccw-hK0RKgaG.js";
import { t as useAppStore } from "./store-CgXrfmaH.js";
import { t as TriangleAlert } from "./triangle-alert-HrLt1y9s.js";
import { t as X } from "./x-BrGKE4uz.js";
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
import { t as installWindowVisibilityInterval } from "./window-visibility-interval-CtnbYoau.js";
import "./native-chat-session-option-cache-DGE3h47U.js";
import "./github-links-C1M8w9wX.js";
import "./connection-context-BUPsamzR.js";
import "./localized-catalog-DubKHKUR.js";
import { o as getCreationProgressLabel, t as retryBackgroundWorktreeCreation } from "./worktree-creation-flow-B-X9YpIm.js";
import "./ssh-types-Caw2Ltsn.js";
import "./workspace-activation-terminal-focus-CpPnzh-J.js";
import "./ephemeral-vm-recipes-D4s3J2cQ.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function WorktreeCreationPanel({ creationId, reserveCollapsedSidebarHeaderSpace = false }) {
	const entry = useAppStore((s) => s.pendingWorktreeCreations[creationId]);
	const [now, setNow] = import_react.useState(() => Date.now());
	const entryStatus = entry?.status;
	import_react.useEffect(() => {
		if (entryStatus !== "creating") return;
		return installWindowVisibilityInterval({
			run: () => setNow(Date.now()),
			intervalMs: 1e3
		});
	}, [entryStatus]);
	if (!entry) return null;
	const dismiss = () => useAppStore.getState().removePendingWorktreeCreation(creationId);
	const isError = entry.status === "error";
	const isVmCreation = entry.phase === "provisioning-vm";
	const title = entry.request.displayName || entry.request.name;
	const elapsedLabel = formatElapsedTime(now - entry.startedAt);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "absolute inset-0 flex flex-col bg-background",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex h-[36px] shrink-0 items-stretch border-b border-border bg-card",
			children: [reserveCollapsedSidebarHeaderSpace ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "shrink-0",
				style: {
					width: "var(--collapsed-sidebar-header-width)",
					WebkitAppRegion: "no-drag"
				}
			}) : null, /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex h-full max-w-[240px] items-center gap-1.5 border-r border-border px-2.5 text-xs",
				children: [
					isError ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "size-3.5 shrink-0 text-destructive" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(GitBranch, { className: "size-3.5 shrink-0 text-muted-foreground" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "truncate font-medium text-foreground",
						children: title
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
						type: "button",
						title: translate("auto.components.worktree.creation.WorktreeCreationPanel.532aea14ce", "Cancel"),
						"aria-label": translate("auto.components.worktree.creation.WorktreeCreationPanel.a3346fc6ed", "Cancel worktree creation"),
						onClick: dismiss,
						className: "flex size-4 shrink-0 items-center justify-center rounded-sm text-muted-foreground hover:bg-muted hover:text-foreground",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "size-3" })
					})
				]
			})]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "min-h-0 flex-1 p-3",
			children: isVmCreation ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(VmProvisioningStatus, {
				elapsedLabel,
				log: entry.provisioningLog ?? "",
				error: isError ? entry.error ?? translate("auto.components.worktree.creation.WorktreeCreationPanel.767951265d", "Something went wrong while creating the worktree.") : null,
				onCancel: dismiss,
				onRetry: () => retryBackgroundWorktreeCreation(creationId),
				onDismiss: dismiss
			}) : isError ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex flex-wrap items-center gap-x-3 gap-y-1 text-xs",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "font-medium text-destructive",
						children: translate("auto.components.worktree.creation.WorktreeCreationPanel.ed2a664f8b", "Couldn’t create worktree")
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "text-muted-foreground",
						children: entry.error ?? translate("auto.components.worktree.creation.WorktreeCreationPanel.767951265d", "Something went wrong while creating the worktree.")
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
						type: "button",
						onClick: () => retryBackgroundWorktreeCreation(creationId),
						className: "inline-flex items-center gap-1 text-foreground hover:underline",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RotateCcw, { className: "size-3" }), translate("auto.components.worktree.creation.WorktreeCreationPanel.34dd5ee38b", "Retry")]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
						type: "button",
						onClick: dismiss,
						className: "text-muted-foreground hover:text-foreground hover:underline",
						children: translate("auto.components.worktree.creation.WorktreeCreationPanel.dabd226118", "Dismiss")
					})
				]
			}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "flex min-h-0 max-w-3xl flex-col gap-2 text-xs text-muted-foreground",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-center gap-2",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3.5 shrink-0 animate-spin" }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: getCreationProgressLabel(entry) }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-muted-foreground/70",
							children: elapsedLabel
						})
					]
				})
			})
		})]
	});
}
function VmProvisioningStatus({ elapsedLabel, log, error, onCancel, onRetry, onDismiss }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "flex min-h-full justify-center pt-12",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex w-full max-w-2xl flex-col gap-4",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "flex flex-col items-center gap-2 text-center",
				children: error !== void 0 && error !== null ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex flex-wrap items-center justify-center gap-x-2 gap-y-1 text-sm font-medium",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "size-4 shrink-0 text-destructive" }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-destructive",
							children: translate("auto.components.worktree.creation.WorktreeCreationPanel.ed2a664f8b", "Couldn’t create worktree")
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "font-normal text-muted-foreground",
							children: error
						})
					]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-center gap-3 text-xs",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
						type: "button",
						onClick: onRetry,
						className: "inline-flex items-center gap-1 text-foreground hover:underline",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RotateCcw, { className: "size-3" }), translate("auto.components.worktree.creation.WorktreeCreationPanel.34dd5ee38b", "Retry")]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
						type: "button",
						onClick: onDismiss,
						className: "text-muted-foreground hover:text-foreground hover:underline",
						children: translate("auto.components.worktree.creation.WorktreeCreationPanel.dabd226118", "Dismiss")
					})]
				})] }) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-center gap-2 text-sm font-medium text-foreground",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-4 shrink-0 animate-spin text-muted-foreground" }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.worktree.creation.WorktreeCreationPanel.vmProvisioningTitle", "Provisioning VM") }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-xs font-normal text-muted-foreground",
							children: elapsedLabel
						})
					]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
					type: "button",
					onClick: onCancel,
					className: "text-xs text-muted-foreground hover:text-foreground hover:underline",
					children: translate("auto.components.worktree.creation.WorktreeCreationPanel.cancelProvisioning", "Cancel")
				})] })
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RecipeOutputLog, {
				log,
				emptyLabel: translate("auto.components.worktree.creation.WorktreeCreationPanel.vmProvisioningLogEmpty", "Waiting for recipe output…")
			})]
		})
	});
}
function RecipeOutputLog({ log, emptyLabel }) {
	const ref = import_react.useRef(null);
	const pinnedToBottomRef = import_react.useRef(true);
	const handleScroll = import_react.useCallback(() => {
		const el = ref.current;
		if (!el) return;
		pinnedToBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 8;
	}, []);
	import_react.useEffect(() => {
		const el = ref.current;
		if (el && pinnedToBottomRef.current) el.scrollTop = el.scrollHeight;
	}, [log]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("pre", {
		ref,
		onScroll: handleScroll,
		className: "scrollbar-sleek h-72 overflow-auto whitespace-pre-wrap rounded-md bg-muted/40 p-3 font-mono text-[11px] leading-4 text-muted-foreground",
		children: log || /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "text-muted-foreground/60",
			children: emptyLabel
		})
	});
}
function formatElapsedTime(elapsedMs) {
	const totalSeconds = Math.max(0, Math.floor(elapsedMs / 1e3));
	const minutes = Math.floor(totalSeconds / 60);
	const seconds = totalSeconds % 60;
	if (minutes === 0) return `${seconds}s`;
	return `${minutes}m ${seconds.toString().padStart(2, "0")}s`;
}
export { WorktreeCreationPanel as default };
