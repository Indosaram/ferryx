import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as Button } from "./button-DszXJEV6.js";
import { t as Check } from "./check-Lb2n4tDb.js";
import { t as ChevronRight } from "./chevron-right-CZtMe6Ev.js";
import { t as CircleAlert } from "./circle-alert-keRTpMg-.js";
import { t as LoaderCircle } from "./loader-circle-CRZpWdsi.js";
import { t as Minus } from "./minus-Byrkh1sN.js";
import { t as Network } from "./network-CVzMA1Cs.js";
import { t as RotateCw } from "./rotate-cw-DBuQrdY8.js";
import { t as ShieldAlert } from "./shield-alert-vhzQcWf9.js";
import { Fi as getReleaseNotesUrlForVersion, t as useAppStore } from "./store-CgXrfmaH.js";
import { t as X } from "./x-BrGKE4uz.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import { t as Progress } from "./progress-BsVdJvWF.js";
import "./useMountedRef-1omUd-IV.js";
import { t as Card } from "./card-qCUyfMIi.js";
import { t as usePrefersReducedMotion } from "./usePrefersReducedMotion-TEdW-TWP.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function ActionButton({ action, variant, leadingIcon }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
		variant,
		size: "sm",
		onClick: action.onClick,
		"aria-disabled": action.isPending || action.disabled,
		className: "flex-1 gap-1.5 aria-disabled:cursor-default aria-disabled:opacity-50",
		children: [action.isPending ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3.5 animate-spin" }) : leadingIcon, action.isPending && action.pendingLabel ? action.pendingLabel : action.label]
	});
}
function UpdateErrorCardContent({ variant = "default", title, summary, explainer, detail, releaseUrl, manualLabel, primaryAction, secondaryAction, tertiaryAction, footnote, onClose }) {
	const [showDetails, setShowDetails] = (0, import_react.useState)(false);
	const detailId = (0, import_react.useId)();
	const isCompatibility = variant === "http1Compatibility";
	const isSecurity = variant === "security";
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex flex-col gap-3 p-4",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-start gap-3",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: `mt-0.5 flex size-9 shrink-0 items-center justify-center rounded-md border bg-muted/50 ${isSecurity ? "border-destructive/30 text-destructive" : "border-border text-muted-foreground"}`,
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(isCompatibility ? Network : isSecurity ? ShieldAlert : CircleAlert, { className: "size-4" })
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "min-w-0 flex-1 space-y-1",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h3", {
							className: "text-sm font-semibold",
							children: title
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "text-sm leading-relaxed text-muted-foreground",
							children: summary
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						variant: "ghost",
						size: "icon",
						className: "shrink-0 min-w-[44px] min-h-[44px] -m-2",
						onClick: onClose,
						"aria-label": translate("auto.components.UpdateCard.8acbdd3961", "Minimize to status bar"),
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Minus, { className: "size-3.5" })
					})
				]
			}),
			explainer ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "rounded-md border border-border/70 bg-muted/30 px-3 py-2",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "text-xs leading-relaxed text-muted-foreground",
					children: explainer
				})
			}) : null,
			detail ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex flex-col gap-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
					type: "button",
					variant: "ghost",
					size: "xs",
					className: "-ml-2 self-start text-muted-foreground hover:text-foreground",
					onClick: () => setShowDetails((prev) => !prev),
					"aria-expanded": showDetails,
					"aria-controls": detailId,
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronRight, { className: `size-3.5 transition-transform motion-reduce:transition-none ${showDetails ? "rotate-90" : ""}` }), showDetails ? translate("auto.components.UpdateCard.5194358929", "Hide details") : translate("auto.components.UpdateCard.8bc9e17d8f", "Show details")]
				}), showDetails ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					id: detailId,
					className: "rounded-md bg-muted/40 px-3 py-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "mb-1 text-[11px] font-medium uppercase text-muted-foreground",
						children: translate("auto.components.UpdateCard.3553a8672f", "Last error")
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "scrollbar-sleek max-h-20 overflow-auto break-words font-mono text-xs leading-relaxed text-muted-foreground",
						children: detail
					})]
				}) : null]
			}) : null,
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex flex-col gap-2",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex gap-2",
						children: [
							primaryAction && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ActionButton, {
								action: primaryAction,
								variant: "default",
								leadingIcon: isCompatibility ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RotateCw, { className: "size-3.5" }) : void 0
							}),
							secondaryAction && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ActionButton, {
								action: secondaryAction,
								variant: "outline"
							}),
							releaseUrl && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
								variant: "outline",
								size: "sm",
								onClick: () => {
									window.api.shell.openUrl(releaseUrl).catch((error) => {
										console.error("[updates] failed to open the release page:", error);
									});
								},
								className: "flex-1",
								children: manualLabel ?? translate("auto.components.UpdateCard.47126bcf57", "Download Manually")
							})
						]
					}),
					tertiaryAction && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						type: "button",
						variant: "link",
						size: "xs",
						className: "-ml-1 min-h-[44px] self-start p-0 text-xs aria-disabled:cursor-default aria-disabled:opacity-50",
						onClick: tertiaryAction.onClick,
						"aria-disabled": tertiaryAction.isPending || tertiaryAction.disabled,
						children: tertiaryAction.isPending && tertiaryAction.pendingLabel ? tertiaryAction.pendingLabel : tertiaryAction.label
					}),
					footnote && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: `text-xs leading-relaxed ${footnote.tone === "destructive" ? "text-destructive" : "text-muted-foreground"}`,
						children: footnote.text
					})
				]
			})
		]
	});
}
var COPY_CONFIRMATION_MS = 4e3;
function copiedNote(packageFileName) {
	return translate("auto.components.LinuxPackageInstallRecoveryCard.aa57fa4f80", "Command copied. Run it in a system terminal to install {{value0}}, then quit and reopen Orca.", { value0: packageFileName });
}
function toMessage(error) {
	return String(error?.message ?? error).replace(/^Error invoking remote method '[^']*':\s*/, "").replace(/^Error:\s*/, "");
}
function LinuxPackageInstallRecoveryCard({ recovery, diagnostic, releaseUrl, onClose }) {
	const TITLE = translate("auto.components.LinuxPackageInstallRecoveryCard.53e1559f99", "Automatic Install Failed");
	const SUMMARY = translate("auto.components.LinuxPackageInstallRecoveryCard.a7ac6ec78b", "Orca downloaded the update but could not install the system package automatically.");
	const EXPLAINER = translate("auto.components.LinuxPackageInstallRecoveryCard.82c6dbea00", "Copy the command and run it in a system terminal on the computer where Orca is installed. After it finishes, quit and reopen Orca to run the new version.");
	const AGENT_NOTE = translate("auto.components.LinuxPackageInstallRecoveryCard.53c4b8e148", "No usable authentication agent answered the privileged install request.");
	const TRUST_NOTE = translate("auto.components.LinuxPackageInstallRecoveryCard.b7e7c5bc95", "Orca checks the downloaded file against the release metadata at the moment it builds this command. The system package itself is not signature-checked, and Orca cannot vouch for the file after that point.");
	const CHECKING_LABEL = translate("auto.components.LinuxPackageInstallRecoveryCard.c732bcbf8f", "Checking package...");
	const [pendingAction, setPendingAction] = (0, import_react.useState)(null);
	const [actionError, setActionError] = (0, import_react.useState)(null);
	const [copiedFileName, setCopiedFileName] = (0, import_react.useState)(null);
	const [commandUnavailable, setCommandUnavailable] = (0, import_react.useState)(false);
	const mountedRef = (0, import_react.useRef)(true);
	(0, import_react.useEffect)(() => {
		mountedRef.current = true;
		return () => {
			mountedRef.current = false;
		};
	}, []);
	(0, import_react.useEffect)(() => {
		setPendingAction((current) => current === "retry" ? null : current);
	}, [recovery]);
	(0, import_react.useEffect)(() => {
		if (!copiedFileName) return;
		const timer = window.setTimeout(() => setCopiedFileName(null), COPY_CONFIRMATION_MS);
		return () => window.clearTimeout(timer);
	}, [copiedFileName]);
	const handleCopyCommand = (0, import_react.useCallback)(() => {
		if (pendingAction) return;
		setPendingAction("copy");
		setActionError(null);
		setCopiedFileName(null);
		(async () => {
			let instructions;
			try {
				instructions = await window.api.updater.getLinuxPackageInstallInstructions();
			} catch (error) {
				if (mountedRef.current) setActionError(toMessage(error));
				return;
			}
			if (!instructions.ok) {
				if (mountedRef.current) {
					setCommandUnavailable(true);
					setActionError(instructions.message);
				}
				return;
			}
			try {
				await window.api.ui.writeClipboardText(instructions.command);
				if (mountedRef.current) setCopiedFileName(instructions.packageFileName);
			} catch (error) {
				if (mountedRef.current) setActionError(toMessage(error));
			}
		})().finally(() => {
			if (mountedRef.current) setPendingAction(null);
		});
	}, [pendingAction]);
	const handleShowPackage = (0, import_react.useCallback)(() => {
		if (pendingAction) return;
		setPendingAction("show");
		setActionError(null);
		setCopiedFileName(null);
		window.api.updater.showLinuxPackage().catch((error) => {
			if (mountedRef.current) setActionError(toMessage(error));
		}).finally(() => {
			if (mountedRef.current) setPendingAction(null);
		});
	}, [pendingAction]);
	const handleRetryAutomatic = (0, import_react.useCallback)(() => {
		if (pendingAction) return;
		setPendingAction("retry");
		setActionError(null);
		setCopiedFileName(null);
		setCommandUnavailable(false);
		window.api.updater.quitAndInstall().catch((error) => {
			if (mountedRef.current) {
				setActionError(toMessage(error));
				setPendingAction(null);
			}
		});
	}, [pendingAction]);
	const copyAction = {
		label: translate("auto.components.LinuxPackageInstallRecoveryCard.55c86654b7", "Copy Install Command"),
		pendingLabel: CHECKING_LABEL,
		isPending: pendingAction === "copy",
		disabled: pendingAction !== null,
		onClick: handleCopyCommand
	};
	const showAction = {
		label: translate("auto.components.LinuxPackageInstallRecoveryCard.e3de29c86a", "Show Package"),
		pendingLabel: CHECKING_LABEL,
		isPending: pendingAction === "show",
		disabled: pendingAction !== null,
		onClick: handleShowPackage
	};
	const retryAction = {
		label: translate("auto.components.LinuxPackageInstallRecoveryCard.3da99454c6", "Try Automatic Install Again"),
		pendingLabel: CHECKING_LABEL,
		isPending: pendingAction === "retry",
		disabled: pendingAction !== null,
		onClick: handleRetryAutomatic
	};
	const officialReleaseAction = releaseUrl ? {
		label: translate("auto.components.UpdateCard.47126bcf57", "Download Manually"),
		onClick: () => void window.api.shell.openUrl(releaseUrl)
	} : void 0;
	const detail = [
		recovery.reason === "authentication-agent-unavailable" ? AGENT_NOTE : null,
		diagnostic,
		TRUST_NOTE
	].filter(Boolean).join(" ");
	const footnote = actionError ? {
		text: actionError,
		tone: "destructive"
	} : copiedFileName ? { text: copiedNote(copiedFileName) } : void 0;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(UpdateErrorCardContent, {
		title: TITLE,
		summary: SUMMARY,
		explainer: commandUnavailable ? void 0 : EXPLAINER,
		detail,
		primaryAction: commandUnavailable ? showAction : copyAction,
		secondaryAction: retryAction,
		tertiaryAction: commandUnavailable ? officialReleaseAction : showAction,
		footnote,
		onClose
	});
}
function isWindowsSignatureCheckUnavailableFailure(message) {
	const normalized = message.toLowerCase();
	if (normalized.includes("not signed by the application owner")) return false;
	return normalized.includes("get-authenticodesignature");
}
function isWindowsSignatureMismatchFailure(message) {
	return message.toLowerCase().includes("not signed by the application owner");
}
function isAnimatedGif(url) {
	return typeof url === "string" && url.toLowerCase().endsWith(".gif");
}
function isHttp2ProtocolError(message) {
	const normalized = message.toLowerCase();
	return normalized.includes("err_http2_protocol_error") || normalized.includes("http2_protocol_error") || normalized.includes("http/2") && normalized.includes("protocol");
}
function CompactCardContent({ icon, text, onClose, action }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex items-center gap-3 p-3",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "shrink-0 text-muted-foreground",
				children: [
					icon === "spinner" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-4 animate-spin" }),
					icon === "check" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, { className: "size-4" }),
					icon === "error" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CircleAlert, { className: "size-4" })
				]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex-1 min-w-0",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "text-sm truncate",
					children: text
				}), action && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
					className: "text-xs text-muted-foreground underline hover:text-foreground mt-0.5",
					onClick: () => void window.api.shell.openUrl(action.url),
					children: action.label
				})]
			}),
			onClose && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				variant: "ghost",
				size: "icon",
				className: "size-7 shrink-0",
				onClick: onClose,
				"aria-label": translate("auto.components.UpdateCard.a726967bd3", "Dismiss"),
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "size-3.5" })
			})
		]
	});
}
function UpdateCard() {
	const status = useAppStore((s) => s.updateStatus);
	const storeChangelog = useAppStore((s) => s.updateChangelog);
	const updateUserInitiatedCycle = useAppStore((s) => s.updateUserInitiatedCycle);
	const dismissedVersion = useAppStore((s) => s.dismissedUpdateVersion);
	const dismissUpdate = useAppStore((s) => s.dismissUpdate);
	const collapsed = useAppStore((s) => s.updateCardCollapsed);
	const setCollapsed = useAppStore((s) => s.setUpdateCardCollapsed);
	const reassuranceSeen = useAppStore((s) => s.updateReassuranceSeen);
	const markReassuranceSeen = useAppStore((s) => s.markUpdateReassuranceSeen);
	const hasStartedDownload = (0, import_react.useRef)(false);
	const dismissAnimationTimerRef = (0, import_react.useRef)(null);
	const collapseAnimationTimerRef = (0, import_react.useRef)(null);
	const [mediaFailed, setMediaFailed] = (0, import_react.useState)(false);
	const [mediaLoaded, setMediaLoaded] = (0, import_react.useState)(false);
	const [installError, setInstallError] = (0, import_react.useState)(null);
	const [compatibilityRelaunching, setCompatibilityRelaunching] = (0, import_react.useState)(false);
	const [compatibilitySetupError, setCompatibilitySetupError] = (0, import_react.useState)(null);
	const [errorDismissed, setErrorDismissed] = (0, import_react.useState)(false);
	const [autoDismissed, setAutoDismissed] = (0, import_react.useState)(false);
	const [exiting, setExiting] = (0, import_react.useState)(false);
	const changelog = storeChangelog;
	const isLocalBuild = status.source === "local";
	const versionRef = (0, import_react.useRef)(null);
	if ("version" in status && status.version) versionRef.current = status.version;
	else if (status.state === "checking" || status.state === "idle" || status.state === "not-available") versionRef.current = null;
	const prevVersionRef = (0, import_react.useRef)(null);
	if (status.state === "available" && status.version !== prevVersionRef.current) {
		prevVersionRef.current = status.version;
		hasStartedDownload.current = false;
		setMediaFailed(false);
		setMediaLoaded(false);
		setInstallError(null);
	}
	const prevStateRef = (0, import_react.useRef)(status.state);
	if (status.state !== prevStateRef.current) {
		prevStateRef.current = status.state;
		if (autoDismissed) setAutoDismissed(false);
		if (exiting) setExiting(false);
		if (errorDismissed) setErrorDismissed(false);
	}
	const shouldAutoDismissLatest = status.state === "not-available" && "userInitiated" in status && Boolean(status.userInitiated);
	(0, import_react.useEffect)(() => {
		if (!shouldAutoDismissLatest) return;
		const timer = setTimeout(() => setAutoDismissed(true), 3e3);
		return () => clearTimeout(timer);
	}, [shouldAutoDismissLatest]);
	(0, import_react.useEffect)(() => {
		if (status.state === "downloaded" && hasStartedDownload.current) window.api.updater.quitAndInstall().catch((error) => {
			setInstallError(String(error?.message ?? error));
		});
	}, [status.state]);
	const prefersReducedMotion = usePrefersReducedMotion();
	const clearAnimationTimers = (0, import_react.useCallback)(() => {
		if (dismissAnimationTimerRef.current !== null) {
			window.clearTimeout(dismissAnimationTimerRef.current);
			dismissAnimationTimerRef.current = null;
		}
		if (collapseAnimationTimerRef.current !== null) {
			window.clearTimeout(collapseAnimationTimerRef.current);
			collapseAnimationTimerRef.current = null;
		}
	}, []);
	const cardRootRef = (0, import_react.useCallback)((node) => {
		if (node !== null) return;
		clearAnimationTimers();
	}, [clearAnimationTimers]);
	const isUserInitiated = "userInitiated" in status && status.userInitiated;
	const cachedVersion = versionRef.current;
	const shouldShowDetailedErrorCard = status.state === "error" && (hasStartedDownload.current || cachedVersion !== null);
	if (status.state === "checking" && !isUserInitiated) return null;
	if (status.state === "not-available" && !isUserInitiated) return null;
	if (status.state === "not-available" && autoDismissed) return null;
	if (status.state === "idle") return null;
	if (status.state === "error" && !shouldShowDetailedErrorCard && !isUserInitiated) return null;
	if (status.state === "error" && errorDismissed) return null;
	if (versionRef.current && dismissedVersion === versionRef.current && !updateUserInitiatedCycle) {
		if (status.state !== "downloading" && status.state !== "error") return null;
	}
	if (collapsed && (status.state === "downloading" || status.state === "downloaded" || status.state === "error")) return null;
	const isRichMode = changelog?.release != null;
	const handleUpdate = () => {
		hasStartedDownload.current = true;
		if (!reassuranceSeen) markReassuranceSeen();
		window.api.updater.download();
	};
	const handleClose = () => {
		if (status.state === "error") {
			setErrorDismissed(true);
			if (cachedVersion) dismissUpdate(cachedVersion);
			return;
		}
		dismissUpdate();
	};
	const handleInstallRetry = () => {
		window.api.updater.quitAndInstall().catch((error) => {
			setInstallError(String(error?.message ?? error));
		});
	};
	const handleEnableHttp1Compatibility = () => {
		if (compatibilityRelaunching) return;
		setCompatibilityRelaunching(true);
		setCompatibilitySetupError(null);
		window.api.settings.set({ electronHttp1CompatibilityMode: true }).then(() => window.api.app.relaunch()).catch((error) => {
			const message = String(error?.message ?? error);
			console.error("[updates] failed to enable HTTP/1.1 compatibility:", error);
			setCompatibilitySetupError(`Could not enable compatibility mode. ${message}`);
			setCompatibilityRelaunching(false);
		});
	};
	const isHttp2UpdateError = status.state === "error" && isHttp2ProtocolError(status.message);
	const isSignatureMismatchError = status.state === "error" && isWindowsSignatureMismatchFailure(status.message);
	const isSignatureCheckBlockedError = status.state === "error" && isWindowsSignatureCheckUnavailableFailure(status.message);
	const linuxPackageRecovery = status.state === "error" && status.recovery?.kind === "linux-package-install" ? {
		recovery: status.recovery,
		diagnostic: status.message
	} : null;
	const errorCard = status.state === "error" ? isLocalBuild ? {
		title: cachedVersion ? translate("auto.components.UpdateCard.8cf17b10af", "Local Build Error") : translate("auto.components.UpdateCard.a4650b0dc4", "Could Not Use Local Build"),
		summary: cachedVersion ? translate("auto.components.UpdateCard.b1e390250d", "Could not complete the local build switch.") : translate("auto.components.UpdateCard.d29740d175", "The selected build could not be used."),
		detail: status.message,
		primaryAction: {
			label: translate("auto.components.UpdateCard.37d45c9ec1", "Choose Another Build"),
			onClick: () => {
				window.api.updater.check({ localBuild: true });
			}
		}
	} : isHttp2UpdateError ? {
		variant: "http1Compatibility",
		title: translate("auto.components.UpdateCard.1339b82cee", "HTTP/2 Download Blocked"),
		summary: "Orca can retry through HTTP/1.1 compatibility mode.",
		explainer: translate("auto.components.UpdateCard.90559b14e3", "This turns on a process-wide Electron networking switch after restart. Use it for corporate VPNs or proxies that reject HTTP/2 update downloads."),
		detail: compatibilitySetupError ?? status.message,
		releaseUrl: getReleaseNotesUrlForVersion(cachedVersion),
		primaryAction: {
			label: translate("auto.components.UpdateCard.933c6fdf5b", "Enable & Restart"),
			pendingLabel: "Restarting...",
			isPending: compatibilityRelaunching,
			onClick: handleEnableHttp1Compatibility
		}
	} : isSignatureMismatchError ? {
		variant: "security",
		title: translate("auto.components.UpdateCard.5b309b19f3", "Update Wasn't Installed"),
		summary: translate("auto.components.UpdateCard.092f09fc14", "The installer's publisher doesn't match Orca, so we stopped the update. Don't install this download; check official releases for a corrected version."),
		detail: status.message,
		releaseUrl: getReleaseNotesUrlForVersion(null),
		manualLabel: translate("auto.components.UpdateCard.c9ff9b9ec2", "Check official releases")
	} : isSignatureCheckBlockedError ? {
		title: translate("auto.components.UpdateCard.e944c2de43", "Update Verification Blocked"),
		summary: translate("auto.components.UpdateCard.a05992a26b", "The signature check couldn't run — usually because antivirus software blocked it. Retry the download, or get the installer from our official releases."),
		detail: status.message,
		releaseUrl: getReleaseNotesUrlForVersion(cachedVersion),
		primaryAction: {
			label: translate("auto.components.UpdateCard.48565a32bc", "Retry Download"),
			onClick: handleUpdate
		}
	} : {
		title: cachedVersion ? "Update Error" : "Update Check Failed",
		summary: cachedVersion ? "Could not complete the update." : "Could not check for updates.",
		detail: status.message,
		releaseUrl: getReleaseNotesUrlForVersion(cachedVersion),
		primaryAction: cachedVersion ? {
			label: translate("auto.components.UpdateCard.48565a32bc", "Retry Download"),
			onClick: handleUpdate
		} : {
			label: translate("auto.components.UpdateCard.6b0085010d", "Re-check"),
			onClick: () => {
				window.api.updater.check({ includePrerelease: false });
			}
		}
	} : installError ? {
		title: translate("auto.components.UpdateCard.4cf109845a", "Update Error"),
		summary: "Could not restart to install the update.",
		detail: installError,
		releaseUrl: getReleaseNotesUrlForVersion(cachedVersion),
		primaryAction: {
			label: translate("auto.components.UpdateCard.2c2d3e03ca", "Try Again"),
			onClick: handleInstallRetry
		}
	} : null;
	const handleDismissWithAnimation = () => {
		if (prefersReducedMotion) {
			handleClose();
			return;
		}
		setExiting(true);
		if (dismissAnimationTimerRef.current !== null) window.clearTimeout(dismissAnimationTimerRef.current);
		dismissAnimationTimerRef.current = window.setTimeout(() => {
			dismissAnimationTimerRef.current = null;
			handleClose();
		}, 150);
	};
	const handleCollapseWithAnimation = () => {
		if (prefersReducedMotion) {
			setCollapsed(true);
			return;
		}
		setExiting(true);
		if (collapseAnimationTimerRef.current !== null) window.clearTimeout(collapseAnimationTimerRef.current);
		collapseAnimationTimerRef.current = window.setTimeout(() => {
			collapseAnimationTimerRef.current = null;
			setCollapsed(true);
			setExiting(false);
		}, 150);
	};
	const handleKeyDown = (e) => {
		if (e.key !== "Escape") return;
		e.preventDefault();
		if (status.state === "downloading" || status.state === "downloaded" || status.state === "error") handleCollapseWithAnimation();
		else handleDismissWithAnimation();
	};
	const ariaLabel = status.state === "checking" ? "Checking for updates" : status.state === "not-available" ? "You're on the latest version" : status.state === "available" ? "Update available" : status.state === "downloading" ? "Downloading update" : status.state === "downloaded" ? "Update ready to install" : status.state === "error" ? "Update error" : "Update status";
	const animationClass = prefersReducedMotion ? "" : exiting ? "animate-update-card-exit" : "animate-update-card-enter";
	const cardContent = (() => {
		if (status.state === "checking") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CompactCardContent, {
			icon: "spinner",
			text: translate("auto.components.UpdateCard.ba5ffc949c", "Checking for updates...")
		});
		if (status.state === "not-available") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CompactCardContent, {
			icon: "check",
			text: translate("auto.components.UpdateCard.ea2a41adbe", "You're on the latest version.")
		});
		if (linuxPackageRecovery) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LinuxPackageInstallRecoveryCard, {
			recovery: linuxPackageRecovery.recovery,
			diagnostic: linuxPackageRecovery.diagnostic,
			releaseUrl: isLocalBuild ? void 0 : getReleaseNotesUrlForVersion(cachedVersion),
			onClose: handleCollapseWithAnimation
		});
		if (errorCard) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(UpdateErrorCardContent, {
			...errorCard,
			onClose: handleCollapseWithAnimation
		});
		if (status.state === "downloaded") {
			if (hasStartedDownload.current) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "p-4",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "text-sm",
					children: translate("auto.components.UpdateCard.09a55c39b5", "Installing...")
				})
			});
			return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ReadyToInstallContent, {
				version: status.version,
				onRestart: handleInstallRetry,
				onClose: handleCollapseWithAnimation
			});
		}
		if (status.state === "downloading") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DownloadingContent, {
			version: status.version,
			percent: status.percent,
			changelog,
			prefersReducedMotion,
			mediaFailed,
			mediaLoaded,
			onMediaError: () => setMediaFailed(true),
			onMediaLoad: () => setMediaLoaded(true),
			onCollapse: handleCollapseWithAnimation,
			showReleaseNotes: !isLocalBuild
		});
		if (status.state !== "available") return null;
		const releaseUrl = isLocalBuild ? void 0 : ("releaseUrl" in status ? status.releaseUrl : void 0) ?? getReleaseNotesUrlForVersion(status.version);
		if (isRichMode && changelog) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RichCardContent, {
			release: changelog.release,
			releasesBehind: changelog.releasesBehind,
			prefersReducedMotion,
			mediaFailed,
			mediaLoaded,
			onMediaError: () => setMediaFailed(true),
			onMediaLoad: () => setMediaLoaded(true),
			onUpdate: handleUpdate,
			onClose: handleDismissWithAnimation
		});
		return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SimpleCardContent, {
			version: status.version,
			releaseUrl,
			onUpdate: handleUpdate,
			onClose: handleDismissWithAnimation
		});
	})();
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		ref: cardRootRef,
		className: "fixed bottom-10 right-4 z-40 w-[360px] max-w-[calc(100vw-32px)] flex flex-col gap-2\n      max-[480px]:left-4 max-[480px]:right-4 max-[480px]:w-auto",
		children: [!reassuranceSeen && (status.state === "available" || status.state === "downloading") && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, {
			className: `py-0 gap-0 ${animationClass}`,
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-center gap-3 p-3",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "flex-1 min-w-0",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "text-xs text-muted-foreground",
						children: translate("auto.components.UpdateCard.b1d867f4fb", "Your terminal sessions won't be interrupted during the update.")
					})
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					variant: "ghost",
					size: "icon",
					className: "size-7 shrink-0",
					onClick: markReassuranceSeen,
					"aria-label": translate("auto.components.UpdateCard.7274ef6e59", "Dismiss tip"),
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "size-3.5" })
				})]
			})
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, {
			role: "complementary",
			"aria-label": ariaLabel,
			"aria-live": "polite",
			tabIndex: -1,
			onKeyDown: handleKeyDown,
			className: `py-0 gap-0 ${animationClass}`,
			children: cardContent
		})]
	});
}
function RichCardContent({ release, releasesBehind, prefersReducedMotion, mediaFailed, mediaLoaded, onMediaError, onMediaLoad, onUpdate, onClose }) {
	const showMedia = release.mediaUrl && !mediaFailed && !(prefersReducedMotion && isAnimatedGif(release.mediaUrl));
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex flex-col gap-3 p-4",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-start justify-between gap-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("h3", {
					className: "text-sm font-semibold",
					children: [
						translate("auto.components.UpdateCard.f58b5c57a6", "New:"),
						" ",
						release.title
					]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					variant: "ghost",
					size: "icon",
					className: "size-7 shrink-0 min-w-[44px] min-h-[44px] -m-2",
					onClick: onClose,
					"aria-label": translate("auto.components.UpdateCard.318d3b4bc7", "Dismiss update"),
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "size-3.5" })
				})]
			}),
			showMedia && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "relative overflow-hidden rounded-md",
				children: [!mediaLoaded && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "w-full bg-muted/50 animate-pulse rounded-md",
					style: { aspectRatio: "16/9" }
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("img", {
					src: release.mediaUrl,
					alt: "",
					className: `w-full rounded-md ${mediaLoaded ? "" : "absolute inset-0"}`,
					style: !mediaLoaded ? { visibility: "hidden" } : void 0,
					onError: onMediaError,
					onLoad: onMediaLoad
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
				className: "text-sm text-muted-foreground",
				children: [release.description, releasesBehind !== null && releasesBehind > 1 && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [" ", /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
					className: "text-xs text-muted-foreground/70 underline hover:text-foreground inline",
					onClick: () => void window.api.shell.openUrl(release.releaseNotesUrl),
					children: [
						"+",
						releasesBehind - 1,
						" ",
						translate("auto.components.UpdateCard.ccd8b0a793", "more since your last update")
					]
				})] })]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				className: "text-xs text-muted-foreground underline hover:text-foreground self-start",
				onClick: () => void window.api.shell.openUrl(release.releaseNotesUrl),
				children: translate("auto.components.UpdateCard.aad383aecc", "Read the full release notes")
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				variant: "default",
				size: "sm",
				onClick: onUpdate,
				className: "w-full cursor-pointer",
				children: translate("auto.components.UpdateCard.ec8fe71cfc", "Update")
			})
		]
	});
}
function SimpleCardContent({ version, releaseUrl, onUpdate, onClose }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex flex-col gap-2.5 p-3.5",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-start justify-between gap-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h3", {
					className: "text-sm font-semibold",
					children: translate("auto.components.UpdateCard.9abc59f814", "Update Available")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					variant: "ghost",
					size: "icon",
					className: "size-7 shrink-0 min-w-[44px] min-h-[44px] -m-2",
					onClick: onClose,
					"aria-label": translate("auto.components.UpdateCard.318d3b4bc7", "Dismiss update"),
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "size-3.5" })
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				className: "text-sm text-muted-foreground",
				children: translate("auto.components.UpdateCard.05ad78a6d1", "Orca v{{value0}} is ready.", { value0: version })
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				className: "text-xs leading-relaxed text-muted-foreground",
				children: translate("auto.components.UpdateCard.fdd4a364fa", "Sessions won't be interrupted.")
			}),
			releaseUrl && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				className: "text-xs text-muted-foreground underline underline-offset-2 hover:text-foreground self-start",
				onClick: () => void window.api.shell.openUrl(releaseUrl),
				children: translate("auto.components.UpdateCard.44324ef542", "Release notes")
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				variant: "default",
				size: "sm",
				onClick: onUpdate,
				className: "mt-0.5 w-full cursor-pointer",
				children: translate("auto.components.UpdateCard.ec8fe71cfc", "Update")
			})
		]
	});
}
function DownloadingContent({ version, percent, changelog, prefersReducedMotion, mediaFailed, mediaLoaded, onMediaError, onMediaLoad, onCollapse, showReleaseNotes }) {
	const release = changelog?.release;
	const showMedia = release?.mediaUrl && !mediaFailed && !(prefersReducedMotion && isAnimatedGif(release.mediaUrl));
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex flex-col gap-3 p-4",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-start justify-between gap-2",
				children: [release ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("h3", {
					className: "text-sm font-semibold",
					children: [
						translate("auto.components.UpdateCard.f58b5c57a6", "New:"),
						" ",
						release.title
					]
				}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("h3", {
					className: "text-sm font-semibold",
					children: translate("auto.components.UpdateCard.558842597d", "Downloading Update")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					variant: "ghost",
					size: "icon",
					className: "size-7 shrink-0 min-w-[44px] min-h-[44px] -m-2",
					onClick: onCollapse,
					"aria-label": translate("auto.components.UpdateCard.8acbdd3961", "Minimize to status bar"),
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Minus, { className: "size-3.5" })
				})]
			}),
			showMedia && release?.mediaUrl && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "relative overflow-hidden rounded-md",
				children: [!mediaLoaded && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "w-full bg-muted/50 animate-pulse rounded-md",
					style: { aspectRatio: "16/9" }
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("img", {
					src: release.mediaUrl,
					alt: "",
					className: `w-full rounded-md ${mediaLoaded ? "" : "absolute inset-0"}`,
					style: !mediaLoaded ? { visibility: "hidden" } : void 0,
					onError: onMediaError,
					onLoad: onMediaLoad
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				className: "text-sm text-muted-foreground",
				children: release ? release.description : translate("auto.components.UpdateCard.93794ea932", "Orca v{{value0}} is downloading.", { value0: version })
			}),
			showReleaseNotes && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				className: "text-xs text-muted-foreground underline hover:text-foreground self-start",
				onClick: () => void window.api.shell.openUrl(release ? release.releaseNotesUrl : getReleaseNotesUrlForVersion(version)),
				children: release ? translate("auto.components.UpdateCard.aad383aecc", "Read the full release notes") : translate("auto.components.UpdateCard.44324ef542", "Release notes")
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex flex-col gap-2 mt-1",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Progress, {
					value: percent,
					className: "h-1.5"
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
					className: "text-xs text-muted-foreground",
					children: [
						translate("auto.components.UpdateCard.6e45bfa2e0", "Downloading..."),
						" ",
						percent,
						"%"
					]
				})]
			})
		]
	});
}
function ReadyToInstallContent({ version, onRestart, onClose }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex flex-col gap-3 p-4",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-start justify-between gap-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h3", {
					className: "text-sm font-semibold",
					children: translate("auto.components.UpdateCard.17412483da", "Ready to Install")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					variant: "ghost",
					size: "icon",
					className: "size-7 shrink-0 min-w-[44px] min-h-[44px] -m-2",
					onClick: onClose,
					"aria-label": translate("auto.components.UpdateCard.8acbdd3961", "Minimize to status bar"),
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Minus, { className: "size-3.5" })
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				className: "text-sm text-muted-foreground",
				children: translate("auto.components.UpdateCard.6714206e5a", "Orca v{{value0}} is downloaded. Restart when you're ready.", { value0: version })
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				variant: "default",
				size: "sm",
				onClick: onRestart,
				className: "w-full",
				children: translate("auto.components.UpdateCard.68b235d264", "Restart to Update")
			})
		]
	});
}
export { UpdateCard, isHttp2ProtocolError };
