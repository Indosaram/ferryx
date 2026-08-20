import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn, t as Button } from "./button-DszXJEV6.js";
import { t as ArrowLeft } from "./arrow-left-BpDalf_n.js";
import { t as ArrowRight } from "./arrow-right-ct5UxmKv.js";
import { Br as getVisibleContextualTourStepIndexes, Fr as getContextualTourOutcomeStepTotal, Ir as getContextualTourPanelHost, Lr as getContextualTourStepCopy, Rr as getContextualTourStepProgress, Vr as isContextualTourAllowedForModal, bi as getContextualTour, t as useAppStore, zr as getMeasurableContextualTourTarget } from "./store-CgXrfmaH.js";
import { t as X } from "./x-BrGKE4uz.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import { t as require_react_dom } from "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import { c as shift, i as flip, n as autoUpdate, r as computePosition, s as offset, t as arrow } from "./floating-ui.dom-i2UEqmZo.js";
import { t as Switch } from "./switch-NhZdOYtg.js";
import "./useMountedRef-1omUd-IV.js";
import { r as formatShortcutLabel } from "./useShortcutLabel-C-KRYtlB.js";
import "./request-contextual-tour-when-ready-C7EM2Vly.js";
import { n as openWorkspaceCreationComposerWithTourHandoff, t as CONTEXTUAL_TOUR_ENABLE_AUTO_WORKSPACE_NAME_EVENT } from "./contextual-tour-composer-events-BzKuV1U_.js";
import "./feature-wall-setup-steps-D7ga1-7b.js";
import { t as requestActiveTerminalPaneSplit } from "./request-active-terminal-pane-split-So9AiZw3.js";
import { i as trackContextualTourShown, r as trackContextualTourOutcome } from "./feature-education-telemetry-DPRGAVBD.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var LOCALIZED_STEP_COPY = {
	"automations-intro": {
		title: () => translate("auto.components.contextual.tours.contextual.tour.overlay.measurement.automations.intro.title", "What is an automation?"),
		body: () => translate("auto.components.contextual.tours.contextual.tour.overlay.measurement.automations.intro.body", "Automations run agent work on a schedule. Add an automation by clicking this button.")
	},
	"automations-results": {
		title: () => translate("auto.components.contextual.tours.contextual.tour.overlay.measurement.automations.results.title", "Find the results"),
		body: () => translate("auto.components.contextual.tours.contextual.tour.overlay.measurement.automations.results.body", "Runs show when automations ran, what happened, and where to inspect their output.")
	}
};
function getContextualTourDisplayProgress(args) {
	if (!args.activeStep) return null;
	if (args.tour.id === "browser") return {
		current: args.stepIndex + 1,
		total: args.tour.steps.length
	};
	return getContextualTourStepProgress({
		visibleStepIndexes: args.visibleStepIndexes,
		stepIndex: args.stepIndex
	});
}
function getContextualTourMeasurementAction(args) {
	if (args.visibleStepIndexes.some((index) => index > args.activeStepIndex)) return { kind: "advance" };
	if (args.activeStepIndex < args.tour.steps.length - 1 || args.tour.id === "browser") return { kind: "wait" };
	return { kind: "cancel" };
}
function isContextualTourLastDisplayStep(args) {
	if (args.tour.id === "browser") return args.activeStepIndex === args.tour.steps.length - 1;
	return args.progress.current === args.progress.total;
}
function measureContextualTourOverlayRenderState(args) {
	const targetExists = (selector) => getMeasurableContextualTourTarget(selector) !== null;
	const visibleStepIndexes = getVisibleContextualTourStepIndexes(args.tour, targetExists);
	const telemetryTotalSteps = Math.max(args.previousTelemetryTotalSteps, getContextualTourOutcomeStepTotal(visibleStepIndexes));
	const activeStep = args.tour.steps[args.activeStepIndex];
	const target = activeStep ? getMeasurableContextualTourTarget(activeStep.targetSelector) : null;
	const localizedCopy = activeStep?.id ? LOCALIZED_STEP_COPY[activeStep.id] : void 0;
	const localizedTitle = localizedCopy ? localizedCopy.title() : activeStep?.title;
	const localizedBody = localizedCopy ? localizedCopy.body() : activeStep ? getContextualTourStepCopy(activeStep) : void 0;
	const progress = getContextualTourDisplayProgress({
		tour: args.tour,
		visibleStepIndexes,
		stepIndex: args.activeStepIndex,
		activeStep
	});
	if (visibleStepIndexes.length === 0 || !activeStep || !progress) return { kind: "cancel" };
	if (!target) {
		const measurementAction = getContextualTourMeasurementAction({
			tour: args.tour,
			visibleStepIndexes,
			activeStepIndex: args.activeStepIndex
		});
		if (measurementAction.kind === "advance") return { kind: "advance" };
		if (measurementAction.kind === "wait") return { kind: "wait" };
		return { kind: "cancel" };
	}
	const sidebarAlreadyVisible = activeStep.primaryAction?.kind === "show-worktrees" && args.sidebarOpen;
	const primaryAction = sidebarAlreadyVisible ? {
		kind: "next",
		label: translate("auto.components.contextual.tours.contextual.tour.overlay.measurement.38b3155418", "Next")
	} : activeStep.primaryAction;
	const secondaryAction = sidebarAlreadyVisible ? void 0 : activeStep.secondaryAction;
	return {
		kind: "render",
		telemetryTotalSteps,
		renderState: {
			rect: target.rect,
			targetElement: target.element,
			progress,
			title: localizedTitle ?? activeStep.title,
			body: formatContextualTourStepCopy(localizedBody ?? getContextualTourStepCopy(activeStep), args.keybindings),
			control: activeStep.control,
			primaryAction,
			secondaryAction,
			preferredPlacement: activeStep.preferredPlacement,
			targetPulse: activeStep.targetPulse,
			hidePrimaryAction: activeStep.hidePrimaryAction,
			isLastStep: isContextualTourLastDisplayStep({
				tour: args.tour,
				activeStepIndex: args.activeStepIndex,
				progress
			}),
			isFirstStep: progress.current === 1,
			panelHost: getContextualTourPanelHost(target.element)
		}
	};
}
function getContextualTourCleanupOutcome(activeTourId) {
	return useAppStore.getState().lastCompletedContextualTourId === activeTourId ? "completed" : "cancelled";
}
function formatContextualTourStepCopy(copy, keybindings) {
	return copy.replace("{terminal.splitRight}", formatShortcutLabel("terminal.splitRight", keybindings));
}
var import_react_dom = require_react_dom();
var PANEL_GAP = 12;
var COLLISION_PADDING = 12;
var ARROW_PADDING = 16;
var ARROW_WIDTH$1 = 18;
var ARROW_HEIGHT$1 = 8;
var FALLBACK_PLACEMENTS = {
	top: [
		"bottom",
		"right",
		"left"
	],
	right: [
		"left",
		"bottom",
		"top"
	],
	bottom: [
		"top",
		"right",
		"left"
	],
	left: [
		"right",
		"bottom",
		"top"
	]
};
const CONTEXTUAL_TOUR_ARROW_SIZE = {
	width: ARROW_WIDTH$1,
	height: ARROW_HEIGHT$1
};
async function getContextualTourFloatingPosition(args) {
	const initialPlacement = args.preferredPlacement ?? "right";
	const boundary = getContextualTourCollisionBoundary(args.panelHost);
	const result = await computePosition(args.targetElement, args.floatingElement, {
		strategy: args.panelHost ? "absolute" : "fixed",
		placement: initialPlacement,
		middleware: [
			offset(PANEL_GAP),
			flip({
				boundary,
				padding: COLLISION_PADDING,
				fallbackPlacements: FALLBACK_PLACEMENTS[initialPlacement]
			}),
			shift({
				boundary,
				padding: COLLISION_PADDING,
				crossAxis: true
			}),
			arrow({
				element: args.arrowElement,
				padding: ARROW_PADDING
			})
		]
	});
	const panelPlacement = getContextualTourPanelPlacement(result.placement);
	const panelPosition = {
		left: result.x,
		top: result.y
	};
	return {
		arrowPosition: getContextualTourArrowPosition({
			arrowX: result.middlewareData.arrow?.x,
			arrowY: result.middlewareData.arrow?.y,
			panelPlacement
		}),
		panelPlacement,
		panelPosition
	};
}
function watchContextualTourFloatingPosition(args) {
	let disposed = false;
	let updateSequence = 0;
	const update = () => {
		const sequence = ++updateSequence;
		getContextualTourFloatingPosition(args).then((position) => {
			if (!disposed && sequence === updateSequence) args.onPosition(position);
		}).catch(() => void 0);
	};
	const stopAutoUpdate = autoUpdate(args.targetElement, args.floatingElement, update, { animationFrame: true });
	return () => {
		disposed = true;
		stopAutoUpdate();
	};
}
function getContextualTourCollisionBoundary(panelHost) {
	return panelHost ?? "clippingAncestors";
}
function getContextualTourPanelPlacement(placement) {
	return placement.split("-")[0];
}
function getContextualTourArrowPosition(args) {
	const staticSide = {
		top: "bottom",
		right: "left",
		bottom: "top",
		left: "right"
	}[args.panelPlacement];
	return {
		left: args.arrowX,
		top: args.arrowY,
		[staticSide]: -ARROW_HEIGHT$1
	};
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var ARROW_WIDTH = CONTEXTUAL_TOUR_ARROW_SIZE.width;
var ARROW_HEIGHT = CONTEXTUAL_TOUR_ARROW_SIZE.height;
var PLACEMENT_TRANSFORM = {
	top: "rotate(0deg)",
	bottom: "rotate(180deg)",
	left: `translateX(${(ARROW_WIDTH - ARROW_HEIGHT) / 2}px) rotate(-90deg)`,
	right: `translateX(${(ARROW_HEIGHT - ARROW_WIDTH) / 2}px) rotate(90deg)`
};
function ContextualTourArrow({ arrowRef, placement, style }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("svg", {
		ref: arrowRef,
		"aria-hidden": "true",
		width: ARROW_WIDTH,
		height: ARROW_HEIGHT,
		viewBox: `0 0 ${ARROW_WIDTH} ${ARROW_HEIGHT}`,
		className: "absolute block overflow-visible fill-(--contextual-tour-panel-surface) stroke-(--contextual-tour-panel-border)",
		style: {
			...style,
			transform: PLACEMENT_TRANSFORM[placement]
		},
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", {
			d: `M0,0 L${ARROW_WIDTH / 2},${ARROW_HEIGHT} L${ARROW_WIDTH},0`,
			strokeWidth: 1
		})
	});
}
function ContextualTourControl({ control }) {
	switch (control.kind) {
		case "auto-rename-branch-from-work": return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AutoRenameBranchFromWorkControl, {});
	}
}
function toggleAutoRenameBranchFromWork(args) {
	const nextEnabled = !args.enabled;
	args.updateSettings({ autoRenameBranchFromWork: nextEnabled });
	if (nextEnabled) args.dispatchEvent(new Event(CONTEXTUAL_TOUR_ENABLE_AUTO_WORKSPACE_NAME_EVENT));
}
function AutoRenameBranchFromWorkControl() {
	const enabled = useAppStore((s) => s.settings?.autoRenameBranchFromWork === true);
	const updateSettings = useAppStore((s) => s.updateSettings);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "mt-3 rounded-md border border-border/70 bg-muted/35 px-3 py-2.5",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex items-center justify-between gap-3",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "min-w-0",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "text-xs font-medium text-foreground",
					children: translate("auto.components.contextual.tours.ContextualTourControl.731c5573df", "Auto-name from first message")
				})
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Switch, {
				checked: enabled,
				"aria-label": translate("auto.components.contextual.tours.ContextualTourControl.186eecc34f", "Auto-name workspace from first agent message"),
				onCheckedChange: () => {
					toggleAutoRenameBranchFromWork({
						enabled,
						updateSettings,
						dispatchEvent: (event) => window.dispatchEvent(event)
					});
				}
			})]
		})
	});
}
function ContextualTourProgressDots({ current, total }) {
	if (total <= 1) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		"aria-hidden": "true",
		className: "h-1.5 w-4"
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex items-center gap-2",
		role: "progressbar",
		"aria-valuemin": 1,
		"aria-valuemax": total,
		"aria-valuenow": current,
		"aria-label": translate("auto.components.contextual.tours.ContextualTourProgressDots.dcd6e6b03e", "Step {{value0}} of {{value1}}", {
			value0: current,
			value1: total
		}),
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "flex items-center gap-1.5",
			"aria-hidden": "true",
			children: Array.from({ length: total }).map((_, index) => {
				const isActive = index + 1 === current;
				const isComplete = index + 1 < current;
				return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: cn("block h-1.5 rounded-full transition-all duration-200 ease-out", isActive ? "w-4 bg-foreground" : isComplete ? "w-1.5 bg-foreground/55" : "w-1.5 bg-foreground/20") }, index);
			})
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
			className: "whitespace-nowrap text-[11px] font-medium leading-none text-muted-foreground",
			children: [
				current,
				" ",
				translate("auto.components.contextual.tours.ContextualTourProgressDots.7734cb8ad3", "of"),
				" ",
				total
			]
		})]
	});
}
var FOCUSABLE_SELECTOR = "button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex=\"-1\"])";
var SKIP_BUTTON_SELECTOR = "button[aria-label^=\"Skip\"], button[aria-label=\"Dismiss tour\"]";
if (typeof window !== "undefined") {
	const guardedWindow = window;
	if (!guardedWindow.__orcaContextualTourGlobalKeyGuardInstalled) {
		guardedWindow.__orcaContextualTourGlobalKeyGuardInstalled = true;
		window.addEventListener("keydown", handleContextualTourGlobalKeyDown, true);
	}
}
var PANEL_BASE_CLASSES = "orca-contextual-tour-panel rounded-lg border border-border text-popover-foreground backdrop-blur-[2px]";
var PANEL_ANIMATION_CLASSES = "animate-in fade-in-0 zoom-in-95 duration-200 ease-out";
function ContextualTourOverlaySurface({ activeTourId, renderState, panelRef, panelHost, onSkip, onBack, onNext, onStepAction, onOverlayKeyDownCapture }) {
	const arrowRef = (0, import_react.useRef)(null);
	const [floatingPosition, setFloatingPosition] = (0, import_react.useState)(null);
	const panelHostSlot = panelHost?.getAttribute("data-slot");
	const hostedPanelClass = cn(PANEL_BASE_CLASSES, PANEL_ANIMATION_CLASSES, panelHostSlot === "sheet-content" ? "absolute z-[80] w-[min(20rem,calc(100%-1.5rem))]" : "absolute z-[80] w-[min(20rem,calc(100%-2rem))]");
	const floatingPanelClass = cn(PANEL_BASE_CLASSES, PANEL_ANIMATION_CLASSES, "fixed w-[min(20rem,calc(100vw-1.5rem))]");
	const stepKey = `${activeTourId}-${renderState.progress.current}`;
	const defaultPrimaryAction = {
		kind: renderState.isLastStep ? "complete" : "next",
		label: renderState.isLastStep ? translate("auto.components.contextual.tours.ContextualTourOverlaySurface.complete", "Done") : translate("auto.components.contextual.tours.contextual.tour.overlay.measurement.38b3155418", "Next")
	};
	const primaryAction = renderState.primaryAction ?? (renderState.hidePrimaryAction ? null : defaultPrimaryAction);
	const showTargetRings = renderState.targetPulse === true;
	const targetRingStyle = showTargetRings ? {
		left: renderState.rect.left,
		top: renderState.rect.top,
		width: renderState.rect.width,
		height: renderState.rect.height
	} : void 0;
	const unresolvedPanelPosition = {
		left: 0,
		top: 0,
		visibility: "hidden"
	};
	(0, import_react.useLayoutEffect)(() => {
		const panelElement = panelRef.current;
		const arrowElement = arrowRef.current;
		if (!panelElement || !arrowElement) {
			setFloatingPosition(null);
			return;
		}
		setFloatingPosition(null);
		return watchContextualTourFloatingPosition({
			arrowElement,
			floatingElement: panelElement,
			panelHost,
			preferredPlacement: renderState.preferredPlacement,
			targetElement: renderState.targetElement,
			onPosition: setFloatingPosition
		});
	}, [
		panelHost,
		panelRef,
		renderState.preferredPlacement,
		renderState.targetElement
	]);
	const panel = /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", {
		ref: panelRef,
		"aria-live": "polite",
		"aria-label": renderState.title,
		"data-contextual-tour-panel": "",
		"data-placement": floatingPosition?.panelPlacement ?? void 0,
		role: "dialog",
		tabIndex: -1,
		className: panelHost ? hostedPanelClass : floatingPanelClass,
		style: floatingPosition?.panelPosition ?? unresolvedPanelPosition,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ContextualTourArrow, {
			arrowRef,
			placement: floatingPosition?.panelPlacement ?? renderState.preferredPlacement ?? "right",
			style: floatingPosition?.arrowPosition ?? { visibility: "hidden" }
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "animate-in fade-in-0 duration-150 ease-out p-4",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					type: "button",
					variant: "ghost",
					size: "icon-xs",
					"aria-label": renderState.isLastStep ? translate("auto.components.contextual.tours.ContextualTourOverlaySurface.d974f32a83", "Dismiss tour") : translate("auto.components.contextual.tours.ContextualTourOverlaySurface.4f86e2a10b", "Skip tour"),
					onClick: () => onSkip(activeTourId),
					className: "absolute right-2 top-2 text-muted-foreground hover:text-foreground",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, {})
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h2", {
					className: "pr-6 text-sm font-semibold tracking-tight text-foreground",
					children: renderState.title
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "mt-1.5 text-xs leading-5 text-muted-foreground",
					children: renderState.body
				}),
				renderState.control ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ContextualTourControl, { control: renderState.control }) : null,
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "mt-3.5 flex items-center justify-between gap-3",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ContextualTourProgressDots, {
						current: renderState.progress.current,
						total: renderState.progress.total
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-center gap-1.5",
						children: [
							!renderState.isFirstStep ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
								type: "button",
								variant: "ghost",
								size: "xs",
								"aria-label": translate("auto.components.contextual.tours.ContextualTourOverlaySurface.4a9568f773", "Back"),
								onClick: onBack,
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowLeft, {}), translate("auto.components.contextual.tours.ContextualTourOverlaySurface.4a9568f773", "Back")]
							}) : null,
							renderState.secondaryAction ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
								type: "button",
								variant: "ghost",
								size: "xs",
								onClick: () => onStepAction(renderState.secondaryAction),
								children: renderState.secondaryAction.label
							}) : null,
							primaryAction ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
								type: "button",
								size: "xs",
								onClick: primaryAction.kind === defaultPrimaryAction.kind && primaryAction.label === defaultPrimaryAction.label ? onNext : () => onStepAction(primaryAction),
								children: [primaryAction.label, primaryAction.kind === "next" && !renderState.isLastStep ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowRight, {}) : null]
							}) : null
						]
					})]
				})
			]
		}, stepKey)]
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: cn("fixed inset-0 z-[70] pointer-events-none"),
		"data-contextual-tour-overlay": "",
		role: "presentation",
		onKeyDownCapture: onOverlayKeyDownCapture,
		children: [showTargetRings ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			"aria-hidden": "true",
			className: "orca-contextual-tour-target-rings fixed z-[75]",
			"data-contextual-tour-target-rings": "",
			style: targetRingStyle
		}) : null, /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "pointer-events-auto",
			children: panelHost ? (0, import_react_dom.createPortal)(panel, panelHost) : panel
		})]
	});
}
function handleContextualTourOverlayKeyDown(event) {
	if (event.key === "Escape") {
		event.preventDefault();
		event.stopPropagation();
		event.currentTarget.querySelector(SKIP_BUTTON_SELECTOR)?.click();
	}
}
function handleContextualTourGlobalKeyDown(event) {
	if (!useAppStore.getState().activeContextualTourId || event.key !== "Escape") return;
	const overlay = document.querySelector("[data-contextual-tour-overlay]");
	const focusRoot = document.querySelector("[data-contextual-tour-panel]") ?? overlay;
	if (!overlay || !focusRoot) return;
	event.preventDefault();
	event.stopImmediatePropagation();
	const skipButton = focusRoot.querySelector(SKIP_BUTTON_SELECTOR);
	if (skipButton) skipButton.click();
}
function getContextualTourFocusableElements(root) {
	return Array.from(root.querySelectorAll(FOCUSABLE_SELECTOR)).filter((element) => element.getClientRects().length > 0 || element === document.activeElement);
}
function performContextualTourStepAction(args) {
	const advanceOrFinish = () => {
		if (args.isLastStep) args.finishTour();
		else args.advanceContextualTour();
	};
	switch (args.action.kind) {
		case "next":
			advanceOrFinish();
			return;
		case "complete":
			args.finishTour();
			return;
		case "split-terminal-pane":
			if (args.activeTabId) args.dispatchTerminalPaneSplit({
				tabId: args.activeTabId,
				direction: "vertical"
			});
			return;
		case "create-worktree":
			if (args.canCreateWorkspace) {
				args.detachContextualTourSource();
				args.setSidebarOpen(true);
				args.openWorkspaceComposer();
			}
			return;
		case "show-worktrees":
			args.setSidebarOpen(true);
			advanceOrFinish();
			return;
		case "open-tasks":
			args.detachContextualTourSource();
			args.openTaskPage();
			advanceOrFinish();
			return;
		case "open-getting-started":
			args.finishTour();
			args.schedule(() => {
				args.openModal("setup-guide", { telemetrySource: "contextual_tour" });
			});
	}
}
function ContextualTourOverlay() {
	const activeTourId = useAppStore((s) => s.activeContextualTourId);
	const activeStepIndex = useAppStore((s) => s.activeContextualTourStepIndex);
	const activeTourSource = useAppStore((s) => s.activeContextualTourSource);
	const wasFeaturePreviouslyInteracted = useAppStore((s) => s.activeContextualTourWasFeaturePreviouslyInteracted);
	const activeModal = useAppStore((s) => s.activeModal);
	const onboardingVisible = useAppStore((s) => s.contextualToursOnboardingVisible);
	const blockingSurfaceVisible = useAppStore((s) => s.contextualToursBlockingSurfaceVisible);
	const activeTourSuppressed = useAppStore((s) => s.activeContextualTourSuppressed);
	const keybindings = useAppStore((s) => s.keybindings);
	const activeTabId = useAppStore((s) => s.activeTabId);
	const sidebarOpen = useAppStore((s) => s.sidebarOpen);
	const canCreateWorkspace = useAppStore((s) => s.repos.length > 0);
	const markContextualToursSeen = useAppStore((s) => s.markContextualToursSeen);
	const advanceContextualTour = useAppStore((s) => s.advanceContextualTour);
	const regressContextualTour = useAppStore((s) => s.regressContextualTour);
	const dismissContextualTour = useAppStore((s) => s.dismissContextualTour);
	const completeContextualTour = useAppStore((s) => s.completeContextualTour);
	const cancelContextualTour = useAppStore((s) => s.cancelContextualTour);
	const detachContextualTourSource = useAppStore((s) => s.detachContextualTourSource);
	const setSidebarOpen = useAppStore((s) => s.setSidebarOpen);
	const openTaskPage = useAppStore((s) => s.openTaskPage);
	const openModal = useAppStore((s) => s.openModal);
	const [renderState, setRenderState] = (0, import_react.useState)(null);
	const [measureVersion, setMeasureVersion] = (0, import_react.useState)(0);
	const panelRef = (0, import_react.useRef)(null);
	const markedTourIdRef = (0, import_react.useRef)(null);
	const previousFocusRef = (0, import_react.useRef)(null);
	const focusedStepRef = (0, import_react.useRef)(null);
	const telemetryTourIdRef = (0, import_react.useRef)(null);
	const telemetryOutcomeSentRef = (0, import_react.useRef)(false);
	const telemetryStepsSeenRef = (0, import_react.useRef)(/* @__PURE__ */ new Set());
	const telemetryTotalStepsRef = (0, import_react.useRef)(1);
	const telemetryFurthestStepIndexRef = (0, import_react.useRef)(0);
	const telemetryDefinedStepCountRef = (0, import_react.useRef)(1);
	const activeTour = (0, import_react.useMemo)(() => activeTourId ? getContextualTour(activeTourId) : null, [activeTourId]);
	const emitContextualTourOutcome = (0, import_react.useCallback)((outcome) => {
		if (!activeTourId || telemetryOutcomeSentRef.current || telemetryTourIdRef.current !== activeTourId) return;
		telemetryOutcomeSentRef.current = true;
		const furthestStepIndex = telemetryFurthestStepIndexRef.current;
		trackContextualTourOutcome({
			tourId: activeTourId,
			source: activeTourSource,
			outcome,
			stepsSeen: telemetryStepsSeenRef.current.size,
			totalSteps: telemetryTotalStepsRef.current,
			...furthestStepIndex > 0 ? {
				furthestStepIndex,
				definedStepCount: telemetryDefinedStepCountRef.current
			} : {}
		});
	}, [activeTourId, activeTourSource]);
	(0, import_react.useLayoutEffect)(() => {
		if (!activeTourId) {
			setRenderState(null);
			return;
		}
		markedTourIdRef.current = null;
		telemetryTourIdRef.current = null;
		telemetryOutcomeSentRef.current = false;
		telemetryStepsSeenRef.current = /* @__PURE__ */ new Set();
		telemetryTotalStepsRef.current = 1;
		telemetryFurthestStepIndexRef.current = 0;
		telemetryDefinedStepCountRef.current = activeTour?.steps.length ?? 1;
		setRenderState(null);
	}, [activeTour?.steps.length, activeTourId]);
	(0, import_react.useEffect)(() => {
		if (!activeTour || !activeTourId) return;
		if (onboardingVisible || blockingSurfaceVisible || activeTourSuppressed || !isContextualTourAllowedForModal(activeTour, activeModal)) {
			emitContextualTourOutcome("cancelled");
			cancelContextualTour(activeTourId);
		}
	}, [
		activeModal,
		activeTourSuppressed,
		activeTour,
		activeTourId,
		blockingSurfaceVisible,
		cancelContextualTour,
		emitContextualTourOutcome,
		onboardingVisible
	]);
	(0, import_react.useEffect)(() => {
		if (!activeTourId) return;
		const scheduleMeasure = () => setMeasureVersion((version) => version + 1);
		window.addEventListener("resize", scheduleMeasure);
		window.addEventListener("scroll", scheduleMeasure, true);
		const interval = window.setInterval(scheduleMeasure, 500);
		return () => {
			window.removeEventListener("resize", scheduleMeasure);
			window.removeEventListener("scroll", scheduleMeasure, true);
			window.clearInterval(interval);
		};
	}, [activeTourId]);
	(0, import_react.useLayoutEffect)(() => {
		if (!activeTour || activeTourId === null) {
			setRenderState(null);
			return;
		}
		telemetryDefinedStepCountRef.current = activeTour.steps.length;
		const measurement = measureContextualTourOverlayRenderState({
			tour: activeTour,
			activeStepIndex,
			sidebarOpen,
			keybindings,
			previousTelemetryTotalSteps: telemetryTotalStepsRef.current
		});
		telemetryTotalStepsRef.current = Math.max(telemetryTotalStepsRef.current, measurement.kind === "render" ? measurement.telemetryTotalSteps : 0);
		if (measurement.kind === "advance") {
			advanceContextualTour();
			return;
		}
		if (measurement.kind === "wait") return;
		if (measurement.kind === "cancel") {
			emitContextualTourOutcome("cancelled");
			cancelContextualTour(activeTourId);
			return;
		}
		setRenderState(measurement.renderState);
	}, [
		activeStepIndex,
		activeTour,
		activeTourId,
		advanceContextualTour,
		cancelContextualTour,
		emitContextualTourOutcome,
		keybindings,
		measureVersion,
		sidebarOpen
	]);
	(0, import_react.useEffect)(() => {
		if (!activeTourId || !renderState || markedTourIdRef.current === activeTourId) return;
		markedTourIdRef.current = activeTourId;
		markContextualToursSeen([activeTourId]);
	}, [
		activeTourId,
		markContextualToursSeen,
		renderState
	]);
	(0, import_react.useEffect)(() => {
		if (!activeTourId || !renderState || telemetryTourIdRef.current === activeTourId) return;
		telemetryTourIdRef.current = activeTourId;
		telemetryStepsSeenRef.current.add(activeStepIndex);
		telemetryFurthestStepIndexRef.current = Math.max(telemetryFurthestStepIndexRef.current, activeStepIndex + 1);
		trackContextualTourShown({
			tourId: activeTourId,
			source: activeTourSource,
			wasFeaturePreviouslyInteracted
		});
	}, [
		activeStepIndex,
		activeTourId,
		activeTourSource,
		renderState,
		wasFeaturePreviouslyInteracted
	]);
	(0, import_react.useEffect)(() => {
		if (!activeTourId || !renderState) return;
		telemetryStepsSeenRef.current.add(activeStepIndex);
		telemetryFurthestStepIndexRef.current = Math.max(telemetryFurthestStepIndexRef.current, activeStepIndex + 1);
	}, [
		activeStepIndex,
		activeTourId,
		renderState
	]);
	(0, import_react.useEffect)(() => {
		if (!activeTourId) return;
		const emitPendingCancellation = () => {
			emitContextualTourOutcome(getContextualTourCleanupOutcome(activeTourId));
		};
		window.addEventListener("beforeunload", emitPendingCancellation);
		return () => {
			window.removeEventListener("beforeunload", emitPendingCancellation);
			emitPendingCancellation();
		};
	}, [activeTourId, emitContextualTourOutcome]);
	(0, import_react.useEffect)(() => {
		if (!activeTourId || !renderState) return;
		const focusKey = `${activeTourId}:${activeStepIndex}`;
		if (focusedStepRef.current === focusKey) return;
		focusedStepRef.current = focusKey;
		const currentFocus = document.activeElement;
		if (!previousFocusRef.current && currentFocus instanceof HTMLElement && !panelRef.current?.contains(currentFocus)) previousFocusRef.current = currentFocus;
		const timeout = window.setTimeout(() => {
			const panel = panelRef.current;
			((panel ? getContextualTourFocusableElements(panel)[0] : null) ?? panel)?.focus({ preventScroll: true });
		}, 0);
		return () => window.clearTimeout(timeout);
	}, [
		activeStepIndex,
		activeTourId,
		renderState
	]);
	(0, import_react.useEffect)(() => {
		if (activeTourId) return;
		focusedStepRef.current = null;
		const previousFocus = previousFocusRef.current;
		previousFocusRef.current = null;
		if (previousFocus?.isConnected) previousFocus.focus({ preventScroll: true });
	}, [activeTourId]);
	if (!activeTourId || !renderState) return null;
	const finishTour = () => {
		emitContextualTourOutcome("completed");
		completeContextualTour(activeTourId);
	};
	const handleStepAction = (action) => {
		performContextualTourStepAction({
			action,
			activeTabId,
			isLastStep: renderState.isLastStep,
			finishTour,
			advanceContextualTour,
			detachContextualTourSource: () => {
				if (activeTourSource) detachContextualTourSource(activeTourId, activeTourSource);
			},
			setSidebarOpen,
			openTaskPage,
			openModal,
			canCreateWorkspace,
			openWorkspaceComposer: openWorkspaceCreationComposerWithTourHandoff,
			dispatchTerminalPaneSplit: requestActiveTerminalPaneSplit,
			schedule: (callback) => {
				window.setTimeout(callback, 0);
			}
		});
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ContextualTourOverlaySurface, {
		activeTourId,
		renderState,
		panelRef,
		panelHost: renderState.panelHost,
		onSkip: (id) => {
			emitContextualTourOutcome("skipped");
			dismissContextualTour(id);
		},
		onBack: regressContextualTour,
		onNext: () => {
			if (renderState.isLastStep) finishTour();
			else advanceContextualTour();
		},
		onStepAction: handleStepAction,
		onOverlayKeyDownCapture: handleContextualTourOverlayKeyDown
	});
}
export { ContextualTourOverlay, getContextualTourCleanupOutcome };
