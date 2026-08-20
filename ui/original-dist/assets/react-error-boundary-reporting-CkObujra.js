const __vite__mapDeps=(i,m=__vite__mapDeps,d=(m.f||(m.f=["./store-BgrlX3wd.js","./button-DszXJEV6.js","./jsx-runtime-Cv_nyRjc.js","./preload-helper-Cgw39-ka.js","./chunk-Dhmk_5SA.js","./react-Da2TLWQy.js","./store-CgXrfmaH.js","./defineProperty-BAtR-r70.js","./dist-DgqligFk.js","./react-dom-Da8MQai-.js","./plugin-manifest-Bs-50M_g.js","./useMountedRef-1omUd-IV.js","./agent-status-3vUKbY6l.js","./agent-kind-Dfx6MnkP.js","./telemetry-ZyUPyKMD.js"])))=>i.map(i=>d[i]);
import { t as __vitePreload } from "./preload-helper-Cgw39-ka.js";
var reportedRendererErrorKeys = [];
var reportedRendererErrorKeySet = /* @__PURE__ */ new Set();
var MAX_REPORTED_RENDERER_ERROR_KEYS = 50;
var pendingReactErrorBoundaryReport = null;
const REACT_ERROR_BOUNDARY_REPORT_AVAILABLE_EVENT = "orca:react-error-boundary-report-available";
function stringFromThrown(value) {
	if (value instanceof Error) return {
		name: value.name || "Error",
		message: value.message || String(value),
		...value.stack ? { stack: value.stack } : {}
	};
	return {
		name: "NonErrorThrown",
		message: String(value)
	};
}
async function collectRendererErrorContext() {
	try {
		const { useAppStore } = await __vitePreload(async () => {
			const { useAppStore: useAppStore$1 } = await import("./store-BgrlX3wd.js");
			return { useAppStore: useAppStore$1 };
		}, __vite__mapDeps([0,1,2,3,4,5,6,7,8,9,10,11,12,13,14]), import.meta.url);
		const state = useAppStore.getState();
		return {
			activeView: state.activeView,
			activeModal: state.activeModal,
			activeTabType: state.activeTabType,
			activeRightSidebarTab: state.rightSidebarTab,
			hasActiveWorktree: state.activeWorktreeId !== null
		};
	} catch {
		return {};
	}
}
function buildReactErrorBoundaryReportArgs({ boundaryId, surface, error, errorInfo, context }) {
	const fields = stringFromThrown(error);
	const componentStack = errorInfo?.componentStack?.trim();
	return {
		boundaryId,
		surface,
		errorName: fields.name,
		errorMessage: fields.message,
		...fields.stack ? { errorStack: fields.stack } : {},
		...componentStack ? { componentStack } : {},
		...context?.activeView ? { activeView: context.activeView } : {},
		...context?.activeModal !== void 0 ? { activeModal: context.activeModal } : {},
		...context?.activeTabType ? { activeTabType: context.activeTabType } : {},
		...context?.activeRightSidebarTab ? { activeRightSidebarTab: context.activeRightSidebarTab } : {},
		...context?.hasActiveWorktree !== void 0 ? { hasActiveWorktree: context.hasActiveWorktree } : {}
	};
}
function rememberRendererErrorKey(key) {
	if (reportedRendererErrorKeySet.has(key)) return false;
	reportedRendererErrorKeySet.add(key);
	reportedRendererErrorKeys.push(key);
	if (reportedRendererErrorKeys.length > MAX_REPORTED_RENDERER_ERROR_KEYS) {
		const expiredKey = reportedRendererErrorKeys.shift();
		if (expiredKey) reportedRendererErrorKeySet.delete(expiredKey);
	}
	return true;
}
function getRendererErrorKey(args) {
	return JSON.stringify({
		boundaryId: args.boundaryId,
		surface: args.surface,
		errorName: args.errorName,
		errorMessage: args.errorMessage,
		componentStack: args.componentStack
	});
}
function takePendingReactErrorBoundaryReport() {
	const report = pendingReactErrorBoundaryReport;
	pendingReactErrorBoundaryReport = null;
	return report;
}
function notifyReactErrorBoundaryReportAvailable(report) {
	pendingReactErrorBoundaryReport = report;
	window.dispatchEvent(new CustomEvent(REACT_ERROR_BOUNDARY_REPORT_AVAILABLE_EVENT));
}
async function reportReactErrorBoundaryCrash(input) {
	const context = await collectRendererErrorContext();
	const args = buildReactErrorBoundaryReportArgs({
		...input,
		context
	});
	if (!rememberRendererErrorKey(getRendererErrorKey(args))) return;
	try {
		const result = await window.api?.crashReports?.recordRendererError?.(args);
		if (result && !result.ok) {
			console.warn("[react-error-boundary] Failed to record renderer crash:", result.error);
			return;
		}
		if (result?.ok && result.report && !result.deduped) notifyReactErrorBoundaryReportAvailable(result.report);
	} catch (error) {
		console.warn("[react-error-boundary] Crash reporting IPC failed:", error);
	}
}
export { reportReactErrorBoundaryCrash as n, takePendingReactErrorBoundaryReport as r, REACT_ERROR_BOUNDARY_REPORT_AVAILABLE_EVENT as t };
