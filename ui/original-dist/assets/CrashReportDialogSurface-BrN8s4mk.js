import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as Button } from "./button-DszXJEV6.js";
import { t as Clipboard } from "./clipboard-CEJLS54W.js";
import { t as Send } from "./send-CyCdniMF.js";
import { t as TriangleAlert } from "./triangle-alert-HrLt1y9s.js";
import "./react-dom-Da8MQai-.js";
import { n as toast } from "./dist-DgqligFk.js";
import "./es2015-B5WZ-7WO.js";
import { t as Checkbox } from "./checkbox-PAbetBh2.js";
import { t as Label } from "./label-D-n9s_wS.js";
import { t as useMountedRef } from "./useMountedRef-1omUd-IV.js";
import { a as DialogFooter, i as DialogDescription, o as DialogHeader, r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
function appendDiagnosticBundleLines(lines, diagnosticBundle, sanitizeString) {
	if (!diagnosticBundle) return;
	lines.push("", "Diagnostic log:");
	if (diagnosticBundle.status === "attached") {
		lines.push("- Status: attached", `- Bundle submission ID: ${sanitizeString(diagnosticBundle.bundleSubmissionId)}`, `- Spans: ${diagnosticBundle.spanCount}`, `- Bytes: ${diagnosticBundle.bytes}`);
		return;
	}
	if (diagnosticBundle.status === "uploaded") {
		lines.push("- Status: uploaded", `- Ticket ID: ${sanitizeString(diagnosticBundle.ticketId)}`, `- Bundle submission ID: ${sanitizeString(diagnosticBundle.bundleSubmissionId)}`, `- Spans: ${diagnosticBundle.spanCount}`, `- Bytes: ${diagnosticBundle.bytes}`);
		return;
	}
	lines.push("- Status: not uploaded", `- Reason: ${sanitizeString(diagnosticBundle.reason)}`);
	if (diagnosticBundle.bundleSubmissionId) lines.push(`- Bundle submission ID: ${sanitizeString(diagnosticBundle.bundleSubmissionId)}`);
	if (typeof diagnosticBundle.spanCount === "number") lines.push(`- Spans: ${diagnosticBundle.spanCount}`);
	if (typeof diagnosticBundle.bytes === "number") lines.push(`- Bytes: ${diagnosticBundle.bytes}`);
}
var MAX_STRING_DETAIL_LENGTH = 240;
var MAX_FORMATTED_REPORT_LENGTH = 64e3;
var FORMATTED_REPORT_TRUNCATION_SUFFIX = "\n\n[Crash report truncated to fit feedback endpoint limits.]";
var SECRET_PATTERNS = [
	/\b(gh[pousr]_[A-Za-z0-9_]{20,})\b/g,
	/\b(sk-[A-Za-z0-9_-]{20,})\b/g,
	/\b([A-Za-z0-9._%+-]+:[A-Za-z0-9._%+-]+@)(?=[^/\s]+)/g,
	/\b(token|api[_-]?key|secret|password)=([^&\s]+)/gi
];
var PATH_PATTERNS = [
	/\/(?:Users|home)\/(?:(?!\s+(?:\/|[A-Za-z]:\\|\\\\|gh[pousr]_|sk-|(?:token|api[_-]?key|secret|password)=))[^"'`<>\n\r)])+/gi,
	/\/(?:Applications|Library|System|Volumes|etc|media|mnt|opt|private|root|srv|tmp|usr|var)\/(?:(?!\s+(?:\/|[A-Za-z]:\\|\\\\|gh[pousr]_|sk-|(?:token|api[_-]?key|secret|password)=))[^"'`<>\n\r)])+/gi,
	/\/[A-Za-z0-9._ -]+\/(?:(?!\s+(?:\/|[A-Za-z]:\\|\\\\|gh[pousr]_|sk-|(?:token|api[_-]?key|secret|password)=))[^"'`<>\n\r)])+/gi,
	/[A-Za-z]:\\(?:(?!\s+(?:\/|[A-Za-z]:\\|\\\\|gh[pousr]_|sk-|(?:token|api[_-]?key|secret|password)=))[^"'`<>\n\r)])+/gi,
	/\\\\[^\\\s"'`<>\n\r)]+\\(?:(?!\s+(?:\/|[A-Za-z]:\\|\\\\|gh[pousr]_|sk-|(?:token|api[_-]?key|secret|password)=))[^"'`<>\n\r)])+/gi
];
function isReactErrorBoundaryReport(report) {
	return report.source === "renderer" && report.processType === "react-render" && report.reason === "react-error-boundary";
}
function sanitizeCrashReportString(value, maxLength = MAX_STRING_DETAIL_LENGTH) {
	let sanitized = value;
	for (const pattern of PATH_PATTERNS) sanitized = sanitized.replace(pattern, "[redacted-path]");
	for (const pattern of SECRET_PATTERNS) sanitized = sanitized.replace(pattern, (match, key) => {
		if (key && /^(token|api[_-]?key|secret|password)$/i.test(key)) return `${key}=[redacted]`;
		return match.includes("@") ? "[redacted-credential]@" : "[redacted-secret]";
	});
	return sanitized.length > maxLength ? `${sanitized.slice(0, maxLength)}...` : sanitized;
}
function formatCrashReportText(report, notes, diagnosticBundle) {
	const lines = [
		"[Crash Report]",
		"",
		`Report ID: ${report.id}`,
		`Created: ${report.createdAt}`,
		`Status: ${report.status}`,
		`Source: ${report.source}`,
		`Process: ${report.processType}`,
		`Reason: ${report.reason}`,
		`Exit code: ${report.exitCode ?? "unknown"}`,
		`App version: ${report.appVersion}`,
		`Platform: ${report.platform} ${report.osRelease} ${report.arch}`,
		`Electron: ${report.electronVersion}`,
		`Chrome: ${report.chromeVersion}`
	];
	appendDiagnosticBundleLines(lines, diagnosticBundle, sanitizeCrashReportString);
	const details = Object.entries(report.details);
	if (details.length > 0) {
		lines.push("", "Details:");
		for (const [key, value] of details) lines.push(`- ${key}: ${String(value)}`);
	}
	if (report.breadcrumbs && report.breadcrumbs.length > 0) {
		lines.push("", "Recent activity:");
		for (const breadcrumb of report.breadcrumbs) {
			const data = breadcrumb.data ? Object.entries(breadcrumb.data) : [];
			const suffix = data.length > 0 ? ` (${data.map(([key, value]) => `${key}=${String(value)}`).join(", ")})` : "";
			lines.push(`- ${breadcrumb.createdAt}: ${breadcrumb.name}${suffix}`);
		}
	}
	const trimmedNotes = notes?.trim();
	if (trimmedNotes) lines.push("", "User notes:", sanitizeCrashReportString(trimmedNotes));
	return truncateFormattedCrashReport(lines.join("\n"));
}
function truncateFormattedCrashReport(text) {
	if (text.length <= MAX_FORMATTED_REPORT_LENGTH) return text;
	const budget = MAX_FORMATTED_REPORT_LENGTH - 59;
	return `${text.slice(0, Math.max(0, budget)).trimEnd()}${FORMATTED_REPORT_TRUNCATION_SUFFIX}`;
}
const CRASH_REPORT_SUBMIT_FAILURE_TOAST_ID = "crash-report-submit-failure";
function normalizedFailureMessage(error) {
	return sanitizeCrashReportString(error instanceof Error ? error.message : typeof error === "string" ? error : "").trim() || translate("auto.components.crash.report.submit.notice.unknownError", "The crash report request failed before it returned a reason.");
}
function asSentence(message) {
	return /[.!?]$/.test(message) ? message : `${message}.`;
}
function getCrashReportCopySubmissionFailure(failure) {
	const diagnosticContext = failure.diagnosticBundle?.status === "uploaded" ? {
		status: "uploaded",
		ticketId: sanitizeCrashReportString(failure.diagnosticBundle.ticketId)
	} : failure.diagnosticBundle?.status === "not_uploaded" ? {
		status: "not_uploaded",
		reason: normalizedFailureMessage(failure.diagnosticBundle.reason)
	} : void 0;
	return {
		error: normalizedFailureMessage(failure.error),
		...diagnosticContext ? { diagnosticContext } : {}
	};
}
function getCrashReportSubmitFailureNotice(failure, includeDiagnosticLogs) {
	const ticketDetail = failure.diagnosticBundle?.status === "uploaded" ? translate("auto.components.crash.report.submit.notice.ticketUploaded", "Diagnostic ticket {{value0}} was uploaded but not linked.", { value0: sanitizeCrashReportString(failure.diagnosticBundle.ticketId) }) : null;
	const omittedDetail = failure.diagnosticBundle?.status === "not_uploaded" ? asSentence(translate("auto.components.crash.report.submit.notice.diagnosticsReason", "Diagnostic logs were not attached: {{value0}}", { value0: normalizedFailureMessage(failure.diagnosticBundle.reason) })) : null;
	const attachmentAlreadyOmitted = failure.diagnosticBundle?.status === "not_uploaded";
	const recovery = includeDiagnosticLogs && !attachmentAlreadyOmitted ? translate("auto.components.crash.report.submit.notice.uncheckDiagnostics", "Uncheck \"Attach recent diagnostic logs\" and try again, or copy the details.") : translate("auto.components.crash.report.submit.notice.checkConnection", "Check your connection and try again, or copy the details.");
	return {
		title: translate("auto.components.crash.report.submit.notice.notSent", "Crash report wasn't sent"),
		description: [
			asSentence(normalizedFailureMessage(failure.error)),
			ticketDetail,
			omittedDetail,
			recovery
		].filter((part) => Boolean(part)).join(" "),
		actionLabel: translate("auto.components.crash.report.submit.notice.copyDetails", "Copy Details")
	};
}
function getCrashReportSubmitWarningNotice(result, includeDiagnosticLogs) {
	if (!includeDiagnosticLogs || result.diagnosticBundle?.status !== "not_uploaded") return null;
	return {
		title: translate("auto.components.crash.report.submit.notice.sentWithoutDiagnostics", "Crash report sent without diagnostic logs"),
		description: translate("auto.components.crash.report.submit.notice.diagnosticsReason", "Diagnostic logs were not attached: {{value0}}", { value0: normalizedFailureMessage(result.diagnosticBundle.reason) })
	};
}
const CRASH_REPORT_COPY_FAILURE_TOAST_ID = "crash-report-copy-failure";
function showCopyFailure(description) {
	toast.error(translate("auto.components.crash.report.copy.copyFailed", "Crash report details could not be copied."), {
		id: CRASH_REPORT_COPY_FAILURE_TOAST_ID,
		...description ? { description } : {},
		duration: Infinity,
		dismissible: true
	});
}
function useCrashReportCopy(report, notes) {
	const reportId = report?.id ?? null;
	const notesRef = (0, import_react.useRef)({
		reportId,
		value: notes
	});
	if (notesRef.current.reportId === reportId) notesRef.current.value = notes;
	else notesRef.current = {
		reportId,
		value: notes
	};
	const reportNotes = notesRef.current;
	return (0, import_react.useCallback)(async (submissionFailure) => {
		try {
			const result = await window.api.crashReports.copyLatestDiagnostics({
				...report ? { reportId: report.id } : {},
				notes: reportNotes.value,
				...submissionFailure ? { submissionFailure } : {}
			});
			if (!result.ok) {
				showCopyFailure(result.error);
				return;
			}
			toast.dismiss(CRASH_REPORT_COPY_FAILURE_TOAST_ID);
			toast.success(translate("auto.components.crash.report.CrashReportDialog.8b8473c544", "Crash report copied."));
		} catch (error) {
			console.error("Failed to copy crash report details:", error);
			showCopyFailure();
		}
	}, [report, reportNotes]);
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function formatSummary(report) {
	if (isReactErrorBoundaryReport(report)) {
		const surface = typeof report.details.surface === "string" ? report.details.surface : null;
		return surface ? `React render error in ${surface}` : "React render error";
	}
	return `${report.processType} ${report.reason}${report.exitCode === null ? "" : ` (exit ${report.exitCode})`}`;
}
function getDialogTitle(report) {
	if (!report) return "Report a crash";
	return report && isReactErrorBoundaryReport(report) ? "Orca hit a recoverable UI error" : "Orca closed unexpectedly";
}
function getDialogDescription(report) {
	if (!report) return "Send a privacy-safe crash report. Recent redacted diagnostic logs are included when available.";
	return report && isReactErrorBoundaryReport(report) ? "Send a privacy-safe diagnostic report to help us understand the failed UI surface." : "Send a privacy-safe diagnostic report to help us understand what happened.";
}
function getNotesPlaceholder(report) {
	if (!report) return "Optional: what happened?";
	return report && isReactErrorBoundaryReport(report) ? "Optional: what were you doing before this UI error?" : "Optional: what were you doing before Orca closed?";
}
function CrashReportDialogSurface({ open, report, loading, onOpenChange, onReportChange }) {
	const mountedRef = useMountedRef();
	const [notes, setNotes] = (0, import_react.useState)("");
	const [includeDiagnosticLogs, setIncludeDiagnosticLogs] = (0, import_react.useState)(true);
	const [submitting, setSubmitting] = (0, import_react.useState)(false);
	const [viewer, setViewer] = (0, import_react.useState)(null);
	const viewerRequestIdRef = (0, import_react.useRef)(0);
	const deferredNotes = (0, import_react.useDeferredValue)(notes);
	const diagnosticText = (0, import_react.useMemo)(() => report ? formatCrashReportText(report, deferredNotes) : "", [deferredNotes, report]);
	const copyCrashReportDetails = useCrashReportCopy(report, notes);
	const clearViewer = (0, import_react.useCallback)(() => {
		viewerRequestIdRef.current += 1;
		setViewer(null);
	}, []);
	const loadViewerForOpenDialog = (0, import_react.useCallback)(() => {
		const requestId = ++viewerRequestIdRef.current;
		setViewer(null);
		window.api.gh.viewer().then((nextViewer) => {
			if (mountedRef.current && requestId === viewerRequestIdRef.current) setViewer(nextViewer);
		}).catch((error) => {
			if (mountedRef.current && requestId === viewerRequestIdRef.current) {
				setViewer(null);
				console.error("Failed to load GitHub viewer for crash report:", error);
			}
		});
	}, [mountedRef]);
	(0, import_react.useEffect)(() => {
		if (!open) {
			clearViewer();
			return;
		}
		setIncludeDiagnosticLogs(true);
		loadViewerForOpenDialog();
	}, [
		clearViewer,
		loadViewerForOpenDialog,
		open
	]);
	const showSubmitFailure = (error, diagnosticBundle) => {
		const failure = {
			error,
			...diagnosticBundle ? { diagnosticBundle } : {}
		};
		const notice = getCrashReportSubmitFailureNotice(failure, includeDiagnosticLogs);
		const copyFailure = getCrashReportCopySubmissionFailure(failure);
		toast.error(notice.title, {
			id: CRASH_REPORT_SUBMIT_FAILURE_TOAST_ID,
			description: notice.description,
			duration: Infinity,
			dismissible: true,
			action: {
				label: notice.actionLabel,
				onClick: () => {
					copyCrashReportDetails(copyFailure);
				}
			}
		});
	};
	const dismissReportIfNeeded = async () => {
		if (report?.status === "pending") {
			await window.api.crashReports.dismiss({ reportId: report.id });
			if (mountedRef.current) onReportChange({
				...report,
				status: "dismissed"
			});
		}
	};
	const handleDismiss = async () => {
		await dismissReportIfNeeded();
		if (mountedRef.current) onOpenChange(false);
	};
	const handleSubmit = async () => {
		setSubmitting(true);
		try {
			const result = await window.api.crashReports.submit({
				...report ? { reportId: report.id } : {},
				notes,
				includeDiagnosticLogs,
				submitAnonymously: !viewer,
				githubLogin: viewer?.login ?? null,
				githubEmail: null
			});
			if (!result.ok) {
				showSubmitFailure(result.error, result.diagnosticBundle);
				console.error("Failed to submit crash report:", result.error);
				return;
			}
			if (!mountedRef.current) return;
			onReportChange(result.report);
			setNotes("");
			toast.dismiss(CRASH_REPORT_SUBMIT_FAILURE_TOAST_ID);
			const warningNotice = getCrashReportSubmitWarningNotice(result, includeDiagnosticLogs);
			if (warningNotice) toast.warning(warningNotice.title, { description: warningNotice.description });
			else toast.success(translate("auto.components.crash.report.CrashReportDialog.8e24fe4f75", "Crash report sent."));
			onOpenChange(false);
		} catch (error) {
			showSubmitFailure(error);
			console.error("Failed to submit crash report:", error);
		} finally {
			if (mountedRef.current) setSubmitting(false);
		}
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
		open,
		onOpenChange: (nextOpen) => {
			if (submitting && !nextOpen) return;
			if (!nextOpen) {
				clearViewer();
				dismissReportIfNeeded().finally(() => {
					if (mountedRef.current) onOpenChange(false);
				});
				return;
			}
			onOpenChange(true);
		},
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogContent, {
			className: "sm:max-w-xl",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogTitle, {
					className: "flex items-center gap-2 text-sm",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "size-4 text-destructive" }), getDialogTitle(report)]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, {
					className: "text-xs",
					children: getDialogDescription(report)
				})] }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "min-w-0 space-y-3",
					children: [
						report ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "rounded-md border border-border/70 bg-muted/30 p-3 text-xs",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "font-medium text-foreground",
								children: formatSummary(report)
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "mt-1 text-muted-foreground",
								children: [
									new Date(report.createdAt).toLocaleString(),
									" · ",
									report.platform,
									" ",
									report.arch,
									" ·",
									translate("auto.components.crash.report.CrashReportDialog.835037edc9", "Orca"),
									" ",
									report.appVersion
								]
							})]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "min-w-0 space-y-1.5",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "text-[11px] font-medium text-muted-foreground",
								children: translate("auto.components.crash.report.CrashReportDialog.6d3ebe216a", "Diagnostic text")
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("pre", {
								className: "max-h-44 overflow-auto whitespace-pre-wrap [overflow-wrap:anywhere] rounded-md border border-border bg-muted/20 p-3 font-mono text-[11px] leading-5 text-muted-foreground scrollbar-sleek",
								children: diagnosticText
							})]
						})] }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "rounded-md border border-border/70 bg-muted/30 p-3 text-xs text-muted-foreground",
							children: loading ? translate("auto.components.crash.report.CrashReportDialog.765591798d", "Checking for crash reports...") : translate("auto.components.crash.report.CrashReportDialog.ead6fc0510", "No automatic crash report was captured. You can still send details and include recent diagnostic logs when available.")
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("textarea", {
							value: notes,
							onChange: (event) => setNotes(event.target.value),
							rows: 4,
							placeholder: getNotesPlaceholder(report),
							className: "min-h-24 w-full rounded-md border border-border bg-background px-3 py-2 text-sm outline-none ring-offset-background placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex items-start gap-2 rounded-md border border-border/70 bg-muted/20 p-3",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Checkbox, {
								id: "crash-report-attach-diagnostics",
								checked: includeDiagnosticLogs,
								onCheckedChange: (checked) => setIncludeDiagnosticLogs(checked === true),
								disabled: submitting,
								className: "mt-0.5"
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "space-y-1",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
									htmlFor: "crash-report-attach-diagnostics",
									className: "text-xs",
									children: translate("auto.components.crash.report.CrashReportDialog.b082f27490", "Attach recent diagnostic logs")
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "text-xs leading-5 text-muted-foreground",
									children: translate("auto.components.crash.report.CrashReportDialog.e59f0b9427", "Sends a capped redacted log bundle with the report.")
								})]
							})]
						})
					]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogFooter, {
					className: "gap-2",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
							type: "button",
							variant: "outline",
							size: "sm",
							onClick: () => void copyCrashReportDetails(),
							disabled: loading,
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Clipboard, { className: "size-3.5" }), translate("auto.components.crash.report.CrashReportDialog.50b00dc327", "Copy Details")]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							type: "button",
							variant: "ghost",
							size: "sm",
							onClick: handleDismiss,
							disabled: submitting,
							children: translate("auto.components.crash.report.CrashReportDialog.88fea8e84e", "Don't Send")
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
							type: "button",
							size: "sm",
							onClick: handleSubmit,
							disabled: loading || submitting,
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Send, { className: "size-3.5" }), translate("auto.components.crash.report.CrashReportDialog.b4951cd27c", "Send Report")]
						})
					]
				})
			]
		})
	});
}
export { CrashReportDialogSurface };
