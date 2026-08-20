import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { l as createLucideIcon, n as cn, t as Button } from "./button-DszXJEV6.js";
import { t as ArrowRight } from "./arrow-right-ct5UxmKv.js";
import { t as Check } from "./check-Lb2n4tDb.js";
import { t as Copy } from "./copy-jk2iqVkp.js";
import { t as ExternalLink } from "./external-link-BrcDtGAn.js";
import { t as LoaderCircle } from "./loader-circle-CRZpWdsi.js";
import { t as RefreshCw } from "./refresh-cw-BU_ChOig.js";
import { t as Share2 } from "./share-2-BAaIwG8k.js";
import { Dc as dirname, rp as callRuntimeRpc, t as useAppStore } from "./store-CgXrfmaH.js";
import { w as keybindingMatchesAction } from "./plugin-manifest-Bs-50M_g.js";
import { n as toast } from "./dist-DgqligFk.js";
import { i as PopoverTrigger, r as PopoverContent, t as Popover } from "./popover-CgR1mzy7.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
import { n as openArtifactInBrowser, t as copyArtifactLink } from "./artifact-link-actions-q80ozQEP.js";
var GitCompareArrows = createLucideIcon("git-compare-arrows", [
	["circle", {
		cx: "5",
		cy: "6",
		r: "3",
		key: "1qnov2"
	}],
	["path", {
		d: "M12 6h5a2 2 0 0 1 2 2v7",
		key: "1yj91y"
	}],
	["path", {
		d: "m15 9-3-3 3-3",
		key: "1lwv8l"
	}],
	["circle", {
		cx: "19",
		cy: "18",
		r: "3",
		key: "1qljk2"
	}],
	["path", {
		d: "M12 18H7a2 2 0 0 1-2-2V9",
		key: "16sdep"
	}],
	["path", {
		d: "m9 15 3 3-3 3",
		key: "1m3kbl"
	}]
]);
function isAbsolutePathLike(value) {
	return value.startsWith("/") || value.startsWith("\\\\") || /^[A-Za-z]:[\\/]/.test(value);
}
function canUseChangesModeForFile(file) {
	return file.mode === "edit" && !file.isUntitled && file.relativePath !== file.filePath && !isAbsolutePathLike(file.relativePath);
}
function getUntitledFileRoot(file, worktreePath) {
	if (worktreePath) return worktreePath;
	if (!file.relativePath) return dirname(file.filePath);
	const rootLength = file.filePath.length - file.relativePath.length - 1;
	if (rootLength <= 0) return dirname(file.filePath);
	return file.filePath.slice(0, rootLength);
}
var MARKDOWN_EDIT_VIEW_MODES = ["source", "rich"];
var MARKDOWN_DIFF_VIEW_MODES = ["source", "rich"];
var MERMAID_VIEW_MODES = ["source", "rich"];
var CSV_VIEW_MODES = ["source", "rich"];
var NOTEBOOK_VIEW_MODES = ["source", "rich"];
var NO_VIEW_MODES = [];
var CODE_EDIT_TOGGLE_MODES = ["edit", "changes"];
function getEditorToggleModes(target) {
	if (target.mode !== "edit") return getMarkdownViewModes(target);
	if (target.language === "notebook") return NOTEBOOK_VIEW_MODES;
	const languageModes = getMarkdownViewModes(target);
	if (languageModes.length > 0) return [...languageModes, "changes"];
	return CODE_EDIT_TOGGLE_MODES;
}
function getMarkdownViewModes(target) {
	if (target.language === "markdown") {
		if (target.mode === "edit") return MARKDOWN_EDIT_VIEW_MODES;
		if (target.mode === "diff" && target.diffSource !== "combined-all" && target.diffSource !== "combined-uncommitted" && target.diffSource !== "combined-branch" && target.diffSource !== "combined-commit") return MARKDOWN_DIFF_VIEW_MODES;
	}
	if (target.language === "mermaid" && target.mode === "edit") return MERMAID_VIEW_MODES;
	if ((target.language === "csv" || target.language === "tsv") && target.mode === "edit") return CSV_VIEW_MODES;
	if (target.language === "notebook" && target.mode === "edit") return NOTEBOOK_VIEW_MODES;
	return NO_VIEW_MODES;
}
function getDefaultMarkdownViewMode(target) {
	if (target.language === "markdown" && target.mode === "diff") return "source";
	return getMarkdownViewModes(target).includes("rich") ? "rich" : "source";
}
function canOpenMarkdownPreview(target) {
	return target.language === "markdown" && target.mode === "edit";
}
function isMarkdownPreviewShortcut(event, platform, keybindings) {
	return keybindingMatchesAction("editor.markdownPreview", event, platform, keybindings);
}
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function ArtifactPublishedLinkPanel({ shareUrl, publishing, sharingEnabled, onUpdate }) {
	const [copied, setCopied] = (0, import_react.useState)(false);
	const copiedResetTimerRef = (0, import_react.useRef)(null);
	const mountedRef = (0, import_react.useRef)(false);
	const clearCopiedResetTimer = (0, import_react.useCallback)(() => {
		if (copiedResetTimerRef.current !== null) {
			window.clearTimeout(copiedResetTimerRef.current);
			copiedResetTimerRef.current = null;
		}
	}, []);
	const setPanelRef = (0, import_react.useCallback)((node) => {
		mountedRef.current = node !== null;
		if (!node) clearCopiedResetTimer();
	}, [clearCopiedResetTimer]);
	const copyLink = async () => {
		if (!await copyArtifactLink(shareUrl, { showSuccessToast: false })) return;
		if (!mountedRef.current) return;
		setCopied(true);
		clearCopiedResetTimer();
		copiedResetTimerRef.current = window.setTimeout(() => {
			copiedResetTimerRef.current = null;
			setCopied(false);
		}, 1500);
	};
	const copyLabel = copied ? translate("auto.components.artifacts.copySuccess", "Artifact link copied") : translate("auto.components.artifacts.ArtifactPublishedLinkPanel.copyLink", "Copy link");
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		ref: setPanelRef,
		className: "space-y-3",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex min-w-0 items-center gap-0.5",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "min-w-0 flex-1 truncate font-mono text-[11px] text-muted-foreground",
					title: shareUrl,
					children: shareUrl
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
					asChild: true,
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						type: "button",
						variant: "ghost",
						size: "icon-xs",
						className: "text-muted-foreground hover:text-foreground",
						onClick: () => void copyLink(),
						"aria-label": copyLabel,
						children: copied ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, { className: "size-3.5" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Copy, {})
					})
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
					side: "bottom",
					sideOffset: 4,
					children: copyLabel
				})] }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
					asChild: true,
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						type: "button",
						variant: "ghost",
						size: "icon-xs",
						className: "text-muted-foreground hover:text-foreground",
						onClick: () => openArtifactInBrowser(shareUrl),
						"aria-label": translate("auto.components.artifacts.ArtifactPublishedLinkPanel.openLink", "Open link"),
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExternalLink, {})
					})
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
					side: "bottom",
					sideOffset: 4,
					children: translate("auto.components.artifacts.ArtifactPublishedLinkPanel.openLink", "Open link")
				})] })
			]
		}), sharingEnabled ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
			type: "button",
			variant: "outline",
			size: "sm",
			className: "w-full",
			disabled: publishing,
			onClick: onUpdate,
			children: [publishing ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "animate-spin" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RefreshCw, {}), publishing ? translate("auto.components.artifacts.ArtifactPublishedLinkPanel.updating", "Updating…") : translate("auto.components.artifacts.ArtifactPublishedLinkPanel.update", "Update shared content")]
		}) : null]
	});
}
var LOCAL_RUNTIME$1 = { kind: "local" };
async function getPublishedArtifactLink(sourceKey) {
	const result = await callRuntimeRpc(LOCAL_RUNTIME$1, "artifacts.getPublishedLink", { sourceKey });
	if (result.status === "ok") return result.value?.shareUrl ?? null;
	throw new Error(result.status);
}
const ARTIFACT_CLI_MAX_RPC_BYTES = 800 * 1024;
function artifactWriteRequestByteLength(request) {
	return new TextEncoder().encode(JSON.stringify(request)).byteLength;
}
var LOCAL_RUNTIME = { kind: "local" };
var ArtifactPublishPreparationError = class extends Error {
	constructor(code) {
		super(code);
		this.code = code;
	}
};
function validateArtifactPublishRequest(request) {
	if (!request.content) throw new ArtifactPublishPreparationError("empty");
	if (artifactWriteRequestByteLength(request) > 819200) throw new ArtifactPublishPreparationError("too-large");
	return request;
}
async function publishArtifactFromSurface(createRequest) {
	try {
		if (!await ensureArtifactAccountConnected()) return null;
		for (let attempt = 0; attempt < 2; attempt += 1) {
			const result = await callRuntimeRpc(LOCAL_RUNTIME, "artifacts.publish", validateArtifactPublishRequest(await createRequest()));
			if (result.status === "ok") {
				showArtifactPublishedToast(result.value);
				return result.value;
			}
			if (result.status === "unconfigured") {
				toast.error(translate("auto.components.artifacts.artifact-publish-flow.9a078a0c65", "Artifact sharing is unavailable"), { description: result.message });
				return null;
			}
			if (attempt === 0 && await reconnectArtifactAccount()) continue;
			toast.error(translate("auto.components.artifacts.artifact-publish-flow.bba20daa6d", "Sign in to Orca and try again."));
			return null;
		}
	} catch (error) {
		console.error("Failed to publish artifact:", error);
		toast.error(translate("auto.components.artifacts.artifact-publish-flow.54b1805328", "Could not share artifact"), error instanceof ArtifactPublishPreparationError ? { description: artifactPreparationErrorDescription(error.code) } : void 0);
	}
	return null;
}
async function ensureArtifactAccountConnected() {
	const state = useAppStore.getState();
	if (state.orcaProfileAuthStatus?.state === "connected") return true;
	return (await state.connectCurrentOrcaProfile())?.status === "connected";
}
async function reconnectArtifactAccount() {
	return (await useAppStore.getState().connectCurrentOrcaProfile())?.status === "connected";
}
function showArtifactPublishedToast(result) {
	toast.success(result.change === "created" ? translate("auto.components.artifacts.artifact-publish-flow.430019efd0", "Artifact shared") : translate("auto.components.artifacts.artifact-publish-flow.2fc727c831", "Artifact updated"));
}
function artifactPreparationErrorDescription(code) {
	switch (code) {
		case "empty": return translate("auto.components.artifacts.artifact-publish-flow.fbb5018602", "This file is empty.");
		case "too-large": return translate("auto.components.artifacts.artifact-publish-flow.6112db5a1c", "Artifacts shared from Orca must be smaller than 800 KB.");
		case "unreadable": return translate("auto.components.artifacts.artifact-publish-flow.e2ed5acd8c", "Orca couldn't read this file. Open it from a workspace and try again.");
		case "unsupported": return translate("auto.components.artifacts.artifact-publish-flow.6d475e9b25", "Only local HTML and Markdown files can be shared as artifacts.");
		case "binary": return translate("auto.components.artifacts.artifact-publish-flow.29a406be09", "Artifacts must contain text.");
	}
}
function ArtifactPublishButton({ sourceKey, createRequest, className, disabled }) {
	const [open, setOpen] = (0, import_react.useState)(false);
	const [publishing, setPublishing] = (0, import_react.useState)(false);
	const [lookupRevision, setLookupRevision] = (0, import_react.useState)(0);
	const [linkLookup, setLinkLookup] = (0, import_react.useState)(null);
	const lookupSequence = (0, import_react.useRef)(0);
	const popoverContentRef = (0, import_react.useRef)(null);
	const authStatus = useAppStore((state) => state.orcaProfileAuthStatus);
	const connecting = useAppStore((state) => state.orcaProfileConnecting);
	const connect = useAppStore((state) => state.connectCurrentOrcaProfile);
	const openSettingsPage = useAppStore((state) => state.openSettingsPage);
	const openSettingsTarget = useAppStore((state) => state.openSettingsTarget);
	const settings = useAppStore((state) => state.settings);
	const signedIn = authStatus?.state === "connected";
	const sharingEnabled = settings?.artifactSharingEnabled === true;
	const accountKey = authStatus?.state === "connected" ? JSON.stringify([
		authStatus.activeProfileId,
		authStatus.cloud?.userId ?? null,
		authStatus.cloud?.cloudProfileId ?? null,
		authStatus.cloud?.activeOrgId ?? null
	]) : null;
	const lookupKey = accountKey ? JSON.stringify([accountKey, sourceKey]) : null;
	const currentLookup = linkLookup?.key === lookupKey ? linkLookup : null;
	const checkingLink = signedIn && currentLookup?.status !== "loaded" && currentLookup?.status !== "error";
	const publishedLink = currentLookup?.status === "loaded" ? currentLookup.shareUrl : null;
	const busy = publishing || connecting;
	const blocked = disabled || busy;
	(0, import_react.useEffect)(() => {
		const sequence = ++lookupSequence.current;
		if (!open || !lookupKey) {
			setLinkLookup(null);
			return;
		}
		setLinkLookup({
			key: lookupKey,
			status: "loading",
			shareUrl: null
		});
		getPublishedArtifactLink(sourceKey).then((shareUrl) => {
			if (lookupSequence.current === sequence) setLinkLookup({
				key: lookupKey,
				status: "loaded",
				shareUrl
			});
		}).catch((error) => {
			console.error("Failed to check published artifact link:", error);
			if (lookupSequence.current === sequence) setLinkLookup({
				key: lookupKey,
				status: "error",
				shareUrl: null
			});
		});
		return () => {
			lookupSequence.current += 1;
		};
	}, [
		lookupKey,
		lookupRevision,
		open,
		sourceKey
	]);
	const publish = async () => {
		if (blocked || !signedIn || !sharingEnabled) return;
		setPublishing(true);
		try {
			const result = await publishArtifactFromSurface(createRequest);
			if (result) {
				if (lookupKey) setLinkLookup({
					key: lookupKey,
					status: "loaded",
					shareUrl: result.item.shareUrl
				});
			}
		} finally {
			setPublishing(false);
		}
	};
	const openArtifactsSettings = () => {
		setOpen(false);
		openSettingsTarget({
			pane: "artifacts",
			repoId: null
		});
		openSettingsPage();
	};
	const label = translate("auto.components.artifacts.ArtifactPublishButton.a4a49da6af", "Share as artifact");
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Popover, {
		open,
		onOpenChange: (nextOpen) => !busy && setOpen(nextOpen),
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
			asChild: true,
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PopoverTrigger, {
				asChild: true,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					type: "button",
					variant: "ghost",
					size: "icon-sm",
					className: cn("shrink-0", className),
					disabled: blocked,
					"aria-label": label,
					children: publishing ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "animate-spin" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Share2, {})
				})
			})
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
			side: "bottom",
			sideOffset: 4,
			children: label
		})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(PopoverContent, {
			ref: popoverContentRef,
			tabIndex: -1,
			align: "end",
			sideOffset: 6,
			className: "w-80 p-0",
			onOpenAutoFocus: (event) => {
				event.preventDefault();
				popoverContentRef.current?.focus({ preventScroll: true });
			},
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-1 border-b border-border/60 px-4 py-3.5",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h3", {
					className: "text-sm font-semibold",
					children: translate("auto.components.artifacts.ArtifactPublishButton.confirmTitle", "Share as artifact")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "text-xs leading-5 text-muted-foreground",
					children: publishedLink ? translate("auto.components.artifacts.ArtifactPublishButton.publishedDescription", "Anyone with this link can view the shared file.") : translate("auto.components.artifacts.ArtifactPublishButton.confirmDescription", "This publishes the current file at a link anyone with the URL can view.")
				})]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-3 p-4",
				children: [
					!signedIn ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-center justify-between gap-3",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "min-w-0 space-y-0.5",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "text-xs font-medium",
								children: translate("auto.components.artifacts.ArtifactPublishButton.accountTitle", "Orca account")
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "text-[11px] leading-4 text-muted-foreground",
								children: translate("auto.components.artifacts.ArtifactPublishButton.accountDescription", "Sign in to create and manage this link.")
							})]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							type: "button",
							variant: "outline",
							size: "xs",
							disabled: connecting || authStatus?.configured !== true,
							onClick: () => void connect(),
							children: connecting ? translate("auto.components.artifacts.ArtifactPublishButton.signingIn", "Signing in…") : authStatus?.state === "reconnect-required" ? translate("auto.components.artifacts.ArtifactPublishButton.signInAgain", "Sign in again") : translate("auto.components.artifacts.ArtifactPublishButton.signIn", "Sign in")
						})]
					}) : null,
					!sharingEnabled ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: cn("space-y-2", !signedIn && "border-t border-border/60 pt-3"),
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "space-y-0.5",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "text-xs font-medium",
								children: translate("auto.components.artifacts.ArtifactPublishButton.publishingOffTitle", "Artifact sharing is off")
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "text-[11px] leading-4 text-muted-foreground",
								children: translate("auto.components.artifacts.ArtifactPublishButton.publishingOffDescription", "Learn about public links and enable sharing in Settings.")
							})]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
							type: "button",
							variant: "outline",
							size: "sm",
							className: "w-full",
							onClick: openArtifactsSettings,
							children: [translate("auto.components.artifacts.ArtifactPublishButton.openSettings", "Open Artifacts settings"), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowRight, {})]
						})]
					}) : null,
					checkingLink ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-center justify-center gap-2 py-2 text-xs text-muted-foreground",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3.5 animate-spin" }), translate("auto.components.artifacts.ArtifactPublishButton.checkingLink", "Checking for an existing link…")]
					}) : currentLookup?.status === "error" ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "space-y-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "text-xs leading-5 text-muted-foreground",
							children: translate("auto.components.artifacts.ArtifactPublishButton.checkFailed", "Could not check for an existing link.")
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							type: "button",
							variant: "outline",
							size: "sm",
							className: "w-full",
							onClick: () => setLookupRevision((current) => current + 1),
							children: translate("auto.components.artifacts.ArtifactPublishButton.tryAgain", "Try again")
						})]
					}) : publishedLink ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArtifactPublishedLinkPanel, {
						shareUrl: publishedLink,
						publishing,
						sharingEnabled,
						onUpdate: () => void publish()
					}, publishedLink) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
						type: "button",
						size: "sm",
						className: "w-full",
						disabled: !signedIn || !sharingEnabled || busy,
						onClick: () => void publish(),
						children: [publishing ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "animate-spin" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Share2, {}), publishing ? translate("auto.components.artifacts.ArtifactPublishButton.sharing", "Sharing…") : translate("auto.components.artifacts.ArtifactPublishButton.sharePublicLink", "Share public link")]
					})
				]
			})]
		})]
	});
}
export { getDefaultMarkdownViewMode as a, isMarkdownPreviewShortcut as c, isAbsolutePathLike as d, GitCompareArrows as f, canOpenMarkdownPreview as i, getUntitledFileRoot as l, ArtifactPublishPreparationError as n, getEditorToggleModes as o, ARTIFACT_CLI_MAX_RPC_BYTES as r, getMarkdownViewModes as s, ArtifactPublishButton as t, canUseChangesModeForFile as u };
