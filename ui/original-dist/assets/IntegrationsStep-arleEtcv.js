import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn, t as Button } from "./button-DszXJEV6.js";
import { t as OnboardingInlineCommandTerminal } from "./OnboardingInlineCommandTerminal-B5SNC4Sp.js";
import { t as ExternalLink } from "./external-link-BrcDtGAn.js";
import { t as Github } from "./github-Dx7d1WbZ.js";
import { t as Terminal } from "./terminal-Cen7Un9b.js";
import { t as useAppStore } from "./store-CgXrfmaH.js";
import { t as LinearIcon } from "./LinearIcon-CTELA_97.js";
import { t as LinearApiKeyDialog } from "./linear-api-key-dialog-BorWNZv7.js";
import { t as IntegrationStatusPill } from "./integration-status-pill-CCHE4njv.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function getGitHubSetupState(status) {
	if (!status) return "checking";
	if (!status.gh.installed) return "not-installed";
	return status.gh.authenticated ? "connected" : "not-authenticated";
}
function GitHubRow(props = {}) {
	const { compact = false } = props;
	const preflightStatus = useAppStore((s) => s.preflightStatus);
	const preflightStatusLoading = useAppStore((s) => s.preflightStatusLoading);
	const refreshPreflightStatus = useAppStore((s) => s.refreshPreflightStatus);
	const state = preflightStatusLoading ? "checking" : getGitHubSetupState(preflightStatus);
	const [githubTerminalOpen, setGithubTerminalOpen] = (0, import_react.useState)(false);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "rounded-xl border border-border bg-muted/20",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: cn(compact ? "flex flex-col gap-3 p-4" : "flex items-start gap-4 p-5"),
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: cn("flex items-start gap-3", compact ? "" : "gap-4 flex-1 min-w-0"),
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "flex size-10 shrink-0 items-center justify-center rounded-lg border border-border bg-background text-foreground",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Github, { className: "size-5" })
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "min-w-0 flex-1",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex flex-wrap items-center gap-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h3", {
							className: "text-[15px] font-semibold leading-tight text-foreground",
							children: translate("auto.components.onboarding.IntegrationsStep.217beb0658", "GitHub")
						}), state === "connected" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(IntegrationStatusPill, {
							tone: "connected",
							children: translate("auto.components.onboarding.IntegrationsStep.c91a5782f1", "Connected")
						}) : state === "not-installed" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(IntegrationStatusPill, {
							tone: "attention",
							children: translate("auto.components.onboarding.IntegrationsStep.5c115cb713", "CLI not installed")
						}) : state === "not-authenticated" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(IntegrationStatusPill, {
							tone: "attention",
							children: translate("auto.components.onboarding.IntegrationsStep.8405043962", "Sign in needed")
						}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(IntegrationStatusPill, {
							tone: "neutral",
							children: translate("auto.components.onboarding.IntegrationsStep.c1547656f0", "Checking…")
						})]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "mt-1 text-[13px] leading-relaxed text-muted-foreground",
						children: translate("auto.components.onboarding.IntegrationsStep.50db38cf4b", "Pull requests, issues, and check status.")
					})]
				})]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: cn("flex items-center gap-2", compact ? "flex-wrap" : "shrink-0"),
				children: [
					state === "not-installed" ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
						variant: "outline",
						size: "sm",
						onClick: () => window.api.shell.openUrl("https://cli.github.com"),
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExternalLink, { className: "size-3.5" }), translate("auto.components.onboarding.IntegrationsStep.bd5d976fb2", "Install gh")]
					}) : null,
					state === "not-authenticated" ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
						variant: "outline",
						size: "sm",
						disabled: githubTerminalOpen,
						onClick: () => setGithubTerminalOpen(true),
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Terminal, { className: "size-3.5" }), githubTerminalOpen ? translate("auto.components.onboarding.IntegrationsStep.0b4a7d23ab", "Signing in") : translate("auto.components.onboarding.IntegrationsStep.d6e5dba05a", "Sign in")]
					}) : null,
					state !== "connected" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						variant: "ghost",
						size: "sm",
						onClick: () => void refreshPreflightStatus({ force: true }),
						children: translate("auto.components.onboarding.IntegrationsStep.80e3ce0bc9", "Re-check")
					}) : null
				]
			})]
		}), state === "not-authenticated" && githubTerminalOpen ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: cn(compact ? "px-4 pb-4" : "px-5 pb-5"),
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(OnboardingInlineCommandTerminal, {
				command: "gh auth login",
				title: translate("auto.components.onboarding.IntegrationsStep.6d469169f2", "GitHub setup"),
				ariaLabel: translate("auto.components.onboarding.IntegrationsStep.f9d2e12d17", "GitHub sign in command"),
				description: translate("auto.components.onboarding.IntegrationsStep.af69f42372", "Press Enter to run GitHub CLI auth. Re-check GitHub after the browser or device flow finishes.")
			})
		}) : null]
	});
}
function LinearRow(props = {}) {
	const { compact = false } = props;
	const linearStatus = useAppStore((s) => s.linearStatus);
	const checkLinearConnection = useAppStore((s) => s.checkLinearConnection);
	const [dialogOpen, setDialogOpen] = (0, import_react.useState)(false);
	const workspaceCount = linearStatus.workspaces?.length ?? (linearStatus.connected ? 1 : 0);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "rounded-xl border border-border bg-muted/20",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: cn(compact ? "flex flex-col gap-3 p-4" : "flex items-start gap-4 p-5"),
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: cn("flex items-start gap-3", compact ? "" : "gap-4 flex-1 min-w-0"),
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "flex size-10 shrink-0 items-center justify-center rounded-lg border border-border bg-background text-foreground",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LinearIcon, { className: "size-5" })
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "min-w-0 flex-1",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex flex-wrap items-center gap-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h3", {
							className: "text-[15px] font-semibold leading-tight text-foreground",
							children: translate("auto.components.onboarding.IntegrationsStep.27743304b1", "Linear")
						}), linearStatus.connected ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(IntegrationStatusPill, {
							tone: "connected",
							children: translate("auto.components.onboarding.IntegrationsStep.c91a5782f1", "Connected")
						}) : null]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "mt-1 text-[13px] leading-relaxed text-muted-foreground",
						children: linearStatus.connected ? translate("auto.components.onboarding.IntegrationsStep.b08a6ac93c", "{{value0}} workspace{{value1}} linked. Add another workspace or replace a restricted key any time.", {
							value0: workspaceCount,
							value1: workspaceCount === 1 ? "" : "s"
						}) : translate("auto.components.onboarding.IntegrationsStep.4983ae7433", "Add Linear access with a Personal API key. Full-access keys can show every team the key owner can access.")
					})]
				})]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: cn("flex items-center gap-2", compact ? "flex-wrap" : "shrink-0"),
				children: [linearStatus.connected ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					variant: "outline",
					size: "sm",
					onClick: () => setDialogOpen(true),
					children: translate("auto.components.onboarding.IntegrationsStep.dd9c186a8b", "Add workspace access")
				}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					size: "sm",
					onClick: () => setDialogOpen(true),
					children: translate("auto.components.onboarding.IntegrationsStep.04ef416712", "Add Linear access")
				}), !linearStatus.connected ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					variant: "ghost",
					size: "sm",
					onClick: () => void checkLinearConnection(true),
					children: translate("auto.components.onboarding.IntegrationsStep.80e3ce0bc9", "Re-check")
				}) : null]
			})]
		})
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LinearApiKeyDialog, {
		open: dialogOpen,
		onOpenChange: setDialogOpen,
		overlayClassName: "z-[110]",
		contentClassName: "z-[120]",
		connectLabel: "Add Linear access"
	})] });
}
var CAPABILITIES = [
	"Start a workspace from any GitHub issue or pull request, prefilled with its title and context",
	"Browse GitHub issues and pull requests in the Tasks view without leaving Orca",
	"See issue state, review status, and CI checks on every worktree",
	"Read, comment on, and merge pull requests without leaving Orca"
];
function IntegrationsStep() {
	const refreshPreflightStatus = useAppStore((s) => s.refreshPreflightStatus);
	(0, import_react.useEffect)(() => {
		refreshPreflightStatus();
	}, [refreshPreflightStatus]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-6",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("ul", {
			className: "-mt-6 space-y-1.5 text-[14px] leading-relaxed text-muted-foreground",
			children: CAPABILITIES.map((line) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("li", {
				className: "flex gap-2.5",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "mt-2 size-1 shrink-0 rounded-full bg-muted-foreground",
					"aria-hidden": true
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: line })]
			}, line))
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "space-y-3",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(GitHubRow, {}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "mt-4 rounded-xl border border-border bg-muted/10 px-5 py-4",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex flex-col gap-1 sm:flex-row sm:items-center sm:justify-between",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "text-[14px] font-medium text-foreground/70",
						children: translate("auto.components.onboarding.IntegrationsStep.3a3e360289", "More task sources")
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "text-[13px] leading-relaxed text-muted-foreground",
						children: translate("auto.components.onboarding.IntegrationsStep.277f30eb34", "Linear, GitLab, Bitbucket, Azure DevOps, Gitea, and Jira live in Settings > Integrations.")
					})]
				})
			})]
		})]
	});
}
export { IntegrationsStep as n, LinearRow as r, GitHubRow as t };
