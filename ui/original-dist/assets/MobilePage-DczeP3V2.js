import { o as __toESM, t as __commonJSMin } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn, t as Button } from "./button-DszXJEV6.js";
import { t as ArrowLeft } from "./arrow-left-BpDalf_n.js";
import { t as ArrowRight } from "./arrow-right-ct5UxmKv.js";
import { t as ChevronDown } from "./chevron-down-BRkP96Md.js";
import { t as CircleAlert } from "./circle-alert-keRTpMg-.js";
import { t as Copy } from "./copy-jk2iqVkp.js";
import { t as RefreshCw } from "./refresh-cw-BU_ChOig.js";
import { t as Smartphone } from "./smartphone-BbD-8pvm.js";
import { m_ as Trash2, t as useAppStore } from "./store-CgXrfmaH.js";
import { t as X } from "./x-BrGKE4uz.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import { n as toast } from "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import "./label-D-n9s_wS.js";
import "./popover-CgR1mzy7.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
import { t as useMountedRef } from "./useMountedRef-1omUd-IV.js";
import "./badge-BBptl5GG.js";
import "./command-D8Tw17HJ.js";
import { n as replacePairedMobileDevices, r as usePairedMobileDevices, t as getPairedMobileDevicesSnapshot } from "./paired-mobile-devices-jlj8Z3db.js";
import "./dialog-BbelfMSB.js";
import "./input-DV5rpysh.js";
import { a as OpenAIIcon, t as ClaudeIcon } from "./icons-jFAuHbv9.js";
import { n as CollapsibleContent, r as CollapsibleTrigger, t as Collapsible } from "./collapsible-raq6sIQA.js";
import { a as WindowsFirewallNotice, c as MobilePairingConnectionOptions, d as AndroidLogo, f as IosBrandIcon, i as canMintMobilePairingOffer, l as NetworkInterfacePicker, n as useMobilePairingDevicePolling, o as MobileRelayMintFailureNotice, r as useMobilePairingConnectionMode, s as MobileRelayBetaNotice, t as useMobilePairingAddressPreference } from "./use-mobile-pairing-address-preference-Bvq_cjm8.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
function isEditableElement(target) {
	return target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement || target instanceof HTMLElement && target.isContentEditable;
}
function useMobilePageEscape(onClose) {
	(0, import_react.useEffect)(() => {
		function onKeyDown(event) {
			if (event.key !== "Escape" || event.defaultPrevented) return;
			const target = event.target;
			if (!(target instanceof HTMLElement)) return;
			if (isEditableElement(target)) {
				event.preventDefault();
				target.blur();
				return;
			}
			event.preventDefault();
			onClose();
		}
		window.addEventListener("keydown", onKeyDown);
		return () => window.removeEventListener("keydown", onKeyDown);
	}, [onClose]);
}
var IOS_CHANNEL_COPY = {
	stable: {
		ctaLabel: "Open App Store",
		url: "https://apps.apple.com/app/orca-ide/id6766130217"
	},
	preview: {
		ctaLabel: "Open TestFlight",
		url: "https://testflight.apple.com/join/YjeGMQBA"
	}
};
var ANDROID_COPY = {
	ctaLabel: "Download APK",
	url: "https://github.com/stablyai/orca/releases/download/mobile-android-v0.0.37/app-release.apk"
};
function getInstallCopy(platform, iosChannel) {
	return platform === "ios" ? IOS_CHANNEL_COPY[iosChannel] : ANDROID_COPY;
}
function getChannelTagline(iosChannel) {
	return iosChannel === "preview" ? translate("auto.components.mobile.mobile.platform.copy.preview.tagline", "Newest features, updated daily.") : translate("auto.components.mobile.mobile.platform.copy.stable.tagline", "The public release, updated weekly.");
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function pairDeviceHeading() {
	const ua = navigator.userAgent;
	if (ua.includes("Mac")) return translate("auto.components.mobile.MobileHero.pairThisMac", "Pair this Mac.");
	if (ua.includes("Windows")) return translate("auto.components.mobile.MobileHero.pairThisPc", "Pair this PC.");
	return translate("auto.components.mobile.MobileHero.pairThisComputer", "Pair this computer.");
}
function emptyPairingQrMessage(args) {
	if (args.relayMintFailure != null) return translate("auto.components.mobile.MobileHero.noRelayCode", "No pairing code available");
	if (!args.canGeneratePairing && args.connectionMode === "automatic") return translate("auto.components.mobile.MobileHero.qrSignInRequired", "Sign in to create a Relay pairing code");
	if (args.pairingQrError && args.pairingUrl != null) return translate("auto.components.mobile.MobileHero.qrRenderFailed", "QR couldn’t be rendered — copy the code below");
	if (!args.canGeneratePairing) return translate("auto.components.mobile.MobileHero.noPairingCode", "No pairing code available");
	return translate("auto.components.mobile.MobileHero.qrGeneratePrompt", "Generate a pairing code to continue");
}
function MobileHeroPairingStep({ pairQrDataUrl, pairingUrl, pairingQrError, relayMintFailure, onUseLan, onRetryRelay, onCopyRelayDiagnostics, pairLoading, connectionMode, onConnectionModeChange, onRegeneratePairing, canGeneratePairing, onCopyPairingCode, networkInterfaces, customAddresses, selectedAddress, selectedAddressIsCustom, onSelectedAddressChange, onCustomAddressSelect, onCustomAddressRemove, beforeCustomAddressChange, onRefreshNetworkInterfaces, refreshingNetworkInterfaces }) {
	const copyPairingCodeRef = (0, import_react.useRef)(null);
	const pairingWasReadyRef = (0, import_react.useRef)(pairingUrl != null && !pairLoading);
	const usingRelay = connectionMode === "automatic";
	const [networkDisclosureOpen, setNetworkDisclosureOpen] = (0, import_react.useState)(false);
	const networkDisclosurePinned = selectedAddressIsCustom;
	const emptyQrMessage = !pairLoading && pairQrDataUrl == null ? emptyPairingQrMessage({
		relayMintFailure,
		canGeneratePairing,
		connectionMode,
		pairingQrError,
		pairingUrl
	}) : null;
	(0, import_react.useEffect)(() => {
		const pairingReady = pairingUrl != null && !pairLoading;
		const becameReady = !pairingWasReadyRef.current && pairingReady;
		pairingWasReadyRef.current = pairingReady;
		if (becameReady && document.activeElement === document.body) copyPairingCodeRef.current?.focus();
	}, [pairLoading, pairingUrl]);
	const networkRow = /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "mp-network-row",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "mp-network-label",
				children: translate("auto.components.mobile.MobileHero.dfd2aa9d5d", "Network")
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(NetworkInterfacePicker, {
				networkInterfaces,
				customAddresses,
				selectedAddress,
				selectedAddressIsCustom,
				onSelectedAddressChange,
				onCustomAddressSelect,
				onCustomAddressRemove,
				beforeCustomAddressChange,
				disabled: false,
				className: "mp-network-select"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				type: "button",
				className: cn("mp-network-refresh", refreshingNetworkInterfaces && "is-spinning"),
				onClick: onRefreshNetworkInterfaces,
				disabled: refreshingNetworkInterfaces,
				"aria-label": translate("auto.components.mobile.MobileHero.85067b9e06", "Refresh network interfaces"),
				title: translate("auto.components.mobile.MobileHero.85067b9e06", "Refresh network interfaces"),
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RefreshCw, { className: "size-3.5" })
			})
		]
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: cn("mp-pairing-layout", relayMintFailure != null && "has-failure"),
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "mp-step2-copy mp-pairing-copy",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "mp-eyebrow-row",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-step-num",
							children: "2"
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "mp-eyebrow",
							children: translate("auto.components.mobile.MobileHero.3960f5c339", "Step 2 of 2")
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h2", {
						className: "mp-h2",
						children: pairDeviceHeading()
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
						className: "mp-lead-sm",
						children: [
							translate("auto.components.mobile.MobileHero.d1495e5e64", "Open Orca Mobile, tap"),
							" ",
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("strong", { children: translate("auto.components.mobile.MobileHero.3aa7bb2d8b", "Pair Desktop") }),
							translate("auto.components.mobile.MobileHero.2f077ef4eb", ", and scan the code.")
						]
					})
				]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "mp-pairing-relay",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(MobilePairingConnectionOptions, {
					value: connectionMode,
					onChange: onConnectionModeChange,
					compact: true,
					relayMintFailed: relayMintFailure != null,
					relayMintRetrying: relayMintFailure != null && pairLoading
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MobileRelayBetaNotice, { className: "mt-1.5" })]
			}),
			relayMintFailure != null ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MobileRelayMintFailureNotice, {
				className: "mp-pairing-failure",
				failure: relayMintFailure,
				onUseLan,
				onRetry: onRetryRelay,
				onCopyDiagnostics: onCopyRelayDiagnostics,
				compact: true,
				busy: pairLoading
			}) : null,
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "mp-qr-stack mp-pairing-qr",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "mp-qr mp-qr-large",
						"aria-busy": pairLoading,
						children: [
							pairQrDataUrl ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("img", {
								src: pairQrDataUrl,
								alt: translate("auto.components.mobile.MobileHero.27735e5f4e", "Pairing QR"),
								className: cn(pairLoading && "mp-qr-refreshing")
							}) : null,
							pairLoading ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "mp-qr-loading",
								children: translate("auto.components.mobile.MobileHero.65b3f2e8bc", "Generating…")
							}) : null,
							emptyQrMessage != null ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "mp-qr-empty text-center text-xs text-muted-foreground px-3",
								children: emptyQrMessage
							}) : null
						]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "sr-only",
						role: "status",
						"aria-live": "polite",
						children: pairQrDataUrl != null && !pairLoading ? translate("auto.components.mobile.MobileHero.pairingCodeReady", "Pairing code ready") : ""
					}),
					relayMintFailure == null ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
						type: "button",
						className: "mp-link-under",
						onClick: onRegeneratePairing,
						disabled: pairLoading || !canGeneratePairing,
						children: pairLoading ? translate("auto.components.mobile.MobileHero.65b3f2e8bc", "Generating…") : pairQrDataUrl ? translate("auto.components.mobile.MobileHero.e59a252eca", "Regenerate code") : translate("auto.components.mobile.MobileHero.a6cffbbb0b", "Generate code")
					}) : null,
					pairingQrError ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
						className: "flex w-full min-w-0 items-start gap-1.5 text-xs text-destructive",
						role: "alert",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CircleAlert, {
							className: "mt-0.5 size-3.5 shrink-0",
							"aria-hidden": true
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "min-w-0",
							children: translate("auto.components.mobile.MobileHero.pairingQrError", "This pairing code couldn’t be rendered as a QR code. Copy it into Orca Mobile instead.")
						})]
					}) : null
				]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "mp-pairing-controls",
				children: [
					usingRelay && !networkDisclosurePinned ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Collapsible, {
						open: networkDisclosureOpen,
						onOpenChange: setNetworkDisclosureOpen,
						className: "mb-[18px]",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CollapsibleTrigger, {
							asChild: true,
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
								type: "button",
								className: "mp-disclosure-trigger",
								children: [translate("auto.components.mobile.MobileHero.directAddressDisclosure", "Also use a faster local path"), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronDown, { className: cn("size-3.5 transition-transform", networkDisclosureOpen && "rotate-180") })]
							})
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CollapsibleContent, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "mt-2 space-y-2 [&>.mp-network-row]:mb-0!",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "mp-disclosure-hint",
								children: translate("auto.components.mobile.MobileHero.directAddressHint", "Optional. Pick the Wi‑Fi or Tailscale address your phone should use when nearby — usually faster than Relay. Relay still works when you’re away.")
							}), networkRow]
						}) })]
					}) : networkRow,
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "mp-inline-actions",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "mp-action-divider",
							children: translate("auto.components.mobile.MobileHero.4c1df4eba7", "Can't scan?")
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
							ref: copyPairingCodeRef,
							type: "button",
							className: "mp-text-link",
							onClick: onCopyPairingCode,
							disabled: !pairingUrl || pairLoading,
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Copy, { className: "size-3.5" }), translate("auto.components.mobile.MobileHero.010dddcf27", "Copy pairing code")]
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WindowsFirewallNotice, {
						pairingReady: pairQrDataUrl != null,
						address: selectedAddress,
						usingRelay,
						className: "mt-3"
					})
				]
			})
		]
	});
}
function HeroIntro({ onStart }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "mp-intro-shell",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "mp-eyebrow-row",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "mp-eyebrow",
					children: translate("auto.components.mobile.MobileHero.5410d55d79", "Orca Mobile")
				})
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h1", {
				className: "mp-h1",
				children: translate("auto.components.mobile.MobileHero.cd4e5e816f", "Your workspaces, in your pocket.")
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				className: "mp-lead",
				children: translate("auto.components.mobile.MobileHero.b4ccce5cb7", "Control Orca from your phone. Check on agents, review changes, and kick off tasks while you're away from your desk.")
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "mp-platform-badges",
				"aria-label": translate("auto.components.mobile.MobileHero.ec0607bf66", "Supported mobile platforms"),
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "mp-platform-label",
						children: translate("auto.components.mobile.MobileHero.da1d5e5ed0", "Available on")
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
						className: "mp-platform-badge",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(IosBrandIcon, {}), translate("auto.components.mobile.MobileHero.711e6f4b47", "iOS")]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
						className: "mp-platform-badge",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AndroidLogo, {}), translate("auto.components.mobile.MobileHero.ac1eb64952", "Android")]
					})
				]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "mp-cta-row",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
					type: "button",
					className: "mp-primary-action mp-flow-primary-action",
					onClick: onStart,
					children: [translate("auto.components.mobile.MobileHero.10d27b4cba", "Get started"), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowRight, { className: "size-3.5" })]
				})
			})
		]
	});
}
function HeroPaired({ devices, onPairAnother, onRevoke, revokingDeviceIds }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [
		/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "mp-eyebrow-row",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "mp-eyebrow",
				children: translate("auto.components.mobile.MobileHero.5410d55d79", "Orca Mobile")
			})
		}),
		/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h1", {
			className: "mp-h1",
			children: devices.length === 1 ? translate("auto.components.mobile.MobileHero.051978a785", "Your phone is paired.") : translate("auto.components.mobile.MobileHero.d0b52871ce", "Your phones are paired.")
		}),
		/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
			className: "mp-lead-sm",
			children: translate("auto.components.mobile.MobileHero.266c18c105", "Open Orca Mobile to pick up where you left off, or pair another device.")
		}),
		/* @__PURE__ */ (0, import_jsx_runtime.jsx)("ul", {
			className: "mp-paired-list",
			children: devices.map((device) => {
				const revoking = revokingDeviceIds.includes(device.deviceId);
				return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("li", {
					className: "mp-paired-row",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-paired-icon",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Smartphone, { className: "size-4" })
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "mp-paired-main",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "mp-paired-name",
								children: device.name
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "mp-paired-meta",
								children: [
									translate("auto.components.mobile.MobileHero.94829abdb1", "Paired"),
									" ",
									new Date(device.pairedAt).toLocaleDateString()
								]
							})]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
							type: "button",
							className: "mp-paired-revoke",
							onClick: () => onRevoke(device.deviceId),
							disabled: revoking,
							"aria-label": translate("auto.components.mobile.MobileHero.34f878d04f", "Revoke {{value0}}", { value0: device.name }),
							title: translate("auto.components.mobile.MobileHero.f9cbf4bb53", "Revoke device"),
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, { className: "size-3.5" })
						})
					]
				}, device.deviceId);
			})
		}),
		/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "mp-flow-actions",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
				type: "button",
				className: "mp-secondary-action",
				onClick: onPairAnother,
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Smartphone, { className: "size-3.5" }), translate("auto.components.mobile.MobileHero.ff48d9d520", "Pair another device")]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {})]
		})
	] });
}
function HeroFlow({ stepIdx, platform, onPlatformChange, installQrUrl, installCopy, iosChannel, onIosChannelChange, onOpenInstallUrl, onCopyInstallUrl, pairQrDataUrl, pairingUrl, pairingQrError, relayMintFailure, onUseLan, onRetryRelay, onCopyRelayDiagnostics, pairLoading, connectionMode, onConnectionModeChange, onRegeneratePairing, canGeneratePairing, onCopyPairingCode, networkInterfaces, customAddresses, selectedAddress, selectedAddressIsCustom, onSelectedAddressChange, onCustomAddressSelect, onCustomAddressRemove, beforeCustomAddressChange, onRefreshNetworkInterfaces, refreshingNetworkInterfaces, onBack, onContinue, onDone }) {
	const isLast = stepIdx === 1;
	const screenRefs = (0, import_react.useRef)([]);
	const [viewportHeight, setViewportHeight] = (0, import_react.useState)();
	(0, import_react.useLayoutEffect)(() => {
		const activeScreen = screenRefs.current[stepIdx];
		if (!activeScreen) return;
		const measure = () => setViewportHeight(activeScreen.scrollHeight);
		measure();
		if (typeof ResizeObserver === "undefined") return;
		const observer = new ResizeObserver(measure);
		observer.observe(activeScreen);
		return () => observer.disconnect();
	}, [stepIdx]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "mp-flow-card",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "mp-flow-viewport",
			style: viewportHeight === void 0 ? void 0 : { height: viewportHeight },
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				ref: (element) => {
					screenRefs.current[0] = element;
				},
				className: cn("mp-flow-screen", stepIdx === 0 ? "is-active" : "is-past"),
				"aria-hidden": stepIdx !== 0,
				inert: stepIdx !== 0,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "mp-step2-layout",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "mp-step2-copy",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "mp-eyebrow-row",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "mp-step-num",
									children: stepIdx + 1
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "mp-eyebrow",
									children: translate("auto.components.mobile.MobileHero.92ddfdfa1f", "Step 1 of 2")
								})]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h2", {
								className: "mp-h2",
								children: translate("auto.components.mobile.MobileHero.0d9b33299e", "Get the app.")
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "mp-lead-sm",
								children: translate("auto.components.mobile.MobileHero.e75647ace0", "Scan the QR with your phone or open the install link to grab Orca Mobile.")
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "mp-tab-toggle",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
									type: "button",
									className: cn(platform === "ios" && "is-active"),
									"aria-pressed": platform === "ios",
									onClick: () => onPlatformChange("ios"),
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(IosBrandIcon, {}), translate("auto.components.mobile.MobileHero.711e6f4b47", "iOS")]
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
									type: "button",
									className: cn(platform === "android" && "is-active"),
									"aria-pressed": platform === "android",
									onClick: () => onPlatformChange("android"),
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AndroidLogo, {}), translate("auto.components.mobile.MobileHero.ac1eb64952", "Android")]
								})]
							}),
							platform === "ios" ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "mp-channel-toggle",
								role: "radiogroup",
								"aria-label": translate("auto.components.mobile.MobileHero.channel.group", "Release channel"),
								children: [
									/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
										type: "button",
										role: "radio",
										"aria-checked": iosChannel === "preview",
										className: cn(iosChannel === "preview" && "is-active"),
										onClick: () => onIosChannelChange("preview"),
										children: translate("auto.components.mobile.MobileHero.channel.preview", "Preview")
									}),
									/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
										type: "button",
										role: "radio",
										"aria-checked": iosChannel === "stable",
										className: cn(iosChannel === "stable" && "is-active"),
										onClick: () => onIosChannelChange("stable"),
										children: translate("auto.components.mobile.MobileHero.channel.stable", "Stable")
									}),
									/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
										className: "mp-channel-tagline",
										children: getChannelTagline(iosChannel)
									})
								]
							}) : null,
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "mp-inline-actions",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
									type: "button",
									className: "mp-ghost-action",
									onClick: onOpenInstallUrl,
									children: installCopy.ctaLabel
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
									type: "button",
									className: "mp-text-link",
									onClick: onCopyInstallUrl,
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Copy, { className: "size-3.5" }), translate("auto.components.mobile.MobileHero.aa97420ba4", "Copy install link")]
								})]
							})
						]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "mp-qr mp-qr-large",
						children: installQrUrl ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("img", {
							src: installQrUrl,
							alt: translate("auto.components.mobile.MobileHero.3241f3c26a", "Install QR")
						}) : null
					})]
				})
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				ref: (element) => {
					screenRefs.current[1] = element;
				},
				className: cn("mp-flow-screen", stepIdx === 1 && "is-active"),
				"aria-hidden": stepIdx !== 1,
				inert: stepIdx !== 1,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MobileHeroPairingStep, {
					pairQrDataUrl,
					pairingUrl,
					pairingQrError,
					relayMintFailure,
					onUseLan,
					onRetryRelay,
					onCopyRelayDiagnostics,
					pairLoading,
					connectionMode,
					onConnectionModeChange,
					onRegeneratePairing,
					canGeneratePairing,
					onCopyPairingCode,
					networkInterfaces,
					customAddresses,
					selectedAddress,
					selectedAddressIsCustom,
					onSelectedAddressChange,
					onCustomAddressSelect,
					onCustomAddressRemove,
					beforeCustomAddressChange,
					onRefreshNetworkInterfaces,
					refreshingNetworkInterfaces
				})
			})]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "mp-flow-actions",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
				type: "button",
				className: "mp-flow-back",
				onClick: onBack,
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowLeft, { className: "size-3" }), translate("auto.components.mobile.MobileHero.b622eba64d", "Back")]
			}), isLast ? onDone ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
				type: "button",
				className: "mp-primary-action mp-flow-primary-action",
				onClick: onDone,
				children: [translate("auto.components.mobile.MobileHero.3f90dbd274", "Done"), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowRight, { className: "size-3.5" })]
			}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {}) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
				type: "button",
				className: "mp-flow-continue mp-flow-primary-action",
				onClick: onContinue,
				children: [translate("auto.components.mobile.MobileHero.a8fb43cf1c", "Continue"), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowRight, { className: "size-3.5" })]
			})]
		})]
	});
}
function MobilePageToolbar({ showMobileButton, onClose, onToggleMobileSidebarButton }) {
	const sidebarToggleLabel = showMobileButton ? translate("auto.components.mobile.MobilePageToolbar.c669abcf8f", "Hide from sidebar") : translate("auto.components.mobile.MobilePageToolbar.fb5f28330e", "Show in sidebar");
	const sidebarToggleTooltip = showMobileButton ? translate("auto.components.mobile.MobilePageToolbar.e1c7b4a92d", "Configure in Settings > Mobile.") : translate("auto.components.mobile.MobilePageToolbar.f3d8e5b71a", "Adds the shortcut back to the sidebar.");
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "mp-page-toolbar",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "mp-page-toolbar-primary",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
				asChild: true,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					variant: showMobileButton ? "default" : "secondary",
					size: "sm",
					className: "mp-sidebar-toggle-btn",
					onClick: onToggleMobileSidebarButton,
					"aria-label": sidebarToggleLabel,
					children: sidebarToggleLabel
				})
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
				side: "bottom",
				sideOffset: 6,
				children: sidebarToggleTooltip
			})] })
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
			asChild: true,
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				variant: "ghost",
				size: "icon",
				className: "mp-page-toolbar-close size-7 shrink-0 rounded-full",
				onClick: onClose,
				"aria-label": translate("auto.components.mobile.MobilePageToolbar.9883b58693", "Close Orca Mobile"),
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "size-4" })
			})
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
			side: "bottom",
			sideOffset: 6,
			children: translate("auto.components.mobile.MobilePageToolbar.ad2284a9e2", "Close · Esc")
		})] })]
	});
}
function HomeSlide({ tapping }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "mp-device-screen",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "mp-app-topbar",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "mp-app-brand",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(OrcaLogo, {}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "mp-app-brand-name",
					children: translate("auto.components.mobile.slides.HomeSlide.5d94e8ddcc", "Orca")
				})]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				type: "button",
				className: "mp-icon-button",
				"aria-label": translate("auto.components.mobile.slides.HomeSlide.af761a0c0d", "Settings"),
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsIcon, {})
			})]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "mp-scroll-region",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "mp-greeting",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "mp-greeting-title",
						children: translate("auto.components.mobile.slides.HomeSlide.c0e2e9dcd9", "Welcome back")
					})
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "mp-stat-row",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Stat, {
							value: "1,284",
							label: translate("auto.components.mobile.slides.HomeSlide.00a6903322", "Agents spawned")
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Stat, {
							value: "142h",
							label: translate("auto.components.mobile.slides.HomeSlide.4a40af029b", "Agent time")
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Stat, {
							value: "96",
							label: translate("auto.components.mobile.slides.HomeSlide.156db8a68a", "PRs created")
						})
					]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "mp-section-label",
					children: translate("auto.components.mobile.slides.HomeSlide.2f1a1d10c4", "Desktops")
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: cn("mp-host-card", tapping && "is-tapping"),
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-host-icon",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DesktopIcon, {})
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "mp-host-main",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "mp-host-name",
								children: translate("auto.components.mobile.slides.HomeSlide.19c212e25e", "MacBook Pro")
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "mp-host-meta",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "mp-status-dot is-green" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.mobile.slides.HomeSlide.0bc1881bc4", "Connected · 40 worktrees · 5 active") })]
							})]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-chevron-right",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronIcon, {})
						})
					]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "mp-host-card",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-host-icon is-dim",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DesktopIcon, {})
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "mp-host-main",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "mp-host-name is-dim",
								children: translate("auto.components.mobile.slides.HomeSlide.091355da3d", "M1 Mini · home")
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "mp-host-meta",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "mp-status-dot is-muted" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.mobile.slides.HomeSlide.cf3f98fa3f", "Disconnected") })]
							})]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-chevron-right",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronIcon, {})
						})
					]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "mp-section-label",
					style: { marginTop: 14 },
					children: translate("auto.components.mobile.slides.HomeSlide.c791677f2f", "Resume")
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "mp-resume-card",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-resume-icon",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ResumeIcon, {})
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "mp-host-main",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "mp-resume-title",
								children: translate("auto.components.mobile.slides.HomeSlide.25d6e8a491", "feat/mobile-page")
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "mp-resume-sub",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "mp-repo-dot",
									style: { background: "#3b82f6" }
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.mobile.slides.HomeSlide.d33d7a9c29", "orca  ·  feat/mobile-page") })]
							})]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-chevron-right",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronIcon, {})
						})
					]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "mp-section-label",
					style: { marginTop: 10 },
					children: translate("auto.components.mobile.slides.HomeSlide.a4c3f7b7aa", "Tasks")
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "mp-task-home-card",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-task-home-icon",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ListTodoIcon, {})
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "mp-host-main",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "mp-task-home-title",
								children: translate("auto.components.mobile.slides.HomeSlide.a4c3f7b7aa", "Tasks")
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "mp-task-home-subtitle",
								children: translate("auto.components.mobile.slides.HomeSlide.d047197480", "GitHub · Linear")
							})]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "mp-task-home-providers",
							"aria-label": translate("auto.components.mobile.slides.HomeSlide.0bad5b07c8", "GitHub and Linear"),
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "mp-task-home-provider-button",
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(GithubIcon, {})
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "mp-task-home-provider-button",
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LinearIcon, {})
							})]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-chevron-right",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronIcon, {})
						})
					]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "mp-section-label",
					style: { marginTop: 14 },
					children: translate("auto.components.mobile.slides.HomeSlide.0b00c98506", "Quick Actions")
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "mp-quick-actions",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "mp-quick-action",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-quick-action-icon",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(QrSmallIcon, {})
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-quick-action-label",
							children: translate("auto.components.mobile.slides.HomeSlide.4405f3c440", "Pair Desktop")
						})]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "mp-quick-action",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-quick-action-icon",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PlusIcon$2, {})
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-quick-action-label",
							children: translate("auto.components.mobile.slides.HomeSlide.e27fdaee51", "New Workspace")
						})]
					})]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "mp-section-label",
					style: { marginTop: 14 },
					children: translate("auto.components.mobile.slides.HomeSlide.8a350a4784", "Account usage")
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "mp-accounts-card",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AccountRow, {
						icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ClaudeIcon, { size: 18 }),
						email: "claude@stably.ai",
						sessionPct: 42,
						weekPct: 18
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AccountRow, {
						icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(OpenAIIcon, { size: 18 }),
						email: "codex@stably.ai",
						sessionPct: 67,
						weekPct: 31
					})]
				})
			]
		})]
	});
}
function Stat({ value, label }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "mp-stat-card",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "mp-stat-value",
			children: value
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "mp-stat-label",
			children: label
		})]
	});
}
function AccountRow({ icon, email, sessionPct, weekPct }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "mp-accounts-row",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "mp-accounts-icon",
			children: icon
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "mp-accounts-info",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "mp-accounts-email",
				children: email
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "mp-accounts-bars",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(UsageBar, {
					label: translate("auto.components.mobile.slides.HomeSlide.a3d5476811", "5h"),
					pct: sessionPct
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(UsageBar, {
					label: translate("auto.components.mobile.slides.HomeSlide.a7d9e2c44d", "7d"),
					pct: weekPct
				})]
			})]
		})]
	});
}
function UsageBar({ label, pct }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "mp-usage-bar",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "mp-usage-bar-label",
			children: label
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "mp-usage-bar-track",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "mp-usage-bar-fill",
				style: { width: `${pct}%` }
			})
		})]
	});
}
function OrcaLogo() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("svg", {
		className: "mp-orca-logo",
		viewBox: "0 0 318.60232 202.66667",
		fill: "currentColor",
		"aria-hidden": true,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("g", {
			transform: "translate(-6.6666669,-70.666669)",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m 177.81311,248.33334 c 23.82304,-41.29793 40.54045,-66.84626 49.51207,-75.66667 6.81685,-6.70196 10.07373,-8.7374 20.07265,-12.54475 34.57822,-13.16655 61.04674,-26.78733 72.37222,-37.24295 9.62924,-8.88966 9.34286,-9.01142 -23.43671,-9.964 -35.71756,-1.03796 -43.72989,0.42119 -62.17546,11.323 -16.72118,9.88265 -34.20103,30.11225 -42.74704,49.47157 -2.57353,5.82985 -14.81294,44.3056 -27.96399,87.90747 -2.86036,9.48343 -3.02466,11.71633 -0.86213,11.71633 0.44382,0 7.29659,-11.25 15.22839,-25 z m -65.14644,-8.32267 C 120,239.3326 130.5,237.50979 136,235.95998 c 5.5,-1.5498 12.25,-3.13783 15,-3.52895 2.75,-0.39111 5,-0.95485 5,-1.25275 0,-0.29789 2.15135,-7.58487 4.78078,-16.19328 8.49209,-27.80201 12.21334,-40.41629 21.13747,-71.65166 4.81891,-16.86667 11.23502,-39.185 14.25802,-49.596301 5.12803,-17.66103 5.74763,-23.07037 2.64253,-23.07037 -1.84887,0 -4.07048,6.908293 -16.72243,52.000001 -21.78975,77.65896 -20.80806,74.74393 -26.84794,79.72251 -7.5925,6.25838 -25.03916,14.82524 -36.10856,17.73044 -17.0947,4.48656 -33.410599,3.86724 -53.116765,-2.01622 -18.569242,-5.54403 -23.142662,-5.80284 -33.639754,-1.9037 -5.875424,2.18242 -9.864152,5.04363 -16.716684,11.99127 -4.95,5.0187 -9.0000001,10.02884 -9.0000001,11.13364 0,1.75174 5.9276921,2.00299 46.3333351,1.96383 25.483334,-0.0247 52.333338,-0.59969 59.666668,-1.27777 z M 252.69513,104.63708 c 12.18267,-3.48651 15.77304,-7.895503 9.63821,-11.835773 -10.19296,-6.546726 -36.19849,-1.77301 -41.19436,7.561863 -1.2556,2.3461 -0.98698,3.2037 1.68353,5.375 2.69471,2.19098 4.59991,2.47691 12.53928,1.88189 5.14899,-0.3859 12.94899,-1.72824 17.33334,-2.98298 z" })
		})
	});
}
function SettingsIcon() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", {
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", {
			cx: "12",
			cy: "12",
			r: "3"
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h0a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h0a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v0a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1Z" })]
	});
}
function DesktopIcon() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", {
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", {
				x: "2",
				y: "3",
				width: "20",
				height: "14",
				rx: "2"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M8 21h8" }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M12 17v4" })
		]
	});
}
function ChevronIcon() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("svg", {
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m9 18 6-6-6-6" })
	});
}
function ResumeIcon() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", {
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m4 17 6-6-6-6" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M12 19h8" })]
	});
}
function ListTodoIcon() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", {
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", {
				x: "3",
				y: "5",
				width: "6",
				height: "6",
				rx: "1"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m3 17 2 2 4-4" }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M13 6h8" }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M13 12h8" }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M13 18h8" })
		]
	});
}
function GithubIcon() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", {
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M15 22v-4a4.8 4.8 0 0 0-1-3.5c3 0 6-2 6-5.5.08-1.25-.27-2.48-1-3.5.28-1.15.28-2.35 0-3.5 0 0-1 0-3 1.5-2.64-.5-5.36-.5-8 0C6 2 5 2 5 2c-.3 1.15-.3 2.35 0 3.5A5.4 5.4 0 0 0 4 9c0 3.5 3 5.5 6 5.5-.39.49-.68 1.05-.85 1.65-.17.6-.22 1.23-.15 1.85v4" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M9 18c-4.51 2-5-2-7-2" })]
	});
}
function LinearIcon() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("svg", {
		viewBox: "0 0 100 100",
		fill: "currentColor",
		"aria-hidden": true,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M1.225 61.523c-.187-.738.708-1.235 1.246-.697l36.703 36.703c.538.538.041 1.433-.697 1.246C20.6 94.16 5.84 79.4 1.225 61.523ZM.002 46.811a.997.997 0 0 0 .291.749l52.147 52.147a.998.998 0 0 0 .749.291 50.328 50.328 0 0 0 9.235-1.119c.667-.149.904-.972.422-1.454L1.575 37.154c-.482-.482-1.305-.245-1.454.422A50.328 50.328 0 0 0 .002 46.81Zm4.528-18.34a.998.998 0 0 0 .195 1.144l64.66 64.66a.998.998 0 0 0 1.144.195 50.45 50.45 0 0 0 5.913-3.46.999.999 0 0 0 .14-1.518L9.51 22.418a.999.999 0 0 0-1.518.14 50.45 50.45 0 0 0-3.46 5.913Zm10.435-13.075a.999.999 0 0 0 .002 1.41l68.226 68.226a.999.999 0 0 0 1.41.002c19.292-19.477 19.234-50.97-.176-70.378-19.410-19.410-50.901-19.468-70.378-.176-1.061 1.044.916 1.916.916 1.916Z" })
	});
}
function QrSmallIcon() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", {
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", {
				x: "3",
				y: "3",
				width: "7",
				height: "7",
				rx: "1"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", {
				x: "14",
				y: "3",
				width: "7",
				height: "7",
				rx: "1"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", {
				x: "3",
				y: "14",
				width: "7",
				height: "7",
				rx: "1"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", {
				x: "14",
				y: "14",
				width: "3",
				height: "3"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", {
				x: "18",
				y: "14",
				width: "3",
				height: "3"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", {
				x: "14",
				y: "18",
				width: "3",
				height: "3"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", {
				x: "18",
				y: "18",
				width: "3",
				height: "3"
			})
		]
	});
}
function PlusIcon$2() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", {
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M5 12h14" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M12 5v14" })]
	});
}
function WorktreeListSlide({ tapping }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "mp-device-screen",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "mp-wl-chrome",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "mp-wl-statusrow",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
						type: "button",
						className: "mp-wl-back",
						"aria-label": translate("auto.components.mobile.slides.WorktreeListSlide.cefd048225", "Back"),
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronLeftIcon$1, {})
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "mp-wl-host",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "mp-status-dot is-green" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "mp-wl-host-name",
							children: translate("auto.components.mobile.slides.WorktreeListSlide.b4271864bd", "MacBook Pro")
						})]
					})]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "mp-wl-toolbar",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
							type: "button",
							className: "mp-wl-chip",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(FilterIcon, {}), translate("auto.components.mobile.slides.WorktreeListSlide.0e3e809a4b", "Filter")]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
							type: "button",
							className: "mp-wl-button",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SortIcon, {}), translate("auto.components.mobile.slides.WorktreeListSlide.17f9e0d226", "Recent")]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
							type: "button",
							className: "mp-wl-button",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(GroupIcon, {}), translate("auto.components.mobile.slides.WorktreeListSlide.22971156df", "Repo")]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "mp-wl-spacer" }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "mp-wl-icon",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(UserCircleIcon, {})
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "mp-wl-icon",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PlusIcon$1, {})
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "mp-wl-icon",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SearchIcon, {})
						})
					]
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "mp-wl-section",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CaretIcon, {}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(PinIcon, {}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						style: { marginLeft: 4 },
						children: translate("auto.components.mobile.slides.WorktreeListSlide.79a24ff530", "Pinned")
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						style: {
							marginLeft: 4,
							color: "var(--m-text-muted)"
						},
						children: "3"
					})
				]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "mp-wl-list",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorktreeRow, {
						indicator: "spinner",
						name: "feat/mobile-page",
						pr: "#2491",
						repoColor: "#3b82f6",
						repo: "orca",
						branch: "feat/mobile-page",
						preview: "claude · refactoring v3 mock to use real screens…",
						tcount: 2,
						tapping
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "mp-wl-sep" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorktreeRow, {
						indicator: "green",
						name: "runtime/web-pairing",
						pr: "#2487",
						repoColor: "#22c55e",
						repo: "orca",
						branch: "feat/web-pairing",
						preview: "$ pnpm test --filter web-runtime",
						tcount: 1
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "mp-wl-sep" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorktreeRow, {
						indicator: "red",
						name: "infra/notifier",
						repoColor: "#f97316",
						repo: "orca",
						branch: "main",
						preview: "awaiting permission · sudo apt install",
						tcount: 1
					})
				]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "mp-wl-section",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CaretIcon, {}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.mobile.slides.WorktreeListSlide.357a519567", "Active") }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						style: {
							marginLeft: 4,
							color: "var(--m-text-muted)"
						},
						children: "37"
					})
				]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "mp-wl-list",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorktreeRow, {
						indicator: "green",
						name: "docs/styleguide-update",
						repoColor: "#8b5cf6",
						repo: "orca",
						branch: "feat/styleguide",
						preview: "$ pnpm lint",
						tcount: 1
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "mp-wl-sep" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorktreeRow, {
						indicator: "muted",
						name: "feat/runtime-perf",
						repoColor: "#3b82f6",
						repo: "orca",
						branch: "feat/runtime-perf"
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "mp-wl-sep" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorktreeRow, {
						indicator: "spinner",
						name: "fix/notifier-cooldown",
						pr: "#2483",
						repoColor: "#f97316",
						repo: "orca",
						branch: "feat/notifier-cooldown",
						preview: "claude · investigating macOS notification queue…",
						tcount: 1
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "mp-wl-sep" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorktreeRow, {
						indicator: "muted",
						name: "chore/deps-bump",
						repoColor: "#22c55e",
						repo: "orca",
						branch: "feat/deps-bump"
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "mp-wl-sep" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorktreeRow, {
						indicator: "green",
						name: "experiment/ssh-multiplex",
						repoColor: "#3b82f6",
						repo: "orca",
						branch: "feat/ssh-mux",
						preview: "$ ssh -O check orca-relay",
						tcount: 2
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "mp-wl-sep" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorktreeRow, {
						indicator: "muted",
						name: "refactor/host-store",
						repoColor: "#8b5cf6",
						repo: "orca",
						branch: "feat/host-store"
					})
				]
			})
		]
	});
}
function WorktreeRow({ indicator, name, pr, repoColor, repo, branch, preview, tcount, tapping }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: cn("mp-wl-row", tapping && "is-tapping"),
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "mp-wl-indicator",
				children: indicator === "spinner" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "mp-wl-spinner" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: cn("mp-wl-dot", `is-${indicator}`) })
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "mp-wl-main",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "mp-wl-name-row",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-wl-name",
							children: name
						}), pr ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "mp-wl-pr",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(PrIcon, {}), pr]
						}) : null]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "mp-wl-meta-row",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "mp-repo-dot",
								style: { background: repoColor }
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: repo }),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "mp-wl-branch",
								children: branch
							})
						]
					}),
					preview ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "mp-wl-preview",
						children: preview
					}) : null
				]
			}),
			tcount !== void 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "mp-wl-tcount",
				children: tcount
			}) : null
		]
	});
}
function ChevronLeftIcon$1() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("svg", {
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m15 18-6-6 6-6" })
	});
}
function FilterIcon() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("svg", {
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("polygon", { points: "22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3" })
	});
}
function SortIcon() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", {
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("line", {
				x1: "21",
				y1: "4",
				x2: "14",
				y2: "4"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("line", {
				x1: "10",
				y1: "4",
				x2: "3",
				y2: "4"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("line", {
				x1: "21",
				y1: "12",
				x2: "12",
				y2: "12"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("line", {
				x1: "8",
				y1: "12",
				x2: "3",
				y2: "12"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("line", {
				x1: "21",
				y1: "20",
				x2: "16",
				y2: "20"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("line", {
				x1: "12",
				y1: "20",
				x2: "3",
				y2: "20"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("line", {
				x1: "14",
				y1: "2",
				x2: "14",
				y2: "6"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("line", {
				x1: "8",
				y1: "10",
				x2: "8",
				y2: "14"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("line", {
				x1: "16",
				y1: "18",
				x2: "16",
				y2: "22"
			})
		]
	});
}
function GroupIcon() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", {
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.91a1 1 0 0 0 0-1.83Z" }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M2 12a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 12" }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M2 17a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 17" })
		]
	});
}
function UserCircleIcon() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", {
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", {
				cx: "12",
				cy: "12",
				r: "10"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M18 20a6 6 0 0 0-12 0" }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", {
				cx: "12",
				cy: "10",
				r: "4"
			})
		]
	});
}
function PlusIcon$1() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", {
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M5 12h14" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M12 5v14" })]
	});
}
function SearchIcon() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", {
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", {
			cx: "11",
			cy: "11",
			r: "8"
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m21 21-4.3-4.3" })]
	});
}
function CaretIcon() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("svg", {
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m6 9 6 6 6-6" })
	});
}
function PinIcon() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", {
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		style: { marginLeft: 2 },
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M12 17v5" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1Z" })]
	});
}
function PrIcon() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", {
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", {
				cx: "6",
				cy: "6",
				r: "3"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M6 9v12" }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", {
				cx: "18",
				cy: "18",
				r: "3"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M13 6h3a2 2 0 0 1 2 2v7" })
		]
	});
}
function TerminalSlide() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "mp-device-screen",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "mp-session-chrome",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "mp-session-topbar",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
							type: "button",
							className: "mp-session-back",
							"aria-label": translate("auto.components.mobile.slides.TerminalSlide.8fd998acd3", "Back"),
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronLeftIcon, {})
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "mp-session-title-block",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "mp-session-title",
								children: translate("auto.components.mobile.slides.TerminalSlide.8432787c4e", "feat/mobile-page")
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "mp-session-meta-row",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "mp-status-dot is-green" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.mobile.slides.TerminalSlide.8d6516312d", "2 terminals · claude active") })]
							})]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
							type: "button",
							className: "mp-session-iconbtn",
							"aria-label": translate("auto.components.mobile.slides.TerminalSlide.94febb0976", "Source control"),
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(BranchIcon, {})
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
							type: "button",
							className: "mp-session-iconbtn",
							"aria-label": translate("auto.components.mobile.slides.TerminalSlide.606aa93192", "Files"),
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(FolderIcon, {})
						})
					]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "mp-session-tabbar",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-session-tab is-active",
							children: translate("auto.components.mobile.slides.TerminalSlide.2c10d43745", "claude")
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-session-tab",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.mobile.slides.TerminalSlide.e4befee569", "shell") })
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "mp-session-tab",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(FileIcon, {}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: translate("auto.components.mobile.slides.TerminalSlide.da121ba48d", "PLAN.md") })]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-session-tab-add",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PlusIcon, {})
						})
					]
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "mp-terminal",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
						className: "mp-term-line",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "mp-term-prompt",
								children: translate("auto.components.mobile.slides.TerminalSlide.2defc05141", "dev@mac")
							}),
							" ",
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "mp-term-dim",
								children: translate("auto.components.mobile.slides.TerminalSlide.e0f98be657", "orca/feat-mobile-page")
							}),
							" ",
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "mp-term-prompt",
								children: "$"
							}),
							" ",
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "mp-term-cmd",
								children: translate("auto.components.mobile.slides.TerminalSlide.2c10d43745", "claude")
							})
						]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "mp-term-line" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
						className: "mp-term-line",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "mp-term-tool",
								children: "●"
							}),
							" ",
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "mp-term-mid",
								children: translate("auto.components.mobile.slides.TerminalSlide.80cc356591", "Read")
							}),
							" ",
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "mp-term-dim",
								children: translate("auto.components.mobile.slides.TerminalSlide.336c0e070e", "mobile/orca-mobile-sidebar-mock-v3.html")
							})
						]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
						className: "mp-term-line",
						children: ["  ", /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "mp-term-comment",
							children: translate("auto.components.mobile.slides.TerminalSlide.fc83e0d5ef", "⎿ Read 2103 lines")
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "mp-term-line" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
						className: "mp-term-line",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "mp-term-tool",
								children: "●"
							}),
							" ",
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "mp-term-mid",
								children: translate("auto.components.mobile.slides.TerminalSlide.6d4ebd5833", "Edit")
							}),
							" ",
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "mp-term-dim",
								children: translate("auto.components.mobile.slides.TerminalSlide.336c0e070e", "mobile/orca-mobile-sidebar-mock-v3.html")
							})
						]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
						className: "mp-term-line",
						children: ["  ", /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "mp-term-comment",
							children: translate("auto.components.mobile.slides.TerminalSlide.d6d1041a1c", "⎿ Replaced pair-scan slide with terminal session")
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "mp-term-line" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
						className: "mp-term-line",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "mp-term-tool",
								children: "●"
							}),
							" ",
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "mp-term-mid",
								children: translate("auto.components.mobile.slides.TerminalSlide.21b67dfc92", "Bash")
							}),
							" ",
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "mp-term-dim",
								children: translate("auto.components.mobile.slides.TerminalSlide.a6e7cdc688", "pnpm test --filter mobile")
							})
						]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
						className: "mp-term-line",
						children: [
							"  ",
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "mp-term-comment",
								children: "⎿ "
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "mp-term-ok",
								children: translate("auto.components.mobile.slides.TerminalSlide.1d448b69f7", "PASS")
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
								className: "mp-term-comment",
								children: [" ", translate("auto.components.mobile.slides.TerminalSlide.d39445686a", "src/transport/host-store.test.ts")]
							})
						]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
						className: "mp-term-line",
						children: [
							"     ",
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "mp-term-ok",
								children: translate("auto.components.mobile.slides.TerminalSlide.1d448b69f7", "PASS")
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
								className: "mp-term-comment",
								children: [" ", translate("auto.components.mobile.slides.TerminalSlide.4b3666f9a9", "src/cache/worktree-cache.test.ts")]
							})
						]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
						className: "mp-term-line",
						children: [
							"     ",
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "mp-term-warn",
								children: "●"
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
								className: "mp-term-comment",
								children: [" ", translate("auto.components.mobile.slides.TerminalSlide.3ce3e8c892", "14 passed, 1 skipped (1.8s)")]
							})
						]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "mp-term-line" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "mp-term-line",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "mp-term-mid",
							children: translate("auto.components.mobile.slides.TerminalSlide.e75112c834", "I've replaced the pair-scan slide with a high-fidelity")
						})
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "mp-term-line",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "mp-term-mid",
							children: translate("auto.components.mobile.slides.TerminalSlide.aa64b519c6", "terminal screen. Tokyonight palette, Menlo, real claude")
						})
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "mp-term-line",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "mp-term-mid",
							children: translate("auto.components.mobile.slides.TerminalSlide.58a9ee6003", "tool-call formatting. Want me to add the diff next?")
						})
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "mp-term-line" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
						className: "mp-term-line",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "mp-term-prompt",
								children: "›"
							}),
							" ",
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "mp-term-cursor" })
						]
					})
				]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "mp-accessory-bar",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "mp-accessory-content",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-accessory-key is-icon",
							"aria-label": translate("auto.components.mobile.slides.TerminalSlide.985373052e", "Switch to phone mode"),
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PhoneIcon, {})
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-accessory-key",
							children: translate("auto.components.mobile.slides.TerminalSlide.fa22927f13", "Paste")
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-accessory-key",
							children: translate("auto.components.mobile.slides.TerminalSlide.4930eaaae7", "Esc")
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-accessory-key",
							children: translate("auto.components.mobile.slides.TerminalSlide.53ff909568", "Tab")
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-accessory-key",
							children: "⌫"
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-accessory-key",
							children: "↑"
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-accessory-key",
							children: "↓"
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-accessory-key",
							children: "←"
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-accessory-key",
							children: "→"
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mp-accessory-key",
							children: translate("auto.components.mobile.slides.TerminalSlide.817090af40", "Ctrl+C")
						})
					]
				})
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "mp-input-bar",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "mp-text-input",
						children: translate("auto.components.mobile.slides.TerminalSlide.29f2d13839", "Type a command…")
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "mp-round-button",
						"aria-label": translate("auto.components.mobile.slides.TerminalSlide.69334b4b10", "Voice dictation"),
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MicIcon, {})
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "mp-round-button",
						"aria-label": translate("auto.components.mobile.slides.TerminalSlide.0bb39f8fe6", "Send"),
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowUpIcon, {})
					})
				]
			})
		]
	});
}
function ChevronLeftIcon() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("svg", {
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m15 18-6-6 6-6" })
	});
}
function BranchIcon() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", {
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", {
				cx: "6",
				cy: "3",
				r: "2.5"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", {
				cx: "6",
				cy: "21",
				r: "2.5"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("circle", {
				cx: "18",
				cy: "12",
				r: "2.5"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M6 5.5v13" }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M18 9.5a6 6 0 0 0-6-6" })
		]
	});
}
function FolderIcon() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("svg", {
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M4 4h6l2 2h8a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2Z" })
	});
}
function FileIcon() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", {
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M14 2v6h6" })]
	});
}
function PlusIcon() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", {
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M5 12h14" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M12 5v14" })]
	});
}
function PhoneIcon() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", {
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", {
			x: "5",
			y: "2",
			width: "14",
			height: "20",
			rx: "2",
			ry: "2"
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M12 18h.01" })]
	});
}
function MicIcon() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", {
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("rect", {
				x: "9",
				y: "2",
				width: "6",
				height: "12",
				rx: "3"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M19 10v2a7 7 0 0 1-14 0v-2" }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("line", {
				x1: "12",
				y1: "19",
				x2: "12",
				y2: "22"
			})
		]
	});
}
function ArrowUpIcon() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", {
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M12 19V5" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "m5 12 7-7 7 7" })]
	});
}
var DWELL_MS = 4500;
var TAP_BEFORE_PUSH_MS = 240;
function PhoneCarousel() {
	const [activeIdx, setActiveIdx] = (0, import_react.useState)(0);
	const [phase, setPhase] = (0, import_react.useState)("normal");
	const [tappingSlide, setTappingSlide] = (0, import_react.useState)(null);
	const containerRef = (0, import_react.useRef)(null);
	(0, import_react.useEffect)(() => {
		if (typeof window === "undefined") return;
		if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
		let cancelled = false;
		let dwellTimer = null;
		let tapTimer = null;
		let advanceTimer = null;
		let resetTimer = null;
		const schedule = (idx) => {
			dwellTimer = setTimeout(() => {
				if (cancelled) return;
				if (idx < 2) {
					setTappingSlide(idx);
					tapTimer = setTimeout(() => {
						if (cancelled) return;
						setTappingSlide(null);
					}, 320);
					advanceTimer = setTimeout(() => {
						if (cancelled) return;
						const next = idx + 1;
						setActiveIdx(next);
						schedule(next);
					}, TAP_BEFORE_PUSH_MS);
				} else {
					setPhase("reset");
					setActiveIdx(0);
					resetTimer = setTimeout(() => {
						if (cancelled) return;
						setPhase("normal");
						schedule(0);
					}, 30);
				}
			}, DWELL_MS);
		};
		schedule(0);
		return () => {
			cancelled = true;
			if (dwellTimer) clearTimeout(dwellTimer);
			if (tapTimer) clearTimeout(tapTimer);
			if (advanceTimer) clearTimeout(advanceTimer);
			if (resetTimer) clearTimeout(resetTimer);
		};
	}, []);
	(0, import_react.useEffect)(() => {
		if (phase !== "reset") return;
		const id = requestAnimationFrame(() => {
			containerRef.current?.offsetHeight;
		});
		return () => cancelAnimationFrame(id);
	}, [phase]);
	const slideClass = (idx) => cn("mp-screen-slide", phase === "reset" && "is-reset", idx === activeIdx && "is-active", idx < activeIdx && "is-past");
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "mp-phone-frame",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "mp-phone-screen",
			ref: containerRef,
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: slideClass(0),
					role: "img",
					"aria-label": translate("auto.components.mobile.PhoneCarousel.89c7713645", "Orca Mobile home screen"),
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(HomeSlide, { tapping: tappingSlide === 0 })
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: slideClass(1),
					role: "img",
					"aria-label": translate("auto.components.mobile.PhoneCarousel.93217b41c1", "Worktree list"),
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorktreeListSlide, { tapping: tappingSlide === 1 })
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: slideClass(2),
					role: "img",
					"aria-label": translate("auto.components.mobile.PhoneCarousel.96d651cb87", "Terminal session"),
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TerminalSlide, {})
				})
			]
		})
	});
}
function MobilePageContent({ closeMobilePage, copyInstallUrl, copyPairingCode, devices, enterFlow, generatePairing, canGeneratePairing, handleAddressChange, customAddresses, selectedAddressIsCustom, onCustomAddressSelect, onCustomAddressRemove, beforeCustomAddressChange, handleBack, handleContinue, installQrUrl, iosChannel, setIosChannel, loadNetworkInterfaces, networkInterfaces, openInstallUrl, pairAnotherDevice, pairLoading, connectionMode, handleConnectionModeChange, pairQrDataUrl, pairingUrl, pairingQrError, relayMintFailure, onUseLan, onRetryRelay, onCopyRelayDiagnostics, platform, refreshingNetworkInterfaces, revokeDevice, revokingDeviceIds, selectedAddress, setPlatform, showMobileButton, showPairedDevices, stage, stepIdx, toggleMobileSidebarButton }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "mobile-page-root scrollbar-sleek",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(MobilePageToolbar, {
			showMobileButton,
			onClose: closeMobilePage,
			onToggleMobileSidebarButton: toggleMobileSidebarButton
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", {
			className: "mp-hero",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "mp-hero-copy",
				children: stage === null ? null : stage === "intro" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(HeroIntro, { onStart: enterFlow }) : stage === "paired" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(HeroPaired, {
					devices,
					onPairAnother: pairAnotherDevice,
					onRevoke: (id) => revokeDevice(id),
					revokingDeviceIds
				}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(HeroFlow, {
					stepIdx,
					platform,
					onPlatformChange: setPlatform,
					installQrUrl,
					installCopy: getInstallCopy(platform, iosChannel),
					iosChannel,
					onIosChannelChange: setIosChannel,
					onOpenInstallUrl: openInstallUrl,
					onCopyInstallUrl: copyInstallUrl,
					pairQrDataUrl,
					pairingUrl,
					pairingQrError,
					relayMintFailure,
					onUseLan,
					onRetryRelay,
					onCopyRelayDiagnostics,
					pairLoading,
					connectionMode,
					onConnectionModeChange: handleConnectionModeChange,
					onRegeneratePairing: () => generatePairing(true),
					canGeneratePairing,
					onCopyPairingCode: copyPairingCode,
					networkInterfaces,
					customAddresses,
					selectedAddress,
					selectedAddressIsCustom,
					onSelectedAddressChange: handleAddressChange,
					onCustomAddressSelect,
					onCustomAddressRemove,
					beforeCustomAddressChange,
					onRefreshNetworkInterfaces: loadNetworkInterfaces,
					refreshingNetworkInterfaces,
					onBack: handleBack,
					onContinue: handleContinue,
					onDone: devices.length > 0 ? () => showPairedDevices(devices.length) : void 0
				})
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "mp-stage",
				"aria-label": translate("auto.components.mobile.MobilePage.e17393c6a3", "Phone preview"),
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PhoneCarousel, {})
			})]
		})]
	});
}
var require_can_promise = /* @__PURE__ */ __commonJSMin(((exports, module) => {
	module.exports = function() {
		return typeof Promise === "function" && Promise.prototype && Promise.prototype.then;
	};
}));
var require_utils$1 = /* @__PURE__ */ __commonJSMin(((exports) => {
	var toSJISFunction;
	var CODEWORDS_COUNT = [
		0,
		26,
		44,
		70,
		100,
		134,
		172,
		196,
		242,
		292,
		346,
		404,
		466,
		532,
		581,
		655,
		733,
		815,
		901,
		991,
		1085,
		1156,
		1258,
		1364,
		1474,
		1588,
		1706,
		1828,
		1921,
		2051,
		2185,
		2323,
		2465,
		2611,
		2761,
		2876,
		3034,
		3196,
		3362,
		3532,
		3706
	];
	exports.getSymbolSize = function getSymbolSize$2(version) {
		if (!version) throw new Error("\"version\" cannot be null or undefined");
		if (version < 1 || version > 40) throw new Error("\"version\" should be in range from 1 to 40");
		return version * 4 + 17;
	};
	exports.getSymbolTotalCodewords = function getSymbolTotalCodewords(version) {
		return CODEWORDS_COUNT[version];
	};
	exports.getBCHDigit = function(data) {
		let digit = 0;
		while (data !== 0) {
			digit++;
			data >>>= 1;
		}
		return digit;
	};
	exports.setToSJISFunction = function setToSJISFunction(f) {
		if (typeof f !== "function") throw new Error("\"toSJISFunc\" is not a valid function.");
		toSJISFunction = f;
	};
	exports.isKanjiModeEnabled = function() {
		return typeof toSJISFunction !== "undefined";
	};
	exports.toSJIS = function toSJIS(kanji$1) {
		return toSJISFunction(kanji$1);
	};
}));
var require_error_correction_level = /* @__PURE__ */ __commonJSMin(((exports) => {
	exports.L = { bit: 1 };
	exports.M = { bit: 0 };
	exports.Q = { bit: 3 };
	exports.H = { bit: 2 };
	function fromString$1(string) {
		if (typeof string !== "string") throw new Error("Param is not a string");
		switch (string.toLowerCase()) {
			case "l":
			case "low": return exports.L;
			case "m":
			case "medium": return exports.M;
			case "q":
			case "quartile": return exports.Q;
			case "h":
			case "high": return exports.H;
			default: throw new Error("Unknown EC Level: " + string);
		}
	}
	exports.isValid = function isValid(level) {
		return level && typeof level.bit !== "undefined" && level.bit >= 0 && level.bit < 4;
	};
	exports.from = function from(value, defaultValue) {
		if (exports.isValid(value)) return value;
		try {
			return fromString$1(value);
		} catch (e) {
			return defaultValue;
		}
	};
}));
var require_bit_buffer = /* @__PURE__ */ __commonJSMin(((exports, module) => {
	function BitBuffer$1() {
		this.buffer = [];
		this.length = 0;
	}
	BitBuffer$1.prototype = {
		get: function(index) {
			const bufIndex = Math.floor(index / 8);
			return (this.buffer[bufIndex] >>> 7 - index % 8 & 1) === 1;
		},
		put: function(num, length) {
			for (let i = 0; i < length; i++) this.putBit((num >>> length - i - 1 & 1) === 1);
		},
		getLengthInBits: function() {
			return this.length;
		},
		putBit: function(bit) {
			const bufIndex = Math.floor(this.length / 8);
			if (this.buffer.length <= bufIndex) this.buffer.push(0);
			if (bit) this.buffer[bufIndex] |= 128 >>> this.length % 8;
			this.length++;
		}
	};
	module.exports = BitBuffer$1;
}));
var require_bit_matrix = /* @__PURE__ */ __commonJSMin(((exports, module) => {
	function BitMatrix$1(size) {
		if (!size || size < 1) throw new Error("BitMatrix size must be defined and greater than 0");
		this.size = size;
		this.data = new Uint8Array(size * size);
		this.reservedBit = new Uint8Array(size * size);
	}
	BitMatrix$1.prototype.set = function(row, col, value, reserved) {
		const index = row * this.size + col;
		this.data[index] = value;
		if (reserved) this.reservedBit[index] = true;
	};
	BitMatrix$1.prototype.get = function(row, col) {
		return this.data[row * this.size + col];
	};
	BitMatrix$1.prototype.xor = function(row, col, value) {
		this.data[row * this.size + col] ^= value;
	};
	BitMatrix$1.prototype.isReserved = function(row, col) {
		return this.reservedBit[row * this.size + col];
	};
	module.exports = BitMatrix$1;
}));
var require_alignment_pattern = /* @__PURE__ */ __commonJSMin(((exports) => {
	var getSymbolSize$1 = require_utils$1().getSymbolSize;
	exports.getRowColCoords = function getRowColCoords(version) {
		if (version === 1) return [];
		const posCount = Math.floor(version / 7) + 2;
		const size = getSymbolSize$1(version);
		const intervals = size === 145 ? 26 : Math.ceil((size - 13) / (2 * posCount - 2)) * 2;
		const positions = [size - 7];
		for (let i = 1; i < posCount - 1; i++) positions[i] = positions[i - 1] - intervals;
		positions.push(6);
		return positions.reverse();
	};
	exports.getPositions = function getPositions(version) {
		const coords = [];
		const pos = exports.getRowColCoords(version);
		const posLength = pos.length;
		for (let i = 0; i < posLength; i++) for (let j = 0; j < posLength; j++) {
			if (i === 0 && j === 0 || i === 0 && j === posLength - 1 || i === posLength - 1 && j === 0) continue;
			coords.push([pos[i], pos[j]]);
		}
		return coords;
	};
}));
var require_finder_pattern = /* @__PURE__ */ __commonJSMin(((exports) => {
	var getSymbolSize = require_utils$1().getSymbolSize;
	var FINDER_PATTERN_SIZE = 7;
	exports.getPositions = function getPositions(version) {
		const size = getSymbolSize(version);
		return [
			[0, 0],
			[size - FINDER_PATTERN_SIZE, 0],
			[0, size - FINDER_PATTERN_SIZE]
		];
	};
}));
var require_mask_pattern = /* @__PURE__ */ __commonJSMin(((exports) => {
	exports.Patterns = {
		PATTERN000: 0,
		PATTERN001: 1,
		PATTERN010: 2,
		PATTERN011: 3,
		PATTERN100: 4,
		PATTERN101: 5,
		PATTERN110: 6,
		PATTERN111: 7
	};
	var PenaltyScores = {
		N1: 3,
		N2: 3,
		N3: 40,
		N4: 10
	};
	exports.isValid = function isValid(mask) {
		return mask != null && mask !== "" && !isNaN(mask) && mask >= 0 && mask <= 7;
	};
	exports.from = function from(value) {
		return exports.isValid(value) ? parseInt(value, 10) : void 0;
	};
	exports.getPenaltyN1 = function getPenaltyN1(data) {
		const size = data.size;
		let points = 0;
		let sameCountCol = 0;
		let sameCountRow = 0;
		let lastCol = null;
		let lastRow = null;
		for (let row = 0; row < size; row++) {
			sameCountCol = sameCountRow = 0;
			lastCol = lastRow = null;
			for (let col = 0; col < size; col++) {
				let module$1 = data.get(row, col);
				if (module$1 === lastCol) sameCountCol++;
				else {
					if (sameCountCol >= 5) points += PenaltyScores.N1 + (sameCountCol - 5);
					lastCol = module$1;
					sameCountCol = 1;
				}
				module$1 = data.get(col, row);
				if (module$1 === lastRow) sameCountRow++;
				else {
					if (sameCountRow >= 5) points += PenaltyScores.N1 + (sameCountRow - 5);
					lastRow = module$1;
					sameCountRow = 1;
				}
			}
			if (sameCountCol >= 5) points += PenaltyScores.N1 + (sameCountCol - 5);
			if (sameCountRow >= 5) points += PenaltyScores.N1 + (sameCountRow - 5);
		}
		return points;
	};
	exports.getPenaltyN2 = function getPenaltyN2(data) {
		const size = data.size;
		let points = 0;
		for (let row = 0; row < size - 1; row++) for (let col = 0; col < size - 1; col++) {
			const last = data.get(row, col) + data.get(row, col + 1) + data.get(row + 1, col) + data.get(row + 1, col + 1);
			if (last === 4 || last === 0) points++;
		}
		return points * PenaltyScores.N2;
	};
	exports.getPenaltyN3 = function getPenaltyN3(data) {
		const size = data.size;
		let points = 0;
		let bitsCol = 0;
		let bitsRow = 0;
		for (let row = 0; row < size; row++) {
			bitsCol = bitsRow = 0;
			for (let col = 0; col < size; col++) {
				bitsCol = bitsCol << 1 & 2047 | data.get(row, col);
				if (col >= 10 && (bitsCol === 1488 || bitsCol === 93)) points++;
				bitsRow = bitsRow << 1 & 2047 | data.get(col, row);
				if (col >= 10 && (bitsRow === 1488 || bitsRow === 93)) points++;
			}
		}
		return points * PenaltyScores.N3;
	};
	exports.getPenaltyN4 = function getPenaltyN4(data) {
		let darkCount = 0;
		const modulesCount = data.data.length;
		for (let i = 0; i < modulesCount; i++) darkCount += data.data[i];
		return Math.abs(Math.ceil(darkCount * 100 / modulesCount / 5) - 10) * PenaltyScores.N4;
	};
	function getMaskAt(maskPattern, i, j) {
		switch (maskPattern) {
			case exports.Patterns.PATTERN000: return (i + j) % 2 === 0;
			case exports.Patterns.PATTERN001: return i % 2 === 0;
			case exports.Patterns.PATTERN010: return j % 3 === 0;
			case exports.Patterns.PATTERN011: return (i + j) % 3 === 0;
			case exports.Patterns.PATTERN100: return (Math.floor(i / 2) + Math.floor(j / 3)) % 2 === 0;
			case exports.Patterns.PATTERN101: return i * j % 2 + i * j % 3 === 0;
			case exports.Patterns.PATTERN110: return (i * j % 2 + i * j % 3) % 2 === 0;
			case exports.Patterns.PATTERN111: return (i * j % 3 + (i + j) % 2) % 2 === 0;
			default: throw new Error("bad maskPattern:" + maskPattern);
		}
	}
	exports.applyMask = function applyMask(pattern, data) {
		const size = data.size;
		for (let col = 0; col < size; col++) for (let row = 0; row < size; row++) {
			if (data.isReserved(row, col)) continue;
			data.xor(row, col, getMaskAt(pattern, row, col));
		}
	};
	exports.getBestMask = function getBestMask(data, setupFormatFunc) {
		const numPatterns = Object.keys(exports.Patterns).length;
		let bestPattern = 0;
		let lowerPenalty = Infinity;
		for (let p = 0; p < numPatterns; p++) {
			setupFormatFunc(p);
			exports.applyMask(p, data);
			const penalty = exports.getPenaltyN1(data) + exports.getPenaltyN2(data) + exports.getPenaltyN3(data) + exports.getPenaltyN4(data);
			exports.applyMask(p, data);
			if (penalty < lowerPenalty) {
				lowerPenalty = penalty;
				bestPattern = p;
			}
		}
		return bestPattern;
	};
}));
var require_error_correction_code = /* @__PURE__ */ __commonJSMin(((exports) => {
	var ECLevel$2 = require_error_correction_level();
	var EC_BLOCKS_TABLE = [
		1,
		1,
		1,
		1,
		1,
		1,
		1,
		1,
		1,
		1,
		2,
		2,
		1,
		2,
		2,
		4,
		1,
		2,
		4,
		4,
		2,
		4,
		4,
		4,
		2,
		4,
		6,
		5,
		2,
		4,
		6,
		6,
		2,
		5,
		8,
		8,
		4,
		5,
		8,
		8,
		4,
		5,
		8,
		11,
		4,
		8,
		10,
		11,
		4,
		9,
		12,
		16,
		4,
		9,
		16,
		16,
		6,
		10,
		12,
		18,
		6,
		10,
		17,
		16,
		6,
		11,
		16,
		19,
		6,
		13,
		18,
		21,
		7,
		14,
		21,
		25,
		8,
		16,
		20,
		25,
		8,
		17,
		23,
		25,
		9,
		17,
		23,
		34,
		9,
		18,
		25,
		30,
		10,
		20,
		27,
		32,
		12,
		21,
		29,
		35,
		12,
		23,
		34,
		37,
		12,
		25,
		34,
		40,
		13,
		26,
		35,
		42,
		14,
		28,
		38,
		45,
		15,
		29,
		40,
		48,
		16,
		31,
		43,
		51,
		17,
		33,
		45,
		54,
		18,
		35,
		48,
		57,
		19,
		37,
		51,
		60,
		19,
		38,
		53,
		63,
		20,
		40,
		56,
		66,
		21,
		43,
		59,
		70,
		22,
		45,
		62,
		74,
		24,
		47,
		65,
		77,
		25,
		49,
		68,
		81
	];
	var EC_CODEWORDS_TABLE = [
		7,
		10,
		13,
		17,
		10,
		16,
		22,
		28,
		15,
		26,
		36,
		44,
		20,
		36,
		52,
		64,
		26,
		48,
		72,
		88,
		36,
		64,
		96,
		112,
		40,
		72,
		108,
		130,
		48,
		88,
		132,
		156,
		60,
		110,
		160,
		192,
		72,
		130,
		192,
		224,
		80,
		150,
		224,
		264,
		96,
		176,
		260,
		308,
		104,
		198,
		288,
		352,
		120,
		216,
		320,
		384,
		132,
		240,
		360,
		432,
		144,
		280,
		408,
		480,
		168,
		308,
		448,
		532,
		180,
		338,
		504,
		588,
		196,
		364,
		546,
		650,
		224,
		416,
		600,
		700,
		224,
		442,
		644,
		750,
		252,
		476,
		690,
		816,
		270,
		504,
		750,
		900,
		300,
		560,
		810,
		960,
		312,
		588,
		870,
		1050,
		336,
		644,
		952,
		1110,
		360,
		700,
		1020,
		1200,
		390,
		728,
		1050,
		1260,
		420,
		784,
		1140,
		1350,
		450,
		812,
		1200,
		1440,
		480,
		868,
		1290,
		1530,
		510,
		924,
		1350,
		1620,
		540,
		980,
		1440,
		1710,
		570,
		1036,
		1530,
		1800,
		570,
		1064,
		1590,
		1890,
		600,
		1120,
		1680,
		1980,
		630,
		1204,
		1770,
		2100,
		660,
		1260,
		1860,
		2220,
		720,
		1316,
		1950,
		2310,
		750,
		1372,
		2040,
		2430
	];
	exports.getBlocksCount = function getBlocksCount(version, errorCorrectionLevel) {
		switch (errorCorrectionLevel) {
			case ECLevel$2.L: return EC_BLOCKS_TABLE[(version - 1) * 4 + 0];
			case ECLevel$2.M: return EC_BLOCKS_TABLE[(version - 1) * 4 + 1];
			case ECLevel$2.Q: return EC_BLOCKS_TABLE[(version - 1) * 4 + 2];
			case ECLevel$2.H: return EC_BLOCKS_TABLE[(version - 1) * 4 + 3];
			default: return;
		}
	};
	exports.getTotalCodewordsCount = function getTotalCodewordsCount(version, errorCorrectionLevel) {
		switch (errorCorrectionLevel) {
			case ECLevel$2.L: return EC_CODEWORDS_TABLE[(version - 1) * 4 + 0];
			case ECLevel$2.M: return EC_CODEWORDS_TABLE[(version - 1) * 4 + 1];
			case ECLevel$2.Q: return EC_CODEWORDS_TABLE[(version - 1) * 4 + 2];
			case ECLevel$2.H: return EC_CODEWORDS_TABLE[(version - 1) * 4 + 3];
			default: return;
		}
	};
}));
var require_galois_field = /* @__PURE__ */ __commonJSMin(((exports) => {
	var EXP_TABLE = new Uint8Array(512);
	var LOG_TABLE = new Uint8Array(256);
	(function initTables() {
		let x = 1;
		for (let i = 0; i < 255; i++) {
			EXP_TABLE[i] = x;
			LOG_TABLE[x] = i;
			x <<= 1;
			if (x & 256) x ^= 285;
		}
		for (let i = 255; i < 512; i++) EXP_TABLE[i] = EXP_TABLE[i - 255];
	})();
	exports.log = function log(n) {
		if (n < 1) throw new Error("log(" + n + ")");
		return LOG_TABLE[n];
	};
	exports.exp = function exp(n) {
		return EXP_TABLE[n];
	};
	exports.mul = function mul(x, y) {
		if (x === 0 || y === 0) return 0;
		return EXP_TABLE[LOG_TABLE[x] + LOG_TABLE[y]];
	};
}));
var require_polynomial = /* @__PURE__ */ __commonJSMin(((exports) => {
	var GF = require_galois_field();
	exports.mul = function mul(p1, p2) {
		const coeff = new Uint8Array(p1.length + p2.length - 1);
		for (let i = 0; i < p1.length; i++) for (let j = 0; j < p2.length; j++) coeff[i + j] ^= GF.mul(p1[i], p2[j]);
		return coeff;
	};
	exports.mod = function mod(divident, divisor) {
		let result = new Uint8Array(divident);
		while (result.length - divisor.length >= 0) {
			const coeff = result[0];
			for (let i = 0; i < divisor.length; i++) result[i] ^= GF.mul(divisor[i], coeff);
			let offset = 0;
			while (offset < result.length && result[offset] === 0) offset++;
			result = result.slice(offset);
		}
		return result;
	};
	exports.generateECPolynomial = function generateECPolynomial(degree) {
		let poly = new Uint8Array([1]);
		for (let i = 0; i < degree; i++) poly = exports.mul(poly, new Uint8Array([1, GF.exp(i)]));
		return poly;
	};
}));
var require_reed_solomon_encoder = /* @__PURE__ */ __commonJSMin(((exports, module) => {
	var Polynomial = require_polynomial();
	function ReedSolomonEncoder$1(degree) {
		this.genPoly = void 0;
		this.degree = degree;
		if (this.degree) this.initialize(this.degree);
	}
	ReedSolomonEncoder$1.prototype.initialize = function initialize(degree) {
		this.degree = degree;
		this.genPoly = Polynomial.generateECPolynomial(this.degree);
	};
	ReedSolomonEncoder$1.prototype.encode = function encode(data) {
		if (!this.genPoly) throw new Error("Encoder not initialized");
		const paddedData = new Uint8Array(data.length + this.degree);
		paddedData.set(data);
		const remainder = Polynomial.mod(paddedData, this.genPoly);
		const start = this.degree - remainder.length;
		if (start > 0) {
			const buff = new Uint8Array(this.degree);
			buff.set(remainder, start);
			return buff;
		}
		return remainder;
	};
	module.exports = ReedSolomonEncoder$1;
}));
var require_version_check = /* @__PURE__ */ __commonJSMin(((exports) => {
	exports.isValid = function isValid(version) {
		return !isNaN(version) && version >= 1 && version <= 40;
	};
}));
var require_regex = /* @__PURE__ */ __commonJSMin(((exports) => {
	var numeric = "[0-9]+";
	var alphanumeric = "[A-Z $%*+\\-./:]+";
	var kanji = "(?:[u3000-u303F]|[u3040-u309F]|[u30A0-u30FF]|[uFF00-uFFEF]|[u4E00-u9FAF]|[u2605-u2606]|[u2190-u2195]|u203B|[u2010u2015u2018u2019u2025u2026u201Cu201Du2225u2260]|[u0391-u0451]|[u00A7u00A8u00B1u00B4u00D7u00F7])+";
	kanji = kanji.replace(/u/g, "\\u");
	var byte = "(?:(?![A-Z0-9 $%*+\\-./:]|" + kanji + ")(?:.|[\r\n]))+";
	exports.KANJI = new RegExp(kanji, "g");
	exports.BYTE_KANJI = new RegExp("[^A-Z0-9 $%*+\\-./:]+", "g");
	exports.BYTE = new RegExp(byte, "g");
	exports.NUMERIC = new RegExp(numeric, "g");
	exports.ALPHANUMERIC = new RegExp(alphanumeric, "g");
	var TEST_KANJI = /* @__PURE__ */ new RegExp("^" + kanji + "$");
	var TEST_NUMERIC = /* @__PURE__ */ new RegExp("^" + numeric + "$");
	var TEST_ALPHANUMERIC = /* @__PURE__ */ new RegExp("^[A-Z0-9 $%*+\\-./:]+$");
	exports.testKanji = function testKanji(str) {
		return TEST_KANJI.test(str);
	};
	exports.testNumeric = function testNumeric(str) {
		return TEST_NUMERIC.test(str);
	};
	exports.testAlphanumeric = function testAlphanumeric(str) {
		return TEST_ALPHANUMERIC.test(str);
	};
}));
var require_mode = /* @__PURE__ */ __commonJSMin(((exports) => {
	var VersionCheck$1 = require_version_check();
	var Regex$1 = require_regex();
	exports.NUMERIC = {
		id: "Numeric",
		bit: 1,
		ccBits: [
			10,
			12,
			14
		]
	};
	exports.ALPHANUMERIC = {
		id: "Alphanumeric",
		bit: 2,
		ccBits: [
			9,
			11,
			13
		]
	};
	exports.BYTE = {
		id: "Byte",
		bit: 4,
		ccBits: [
			8,
			16,
			16
		]
	};
	exports.KANJI = {
		id: "Kanji",
		bit: 8,
		ccBits: [
			8,
			10,
			12
		]
	};
	exports.MIXED = { bit: -1 };
	exports.getCharCountIndicator = function getCharCountIndicator(mode, version) {
		if (!mode.ccBits) throw new Error("Invalid mode: " + mode);
		if (!VersionCheck$1.isValid(version)) throw new Error("Invalid version: " + version);
		if (version >= 1 && version < 10) return mode.ccBits[0];
		else if (version < 27) return mode.ccBits[1];
		return mode.ccBits[2];
	};
	exports.getBestModeForData = function getBestModeForData(dataStr) {
		if (Regex$1.testNumeric(dataStr)) return exports.NUMERIC;
		else if (Regex$1.testAlphanumeric(dataStr)) return exports.ALPHANUMERIC;
		else if (Regex$1.testKanji(dataStr)) return exports.KANJI;
		else return exports.BYTE;
	};
	exports.toString = function toString(mode) {
		if (mode && mode.id) return mode.id;
		throw new Error("Invalid mode");
	};
	exports.isValid = function isValid(mode) {
		return mode && mode.bit && mode.ccBits;
	};
	function fromString(string) {
		if (typeof string !== "string") throw new Error("Param is not a string");
		switch (string.toLowerCase()) {
			case "numeric": return exports.NUMERIC;
			case "alphanumeric": return exports.ALPHANUMERIC;
			case "kanji": return exports.KANJI;
			case "byte": return exports.BYTE;
			default: throw new Error("Unknown mode: " + string);
		}
	}
	exports.from = function from(value, defaultValue) {
		if (exports.isValid(value)) return value;
		try {
			return fromString(value);
		} catch (e) {
			return defaultValue;
		}
	};
}));
var require_version = /* @__PURE__ */ __commonJSMin(((exports) => {
	var Utils$6 = require_utils$1();
	var ECCode$1 = require_error_correction_code();
	var ECLevel$1 = require_error_correction_level();
	var Mode$6 = require_mode();
	var VersionCheck = require_version_check();
	var G18 = 7973;
	var G18_BCH = Utils$6.getBCHDigit(G18);
	function getBestVersionForDataLength(mode, length, errorCorrectionLevel) {
		for (let currentVersion = 1; currentVersion <= 40; currentVersion++) if (length <= exports.getCapacity(currentVersion, errorCorrectionLevel, mode)) return currentVersion;
	}
	function getReservedBitsCount(mode, version) {
		return Mode$6.getCharCountIndicator(mode, version) + 4;
	}
	function getTotalBitsFromDataArray(segments, version) {
		let totalBits = 0;
		segments.forEach(function(data) {
			const reservedBits = getReservedBitsCount(data.mode, version);
			totalBits += reservedBits + data.getBitsLength();
		});
		return totalBits;
	}
	function getBestVersionForMixedData(segments, errorCorrectionLevel) {
		for (let currentVersion = 1; currentVersion <= 40; currentVersion++) if (getTotalBitsFromDataArray(segments, currentVersion) <= exports.getCapacity(currentVersion, errorCorrectionLevel, Mode$6.MIXED)) return currentVersion;
	}
	exports.from = function from(value, defaultValue) {
		if (VersionCheck.isValid(value)) return parseInt(value, 10);
		return defaultValue;
	};
	exports.getCapacity = function getCapacity(version, errorCorrectionLevel, mode) {
		if (!VersionCheck.isValid(version)) throw new Error("Invalid QR Code version");
		if (typeof mode === "undefined") mode = Mode$6.BYTE;
		const dataTotalCodewordsBits = (Utils$6.getSymbolTotalCodewords(version) - ECCode$1.getTotalCodewordsCount(version, errorCorrectionLevel)) * 8;
		if (mode === Mode$6.MIXED) return dataTotalCodewordsBits;
		const usableBits = dataTotalCodewordsBits - getReservedBitsCount(mode, version);
		switch (mode) {
			case Mode$6.NUMERIC: return Math.floor(usableBits / 10 * 3);
			case Mode$6.ALPHANUMERIC: return Math.floor(usableBits / 11 * 2);
			case Mode$6.KANJI: return Math.floor(usableBits / 13);
			case Mode$6.BYTE:
			default: return Math.floor(usableBits / 8);
		}
	};
	exports.getBestVersionForData = function getBestVersionForData(data, errorCorrectionLevel) {
		let seg;
		const ecl = ECLevel$1.from(errorCorrectionLevel, ECLevel$1.M);
		if (Array.isArray(data)) {
			if (data.length > 1) return getBestVersionForMixedData(data, ecl);
			if (data.length === 0) return 1;
			seg = data[0];
		} else seg = data;
		return getBestVersionForDataLength(seg.mode, seg.getLength(), ecl);
	};
	exports.getEncodedBits = function getEncodedBits(version) {
		if (!VersionCheck.isValid(version) || version < 7) throw new Error("Invalid QR Code version");
		let d = version << 12;
		while (Utils$6.getBCHDigit(d) - G18_BCH >= 0) d ^= G18 << Utils$6.getBCHDigit(d) - G18_BCH;
		return version << 12 | d;
	};
}));
var require_format_info = /* @__PURE__ */ __commonJSMin(((exports) => {
	var Utils$5 = require_utils$1();
	var G15 = 1335;
	var G15_MASK = 21522;
	var G15_BCH = Utils$5.getBCHDigit(G15);
	exports.getEncodedBits = function getEncodedBits(errorCorrectionLevel, mask) {
		const data = errorCorrectionLevel.bit << 3 | mask;
		let d = data << 10;
		while (Utils$5.getBCHDigit(d) - G15_BCH >= 0) d ^= G15 << Utils$5.getBCHDigit(d) - G15_BCH;
		return (data << 10 | d) ^ G15_MASK;
	};
}));
var require_numeric_data = /* @__PURE__ */ __commonJSMin(((exports, module) => {
	var Mode$5 = require_mode();
	function NumericData$1(data) {
		this.mode = Mode$5.NUMERIC;
		this.data = data.toString();
	}
	NumericData$1.getBitsLength = function getBitsLength(length) {
		return 10 * Math.floor(length / 3) + (length % 3 ? length % 3 * 3 + 1 : 0);
	};
	NumericData$1.prototype.getLength = function getLength() {
		return this.data.length;
	};
	NumericData$1.prototype.getBitsLength = function getBitsLength() {
		return NumericData$1.getBitsLength(this.data.length);
	};
	NumericData$1.prototype.write = function write(bitBuffer) {
		let i, group, value;
		for (i = 0; i + 3 <= this.data.length; i += 3) {
			group = this.data.substr(i, 3);
			value = parseInt(group, 10);
			bitBuffer.put(value, 10);
		}
		const remainingNum = this.data.length - i;
		if (remainingNum > 0) {
			group = this.data.substr(i);
			value = parseInt(group, 10);
			bitBuffer.put(value, remainingNum * 3 + 1);
		}
	};
	module.exports = NumericData$1;
}));
var require_alphanumeric_data = /* @__PURE__ */ __commonJSMin(((exports, module) => {
	var Mode$4 = require_mode();
	var ALPHA_NUM_CHARS = [
		"0",
		"1",
		"2",
		"3",
		"4",
		"5",
		"6",
		"7",
		"8",
		"9",
		"A",
		"B",
		"C",
		"D",
		"E",
		"F",
		"G",
		"H",
		"I",
		"J",
		"K",
		"L",
		"M",
		"N",
		"O",
		"P",
		"Q",
		"R",
		"S",
		"T",
		"U",
		"V",
		"W",
		"X",
		"Y",
		"Z",
		" ",
		"$",
		"%",
		"*",
		"+",
		"-",
		".",
		"/",
		":"
	];
	function AlphanumericData$1(data) {
		this.mode = Mode$4.ALPHANUMERIC;
		this.data = data;
	}
	AlphanumericData$1.getBitsLength = function getBitsLength(length) {
		return 11 * Math.floor(length / 2) + 6 * (length % 2);
	};
	AlphanumericData$1.prototype.getLength = function getLength() {
		return this.data.length;
	};
	AlphanumericData$1.prototype.getBitsLength = function getBitsLength() {
		return AlphanumericData$1.getBitsLength(this.data.length);
	};
	AlphanumericData$1.prototype.write = function write(bitBuffer) {
		let i;
		for (i = 0; i + 2 <= this.data.length; i += 2) {
			let value = ALPHA_NUM_CHARS.indexOf(this.data[i]) * 45;
			value += ALPHA_NUM_CHARS.indexOf(this.data[i + 1]);
			bitBuffer.put(value, 11);
		}
		if (this.data.length % 2) bitBuffer.put(ALPHA_NUM_CHARS.indexOf(this.data[i]), 6);
	};
	module.exports = AlphanumericData$1;
}));
var require_byte_data = /* @__PURE__ */ __commonJSMin(((exports, module) => {
	var Mode$3 = require_mode();
	function ByteData$1(data) {
		this.mode = Mode$3.BYTE;
		if (typeof data === "string") this.data = new TextEncoder().encode(data);
		else this.data = new Uint8Array(data);
	}
	ByteData$1.getBitsLength = function getBitsLength(length) {
		return length * 8;
	};
	ByteData$1.prototype.getLength = function getLength() {
		return this.data.length;
	};
	ByteData$1.prototype.getBitsLength = function getBitsLength() {
		return ByteData$1.getBitsLength(this.data.length);
	};
	ByteData$1.prototype.write = function(bitBuffer) {
		for (let i = 0, l = this.data.length; i < l; i++) bitBuffer.put(this.data[i], 8);
	};
	module.exports = ByteData$1;
}));
var require_kanji_data = /* @__PURE__ */ __commonJSMin(((exports, module) => {
	var Mode$2 = require_mode();
	var Utils$4 = require_utils$1();
	function KanjiData$1(data) {
		this.mode = Mode$2.KANJI;
		this.data = data;
	}
	KanjiData$1.getBitsLength = function getBitsLength(length) {
		return length * 13;
	};
	KanjiData$1.prototype.getLength = function getLength() {
		return this.data.length;
	};
	KanjiData$1.prototype.getBitsLength = function getBitsLength() {
		return KanjiData$1.getBitsLength(this.data.length);
	};
	KanjiData$1.prototype.write = function(bitBuffer) {
		let i;
		for (i = 0; i < this.data.length; i++) {
			let value = Utils$4.toSJIS(this.data[i]);
			if (value >= 33088 && value <= 40956) value -= 33088;
			else if (value >= 57408 && value <= 60351) value -= 49472;
			else throw new Error("Invalid SJIS character: " + this.data[i] + "\nMake sure your charset is UTF-8");
			value = (value >>> 8 & 255) * 192 + (value & 255);
			bitBuffer.put(value, 13);
		}
	};
	module.exports = KanjiData$1;
}));
var require_dijkstra = /* @__PURE__ */ __commonJSMin(((exports, module) => {
	var dijkstra$1 = {
		single_source_shortest_paths: function(graph, s, d) {
			var predecessors = {};
			var costs = {};
			costs[s] = 0;
			var open = dijkstra$1.PriorityQueue.make();
			open.push(s, 0);
			var closest, u, v, cost_of_s_to_u, adjacent_nodes, cost_of_e, cost_of_s_to_u_plus_cost_of_e, cost_of_s_to_v, first_visit;
			while (!open.empty()) {
				closest = open.pop();
				u = closest.value;
				cost_of_s_to_u = closest.cost;
				adjacent_nodes = graph[u] || {};
				for (v in adjacent_nodes) if (adjacent_nodes.hasOwnProperty(v)) {
					cost_of_e = adjacent_nodes[v];
					cost_of_s_to_u_plus_cost_of_e = cost_of_s_to_u + cost_of_e;
					cost_of_s_to_v = costs[v];
					first_visit = typeof costs[v] === "undefined";
					if (first_visit || cost_of_s_to_v > cost_of_s_to_u_plus_cost_of_e) {
						costs[v] = cost_of_s_to_u_plus_cost_of_e;
						open.push(v, cost_of_s_to_u_plus_cost_of_e);
						predecessors[v] = u;
					}
				}
			}
			if (typeof d !== "undefined" && typeof costs[d] === "undefined") {
				var msg = [
					"Could not find a path from ",
					s,
					" to ",
					d,
					"."
				].join("");
				throw new Error(msg);
			}
			return predecessors;
		},
		extract_shortest_path_from_predecessor_list: function(predecessors, d) {
			var nodes = [];
			var u = d;
			while (u) {
				nodes.push(u);
				predecessors[u];
				u = predecessors[u];
			}
			nodes.reverse();
			return nodes;
		},
		find_path: function(graph, s, d) {
			var predecessors = dijkstra$1.single_source_shortest_paths(graph, s, d);
			return dijkstra$1.extract_shortest_path_from_predecessor_list(predecessors, d);
		},
		PriorityQueue: {
			make: function(opts) {
				var T = dijkstra$1.PriorityQueue, t = {}, key;
				opts = opts || {};
				for (key in T) if (T.hasOwnProperty(key)) t[key] = T[key];
				t.queue = [];
				t.sorter = opts.sorter || T.default_sorter;
				return t;
			},
			default_sorter: function(a, b) {
				return a.cost - b.cost;
			},
			push: function(value, cost) {
				var item = {
					value,
					cost
				};
				this.queue.push(item);
				this.queue.sort(this.sorter);
			},
			pop: function() {
				return this.queue.shift();
			},
			empty: function() {
				return this.queue.length === 0;
			}
		}
	};
	if (typeof module !== "undefined") module.exports = dijkstra$1;
}));
var require_segments = /* @__PURE__ */ __commonJSMin(((exports) => {
	var Mode$1 = require_mode();
	var NumericData = require_numeric_data();
	var AlphanumericData = require_alphanumeric_data();
	var ByteData = require_byte_data();
	var KanjiData = require_kanji_data();
	var Regex = require_regex();
	var Utils$3 = require_utils$1();
	var dijkstra = require_dijkstra();
	function getStringByteLength(str) {
		return unescape(encodeURIComponent(str)).length;
	}
	function getSegments(regex, mode, str) {
		const segments = [];
		let result;
		while ((result = regex.exec(str)) !== null) segments.push({
			data: result[0],
			index: result.index,
			mode,
			length: result[0].length
		});
		return segments;
	}
	function getSegmentsFromString(dataStr) {
		const numSegs = getSegments(Regex.NUMERIC, Mode$1.NUMERIC, dataStr);
		const alphaNumSegs = getSegments(Regex.ALPHANUMERIC, Mode$1.ALPHANUMERIC, dataStr);
		let byteSegs;
		let kanjiSegs;
		if (Utils$3.isKanjiModeEnabled()) {
			byteSegs = getSegments(Regex.BYTE, Mode$1.BYTE, dataStr);
			kanjiSegs = getSegments(Regex.KANJI, Mode$1.KANJI, dataStr);
		} else {
			byteSegs = getSegments(Regex.BYTE_KANJI, Mode$1.BYTE, dataStr);
			kanjiSegs = [];
		}
		return numSegs.concat(alphaNumSegs, byteSegs, kanjiSegs).sort(function(s1, s2) {
			return s1.index - s2.index;
		}).map(function(obj) {
			return {
				data: obj.data,
				mode: obj.mode,
				length: obj.length
			};
		});
	}
	function getSegmentBitsLength(length, mode) {
		switch (mode) {
			case Mode$1.NUMERIC: return NumericData.getBitsLength(length);
			case Mode$1.ALPHANUMERIC: return AlphanumericData.getBitsLength(length);
			case Mode$1.KANJI: return KanjiData.getBitsLength(length);
			case Mode$1.BYTE: return ByteData.getBitsLength(length);
		}
	}
	function mergeSegments(segs) {
		return segs.reduce(function(acc, curr) {
			const prevSeg = acc.length - 1 >= 0 ? acc[acc.length - 1] : null;
			if (prevSeg && prevSeg.mode === curr.mode) {
				acc[acc.length - 1].data += curr.data;
				return acc;
			}
			acc.push(curr);
			return acc;
		}, []);
	}
	function buildNodes(segs) {
		const nodes = [];
		for (let i = 0; i < segs.length; i++) {
			const seg = segs[i];
			switch (seg.mode) {
				case Mode$1.NUMERIC:
					nodes.push([
						seg,
						{
							data: seg.data,
							mode: Mode$1.ALPHANUMERIC,
							length: seg.length
						},
						{
							data: seg.data,
							mode: Mode$1.BYTE,
							length: seg.length
						}
					]);
					break;
				case Mode$1.ALPHANUMERIC:
					nodes.push([seg, {
						data: seg.data,
						mode: Mode$1.BYTE,
						length: seg.length
					}]);
					break;
				case Mode$1.KANJI:
					nodes.push([seg, {
						data: seg.data,
						mode: Mode$1.BYTE,
						length: getStringByteLength(seg.data)
					}]);
					break;
				case Mode$1.BYTE: nodes.push([{
					data: seg.data,
					mode: Mode$1.BYTE,
					length: getStringByteLength(seg.data)
				}]);
			}
		}
		return nodes;
	}
	function buildGraph(nodes, version) {
		const table = {};
		const graph = { start: {} };
		let prevNodeIds = ["start"];
		for (let i = 0; i < nodes.length; i++) {
			const nodeGroup = nodes[i];
			const currentNodeIds = [];
			for (let j = 0; j < nodeGroup.length; j++) {
				const node = nodeGroup[j];
				const key = "" + i + j;
				currentNodeIds.push(key);
				table[key] = {
					node,
					lastCount: 0
				};
				graph[key] = {};
				for (let n = 0; n < prevNodeIds.length; n++) {
					const prevNodeId = prevNodeIds[n];
					if (table[prevNodeId] && table[prevNodeId].node.mode === node.mode) {
						graph[prevNodeId][key] = getSegmentBitsLength(table[prevNodeId].lastCount + node.length, node.mode) - getSegmentBitsLength(table[prevNodeId].lastCount, node.mode);
						table[prevNodeId].lastCount += node.length;
					} else {
						if (table[prevNodeId]) table[prevNodeId].lastCount = node.length;
						graph[prevNodeId][key] = getSegmentBitsLength(node.length, node.mode) + 4 + Mode$1.getCharCountIndicator(node.mode, version);
					}
				}
			}
			prevNodeIds = currentNodeIds;
		}
		for (let n = 0; n < prevNodeIds.length; n++) graph[prevNodeIds[n]].end = 0;
		return {
			map: graph,
			table
		};
	}
	function buildSingleSegment(data, modesHint) {
		let mode;
		const bestMode = Mode$1.getBestModeForData(data);
		mode = Mode$1.from(modesHint, bestMode);
		if (mode !== Mode$1.BYTE && mode.bit < bestMode.bit) throw new Error("\"" + data + "\" cannot be encoded with mode " + Mode$1.toString(mode) + ".\n Suggested mode is: " + Mode$1.toString(bestMode));
		if (mode === Mode$1.KANJI && !Utils$3.isKanjiModeEnabled()) mode = Mode$1.BYTE;
		switch (mode) {
			case Mode$1.NUMERIC: return new NumericData(data);
			case Mode$1.ALPHANUMERIC: return new AlphanumericData(data);
			case Mode$1.KANJI: return new KanjiData(data);
			case Mode$1.BYTE: return new ByteData(data);
		}
	}
	exports.fromArray = function fromArray(array) {
		return array.reduce(function(acc, seg) {
			if (typeof seg === "string") acc.push(buildSingleSegment(seg, null));
			else if (seg.data) acc.push(buildSingleSegment(seg.data, seg.mode));
			return acc;
		}, []);
	};
	exports.fromString = function fromString$2(data, version) {
		const graph = buildGraph(buildNodes(getSegmentsFromString(data, Utils$3.isKanjiModeEnabled())), version);
		const path = dijkstra.find_path(graph.map, "start", "end");
		const optimizedSegs = [];
		for (let i = 1; i < path.length - 1; i++) optimizedSegs.push(graph.table[path[i]].node);
		return exports.fromArray(mergeSegments(optimizedSegs));
	};
	exports.rawSplit = function rawSplit(data) {
		return exports.fromArray(getSegmentsFromString(data, Utils$3.isKanjiModeEnabled()));
	};
}));
var require_qrcode = /* @__PURE__ */ __commonJSMin(((exports) => {
	var Utils$2 = require_utils$1();
	var ECLevel = require_error_correction_level();
	var BitBuffer = require_bit_buffer();
	var BitMatrix = require_bit_matrix();
	var AlignmentPattern = require_alignment_pattern();
	var FinderPattern = require_finder_pattern();
	var MaskPattern = require_mask_pattern();
	var ECCode = require_error_correction_code();
	var ReedSolomonEncoder = require_reed_solomon_encoder();
	var Version = require_version();
	var FormatInfo = require_format_info();
	var Mode = require_mode();
	var Segments = require_segments();
	function setupFinderPattern(matrix, version) {
		const size = matrix.size;
		const pos = FinderPattern.getPositions(version);
		for (let i = 0; i < pos.length; i++) {
			const row = pos[i][0];
			const col = pos[i][1];
			for (let r = -1; r <= 7; r++) {
				if (row + r <= -1 || size <= row + r) continue;
				for (let c = -1; c <= 7; c++) {
					if (col + c <= -1 || size <= col + c) continue;
					if (r >= 0 && r <= 6 && (c === 0 || c === 6) || c >= 0 && c <= 6 && (r === 0 || r === 6) || r >= 2 && r <= 4 && c >= 2 && c <= 4) matrix.set(row + r, col + c, true, true);
					else matrix.set(row + r, col + c, false, true);
				}
			}
		}
	}
	function setupTimingPattern(matrix) {
		const size = matrix.size;
		for (let r = 8; r < size - 8; r++) {
			const value = r % 2 === 0;
			matrix.set(r, 6, value, true);
			matrix.set(6, r, value, true);
		}
	}
	function setupAlignmentPattern(matrix, version) {
		const pos = AlignmentPattern.getPositions(version);
		for (let i = 0; i < pos.length; i++) {
			const row = pos[i][0];
			const col = pos[i][1];
			for (let r = -2; r <= 2; r++) for (let c = -2; c <= 2; c++) if (r === -2 || r === 2 || c === -2 || c === 2 || r === 0 && c === 0) matrix.set(row + r, col + c, true, true);
			else matrix.set(row + r, col + c, false, true);
		}
	}
	function setupVersionInfo(matrix, version) {
		const size = matrix.size;
		const bits = Version.getEncodedBits(version);
		let row, col, mod;
		for (let i = 0; i < 18; i++) {
			row = Math.floor(i / 3);
			col = i % 3 + size - 8 - 3;
			mod = (bits >> i & 1) === 1;
			matrix.set(row, col, mod, true);
			matrix.set(col, row, mod, true);
		}
	}
	function setupFormatInfo(matrix, errorCorrectionLevel, maskPattern) {
		const size = matrix.size;
		const bits = FormatInfo.getEncodedBits(errorCorrectionLevel, maskPattern);
		let i, mod;
		for (i = 0; i < 15; i++) {
			mod = (bits >> i & 1) === 1;
			if (i < 6) matrix.set(i, 8, mod, true);
			else if (i < 8) matrix.set(i + 1, 8, mod, true);
			else matrix.set(size - 15 + i, 8, mod, true);
			if (i < 8) matrix.set(8, size - i - 1, mod, true);
			else if (i < 9) matrix.set(8, 15 - i - 1 + 1, mod, true);
			else matrix.set(8, 15 - i - 1, mod, true);
		}
		matrix.set(size - 8, 8, 1, true);
	}
	function setupData(matrix, data) {
		const size = matrix.size;
		let inc = -1;
		let row = size - 1;
		let bitIndex = 7;
		let byteIndex = 0;
		for (let col = size - 1; col > 0; col -= 2) {
			if (col === 6) col--;
			while (true) {
				for (let c = 0; c < 2; c++) if (!matrix.isReserved(row, col - c)) {
					let dark = false;
					if (byteIndex < data.length) dark = (data[byteIndex] >>> bitIndex & 1) === 1;
					matrix.set(row, col - c, dark);
					bitIndex--;
					if (bitIndex === -1) {
						byteIndex++;
						bitIndex = 7;
					}
				}
				row += inc;
				if (row < 0 || size <= row) {
					row -= inc;
					inc = -inc;
					break;
				}
			}
		}
	}
	function createData(version, errorCorrectionLevel, segments) {
		const buffer = new BitBuffer();
		segments.forEach(function(data) {
			buffer.put(data.mode.bit, 4);
			buffer.put(data.getLength(), Mode.getCharCountIndicator(data.mode, version));
			data.write(buffer);
		});
		const dataTotalCodewordsBits = (Utils$2.getSymbolTotalCodewords(version) - ECCode.getTotalCodewordsCount(version, errorCorrectionLevel)) * 8;
		if (buffer.getLengthInBits() + 4 <= dataTotalCodewordsBits) buffer.put(0, 4);
		while (buffer.getLengthInBits() % 8 !== 0) buffer.putBit(0);
		const remainingByte = (dataTotalCodewordsBits - buffer.getLengthInBits()) / 8;
		for (let i = 0; i < remainingByte; i++) buffer.put(i % 2 ? 17 : 236, 8);
		return createCodewords(buffer, version, errorCorrectionLevel);
	}
	function createCodewords(bitBuffer, version, errorCorrectionLevel) {
		const totalCodewords = Utils$2.getSymbolTotalCodewords(version);
		const dataTotalCodewords = totalCodewords - ECCode.getTotalCodewordsCount(version, errorCorrectionLevel);
		const ecTotalBlocks = ECCode.getBlocksCount(version, errorCorrectionLevel);
		const blocksInGroup1 = ecTotalBlocks - totalCodewords % ecTotalBlocks;
		const totalCodewordsInGroup1 = Math.floor(totalCodewords / ecTotalBlocks);
		const dataCodewordsInGroup1 = Math.floor(dataTotalCodewords / ecTotalBlocks);
		const dataCodewordsInGroup2 = dataCodewordsInGroup1 + 1;
		const ecCount = totalCodewordsInGroup1 - dataCodewordsInGroup1;
		const rs = new ReedSolomonEncoder(ecCount);
		let offset = 0;
		const dcData = new Array(ecTotalBlocks);
		const ecData = new Array(ecTotalBlocks);
		let maxDataSize = 0;
		const buffer = new Uint8Array(bitBuffer.buffer);
		for (let b = 0; b < ecTotalBlocks; b++) {
			const dataSize = b < blocksInGroup1 ? dataCodewordsInGroup1 : dataCodewordsInGroup2;
			dcData[b] = buffer.slice(offset, offset + dataSize);
			ecData[b] = rs.encode(dcData[b]);
			offset += dataSize;
			maxDataSize = Math.max(maxDataSize, dataSize);
		}
		const data = new Uint8Array(totalCodewords);
		let index = 0;
		let i, r;
		for (i = 0; i < maxDataSize; i++) for (r = 0; r < ecTotalBlocks; r++) if (i < dcData[r].length) data[index++] = dcData[r][i];
		for (i = 0; i < ecCount; i++) for (r = 0; r < ecTotalBlocks; r++) data[index++] = ecData[r][i];
		return data;
	}
	function createSymbol(data, version, errorCorrectionLevel, maskPattern) {
		let segments;
		if (Array.isArray(data)) segments = Segments.fromArray(data);
		else if (typeof data === "string") {
			let estimatedVersion = version;
			if (!estimatedVersion) {
				const rawSegments = Segments.rawSplit(data);
				estimatedVersion = Version.getBestVersionForData(rawSegments, errorCorrectionLevel);
			}
			segments = Segments.fromString(data, estimatedVersion || 40);
		} else throw new Error("Invalid data");
		const bestVersion = Version.getBestVersionForData(segments, errorCorrectionLevel);
		if (!bestVersion) throw new Error("The amount of data is too big to be stored in a QR Code");
		if (!version) version = bestVersion;
		else if (version < bestVersion) throw new Error("\nThe chosen QR Code version cannot contain this amount of data.\nMinimum version required to store current data is: " + bestVersion + ".\n");
		const dataBits = createData(version, errorCorrectionLevel, segments);
		const modules = new BitMatrix(Utils$2.getSymbolSize(version));
		setupFinderPattern(modules, version);
		setupTimingPattern(modules);
		setupAlignmentPattern(modules, version);
		setupFormatInfo(modules, errorCorrectionLevel, 0);
		if (version >= 7) setupVersionInfo(modules, version);
		setupData(modules, dataBits);
		if (isNaN(maskPattern)) maskPattern = MaskPattern.getBestMask(modules, setupFormatInfo.bind(null, modules, errorCorrectionLevel));
		MaskPattern.applyMask(maskPattern, modules);
		setupFormatInfo(modules, errorCorrectionLevel, maskPattern);
		return {
			modules,
			version,
			errorCorrectionLevel,
			maskPattern,
			segments
		};
	}
	exports.create = function create(data, options) {
		if (typeof data === "undefined" || data === "") throw new Error("No input text");
		let errorCorrectionLevel = ECLevel.M;
		let version;
		let mask;
		if (typeof options !== "undefined") {
			errorCorrectionLevel = ECLevel.from(options.errorCorrectionLevel, ECLevel.M);
			version = Version.from(options.version);
			mask = MaskPattern.from(options.maskPattern);
			if (options.toSJISFunc) Utils$2.setToSJISFunction(options.toSJISFunc);
		}
		return createSymbol(data, version, errorCorrectionLevel, mask);
	};
}));
var require_utils = /* @__PURE__ */ __commonJSMin(((exports) => {
	function hex2rgba(hex) {
		if (typeof hex === "number") hex = hex.toString();
		if (typeof hex !== "string") throw new Error("Color should be defined as hex string");
		let hexCode = hex.slice().replace("#", "").split("");
		if (hexCode.length < 3 || hexCode.length === 5 || hexCode.length > 8) throw new Error("Invalid hex color: " + hex);
		if (hexCode.length === 3 || hexCode.length === 4) hexCode = Array.prototype.concat.apply([], hexCode.map(function(c) {
			return [c, c];
		}));
		if (hexCode.length === 6) hexCode.push("F", "F");
		const hexValue = parseInt(hexCode.join(""), 16);
		return {
			r: hexValue >> 24 & 255,
			g: hexValue >> 16 & 255,
			b: hexValue >> 8 & 255,
			a: hexValue & 255,
			hex: "#" + hexCode.slice(0, 6).join("")
		};
	}
	exports.getOptions = function getOptions(options) {
		if (!options) options = {};
		if (!options.color) options.color = {};
		const margin = typeof options.margin === "undefined" || options.margin === null || options.margin < 0 ? 4 : options.margin;
		const width = options.width && options.width >= 21 ? options.width : void 0;
		const scale = options.scale || 4;
		return {
			width,
			scale: width ? 4 : scale,
			margin,
			color: {
				dark: hex2rgba(options.color.dark || "#000000ff"),
				light: hex2rgba(options.color.light || "#ffffffff")
			},
			type: options.type,
			rendererOpts: options.rendererOpts || {}
		};
	};
	exports.getScale = function getScale(qrSize, opts) {
		return opts.width && opts.width >= qrSize + opts.margin * 2 ? opts.width / (qrSize + opts.margin * 2) : opts.scale;
	};
	exports.getImageWidth = function getImageWidth(qrSize, opts) {
		const scale = exports.getScale(qrSize, opts);
		return Math.floor((qrSize + opts.margin * 2) * scale);
	};
	exports.qrToImageData = function qrToImageData(imgData, qr, opts) {
		const size = qr.modules.size;
		const data = qr.modules.data;
		const scale = exports.getScale(size, opts);
		const symbolSize = Math.floor((size + opts.margin * 2) * scale);
		const scaledMargin = opts.margin * scale;
		const palette = [opts.color.light, opts.color.dark];
		for (let i = 0; i < symbolSize; i++) for (let j = 0; j < symbolSize; j++) {
			let posDst = (i * symbolSize + j) * 4;
			let pxColor = opts.color.light;
			if (i >= scaledMargin && j >= scaledMargin && i < symbolSize - scaledMargin && j < symbolSize - scaledMargin) {
				const iSrc = Math.floor((i - scaledMargin) / scale);
				const jSrc = Math.floor((j - scaledMargin) / scale);
				pxColor = palette[data[iSrc * size + jSrc] ? 1 : 0];
			}
			imgData[posDst++] = pxColor.r;
			imgData[posDst++] = pxColor.g;
			imgData[posDst++] = pxColor.b;
			imgData[posDst] = pxColor.a;
		}
	};
}));
var require_canvas = /* @__PURE__ */ __commonJSMin(((exports) => {
	var Utils$1 = require_utils();
	function clearCanvas(ctx, canvas, size) {
		ctx.clearRect(0, 0, canvas.width, canvas.height);
		if (!canvas.style) canvas.style = {};
		canvas.height = size;
		canvas.width = size;
		canvas.style.height = size + "px";
		canvas.style.width = size + "px";
	}
	function getCanvasElement() {
		try {
			return document.createElement("canvas");
		} catch (e) {
			throw new Error("You need to specify a canvas element");
		}
	}
	exports.render = function render(qrData, canvas, options) {
		let opts = options;
		let canvasEl = canvas;
		if (typeof opts === "undefined" && (!canvas || !canvas.getContext)) {
			opts = canvas;
			canvas = void 0;
		}
		if (!canvas) canvasEl = getCanvasElement();
		opts = Utils$1.getOptions(opts);
		const size = Utils$1.getImageWidth(qrData.modules.size, opts);
		const ctx = canvasEl.getContext("2d");
		const image = ctx.createImageData(size, size);
		Utils$1.qrToImageData(image.data, qrData, opts);
		clearCanvas(ctx, canvasEl, size);
		ctx.putImageData(image, 0, 0);
		return canvasEl;
	};
	exports.renderToDataURL = function renderToDataURL(qrData, canvas, options) {
		let opts = options;
		if (typeof opts === "undefined" && (!canvas || !canvas.getContext)) {
			opts = canvas;
			canvas = void 0;
		}
		if (!opts) opts = {};
		const canvasEl = exports.render(qrData, canvas, opts);
		const type = opts.type || "image/png";
		const rendererOpts = opts.rendererOpts || {};
		return canvasEl.toDataURL(type, rendererOpts.quality);
	};
}));
var require_svg_tag = /* @__PURE__ */ __commonJSMin(((exports) => {
	var Utils = require_utils();
	function getColorAttrib(color, attrib) {
		const alpha = color.a / 255;
		const str = attrib + "=\"" + color.hex + "\"";
		return alpha < 1 ? str + " " + attrib + "-opacity=\"" + alpha.toFixed(2).slice(1) + "\"" : str;
	}
	function svgCmd(cmd, x, y) {
		let str = cmd + x;
		if (typeof y !== "undefined") str += " " + y;
		return str;
	}
	function qrToPath(data, size, margin) {
		let path = "";
		let moveBy = 0;
		let newRow = false;
		let lineLength = 0;
		for (let i = 0; i < data.length; i++) {
			const col = Math.floor(i % size);
			const row = Math.floor(i / size);
			if (!col && !newRow) newRow = true;
			if (data[i]) {
				lineLength++;
				if (!(i > 0 && col > 0 && data[i - 1])) {
					path += newRow ? svgCmd("M", col + margin, .5 + row + margin) : svgCmd("m", moveBy, 0);
					moveBy = 0;
					newRow = false;
				}
				if (!(col + 1 < size && data[i + 1])) {
					path += svgCmd("h", lineLength);
					lineLength = 0;
				}
			} else moveBy++;
		}
		return path;
	}
	exports.render = function render(qrData, options, cb) {
		const opts = Utils.getOptions(options);
		const size = qrData.modules.size;
		const data = qrData.modules.data;
		const qrcodesize = size + opts.margin * 2;
		const bg = !opts.color.light.a ? "" : "<path " + getColorAttrib(opts.color.light, "fill") + " d=\"M0 0h" + qrcodesize + "v" + qrcodesize + "H0z\"/>";
		const path = "<path " + getColorAttrib(opts.color.dark, "stroke") + " d=\"" + qrToPath(data, size, opts.margin) + "\"/>";
		const viewBox = "viewBox=\"0 0 " + qrcodesize + " " + qrcodesize + "\"";
		const svgTag = "<svg xmlns=\"http://www.w3.org/2000/svg\" " + (!opts.width ? "" : "width=\"" + opts.width + "\" height=\"" + opts.width + "\" ") + viewBox + " shape-rendering=\"crispEdges\">" + bg + path + "</svg>\n";
		if (typeof cb === "function") cb(null, svgTag);
		return svgTag;
	};
}));
var import_browser = /* @__PURE__ */ __toESM((/* @__PURE__ */ __commonJSMin(((exports) => {
	var canPromise = require_can_promise();
	var QRCode = require_qrcode();
	var CanvasRenderer = require_canvas();
	var SvgRenderer = require_svg_tag();
	function renderCanvas(renderFunc, canvas, text, opts, cb) {
		const args = [].slice.call(arguments, 1);
		const argsNum = args.length;
		const isLastArgCb = typeof args[argsNum - 1] === "function";
		if (!isLastArgCb && !canPromise()) throw new Error("Callback required as last argument");
		if (isLastArgCb) {
			if (argsNum < 2) throw new Error("Too few arguments provided");
			if (argsNum === 2) {
				cb = text;
				text = canvas;
				canvas = opts = void 0;
			} else if (argsNum === 3) if (canvas.getContext && typeof cb === "undefined") {
				cb = opts;
				opts = void 0;
			} else {
				cb = opts;
				opts = text;
				text = canvas;
				canvas = void 0;
			}
		} else {
			if (argsNum < 1) throw new Error("Too few arguments provided");
			if (argsNum === 1) {
				text = canvas;
				canvas = opts = void 0;
			} else if (argsNum === 2 && !canvas.getContext) {
				opts = text;
				text = canvas;
				canvas = void 0;
			}
			return new Promise(function(resolve, reject) {
				try {
					resolve(renderFunc(QRCode.create(text, opts), canvas, opts));
				} catch (e) {
					reject(e);
				}
			});
		}
		try {
			const data = QRCode.create(text, opts);
			cb(null, renderFunc(data, canvas, opts));
		} catch (e) {
			cb(e);
		}
	}
	exports.create = QRCode.create;
	exports.toCanvas = renderCanvas.bind(null, CanvasRenderer.render);
	exports.toDataURL = renderCanvas.bind(null, CanvasRenderer.renderToDataURL);
	exports.toString = renderCanvas.bind(null, function(data, _, opts) {
		return SvgRenderer.render(data, opts);
	});
})))());
async function renderQrDataUrl(text) {
	return import_browser.toDataURL(text, {
		errorCorrectionLevel: "M",
		margin: 2,
		width: 232
	});
}
function useMobileInstallQr(stage, platform, iosChannel) {
	const [installQrUrl, setInstallQrUrl] = (0, import_react.useState)(null);
	(0, import_react.useEffect)(() => {
		if (stage !== "flow") return;
		setInstallQrUrl(null);
		let cancelled = false;
		(async () => {
			try {
				const dataUrl = await renderQrDataUrl(getInstallCopy(platform, iosChannel).url);
				if (!cancelled) setInstallQrUrl(dataUrl);
			} catch {
				if (!cancelled) setInstallQrUrl(null);
			}
		})();
		return () => {
			cancelled = true;
		};
	}, [
		platform,
		iosChannel,
		stage
	]);
	return installQrUrl;
}
function useMobilePairingGeneration(params) {
	const { connectionMode, signedIn, selectedAddress, mountedRef, hasGeneratedRef, pairingRequestIdRef, setPairQrDataUrl, setPairingUrl, setPairingQrError, setPairLoading, setRelayMintFailure } = params;
	return { generatePairing: (0, import_react.useCallback)(async (rotate, addressOverride, connectionModeOverride) => {
		const preferredMode = connectionModeOverride ?? connectionMode;
		if (!canMintMobilePairingOffer({
			connectionMode: preferredMode,
			signedIn
		})) return;
		const requestId = ++pairingRequestIdRef.current;
		hasGeneratedRef.current = true;
		if (mountedRef.current) setPairLoading(true);
		try {
			const address = addressOverride ?? selectedAddress;
			const result = await window.api.mobile.getPairingQR({
				...address ? { address } : {},
				connectionMode: preferredMode,
				...rotate ? { rotate: true } : {}
			});
			if (requestId !== pairingRequestIdRef.current) return;
			if (result.available) {
				if (mountedRef.current) {
					setPairQrDataUrl(result.qrDataUrl);
					setPairingUrl(result.pairingUrl);
					setPairingQrError(result.qrDataUrl === null);
					setRelayMintFailure(null);
				}
			} else if (mountedRef.current) {
				setPairQrDataUrl(null);
				setPairingUrl(null);
				setPairingQrError(false);
				if (result.reason === "relay_mint_failed" && result.relayFailure) setRelayMintFailure(result.relayFailure);
				else {
					setRelayMintFailure(null);
					toast.error(result.guidance ?? translate("auto.components.mobile.MobilePage.b353e18de1", "WebSocket transport is not running"));
				}
			}
		} catch {
			if (mountedRef.current && requestId === pairingRequestIdRef.current) {
				hasGeneratedRef.current = false;
				setPairQrDataUrl(null);
				setPairingUrl(null);
				setPairingQrError(false);
				setRelayMintFailure(null);
				toast.error(translate("auto.components.mobile.MobilePage.4c8bd11c1a", "Failed to generate pairing code"));
			}
		} finally {
			if (mountedRef.current && requestId === pairingRequestIdRef.current) setPairLoading(false);
		}
	}, [
		connectionMode,
		hasGeneratedRef,
		mountedRef,
		pairingRequestIdRef,
		selectedAddress,
		setPairLoading,
		setPairQrDataUrl,
		setPairingUrl,
		setPairingQrError,
		setRelayMintFailure,
		signedIn
	]) };
}
function useMobilePairingQrInvalidation(params) {
	const { connectionMode, signedIn, pairLoading, hasGeneratedRef, pairingRequestIdRef, setPairQrDataUrl, setPairingUrl, setPairingQrError, setPairLoading, setRelayMintFailure, regenerate } = params;
	const wasSignedInRef = (0, import_react.useRef)(signedIn);
	const handledModeRef = (0, import_react.useRef)(connectionMode);
	(0, import_react.useEffect)(() => {
		const wasSignedIn = wasSignedInRef.current;
		wasSignedInRef.current = signedIn;
		if (connectionMode !== "automatic" || !hasGeneratedRef.current || wasSignedIn === signedIn) return;
		pairingRequestIdRef.current += 1;
		hasGeneratedRef.current = false;
		setPairingUrl(null);
		setPairingQrError(false);
		setPairQrDataUrl(null);
		setRelayMintFailure?.(null);
		if (signedIn && canMintMobilePairingOffer({
			connectionMode,
			signedIn
		})) regenerate(connectionMode, { rotate: true });
		else setPairLoading(false);
	}, [
		connectionMode,
		signedIn,
		hasGeneratedRef,
		pairingRequestIdRef,
		setPairQrDataUrl,
		setPairingUrl,
		setPairingQrError,
		setPairLoading,
		setRelayMintFailure,
		regenerate
	]);
	(0, import_react.useEffect)(() => {
		if (connectionMode === handledModeRef.current) return;
		handledModeRef.current = connectionMode;
		pairingRequestIdRef.current += 1;
		const shouldRegenerate = hasGeneratedRef.current || pairLoading;
		hasGeneratedRef.current = false;
		setPairingUrl(null);
		setPairingQrError(false);
		setPairQrDataUrl(null);
		setRelayMintFailure?.(null);
		if (shouldRegenerate && canMintMobilePairingOffer({
			connectionMode,
			signedIn
		})) regenerate(connectionMode, { rotate: false });
		else setPairLoading(false);
	}, [
		connectionMode,
		signedIn,
		pairLoading,
		hasGeneratedRef,
		pairingRequestIdRef,
		setPairQrDataUrl,
		setPairingUrl,
		setPairingQrError,
		setPairLoading,
		setRelayMintFailure,
		regenerate
	]);
}
function useMobileInstallActions(platform, iosChannel) {
	const mountedRef = useMountedRef();
	const openInstallUrl = (0, import_react.useCallback)(() => {
		window.api.shell.openUrl(getInstallCopy(platform, iosChannel).url);
	}, [iosChannel, platform]);
	return {
		copyInstallUrl: (0, import_react.useCallback)(async () => {
			try {
				await window.api.ui.writeClipboardText(getInstallCopy(platform, iosChannel).url);
				if (mountedRef.current) toast.success(translate("auto.components.mobile.MobilePage.fad833de8d", "Install link copied"));
			} catch (error) {
				console.error("writeClipboardText failed", error);
				if (mountedRef.current) toast.error(translate("auto.components.mobile.MobilePage.baea63c445", "Failed to copy link"));
			}
		}, [
			iosChannel,
			mountedRef,
			platform
		]),
		openInstallUrl
	};
}
function shouldShowPairedAfterDeviceRefresh({ stage, deviceCountAtPairStart, nextDeviceCount }) {
	return stage === "flow" && deviceCountAtPairStart !== null && nextDeviceCount > deviceCountAtPairStart;
}
function useMobilePagePairedDevices({ stepIdx, setStepIdx }) {
	const [stage, setStage] = (0, import_react.useState)(null);
	const [revokingDeviceIds, setRevokingDeviceIds] = (0, import_react.useState)([]);
	const [deviceCountAtPairStart, setDeviceCountAtPairStart] = (0, import_react.useState)(null);
	const mountedRef = useMountedRef();
	const stageRef = (0, import_react.useRef)(null);
	const deviceCountAtPairStartRef = (0, import_react.useRef)(null);
	const { devices, refresh: refreshDevices } = usePairedMobileDevices({ refreshOnMount: false });
	const setPairingDeviceBaseline = (0, import_react.useCallback)((count) => {
		deviceCountAtPairStartRef.current = count;
		if (mountedRef.current) setDeviceCountAtPairStart(count);
	}, [mountedRef]);
	const showStage = (0, import_react.useCallback)((nextStage) => {
		stageRef.current = nextStage;
		if (mountedRef.current) setStage(nextStage);
	}, [mountedRef]);
	const showPairedDevices = (0, import_react.useCallback)((deviceCount) => {
		setPairingDeviceBaseline(deviceCount);
		showStage("paired");
	}, [setPairingDeviceBaseline, showStage]);
	const loadDevices = (0, import_react.useCallback)(async (opts = {}) => {
		try {
			const nextDevices = await refreshDevices(opts);
			if (mountedRef.current) {
				if (shouldShowPairedAfterDeviceRefresh({
					stage: stageRef.current,
					deviceCountAtPairStart: deviceCountAtPairStartRef.current,
					nextDeviceCount: nextDevices.length
				})) showPairedDevices(nextDevices.length);
			}
			return nextDevices;
		} catch (err) {
			console.error("mobile.listDevices failed", err);
			return [];
		}
	}, [
		mountedRef,
		refreshDevices,
		showPairedDevices
	]);
	(0, import_react.useEffect)(() => {
		let cancelled = false;
		(async () => {
			const initialDevices = await loadDevices();
			if (cancelled) return;
			if (initialDevices.length > 0) showPairedDevices(initialDevices.length);
			else showStage("intro");
		})();
		return () => {
			cancelled = true;
		};
	}, [
		loadDevices,
		showPairedDevices,
		showStage
	]);
	const revokeDevice = (0, import_react.useCallback)(async (deviceId) => {
		let alreadyRevoking = false;
		setRevokingDeviceIds((prev) => {
			if (prev.includes(deviceId)) {
				alreadyRevoking = true;
				return prev;
			}
			return [...prev, deviceId];
		});
		if (alreadyRevoking) return;
		try {
			const { revoked } = await window.api.mobile.revokeDevice({ deviceId });
			if (!revoked) throw new Error("mobile.revokeDevice returned revoked=false");
			let remaining;
			try {
				remaining = await refreshDevices({ force: true });
			} catch (err) {
				console.error("mobile.listDevices failed after revoke", err);
				remaining = getPairedMobileDevicesSnapshot().filter((d) => d.deviceId !== deviceId);
				replacePairedMobileDevices(remaining);
			}
			if (mountedRef.current) toast.success(translate("auto.components.mobile.MobilePage.255372e6e8", "Device revoked"));
			if (remaining.length === 0 && mountedRef.current) showStage("intro");
		} catch {
			if (mountedRef.current) toast.error(translate("auto.components.mobile.MobilePage.4e1eb5d55c", "Failed to revoke device"));
		} finally {
			if (mountedRef.current) setRevokingDeviceIds((prev) => prev.filter((id) => id !== deviceId));
		}
	}, [
		mountedRef,
		refreshDevices,
		showStage
	]);
	const polledLoadDevices = (0, import_react.useCallback)(async () => {
		await loadDevices();
	}, [loadDevices]);
	useMobilePairingDevicePolling({
		deviceCountAtQr: stage === "flow" && stepIdx === 1 || stage === "paired" ? deviceCountAtPairStart : null,
		currentDeviceCount: devices.length,
		loadDevices: polledLoadDevices
	});
	const enterFlow = () => {
		setStepIdx(0);
		setPairingDeviceBaseline(devices.length);
		showStage("flow");
	};
	const pairAnotherDevice = () => {
		setStepIdx(1);
		setPairingDeviceBaseline(devices.length);
		showStage("flow");
	};
	const handleBack = () => {
		if (stepIdx === 1) setStepIdx(0);
		else if (devices.length > 0) showPairedDevices(devices.length);
		else showStage("intro");
	};
	return {
		devices,
		stage,
		revokingDeviceIds,
		enterFlow,
		handleBack,
		pairAnotherDevice,
		revokeDevice,
		showPairedDevices
	};
}
function MobilePage() {
	const [stepIdx, setStepIdx] = (0, import_react.useState)(0);
	const [platform, setPlatform] = (0, import_react.useState)("ios");
	const [iosChannel, setIosChannel] = (0, import_react.useState)("preview");
	const [pairQrDataUrl, setPairQrDataUrl] = (0, import_react.useState)(null);
	const [pairingUrl, setPairingUrl] = (0, import_react.useState)(null);
	const [pairingQrError, setPairingQrError] = (0, import_react.useState)(false);
	const [relayMintFailure, setRelayMintFailure] = (0, import_react.useState)(null);
	const [pairLoading, setPairLoading] = (0, import_react.useState)(false);
	const signedIn = useAppStore((state) => state.orcaProfileAuthStatus?.state === "connected");
	const [connectionMode, setConnectionMode] = useMobilePairingConnectionMode();
	const [networkInterfaces, setNetworkInterfaces] = (0, import_react.useState)([]);
	const pairingAddressChangeRef = (0, import_react.useRef)(() => {});
	const { selectedAddress, selectedAddressIsCustom, customAddresses, selectAddress: handleAddressChange, selectCustomAddress: handleCustomAddressSelect, removeCustomAddress: handleCustomAddressRemove, selectAddressAfterRefresh } = useMobilePairingAddressPreference({
		networkInterfaces,
		onSelectionInvalidated: (0, import_react.useCallback)((change) => pairingAddressChangeRef.current(change), [])
	});
	const [refreshingNetworkInterfaces, setRefreshingNetworkInterfaces] = (0, import_react.useState)(false);
	const hasGeneratedRef = (0, import_react.useRef)(false);
	const pairingRequestIdRef = (0, import_react.useRef)(0);
	const mountedRef = useMountedRef();
	const closeMobilePage = useAppStore((s) => s.closeMobilePage);
	const showMobileButton = useAppStore((s) => s.settings?.showMobileButton !== false);
	const updateSettings = useAppStore((s) => s.updateSettings);
	const { devices, enterFlow: showFirstPairingFlow, handleBack, pairAnotherDevice: showPairAnotherDeviceFlow, revokeDevice, revokingDeviceIds, showPairedDevices, stage } = useMobilePagePairedDevices({
		stepIdx,
		setStepIdx
	});
	const installQrUrl = useMobileInstallQr(stage, platform, iosChannel);
	const { copyInstallUrl, openInstallUrl } = useMobileInstallActions(platform, iosChannel);
	const { generatePairing } = useMobilePairingGeneration({
		connectionMode,
		signedIn,
		selectedAddress,
		mountedRef,
		hasGeneratedRef,
		pairingRequestIdRef,
		setPairQrDataUrl,
		setPairingUrl,
		setPairingQrError,
		setPairLoading,
		setRelayMintFailure
	});
	(0, import_react.useLayoutEffect)(() => {
		pairingAddressChangeRef.current = ({ address, source }) => {
			const pairingContext = {
				connectionMode,
				signedIn
			};
			if (source === "user") {
				if (canMintMobilePairingOffer(pairingContext)) generatePairing(true, address ?? "");
				return;
			}
			if (source === "refresh") {
				if (hasGeneratedRef.current && canMintMobilePairingOffer(pairingContext)) generatePairing(true, address);
				return;
			}
			const shouldRegenerate = hasGeneratedRef.current || pairLoading;
			pairingRequestIdRef.current += 1;
			hasGeneratedRef.current = false;
			setPairQrDataUrl(null);
			setPairingUrl(null);
			setPairingQrError(false);
			setRelayMintFailure(null);
			setPairLoading(false);
			if (shouldRegenerate && canMintMobilePairingOffer(pairingContext)) generatePairing(true, address ?? "");
		};
	}, [
		connectionMode,
		generatePairing,
		pairLoading,
		signedIn
	]);
	const handleConnectionModeChange = (0, import_react.useCallback)((nextMode) => {
		if (nextMode === connectionMode) return;
		setRelayMintFailure(null);
		setConnectionMode(nextMode);
		updateSettings({ mobilePairingConnectionMode: nextMode });
	}, [
		connectionMode,
		updateSettings,
		setConnectionMode
	]);
	const copyRelayDiagnostics = (0, import_react.useCallback)(async () => {
		if (relayMintFailure == null) return;
		const payload = {
			kind: "mobile_pairing_relay_failure",
			preferredConnectionMode: connectionMode,
			failure: relayMintFailure,
			at: (/* @__PURE__ */ new Date()).toISOString()
		};
		try {
			await window.api.ui.writeClipboardText(JSON.stringify(payload, null, 2));
			if (mountedRef.current) toast.success(translate("auto.components.mobile.MobilePage.diagnosticsCopied", "Diagnostics copied"));
		} catch {
			if (mountedRef.current) toast.error(translate("auto.components.mobile.MobilePage.diagnosticsCopyFailed", "Failed to copy diagnostics"));
		}
	}, [
		connectionMode,
		mountedRef,
		relayMintFailure
	]);
	useMobilePairingQrInvalidation({
		connectionMode,
		signedIn,
		pairLoading,
		hasGeneratedRef,
		pairingRequestIdRef,
		setPairQrDataUrl,
		setPairingUrl,
		setPairingQrError,
		setPairLoading,
		setRelayMintFailure,
		regenerate: (mode, opts) => void generatePairing(opts.rotate, void 0, mode)
	});
	const loadNetworkInterfaces = (0, import_react.useCallback)(async () => {
		if (mountedRef.current) setRefreshingNetworkInterfaces(true);
		try {
			const result = await window.api.mobile.listNetworkInterfaces();
			if (mountedRef.current) {
				setNetworkInterfaces(result.interfaces);
				selectAddressAfterRefresh(result.interfaces);
			}
		} catch {} finally {
			if (mountedRef.current) setRefreshingNetworkInterfaces(false);
		}
	}, [mountedRef, selectAddressAfterRefresh]);
	(0, import_react.useEffect)(() => {
		if (stage !== "flow") return;
		loadNetworkInterfaces();
	}, [stage, loadNetworkInterfaces]);
	const beforeCustomAddressChange = (0, import_react.useCallback)(async (address) => {
		if (!canMintMobilePairingOffer({
			connectionMode,
			signedIn
		})) return true;
		try {
			const result = await window.api.mobile.getPairingQR({
				address,
				connectionMode
			});
			return result.available && result.qrDataUrl !== null;
		} catch {
			return false;
		}
	}, [connectionMode, signedIn]);
	const copyPairingCode = (0, import_react.useCallback)(async () => {
		if (!pairingUrl) return;
		try {
			await window.api.ui.writeClipboardText(pairingUrl);
			if (mountedRef.current) toast.success(translate("auto.components.mobile.MobilePage.3c1f7168bb", "Pairing code copied"));
		} catch (err) {
			console.error("writeClipboardText failed", err);
			if (mountedRef.current) toast.error(translate("auto.components.mobile.MobilePage.6a66e38943", "Failed to copy pairing code"));
		}
	}, [mountedRef, pairingUrl]);
	const canGenerate = canMintMobilePairingOffer({
		connectionMode,
		signedIn
	});
	(0, import_react.useEffect)(() => {
		if (stage !== "flow" || stepIdx !== 1 || hasGeneratedRef.current) return;
		if (!canGenerate) return;
		generatePairing(false);
	}, [
		stage,
		stepIdx,
		canGenerate,
		generatePairing
	]);
	const enterFlow = () => {
		hasGeneratedRef.current = false;
		setPairQrDataUrl(null);
		setPairingUrl(null);
		setPairingQrError(false);
		setRelayMintFailure(null);
		showFirstPairingFlow();
	};
	const pairAnotherDevice = () => {
		hasGeneratedRef.current = false;
		setPairQrDataUrl(null);
		setPairingUrl(null);
		setPairingQrError(false);
		setRelayMintFailure(null);
		showPairAnotherDeviceFlow();
	};
	const handleContinue = () => {
		if (stepIdx === 0) setStepIdx(1);
	};
	const toggleMobileSidebarButton = (0, import_react.useCallback)(() => {
		const nextShowMobileButton = !showMobileButton;
		updateSettings({ showMobileButton: nextShowMobileButton });
		if (!nextShowMobileButton) toast.message(translate("auto.components.mobile.MobilePageToolbar.e1c7b4a92d", "Configure in Settings > Mobile."));
	}, [showMobileButton, updateSettings]);
	useMobilePageEscape(closeMobilePage);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MobilePageContent, {
		closeMobilePage,
		copyInstallUrl: () => void copyInstallUrl(),
		copyPairingCode: () => void copyPairingCode(),
		devices,
		enterFlow,
		generatePairing: (rotate) => void generatePairing(rotate),
		canGeneratePairing: canGenerate,
		handleAddressChange,
		customAddresses,
		selectedAddressIsCustom,
		onCustomAddressSelect: handleCustomAddressSelect,
		onCustomAddressRemove: handleCustomAddressRemove,
		beforeCustomAddressChange,
		handleBack,
		handleContinue,
		installQrUrl,
		iosChannel,
		setIosChannel,
		loadNetworkInterfaces: () => void loadNetworkInterfaces(),
		networkInterfaces,
		openInstallUrl,
		pairAnotherDevice,
		pairLoading,
		connectionMode,
		handleConnectionModeChange,
		pairQrDataUrl,
		pairingUrl,
		pairingQrError,
		relayMintFailure: connectionMode === "automatic" && pairQrDataUrl == null ? relayMintFailure : null,
		onUseLan: () => handleConnectionModeChange("local-only"),
		onRetryRelay: () => void generatePairing(true),
		onCopyRelayDiagnostics: () => void copyRelayDiagnostics(),
		platform,
		refreshingNetworkInterfaces,
		revokeDevice: (id) => void revokeDevice(id),
		revokingDeviceIds,
		selectedAddress,
		setPlatform,
		showMobileButton,
		showPairedDevices,
		stage,
		stepIdx,
		toggleMobileSidebarButton
	});
}
export { MobilePage as default };
