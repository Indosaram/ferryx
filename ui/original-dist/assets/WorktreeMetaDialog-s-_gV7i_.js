import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn, t as Button } from "./button-DszXJEV6.js";
import { t as ChevronDown } from "./chevron-down-BRkP96Md.js";
import { t as ExternalLink } from "./external-link-BrcDtGAn.js";
import { t as Github } from "./github-Dx7d1WbZ.js";
import { t as LoaderCircle } from "./loader-circle-CRZpWdsi.js";
import { Xn as issueCacheKey, ed as findIndexedWorktreeOwner, t as useAppStore, vd as parseWorkspaceKey, xc as folderWorkspaceToWorktree } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import { c as DropdownMenuRadioItem, m as DropdownMenuTrigger, r as DropdownMenuContent, s as DropdownMenuRadioGroup, t as DropdownMenu } from "./dropdown-menu-Dth6LPK-.js";
import { t as Label } from "./label-D-n9s_wS.js";
import "./popover-CgR1mzy7.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
import { t as useMountedRef } from "./useMountedRef-1omUd-IV.js";
import { a as parseGitHubIssueOrPRNumber, c as buildLinearIssueUrl, h as parseLinearIssueInput, i as parseGitHubIssueOrPRLink, n as isWorkItemLinkQueryTooLarge, o as LINEAR_ISSUE_LINK_CLEARED, s as buildLinearIssueLinkUpdates } from "./github-links-C1M8w9wX.js";
import "./command-D8Tw17HJ.js";
import { t as LinearIcon } from "./LinearIcon-CTELA_97.js";
import { a as DialogFooter, i as DialogDescription, o as DialogHeader, r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
import { t as Input } from "./input-DV5rpysh.js";
import "./ime-composition-keyboard-event-HdRxQ6x2.js";
import { o as WorkspaceEmojiSuggestionPopover, t as useWorkspaceEmojiShortcodeInput } from "./useWorkspaceEmojiShortcodeInput-DLLHCF-J.js";
import { n as getScreenSubmitShortcutLabel, r as isScreenSubmitShortcut } from "./screen-submit-shortcut-BNcVTUFc.js";
import "./github-links-YjCshxad.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
const ISSUE_LINK_PROVIDERS = ["github", "linear"];
function isIssueLinkProvider(value) {
	return ISSUE_LINK_PROVIDERS.includes(value);
}
function getIssueLinkProviderFromUrl(input) {
	const trimmed = input.trim();
	if (!/^https?:\/\//i.test(trimmed)) return null;
	if (parseGitHubIssueOrPRLink(trimmed)?.type === "issue") return "github";
	if (parseLinearIssueInput(trimmed)) return "linear";
	return null;
}
function parseIssueLinkInput(input, provider) {
	const trimmed = input.trim();
	if (!trimmed) return null;
	if (provider === "linear") {
		const parsed$1 = parseLinearIssueInput(trimmed);
		return parsed$1 ? {
			provider: "linear",
			...parsed$1
		} : null;
	}
	const link = parseGitHubIssueOrPRLink(trimmed);
	if (link) return link.type === "issue" ? {
		provider: "github",
		number: link.number
	} : null;
	const numeric = trimmed.startsWith("#") ? trimmed.slice(1) : trimmed;
	if (!/^\d+$/.test(numeric)) return null;
	const parsed = Number.parseInt(numeric, 10);
	return Number.isSafeInteger(parsed) && parsed > 0 ? {
		provider: "github",
		number: parsed
	} : null;
}
function parseExplicitGitHubIssueUrl(input) {
	const trimmed = input.trim();
	const link = parseGitHubIssueOrPRLink(trimmed);
	if (!link || link.type !== "issue") return null;
	return trimmed;
}
function parseGitHubWorkItemNumberForMetaField(input, expectedType) {
	const link = parseGitHubIssueOrPRLink(input);
	if (link) return link.type === expectedType ? link.number : null;
	return parseGitHubIssueOrPRNumber(input);
}
function buildDisplayNameUpdate(draft, current) {
	const trimmed = draft.displayNameInput.trim();
	return trimmed === current.displayName ? {} : { displayName: trimmed };
}
function buildCommentUpdate(draft, current) {
	const trimmed = draft.commentInput.trim();
	return trimmed === current.comment ? {} : { comment: trimmed };
}
function issueLinkIdentity(input, provider, storedLinearOrganizationUrlKey) {
	const trimmed = input.trim();
	if (trimmed === "") return "";
	const parsed = parseIssueLinkInput(trimmed, provider);
	if (!parsed) return `raw:${provider}:${trimmed}`;
	if (parsed.provider === "github") return `github:${parsed.number}`;
	const organizationUrlKey = parsed.organizationUrlKey ?? storedLinearOrganizationUrlKey ?? "";
	return `linear:${parsed.identifier}:${organizationUrlKey.trim().toLowerCase()}`;
}
function isIssueFieldDirty(draft, current) {
	const storedOrganizationUrlKey = current.linkedLinearIssueOrganizationUrlKey ?? null;
	return issueLinkIdentity(draft.issueInput, draft.issueProvider, storedOrganizationUrlKey) !== issueLinkIdentity(current.issueInput, current.issueProvider, storedOrganizationUrlKey);
}
function keepsLinkedWorkItem(input, provider, live) {
	const parsed = parseIssueLinkInput(input.trim(), provider);
	if (!parsed || live.linkedWorkItemType !== "issue") return false;
	if (parsed.provider === "github") return live.linkedWorkItemProvider === "github" && parsed.number === live.linkedIssue;
	if (live.linkedWorkItemProvider !== "linear" || parsed.identifier.toUpperCase() !== live.linkedLinearIssue?.trim().toUpperCase()) return false;
	const storedOrganizationUrlKey = live.linkedLinearIssueOrganizationUrlKey?.trim();
	const nextOrganizationUrlKey = parsed.organizationUrlKey?.trim();
	return !storedOrganizationUrlKey || !nextOrganizationUrlKey || storedOrganizationUrlKey.toLowerCase() === nextOrganizationUrlKey.toLowerCase();
}
function buildIssueLinkUpdates(draft, current, live) {
	if (!isIssueFieldDirty(draft, current)) return {};
	const trimmed = draft.issueInput.trim();
	const displacedWorkItem = !keepsLinkedWorkItem(trimmed, draft.issueProvider, live) && (live.linkedWorkItemProvider === "github" || live.linkedWorkItemProvider === "linear") && live.linkedWorkItemType === "issue" ? {
		linkedWorkItem: null,
		linkedTaskSourceContext: null
	} : {};
	const displacedLinear = live.linkedLinearIssue ? LINEAR_ISSUE_LINK_CLEARED : {};
	if (trimmed === "") return {
		linkedIssue: null,
		...displacedLinear,
		...displacedWorkItem
	};
	const parsed = parseIssueLinkInput(trimmed, draft.issueProvider);
	if (!parsed) return {};
	if (parsed.provider === "github") return {
		linkedIssue: parsed.number,
		...displacedLinear,
		...displacedWorkItem
	};
	const linearUpdates = buildLinearIssueLinkUpdates(trimmed);
	return linearUpdates ? {
		linkedIssue: null,
		...linearUpdates,
		...displacedWorkItem
	} : {};
}
function buildPrLinkUpdate(draft) {
	const trimmed = draft.prInput.trim();
	if (trimmed === "") return { linkedPR: null };
	const number = parseGitHubWorkItemNumberForMetaField(trimmed, "pr");
	return number === null ? {} : { linkedPR: number };
}
function buildWorktreeMetaUpdates(draft, current, live) {
	return {
		...buildCommentUpdate(draft, current),
		...buildDisplayNameUpdate(draft, current),
		...buildIssueLinkUpdates(draft, current, live),
		...buildPrLinkUpdate(draft)
	};
}
function formatLinkLabel(provider, value) {
	return provider === "linear" ? translate("auto.components.sidebar.worktreeIssueDisplacement.3f61c0a8d2", "Linear {{value}}", { value }) : translate("auto.components.sidebar.worktreeIssueDisplacement.9c4b7e1f60", "GitHub #{{value}}", { value });
}
function getDisplacedLinkLabels(args) {
	const { draft, snapshot, isFolderWorkspace, linkedIssue, linkedLinearIssue } = args;
	if (isFolderWorkspace || !isIssueFieldDirty(draft, snapshot)) return null;
	const keeping = draft.issueInput.trim() === "" ? null : draft.issueProvider;
	const displaced = [];
	if (keeping !== "linear" && linkedLinearIssue) displaced.push(formatLinkLabel("linear", linkedLinearIssue));
	if (keeping !== "github" && typeof linkedIssue === "number") displaced.push(formatLinkLabel("github", String(linkedIssue)));
	return displaced.length > 0 ? displaced : null;
}
var OPEN_ISSUE_TIMEOUT_MS = 35e3;
async function resolveIssueUrlWithinTimeout(lookup) {
	let timer;
	try {
		return (await Promise.race([lookup, new Promise((resolve) => {
			timer = setTimeout(() => resolve(null), OPEN_ISSUE_TIMEOUT_MS);
		})]))?.url ?? null;
	} catch {
		return null;
	} finally {
		clearTimeout(timer);
	}
}
function useWorktreeIssueLink(args) {
	const { worktreeId, ownerRepoId, issueInput, issueProvider, linearOrganizationUrlKey, linkedLinearIssue, linearSourceContext } = args;
	const isLinear = issueProvider === "linear";
	const fetchIssue = useAppStore((s) => s.fetchIssue);
	const fetchLinearIssue = useAppStore((s) => s.fetchLinearIssue);
	const [openingIssue, setOpeningIssue] = (0, import_react.useState)(false);
	const [failedIssueInput, setFailedIssueInput] = (0, import_react.useState)(null);
	const mountedRef = useMountedRef();
	const openRequestRef = (0, import_react.useRef)(0);
	const latestRequestKeyRef = (0, import_react.useRef)("");
	latestRequestKeyRef.current = `${issueProvider}\u0000${issueInput}`;
	const boundedInput = (0, import_react.useMemo)(() => isWorkItemLinkQueryTooLarge(issueInput) ? "" : issueInput, [issueInput]);
	const issueNumber = (0, import_react.useMemo)(() => isLinear ? null : parseGitHubIssueOrPRNumber(boundedInput), [isLinear, boundedInput]);
	const issueUrlFromInput = (0, import_react.useMemo)(() => isLinear ? null : parseExplicitGitHubIssueUrl(boundedInput), [isLinear, boundedInput]);
	const issueInputLooksLikeUrl = (0, import_react.useMemo)(() => /^https?:\/\//i.test(boundedInput.trim()), [boundedInput]);
	const parsedLinearIssue = (0, import_react.useMemo)(() => isLinear ? parseLinearIssueInput(boundedInput) : null, [isLinear, boundedInput]);
	const linearIssueUrl = (0, import_react.useMemo)(() => {
		if (!parsedLinearIssue) return null;
		const storedKeyApplies = typeof linkedLinearIssue === "string" && linkedLinearIssue.toUpperCase() === parsedLinearIssue.identifier.toUpperCase();
		return buildLinearIssueUrl({
			identifier: parsedLinearIssue.identifier,
			organizationUrlKey: parsedLinearIssue.organizationUrlKey ?? (storedKeyApplies ? linearOrganizationUrlKey : null)
		});
	}, [
		parsedLinearIssue,
		linkedLinearIssue,
		linearOrganizationUrlKey
	]);
	const issueRepo = useAppStore((s) => {
		const repoId = ownerRepoId ?? findIndexedWorktreeOwner(s.worktreesByRepo, worktreeId)?.repoId;
		return repoId ? s.repos.find((repo) => repo.id === repoId) : void 0;
	});
	const cachedIssueUrl = useAppStore((s) => {
		if (!issueRepo || issueNumber === null) return null;
		return s.issueCache[issueCacheKey(issueRepo.path, issueRepo.id, issueNumber, s.settings, issueRepo.connectionId, issueRepo.executionHostId, true)]?.data?.url ?? null;
	});
	const canOpenIssue = isLinear ? Boolean(parsedLinearIssue) : issueInputLooksLikeUrl ? Boolean(issueUrlFromInput) : Boolean(cachedIssueUrl || issueRepo && issueNumber);
	const handleOpenIssue = (0, import_react.useCallback)(async () => {
		if (openingIssue) return;
		setFailedIssueInput(null);
		const generation = ++openRequestRef.current;
		const requestKey = latestRequestKeyRef.current;
		const isCurrentRequest = () => mountedRef.current && openRequestRef.current === generation && latestRequestKeyRef.current === requestKey;
		if (isLinear) {
			if (!parsedLinearIssue) return;
			if (linearIssueUrl) {
				window.api.shell.openUrl(linearIssueUrl);
				return;
			}
			setOpeningIssue(true);
			try {
				const url = await resolveIssueUrlWithinTimeout(fetchLinearIssue(parsedLinearIssue.identifier, "all", { sourceContext: linearSourceContext ?? null }));
				if (!isCurrentRequest()) return;
				if (url) window.api.shell.openUrl(url);
				else setFailedIssueInput(issueInput);
			} finally {
				if (mountedRef.current) setOpeningIssue(false);
			}
			return;
		}
		if (issueUrlFromInput) {
			window.api.shell.openUrl(issueUrlFromInput);
			return;
		}
		if (issueInputLooksLikeUrl) return;
		if (cachedIssueUrl) {
			window.api.shell.openUrl(cachedIssueUrl);
			return;
		}
		if (!issueRepo || issueNumber === null) return;
		setOpeningIssue(true);
		try {
			const url = await resolveIssueUrlWithinTimeout(fetchIssue(issueRepo.path, issueNumber, { repoId: issueRepo.id }));
			if (!isCurrentRequest()) return;
			if (url) window.api.shell.openUrl(url);
			else setFailedIssueInput(issueInput);
		} finally {
			if (mountedRef.current) setOpeningIssue(false);
		}
	}, [
		cachedIssueUrl,
		fetchIssue,
		fetchLinearIssue,
		isLinear,
		issueInput,
		issueInputLooksLikeUrl,
		issueNumber,
		issueRepo,
		issueUrlFromInput,
		linearIssueUrl,
		linearSourceContext,
		mountedRef,
		openingIssue,
		parsedLinearIssue
	]);
	const resetOpeningIssue = (0, import_react.useCallback)(() => {
		openRequestRef.current += 1;
		setOpeningIssue(false);
		setFailedIssueInput(null);
	}, []);
	return {
		canOpenIssue,
		openingIssue,
		openIssueFailed: failedIssueInput !== null && failedIssueInput === issueInput,
		handleOpenIssue,
		resetOpeningIssue
	};
}
function useWorktreeMetaWorkspace(args) {
	const { worktreeId, ownerRepoId } = args;
	const workspaceScope = (0, import_react.useMemo)(() => parseWorkspaceKey(worktreeId), [worktreeId]);
	const indexedWorktree = useAppStore((s) => {
		const scoped = ownerRepoId ? s.worktreesByRepo[ownerRepoId]?.find((item) => item.id === worktreeId) : void 0;
		if (scoped) return scoped;
		const owner = findIndexedWorktreeOwner(s.worktreesByRepo, worktreeId);
		return owner ? s.worktreesByRepo[owner.repoId]?.find((item) => item.id === worktreeId) : void 0;
	});
	const folderWorkspace = useAppStore((s) => workspaceScope?.type === "folder" ? s.folderWorkspaces.find((item) => item.id === workspaceScope.folderWorkspaceId) ?? null : null);
	const worktree = (0, import_react.useMemo)(() => folderWorkspace ? folderWorkspaceToWorktree(folderWorkspace) : indexedWorktree, [folderWorkspace, indexedWorktree]);
	const linkedIssue = worktree?.linkedIssue ?? null;
	const linkedLinearIssue = worktree?.linkedLinearIssue ?? null;
	const currentProvider = typeof linkedIssue === "number" ? "github" : linkedLinearIssue ? "linear" : "github";
	const currentIssue = currentProvider === "linear" ? linkedLinearIssue ?? "" : typeof linkedIssue === "number" ? String(linkedIssue) : "";
	const liveLinks = (0, import_react.useMemo)(() => ({
		linkedIssue,
		linkedLinearIssue,
		linkedLinearIssueOrganizationUrlKey: worktree?.linkedLinearIssueOrganizationUrlKey ?? null,
		linkedWorkItemProvider: worktree?.linkedWorkItem?.provider ?? null,
		linkedWorkItemType: worktree?.linkedWorkItem?.type ?? null
	}), [
		linkedIssue,
		linkedLinearIssue,
		worktree?.linkedLinearIssueOrganizationUrlKey,
		worktree?.linkedWorkItem
	]);
	return {
		worktree,
		linkedIssue,
		linkedLinearIssue,
		currentIssue,
		currentProvider,
		isFolderWorkspace: workspaceScope?.type === "folder",
		liveLinks
	};
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var ADORNMENT_RESERVE_PX = 70;
var MAX_RESERVE = "55%";
function issueAdornmentReserve(providerLabel$1) {
	return `min(calc(${ADORNMENT_RESERVE_PX}px + ${providerLabel$1.length}ch), ${MAX_RESERVE})`;
}
function providerLabel(provider) {
	return provider === "linear" ? translate("auto.components.sidebar.WorktreeIssueLinkField.25852bfc59", "Linear") : translate("auto.components.sidebar.WorktreeIssueLinkField.5b440069e6", "GitHub");
}
function ProviderIcon({ provider, className }) {
	return provider === "linear" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LinearIcon, { className }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Github, { className });
}
function WorktreeIssueLinkField(props) {
	const { inputRef, value, provider, isInvalid, displacedLinkLabels, isReadOnly, canOpenIssue, openingIssue, openIssueFailed, onValueChange, onProviderChange, onOpenIssue, onKeyDown } = props;
	const helperId = (0, import_react.useId)();
	const inputId = (0, import_react.useId)();
	const label = providerLabel(provider);
	const openIssueLabel = translate("auto.components.sidebar.WorktreeIssueLinkField.161b2d053a", "Open linked issue");
	const handleProviderChange = (0, import_react.useCallback)((next) => {
		if (isIssueLinkProvider(next)) onProviderChange(next);
	}, [onProviderChange]);
	const helperText = (0, import_react.useMemo)(() => {
		if (isReadOnly) return translate("auto.components.sidebar.WorktreeIssueLinkField.d4785f9954", "Issue links are set when a folder workspace is created and can't be changed here yet.");
		if (isInvalid) return provider === "linear" ? translate("auto.components.sidebar.WorktreeIssueLinkField.964d9bc00a", "Not a Linear issue key or linear.app issue URL.") : translate("auto.components.sidebar.WorktreeIssueLinkField.0a7a2c6efd", "Not a GitHub issue number or issue URL.");
		if (openIssueFailed) return provider === "linear" ? translate("auto.components.sidebar.WorktreeIssueLinkField.d8c8a30d1f", "Couldn't open that issue. Check the identifier and your Linear connection.") : translate("auto.components.sidebar.WorktreeIssueLinkField.269198eeda", "Couldn't open that issue. Check the number and your GitHub connection.");
		if (displacedLinkLabels && displacedLinkLabels.length > 1) return translate("auto.components.sidebar.WorktreeIssueLinkField.72486800ff", "Saving unlinks {{first}} and {{second}} — a workspace tracks one issue.", {
			first: displacedLinkLabels[0],
			second: displacedLinkLabels[1]
		});
		if (displacedLinkLabels?.length) return translate("auto.components.sidebar.WorktreeIssueLinkField.2c245ac134", "Saving unlinks {{link}} — a workspace tracks one issue.", { link: displacedLinkLabels[0] });
		return translate("auto.components.sidebar.WorktreeIssueLinkField.f047887705", "Paste a GitHub or Linear URL, or enter a number. Leave blank to remove the link.");
	}, [
		displacedLinkLabels,
		isInvalid,
		isReadOnly,
		openIssueFailed,
		provider
	]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-1",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("label", {
				htmlFor: inputId,
				className: "text-[11px] font-medium text-muted-foreground",
				children: translate("auto.components.sidebar.WorktreeIssueLinkField.ad78f9bee2", "Issue")
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "relative",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
					ref: inputRef,
					id: inputId,
					"aria-describedby": helperId,
					value,
					onChange: (e) => onValueChange(e.target.value),
					onKeyDown,
					disabled: isReadOnly,
					"aria-invalid": isInvalid || void 0,
					placeholder: translate("auto.components.sidebar.WorktreeIssueLinkField.662ae142f8", "Issue #, or a GitHub or Linear URL"),
					className: "h-8 text-xs",
					style: { paddingRight: issueAdornmentReserve(label) }
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "absolute right-1 top-1 flex items-center gap-0.5",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenu, {
						modal: false,
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuTrigger, {
							asChild: true,
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
								type: "button",
								variant: "ghost",
								size: "xs",
								disabled: isReadOnly,
								"aria-label": translate("auto.components.sidebar.WorktreeIssueLinkField.929c98d05a", "Issue provider"),
								className: "h-6 px-1 text-[10px] font-medium text-muted-foreground",
								children: [
									/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ProviderIcon, {
										provider,
										className: "size-3"
									}),
									label,
									/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronDown, { className: "size-2.5 opacity-60" })
								]
							})
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuContent, {
							align: "end",
							className: "min-w-32",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuRadioGroup, {
								value: provider,
								onValueChange: handleProviderChange,
								children: ISSUE_LINK_PROVIDERS.map((item) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuRadioItem, {
									value: item,
									className: "text-xs",
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ProviderIcon, {
										provider: item,
										className: "size-3"
									}), providerLabel(item)]
								}, item))
							})
						})]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
						asChild: true,
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							type: "button",
							variant: "ghost",
							size: "icon-xs",
							"aria-label": openIssueLabel,
							disabled: !canOpenIssue || openingIssue,
							onClick: onOpenIssue,
							className: "text-muted-foreground",
							children: openingIssue ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3 animate-spin" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExternalLink, { className: "size-3" })
						})
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
						side: "top",
						sideOffset: 4,
						children: openIssueLabel
					})] })]
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				id: helperId,
				role: "status",
				"aria-live": "polite",
				className: cn("min-h-[28px] text-[10px] leading-[14px]", (openIssueFailed || displacedLinkLabels?.length) && !isInvalid && !isReadOnly ? "text-amber-600 dark:text-amber-400" : "text-muted-foreground"),
				children: helperText
			})
		]
	});
}
function WorktreeDisplayNameField({ disabled, inputRef, onEnter, onValueChange, portalContainer, value }) {
	const inputId = (0, import_react.useId)();
	const emojiInput = useWorkspaceEmojiShortcodeInput({
		disabled,
		inputRef,
		onValueChange,
		value
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-1",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
				htmlFor: inputId,
				className: "text-[11px] font-medium text-muted-foreground",
				children: translate("auto.components.sidebar.WorktreeMetaDialog.ad5e4e514f", "Display Name")
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
				id: inputId,
				ref: inputRef,
				value,
				onChange: (event) => emojiInput.handleValueChange(event.target.value, event.target.selectionStart),
				onSelect: (event) => emojiInput.syncCursor(event.currentTarget),
				onKeyDown: (event) => {
					if (emojiInput.handleKeyDown(event) || event.key !== "Enter") return;
					event.preventDefault();
					onEnter();
				},
				placeholder: translate("auto.components.sidebar.WorktreeMetaDialog.7f21e0464f", "Custom display name..."),
				className: "h-8 text-xs"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorkspaceEmojiSuggestionPopover, {
				anchorRef: inputRef,
				open: emojiInput.open,
				commandValue: emojiInput.commandValue,
				heading: translate("auto.components.new.workspace.SmartWorkspaceNameField.emoji", "Emoji"),
				suggestions: emojiInput.suggestions,
				onCommandValueChange: emojiInput.onCommandValueChange,
				onSelect: emojiInput.selectSuggestion,
				onOpenChange: (open) => !open && emojiInput.close(),
				portalContainer,
				side: "bottom"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				className: "text-[10px] text-muted-foreground",
				children: translate("auto.components.sidebar.WorktreeMetaDialog.459ad7f650", "Only changes the name shown in the sidebar — the folder on disk stays the same. Leave blank to use the branch or folder name.")
			})
		]
	});
}
function resizeCommentTextarea(textarea) {
	textarea.style.height = "auto";
	textarea.style.height = `${textarea.scrollHeight}px`;
}
var EMPTY_SNAPSHOT = {
	displayName: "",
	comment: "",
	issueInput: "",
	issueProvider: "github"
};
var WorktreeMetaDialog_default = import_react.memo(function WorktreeMetaDialog$1() {
	const activeModal = useAppStore((s) => s.activeModal);
	const modalData = useAppStore((s) => s.modalData);
	const closeModal = useAppStore((s) => s.closeModal);
	const updateWorktreeMeta = useAppStore((s) => s.updateWorktreeMeta);
	const submitShortcutLabel = getScreenSubmitShortcutLabel();
	const isEditMeta = activeModal === "edit-meta";
	const isOpen = isEditMeta;
	const worktreeId = typeof modalData.worktreeId === "string" ? modalData.worktreeId : "";
	const currentDisplayName = typeof modalData.currentDisplayName === "string" ? modalData.currentDisplayName : "";
	const currentComment = typeof modalData.currentComment === "string" ? modalData.currentComment : "";
	const focusField = typeof modalData.focus === "string" ? modalData.focus : "comment";
	const afterSave = typeof modalData.afterSave === "function" ? modalData.afterSave : null;
	const ownerRepoId = typeof modalData.repoId === "string" ? modalData.repoId : null;
	const { worktree, linkedIssue, linkedLinearIssue, currentIssue, currentProvider, isFolderWorkspace, liveLinks } = useWorktreeMetaWorkspace({
		worktreeId,
		ownerRepoId
	});
	const currentPR = typeof modalData.currentPR === "number" ? String(modalData.currentPR) : worktree?.linkedPR != null ? String(worktree.linkedPR) : "";
	const [displayNameInput, setDisplayNameInput] = (0, import_react.useState)("");
	const [issueInput, setIssueInput] = (0, import_react.useState)("");
	const [issueProvider, setIssueProvider] = (0, import_react.useState)("github");
	const [prInput, setPrInput] = (0, import_react.useState)("");
	const [commentInput, setCommentInput] = (0, import_react.useState)("");
	const [saving, setSaving] = (0, import_react.useState)(false);
	const [saveError, setSaveError] = (0, import_react.useState)(null);
	const [snapshot, setSnapshot] = (0, import_react.useState)(EMPTY_SNAPSHOT);
	const [dialogElement, setDialogElement] = (0, import_react.useState)(null);
	const { canOpenIssue, openingIssue, openIssueFailed, handleOpenIssue, resetOpeningIssue } = useWorktreeIssueLink({
		worktreeId,
		ownerRepoId,
		issueInput,
		issueProvider,
		linearOrganizationUrlKey: worktree?.linkedLinearIssueOrganizationUrlKey ?? null,
		linkedLinearIssue: worktree?.linkedLinearIssue ?? null,
		linearSourceContext: worktree?.linkedTaskSourceContext ?? null
	});
	const issueInputRef = (0, import_react.useRef)(null);
	const prInputRef = (0, import_react.useRef)(null);
	const textareaRef = (0, import_react.useRef)(null);
	const prevIsOpenRef = (0, import_react.useRef)(false);
	const displayNameInputRef = (0, import_react.useRef)(null);
	const mountedRef = useMountedRef();
	if (isOpen && !prevIsOpenRef.current) {
		setDisplayNameInput(currentDisplayName);
		setIssueInput(currentIssue);
		setIssueProvider(currentProvider);
		setPrInput(currentPR);
		setCommentInput(currentComment);
		setSnapshot({
			displayName: currentDisplayName,
			comment: currentComment,
			issueInput: currentIssue,
			issueProvider: currentProvider,
			linkedLinearIssueOrganizationUrlKey: worktree?.linkedLinearIssueOrganizationUrlKey ?? null
		});
		setSaveError(null);
		resetOpeningIssue();
	}
	prevIsOpenRef.current = isOpen;
	const draft = (0, import_react.useMemo)(() => ({
		displayNameInput,
		issueInput,
		issueProvider,
		prInput,
		commentInput
	}), [
		displayNameInput,
		issueInput,
		issueProvider,
		prInput,
		commentInput
	]);
	const handleIssueInputChange = (0, import_react.useCallback)((next) => {
		setIssueInput(next);
		const detected = isWorkItemLinkQueryTooLarge(next) ? null : getIssueLinkProviderFromUrl(next);
		if (detected) setIssueProvider(detected);
	}, []);
	const setCommentTextareaRef = (0, import_react.useCallback)((textarea) => {
		textareaRef.current = textarea;
		if (textarea && isEditMeta) resizeCommentTextarea(textarea);
	}, [isEditMeta]);
	const handleCommentChange = (0, import_react.useCallback)((event) => {
		setCommentInput(event.target.value);
		resizeCommentTextarea(event.currentTarget);
	}, []);
	const issueInvalid = (0, import_react.useMemo)(() => {
		const trimmed = issueInput.trim();
		if (trimmed === "" || isFolderWorkspace) return false;
		return isWorkItemLinkQueryTooLarge(trimmed) || parseIssueLinkInput(trimmed, issueProvider) === null;
	}, [
		isFolderWorkspace,
		issueInput,
		issueProvider
	]);
	const canSave = (0, import_react.useMemo)(() => {
		if (!worktreeId) return false;
		const trimmedPR = prInput.trim();
		const prValid = trimmedPR === "" || !isWorkItemLinkQueryTooLarge(trimmedPR) && parseGitHubWorkItemNumberForMetaField(trimmedPR, "pr") !== null;
		return !issueInvalid && prValid;
	}, [
		worktreeId,
		issueInvalid,
		prInput
	]);
	const displacedLinkLabels = (0, import_react.useMemo)(() => getDisplacedLinkLabels({
		draft,
		snapshot,
		isFolderWorkspace,
		linkedIssue,
		linkedLinearIssue
	}), [
		draft,
		snapshot,
		isFolderWorkspace,
		linkedIssue,
		linkedLinearIssue
	]);
	const handleOpenChange = (0, import_react.useCallback)((open) => {
		if (!open) closeModal();
	}, [closeModal]);
	const handleSave = (0, import_react.useCallback)(async () => {
		if (!canSave) return;
		setSaving(true);
		setSaveError(null);
		try {
			const updates = buildWorktreeMetaUpdates(draft, snapshot, liveLinks);
			const result = await updateWorktreeMeta(worktreeId, updates);
			if (!result.ok) {
				if (mountedRef.current) setSaveError(result.error);
				return;
			}
			closeModal();
			try {
				Promise.resolve(afterSave?.({
					worktreeId,
					updates
				})).catch(console.error);
			} catch (error) {
				console.error(error);
			}
		} finally {
			if (mountedRef.current) setSaving(false);
		}
	}, [
		worktreeId,
		canSave,
		draft,
		snapshot,
		liveLinks,
		updateWorktreeMeta,
		closeModal,
		afterSave,
		mountedRef
	]);
	const handleCommentKeyDown = (0, import_react.useCallback)((e) => {
		if (e.key === "Enter" && !e.shiftKey && !e.altKey && !e.metaKey && !e.ctrlKey || isScreenSubmitShortcut(e)) {
			e.preventDefault();
			e.stopPropagation();
			handleSave();
		}
	}, [handleSave]);
	const handleIssueKeyDown = (0, import_react.useCallback)((e) => {
		if (e.key === "Enter") {
			e.preventDefault();
			handleSave();
		}
	}, [handleSave]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
		open: isOpen,
		onOpenChange: handleOpenChange,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogContent, {
			ref: setDialogElement,
			className: "max-w-md",
			onOpenAutoFocus: (e) => {
				e.preventDefault();
				if (focusField === "displayName") displayNameInputRef.current?.focus();
				else if (focusField === "issue") issueInputRef.current?.focus();
				else if (focusField === "pr") prInputRef.current?.focus();
				else textareaRef.current?.focus();
			},
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, {
					className: "text-sm",
					children: translate("auto.components.sidebar.WorktreeMetaDialog.382fd11a3e", "Edit Worktree Details")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, {
					className: "text-xs",
					children: translate("auto.components.sidebar.WorktreeMetaDialog.a0d191b7a7", "Edit issue links, pull request links, and notes for this workspace.")
				})] }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "space-y-4",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorktreeDisplayNameField, {
							disabled: saving,
							inputRef: displayNameInputRef,
							onEnter: handleSave,
							onValueChange: setDisplayNameInput,
							portalContainer: dialogElement,
							value: displayNameInput
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorktreeIssueLinkField, {
							inputRef: issueInputRef,
							value: issueInput,
							provider: issueProvider,
							isInvalid: issueInvalid,
							displacedLinkLabels,
							isReadOnly: isFolderWorkspace,
							canOpenIssue,
							openingIssue,
							openIssueFailed,
							onValueChange: handleIssueInputChange,
							onProviderChange: setIssueProvider,
							onOpenIssue: handleOpenIssue,
							onKeyDown: handleIssueKeyDown
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "space-y-1",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("label", {
									className: "text-[11px] font-medium text-muted-foreground",
									children: translate("auto.components.sidebar.WorktreeMetaDialog.1b91db7e14", "GH PR")
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
									ref: prInputRef,
									value: prInput,
									onChange: (e) => setPrInput(e.target.value),
									onKeyDown: handleIssueKeyDown,
									placeholder: translate("auto.components.sidebar.WorktreeMetaDialog.077a4f7b5c", "PR # or GitHub URL"),
									className: "h-8 text-xs"
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
									className: "text-[10px] text-muted-foreground",
									children: translate("auto.components.sidebar.WorktreeMetaDialog.5ae06f40fd", "Paste a pull request URL, or enter a number. Leave blank to remove the link.")
								})
							]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "space-y-1",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("label", {
									className: "text-[11px] font-medium text-muted-foreground",
									children: translate("auto.components.sidebar.WorktreeMetaDialog.9c1d1e9b71", "Comment")
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("textarea", {
									ref: setCommentTextareaRef,
									value: commentInput,
									onChange: handleCommentChange,
									onKeyDown: handleCommentKeyDown,
									placeholder: translate("auto.components.sidebar.WorktreeMetaDialog.030d484fc0", "Notes about this worktree..."),
									rows: 3,
									className: "w-full min-w-0 rounded-md border border-input bg-transparent px-3 py-2 text-xs shadow-xs transition-[color,box-shadow] outline-none placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 resize-none max-h-60 overflow-y-auto scrollbar-sleek"
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
									className: "text-[10px] text-muted-foreground",
									children: [
										translate("auto.components.sidebar.WorktreeMetaDialog.7f0be5e9a6", "Supports **markdown** — bold, lists, `code`, links. Press Enter or"),
										" ",
										submitShortcutLabel,
										" ",
										translate("auto.components.sidebar.WorktreeMetaDialog.b48c271d39", "to save, Shift+Enter for a new line.")
									]
								})
							]
						})
					]
				}),
				saveError ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					role: "alert",
					className: "text-[11px] leading-[15px] text-destructive",
					children: saveError
				}) : null,
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogFooter, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					variant: "outline",
					size: "sm",
					onClick: () => handleOpenChange(false),
					className: "text-xs",
					children: translate("auto.components.sidebar.WorktreeMetaDialog.3db0a2a593", "Cancel")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					size: "sm",
					onClick: handleSave,
					disabled: !canSave || saving,
					className: "text-xs",
					children: saving ? translate("auto.components.sidebar.WorktreeMetaDialog.61d6f612cf", "Saving...") : translate("auto.components.sidebar.WorktreeMetaDialog.2174f17011", "Save")
				})] })
			]
		})
	});
});
export { WorktreeMetaDialog_default as default };
