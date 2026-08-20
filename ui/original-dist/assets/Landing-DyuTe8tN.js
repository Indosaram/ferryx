import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn } from "./button-DszXJEV6.js";
import { t as ExternalLink } from "./external-link-BrcDtGAn.js";
import { t as FolderPlus } from "./folder-plus-09lX5Kg7.js";
import { t as GitBranchPlus } from "./git-branch-plus-DZCVwQyy.js";
import { t as Star } from "./star-qKN37Pn0.js";
import { Cm as projectHostSetupProjectionFromRepos, qg as isGitRepoKind, t as useAppStore, xm as isGitHubBackedRepo, ym as getProjectIdentityKey } from "./store-CgXrfmaH.js";
import { t as TriangleAlert } from "./triangle-alert-HrLt1y9s.js";
import { t as X } from "./x-BrGKE4uz.js";
import { t as logo_default } from "./logo-BwOQUDc0.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import { t as useMountedRef } from "./useMountedRef-1omUd-IV.js";
import { o as useShortcutKeyDetails } from "./useShortcutLabel-C-KRYtlB.js";
import { t as ShortcutKeyCombo } from "./ShortcutKeyCombo-Ch456Md0.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var STORAGE_PREFIX = "orca.preflightBanner.dismissed.";
function storageKey(issueId) {
	return `${STORAGE_PREFIX}${issueId}`;
}
function githubProjectKeys(repos) {
	const keys = repos.filter((repo) => isGitHubBackedRepo(repo)).map((repo) => getProjectIdentityKey(repo));
	return [...new Set(keys)].sort();
}
function readRecord(issueId) {
	try {
		const raw = localStorage.getItem(storageKey(issueId));
		if (!raw) return null;
		const parsed = JSON.parse(raw);
		return Array.isArray(parsed?.githubKeys) ? parsed : null;
	} catch {
		return null;
	}
}
function isPreflightIssueDismissed(issueId, repos) {
	const record = readRecord(issueId);
	if (!record) return false;
	const snapshot = new Set(record.githubKeys);
	return !githubProjectKeys(repos).some((key) => !snapshot.has(key));
}
function dismissPreflightIssue(issueId, repos) {
	try {
		const record = { githubKeys: githubProjectKeys(repos) };
		localStorage.setItem(storageKey(issueId), JSON.stringify(record));
	} catch {}
}
function hasGitHubBackedProject(repos) {
	return projectHostSetupProjectionFromRepos(repos).projects.some((project) => project.providerIdentity?.provider === "github");
}
function getLandingPreflightIssues(status, options) {
	const issues = [];
	if (!status.git.installed) issues.push({
		id: "git",
		title: translate("auto.components.Landing.e5b7296d9d", "Git is not installed"),
		description: translate("auto.components.Landing.b673e7cf1b", "Git is required for Git projects, source control, and workspace management."),
		fixLabel: "Install Git",
		fixUrl: "https://git-scm.com/downloads"
	});
	if (!options.hasGitHubBackedProject) return issues;
	if (!status.gh.installed) issues.push({
		id: "gh",
		title: translate("auto.components.Landing.5beaef5f9e", "GitHub CLI is not installed"),
		description: translate("auto.components.Landing.73e1ad4282", "Orca uses the GitHub CLI (gh) to show pull requests, issues, and checks."),
		fixLabel: "Install GitHub CLI",
		fixUrl: "https://cli.github.com",
		dismissible: true
	});
	else if (!status.gh.authenticated) issues.push({
		id: "gh-auth",
		title: translate("auto.components.Landing.9f96d018b7", "GitHub CLI is not authenticated"),
		description: translate("auto.components.Landing.00cee697c1", "Run \"gh auth login\" in a terminal to connect your GitHub account."),
		fixLabel: "Learn more",
		fixUrl: "https://cli.github.com/manual/gh_auth_login",
		dismissible: true
	});
	return issues;
}
function useLandingPreflightRuntime() {
	const repos = useAppStore((s) => s.repos);
	const preflightStatus = useAppStore((s) => s.preflightStatus);
	const refreshPreflightStatus = useAppStore((s) => s.refreshPreflightStatus);
	const invalidatePreflightStatus = useAppStore((s) => s.invalidatePreflightStatus);
	const activeRuntimeState = useAppStore((s) => {
		const environmentId = s.settings?.activeRuntimeEnvironmentId?.trim();
		if (!environmentId) return "local";
		const runtimeStatus = s.runtimeStatusByEnvironmentId.get(environmentId);
		const reachability = runtimeStatus ? runtimeStatus.status === null ? "unreachable" : "reachable" : "unknown";
		return `${environmentId}:${runtimeStatus?.connectionGeneration ?? 0}:${reachability}`;
	});
	const hasGitHubProject = (0, import_react.useMemo)(() => hasGitHubBackedProject(repos), [repos]);
	const preflightIssues = (0, import_react.useMemo)(() => preflightStatus ? getLandingPreflightIssues(preflightStatus, { hasGitHubBackedProject: hasGitHubProject }) : [], [preflightStatus, hasGitHubProject]);
	(0, import_react.useEffect)(() => {
		if (activeRuntimeState !== "local" && !activeRuntimeState.endsWith(":reachable")) {
			invalidatePreflightStatus();
			return;
		}
		refreshPreflightStatus();
		const handleWindowActive = () => {
			if (document.visibilityState === "visible") refreshPreflightStatus({ force: true });
		};
		document.addEventListener("visibilitychange", handleWindowActive);
		window.addEventListener("focus", handleWindowActive);
		return () => {
			document.removeEventListener("visibilitychange", handleWindowActive);
			window.removeEventListener("focus", handleWindowActive);
		};
	}, [
		activeRuntimeState,
		invalidatePreflightStatus,
		refreshPreflightStatus
	]);
	(0, import_react.useEffect)(() => {
		if (preflightIssues.length === 0) return;
		const intervalId = window.setInterval(() => {
			refreshPreflightStatus({ force: true });
		}, 3e4);
		return () => window.clearInterval(intervalId);
	}, [preflightIssues.length, refreshPreflightStatus]);
	return { preflightIssues };
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var ORCA_GITHUB_URL = "https://github.com/stablyai/orca";
function GitHubStarButton({ hasRepos }) {
	const [state, setState] = (0, import_react.useState)("loading");
	const [menuOpen, setMenuOpen] = (0, import_react.useState)(false);
	const wrapperRef = (0, import_react.useRef)(null);
	const mountedRef = useMountedRef();
	(0, import_react.useEffect)(() => {
		let cancelled = false;
		window.api.gh.checkOrcaStarred().then((result) => {
			if (cancelled) return;
			if (result === null) setState("web-fallback");
			else setState(result ? "starred" : "not-starred");
		});
		return () => {
			cancelled = true;
		};
	}, []);
	(0, import_react.useEffect)(() => {
		if (!menuOpen) return;
		const onDocClick = (e) => {
			if (!wrapperRef.current?.contains(e.target)) setMenuOpen(false);
		};
		document.addEventListener("mousedown", onDocClick);
		return () => document.removeEventListener("mousedown", onDocClick);
	}, [menuOpen]);
	const handleClick = async () => {
		if (state === "starred") {
			setMenuOpen((v) => !v);
			return;
		}
		if (state === "web-fallback") {
			await window.api.shell.openUrl(ORCA_GITHUB_URL);
			return;
		}
		if (state !== "not-starred") return;
		setState("starred");
		if (!await window.api.gh.starOrca("landing")) {
			if (mountedRef.current) setState("web-fallback");
			return;
		}
		await window.api.starNag.complete();
	};
	if (state === "hidden" || state === "starred" && hasRepos) return null;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		ref: wrapperRef,
		className: "relative inline-block",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
			className: cn("inline-flex items-center gap-2 rounded-full border px-4 py-1.5 text-[13px] font-medium transition-all duration-300", state === "loading" && "pointer-events-none opacity-0", state !== "starred" && "cursor-pointer border-amber-500/60 text-amber-700 hover:border-amber-500/80 hover:bg-amber-400/10 dark:border-amber-400/30 dark:text-amber-300/90 dark:hover:border-amber-400/50 dark:hover:bg-amber-400/[0.08]", state === "starred" && "cursor-pointer border-amber-500/50 bg-amber-400/10 text-amber-700 dark:border-amber-400/25 dark:bg-amber-400/[0.06] dark:text-amber-400/60"),
			onClick: handleClick,
			disabled: state === "loading",
			children: [state === "web-fallback" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExternalLink, { className: "size-3.5 text-amber-600 transition-all duration-300 dark:text-amber-400/80" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Star, { className: cn("size-3.5 transition-all duration-300", state === "starred" ? "fill-amber-500/70 text-amber-500/70 dark:fill-amber-400/60 dark:text-amber-400/60" : "text-amber-600 dark:text-amber-400/80") }), state === "starred" ? translate("auto.components.Landing.ec43b38ba7", "Starred on GitHub") : state === "web-fallback" ? translate("auto.components.Landing.157bb5ecbb", "Open GitHub") : translate("auto.components.Landing.0d0ace8861", "Star on GitHub")]
		}), state === "starred" && menuOpen && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "absolute right-0 top-[calc(100%+4px)] z-10 min-w-[100px] rounded-md border border-border bg-popover py-1 shadow-md",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				className: "w-full px-3 py-1.5 text-left text-[13px] text-foreground hover:bg-muted",
				onClick: () => {
					setMenuOpen(false);
					setState("hidden");
				},
				children: translate("auto.components.Landing.c1cf168479", "Hide")
			})
		})]
	});
}
function PreflightBanner({ issues, repos }) {
	const githubKey = githubProjectKeys(repos).join("|");
	const [dismissed, setDismissed] = (0, import_react.useState)(() => new Set(issues.filter((issue) => issue.dismissible && isPreflightIssueDismissed(issue.id, repos)).map((issue) => issue.id)));
	(0, import_react.useEffect)(() => {
		setDismissed(new Set(issues.filter((issue) => issue.dismissible && isPreflightIssueDismissed(issue.id, repos)).map((issue) => issue.id)));
	}, [githubKey]);
	const visibleIssues = issues.filter((issue) => !dismissed.has(issue.id));
	if (visibleIssues.length === 0) return null;
	const dismiss = (issue) => {
		dismissPreflightIssue(issue.id, repos);
		setDismissed((prev) => new Set(prev).add(issue.id));
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "w-full max-w-sm space-y-1.5 rounded-lg border border-border bg-muted/40 p-3",
		children: visibleIssues.map((issue) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex items-start gap-3 rounded-md px-1 py-1.5 first:pt-0 last:pb-0",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "mt-0.5 size-4 shrink-0 text-amber-500/70" }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "min-w-0 flex-1 space-y-0.5",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "text-[13px] font-medium leading-snug text-foreground",
							children: issue.title
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "text-xs leading-snug text-muted-foreground",
							children: issue.description
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
							className: "mt-1 inline-flex items-center gap-1 text-xs font-medium text-primary underline-offset-4 hover:underline cursor-pointer",
							onClick: () => window.api.shell.openUrl(issue.fixUrl),
							children: [issue.fixLabel, /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExternalLink, { className: "size-3" })]
						})
					]
				}),
				issue.dismissible && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
					className: "-mr-1 -mt-0.5 shrink-0 rounded p-1 text-muted-foreground/70 transition-colors hover:bg-accent hover:text-foreground cursor-pointer",
					onClick: () => dismiss(issue),
					"aria-label": translate("auto.components.Landing.preflightDismiss", "Dismiss"),
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "size-3.5" })
				})
			]
		}, issue.id))
	});
}
function Landing() {
	const repos = useAppStore((s) => s.repos);
	const openModal = useAppStore((s) => s.openModal);
	const createTargetLabel = repos.length > 0 && repos.every((repo) => isGitRepoKind(repo)) ? "Worktree" : "Workspace";
	const canCreateWorktree = repos.length > 0;
	const hasGitHubProject = (0, import_react.useMemo)(() => hasGitHubBackedProject(repos), [repos]);
	const showGitHubSupportFooter = repos.length === 0 || hasGitHubProject;
	const { preflightIssues } = useLandingPreflightRuntime();
	const createWorktreeShortcut = useShortcutKeyDetails("workspace.create");
	const previousWorktreeShortcut = useShortcutKeyDetails("worktree.navigateUp");
	const nextWorktreeShortcut = useShortcutKeyDetails("worktree.navigateDown");
	const shortcuts = (0, import_react.useMemo)(() => {
		return [
			{
				id: "create",
				shortcut: createWorktreeShortcut,
				action: `Create ${createTargetLabel.toLowerCase()}`
			},
			{
				id: "up",
				shortcut: previousWorktreeShortcut,
				action: "Move up workspace"
			},
			{
				id: "down",
				shortcut: nextWorktreeShortcut,
				action: "Move down workspace"
			}
		];
	}, [
		createTargetLabel,
		createWorktreeShortcut,
		nextWorktreeShortcut,
		previousWorktreeShortcut
	]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "absolute inset-0 flex items-center justify-center bg-background",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "w-full max-w-lg px-6",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex flex-col items-center gap-4 py-8",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "flex items-center justify-center size-20 rounded-2xl border border-border/80 shadow-lg shadow-black/40",
						style: { backgroundColor: "#12181e" },
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("img", {
							src: logo_default,
							alt: translate("auto.components.Landing.520304a067", "Orca logo"),
							className: "size-12"
						})
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h1", {
						className: "text-4xl font-bold text-foreground tracking-tight",
						children: translate("auto.components.Landing.6ca6ff404e", "ORCA")
					}),
					preflightIssues.length > 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PreflightBanner, {
						issues: preflightIssues,
						repos
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "text-sm text-muted-foreground text-center",
						children: canCreateWorktree ? translate("auto.components.Landing.9c00bd4adf", "Select a workspace from the sidebar to begin.") : translate("auto.components.Landing.cd21242762", "Add a project to get started.")
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-center justify-center gap-2.5 flex-wrap",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
							className: "inline-flex items-center gap-1.5 bg-secondary/70 border border-border/80 text-foreground font-medium text-sm px-4 py-2 rounded-md cursor-pointer hover:bg-accent transition-colors",
							onClick: () => openModal("add-repo"),
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(FolderPlus, { className: "size-3.5" }), translate("auto.components.Landing.f9eaa9e12d", "Add Project")]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
							className: "inline-flex items-center gap-1.5 bg-secondary/70 border border-border/80 text-foreground font-medium text-sm px-4 py-2 rounded-md transition-colors disabled:opacity-40 disabled:cursor-not-allowed enabled:cursor-pointer enabled:hover:bg-accent",
							disabled: !canCreateWorktree,
							title: !canCreateWorktree ? translate("auto.components.Landing.f05d237049", "Add a project first") : void 0,
							onClick: () => openModal("new-workspace-composer", { telemetrySource: "unknown" }),
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(GitBranchPlus, { className: "size-3.5" }),
								translate("auto.components.Landing.76a95f7f47", "Create"),
								" ",
								createTargetLabel.toLowerCase()
							]
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "mt-6 w-full max-w-xs space-y-2",
						children: shortcuts.map((shortcut) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "grid grid-cols-[1fr_auto] items-center gap-3",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "text-sm text-muted-foreground",
								children: shortcut.action
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ShortcutKeyCombo, {
								keys: shortcut.shortcut.keys,
								doubleTap: shortcut.shortcut.doubleTap,
								separatorClassName: "mx-0.5 text-[10px] text-muted-foreground"
							})]
						}, shortcut.id))
					})
				]
			})
		}), showGitHubSupportFooter && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "absolute bottom-6 left-0 right-0 flex justify-center",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(GitHubStarButton, { hasRepos: repos.length > 0 })
		})]
	});
}
export { Landing as default };
