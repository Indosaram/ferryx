import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, n as getIntlLocale, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn, t as Button } from "./button-DszXJEV6.js";
import { t as ArrowRight } from "./arrow-right-ct5UxmKv.js";
import { t as CircleAlert } from "./circle-alert-keRTpMg-.js";
import { t as Copy } from "./copy-jk2iqVkp.js";
import { t as ExternalLink } from "./external-link-BrcDtGAn.js";
import { t as FileCodeCorner } from "./file-code-corner-UKtQpxid.js";
import { t as FileText } from "./file-text-DTz7D9iJ.js";
import { t as Files } from "./files-BSrZfmBN.js";
import { t as Globe } from "./globe-c32i33v9.js";
import { t as LoaderCircle } from "./loader-circle-CRZpWdsi.js";
import { t as RefreshCw } from "./refresh-cw-BU_ChOig.js";
import { t as Search } from "./search-DK1nVA6d.js";
import { _l as moveFocusToRendererBeforeWebviewDetach, m_ as Trash2, rp as callRuntimeRpc, t as useAppStore } from "./store-CgXrfmaH.js";
import { t as X } from "./x-BrGKE4uz.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import { n as toast } from "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import { f as ContextMenuTrigger, n as ContextMenuContent, r as ContextMenuItem, s as ContextMenuSeparator, t as ContextMenu } from "./context-menu-D4RKI7hR.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
import "./useMountedRef-1omUd-IV.js";
import { t as Input } from "./input-DV5rpysh.js";
import { n as useConfirmationDialog } from "./confirmation-dialog-context-CeUkq8ZE.js";
import { n as formatUiRelativeTimeFromDate, t as formatUiRelativeTime } from "./relative-time-format-BdBnutwN.js";
import { t as ORCA_BROWSER_GUEST_WEB_PREFERENCES_ATTRIBUTE } from "./browser-guest-web-preferences-1gA9qdBB.js";
import { n as openArtifactInBrowser, t as copyArtifactLink } from "./artifact-link-actions-q80ozQEP.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
function persistConfirmationSkipPreference({ updates, settingsSectionId, updateSettings, openSettingsPage, openSettingsTarget }) {
	updateSettings(updates).then(() => toast.success(translate("auto.components.confirmation.skip.saved", "We'll skip this confirmation next time."), {
		description: translate("auto.components.confirmation.skip.savedDescription", "You can change this in Settings."),
		duration: 8e3,
		action: {
			label: translate("auto.components.confirmation.skip.openSettings", "Open Settings"),
			onClick: () => {
				openSettingsPage();
				openSettingsTarget({
					pane: "general",
					repoId: null,
					sectionId: settingsSectionId
				});
			}
		}
	}), () => toast.error(translate("auto.components.confirmation.skip.preference.0b0cb6e3f9", "Could not save the confirmation preference.")));
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function ArtifactActions({ deleting, item, onDelete }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex shrink-0 items-center gap-2",
		"aria-label": translate("auto.components.artifacts.actions", "Artifact actions"),
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
				size: "sm",
				onClick: () => void copyArtifactLink(item.shareUrl),
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Copy, {}), translate("auto.components.artifacts.copyLink", "Copy link")]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
				asChild: true,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					variant: "ghost",
					size: "icon-sm",
					className: "text-muted-foreground hover:text-foreground",
					onClick: () => openArtifactInBrowser(item.shareUrl),
					"aria-label": translate("auto.components.artifacts.openInBrowser", "Open in browser"),
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExternalLink, {})
				})
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
				side: "bottom",
				sideOffset: 6,
				children: translate("auto.components.artifacts.openInBrowser", "Open in browser")
			})] }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
				asChild: true,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					variant: "ghost",
					size: "icon-sm",
					className: "text-muted-foreground hover:text-destructive",
					disabled: deleting,
					onClick: () => onDelete(item),
					"aria-label": translate("auto.components.artifacts.ArtifactsPage.deleteArtifact", "Delete artifact"),
					children: deleting ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "animate-spin" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, {})
				})
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
				side: "bottom",
				sideOffset: 6,
				children: translate("auto.components.artifacts.ArtifactsPage.deleteArtifact", "Delete artifact")
			})] })
		]
	});
}
function artifactName(item) {
	return item.artifact.title || item.artifact.originalFileName || item.artifact.slug;
}
function formatArtifactDate(value) {
	return new Intl.DateTimeFormat(getIntlLocale(), {
		dateStyle: "medium",
		timeStyle: "short"
	}).format(new Date(value));
}
function formatByteSize(value) {
	if (value < 1024) return `${value} B`;
	if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
	return `${(value / (1024 * 1024)).toFixed(1)} MB`;
}
function formatArtifactUpdatedAt(value) {
	return translate("auto.components.artifacts.updatedAt", "Updated {{when}}", { when: formatUiRelativeTimeFromDate(value, translate("auto.components.artifacts.updatedRecently", "recently")) });
}
function formatArtifactExpiry(value) {
	const expiresAt = new Date(value);
	if (Number.isNaN(expiresAt.getTime())) return translate("auto.components.artifacts.expiryUnknown", "Expiry unknown");
	const remainingMs = expiresAt.getTime() - Date.now();
	return remainingMs <= 0 ? translate("auto.components.artifacts.expired", "Link expired") : translate("auto.components.artifacts.expires", "Link expires {{when}}", { when: formatUiRelativeTime(remainingMs) });
}
function artifactTypeIcon(item) {
	return item.artifact.sourceContentType === "text/markdown" ? FileText : FileCodeCorner;
}
function ArtifactDetailHeader({ deleting, item, onDelete }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex flex-wrap items-start justify-between gap-3 border-b border-border/50 px-4 py-3",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "min-w-40 flex-1 space-y-0.5",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h2", {
					className: "truncate text-sm font-semibold",
					children: artifactName(item)
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex min-w-0 items-center gap-1.5",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
							asChild: true,
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Globe, { className: "size-3 shrink-0 text-muted-foreground" })
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
							side: "bottom",
							sideOffset: 6,
							children: translate("auto.components.artifacts.ArtifactDetailHeader.publicLink", "Anyone with this link can view it")
						})] }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "sr-only",
							children: translate("auto.components.artifacts.ArtifactDetailHeader.publicLink", "Anyone with this link can view it")
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "truncate font-mono text-xs text-muted-foreground",
							children: item.shareUrl
						})
					]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
					className: "truncate text-[11px] text-muted-foreground",
					children: [
						formatArtifactUpdatedAt(item.artifact.updatedAt),
						" ·",
						" ",
						formatByteSize(item.artifact.byteSize),
						" · ",
						formatArtifactExpiry(item.artifact.expiresAt)
					]
				})
			]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArtifactActions, {
			deleting,
			item,
			onDelete
		})]
	});
}
var OPTION_SELECTOR = "[role=\"option\"]";
function moveOptionFocus(listbox, from, step) {
	const options = [...listbox?.querySelectorAll(OPTION_SELECTOR) ?? []];
	options[options.indexOf(from) + step]?.focus();
}
function focusEdgeOption(listbox, edge) {
	const options = [...listbox?.querySelectorAll(OPTION_SELECTOR) ?? []];
	(edge === "first" ? options.at(0) : options.at(-1))?.focus();
}
function ArtifactListPane({ artifacts, className, deletingId, selectedArtifact, selectArtifact, deleteArtifact, hasMore, loadingMore, loadMore }) {
	const listboxRef = (0, import_react.useRef)(null);
	const [query, setQuery] = (0, import_react.useState)("");
	const normalizedQuery = query.trim().toLowerCase();
	const matches = (0, import_react.useMemo)(() => normalizedQuery ? artifacts.filter((item) => artifactName(item).toLowerCase().includes(normalizedQuery)) : artifacts, [artifacts, normalizedQuery]);
	const onOptionKeyDown = (event, slug) => {
		const option = event.currentTarget;
		if (event.key === "ArrowDown") {
			event.preventDefault();
			moveOptionFocus(listboxRef.current, option, 1);
		} else if (event.key === "ArrowUp") {
			event.preventDefault();
			moveOptionFocus(listboxRef.current, option, -1);
		} else if (event.key === "Home") {
			event.preventDefault();
			focusEdgeOption(listboxRef.current, "first");
		} else if (event.key === "End") {
			event.preventDefault();
			focusEdgeOption(listboxRef.current, "last");
		} else if (event.key === "Enter" || event.key === " ") {
			event.preventDefault();
			selectArtifact(slug);
		}
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: cn("flex min-h-0 flex-col", className),
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "relative shrink-0 border-b border-border/40 px-2 py-2",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Search, { className: "pointer-events-none absolute left-4.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
				value: query,
				onChange: (event) => setQuery(event.target.value),
				placeholder: translate("auto.components.artifacts.ArtifactListPane.search", "Search artifacts"),
				className: "h-8 pl-8 text-sm"
			})]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "min-h-0 flex-1 overflow-y-auto scrollbar-sleek",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					ref: listboxRef,
					role: "listbox",
					"aria-label": translate("auto.components.artifacts.ArtifactListPane.listLabel", "Shared artifacts"),
					"aria-orientation": "vertical",
					children: matches.map((item) => {
						const selected = item.artifact.slug === selectedArtifact.artifact.slug;
						const name = artifactName(item);
						const TypeIcon = artifactTypeIcon(item);
						return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ContextMenu, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ContextMenuTrigger, {
							asChild: true,
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								role: "option",
								"aria-selected": selected,
								"aria-current": selected ? "page" : void 0,
								"data-current": selected ? "true" : void 0,
								tabIndex: selected ? 0 : -1,
								onClick: () => selectArtifact(item.artifact.slug),
								onKeyDown: (event) => onOptionKeyDown(event, item.artifact.slug),
								className: cn("flex w-full cursor-pointer items-center gap-3 border-b border-border/50 px-3 py-3 text-left transition-colors last:border-b-0 hover:bg-accent/50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring", selected && "bg-accent"),
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TypeIcon, { className: "size-4 shrink-0 text-muted-foreground" }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
									className: "min-w-0 flex-1",
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
										asChild: true,
										children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
											className: "block truncate text-sm font-medium",
											children: name
										})
									}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(TooltipContent, {
										side: "right",
										sideOffset: 6,
										children: [
											/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
												className: "font-medium",
												children: name
											}),
											/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
												className: "text-background/70",
												children: formatArtifactDate(item.artifact.updatedAt)
											}),
											/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
												className: "text-background/70",
												children: formatArtifactExpiry(item.artifact.expiresAt)
											})
										]
									})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
										className: "block truncate text-xs text-muted-foreground",
										children: [
											formatArtifactUpdatedAt(item.artifact.updatedAt),
											" ·",
											" ",
											formatByteSize(item.artifact.byteSize)
										]
									})]
								})]
							})
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ContextMenuContent, { children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ContextMenuItem, {
								onSelect: () => void copyArtifactLink(item.shareUrl),
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Copy, {}), translate("auto.components.artifacts.copyLink", "Copy link")]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ContextMenuItem, {
								onSelect: () => openArtifactInBrowser(item.shareUrl),
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExternalLink, {}), translate("auto.components.artifacts.openInBrowser", "Open in browser")]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ContextMenuSeparator, {}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ContextMenuItem, {
								variant: "destructive",
								disabled: deletingId === item.artifact.slug,
								onSelect: () => deleteArtifact(item),
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, {}), translate("auto.components.artifacts.ArtifactsPage.deleteArtifact", "Delete artifact")]
							})
						] })] }, item.artifact.slug);
					})
				}),
				matches.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "px-3 py-6 text-center text-xs text-muted-foreground",
					children: translate("auto.components.artifacts.ArtifactListPane.noMatches", "No matches")
				}) : null,
				hasMore ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "border-t border-border/50 p-2",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
						type: "button",
						variant: "ghost",
						size: "sm",
						className: "w-full",
						disabled: loadingMore,
						onClick: loadMore,
						children: [loadingMore ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "animate-spin" }) : null, translate("auto.components.artifacts.ArtifactCollection.loadMore", "Load more")]
					})
				}) : null
			]
		})]
	});
}
var ARTIFACT_PREVIEW_LOAD_TIMEOUT_MS = 2e4;
function scheduleArtifactPreviewTimeout(onTimeout) {
	const timeout = setTimeout(onTimeout, ARTIFACT_PREVIEW_LOAD_TIMEOUT_MS);
	return () => clearTimeout(timeout);
}
function artifactPreviewUrl(shareUrl) {
	const url = new URL(shareUrl);
	url.searchParams.set("embed", "1");
	return url.toString();
}
function attachArtifactWebview({ container, partition, shareUrl, onLoadStarted, onLoadStopped, onLoadFailed }) {
	const webview = document.createElement("webview");
	webview.setAttribute("partition", partition);
	webview.setAttribute("webpreferences", ORCA_BROWSER_GUEST_WEB_PREFERENCES_ATTRIBUTE);
	webview.setAttribute("aria-label", translate("auto.components.artifacts.preview", "Artifact preview"));
	webview.style.display = "flex";
	webview.style.width = "100%";
	webview.style.height = "100%";
	webview.style.border = "none";
	webview.addEventListener("did-start-loading", onLoadStarted);
	webview.addEventListener("did-stop-loading", onLoadStopped);
	webview.addEventListener("did-fail-load", onLoadFailed);
	container.appendChild(webview);
	webview.setAttribute("src", artifactPreviewUrl(shareUrl));
	return () => {
		webview.removeEventListener("did-start-loading", onLoadStarted);
		webview.removeEventListener("did-stop-loading", onLoadStopped);
		webview.removeEventListener("did-fail-load", onLoadFailed);
		moveFocusToRendererBeforeWebviewDetach(webview);
		webview.remove();
	};
}
function ArtifactPreview({ shareUrl }) {
	const containerRef = (0, import_react.useRef)(null);
	const [state, setState] = (0, import_react.useState)("loading");
	(0, import_react.useEffect)(() => {
		let disposed = false;
		let detachPreview;
		let cancelLoadTimeout;
		let loadFailed = false;
		const clearLoadTimeout = () => {
			cancelLoadTimeout?.();
			cancelLoadTimeout = void 0;
		};
		const startLoadTimeout = () => {
			clearLoadTimeout();
			cancelLoadTimeout = scheduleArtifactPreviewTimeout(() => {
				loadFailed = true;
				setState("unavailable");
			});
		};
		const onLoadStarted = () => {
			loadFailed = false;
			setState("loading");
			startLoadTimeout();
		};
		const onLoadStopped = () => {
			clearLoadTimeout();
			if (!loadFailed) setState("ready");
		};
		const onLoadFailed = (event) => {
			if (!event.isMainFrame || event.errorCode === -3) return;
			clearLoadTimeout();
			loadFailed = true;
			setState("unavailable");
		};
		setState("loading");
		startLoadTimeout();
		window.api.browser.sessionResolvePartition({ profileId: null }).then((partition) => {
			if (disposed || !partition || !containerRef.current) {
				if (!disposed) {
					clearLoadTimeout();
					setState("unavailable");
				}
				return;
			}
			detachPreview = attachArtifactWebview({
				container: containerRef.current,
				partition,
				shareUrl,
				onLoadStarted,
				onLoadStopped,
				onLoadFailed
			});
			startLoadTimeout();
		}).catch(() => {
			if (!disposed) {
				clearLoadTimeout();
				setState("unavailable");
			}
		});
		return () => {
			disposed = true;
			clearLoadTimeout();
			detachPreview?.();
		};
	}, [shareUrl]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "relative flex min-h-0 flex-1 overflow-hidden bg-editor-surface",
		ref: containerRef,
		children: [state === "loading" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "absolute inset-0 z-10 flex items-center justify-center bg-editor-surface",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-5 animate-spin text-muted-foreground" })
		}) : null, state === "unavailable" ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "absolute inset-0 z-10 flex flex-col items-center justify-center gap-2 bg-editor-surface px-6 text-center",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CircleAlert, { className: "size-6 text-muted-foreground" }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "text-sm font-medium",
					children: translate("auto.components.artifacts.previewUnavailable", "Preview unavailable")
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "max-w-sm text-xs text-muted-foreground",
					children: translate("auto.components.artifacts.previewUnavailableDescription", "Open this artifact in your browser to view it.")
				})
			]
		}) : null]
	});
}
function ArtifactCollection({ artifacts, deletingId, selectedArtifact, selectArtifact, deleteArtifact, hasMore, loadingMore, loadMore }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "grid min-h-0 flex-1 grid-cols-1 grid-rows-[auto_minmax(0,1fr)] overflow-hidden lg:grid-cols-[minmax(240px,300px)_minmax(0,1fr)] lg:grid-rows-1",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArtifactListPane, {
			className: "max-h-56 border-b border-border/50 bg-muted/20 lg:max-h-none lg:border-b-0 lg:border-r",
			artifacts,
			deletingId,
			selectedArtifact,
			selectArtifact,
			deleteArtifact,
			hasMore,
			loadingMore,
			loadMore
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", {
			className: "flex min-h-0 min-w-0 flex-1 flex-col bg-background",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArtifactDetailHeader, {
				deleting: deletingId === selectedArtifact.artifact.slug,
				item: selectedArtifact,
				onDelete: deleteArtifact
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArtifactPreview, { shareUrl: selectedArtifact.shareUrl })]
		})]
	});
}
var LOCAL_RUNTIME$1 = { kind: "local" };
var EMPTY_ARTIFACTS = [];
function artifactAccountIdentity(authStatus) {
	return authStatus?.state === "connected" ? `${authStatus.activeProfileId}:${authStatus.cloud?.userId ?? ""}:${authStatus.cloud?.cloudProfileId ?? ""}:${authStatus.cloud?.activeOrgId ?? ""}` : null;
}
function appendArtifactPage(current, incoming) {
	const knownSlugs = new Set(current.map(({ artifact }) => artifact.slug));
	return [...current, ...incoming.filter(({ artifact }) => !knownSlugs.has(artifact.slug))];
}
function artifactRequestIsCurrent(sequence, currentSequence, identity) {
	return sequence === currentSequence && artifactAccountIdentity(useAppStore.getState().orcaProfileAuthStatus) === identity;
}
function useArtifactPagination(authStatus, refreshAuth) {
	const accountIdentity = artifactAccountIdentity(authStatus);
	const [artifactState, setArtifactState] = (0, import_react.useState)({
		identity: null,
		page: { artifacts: [] }
	});
	const [loading, setLoading] = (0, import_react.useState)(false);
	const [loadingMore, setLoadingMore] = (0, import_react.useState)(false);
	const [error, setError] = (0, import_react.useState)(null);
	const loadSequence = (0, import_react.useRef)(0);
	const loadingCursor = (0, import_react.useRef)(null);
	const currentPage = artifactState.identity === accountIdentity ? artifactState.page : null;
	const artifacts = currentPage?.artifacts ?? EMPTY_ARTIFACTS;
	const loadArtifacts = (0, import_react.useCallback)(async () => {
		const sequence = ++loadSequence.current;
		loadingCursor.current = null;
		setLoadingMore(false);
		if (!accountIdentity) {
			setArtifactState({
				identity: null,
				page: { artifacts: [] }
			});
			setError(null);
			setLoading(false);
			return;
		}
		setLoading(true);
		setError(null);
		try {
			const result = await callRuntimeRpc(LOCAL_RUNTIME$1, "artifacts.list", {});
			if (!artifactRequestIsCurrent(sequence, loadSequence.current, accountIdentity)) return;
			if (result.status === "ok") setArtifactState({
				identity: accountIdentity,
				page: result.value
			});
			else {
				await refreshAuth();
				if (!artifactRequestIsCurrent(sequence, loadSequence.current, accountIdentity)) return;
				setError(translate("auto.components.artifacts.ArtifactsPage.signInAgain", "Sign in to Orca again to load artifacts."));
			}
		} catch (loadError) {
			if (!artifactRequestIsCurrent(sequence, loadSequence.current, accountIdentity)) return;
			console.error("Failed to load artifacts:", loadError);
			setError(translate("auto.components.artifacts.ArtifactsPage.loadFailed", "Could not load artifacts."));
		} finally {
			if (artifactRequestIsCurrent(sequence, loadSequence.current, accountIdentity)) setLoading(false);
		}
	}, [accountIdentity, refreshAuth]);
	(0, import_react.useEffect)(() => {
		loadArtifacts();
		return () => {
			loadSequence.current += 1;
		};
	}, [loadArtifacts]);
	const loadMoreArtifacts = (0, import_react.useCallback)(async () => {
		const cursor = currentPage?.nextCursor;
		if (!accountIdentity || !cursor || loadingCursor.current) return;
		const sequence = loadSequence.current;
		loadingCursor.current = cursor;
		setLoadingMore(true);
		setError(null);
		try {
			const result = await callRuntimeRpc(LOCAL_RUNTIME$1, "artifacts.list", { cursor });
			if (!artifactRequestIsCurrent(sequence, loadSequence.current, accountIdentity)) return;
			if (result.status !== "ok") {
				await refreshAuth();
				if (!artifactRequestIsCurrent(sequence, loadSequence.current, accountIdentity)) return;
				setError(translate("auto.components.artifacts.ArtifactsPage.signInAgain", "Sign in to Orca again to load artifacts."));
				return;
			}
			setArtifactState((current) => current.identity === accountIdentity ? {
				identity: current.identity,
				page: {
					artifacts: appendArtifactPage(current.page.artifacts, result.value.artifacts),
					...result.value.nextCursor && result.value.nextCursor !== cursor ? { nextCursor: result.value.nextCursor } : {}
				}
			} : current);
		} catch (loadError) {
			if (!artifactRequestIsCurrent(sequence, loadSequence.current, accountIdentity)) return;
			console.error("Failed to load more artifacts:", loadError);
			setError(translate("auto.components.artifacts.ArtifactsPage.loadMoreFailed", "Could not load more artifacts."));
		} finally {
			if (loadingCursor.current === cursor) {
				loadingCursor.current = null;
				setLoadingMore(false);
			}
		}
	}, [
		accountIdentity,
		currentPage?.nextCursor,
		refreshAuth
	]);
	const removeArtifact = (0, import_react.useCallback)((identity, slug) => {
		loadSequence.current += 1;
		loadingCursor.current = null;
		setLoading(false);
		setLoadingMore(false);
		setArtifactState((current) => current.identity === identity ? {
			...current,
			page: {
				...current.page,
				artifacts: current.page.artifacts.filter((item) => item.artifact.slug !== slug)
			}
		} : current);
	}, []);
	return {
		accountIdentity,
		artifacts,
		error,
		loading,
		loadingMore,
		nextCursor: currentPage?.nextCursor,
		loadArtifacts,
		loadMoreArtifacts,
		removeArtifact,
		setError
	};
}
var LOCAL_RUNTIME = { kind: "local" };
function ArtifactsPage() {
	const closePage = useAppStore((state) => state.closeArtifactsPage);
	const authStatus = useAppStore((state) => state.orcaProfileAuthStatus);
	const connecting = useAppStore((state) => state.orcaProfileConnecting);
	const connect = useAppStore((state) => state.connectCurrentOrcaProfile);
	const refreshAuth = useAppStore((state) => state.refreshCurrentOrcaProfileAuth);
	const openSettingsPage = useAppStore((state) => state.openSettingsPage);
	const openSettingsTarget = useAppStore((state) => state.openSettingsTarget);
	const settings = useAppStore((state) => state.settings);
	const updateSettings = useAppStore((state) => state.updateSettings);
	const confirm = useConfirmationDialog();
	const publishingBlocked = settings ? settings.artifactSharingEnabled !== true : false;
	const [deleting, setDeleting] = (0, import_react.useState)(null);
	const [selectedSlug, setSelectedSlug] = (0, import_react.useState)(null);
	const signedIn = authStatus?.state === "connected";
	const needsReconnect = authStatus?.state === "reconnect-required";
	const openAccountSettings = () => {
		openSettingsTarget({
			pane: "orca-account",
			repoId: null
		});
		openSettingsPage();
	};
	const { accountIdentity, artifacts, error, loading, loadingMore, nextCursor, loadArtifacts, loadMoreArtifacts, removeArtifact, setError } = useArtifactPagination(authStatus, refreshAuth);
	const deletingId = deleting?.identity === accountIdentity ? deleting.slug : null;
	const selectedArtifact = artifacts.find(({ artifact }) => artifact.slug === selectedSlug) ?? artifacts[0] ?? null;
	(0, import_react.useEffect)(() => {
		setSelectedSlug((current) => {
			if (current && artifacts.some(({ artifact }) => artifact.slug === current)) return current;
			return artifacts[0]?.artifact.slug ?? null;
		});
	}, [artifacts]);
	(0, import_react.useEffect)(() => {
		function onKeyDown(event) {
			if (event.key !== "Escape" || event.defaultPrevented) return;
			const target = event.target;
			if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement || target instanceof HTMLElement && target.isContentEditable) {
				event.preventDefault();
				target.blur();
				return;
			}
			event.preventDefault();
			closePage();
		}
		window.addEventListener("keydown", onKeyDown);
		return () => window.removeEventListener("keydown", onKeyDown);
	}, [closePage]);
	const deleteArtifact = async (item) => {
		const requestedIdentity = accountIdentity;
		if (!requestedIdentity) return;
		const requestedAccountIsCurrent = () => artifactAccountIdentity(useAppStore.getState().orcaProfileAuthStatus) === requestedIdentity;
		const name = item.artifact.title || item.artifact.originalFileName || item.artifact.slug;
		if (!settings?.skipDeleteArtifactConfirm) {
			if (!await confirm({
				title: translate("auto.components.artifacts.ArtifactsPage.deleteTitle", "Delete artifact?"),
				description: translate("auto.components.artifacts.ArtifactsPage.deleteDescription", "“{{name}}” will no longer be available at its public link.", { name }),
				confirmLabel: translate("auto.components.artifacts.ArtifactsPage.delete", "Delete"),
				confirmVariant: "destructive",
				dontAskAgain: { onConfirmed: () => persistConfirmationSkipPreference({
					updates: { skipDeleteArtifactConfirm: true },
					settingsSectionId: "general-skip-delete-artifact-confirm",
					updateSettings,
					openSettingsPage,
					openSettingsTarget
				}) }
			})) return;
		}
		if (!requestedAccountIsCurrent()) return;
		setDeleting({
			identity: requestedIdentity,
			slug: item.artifact.slug
		});
		try {
			const result = await callRuntimeRpc(LOCAL_RUNTIME, "artifacts.delete", { id: item.artifact.slug });
			if (!requestedAccountIsCurrent()) return;
			if (result.status !== "ok") {
				await refreshAuth();
				throw new Error(result.status);
			}
			removeArtifact(requestedIdentity, item.artifact.slug);
		} catch (deleteError) {
			console.error("Failed to delete artifact:", deleteError);
			if (requestedAccountIsCurrent()) setError(translate("auto.components.artifacts.ArtifactsPage.deleteFailed", "Could not delete the artifact."));
		} finally {
			setDeleting((current) => current?.identity === requestedIdentity && current.slug === item.artifact.slug ? null : current);
		}
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("main", {
		className: "relative flex h-full min-h-0 flex-1 flex-col bg-background text-foreground",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("header", {
			className: "flex shrink-0 items-center justify-between px-5 pb-3 pt-1.5 md:px-8",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex min-w-0 items-center gap-2",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
						asChild: true,
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							variant: "ghost",
							size: "icon",
							className: "size-7 shrink-0 rounded-full",
							onClick: closePage,
							"aria-label": translate("auto.components.artifacts.ArtifactsPage.closeArtifacts", "Close artifacts"),
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "size-4" })
						})
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
						side: "bottom",
						sideOffset: 6,
						children: translate("auto.components.artifacts.ArtifactsPage.closeTooltip", "Close · Esc")
					})] }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "mx-1 h-5 w-px bg-border/50",
						"aria-hidden": true
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Files, { className: "size-4 shrink-0 text-muted-foreground" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "min-w-0",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h1", {
							className: "truncate text-sm font-semibold",
							children: translate("auto.components.artifacts.ArtifactsPage.title", "Artifacts")
						}), signedIn && artifacts.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "truncate text-xs text-muted-foreground",
							children: nextCursor ? translate("auto.components.artifacts.ArtifactsPage.loadedCountMore", "{{count}} loaded · more available", { count: artifacts.length }) : translate("auto.components.artifacts.ArtifactsPage.loadedCount", "{{count}} shared", { count: artifacts.length })
						}) : null]
					})
				]
			}), signedIn ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
				asChild: true,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					variant: "ghost",
					size: "icon-sm",
					className: "border border-border/50 bg-transparent hover:bg-muted/50",
					onClick: () => void loadArtifacts(),
					disabled: loading,
					"aria-label": translate("auto.components.artifacts.ArtifactsPage.refresh", "Refresh"),
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RefreshCw, { className: loading ? "animate-spin" : void 0 })
				})
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
				side: "bottom",
				sideOffset: 6,
				children: translate("auto.components.artifacts.ArtifactsPage.refresh", "Refresh")
			})] }) : null]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex min-h-0 w-full flex-1 flex-col border-t border-border/50",
			children: [error ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex flex-wrap items-center justify-between gap-3 border-b border-destructive/30 bg-destructive/10 px-5 py-2 md:px-8",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "min-w-0 flex-1 text-xs text-destructive",
					children: error
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					type: "button",
					variant: "outline",
					size: "xs",
					disabled: loading,
					onClick: () => void loadArtifacts(),
					children: translate("auto.components.artifacts.ArtifactsPage.retry", "Retry")
				})]
			}) : null, !signedIn ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex min-h-72 flex-1 flex-col items-center justify-center gap-3 px-5 py-5 text-center md:px-8",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Files, { className: "size-8 text-muted-foreground" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "space-y-1",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h2", {
							className: "text-sm font-semibold",
							children: needsReconnect ? translate("auto.components.artifacts.ArtifactsPage.reconnectHeading", "Sign in to Orca again") : translate("auto.components.artifacts.ArtifactsPage.signInHeading", "Sign in to share artifacts")
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "max-w-sm text-xs leading-5 text-muted-foreground",
							children: needsReconnect ? translate("auto.components.artifacts.ArtifactsPage.reconnectCopy", "Sign in again to view and manage the artifacts shared through your account.") : translate("auto.components.artifacts.ArtifactsPage.signInCopy", "Use your Orca account to upload artifacts and manage their public links.")
						})]
					}),
					authStatus?.configured === true ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						size: "sm",
						disabled: connecting,
						onClick: () => void connect(),
						children: connecting ? translate("auto.components.artifacts.ArtifactsPage.signingIn", "Signing in…") : needsReconnect ? translate("auto.components.artifacts.ArtifactsPage.signInAgainAction", "Sign in again") : translate("auto.components.artifacts.ArtifactsPage.signIn", "Sign in to Orca")
					}) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex flex-col items-center gap-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "max-w-sm text-xs leading-5 text-muted-foreground",
							children: translate("auto.components.artifacts.ArtifactsPage.unconfiguredCopy", "Orca account sign-in is not configured on this machine yet.")
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
							variant: "outline",
							size: "sm",
							onClick: openAccountSettings,
							children: [translate("auto.components.artifacts.ArtifactsPage.openAccountSettings", "Open account settings"), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowRight, {})]
						})]
					})
				]
			}) : loading && artifacts.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "flex min-h-72 flex-1 items-center justify-center",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-6 animate-spin text-muted-foreground" })
			}) : artifacts.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex flex-1 flex-col items-center justify-center gap-2 px-5 py-5 text-center md:px-8",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Files, { className: "size-8 text-muted-foreground" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h2", {
						className: "text-sm font-semibold",
						children: nextCursor ? translate("auto.components.artifacts.ArtifactsPage.moreAvailable", "More artifacts are available") : publishingBlocked ? translate("auto.components.artifacts.ArtifactsPage.publishingOff", "Publishing is turned off") : translate("auto.components.artifacts.ArtifactsPage.empty", "No shared artifacts")
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "max-w-sm text-xs leading-5 text-muted-foreground",
						children: nextCursor ? translate("auto.components.artifacts.ArtifactsPage.moreAvailableCopy", "Load the next page to continue.") : publishingBlocked ? translate("auto.components.artifacts.ArtifactsPage.publishingOffCopy", "Nothing on this device can create a public artifact link yet. Allow publishing in Settings → Artifacts, then share from an open HTML or Markdown file or ask your agent.") : translate("auto.components.artifacts.ArtifactsPage.emptyCopy", "Open an HTML or Markdown file and select Share as artifact, or ask your agent to share it.")
					}),
					!nextCursor && publishingBlocked ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						type: "button",
						variant: "outline",
						size: "sm",
						className: "mt-1",
						onClick: () => {
							openSettingsTarget({
								pane: "artifacts",
								repoId: null
							});
							openSettingsPage();
						},
						children: translate("auto.components.artifacts.ArtifactsPage.openArtifactsSettings", "Open Settings → Artifacts")
					}) : null,
					nextCursor ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
						type: "button",
						variant: "outline",
						size: "sm",
						className: "mt-1",
						disabled: loadingMore,
						onClick: () => void loadMoreArtifacts(),
						children: [loadingMore ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "animate-spin" }) : null, translate("auto.components.artifacts.ArtifactCollection.loadMore", "Load more")]
					}) : null
				]
			}) : selectedArtifact && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArtifactCollection, {
				artifacts,
				deletingId,
				selectedArtifact,
				selectArtifact: setSelectedSlug,
				deleteArtifact: (target) => void deleteArtifact(target),
				hasMore: Boolean(nextCursor),
				loadingMore,
				loadMore: () => void loadMoreArtifacts()
			})]
		})]
	});
}
export { ArtifactsPage as default };
