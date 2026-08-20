import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn, t as Button } from "./button-DszXJEV6.js";
import { t as ArrowLeft } from "./arrow-left-BpDalf_n.js";
import { t as ArrowUp } from "./arrow-up-CCUzfqnh.js";
import "./workspace-status-wl52y3xd.js";
import { t as Check } from "./check-Lb2n4tDb.js";
import { t as ChevronDown } from "./chevron-down-BRkP96Md.js";
import { t as ChevronRight } from "./chevron-right-CZtMe6Ev.js";
import { t as ChevronsUpDown } from "./chevrons-up-down-avw2FWhd.js";
import { t as CircleQuestionMark } from "./circle-question-mark-Cytc5uFO.js";
import { t as CircleStop } from "./circle-stop-ByijsK0o.js";
import { t as getFileTypeIcon } from "./file-type-icons-CeipsYgO.js";
import { t as FolderOpen } from "./folder-open-B2ZB-rfY.js";
import { Y as shouldShowHostScopeControls, ct as getRepoDisplayLabelsByPath, q as getSidebarHostHealthLabel, r as activateAndRevealWorktree, st as getRepoDisplayLabelKey } from "./worktree-activation-BDsaiyMf.js";
import { t as Folder } from "./folder-CYUB3i-Q.js";
import { t as GitBranch } from "./git-branch-CnBuDEti.js";
import { t as Globe } from "./globe-c32i33v9.js";
import { t as House } from "./house-BFGM8_Lc.js";
import { n as Lightbulb, t as AddRemoteHostDialog } from "./AddRemoteHostDialog-CQVgufAn.js";
import { t as LoaderCircle } from "./loader-circle-CRZpWdsi.js";
import { t as Monitor } from "./monitor-Dnvi4Ju0.js";
import { t as Pencil } from "./pencil-CLc9a5do.js";
import { t as Plus } from "./plus-Db0kWPVa.js";
import { t as Search } from "./search-DK1nVA6d.js";
import { t as Server } from "./server-DYdwnXME.js";
import { t as Settings } from "./settings-BX3azETW.js";
import { $a as isClipboardTextByteLengthOverLimit, Ea as isWebClientLocation, Ha as extractIpcErrorMessage, Mp as getRuntimePathBasename, Pd as markOnboardingProjectAdded, mp as describeRuntimeCompatBlock, op as getActiveRuntimeTarget, qg as isGitRepoKind, r as isEphemeralVmRuntimeEnvironment, rp as callRuntimeRpc, t as useAppStore } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import { X as LOCAL_EXECUTION_HOST_ID, et as getSettingsFocusedExecutionHostId, st as parseExecutionHostId } from "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import { n as toast } from "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import { a as track } from "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import "./checkbox-PAbetBh2.js";
import { t as Label } from "./label-D-n9s_wS.js";
import { i as PopoverTrigger, r as PopoverContent, t as Popover } from "./popover-CgR1mzy7.js";
import "./scroll-area-DifvZO0h.js";
import "./switch-NhZdOYtg.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
import { t as useMountedRef } from "./useMountedRef-1omUd-IV.js";
import "./selectors-XOBeaOSb.js";
import "./web-runtime-session-CN2syA39.js";
import "./agent-paste-draft-C2PA7vXu.js";
import "./agent-process-recognition-BB0O3DaN.js";
import "./terminal-pty-input-transaction-2UskR-Bm.js";
import "./web-session-tabs-sync-CYKZbAxS.js";
import "./pane-agent-owner-BPfoVAtS.js";
import "./native-chat-session-option-cache-DGE3h47U.js";
import "./github-links-C1M8w9wX.js";
import "./connection-context-BUPsamzR.js";
import "./localized-catalog-DubKHKUR.js";
import { n as isConnectingSshStatus } from "./ssh-connection-recoverability-CNHp0WBp.js";
import "./SettingsFormControls-C0chb_HE.js";
import "./badge-BBptl5GG.js";
import { o as CommandItem, s as CommandList, t as Command } from "./command-D8Tw17HJ.js";
import { d as useSidebarHostScopeOptions } from "./SshHostAdvancedFields-DHVKhP0i.js";
import { t as ShortcutKeyCombo } from "./ShortcutKeyCombo-Ch456Md0.js";
import { i as DialogDescription, o as DialogHeader, r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
import { t as Input } from "./input-DV5rpysh.js";
import { a as useSshConnectInFlight, n as endSshConnect, r as isSshConnectInFlight, t as beginSshConnect } from "./ssh-connect-in-flight-PkUz5iol.js";
import { t as compareWorktreeDisplayName } from "./worktree-display-name-order-AUI-ZkJy.js";
import "./collapsible-raq6sIQA.js";
import { i as shouldHandleTextControlPaste } from "./text-control-paste-PhBVbE2p.js";
import "./paste-payload-metadata-pr3nuODB.js";
import "./ssh-types-Caw2Ltsn.js";
import "./remote-pairing-copy-BzsTvPoC.js";
import { n as getDefaultCreateProjectParent, r as joinCreateProjectPath, t as formatCreateProjectParentSummary } from "./create-project-defaults-CWkW39Bo.js";
import { n as sortDirEntries } from "./file-name-sort-EN_9pleA.js";
import { t as finishProjectAddWithDefaultCheckout } from "./project-added-default-checkout-D1bj4zkk.js";
import { n as upsertAddedRepoWithProjectHostSetup, t as worktreeRefreshOptions } from "./add-repo-runtime-owner-CMgoZO3u.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
function capNestedRepoTelemetryCount(count) {
	if (!Number.isFinite(count)) return 0;
	return Math.max(0, Math.min(500, Math.floor(count)));
}
function normalizeNestedRepoTelemetryCount(count) {
	if (!Number.isFinite(count)) return 0;
	return Math.max(0, Math.floor(count));
}
function bucketNestedRepoTelemetryCount(count) {
	const capped = capNestedRepoTelemetryCount(count);
	if (capped === 0) return "0";
	if (capped === 1) return "1";
	if (capped <= 3) return "2-3";
	if (capped <= 7) return "4-7";
	if (capped <= 15) return "8-15";
	return "16+";
}
function shouldEmitNestedRepoImportSubmitTelemetry(args) {
	return Boolean(args.attemptId && args.selectedCount > 0 && !args.isBusy);
}
function createNestedRepoTelemetryAttemptId() {
	const cryptoApi = globalThis.crypto;
	if (typeof cryptoApi?.randomUUID === "function") return cryptoApi.randomUUID();
	const bytes = new Uint8Array(16);
	if (typeof cryptoApi?.getRandomValues === "function") cryptoApi.getRandomValues(bytes);
	else for (let i = 0; i < bytes.length; i++) bytes[i] = Math.floor(Math.random() * 256);
	bytes[6] = bytes[6] & 15 | 64;
	bytes[8] = bytes[8] & 63 | 128;
	const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0"));
	return `${hex.slice(0, 4).join("")}-${hex.slice(4, 6).join("")}-${hex.slice(6, 8).join("")}-${hex.slice(8, 10).join("")}-${hex.slice(10, 16).join("")}`;
}
function buildNestedRepoScanTelemetry(args) {
	const foundCount = capNestedRepoTelemetryCount(args.scan?.repos.length ?? 0);
	const result = args.scan === null ? "scan_failed" : args.scan.selectedPathKind === "git_repo" ? "git_repo" : foundCount > 0 ? "review_shown" : "no_nested_repos";
	return {
		attempt_id: args.attemptId,
		surface: args.surface,
		runtime_kind: args.runtimeKind,
		result,
		...args.scan ? { selected_path_kind: args.scan.selectedPathKind } : {},
		found_count: foundCount,
		found_count_bucket: bucketNestedRepoTelemetryCount(foundCount),
		truncated: args.scan?.truncated ?? false,
		timed_out: args.scan?.timedOut ?? false
	};
}
function buildNestedRepoImportActionTelemetry(args) {
	const rawFoundCount = normalizeNestedRepoTelemetryCount(args.foundCount);
	const rawSelectedCount = normalizeNestedRepoTelemetryCount(args.selectedCount);
	const foundCount = capNestedRepoTelemetryCount(args.foundCount);
	const selectedCount = capNestedRepoTelemetryCount(args.selectedCount);
	return {
		attempt_id: args.attemptId,
		surface: args.surface,
		runtime_kind: args.runtimeKind,
		action: args.action,
		found_count: foundCount,
		found_count_bucket: bucketNestedRepoTelemetryCount(foundCount),
		selected_count: selectedCount,
		selected_count_bucket: bucketNestedRepoTelemetryCount(selectedCount),
		all_selected: rawFoundCount > 0 && rawSelectedCount === rawFoundCount
	};
}
function buildNestedRepoImportResultTelemetry(args) {
	const rawFoundCount = normalizeNestedRepoTelemetryCount(args.foundCount);
	const rawSelectedCount = normalizeNestedRepoTelemetryCount(args.selectedCount);
	const foundCount = capNestedRepoTelemetryCount(args.foundCount);
	const selectedCount = capNestedRepoTelemetryCount(args.selectedCount);
	const importedCount = capNestedRepoTelemetryCount(args.result?.importedCount ?? 0);
	const alreadyKnownCount = capNestedRepoTelemetryCount(args.result?.alreadyKnownCount ?? 0);
	const failedCount = capNestedRepoTelemetryCount(args.result?.failedCount ?? selectedCount);
	const outcome = importedCount + alreadyKnownCount === 0 ? "failed" : failedCount > 0 ? "partial_failure" : "success";
	return {
		attempt_id: args.attemptId,
		surface: args.surface,
		runtime_kind: args.runtimeKind,
		mode: args.mode,
		outcome,
		found_count: foundCount,
		found_count_bucket: bucketNestedRepoTelemetryCount(foundCount),
		selected_count: selectedCount,
		selected_count_bucket: bucketNestedRepoTelemetryCount(selectedCount),
		imported_count: importedCount,
		imported_count_bucket: bucketNestedRepoTelemetryCount(importedCount),
		already_known_count: alreadyKnownCount,
		already_known_count_bucket: bucketNestedRepoTelemetryCount(alreadyKnownCount),
		failed_count: failedCount,
		failed_count_bucket: bucketNestedRepoTelemetryCount(failedCount),
		all_selected: rawFoundCount > 0 && rawSelectedCount === rawFoundCount
	};
}
function useRemoteRepo(fetchWorktrees, setStep, closeModal, onGitRepoReady, scanNestedRepos, showNestedRepoReview, onNestedScanResult) {
	const [sshTargets, setSshTargets] = (0, import_react.useState)([]);
	const [selectedTargetId, setSelectedTargetId] = (0, import_react.useState)(null);
	const [remotePath, setRemotePath] = (0, import_react.useState)("~/");
	const [remoteError, setRemoteError] = (0, import_react.useState)(null);
	const [isAddingRemote, setIsAddingRemote] = (0, import_react.useState)(false);
	const [remoteNestedScanId, setRemoteNestedScanId] = (0, import_react.useState)(null);
	const remoteGenRef = (0, import_react.useRef)(0);
	const mountedRef = useMountedRef();
	const cancelNestedRepoScan = useAppStore((s) => s.cancelNestedRepoScan);
	const resetRemoteState = (0, import_react.useCallback)(() => {
		remoteGenRef.current++;
		setSshTargets([]);
		setSelectedTargetId(null);
		setRemotePath("~/");
		setRemoteError(null);
		setIsAddingRemote(false);
		if (remoteNestedScanId) cancelNestedRepoScan(remoteNestedScanId, { runtimeEnvironmentId: null });
		setRemoteNestedScanId(null);
	}, [cancelNestedRepoScan, remoteNestedScanId]);
	const stopRemoteNestedScan = (0, import_react.useCallback)(() => {
		if (!remoteNestedScanId) return;
		cancelNestedRepoScan(remoteNestedScanId, { runtimeEnvironmentId: null });
	}, [cancelNestedRepoScan, remoteNestedScanId]);
	const handleOpenRemoteStep = (0, import_react.useCallback)(async (preferredTargetId) => {
		const gen = ++remoteGenRef.current;
		setStep("remote");
		try {
			const targets = await window.api.ssh.listTargets();
			if (gen !== remoteGenRef.current) return;
			const withState = await Promise.all(targets.map(async (t) => {
				const state = await window.api.ssh.getState({ targetId: t.id });
				return {
					...t,
					state: state ?? void 0
				};
			}));
			if (gen !== remoteGenRef.current) return;
			setSshTargets(withState);
			const preferred = preferredTargetId ? withState.find((t) => t.id === preferredTargetId) : void 0;
			const connected = withState.find((t) => t.state?.status === "connected");
			if (preferred) {
				setSelectedTargetId(preferred.id);
				return;
			}
			if (connected) setSelectedTargetId(connected.id);
		} catch {
			if (gen !== remoteGenRef.current) return;
			setSshTargets([]);
		}
	}, [setStep]);
	(0, import_react.useEffect)(() => {
		return window.api.ssh.onStateChanged(({ targetId, state }) => {
			setSshTargets((prev) => prev.map((t) => t.id === targetId ? {
				...t,
				state
			} : t));
			if (state.status === "connected") setSelectedTargetId((curr) => curr ?? targetId);
		});
	}, []);
	const handleConnectTarget = (0, import_react.useCallback)(async (targetId) => {
		try {
			await window.api.ssh.connect({ targetId });
		} catch (err) {
			toast.error(err instanceof Error ? err.message : translate("auto.components.sidebar.AddRepoSteps.3e64e8a70d", "Connection failed"));
		}
	}, []);
	const handleAddRemoteRepo = (0, import_react.useCallback)(async () => {
		if (!selectedTargetId || !remotePath.trim()) return;
		const trimmedRemotePath = remotePath.trim();
		const gen = ++remoteGenRef.current;
		setIsAddingRemote(true);
		setRemoteError(null);
		try {
			const attemptId = createNestedRepoTelemetryAttemptId();
			const scanId = `nested-repo-scan-${Date.now()}-${Math.random().toString(36).slice(2)}`;
			setRemoteNestedScanId(scanId);
			const scan = await scanNestedRepos?.(trimmedRemotePath, selectedTargetId, {
				scanId,
				runtimeEnvironmentId: null,
				onProgress: (progressScan) => {
					if (gen !== remoteGenRef.current || !mountedRef.current || progressScan.selectedPathKind !== "non_git_folder" || progressScan.repos.length === 0) return;
					showNestedRepoReview?.(progressScan, trimmedRemotePath, selectedTargetId, attemptId, true, scanId);
				}
			});
			if (!mountedRef.current || gen !== remoteGenRef.current) return;
			onNestedScanResult?.(scan ?? null, attemptId);
			if (scan?.selectedPathKind === "non_git_folder" && scan.repos.length > 0) {
				showNestedRepoReview?.(scan, trimmedRemotePath, selectedTargetId, attemptId, false, scanId);
				setRemoteNestedScanId(null);
				return;
			}
			setRemoteNestedScanId(null);
			const result = await window.api.repos.addRemote({
				connectionId: selectedTargetId,
				remotePath: trimmedRemotePath
			});
			if ("error" in result) throw new Error(result.error);
			const { alreadyPresent, repo } = upsertAddedRepoWithProjectHostSetup(result.repo, { sshConnectionId: selectedTargetId });
			if (alreadyPresent) useAppStore.getState().clearOrcaHookTrustForRepo(repo.id);
			if (!mountedRef.current || gen !== remoteGenRef.current) return;
			toast.success(translate("auto.components.sidebar.AddRepoSteps.df8b0e6c22", "Project added on SSH host"), { description: repo.displayName });
			const ownerOptions = worktreeRefreshOptions(void 0, selectedTargetId);
			await fetchWorktrees(repo.id, ownerOptions);
			if (!mountedRef.current || gen !== remoteGenRef.current) return;
			await onGitRepoReady?.(repo.id, ownerOptions.executionHostId);
		} catch (err) {
			const message = extractIpcErrorMessage(err, String(err));
			if (message.includes("Not a valid git repository")) {
				closeModal();
				useAppStore.getState().openModal("confirm-non-git-folder", {
					folderPath: trimmedRemotePath,
					connectionId: selectedTargetId
				});
				return;
			}
			if (mountedRef.current && gen === remoteGenRef.current) setRemoteError(message);
		} finally {
			if (mountedRef.current && gen === remoteGenRef.current) {
				setIsAddingRemote(false);
				setRemoteNestedScanId(null);
			}
		}
	}, [
		selectedTargetId,
		remotePath,
		scanNestedRepos,
		showNestedRepoReview,
		onNestedScanResult,
		fetchWorktrees,
		mountedRef,
		closeModal,
		onGitRepoReady
	]);
	return {
		sshTargets,
		selectedTargetId,
		remotePath,
		remoteError,
		isAddingRemote,
		isScanningNested: Boolean(remoteNestedScanId),
		setSelectedTargetId,
		setRemotePath,
		setRemoteError,
		resetRemoteState,
		handleOpenRemoteStep,
		handleAddRemoteRepo,
		handleConnectTarget,
		stopRemoteNestedScan
	};
}
function useCreateRepo(fetchWorktrees, closeModal, onGitRepoReady, options = {}) {
	const [createName, setCreateName] = (0, import_react.useState)("");
	const [createParent, setCreateParent] = (0, import_react.useState)("");
	const [createError, setCreateError] = (0, import_react.useState)(null);
	const [isCreating, setIsCreating] = (0, import_react.useState)(false);
	const mountedRef = useMountedRef();
	const hostToken = options.hostId ?? options.sshTargetId ?? "";
	const hostTokenRef = (0, import_react.useRef)(hostToken);
	hostTokenRef.current = hostToken;
	const createGenRef = (0, import_react.useRef)(0);
	return {
		createName,
		createParent,
		createError,
		isCreating,
		setCreateName,
		setCreateParent,
		setCreateError,
		resetCreateState: (0, import_react.useCallback)(() => {
			createGenRef.current++;
			setCreateName("");
			setCreateParent("");
			setCreateError(null);
			setIsCreating(false);
		}, []),
		handlePickParent: (0, import_react.useCallback)(async () => {
			if (options.sshTargetId) {
				toast.error(translate("auto.components.sidebar.AddRepoCreateStep.ssh_parent_manual", "Enter an SSH parent path."));
				return null;
			}
			if (options.runtimeEnvironmentId?.trim()) {
				toast.error(translate("auto.components.sidebar.AddRepoCreateStep.875dda0995", "Enter a host parent path."));
				return null;
			}
			const gen = createGenRef.current;
			const dir = await window.api.repos.pickDirectory();
			if (dir && gen === createGenRef.current && mountedRef.current) {
				setCreateParent(dir);
				setCreateError(null);
				return dir;
			}
			return null;
		}, [
			mountedRef,
			options.runtimeEnvironmentId,
			options.sshTargetId
		]),
		handleCreate: (0, import_react.useCallback)(async () => {
			const name = createName.trim();
			const parentPath$1 = createParent.trim();
			if (!name || !parentPath$1) return;
			const requestHostToken = hostTokenRef.current;
			const gen = ++createGenRef.current;
			setIsCreating(true);
			setCreateError(null);
			try {
				const target = options.runtimeEnvironmentId?.trim() ? {
					kind: "environment",
					environmentId: options.runtimeEnvironmentId.trim()
				} : getActiveRuntimeTarget({
					...useAppStore.getState().settings,
					activeRuntimeEnvironmentId: null
				});
				const createKind = "git";
				const result = options.sshTargetId ? await window.api.repos.createRemote({
					connectionId: options.sshTargetId,
					parentPath: parentPath$1,
					name,
					kind: createKind
				}) : target.kind === "environment" ? await callRuntimeRpc(target, "repo.create", {
					parentPath: parentPath$1,
					name,
					kind: createKind
				}, { timeoutMs: 6e4 }) : await window.api.repos.create({
					parentPath: parentPath$1,
					name,
					kind: createKind
				});
				if (gen !== createGenRef.current || requestHostToken !== hostTokenRef.current || !mountedRef.current) return;
				if ("error" in result) {
					setCreateError(result.error);
					return;
				}
				const { alreadyPresent: wasDeduped, repo } = upsertAddedRepoWithProjectHostSetup(result.repo, {
					runtimeEnvironmentId: options.runtimeEnvironmentId,
					sshConnectionId: options.sshTargetId
				});
				if (wasDeduped) toast.info(translate("auto.components.sidebar.AddRepoCreateStep.2c12db1511", "Project already added"), { description: repo.displayName });
				else toast.success(translate("auto.components.sidebar.AddRepoCreateStep.5e97f0c4b9", "Project created"), { description: repo.displayName });
				if (isGitRepoKind(repo)) {
					const ownerOptions = worktreeRefreshOptions(options.runtimeEnvironmentId, options.sshTargetId);
					await fetchWorktrees(repo.id, ownerOptions);
					if (gen !== createGenRef.current || requestHostToken !== hostTokenRef.current || !mountedRef.current) return;
					await (ownerOptions.executionHostId ? onGitRepoReady?.(repo.id, ownerOptions.executionHostId) : onGitRepoReady?.(repo.id));
				} else {
					const ownerOptions = worktreeRefreshOptions(options.runtimeEnvironmentId, options.sshTargetId);
					await (ownerOptions.executionHostId ? fetchWorktrees(repo.id, { executionHostId: ownerOptions.executionHostId }) : fetchWorktrees(repo.id));
					if (gen !== createGenRef.current || requestHostToken !== hostTokenRef.current || !mountedRef.current) return;
					const folderWorktree = useAppStore.getState().worktreesByRepo[repo.id]?.find((worktree) => ownerOptions.executionHostId === void 0 || worktree.hostId === ownerOptions.executionHostId);
					if (folderWorktree) activateAndRevealWorktree(folderWorktree.id, {
						sidebarRevealBehavior: "auto",
						...ownerOptions.executionHostId ? { executionHostId: ownerOptions.executionHostId } : {}
					});
					await markOnboardingProjectAdded("addedFolder");
					closeModal();
				}
			} catch (err) {
				if (gen !== createGenRef.current || requestHostToken !== hostTokenRef.current || !mountedRef.current) return;
				setCreateError(extractIpcErrorMessage(err, String(err)));
			} finally {
				if (gen === createGenRef.current && requestHostToken === hostTokenRef.current && mountedRef.current) setIsCreating(false);
			}
		}, [
			createName,
			createParent,
			fetchWorktrees,
			mountedRef,
			closeModal,
			onGitRepoReady,
			options.runtimeEnvironmentId,
			options.sshTargetId
		])
	};
}
var DRIVE_ANCHOR_RE = /^[A-Za-z]:([\\/]|$)/;
function isDrivePath(p) {
	return DRIVE_ANCHOR_RE.test(p);
}
function isDriveRoot(p) {
	return /^[A-Za-z]:[\\/]?$/.test(p);
}
function driveRootOf(p) {
	return `${p[0].toUpperCase()}:\\`;
}
function splitBrowsePath(p, pathFlavor = "posix") {
	if (pathFlavor === "win32" && isDrivePath(p)) return {
		kind: "drive",
		driveRoot: driveRootOf(p),
		segments: p.slice(2).split(/[\\/]/).filter(Boolean)
	};
	return {
		kind: "posix",
		segments: p.split("/").filter(Boolean)
	};
}
function joinDrivePath(base, name) {
	return `${base.replace(/[\\/]+$/, "")}\\${name}`;
}
function parentOfDrivePath(p) {
	if (isDriveRoot(p)) return "/";
	const parts = splitBrowsePath(p, "win32");
	if (parts.kind !== "drive") return p;
	const parentSegments = parts.segments.slice(0, -1);
	return parentSegments.length === 0 ? parts.driveRoot : `${parts.driveRoot}${parentSegments.join("\\")}`;
}
function driveBreadcrumbPath(driveRoot, segments, endIndex) {
	const kept = segments.slice(0, endIndex + 1);
	return kept.length === 0 ? driveRoot : `${driveRoot}${kept.join("\\")}`;
}
const REMOTE_FILE_BROWSER_FILTER_QUERY_MAX_BYTES = 2 * 1024;
function isRemoteFileBrowserFilterQueryTooLarge(query, maxBytes = REMOTE_FILE_BROWSER_FILTER_QUERY_MAX_BYTES) {
	return isClipboardTextByteLengthOverLimit(query, maxBytes);
}
function filterEntries(entries, filter) {
	if (isRemoteFileBrowserFilterQueryTooLarge(filter)) return [];
	const trimmedFilter = filter.trim();
	if (!trimmedFilter) return entries;
	const q = trimmedFilter.toLowerCase();
	return entries.filter((e) => e.name.toLowerCase().includes(q));
}
function decideEnterAction(filteredEntries) {
	const folders = filteredEntries.filter((e) => e.isDirectory);
	if (folders.length === 1) return {
		type: "navigate",
		name: folders[0].name
	};
	if (folders.length === 0 && filteredEntries.length > 0) return { type: "fileHint" };
	return { type: "noop" };
}
function decideEscAction(filter) {
	return filter.length > 0 ? { type: "clearFilter" } : { type: "cancel" };
}
function joinPath(resolvedPath, name, pathFlavor = "posix") {
	if (pathFlavor === "win32" && resolvedPath === "/" && isDrivePath(name)) return driveRootOf(name);
	if (pathFlavor === "win32" && isDrivePath(resolvedPath)) return joinDrivePath(resolvedPath, name);
	return resolvedPath === "/" ? `/${name}` : `${resolvedPath}/${name}`;
}
function parentPath(p, pathFlavor = "posix") {
	if (pathFlavor === "win32" && isDrivePath(p)) return parentOfDrivePath(p);
	if (p === "/" || p === "") return "/";
	return p.replace(/\/[^/]+\/?$/, "") || "/";
}
function isPathMode(raw, pathFlavor = "posix") {
	if (raw.includes("/")) return true;
	if (pathFlavor === "win32" && isDrivePath(raw)) return true;
	return raw === "~" || raw === "." || raw === "..";
}
function isRemoteFileBrowserPathResolveTextTooLarge(text) {
	return shouldHandleTextControlPaste(text);
}
function shouldDeferRemoteFileBrowserPasteResolve(text) {
	return isRemoteFileBrowserPathResolveTextTooLarge(text);
}
function parsePathInput(raw, pathFlavor = "posix") {
	if (!isPathMode(raw, pathFlavor)) return {
		mode: "filter",
		filter: raw
	};
	if (raw === "~") return {
		mode: "path",
		base: "home",
		committedSegments: [],
		trailingFilter: ""
	};
	if (raw === ".") return {
		mode: "path",
		base: "cwd",
		committedSegments: [],
		trailingFilter: ""
	};
	if (raw === "..") return {
		mode: "path",
		base: "cwd",
		committedSegments: [".."],
		trailingFilter: ""
	};
	let base;
	let driveRoot;
	let remainder;
	if (pathFlavor === "win32" && isDrivePath(raw)) {
		base = "drive";
		driveRoot = driveRootOf(raw);
		remainder = raw.slice(2).replace(/^[\\/]/, "");
	} else if (raw.startsWith("/")) {
		base = "root";
		remainder = raw.slice(1);
	} else if (raw.startsWith("~/")) {
		base = "home";
		remainder = raw.slice(2);
	} else {
		base = "cwd";
		remainder = raw;
	}
	if (base === "drive" ? /[\\/]{2,}/.test(remainder) : remainder.includes("//")) return {
		mode: "path",
		base,
		driveRoot,
		committedSegments: [],
		trailingFilter: "",
		invalid: "Invalid path: repeated separators"
	};
	if (/[\x00-\x1F]/.test(remainder)) return {
		mode: "path",
		base,
		driveRoot,
		committedSegments: [],
		trailingFilter: "",
		invalid: "Invalid path: control characters are not allowed"
	};
	const parts = remainder === "" ? [""] : base === "drive" ? remainder.split(/[\\/]/) : remainder.split("/");
	const trailingFilter = parts.at(-1) ?? "";
	const committedSegments = parts.slice(0, -1);
	return {
		mode: "path",
		base,
		driveRoot,
		committedSegments,
		trailingFilter
	};
}
function resolveSegmentStep(segment, basePath, baseEntries) {
	if (segment === ".") return { type: "stay" };
	if (segment === "..") return { type: "stay" };
	const exact = baseEntries.find((e) => e.name === segment);
	if (exact) {
		if (exact.isDirectory) return {
			type: "descend",
			name: exact.name
		};
		return {
			type: "error",
			message: translate("auto.components.sidebar.remote.file.browser.helpers.4dbd72a7d7", "{{value0}} isn't a directory in {{value1}}", {
				value0: segment,
				value1: basePath
			})
		};
	}
	const segLower = segment.toLowerCase();
	const ciExact = baseEntries.find((e) => e.name.toLowerCase() === segLower);
	if (ciExact) {
		if (ciExact.isDirectory) return {
			type: "descend",
			name: ciExact.name
		};
		return {
			type: "error",
			message: translate("auto.components.sidebar.remote.file.browser.helpers.4dbd72a7d7", "{{value0}} isn't a directory in {{value1}}", {
				value0: segment,
				value1: basePath
			})
		};
	}
	const dirMatches = baseEntries.filter((e) => e.isDirectory && e.name.toLowerCase().startsWith(segLower));
	if (dirMatches.length === 1) return {
		type: "descend",
		name: dirMatches[0].name
	};
	if (dirMatches.length > 1) return {
		type: "error",
		message: translate("auto.components.sidebar.remote.file.browser.helpers.be266af66c", "{{value0}} matches multiple directories in {{value1}}", {
			value0: segment,
			value1: basePath
		})
	};
	return {
		type: "error",
		message: translate("auto.components.sidebar.remote.file.browser.helpers.4dbd72a7d7", "{{value0}} isn't a directory in {{value1}}", {
			value0: segment,
			value1: basePath
		})
	};
}
async function browseRuntimeServerDirectory(environmentId, path) {
	const listing = await callRuntimeRpc({
		kind: "environment",
		environmentId
	}, "files.browseServerDir", { path }, { timeoutMs: 15e3 });
	return {
		...listing,
		entries: sortDirEntries(listing.entries)
	};
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var FILE_HINT_MS = 2e3;
var FILE_HINT_TEXT = "Files can't be opened as a project";
var PATH_DEBOUNCE_MS = 300;
function RemoteFileBrowser({ targetId, runtimeEnvironmentId, initialPath = "~", onSelect, onCancel }) {
	const [resolvedPath, setResolvedPath] = (0, import_react.useState)("");
	const [entries, setEntries] = (0, import_react.useState)([]);
	const [pathFlavor, setPathFlavor] = (0, import_react.useState)("posix");
	const [loading, setLoading] = (0, import_react.useState)(true);
	const [error, setError] = (0, import_react.useState)(null);
	const [filter, setFilter] = (0, import_react.useState)("");
	const [fileHint, setFileHint] = (0, import_react.useState)(false);
	const [preview, setPreview] = (0, import_react.useState)(null);
	const genRef = (0, import_react.useRef)(0);
	const previewGenRef = (0, import_react.useRef)(0);
	const inputRef = (0, import_react.useRef)(null);
	const fileHintTimerRef = (0, import_react.useRef)(null);
	const debounceTimerRef = (0, import_react.useRef)(null);
	const pasteResolveTimerRef = (0, import_react.useRef)(null);
	const clickTimerRef = (0, import_react.useRef)(null);
	const listingCacheRef = (0, import_react.useRef)(/* @__PURE__ */ new Map());
	const homePathRef = (0, import_react.useRef)(null);
	const lastCommittedPrefixRef = (0, import_react.useRef)("");
	const clearFileHint = (0, import_react.useCallback)(() => {
		if (fileHintTimerRef.current) {
			clearTimeout(fileHintTimerRef.current);
			fileHintTimerRef.current = null;
		}
		setFileHint(false);
	}, []);
	const invalidateBrowseRequests = (0, import_react.useCallback)(() => {
		genRef.current++;
		previewGenRef.current++;
	}, []);
	const setBrowserRootRef = (0, import_react.useCallback)((node) => {
		if (node !== null) return;
		invalidateBrowseRequests();
		for (const timerRef of [
			fileHintTimerRef,
			debounceTimerRef,
			pasteResolveTimerRef,
			clickTimerRef
		]) if (timerRef.current) {
			clearTimeout(timerRef.current);
			timerRef.current = null;
		}
	}, [invalidateBrowseRequests]);
	const fetchListing = (0, import_react.useCallback)(async (dirPath) => {
		const cached = listingCacheRef.current.get(dirPath);
		if (cached) return cached;
		const result = targetId ? await window.api.ssh.browseDir({
			targetId,
			dirPath
		}) : await browseRuntimeServerDirectory(requireRuntimeEnvironmentId(runtimeEnvironmentId), dirPath);
		listingCacheRef.current.set(result.resolvedPath, result);
		if (dirPath !== result.resolvedPath) listingCacheRef.current.set(dirPath, result);
		return result;
	}, [runtimeEnvironmentId, targetId]);
	const loadDir = (0, import_react.useCallback)(async (dirPath) => {
		const gen = ++genRef.current;
		setLoading(true);
		setError(null);
		try {
			const result = await fetchListing(dirPath);
			if (gen !== genRef.current) return;
			setResolvedPath(result.resolvedPath);
			setEntries(result.entries);
			setPathFlavor(result.pathFlavor);
			if (dirPath === "~") homePathRef.current = result.resolvedPath;
		} catch (err) {
			if (gen !== genRef.current) return;
			setError(err instanceof Error ? err.message : String(err));
			setEntries([]);
		} finally {
			if (gen === genRef.current) setLoading(false);
		}
	}, [fetchListing]);
	const navigate = (0, import_react.useCallback)((dirPath) => {
		setFilter("");
		setPreview(null);
		previewGenRef.current++;
		lastCommittedPrefixRef.current = "";
		if (debounceTimerRef.current) {
			clearTimeout(debounceTimerRef.current);
			debounceTimerRef.current = null;
		}
		clearFileHint();
		loadDir(dirPath);
	}, [loadDir, clearFileHint]);
	(0, import_react.useEffect)(() => {
		loadDir(initialPath);
	}, [loadDir, initialPath]);
	const navigateInto = (0, import_react.useCallback)((name) => {
		navigate(joinPath(resolvedPath, name, pathFlavor));
	}, [
		resolvedPath,
		navigate,
		pathFlavor
	]);
	const navigateUp = (0, import_react.useCallback)(() => {
		if (resolvedPath === "/") return;
		navigate(parentPath(resolvedPath, pathFlavor));
	}, [
		resolvedPath,
		navigate,
		pathFlavor
	]);
	const filteredEntries = (0, import_react.useMemo)(() => filterEntries(entries, filter), [entries, filter]);
	const previewFilteredEntries = (0, import_react.useMemo)(() => preview ? filterEntries(preview.entries, preview.filter) : [], [preview]);
	const triggerFileHint = (0, import_react.useCallback)(() => {
		if (fileHintTimerRef.current) clearTimeout(fileHintTimerRef.current);
		setFileHint(true);
		fileHintTimerRef.current = setTimeout(() => {
			setFileHint(false);
			fileHintTimerRef.current = null;
		}, FILE_HINT_MS);
	}, []);
	const resolvePathInput = (0, import_react.useCallback)(async (raw) => {
		const parsed = parsePathInput(raw, pathFlavor);
		if (parsed.mode !== "path") return;
		const gen = ++previewGenRef.current;
		if (parsed.invalid) {
			setPreview({
				resolvedPath,
				entries: [],
				filter: "",
				error: parsed.invalid,
				loading: false
			});
			return;
		}
		let basePath;
		if (parsed.base === "root") basePath = "/";
		else if (parsed.base === "drive") basePath = parsed.driveRoot ?? "/";
		else if (parsed.base === "home") {
			if (!homePathRef.current) {
				setPreview({
					resolvedPath,
					entries: [],
					filter: "",
					error: null,
					loading: true
				});
				try {
					const home = await fetchListing("~");
					if (gen !== previewGenRef.current) return;
					homePathRef.current = home.resolvedPath;
				} catch (err) {
					if (gen !== previewGenRef.current) return;
					setPreview({
						resolvedPath,
						entries: [],
						filter: "",
						error: err instanceof Error ? err.message : String(err),
						loading: false
					});
					return;
				}
			}
			basePath = homePathRef.current;
		} else basePath = resolvedPath;
		setPreview((prev) => ({
			resolvedPath: prev?.resolvedPath ?? basePath,
			entries: prev?.entries ?? [],
			filter: prev?.filter ?? "",
			error: null,
			loading: true
		}));
		let currentPath = basePath;
		try {
			for (const segment of parsed.committedSegments) {
				const listing = await fetchListing(currentPath);
				if (gen !== previewGenRef.current) return;
				const outcome = resolveSegmentStep(segment, currentPath, listing.entries);
				if (outcome.type === "error") {
					setPreview({
						resolvedPath: currentPath,
						entries: listing.entries,
						filter: "",
						error: outcome.message,
						loading: false
					});
					return;
				}
				if (outcome.type === "stay") {
					if (segment === "..") currentPath = parentPath(currentPath, listing.pathFlavor);
					continue;
				}
				currentPath = joinPath(currentPath, outcome.name, listing.pathFlavor);
			}
			const finalListing = await fetchListing(currentPath);
			if (gen !== previewGenRef.current) return;
			lastCommittedPrefixRef.current = committedPrefix(raw);
			setPreview({
				resolvedPath: finalListing.resolvedPath,
				entries: finalListing.entries,
				filter: parsed.trailingFilter,
				error: null,
				loading: false
			});
		} catch (err) {
			if (gen !== previewGenRef.current) return;
			setPreview({
				resolvedPath: currentPath,
				entries: [],
				filter: "",
				error: err instanceof Error ? err.message : String(err),
				loading: false
			});
		}
	}, [
		resolvedPath,
		fetchListing,
		pathFlavor
	]);
	const handleInputChange = (0, import_react.useCallback)((raw) => {
		clearFileHint();
		setFilter(raw);
		if (isRemoteFileBrowserPathResolveTextTooLarge(raw)) {
			if (preview) {
				setPreview(null);
				previewGenRef.current++;
			}
			if (debounceTimerRef.current) {
				clearTimeout(debounceTimerRef.current);
				debounceTimerRef.current = null;
			}
			if (pasteResolveTimerRef.current) {
				clearTimeout(pasteResolveTimerRef.current);
				pasteResolveTimerRef.current = null;
			}
			return;
		}
		if (!isPathMode(raw, pathFlavor)) {
			if (preview) {
				setPreview(null);
				previewGenRef.current++;
			}
			if (debounceTimerRef.current) {
				clearTimeout(debounceTimerRef.current);
				debounceTimerRef.current = null;
			}
			return;
		}
		const parsed = parsePathInput(raw, pathFlavor);
		if (parsed.mode === "path" && preview && !preview.error && !parsed.invalid && committedPrefix(raw) === lastCommittedPrefixRef.current) {
			setPreview({
				...preview,
				filter: parsed.trailingFilter
			});
			return;
		}
		if (debounceTimerRef.current) clearTimeout(debounceTimerRef.current);
		debounceTimerRef.current = setTimeout(() => {
			debounceTimerRef.current = null;
			resolvePathInput(raw);
		}, PATH_DEBOUNCE_MS);
	}, [
		clearFileHint,
		preview,
		resolvePathInput,
		pathFlavor
	]);
	const handleInputPaste = (0, import_react.useCallback)((e) => {
		if (e.defaultPrevented) return;
		if (shouldDeferRemoteFileBrowserPasteResolve(e.clipboardData.getData("text/plain"))) return;
		if (pasteResolveTimerRef.current) clearTimeout(pasteResolveTimerRef.current);
		pasteResolveTimerRef.current = setTimeout(() => {
			pasteResolveTimerRef.current = null;
			if (debounceTimerRef.current) {
				clearTimeout(debounceTimerRef.current);
				debounceTimerRef.current = null;
			}
			const value = inputRef.current?.value ?? "";
			if (!isRemoteFileBrowserPathResolveTextTooLarge(value) && isPathMode(value, pathFlavor)) resolvePathInput(value);
		}, 0);
	}, [resolvePathInput, pathFlavor]);
	const handleSelect = (0, import_react.useCallback)(() => {
		onSelect(resolvedPath);
	}, [resolvedPath, onSelect]);
	const listParentPath = preview?.resolvedPath ?? resolvedPath;
	const handleRowClick = (0, import_react.useCallback)((entry) => {
		if (preview?.loading) return;
		if (clickTimerRef.current) clearTimeout(clickTimerRef.current);
		clickTimerRef.current = setTimeout(() => {
			clickTimerRef.current = null;
			if (entry.isDirectory) navigate(joinPath(listParentPath, entry.name, pathFlavor));
			else triggerFileHint();
		}, 220);
	}, [
		navigate,
		triggerFileHint,
		listParentPath,
		preview?.loading,
		pathFlavor
	]);
	const handleRowDoubleClick = (0, import_react.useCallback)((entry) => {
		if (!entry.isDirectory || preview?.loading) return;
		if (clickTimerRef.current) {
			clearTimeout(clickTimerRef.current);
			clickTimerRef.current = null;
		}
		onSelect(joinPath(listParentPath, entry.name, pathFlavor));
	}, [
		listParentPath,
		onSelect,
		preview?.loading,
		pathFlavor
	]);
	const handleFilterKeyDown = (0, import_react.useCallback)((e) => {
		if (e.key === "Enter") {
			if (preview) {
				if (preview.error || preview.loading) {
					e.preventDefault();
					return;
				}
				const parsed = parsePathInput(filter, pathFlavor);
				if (parsed.mode === "path" && parsed.trailingFilter === "") {
					e.preventDefault();
					navigate(preview.resolvedPath);
					return;
				}
				const action$1 = decideEnterAction(filterEntries(preview.entries, preview.filter));
				if (action$1.type === "navigate") {
					e.preventDefault();
					navigate(joinPath(preview.resolvedPath, action$1.name, pathFlavor));
				} else if (action$1.type === "fileHint") {
					e.preventDefault();
					triggerFileHint();
				} else e.preventDefault();
				return;
			}
			const action = decideEnterAction(filteredEntries);
			if (action.type === "navigate") {
				e.preventDefault();
				navigateInto(action.name);
			} else if (action.type === "fileHint") {
				e.preventDefault();
				triggerFileHint();
			}
			return;
		}
		if (e.key === "Escape") if (decideEscAction(filter).type === "clearFilter") {
			e.stopPropagation();
			e.preventDefault();
			setFilter("");
			setPreview(null);
			previewGenRef.current++;
			if (debounceTimerRef.current) {
				clearTimeout(debounceTimerRef.current);
				debounceTimerRef.current = null;
			}
			clearFileHint();
		} else onCancel();
		if (e.key === "Backspace" && filter === "" && !preview) {
			if (resolvedPath !== "/") {
				e.preventDefault();
				navigateUp();
			}
		}
	}, [
		filter,
		filteredEntries,
		preview,
		navigate,
		navigateInto,
		navigateUp,
		resolvedPath,
		triggerFileHint,
		clearFileHint,
		onCancel,
		pathFlavor
	]);
	const browseParts = splitBrowsePath(resolvedPath, pathFlavor);
	const pathSegments = browseParts.segments;
	const breadcrumbPathTo = (0, import_react.useCallback)((segmentIndex) => browseParts.kind === "drive" ? driveBreadcrumbPath(browseParts.driveRoot, browseParts.segments, segmentIndex) : `/${browseParts.segments.slice(0, segmentIndex + 1).join("/")}`, [browseParts]);
	const isPreviewActive = preview !== null;
	const showPreviewLoading = isPreviewActive && preview.loading;
	const displayEntries = isPreviewActive ? previewFilteredEntries : filteredEntries;
	const displayEmptyDirCopy = isPreviewActive ? `${preview.resolvedPath} is empty` : "Empty directory";
	const noMatchesFilter = isPreviewActive ? preview.filter : filter;
	const displayNoMatchesCopy = isRemoteFileBrowserPathResolveTextTooLarge(noMatchesFilter) ? translate("auto.components.sidebar.RemoteFileBrowser.largeInputNoMatches", "No matches for this long input") : translate("auto.components.sidebar.RemoteFileBrowser.00c4235c10", "No matches for '{{value0}}'", { value0: noMatchesFilter });
	const selectDisabled = loading || isPreviewActive && filter !== "";
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		ref: setBrowserRootRef,
		className: "flex flex-col gap-2 min-w-0 w-full",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-center gap-0.5 min-h-[28px] overflow-x-auto scrollbar-none",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
						type: "button",
						onClick: navigateUp,
						disabled: resolvedPath === "/" || loading,
						className: "shrink-0 p-1 rounded hover:bg-accent disabled:opacity-30 transition-colors cursor-pointer disabled:cursor-default",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowUp, { className: "size-3.5" })
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
						type: "button",
						onClick: () => navigate("~"),
						disabled: loading,
						className: "shrink-0 p-1 rounded hover:bg-accent transition-colors cursor-pointer",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(House, { className: "size-3.5" })
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-center gap-0 text-[11px] text-muted-foreground ml-1 min-w-0",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
								type: "button",
								onClick: () => navigate("/"),
								className: "shrink-0 hover:text-foreground transition-colors cursor-pointer px-0.5",
								children: "/"
							}),
							browseParts.kind === "drive" && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronRight, { className: "size-2.5 shrink-0 text-muted-foreground/50" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
								type: "button",
								onClick: () => navigate(browseParts.driveRoot),
								className: cn("truncate max-w-[120px] hover:text-foreground transition-colors cursor-pointer px-0.5", pathSegments.length === 0 && "text-foreground font-medium"),
								children: browseParts.driveRoot.slice(0, 2)
							})] }),
							pathSegments.map((segment, i) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_react.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronRight, { className: "size-2.5 shrink-0 text-muted-foreground/50" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
								type: "button",
								onClick: () => navigate(breadcrumbPathTo(i)),
								className: cn("truncate max-w-[120px] hover:text-foreground transition-colors cursor-pointer px-0.5", i === pathSegments.length - 1 && "text-foreground font-medium"),
								children: segment
							})] }, breadcrumbPathTo(i)))
						]
					})
				]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "relative",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Search, { className: "size-3.5 text-muted-foreground absolute left-2 top-1/2 -translate-y-1/2 pointer-events-none" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("input", {
						ref: inputRef,
						type: "text",
						autoFocus: true,
						value: filter,
						onChange: (e) => handleInputChange(e.target.value),
						onPaste: handleInputPaste,
						onKeyDown: handleFilterKeyDown,
						placeholder: translate("auto.components.sidebar.RemoteFileBrowser.2300612806", "Type to filter or enter a path…"),
						"aria-invalid": !!preview?.error,
						"aria-describedby": preview?.error ? "remote-file-browser-path-error" : void 0,
						className: cn("w-full h-7 pl-7 pr-7 text-xs rounded-md bg-background", "border border-border focus:outline-none focus:ring-1 focus:ring-ring", preview?.error && "border-destructive/60 focus:ring-destructive/60")
					}),
					showPreviewLoading && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3.5 absolute right-2 top-1/2 -translate-y-1/2 animate-spin text-muted-foreground" })
				]
			}),
			preview?.error && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				id: "remote-file-browser-path-error",
				role: "alert",
				className: "text-[11px] text-destructive px-0.5 -mt-1",
				children: preview.error
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "border border-border rounded-md overflow-hidden bg-background",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "h-[240px] overflow-y-auto scrollbar-sleek",
					children: loading ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "flex items-center justify-center h-full",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-5 animate-spin text-muted-foreground" })
					}) : error ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "flex items-center justify-center h-full px-4",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "text-xs text-destructive text-center",
							children: error
						})
					}) : isPreviewActive && preview.entries.length === 0 && !preview.error && !preview.loading ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "flex items-center justify-center h-full",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "text-xs text-muted-foreground",
							children: displayEmptyDirCopy
						})
					}) : !isPreviewActive && entries.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "flex items-center justify-center h-full",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "text-xs text-muted-foreground",
							children: translate("auto.components.sidebar.RemoteFileBrowser.51001182e3", "Empty directory")
						})
					}) : displayEntries.length === 0 && !preview?.error ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-center justify-center h-full",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "text-xs text-muted-foreground",
							children: displayNoMatchesCopy
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "text-xs text-muted-foreground",
							children: displayNoMatchesCopy
						})]
					}) : displayEntries.map((entry) => {
						const FileIcon = getFileTypeIcon(entry.name);
						return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
							type: "button",
							onClick: () => handleRowClick(entry),
							onDoubleClick: () => handleRowDoubleClick(entry),
							onMouseDown: (e) => {
								e.preventDefault();
								inputRef.current?.focus();
							},
							className: cn("w-full flex items-center gap-2 px-3 py-1.5 text-xs text-left transition-colors cursor-pointer", "hover:bg-accent/60"),
							children: [
								entry.isDirectory ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Folder, { className: "size-3.5 text-muted-foreground shrink-0" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(FileIcon, { className: "size-3.5 text-muted-foreground/60 shrink-0" }),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "truncate flex-1 min-w-0",
									children: entry.name
								}),
								entry.isDirectory && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronRight, { className: "size-3.5 text-muted-foreground/60 shrink-0" })
							]
						}, entry.name);
					})
				})
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				className: "block text-[10px] text-muted-foreground truncate w-full",
				title: fileHint ? void 0 : resolvedPath,
				children: fileHint ? FILE_HINT_TEXT : translate("auto.components.sidebar.RemoteFileBrowser.971d85cc84", "Opens as a project on this host · {{value0}}", { value0: resolvedPath })
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-center justify-end gap-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					variant: "outline",
					size: "sm",
					className: "h-7 text-xs",
					onClick: onCancel,
					children: translate("auto.components.sidebar.RemoteFileBrowser.f8b1deb1a4", "Cancel")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					size: "sm",
					className: "h-7 text-xs",
					onClick: handleSelect,
					disabled: selectDisabled,
					title: resolvedPath,
					children: translate("auto.components.sidebar.RemoteFileBrowser.9e060f5815", "Select folder")
				})]
			})
		]
	});
}
function committedPrefix(raw) {
	const i = Math.max(raw.lastIndexOf("/"), raw.lastIndexOf("\\"));
	return i === -1 ? "" : raw.slice(0, i + 1);
}
function requireRuntimeEnvironmentId(runtimeEnvironmentId) {
	if (!runtimeEnvironmentId) throw new Error("Runtime environment is required");
	return runtimeEnvironmentId;
}
function CloneStep({ cloneUrl, cloneDestination, cloneError, cloneProgress, isCloning, disableDestinationPicker = false, runtimeEnvironmentId, sshTargetId, cloneTargetLabel, onUrlChange, onDestChange, onPickDestination, onClone }) {
	const [browsingDestination, setBrowsingDestination] = (0, import_react.useState)(false);
	const isRemoteClone = Boolean(runtimeEnvironmentId || sshTargetId);
	const canBrowseRemoteDestination = isRemoteClone;
	const canClone = !!cloneUrl.trim() && !!cloneDestination.trim() && !isCloning;
	const handleKeyDown = (e) => {
		if (e.key === "Enter" && !e.nativeEvent.isComposing) {
			e.preventDefault();
			if (canClone) onClone();
		}
	};
	if (browsingDestination && (runtimeEnvironmentId || sshTargetId)) return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, { children: translate("auto.components.sidebar.AddRepoSteps.a93ef169b5", "Browse host filesystem") }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, { children: translate("auto.components.sidebar.AddRepoSteps.fe8e629fe3", "Navigate to a directory and click Select to choose it.") })] }), sshTargetId ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RemoteFileBrowser, {
		targetId: sshTargetId,
		initialPath: cloneDestination || "~",
		onSelect: (path) => {
			onDestChange(path);
			setBrowsingDestination(false);
		},
		onCancel: () => setBrowsingDestination(false)
	}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RemoteFileBrowser, {
		runtimeEnvironmentId,
		initialPath: cloneDestination || "~",
		onSelect: (path) => {
			onDestChange(path);
			setBrowsingDestination(false);
		},
		onCancel: () => setBrowsingDestination(false)
	})] });
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, { children: translate("auto.components.sidebar.AddRepoSteps.c05f88a31f", "Clone from URL") }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, { children: cloneTargetLabel ? translate("auto.components.sidebar.AddRepoSteps.cloneOnHostDescription", "Enter the Git URL and choose where to clone it on {{value0}}.", { value0: cloneTargetLabel }) : translate("auto.components.sidebar.AddRepoSteps.5b2ea674b1", "Enter the Git URL and choose where to clone it.") })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-3 pt-1",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-1",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("label", {
					className: "text-[11px] font-medium text-muted-foreground",
					children: translate("auto.components.sidebar.AddRepoSteps.3d4acbe693", "Git URL")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
					value: cloneUrl,
					onChange: (e) => onUrlChange(e.target.value),
					onKeyDown: handleKeyDown,
					placeholder: translate("auto.components.sidebar.AddRepoSteps.b698a4a29d", "https://github.com/user/repo.git"),
					className: "h-8 text-xs",
					disabled: isCloning,
					autoFocus: true
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-1",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("label", {
					className: "text-[11px] font-medium text-muted-foreground",
					children: translate("auto.components.sidebar.AddRepoSteps.cloneParentFolder", "Parent folder")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex gap-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
						value: cloneDestination,
						onChange: (e) => onDestChange(e.target.value),
						onKeyDown: handleKeyDown,
						placeholder: isRemoteClone ? translate("auto.components.sidebar.AddRepoSteps.remoteCloneParentPlaceholder", "/home/user/projects") : translate("auto.components.sidebar.AddRepoSteps.2ce3f6edf8", "/path/to/destination"),
						className: "h-8 text-xs flex-1",
						disabled: isCloning
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						variant: "outline",
						size: "sm",
						className: "h-8 px-2 shrink-0",
						onClick: () => {
							if (canBrowseRemoteDestination) {
								setBrowsingDestination(true);
								return;
							}
							onPickDestination();
						},
						disabled: isCloning || disableDestinationPicker && !canBrowseRemoteDestination,
						title: canBrowseRemoteDestination ? translate("auto.components.sidebar.AddRepoSteps.a93ef169b5", "Browse host filesystem") : translate("auto.components.sidebar.AddRepoSteps.569326d9cc", "Choose folder"),
						"aria-label": canBrowseRemoteDestination ? translate("auto.components.sidebar.AddRepoSteps.a93ef169b5", "Browse host filesystem") : translate("auto.components.sidebar.AddRepoSteps.569326d9cc", "Choose folder"),
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Folder, { className: "size-3.5" })
					})]
				})]
			}),
			cloneError && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				className: "text-[11px] text-destructive",
				children: cloneError
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				onClick: onClone,
				disabled: !cloneUrl.trim() || !cloneDestination.trim() || isCloning,
				className: "w-full",
				children: isCloning ? translate("auto.components.sidebar.AddRepoSteps.69f5b5380d", "Cloning...") : translate("auto.components.sidebar.AddRepoSteps.32a7256d85", "Clone")
			}),
			isCloning && cloneProgress && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-1.5",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-center justify-between text-[11px] text-muted-foreground",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: cloneProgress.phase }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", { children: [cloneProgress.percent, "%"] })]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "h-1.5 w-full rounded-full bg-secondary overflow-hidden",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "h-full rounded-full bg-foreground transition-[width] duration-300 ease-out",
						style: { width: `${cloneProgress.percent}%` }
					})
				})]
			})
		]
	})] });
}
function SshTargetRow({ target, isSelected, onSelect, onConnect }) {
	const connecting = useSshConnectInFlight(target.id);
	const status = target.state?.status ?? "disconnected";
	const isConnected = status === "connected";
	const isBusy = connecting || isConnectingSshStatus(status);
	const dotColor = isConnected ? "bg-green-500" : isBusy ? "bg-yellow-500" : "bg-muted-foreground/30";
	const handleRowClick = () => {
		if (isConnected) onSelect(target.id);
	};
	const handleConnectClick = (e) => {
		e.stopPropagation();
		if (isBusy || isSshConnectInFlight(target.id)) return;
		beginSshConnect(target.id);
		onConnect(target.id).finally(() => {
			endSshConnect(target.id);
		});
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		role: isConnected ? "button" : void 0,
		tabIndex: isConnected ? 0 : void 0,
		className: `w-full flex items-center gap-2 px-3 py-2 rounded-md border text-xs transition-colors ${isSelected ? "border-foreground/30 bg-accent" : "border-border hover:bg-accent/50"} ${isConnected ? "cursor-pointer" : ""}`,
		onClick: handleRowClick,
		onKeyDown: (e) => {
			if (isConnected && (e.key === "Enter" || e.key === " ")) {
				e.preventDefault();
				onSelect(target.id);
			}
		},
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: `size-2 rounded-full shrink-0 ${dotColor}` }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: `font-medium truncate ${!isConnected ? "text-muted-foreground" : ""}`,
				children: target.label || `${target.username}@${target.host}`
			}),
			!isConnected && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				type: "button",
				className: "ml-auto shrink-0 rounded px-1.5 py-0.5 text-[11px] font-medium text-foreground hover:bg-accent/70 disabled:opacity-50 disabled:cursor-default flex items-center gap-1",
				onClick: handleConnectClick,
				disabled: isBusy,
				children: isBusy ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3 animate-spin" }), translate("auto.components.sidebar.SshTargetRow.4677394048", "Connecting…")] }) : translate("auto.components.sidebar.SshTargetRow.75ad429b5d", "Connect")
			})
		]
	});
}
function RemoteStep({ sshTargets, selectedTargetId, lockSshTargetSelection = false, remotePath, remoteError, isAddingRemote, isScanningNested, onSelectTarget, onRemotePathChange, onAdd, onOpenSshSettings, onConnectTarget, onStopNestedScan }) {
	const [browsing, setBrowsing] = (0, import_react.useState)(false);
	const selectedTarget = selectedTargetId ? sshTargets.find((target) => target.id === selectedTargetId) : null;
	const selectedTargetLabel = selectedTarget?.label || (selectedTarget ? `${selectedTarget.username}@${selectedTarget.host}` : selectedTargetId);
	const selectedTargetConnected = (selectedTarget?.state?.status ?? "disconnected") === "connected";
	if (browsing && selectedTargetId) return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, { children: translate("auto.components.sidebar.AddRepoRemoteStep.dd3ff65486", "Browse remote filesystem") }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, { children: translate("auto.components.sidebar.AddRepoRemoteStep.007651bdf9", "Navigate to a directory and click Select to choose it.") })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RemoteFileBrowser, {
		targetId: selectedTargetId,
		initialPath: remotePath || "~",
		onSelect: (path) => {
			onRemotePathChange(path);
			setBrowsing(false);
		},
		onCancel: () => setBrowsing(false)
	})] });
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, { children: translate("auto.components.sidebar.AddRepoRemoteStep.91b93a90a4", "Open project on SSH host") }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, { children: lockSshTargetSelection ? translate("auto.components.sidebar.AddRepoRemoteStep.lockedDescription", "Enter the path to a Git repository on {{value0}}.", { value0: selectedTargetLabel ?? "this SSH target" }) : translate("auto.components.sidebar.AddRepoRemoteStep.80557be85a", "Choose a connected SSH target and enter the path to a Git repository.") })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-3 pt-1",
		children: [
			!lockSshTargetSelection ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-1",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("label", {
					className: "text-[11px] font-medium text-muted-foreground",
					children: translate("auto.components.sidebar.AddRepoRemoteStep.44637f43bd", "SSH target")
				}), sshTargets.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "space-y-1.5 py-1",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "text-xs text-muted-foreground",
						children: translate("auto.components.sidebar.AddRepoRemoteStep.df6fbcf880", "No SSH targets configured.")
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
						variant: "outline",
						size: "sm",
						className: "h-7 text-xs",
						onClick: onOpenSshSettings,
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Settings, { className: "size-3.5" }), translate("auto.components.sidebar.AddRepoRemoteStep.0416bde073", "Add in Settings")]
					})]
				}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "space-y-1.5 max-h-64 overflow-y-auto pr-1 scrollbar-sleek",
					children: sshTargets.map((target) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SshTargetRow, {
						target,
						isSelected: selectedTargetId === target.id,
						onSelect: onSelectTarget,
						onConnect: onConnectTarget
					}, target.id))
				})]
			}) : selectedTarget && !selectedTargetConnected ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-center justify-between gap-3 rounded-md border border-border bg-muted/30 px-3 py-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "min-w-0 text-xs text-muted-foreground",
					children: translate("auto.components.sidebar.AddRepoRemoteStep.lockedDisconnected", "{{value0}} is disconnected.", { value0: selectedTargetLabel ?? "This SSH host" })
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					variant: "outline",
					size: "xs",
					className: "shrink-0",
					onClick: () => onConnectTarget(selectedTarget.id),
					children: translate("auto.components.sidebar.AddRepoRemoteStep.93e0221434", "Connect")
				})]
			}) : null,
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-1",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("label", {
					className: "text-[11px] font-medium text-muted-foreground",
					children: translate("auto.components.sidebar.AddRepoRemoteStep.ef410aa881", "Host path")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex gap-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
						value: remotePath,
						onChange: (event) => onRemotePathChange(event.target.value),
						onKeyDown: (event) => {
							if (event.key === "Enter" && !event.nativeEvent.isComposing) {
								event.preventDefault();
								if (selectedTargetId && remotePath.trim() && !isAddingRemote) onAdd();
							}
						},
						placeholder: translate("auto.components.sidebar.AddRepoRemoteStep.6680289908", "/home/user/project"),
						className: "h-8 text-xs flex-1",
						disabled: isAddingRemote || !selectedTargetId || !selectedTargetConnected
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						variant: "outline",
						size: "sm",
						className: "h-8 px-2 shrink-0",
						onClick: () => setBrowsing(true),
						disabled: !selectedTargetId || !selectedTargetConnected || isAddingRemote,
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(FolderOpen, { className: "size-3.5" })
					})]
				})]
			}),
			remoteError ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				className: "text-[11px] text-destructive",
				children: remoteError
			}) : null,
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				onClick: onAdd,
				disabled: !selectedTargetId || !selectedTargetConnected || !remotePath.trim() || isAddingRemote,
				className: "w-full",
				children: isAddingRemote ? translate("auto.components.sidebar.AddRepoRemoteStep.35831a7312", "Adding...") : translate("auto.components.sidebar.AddRepoRemoteStep.36d427bb66", "Add project on SSH host")
			}),
			isScanningNested ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
				variant: "outline",
				className: "w-full",
				onClick: onStopNestedScan,
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CircleStop, { className: "size-3.5" }), translate("auto.components.sidebar.AddRepoRemoteStep.5b205b5281", "Stop scan")]
			}) : null
		]
	})] });
}
function CreateProjectParentBrowser({ runtimeEnvironmentId, sshTargetId, createParent, onParentChange, onClose }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, { children: translate("auto.components.sidebar.CreateProjectLocationField.f520f83a97", "Browse host filesystem") }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, { children: translate("auto.components.sidebar.CreateProjectLocationField.b589b77997", "Navigate to a directory and click Select to choose it.") })] }), sshTargetId ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RemoteFileBrowser, {
		targetId: sshTargetId,
		initialPath: createParent || "~",
		onSelect: (path) => {
			onParentChange(path);
			onClose();
		},
		onCancel: onClose
	}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RemoteFileBrowser, {
		runtimeEnvironmentId,
		initialPath: createParent || "~",
		onSelect: (path) => {
			onParentChange(path);
			onClose();
		},
		onCancel: onClose
	})] });
}
function CreateProjectLocationField({ createParent, isCreating, manualParentEntry, runtimeEnvironmentId, sshTargetId, onParentChange, onPickParent, onBrowseServer }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-1",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "text-[11px] font-medium text-muted-foreground block",
			children: translate("auto.components.sidebar.CreateProjectLocationField.134e37f711", "Location")
		}), manualParentEntry ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex gap-2",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
				value: createParent,
				onChange: (e) => onParentChange(e.target.value),
				placeholder: translate("auto.components.sidebar.CreateProjectLocationField.2a20a603a3", "/home/user/projects"),
				className: "h-11 min-w-0 flex-1 text-sm font-mono",
				disabled: isCreating,
				spellCheck: false
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
				asChild: true,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					type: "button",
					variant: "outline",
					size: "icon",
					className: "h-11 w-11 shrink-0",
					onClick: onBrowseServer,
					disabled: isCreating || !runtimeEnvironmentId && !sshTargetId,
					"aria-label": translate("auto.components.sidebar.CreateProjectLocationField.f520f83a97", "Browse host filesystem"),
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(FolderOpen, { className: "size-4" })
				})
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
				side: "top",
				sideOffset: 4,
				children: translate("auto.components.sidebar.CreateProjectLocationField.f520f83a97", "Browse host filesystem")
			})] })]
		}) : createParent ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "group flex items-center gap-2.5 rounded-md border border-border bg-background/40 h-11 min-w-0 px-3 text-sm",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "flex-1 min-w-0 truncate font-mono text-[12px]",
				title: createParent,
				children: createParent
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
				type: "button",
				onClick: onPickParent,
				disabled: isCreating,
				className: "shrink-0 inline-flex items-center gap-1 text-[11px] text-muted-foreground hover:text-foreground transition-colors cursor-pointer disabled:cursor-not-allowed",
				"aria-label": translate("auto.components.sidebar.CreateProjectLocationField.afaf54f245", "Change parent folder"),
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Pencil, { className: "size-3" }), translate("auto.components.sidebar.CreateProjectLocationField.632b456b1b", "Change")]
			})]
		}) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
			type: "button",
			variant: "outline",
			onClick: onPickParent,
			disabled: isCreating,
			className: "w-full h-11 justify-start text-sm text-muted-foreground font-normal gap-2.5",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "shrink-0 inline-flex items-center justify-center size-7 rounded-md border border-border/70 bg-background/40",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Folder, { className: "size-3.5" })
			}), translate("auto.components.sidebar.CreateProjectLocationField.95548e33bf", "Choose parent folder...")]
		})]
	});
}
var CREATE_PROJECT_NAME_PLACEHOLDER = "project-name";
function CreateStep({ createName, createParent, createError, isCreating, defaultParent = "", gitAvailability = "unknown", runtimeParentStatus = "idle", parentDefaultPending = false, manualParentEntry = false, runtimeEnvironmentId, sshTargetId, onNameChange, onParentChange, onPickParent, onCreate }) {
	const [browsingParent, setBrowsingParent] = (0, import_react.useState)(false);
	const [advancedOpen, setAdvancedOpen] = (0, import_react.useState)(manualParentEntry);
	const canSubmit = createName.trim().length > 0 && createParent.trim().length > 0 && gitAvailability !== "checking" && gitAvailability !== "unavailable" && !parentDefaultPending && !isCreating;
	const missingLocationLabel = translate("auto.components.sidebar.AddRepoCreateStep.3a13f6e88b", "location not selected");
	const missingServerLocationLabel = translate("auto.components.sidebar.AddRepoCreateStep.6ed14c0281", "host folder not selected");
	const isRemoteHost = Boolean(runtimeEnvironmentId || sshTargetId);
	const summaryParent = (0, import_react.useMemo)(() => formatCreateProjectParentSummary({
		parent: createParent,
		defaultParent,
		runtimeEnvironmentId,
		isRemoteHost,
		missingLocationLabel,
		missingServerLocationLabel
	}), [
		createParent,
		defaultParent,
		isRemoteHost,
		missingLocationLabel,
		missingServerLocationLabel,
		runtimeEnvironmentId
	]);
	const targetPathPreview = (0, import_react.useMemo)(() => {
		const name = createName.trim() || CREATE_PROJECT_NAME_PLACEHOLDER;
		return createParent.trim() ? joinCreateProjectPath(createParent, name) : "";
	}, [createName, createParent]);
	const kindLabel = translate("auto.components.sidebar.AddRepoCreateStep.11fd2a7db8", "Git repository");
	const showGitFallback = gitAvailability === "unavailable";
	const showGitChecking = gitAvailability === "checking";
	const showRuntimeMissingParent = runtimeEnvironmentId && !createParent.trim() && runtimeParentStatus !== "checking";
	if (browsingParent && (runtimeEnvironmentId || sshTargetId)) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CreateProjectParentBrowser, {
		runtimeEnvironmentId,
		sshTargetId,
		createParent,
		onParentChange,
		onClose: () => setBrowsingParent(false)
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, { children: translate("auto.components.sidebar.AddRepoCreateStep.c7b9f94456", "Create a new project") }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, { children: translate("auto.components.sidebar.AddRepoCreateStep.b100311784", "Name it and Orca will create a real project with sensible defaults.") })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-3.5 pt-1 min-w-0",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-1",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("label", {
					htmlFor: "create-project-name",
					className: "text-[11px] font-medium text-muted-foreground block",
					children: translate("auto.components.sidebar.AddRepoCreateStep.a8149a3a5a", "Name")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
					id: "create-project-name",
					value: createName,
					onChange: (e) => onNameChange(e.target.value),
					placeholder: translate("auto.components.sidebar.AddRepoCreateStep.0ae45b8238", "my-project"),
					className: "h-11 text-sm font-mono",
					disabled: isCreating,
					autoFocus: true,
					autoComplete: "off",
					spellCheck: false
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "min-w-0 rounded-md border border-border bg-muted/30",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
					type: "button",
					onClick: () => setAdvancedOpen((open) => !open),
					"aria-expanded": advancedOpen,
					className: "flex w-full min-w-0 items-start gap-2.5 rounded-md px-3 py-2.5 text-left transition-colors cursor-pointer hover:bg-accent/50 focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "mt-0.5 inline-flex size-6 shrink-0 items-center justify-center rounded-md border border-border bg-background/60 text-muted-foreground",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(GitBranch, { className: "size-3.5" })
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "min-w-0 flex-1",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "truncate text-sm font-medium",
								children: translate("auto.components.sidebar.AddRepoCreateStep.685b5eefe1", "{{kind}} in {{parent}}", {
									kind: kindLabel,
									parent: summaryParent
								})
							}), showGitChecking ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
								className: "mt-0.5 flex items-center gap-1.5 text-[11px] text-muted-foreground",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3 animate-spin" }), translate("auto.components.sidebar.AddRepoCreateStep.2a762f3b19", "Checking Git on this host...")]
							}) : showGitFallback ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "mt-0.5 text-[11px] text-destructive",
								children: translate("auto.components.sidebar.AddRepoCreateStep.fe1e616c5b", "Git is required to create a project.")
							}) : showRuntimeMissingParent ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "mt-0.5 text-[11px] text-muted-foreground",
								children: translate("auto.components.sidebar.AddRepoCreateStep.c234df77f7", "Choose or enter a host parent folder before creating.")
							}) : targetPathPreview ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "mt-0.5 truncate font-mono text-[11px] text-muted-foreground",
								title: targetPathPreview,
								children: targetPathPreview
							}) : null]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronDown, { className: cn("size-4 shrink-0 self-center text-muted-foreground transition-transform", advancedOpen && "rotate-180") })
					]
				}), advancedOpen && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "space-y-3 border-t border-border px-3 py-3",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CreateProjectLocationField, {
						createParent,
						isCreating,
						manualParentEntry,
						runtimeEnvironmentId,
						sshTargetId,
						onParentChange,
						onPickParent,
						onBrowseServer: () => setBrowsingParent(true)
					}), targetPathPreview && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "min-w-0 break-all rounded-md border border-border bg-background/40 px-2.5 py-2 font-mono text-[11px] text-muted-foreground",
						children: targetPathPreview
					})]
				})]
			}),
			createError && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				className: "text-[11px] text-destructive",
				role: "alert",
				children: createError
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				onClick: onCreate,
				disabled: !canSubmit,
				size: "lg",
				className: "w-full",
				children: isCreating ? translate("auto.components.sidebar.AddRepoCreateStep.85085d74d2", "Creating…") : translate("auto.components.sidebar.AddRepoCreateStep.45b7c26034", "Create project")
			})
		]
	})] });
}
function getAddRepoLocalStartActions({ isSshLikely, onBrowse, onOpenCloneStep, onOpenRemoteStep, onOpenCreateStep, showRemoteAction = true, canCreateProject = true, browseHostKind = "local" }) {
	const primaryAction = {
		kind: "browse",
		icon: FolderOpen,
		title: browseHostKind === "ssh" ? translate("auto.components.sidebar.add.repo.local.start.actions.sshBrowseTitle", "Open project on SSH host") : translate("auto.components.sidebar.add.repo.local.start.actions.2281fdc8c7", "Browse folder"),
		description: browseHostKind === "ssh" ? translate("auto.components.sidebar.add.repo.local.start.actions.sshBrowseDescription", "Existing Git repository or folder on this SSH host") : browseHostKind === "runtime" ? translate("auto.components.sidebar.add.repo.local.start.actions.runtimeBrowseDescription", "Existing Git repository or folder on this host") : translate("auto.components.sidebar.add.repo.local.start.actions.fb4fc5380e", "Local project, Git repo, or folder with many repos"),
		onClick: onBrowse
	};
	const remote = {
		kind: "remote",
		icon: Monitor,
		title: translate("auto.components.sidebar.add.repo.local.start.actions.3d162cc76f", "Project on SSH host"),
		description: translate("auto.components.sidebar.add.repo.local.start.actions.a6c20dca96", "Open a project folder from an SSH host"),
		onClick: onOpenRemoteStep
	};
	const clone = {
		kind: "clone",
		icon: Globe,
		title: translate("auto.components.sidebar.add.repo.local.start.actions.7edb8ebe24", "Clone from URL"),
		description: translate("auto.components.sidebar.add.repo.local.start.actions.5f9ffac036", "Clone a remote Git repository"),
		onClick: onOpenCloneStep
	};
	const create = {
		kind: "create",
		icon: Plus,
		title: translate("auto.components.sidebar.add.repo.local.start.actions.c709860596", "Create new project"),
		description: canCreateProject ? translate("auto.components.sidebar.add.repo.local.start.actions.d72789705e", "Start from an empty folder") : translate("auto.components.sidebar.add.repo.local.start.actions.sshCreateUnavailable", "Not available for SSH hosts yet"),
		disabled: !canCreateProject,
		onClick: onOpenCreateStep
	};
	return {
		primaryAction,
		secondaryActions: showRemoteAction ? isSshLikely ? [
			remote,
			clone,
			create
		] : [
			clone,
			remote,
			create
		] : [clone, create]
	};
}
function AddRepoNestedScanProgressNotice({ busyLabel, nestedScanInProgress, nestedScanId, onStopNestedScan }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex items-center gap-2 rounded-md border border-border bg-muted px-3 py-2 text-xs text-muted-foreground",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3.5 shrink-0 animate-spin" }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "min-w-0 flex-1",
				children: busyLabel
			}),
			nestedScanInProgress && nestedScanId ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
				asChild: true,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
					type: "button",
					variant: "ghost",
					size: "icon-xs",
					className: "group text-muted-foreground hover:bg-destructive/10 hover:text-destructive focus-visible:bg-destructive/10 focus-visible:text-destructive focus-visible:ring-destructive/40",
					"aria-label": translate("auto.components.sidebar.AddRepoStartSteps.9906cae183", "Stop scan"),
					title: translate("auto.components.sidebar.AddRepoStartSteps.69ea7f8dc4", "Stop scanning"),
					onClick: onStopNestedScan,
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3.5 animate-spin text-annotation-highlight group-hover:hidden group-focus-visible:hidden" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CircleStop, { className: "hidden size-3.5 group-hover:block group-focus-visible:block" })]
				})
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
				side: "top",
				sideOffset: 4,
				children: translate("auto.components.sidebar.AddRepoStartSteps.d301db1c9a", "Scanning repositories. Click to stop.")
			})] }) : null
		]
	});
}
function AddRepoLocalStartStep({ repoCount, isSshLikely, isAdding, addProjectBusyLabel, nestedScanInProgress, nestedScanId, hostSelector, showRemoteAction = true, canCreateProject = true, actionsDisabled = false, browseHostKind = "local", onBrowse, onOpenCloneStep, onOpenRemoteStep, onOpenCreateStep, onStopNestedScan }) {
	const browseActionRef = (0, import_react.useRef)(null);
	const actionsRef = (0, import_react.useRef)(null);
	const actionsUnavailable = isAdding || actionsDisabled;
	const { primaryAction, secondaryActions } = getAddRepoLocalStartActions({
		isSshLikely,
		onBrowse,
		onOpenCloneStep,
		onOpenRemoteStep,
		onOpenCreateStep,
		showRemoteAction,
		canCreateProject,
		browseHostKind
	});
	const [selectedKind, setSelectedKind] = (0, import_react.useState)(primaryAction.kind);
	const visibleSelectedKind = actionsUnavailable ? null : selectedKind;
	(0, import_react.useEffect)(() => {
		if (!actionsUnavailable) browseActionRef.current?.focus();
	}, [actionsUnavailable]);
	const handleArrowNavigation = (event) => {
		if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
		const buttons = Array.from(actionsRef.current?.querySelectorAll("button[data-add-repo-action]") ?? []);
		if (buttons.length === 0) return;
		const nextIndex = (buttons.indexOf(document.activeElement) + (event.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length;
		event.preventDefault();
		buttons[nextIndex]?.focus();
	};
	const handleActionsBlur = (event) => {
		if (!(event.relatedTarget instanceof HTMLButtonElement)) {
			setSelectedKind(null);
			return;
		}
		if (!event.relatedTarget.matches("button[data-add-repo-action]")) setSelectedKind(null);
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, { children: translate("auto.components.sidebar.AddRepoStartSteps.d13757911c", "Add a project") }), repoCount === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, { children: translate("auto.components.sidebar.AddRepoStartSteps.acf895cb42", "Add a project to get started with Orca.") }) : null] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-3 pt-2",
		ref: actionsRef,
		onBlur: handleActionsBlur,
		onKeyDown: handleArrowNavigation,
		children: [
			hostSelector,
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AddRepoPrimaryStartAction, {
				icon: primaryAction.icon,
				title: primaryAction.title,
				description: primaryAction.description,
				disabled: actionsUnavailable,
				selected: visibleSelectedKind === primaryAction.kind,
				buttonRef: browseActionRef,
				onClick: primaryAction.onClick,
				onFocus: () => setSelectedKind(primaryAction.kind)
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-1.5",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "text-xs font-medium uppercase tracking-wider text-muted-foreground",
					children: translate("auto.components.sidebar.AddRepoStartSteps.87596c1446", "Other ways to add")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "overflow-hidden rounded-md border border-input bg-background",
					children: secondaryActions.map((action, index) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AddRepoSecondaryStartAction, {
						icon: action.icon,
						title: action.title,
						description: action.description,
						disabled: actionsUnavailable || Boolean(action.disabled),
						selected: visibleSelectedKind === action.kind,
						onClick: action.onClick,
						onFocus: () => setSelectedKind(action.kind),
						className: cn(index === 0 ? "rounded-t-md" : "border-t border-border/70", index === secondaryActions.length - 1 && "rounded-b-md")
					}, action.kind))
				})]
			}),
			isAdding && addProjectBusyLabel ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AddRepoNestedScanProgressNotice, {
				busyLabel: addProjectBusyLabel,
				nestedScanInProgress,
				nestedScanId,
				onStopNestedScan
			}) : null
		]
	})] });
}
var AddRepoEnterChip = () => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
	"aria-hidden": "true",
	className: "shrink-0",
	children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ShortcutKeyCombo, {
		keys: ["⏎"],
		keyCapClassName: "border-border/80 bg-background/70 text-muted-foreground"
	})
});
var AddRepoPrimaryStartAction = ({ icon: Icon, title, description, disabled, selected, onClick, onFocus, buttonRef }) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
	ref: buttonRef,
	type: "button",
	variant: "ghost",
	onClick,
	onFocus,
	disabled,
	"data-add-repo-action": true,
	className: cn("h-auto min-h-[3.75rem] w-full justify-start gap-3 whitespace-normal px-3 py-2.5 text-left", selected ? "border border-ring bg-foreground/10 text-foreground focus-visible:border-ring focus-visible:ring-0 dark:bg-accent dark:text-accent-foreground" : "border border-border bg-background shadow-none dark:bg-background"),
	children: [
		/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: cn("grid size-7 shrink-0 place-items-center rounded-md", selected ? "bg-background/70 text-accent-foreground" : "text-foreground"),
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { className: "size-4" })
		}),
		/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
			className: "min-w-0 flex-1",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "block text-sm font-medium leading-5",
				children: title
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "mt-0.5 block text-xs font-normal leading-5 text-muted-foreground",
				children: description
			})]
		}),
		selected ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AddRepoEnterChip, {}) : null
	]
});
function AddRepoSecondaryStartAction({ icon: Icon, title, description, disabled, selected, onClick, onFocus, className }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
		type: "button",
		"data-add-repo-action": true,
		disabled,
		onClick,
		onFocus,
		className: cn("flex min-h-[3.25rem] w-full items-center gap-3 border border-transparent px-3 py-2.5 text-left transition-colors focus-visible:outline-none disabled:pointer-events-none disabled:cursor-default disabled:opacity-40", className, selected ? "border-ring bg-foreground/10 text-foreground focus-visible:ring-0 dark:bg-accent dark:text-accent-foreground" : "hover:bg-accent focus-visible:bg-accent focus-visible:ring-[3px] focus-visible:ring-inset focus-visible:ring-ring/50"),
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: cn("grid size-7 shrink-0 place-items-center rounded-md", selected ? "bg-background/70 text-accent-foreground" : "text-muted-foreground"),
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { className: "size-4" })
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
				className: "min-w-0 flex-1",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: cn("block text-sm font-medium leading-5", selected ? "text-accent-foreground" : "text-foreground"),
					children: title
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "block text-xs leading-4 text-muted-foreground",
					children: description
				})]
			}),
			selected ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AddRepoEnterChip, {}) : null
		]
	});
}
function AddRepoServerPathStartStep({ serverPath, runtimeEnvironmentId, isAddingServerPath, addProjectBusyLabel, hostSelector, initialBrowsing = false, onServerPathChange, onAddServerPath, onOpenCloneStep, onOpenCreateStep }) {
	const [browsing, setBrowsing] = (0, import_react.useState)(initialBrowsing);
	const [pathEntryOpen, setPathEntryOpen] = (0, import_react.useState)(initialBrowsing);
	if (browsing && runtimeEnvironmentId) return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, { children: translate("auto.components.sidebar.AddRepoServerStartStep.ac66a3ed2d", "Browse host filesystem") }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, { children: translate("auto.components.sidebar.AddRepoServerStartStep.0f8aba944c", "Navigate to a directory and click Select to choose it.") })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RemoteFileBrowser, {
		runtimeEnvironmentId,
		initialPath: serverPath || "~",
		onSelect: (path) => {
			onServerPathChange(path);
			setBrowsing(false);
			setPathEntryOpen(true);
		},
		onCancel: () => setBrowsing(false)
	})] });
	if (!pathEntryOpen) {
		const disabled = isAddingServerPath || !runtimeEnvironmentId;
		return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, { children: translate("auto.components.sidebar.AddRepoServerStartStep.39bd249b3a", "Add a project") }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, { children: translate("auto.components.sidebar.AddRepoServerStartStep.8efa930eb5", "Add another project from the selected host.") })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "space-y-3 pt-2",
			children: [
				hostSelector,
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "grid grid-cols-3 gap-2",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AddRepoServerStartAction, {
							icon: FolderOpen,
							title: translate("auto.components.sidebar.AddRepoServerStartStep.0adf083af7", "Browse host"),
							description: translate("auto.components.sidebar.AddRepoServerStartStep.516187414c", "Existing project or folder"),
							disabled,
							onClick: () => setBrowsing(true)
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AddRepoServerStartAction, {
							icon: Globe,
							title: translate("auto.components.sidebar.AddRepoServerStartStep.47759c9491", "Clone from URL"),
							description: translate("auto.components.sidebar.AddRepoServerStartStep.a2ea37d549", "Remote Git repository"),
							disabled,
							onClick: onOpenCloneStep
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AddRepoServerStartAction, {
							icon: Server,
							title: translate("auto.components.sidebar.AddRepoServerStartStep.a81ffa0a99", "Create on host"),
							description: translate("auto.components.sidebar.AddRepoServerStartStep.d40d751517", "New repo or folder"),
							disabled,
							onClick: onOpenCreateStep
						})
					]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-center gap-3 rounded-md border border-border bg-muted px-3 py-2.5 text-xs text-muted-foreground",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "grid size-7 shrink-0 place-items-center rounded-md bg-background text-foreground",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Lightbulb, { className: "size-3.5" })
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "min-w-0",
						children: translate("auto.components.sidebar.AddRepoServerStartStep.6b9958492a", "Want to import many repos at once? Browse to the parent folder.")
					})]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
					type: "button",
					onClick: () => setPathEntryOpen(true),
					disabled,
					className: "mx-auto block rounded px-2 py-1 text-xs text-muted-foreground transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:cursor-default disabled:opacity-40",
					children: translate("auto.components.sidebar.AddRepoServerStartStep.438493f214", "Or enter a host path manually")
				})
			]
		})] });
	}
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, { children: translate("auto.components.sidebar.AddRepoServerStartStep.3d0c035483", "Open host project") }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, { children: translate("auto.components.sidebar.AddRepoServerStartStep.423b5d3d31", "Add a Git repository or folder that already exists on the selected host.") })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-3 pt-2",
		children: [
			hostSelector,
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-1",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("label", {
					htmlFor: "server-project-path",
					className: "block text-[11px] font-medium text-muted-foreground",
					children: translate("auto.components.sidebar.AddRepoServerStartStep.867692f505", "Host path")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex gap-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
						id: "server-project-path",
						value: serverPath,
						onChange: (event) => onServerPathChange(event.target.value),
						placeholder: translate("auto.components.sidebar.AddRepoServerStartStep.92d25420a0", "/home/user/project"),
						className: "h-11 min-w-0 flex-1 font-mono text-sm",
						disabled: isAddingServerPath,
						autoFocus: true,
						spellCheck: false
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
						asChild: true,
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							type: "button",
							variant: "outline",
							size: "icon",
							className: "h-11 w-11 shrink-0",
							onClick: () => setBrowsing(true),
							disabled: isAddingServerPath || !runtimeEnvironmentId,
							"aria-label": translate("auto.components.sidebar.AddRepoServerStartStep.ac66a3ed2d", "Browse host filesystem"),
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(FolderOpen, { className: "size-4" })
						})
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
						side: "top",
						sideOffset: 4,
						children: translate("auto.components.sidebar.AddRepoServerStartStep.ac66a3ed2d", "Browse host filesystem")
					})] })]
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "grid grid-cols-2 gap-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					onClick: () => onAddServerPath("git"),
					disabled: !serverPath.trim() || isAddingServerPath,
					className: "h-10",
					children: translate("auto.components.sidebar.AddRepoServerStartStep.8da4d1a5be", "Add Git Project")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					onClick: () => onAddServerPath("folder"),
					disabled: !serverPath.trim() || isAddingServerPath,
					variant: "outline",
					className: "h-10",
					children: translate("auto.components.sidebar.AddRepoServerStartStep.e1710bf831", "Open as Folder")
				})]
			}),
			isAddingServerPath && addProjectBusyLabel ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-center gap-2 rounded-md border border-border bg-muted px-3 py-2 text-xs text-muted-foreground",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3.5 shrink-0 animate-spin" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: addProjectBusyLabel })]
			}) : null,
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				type: "button",
				onClick: () => setPathEntryOpen(false),
				disabled: isAddingServerPath,
				className: "mx-auto block rounded px-2 py-1 text-xs text-muted-foreground transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:cursor-default disabled:opacity-40",
				children: translate("auto.components.sidebar.AddRepoServerStartStep.ae990c86a0", "Back to add options")
			})
		]
	})] });
}
function AddRepoServerStartAction({ icon: Icon, title, description, disabled, onClick }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
		type: "button",
		variant: "outline",
		disabled,
		onClick,
		className: "h-32 min-w-0 flex-col gap-3 whitespace-normal border-border/80 bg-background px-3 py-4 text-center",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "grid size-9 shrink-0 place-items-center rounded-md text-muted-foreground",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { className: "size-5" })
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
			className: "min-w-0",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "block text-[13px] font-semibold leading-5 text-foreground",
				children: title
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "mt-0.5 block text-[11px] font-normal leading-4 text-muted-foreground",
				children: description
			})]
		})]
	});
}
function NestedRepoSelectAllRow({ total, selectedCount, disabled, onToggle }) {
	const allSelected = total > 0 && selectedCount === total;
	const isMixed = !allSelected && !(selectedCount === 0);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("label", {
		className: "flex min-w-0 cursor-pointer items-center gap-2.5 bg-muted/30 px-3 py-2 text-sm hover:bg-muted/50",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("input", {
				ref: (0, import_react.useCallback)((checkbox) => {
					if (checkbox) checkbox.indeterminate = isMixed;
				}, [isMixed]),
				type: "checkbox",
				className: "size-3.5",
				checked: allSelected,
				disabled,
				onChange: onToggle,
				"aria-label": allSelected ? translate("auto.components.repo.NestedRepoChecklist.929734aea5", "Deselect all") : translate("auto.components.repo.NestedRepoChecklist.91b5bcadb6", "Select all")
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "min-w-0 truncate text-[12.5px] font-semibold text-foreground",
				children: allSelected ? translate("auto.components.repo.NestedRepoChecklist.929734aea5", "Deselect all") : translate("auto.components.repo.NestedRepoChecklist.91b5bcadb6", "Select all")
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
				className: "ml-auto shrink-0 text-[11px] text-muted-foreground",
				children: [
					selectedCount,
					" ",
					translate("auto.components.repo.NestedRepoChecklist.ea54c7bf8f", "of"),
					" ",
					total,
					" ",
					translate("auto.components.repo.NestedRepoChecklist.f7e1170567", "selected")
				]
			})
		]
	});
}
function NestedRepoChecklist({ scan, selectedPaths, onSelectedPathsChange, disabled = false, className }) {
	const displayLabelsByPath = (0, import_react.useMemo)(() => getRepoDisplayLabelsByPath(scan.repos), [scan.repos]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: cn("flex max-h-64 min-h-0 min-w-0 max-w-full flex-col overflow-hidden rounded-md border border-border bg-background/60", className),
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(NestedRepoSelectAllRow, {
			total: scan.repos.length,
			selectedCount: selectedPaths.size,
			disabled,
			onToggle: () => {
				onSelectedPathsChange((previous) => {
					if (previous.size === scan.repos.length) return /* @__PURE__ */ new Set();
					return new Set(scan.repos.map((repo) => repo.path));
				});
			}
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("ul", {
			className: "scrollbar-sleek min-h-0 flex-1 overflow-y-auto overflow-x-hidden",
			children: scan.repos.map((repo) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("li", { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("label", {
				className: "flex min-w-0 max-w-full cursor-pointer items-center gap-2.5 overflow-hidden border-t border-border px-3 py-2 text-sm hover:bg-accent",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("input", {
						type: "checkbox",
						className: "size-3.5",
						checked: selectedPaths.has(repo.path),
						disabled,
						onChange: (event) => {
							onSelectedPathsChange((previous) => {
								const next = new Set(previous);
								if (event.target.checked) next.add(repo.path);
								else next.delete(repo.path);
								return next;
							});
						}
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(GitBranch, { className: "size-3.5 shrink-0 text-muted-foreground" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: cn("min-w-0 flex-1 truncate text-[13px] font-medium", selectedPaths.has(repo.path) ? "text-foreground" : "text-muted-foreground"),
						children: displayLabelsByPath.get(getRepoDisplayLabelKey(repo)) ?? repo.displayName
					})
				]
			}) }, repo.path))
		})]
	});
}
function formatTimeout(timeoutMs) {
	if (timeoutMs >= 1e3 && timeoutMs % 1e3 === 0) return `${timeoutMs / 1e3} seconds`;
	return `${timeoutMs} ms`;
}
function nestedRepoScanLimitText(scan) {
	const automaticStops = [`${scan.maxDepth} folder levels`, `${scan.maxRepos} repositories`];
	if (scan.timeoutMs !== null) automaticStops.push(formatTimeout(scan.timeoutMs));
	return `Scan stops after ${automaticStops.join(" or ")}. You can stop scanning early and import repositories found so far.`;
}
function NestedRepoScanLimitNotice({ scan }) {
	const [detailsOpen, setDetailsOpen] = (0, import_react.useState)(false);
	const detailsText = nestedRepoScanLimitText(scan);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "inline-flex min-w-0 items-center gap-1.5 text-[11px] text-muted-foreground",
		onPointerEnter: () => setDetailsOpen(true),
		onPointerLeave: () => setDetailsOpen(false),
		onFocusCapture: () => setDetailsOpen(true),
		onBlurCapture: () => setDetailsOpen(false),
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { children: scan.stopped ? translate("auto.components.repo.NestedRepoScanLimitNotice.03e9beab7b", "Scan stopped early.") : translate("auto.components.repo.NestedRepoScanLimitNotice.574eb5408b", "Showing partial scan results.") }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Popover, {
			open: detailsOpen,
			onOpenChange: setDetailsOpen,
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(PopoverTrigger, {
				asChild: true,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
					type: "button",
					"aria-label": translate("auto.components.repo.NestedRepoScanLimitNotice.642a43c139", "Nested repository scan limits"),
					"aria-expanded": detailsOpen,
					title: detailsText,
					className: "inline-flex size-4 shrink-0 items-center justify-center rounded-sm text-muted-foreground transition hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50",
					onClick: (event) => {
						event.stopPropagation();
						setDetailsOpen(true);
					},
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CircleQuestionMark, { className: "size-3.5" })
				})
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PopoverContent, {
				side: "top",
				sideOffset: 4,
				className: "max-w-[260px] px-3 py-2 text-xs leading-5 text-pretty",
				onOpenAutoFocus: (event) => event.preventDefault(),
				children: detailsText
			})]
		})]
	});
}
function AddRepoNestedImportStep({ scan, groupName, selectedPaths, isAdding, scanInProgress, onGroupNameChange, onSelectedPathsChange, onImport, onOpenAsFolder, onStopScan }) {
	const folderName = getRuntimePathBasename(scan.selectedPath) || scan.selectedPath;
	const groupNameInputId = (0, import_react.useId)();
	const [pendingImportMode, setPendingImportMode] = (0, import_react.useState)(null);
	const noRepositoriesSelected = selectedPaths.size === 0;
	const showOpenAsFolderSpinner = isAdding && pendingImportMode === "folder";
	const showSeparateSpinner = isAdding && pendingImportMode === "separate";
	const showGroupSpinner = isAdding && pendingImportMode === "group";
	(0, import_react.useEffect)(() => {
		if (!isAdding) setPendingImportMode(null);
	}, [isAdding]);
	const handleImport = (mode) => {
		setPendingImportMode(mode);
		onImport(mode);
	};
	const handleOpenAsFolder = () => {
		setPendingImportMode("folder");
		onOpenAsFolder();
	};
	const foundSentence = translate("auto.components.sidebar.AddRepoNestedImportStep.b4263a2ac4", "Found {{value0}} in {{value1}}.", {
		value0: scan.repos.length === 1 ? translate("auto.components.sidebar.AddRepoNestedImportStep.8401a7a0d0", "1 repository") : translate("auto.components.sidebar.AddRepoNestedImportStep.d4f1df62ef", "{{value0}} repositories", { value0: scan.repos.length }),
		value1: scan.selectedPath
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, { children: translate("auto.components.sidebar.AddRepoNestedImportStep.8db50afe1a", "Import repositories from folder") }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex min-w-0 items-center gap-1.5",
		children: [scanInProgress ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AddRepoNestedImportStopButton, { onStopScan }) : null, /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, {
			className: "min-w-0 truncate",
			children: scanInProgress ? translate("auto.components.sidebar.AddRepoNestedImportStep.24eda6c8b2", "Scanning... {{value0}}", { value0: foundSentence }) : foundSentence
		})]
	})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex min-h-0 min-w-0 max-w-full flex-col gap-3 overflow-hidden pt-1",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(NestedRepoChecklist, {
				scan,
				selectedPaths,
				onSelectedPathsChange,
				disabled: isAdding || scanInProgress,
				className: "flex-1"
			}),
			scanInProgress || scan.truncated || scan.timedOut || scan.stopped ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(NestedRepoScanLimitNotice, { scan }) : null,
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "min-w-0 shrink-0 space-y-1",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "text-sm font-medium text-foreground",
					children: translate("auto.components.sidebar.AddRepoNestedImportStep.fb33359f69", "Group these repositories?")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "text-xs text-muted-foreground",
					children: translate("auto.components.sidebar.AddRepoNestedImportStep.d75170194e", "Choose this if these projects belong together — a monorepo, or just a set of related repos. Orca will group them and let you work from the parent folder.")
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "min-w-0 shrink-0 space-y-1",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "flex shrink-0 items-center gap-1",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
						htmlFor: groupNameInputId,
						className: "text-[11px] text-muted-foreground",
						children: translate("auto.components.sidebar.AddRepoNestedImportStep.39d51212cc", "Group name")
					})
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
					id: groupNameInputId,
					"aria-label": translate("auto.components.sidebar.AddRepoNestedImportStep.39d51212cc", "Group name"),
					value: groupName,
					onChange: (event) => onGroupNameChange(event.target.value),
					disabled: isAdding || scanInProgress,
					className: "h-9 min-w-0",
					placeholder: folderName
				})]
			}),
			noRepositoriesSelected ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				className: "shrink-0 text-xs text-muted-foreground",
				children: translate("auto.components.sidebar.AddRepoNestedImportStep.6149d5203f", "No repositories are selected. Open the parent folder instead to use editor, terminal, and search without Git features.")
			}) : null,
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex shrink-0 flex-wrap justify-end gap-2",
				children: [
					noRepositoriesSelected ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
						onClick: handleOpenAsFolder,
						disabled: isAdding || scanInProgress,
						variant: "secondary",
						children: [showOpenAsFolderSpinner ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3.5 animate-spin" }) : null, translate("auto.components.sidebar.AddRepoNestedImportStep.e52454b7f6", "Open as Folder")]
					}) : null,
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
						onClick: () => handleImport("separate"),
						disabled: isAdding || scanInProgress || noRepositoriesSelected,
						variant: "outline",
						children: [showSeparateSpinner ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3.5 animate-spin" }) : null, translate("auto.components.sidebar.AddRepoNestedImportStep.aa0247680d", "No, import separately")]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
						onClick: () => handleImport("group"),
						disabled: isAdding || scanInProgress || noRepositoriesSelected,
						children: [showGroupSpinner ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3.5 animate-spin" }) : null, translate("auto.components.sidebar.AddRepoNestedImportStep.a0bc4d1f8e", "Yes, import as group")]
					})
				]
			})
		]
	})] });
}
function AddRepoNestedImportStopButton({ onStopScan }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
		asChild: true,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
			type: "button",
			variant: "ghost",
			size: "icon-xs",
			className: "group text-muted-foreground hover:bg-destructive/10 hover:text-destructive focus-visible:bg-destructive/10 focus-visible:text-destructive focus-visible:ring-destructive/40",
			"aria-label": translate("auto.components.sidebar.AddRepoNestedImportStep.2f8298f3c3", "Stop scan"),
			title: translate("auto.components.sidebar.AddRepoNestedImportStep.a32bef9516", "Stop scanning"),
			onClick: onStopScan,
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3.5 animate-spin text-annotation-highlight group-hover:hidden group-focus-visible:hidden" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CircleStop, { className: "hidden size-3.5 group-hover:block group-focus-visible:block" })]
		})
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
		side: "top",
		sideOffset: 4,
		children: translate("auto.components.sidebar.AddRepoNestedImportStep.496f68cf8c", "Scanning repositories. Click to stop.")
	})] });
}
function AddRepoDialogStepContent({ step, isRuntimeEnvironmentActive, activeRuntimeEnvironmentId, isSshLikely, repoCount, isAdding, addProjectBusyLabel, nestedScanInProgress, nestedScanId, serverPath, isAddingServerPath, cloneUrl, cloneDestination, cloneError, cloneProgress, isCloning, sshTargets, selectedTargetId, selectedSshTargetId, selectedHostLabel, lockSshTargetSelection = false, remotePath, remoteError, isAddingRemote, isScanningRemoteNested, nestedScan, nestedSelectedPaths, nestedGroupName, createName, createParent, createError, isCreating, hostSelector, showRemoteAction = true, canCreateProject = true, actionsDisabled = false, manualCreateParentEntry = isRuntimeEnvironmentActive, browseHostKind = "local", createDefaultParent, createGitAvailability, createRuntimeParentStatus, createParentDefaultPending, onBrowse, onOpenCloneStep, onOpenCreateStep, onOpenRemoteStep, onStopNestedScan, onServerPathChange, onAddServerPath, onSelectTarget, onRemotePathChange, onAddRemoteRepo, onOpenSshSettings, onConnectTarget, onStopRemoteNestedScan, onCloneUrlChange, onCloneDestinationChange, onPickCloneDestination, onClone, onNestedGroupNameChange, onNestedSelectedPathsChange, onImportNestedRepos, onOpenNestedRootFolder, onCreateNameChange, onCreateParentChange, onPickCreateParent, onCreate }) {
	if (step === "add") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AddRepoLocalStartStep, {
		repoCount,
		isSshLikely,
		isAdding,
		addProjectBusyLabel,
		nestedScanInProgress,
		nestedScanId,
		hostSelector,
		showRemoteAction,
		canCreateProject,
		actionsDisabled,
		browseHostKind,
		onBrowse,
		onOpenCloneStep,
		onOpenRemoteStep,
		onOpenCreateStep,
		onStopNestedScan
	});
	if (step === "server-path") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AddRepoServerPathStartStep, {
		serverPath,
		runtimeEnvironmentId: activeRuntimeEnvironmentId,
		isAddingServerPath,
		addProjectBusyLabel,
		hostSelector,
		initialBrowsing: true,
		onServerPathChange,
		onAddServerPath,
		onOpenCloneStep,
		onOpenCreateStep
	});
	if (step === "remote") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RemoteStep, {
		sshTargets,
		selectedTargetId,
		lockSshTargetSelection,
		remotePath,
		remoteError,
		isAddingRemote,
		isScanningNested: isScanningRemoteNested,
		onSelectTarget,
		onRemotePathChange,
		onAdd: onAddRemoteRepo,
		onOpenSshSettings,
		onConnectTarget,
		onStopNestedScan: onStopRemoteNestedScan
	});
	if (step === "clone") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CloneStep, {
		cloneUrl,
		cloneDestination,
		cloneError,
		cloneProgress,
		isCloning,
		disableDestinationPicker: isRuntimeEnvironmentActive,
		runtimeEnvironmentId: activeRuntimeEnvironmentId,
		sshTargetId: selectedSshTargetId,
		cloneTargetLabel: isRuntimeEnvironmentActive || selectedSshTargetId ? selectedHostLabel : null,
		onUrlChange: onCloneUrlChange,
		onDestChange: onCloneDestinationChange,
		onPickDestination: onPickCloneDestination,
		onClone
	});
	if (step === "nested" && nestedScan) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AddRepoNestedImportStep, {
		scan: nestedScan,
		groupName: nestedGroupName,
		selectedPaths: nestedSelectedPaths,
		isAdding,
		scanInProgress: nestedScanInProgress,
		onGroupNameChange: onNestedGroupNameChange,
		onSelectedPathsChange: onNestedSelectedPathsChange,
		onImport: onImportNestedRepos,
		onOpenAsFolder: onOpenNestedRootFolder,
		onStopScan: onStopNestedScan
	});
	if (step === "create") return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CreateStep, {
		createName,
		createParent,
		createError,
		isCreating,
		defaultParent: createDefaultParent,
		gitAvailability: createGitAvailability,
		runtimeParentStatus: createRuntimeParentStatus,
		parentDefaultPending: createParentDefaultPending,
		manualParentEntry: manualCreateParentEntry,
		runtimeEnvironmentId: activeRuntimeEnvironmentId,
		sshTargetId: selectedSshTargetId,
		onNameChange: onCreateNameChange,
		onParentChange: onCreateParentChange,
		onPickParent: onPickCreateParent,
		onCreate
	});
	return null;
}
function getDefaultCloneParent(workspaceDir) {
	if (!workspaceDir) return "";
	const trimmed = workspaceDir.replace(/[\\/]+$/, "");
	if (!trimmed) return workspaceDir;
	const separatorIndex = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
	if ((separatorIndex === -1 ? trimmed : trimmed.slice(separatorIndex + 1)) !== "workspaces") return workspaceDir;
	const parent = separatorIndex === -1 ? "" : trimmed.slice(0, separatorIndex);
	if (parent === "" && trimmed.startsWith("/")) return "/";
	if (/^[A-Za-z]:$/.test(parent)) return `${parent}${trimmed[separatorIndex]}`;
	return parent;
}
function getCloneDestinationAutoFill({ step, cloneDestination, activeRuntimeEnvironmentId, sshTargetId, workspaceDir, cloneStepAutoFilled }) {
	if (step !== "clone" || cloneStepAutoFilled || cloneDestination) return null;
	if (activeRuntimeEnvironmentId?.trim() || sshTargetId?.trim() || !workspaceDir) return null;
	return { destination: getDefaultCloneParent(workspaceDir) };
}
function useAddRepoCloneFlow({ step, activeRuntimeEnvironmentId, sshTargetId, workspaceDir, fetchWorktrees, onGitRepoReady }) {
	const [cloneUrl, setCloneUrl] = (0, import_react.useState)("");
	const [cloneDestination, setCloneDestination] = (0, import_react.useState)("");
	const [isCloning, setIsCloning] = (0, import_react.useState)(false);
	const [cloneError, setCloneError] = (0, import_react.useState)(null);
	const [cloneProgress, setCloneProgress] = (0, import_react.useState)(null);
	const hostToken = `${activeRuntimeEnvironmentId?.trim() ?? ""}:${sshTargetId?.trim() ?? ""}`;
	const hostTokenRef = (0, import_react.useRef)(hostToken);
	hostTokenRef.current = hostToken;
	const cloneGenRef = (0, import_react.useRef)(0);
	const cloneStepAutoFilledRef = (0, import_react.useRef)(false);
	(0, import_react.useEffect)(() => {
		if (!isCloning) return;
		return window.api.repos.onCloneProgress(setCloneProgress);
	}, [isCloning]);
	const cloneDestinationAutoFill = getCloneDestinationAutoFill({
		step,
		cloneDestination,
		activeRuntimeEnvironmentId,
		sshTargetId,
		workspaceDir,
		cloneStepAutoFilled: cloneStepAutoFilledRef.current
	});
	if (step !== "clone") cloneStepAutoFilledRef.current = false;
	else if (cloneDestinationAutoFill) {
		cloneStepAutoFilledRef.current = true;
		setCloneDestination(cloneDestinationAutoFill.destination);
	}
	return {
		cloneUrl,
		cloneDestination,
		cloneError,
		cloneProgress,
		isCloning,
		setCloneUrl,
		setCloneDestination,
		setCloneError,
		resetCloneFlow: (0, import_react.useCallback)(() => {
			cloneGenRef.current++;
			setCloneUrl("");
			setCloneDestination("");
			setIsCloning(false);
			setCloneError(null);
			setCloneProgress(null);
		}, []),
		handlePickDestination: (0, import_react.useCallback)(async () => {
			if (activeRuntimeEnvironmentId?.trim() || sshTargetId?.trim()) {
				toast.error(translate("auto.components.sidebar.useAddRepoCloneFlow.0dc4d1b657", "Enter a host path for the clone destination."));
				return;
			}
			const gen = cloneGenRef.current;
			const dir = await window.api.repos.pickDirectory();
			if (dir && gen === cloneGenRef.current) {
				setCloneDestination(dir);
				setCloneError(null);
			}
		}, [activeRuntimeEnvironmentId, sshTargetId]),
		handleClone: (0, import_react.useCallback)(async () => {
			const trimmedUrl = cloneUrl.trim();
			if (!trimmedUrl || !cloneDestination.trim()) return;
			const requestHostToken = hostTokenRef.current;
			const gen = ++cloneGenRef.current;
			setIsCloning(true);
			setCloneError(null);
			setCloneProgress(null);
			try {
				const target = activeRuntimeEnvironmentId?.trim() ? {
					kind: "environment",
					environmentId: activeRuntimeEnvironmentId.trim()
				} : getActiveRuntimeTarget({
					...useAppStore.getState().settings,
					activeRuntimeEnvironmentId: null
				});
				const repo = sshTargetId?.trim() ? await window.api.repos.cloneRemote({
					connectionId: sshTargetId.trim(),
					url: trimmedUrl,
					destination: cloneDestination.trim()
				}) : target.kind === "environment" ? (await callRuntimeRpc(target, "repo.clone", {
					url: trimmedUrl,
					destination: cloneDestination.trim()
				}, { timeoutMs: 10 * 6e4 })).repo : await window.api.repos.clone({
					url: trimmedUrl,
					destination: cloneDestination.trim()
				});
				if (gen !== cloneGenRef.current || requestHostToken !== hostTokenRef.current) return;
				const { repo: ownedRepo } = upsertAddedRepoWithProjectHostSetup(repo, {
					runtimeEnvironmentId: activeRuntimeEnvironmentId,
					sshConnectionId: sshTargetId
				});
				toast.success(translate("auto.components.sidebar.useAddRepoCloneFlow.4d0013cc93", "Repository cloned"), { description: ownedRepo.displayName });
				const ownerOptions = worktreeRefreshOptions(activeRuntimeEnvironmentId, sshTargetId);
				await fetchWorktrees(ownedRepo.id, ownerOptions);
				if (gen !== cloneGenRef.current || requestHostToken !== hostTokenRef.current) return;
				await onGitRepoReady(ownedRepo.id, "clone_url", ownerOptions.executionHostId);
			} catch (err) {
				if (gen !== cloneGenRef.current || requestHostToken !== hostTokenRef.current) return;
				setCloneError(extractIpcErrorMessage(err, String(err)));
			} finally {
				if (gen === cloneGenRef.current && requestHostToken === hostTokenRef.current) setIsCloning(false);
			}
		}, [
			activeRuntimeEnvironmentId,
			cloneUrl,
			cloneDestination,
			fetchWorktrees,
			onGitRepoReady,
			sshTargetId
		])
	};
}
function defaultProjectGroupNameForPath(path) {
	return path.replace(/[\\/]+$/g, "").split(/[\\/]/).findLast(Boolean) ?? path;
}
function createNestedRepoScanId() {
	return `nested-repo-scan-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}
function useAddRepoLocalFolderFlow({ isOpen, droppedLocalPath, activeRuntimeEnvironmentId, addRepoPath, closeModal, fetchWorktrees, scanNestedRepos, setActiveNestedScanId, setNestedScanInProgress, showNestedRepoReview, onGitRepoReady, setIsAdding, setAddProjectBusyLabel }) {
	const localAddGenRef = (0, import_react.useRef)(0);
	const droppedLocalPathHandledRef = (0, import_react.useRef)(null);
	const resetLocalFolderFlow = (0, import_react.useCallback)(() => {
		localAddGenRef.current++;
		droppedLocalPathHandledRef.current = null;
	}, []);
	const clearNestedScanState = (0, import_react.useCallback)(() => {
		setNestedScanInProgress(false);
		setActiveNestedScanId(null);
	}, [setActiveNestedScanId, setNestedScanInProgress]);
	const addLocalPathForGeneration = (0, import_react.useCallback)(async (path, source, gen, mode = "single") => {
		if (activeRuntimeEnvironmentId?.trim()) {
			toast.error(translate("auto.components.sidebar.useAddRepoLocalFolderFlow.7ab10e4974", "Use a host path to add projects from a remote host."));
			closeModal();
			return { status: "paused" };
		}
		setAddProjectBusyLabel("Scanning for repositories...");
		try {
			const attemptId = createNestedRepoTelemetryAttemptId();
			const scanId = createNestedRepoScanId();
			setActiveNestedScanId(scanId, activeRuntimeEnvironmentId ?? null);
			setNestedScanInProgress(true);
			const scan = await scanNestedRepos(path, void 0, {
				scanId,
				runtimeEnvironmentId: activeRuntimeEnvironmentId ?? null,
				onProgress: (progressScan) => {
					if (gen !== localAddGenRef.current || mode === "batch" || progressScan.selectedPathKind !== "non_git_folder" || progressScan.repos.length === 0) return;
					showNestedRepoReview({
						scan: progressScan,
						selectedPath: path,
						connectionId: null,
						attemptId,
						runtimeKind: "local",
						inProgress: true,
						scanId,
						runtimeEnvironmentId: activeRuntimeEnvironmentId
					});
				}
			});
			if (gen !== localAddGenRef.current) return { status: "cancelled" };
			clearNestedScanState();
			track("add_repo_nested_scan_result", buildNestedRepoScanTelemetry({
				attemptId,
				surface: "sidebar",
				runtimeKind: "local",
				scan
			}));
			if (scan?.selectedPathKind === "non_git_folder" && mode === "batch") return { status: "skipped" };
			if (scan?.selectedPathKind === "non_git_folder" && scan.repos.length > 0) {
				showNestedRepoReview({
					scan,
					selectedPath: path,
					connectionId: null,
					attemptId,
					runtimeKind: "local",
					inProgress: false,
					scanId,
					runtimeEnvironmentId: activeRuntimeEnvironmentId
				});
				return { status: "paused" };
			}
			setAddProjectBusyLabel("Opening project...");
			const repo = await addRepoPath(path, void 0, { runtimeEnvironmentId: activeRuntimeEnvironmentId ?? null });
			if (gen !== localAddGenRef.current) return { status: "cancelled" };
			if (!repo) return { status: "paused" };
			if (isGitRepoKind(repo)) {
				const ownerOptions = worktreeRefreshOptions(activeRuntimeEnvironmentId ?? null);
				await fetchWorktrees(repo.id, ownerOptions);
				if (gen !== localAddGenRef.current) return { status: "cancelled" };
				if (mode === "batch") return {
					status: "completed",
					repo
				};
				await onGitRepoReady(repo.id, source, ownerOptions.executionHostId);
			} else closeModal();
			return {
				status: "completed",
				repo
			};
		} finally {
			if (gen === localAddGenRef.current) clearNestedScanState();
		}
	}, [
		activeRuntimeEnvironmentId,
		addRepoPath,
		clearNestedScanState,
		closeModal,
		fetchWorktrees,
		onGitRepoReady,
		scanNestedRepos,
		setActiveNestedScanId,
		setAddProjectBusyLabel,
		setNestedScanInProgress,
		showNestedRepoReview
	]);
	const handleAddLocalPath = (0, import_react.useCallback)(async (path, source, mode = "single") => {
		const gen = ++localAddGenRef.current;
		setIsAdding(true);
		try {
			return await addLocalPathForGeneration(path, source, gen, mode);
		} finally {
			if (gen === localAddGenRef.current) {
				clearNestedScanState();
				setIsAdding(false);
				setAddProjectBusyLabel(null);
			}
		}
	}, [
		addLocalPathForGeneration,
		clearNestedScanState,
		setAddProjectBusyLabel,
		setIsAdding
	]);
	const handleAddLocalPaths = (0, import_react.useCallback)(async (paths, source, gen) => {
		const gitRepoIds = [];
		const shouldDeferGitRepoReady = paths.length > 1;
		let skippedCount = 0;
		for (const path of paths) {
			const result = await addLocalPathForGeneration(path, source, gen, shouldDeferGitRepoReady ? "batch" : "single");
			if (result.status === "skipped") {
				skippedCount++;
				continue;
			}
			if (result.status !== "completed") return;
			if (isGitRepoKind(result.repo)) gitRepoIds.push(result.repo.id);
		}
		if (gen !== localAddGenRef.current) return;
		if (skippedCount > 0) toast.info(translate("auto.components.sidebar.useAddRepoLocalFolderFlow.skippedBatchFolders", "Some folders were skipped"), { description: translate("auto.components.sidebar.useAddRepoLocalFolderFlow.skippedBatchFoldersDescription", "Add skipped folders individually to review or confirm them.") });
		if (shouldDeferGitRepoReady && gitRepoIds.length > 0) await onGitRepoReady(gitRepoIds[0], source, worktreeRefreshOptions(activeRuntimeEnvironmentId ?? null).executionHostId);
	}, [
		activeRuntimeEnvironmentId,
		addLocalPathForGeneration,
		onGitRepoReady
	]);
	(0, import_react.useEffect)(() => {
		if (!isOpen || !droppedLocalPath) return;
		if (droppedLocalPathHandledRef.current === droppedLocalPath) return;
		droppedLocalPathHandledRef.current = droppedLocalPath;
		handleAddLocalPath(droppedLocalPath, "local_folder_picker");
	}, [
		droppedLocalPath,
		handleAddLocalPath,
		isOpen
	]);
	return {
		handleBrowse: (0, import_react.useCallback)(async () => {
			const gen = ++localAddGenRef.current;
			setIsAdding(true);
			setAddProjectBusyLabel("Choose a folder...");
			try {
				const paths = await window.api.repos.pickFolders();
				if (paths.length === 0 || gen !== localAddGenRef.current) return;
				await handleAddLocalPaths(paths, "local_folder_picker", gen);
			} finally {
				if (gen === localAddGenRef.current) {
					clearNestedScanState();
					setIsAdding(false);
					setAddProjectBusyLabel(null);
				}
			}
		}, [
			clearNestedScanState,
			handleAddLocalPaths,
			setAddProjectBusyLabel,
			setIsAdding
		]),
		resetLocalFolderFlow
	};
}
function useAddRepoServerPathFlow({ addRepoPath, activeRuntimeEnvironmentId, closeModal, fetchWorktrees, getNestedRepoRuntimeKind, scanNestedRepos, setActiveNestedScanId, setNestedScanInProgress, showNestedRepoReview, onGitRepoReady, setAddProjectBusyLabel }) {
	const [serverPath, setServerPath] = (0, import_react.useState)("");
	const [isAddingServerPath, setIsAddingServerPath] = (0, import_react.useState)(false);
	const serverAddGenRef = (0, import_react.useRef)(0);
	return {
		serverPath,
		isAddingServerPath,
		setServerPath,
		resetServerPathFlow: (0, import_react.useCallback)(() => {
			serverAddGenRef.current++;
			setServerPath("");
			setIsAddingServerPath(false);
		}, []),
		handleAddServerPath: (0, import_react.useCallback)(async (kind) => {
			const path = serverPath.trim();
			if (!path) return;
			const gen = ++serverAddGenRef.current;
			setIsAddingServerPath(true);
			setAddProjectBusyLabel(kind === "git" ? "Scanning for repositories..." : "Opening folder...");
			try {
				if (kind === "git") {
					const attemptId = createNestedRepoTelemetryAttemptId();
					const runtimeKind = getNestedRepoRuntimeKind(null);
					const scanId = runtimeKind !== "runtime" ? createNestedRepoScanId() : null;
					if (scanId) {
						setActiveNestedScanId(scanId, activeRuntimeEnvironmentId);
						setNestedScanInProgress(true);
					}
					const scan = await scanNestedRepos(path, void 0, {
						runtimeEnvironmentId: activeRuntimeEnvironmentId,
						...scanId ? {
							scanId,
							onProgress: (progressScan) => {
								if (gen !== serverAddGenRef.current || progressScan.selectedPathKind !== "non_git_folder" || progressScan.repos.length === 0) return;
								showNestedRepoReview({
									scan: progressScan,
									selectedPath: path,
									connectionId: null,
									attemptId,
									runtimeKind,
									inProgress: true,
									scanId,
									runtimeEnvironmentId: activeRuntimeEnvironmentId
								});
							}
						} : {}
					});
					if (gen !== serverAddGenRef.current) return;
					setNestedScanInProgress(false);
					setActiveNestedScanId(null);
					track("add_repo_nested_scan_result", buildNestedRepoScanTelemetry({
						attemptId,
						surface: "sidebar",
						runtimeKind,
						scan
					}));
					if (scan?.selectedPathKind === "non_git_folder" && scan.repos.length > 0) {
						showNestedRepoReview({
							scan,
							selectedPath: path,
							connectionId: null,
							attemptId,
							runtimeKind,
							inProgress: false,
							scanId,
							runtimeEnvironmentId: activeRuntimeEnvironmentId
						});
						return;
					}
				}
				setAddProjectBusyLabel(kind === "git" ? "Opening project..." : "Opening folder...");
				const repo = await addRepoPath(path, kind, { runtimeEnvironmentId: activeRuntimeEnvironmentId });
				if (gen !== serverAddGenRef.current) return;
				if (repo && isGitRepoKind(repo)) {
					const ownerOptions = worktreeRefreshOptions(activeRuntimeEnvironmentId ?? null);
					await fetchWorktrees(repo.id, ownerOptions);
					if (gen !== serverAddGenRef.current) return;
					await onGitRepoReady(repo.id, "runtime_server_path", ownerOptions.executionHostId);
				} else if (repo) {
					await markOnboardingProjectAdded("addedFolder");
					closeModal();
				}
			} finally {
				if (gen === serverAddGenRef.current) {
					setNestedScanInProgress(false);
					setActiveNestedScanId(null);
					setIsAddingServerPath(false);
					setAddProjectBusyLabel(null);
				}
			}
		}, [
			addRepoPath,
			activeRuntimeEnvironmentId,
			closeModal,
			fetchWorktrees,
			getNestedRepoRuntimeKind,
			onGitRepoReady,
			scanNestedRepos,
			serverPath,
			setActiveNestedScanId,
			setAddProjectBusyLabel,
			setNestedScanInProgress,
			showNestedRepoReview
		])
	};
}
function canSelectAddRepoHost(host) {
	return host.health === "local" || host.health === "available";
}
function canConnectAddRepoHost(host) {
	return host.kind === "ssh" && (host.health === "disconnected" || host.health === "error" || host.health === "connecting");
}
function useAddRepoHostSelection({ isOpen, setStep }) {
	const settings = useAppStore((s) => s.settings);
	const setSshConnectionState = useAppStore((s) => s.setSshConnectionState);
	const sshConnectionStates = useAppStore((s) => s.sshConnectionStates);
	const runtimeEnvironments = useAppStore((s) => s.runtimeEnvironments);
	const { hostOptions } = useSidebarHostScopeOptions();
	const isWebClient = isWebClientLocation();
	const ephemeralRuntimeEnvironmentIds = (0, import_react.useMemo)(() => new Set(runtimeEnvironments.filter(isEphemeralVmRuntimeEnvironment).map((environment) => environment.id)), [runtimeEnvironments]);
	const selectableHostOptions = (0, import_react.useMemo)(() => hostOptions.filter((host) => {
		const parsed = parseExecutionHostId(host.id);
		return !(isWebClient && parsed?.kind === "local") && (parsed?.kind !== "runtime" || !ephemeralRuntimeEnvironmentIds.has(parsed.environmentId));
	}), [
		ephemeralRuntimeEnvironmentIds,
		hostOptions,
		isWebClient
	]);
	const [selectedAddProjectHostId, setSelectedAddProjectHostId] = (0, import_react.useState)(LOCAL_EXECUTION_HOST_ID);
	const [hostSelectorOpen, setHostSelectorOpen] = (0, import_react.useState)(false);
	const previousOpenRef = (0, import_react.useRef)(false);
	const pairedWebRuntimeHost = isWebClient ? selectableHostOptions.find((host) => host.kind === "runtime" && canSelectAddRepoHost(host)) : void 0;
	const selectedHostId = (selectableHostOptions.find((host) => host.id === selectedAddProjectHostId && canSelectAddRepoHost(host)) ?? pairedWebRuntimeHost ?? selectableHostOptions.find((host) => host.id === "local" && canSelectAddRepoHost(host)) ?? selectableHostOptions.find((host) => canSelectAddRepoHost(host)))?.id ?? (isWebClient ? null : "local");
	const selectedParsedHost = parseExecutionHostId(selectedHostId);
	const selectedSshTargetId = selectedParsedHost?.kind === "ssh" ? selectedParsedHost.targetId : null;
	(0, import_react.useEffect)(() => {
		if (isOpen && !previousOpenRef.current) {
			const focusedHostId = getSettingsFocusedExecutionHostId(settings);
			const nextHostId = selectableHostOptions.some((host) => host.id === focusedHostId && canSelectAddRepoHost(host)) ? focusedHostId : pairedWebRuntimeHost?.id ?? (isWebClient ? null : "local");
			if (nextHostId) setSelectedAddProjectHostId(nextHostId);
		}
		if (!isOpen) setHostSelectorOpen(false);
		previousOpenRef.current = isOpen;
	}, [
		isOpen,
		isWebClient,
		pairedWebRuntimeHost?.id,
		selectableHostOptions,
		settings
	]);
	return {
		hostOptions: selectableHostOptions,
		selectedHostId,
		selectedParsedHost,
		selectedSshTargetId,
		hostSelectorOpen,
		setHostSelectorOpen,
		handleSelectAddProjectHost: (0, import_react.useCallback)(async (hostId) => {
			const host = selectableHostOptions.find((candidate) => candidate.id === hostId);
			if (!host || !canSelectAddRepoHost(host)) return;
			setSelectedAddProjectHostId(hostId);
			setStep("add");
		}, [selectableHostOptions, setStep]),
		handleConnectAddProjectHost: (0, import_react.useCallback)(async (hostId) => {
			const host = selectableHostOptions.find((candidate) => candidate.id === hostId);
			const parsed = parseExecutionHostId(hostId);
			if (!host || parsed?.kind !== "ssh") return;
			const previousState = sshConnectionStates.get(parsed.targetId);
			setSshConnectionState(parsed.targetId, {
				targetId: parsed.targetId,
				status: "connecting",
				error: null,
				reconnectAttempt: previousState?.reconnectAttempt ?? 0,
				remotePlatform: previousState?.remotePlatform
			});
			try {
				const state = await window.api.ssh.connect({ targetId: parsed.targetId }) ?? await window.api.ssh.getState({ targetId: parsed.targetId });
				if (state) setSshConnectionState(parsed.targetId, state);
				if (state?.status !== "connected") return;
				setSelectedAddProjectHostId(hostId);
				setStep("add");
				setHostSelectorOpen(false);
			} catch (err) {
				setSshConnectionState(parsed.targetId, previousState ?? {
					targetId: parsed.targetId,
					status: "disconnected",
					error: err instanceof Error ? err.message : translate("auto.components.sidebar.useAddRepoHostSelection.connectionFailed", "SSH connection failed."),
					reconnectAttempt: 0
				});
				toast.error(err instanceof Error ? err.message : translate("auto.components.sidebar.useAddRepoHostSelection.connectionFailed", "SSH connection failed."));
			}
		}, [
			selectableHostOptions,
			setSshConnectionState,
			setStep,
			sshConnectionStates
		])
	};
}
var MAX_REPORTED_WORKSPACES = 50;
function countWorkspaces(count) {
	return Math.min(MAX_REPORTED_WORKSPACES, Math.max(0, count));
}
function branchDisplayName(worktree) {
	return worktree.branch.replace(/^refs\/heads\//, "");
}
function pathBasename(pathValue) {
	return pathValue.replace(/[\\/]+$/, "").split(/[\\/]/).findLast(Boolean) ?? "";
}
function isCustomDisplayName(worktree) {
	const branchName = branchDisplayName(worktree);
	const pathName = pathBasename(worktree.path);
	return Boolean(worktree.displayName && worktree.displayName !== branchName && worktree.displayName !== pathName);
}
function buildAddRepoExistingWorkspacesTelemetry(source, worktrees) {
	if (!source || worktrees.length === 0) return null;
	const mainWorkspaceCount = worktrees.filter((worktree) => worktree.isMainWorktree).length;
	const branchNamedWorkspaceCount = worktrees.filter((worktree) => Boolean(branchDisplayName(worktree))).length;
	const sparseWorkspaceCount = worktrees.filter((worktree) => worktree.isSparse === true).length;
	return {
		source,
		existing_workspace_count: countWorkspaces(worktrees.length),
		existing_linked_workspace_count: countWorkspaces(worktrees.length - mainWorkspaceCount),
		main_workspace_count: countWorkspaces(mainWorkspaceCount),
		branch_named_workspace_count: countWorkspaces(branchNamedWorkspaceCount),
		detached_workspace_count: countWorkspaces(worktrees.length - branchNamedWorkspaceCount),
		custom_named_workspace_count: countWorkspaces(worktrees.filter(isCustomDisplayName).length),
		sparse_workspace_count: countWorkspaces(sparseWorkspaceCount)
	};
}
function shouldTrackAddRepoExistingWorkspacesDetected(payload) {
	if (!payload || payload.existing_linked_workspace_count === 0) return false;
	return payload.source === "local_folder_picker" || payload.source === "runtime_server_path" || payload.source === "ssh_remote_path";
}
function useCompleteGitRepoAdd({ closeModal, setHideDefaultBranchWorkspace, finishProjectAdd }) {
	const detectedTelemetryTrackedRef = (0, import_react.useRef)(/* @__PURE__ */ new Set());
	return (0, import_react.useCallback)(async (repoId, source, executionHostId) => {
		const existingWorkspaceTelemetry = buildAddRepoExistingWorkspacesTelemetry(source, [...(useAppStore.getState().worktreesByRepo[repoId] ?? []).filter((worktree) => executionHostId === void 0 || worktree.hostId === executionHostId || !worktree.hostId && executionHostId === "local")].sort((a, b) => {
			if (a.lastActivityAt !== b.lastActivityAt) return b.lastActivityAt - a.lastActivityAt;
			return compareWorktreeDisplayName(a, b);
		}));
		if (existingWorkspaceTelemetry && shouldTrackAddRepoExistingWorkspacesDetected(existingWorkspaceTelemetry) && !detectedTelemetryTrackedRef.current.has(repoId)) {
			detectedTelemetryTrackedRef.current.add(repoId);
			track("add_repo_existing_workspaces_detected", existingWorkspaceTelemetry);
		}
		if (finishProjectAdd) {
			await finishProjectAdd(repoId, source, executionHostId);
			return;
		}
		await finishProjectAddWithDefaultCheckout({
			repoId,
			source,
			executionHostId,
			closeModal,
			setHideDefaultBranchWorkspace
		});
	}, [
		closeModal,
		finishProjectAdd,
		setHideDefaultBranchWorkspace
	]);
}
var LOCAL_GIT_AVAILABILITY_TIMEOUT_MS = 1500;
var RUNTIME_GIT_AVAILABILITY_TIMEOUT_MS = 3e3;
function withTimeout(promise, timeoutMs) {
	let timeout = null;
	return new Promise((resolve, reject) => {
		timeout = setTimeout(() => reject(/* @__PURE__ */ new Error("Timed out")), timeoutMs);
		promise.then((value) => {
			if (timeout) clearTimeout(timeout);
			resolve(value);
		}, (error) => {
			if (timeout) clearTimeout(timeout);
			reject(error);
		});
	});
}
function useCreateProjectDefaults({ step, activeRuntimeEnvironmentId, sshTargetId, createParent, setCreateParent }) {
	const [createDefaultParent, setCreateDefaultParent] = (0, import_react.useState)("");
	const [createGitAvailability, setCreateGitAvailability] = (0, import_react.useState)("unknown");
	const [createRuntimeParentStatus, setCreateRuntimeParentStatus] = (0, import_react.useState)("idle");
	const createStepAutoFilledRef = (0, import_react.useRef)(false);
	const autoFilledCreateParentRef = (0, import_react.useRef)(null);
	const createParentProvenanceRef = (0, import_react.useRef)(null);
	const createParentTouchedRef = (0, import_react.useRef)(false);
	const createParentDefaultGenRef = (0, import_react.useRef)(0);
	const createGitProbeGenRef = (0, import_react.useRef)(0);
	const activeCreateParentRuntimeEnvironmentId = activeRuntimeEnvironmentId?.trim() || null;
	const activeCreateParentSshTargetId = sshTargetId?.trim() || null;
	const activeCreateParentTargetKey = activeCreateParentRuntimeEnvironmentId ? `runtime:${activeCreateParentRuntimeEnvironmentId}` : activeCreateParentSshTargetId ? `ssh:${activeCreateParentSshTargetId}` : "local";
	const canReplaceCreateParentDefault = (0, import_react.useCallback)((parent) => {
		if (createParentTouchedRef.current) return false;
		const trimmedParent = parent.trim();
		return !trimmedParent || autoFilledCreateParentRef.current?.parent === trimmedParent;
	}, []);
	const resetCreateDefaultState = (0, import_react.useCallback)(() => {
		createParentDefaultGenRef.current++;
		createGitProbeGenRef.current++;
		createStepAutoFilledRef.current = false;
		autoFilledCreateParentRef.current = null;
		createParentProvenanceRef.current = null;
		createParentTouchedRef.current = false;
		setCreateDefaultParent("");
		setCreateGitAvailability("unknown");
		setCreateRuntimeParentStatus("idle");
	}, []);
	const markCreateParentTouched = (0, import_react.useCallback)((value) => {
		autoFilledCreateParentRef.current = null;
		createParentProvenanceRef.current = {
			parent: (value ?? createParent).trim(),
			targetKey: activeCreateParentTargetKey
		};
		createParentTouchedRef.current = true;
	}, [activeCreateParentTargetKey, createParent]);
	const createParentDefaultPending = step === "create" && !createParentTouchedRef.current && Boolean(createParent.trim()) && autoFilledCreateParentRef.current?.parent === createParent.trim() && autoFilledCreateParentRef.current.targetKey !== activeCreateParentTargetKey;
	const createParentTargetPending = step === "create" && Boolean(createParent.trim()) && createParentProvenanceRef.current?.parent === createParent.trim() && createParentProvenanceRef.current.targetKey !== activeCreateParentTargetKey;
	const createParentPending = createParentDefaultPending || createParentTargetPending;
	(0, import_react.useEffect)(() => {
		if (step !== "create") return;
		if (activeCreateParentRuntimeEnvironmentId || activeCreateParentSshTargetId) return;
		const gen = ++createParentDefaultGenRef.current;
		if (!canReplaceCreateParentDefault(createParent)) return;
		if (createParent.trim() && autoFilledCreateParentRef.current?.targetKey !== "local" && autoFilledCreateParentRef.current?.parent === createParent.trim()) {
			setCreateDefaultParent("");
			setCreateParent("");
			return;
		}
		if (autoFilledCreateParentRef.current?.targetKey === "local" && autoFilledCreateParentRef.current.parent === createParent.trim()) return;
		setCreateDefaultParent("");
		window.api.repos.getDefaultCreateProjectParent().then((parent) => {
			if (gen !== createParentDefaultGenRef.current || !canReplaceCreateParentDefault(createParent) || !parent) return;
			setCreateDefaultParent(parent);
			createStepAutoFilledRef.current = true;
			autoFilledCreateParentRef.current = {
				parent,
				targetKey: "local"
			};
			createParentProvenanceRef.current = {
				parent,
				targetKey: "local"
			};
			setCreateParent(parent);
		}).catch(() => {});
	}, [
		activeRuntimeEnvironmentId,
		activeCreateParentRuntimeEnvironmentId,
		activeCreateParentSshTargetId,
		canReplaceCreateParentDefault,
		createParent,
		setCreateParent,
		step
	]);
	(0, import_react.useEffect)(() => {
		if (step !== "create") return;
		const runtimeEnvironmentId = activeCreateParentRuntimeEnvironmentId;
		if (!runtimeEnvironmentId || activeCreateParentSshTargetId) {
			setCreateRuntimeParentStatus("idle");
			return;
		}
		if (!canReplaceCreateParentDefault(createParent)) {
			setCreateRuntimeParentStatus("idle");
			return;
		}
		if (createParent.trim() && autoFilledCreateParentRef.current?.targetKey !== `runtime:${runtimeEnvironmentId}` && autoFilledCreateParentRef.current?.parent === createParent.trim()) {
			setCreateDefaultParent("");
			setCreateRuntimeParentStatus("checking");
			setCreateParent("");
			return;
		}
		if (autoFilledCreateParentRef.current?.targetKey === `runtime:${runtimeEnvironmentId}` && autoFilledCreateParentRef.current.parent === createParent.trim()) {
			setCreateRuntimeParentStatus("idle");
			return;
		}
		setCreateDefaultParent("");
		const gen = ++createParentDefaultGenRef.current;
		setCreateRuntimeParentStatus("checking");
		withTimeout(browseRuntimeServerDirectory(runtimeEnvironmentId, "~"), RUNTIME_GIT_AVAILABILITY_TIMEOUT_MS).then((result) => {
			if (gen !== createParentDefaultGenRef.current || !canReplaceCreateParentDefault(createParent)) return;
			const parent = getDefaultCreateProjectParent(result.resolvedPath);
			createStepAutoFilledRef.current = true;
			autoFilledCreateParentRef.current = {
				parent,
				targetKey: `runtime:${runtimeEnvironmentId}`
			};
			createParentProvenanceRef.current = {
				parent,
				targetKey: `runtime:${runtimeEnvironmentId}`
			};
			setCreateDefaultParent(parent);
			setCreateParent(parent);
			setCreateRuntimeParentStatus("idle");
		}).catch(() => {
			if (gen !== createParentDefaultGenRef.current) return;
			setCreateRuntimeParentStatus("failed");
		});
	}, [
		activeRuntimeEnvironmentId,
		activeCreateParentRuntimeEnvironmentId,
		activeCreateParentSshTargetId,
		canReplaceCreateParentDefault,
		createParent,
		setCreateParent,
		step
	]);
	(0, import_react.useEffect)(() => {
		if (step !== "create") return;
		const runtimeEnvironmentId = activeRuntimeEnvironmentId?.trim();
		const gen = ++createGitProbeGenRef.current;
		if (activeCreateParentSshTargetId) {
			setCreateGitAvailability("unknown");
			return;
		}
		setCreateGitAvailability("checking");
		withTimeout(runtimeEnvironmentId ? callRuntimeRpc({
			kind: "environment",
			environmentId: runtimeEnvironmentId
		}, "repo.gitAvailable", void 0, { timeoutMs: RUNTIME_GIT_AVAILABILITY_TIMEOUT_MS }).then((result) => result.available) : window.api.repos.isGitAvailable(), runtimeEnvironmentId ? RUNTIME_GIT_AVAILABILITY_TIMEOUT_MS : LOCAL_GIT_AVAILABILITY_TIMEOUT_MS).then((available) => {
			if (gen !== createGitProbeGenRef.current) return;
			setCreateGitAvailability(available ? "available" : "unavailable");
		}).catch(() => {
			if (gen !== createGitProbeGenRef.current) return;
			setCreateGitAvailability("unknown");
		});
	}, [
		activeRuntimeEnvironmentId,
		activeCreateParentSshTargetId,
		step
	]);
	return {
		createDefaultParent,
		createGitAvailability,
		createRuntimeParentStatus,
		createParentDefaultPending: createParentPending,
		resetCreateDefaultState,
		markCreateParentTouched
	};
}
function useAddRepoHostChangeReset({ isOpen, selectedHostId, onResetClosed, onResetHostScopedState }) {
	const previousSelectedHostIdRef = (0, import_react.useRef)(selectedHostId);
	(0, import_react.useEffect)(() => {
		if (!isOpen) {
			previousSelectedHostIdRef.current = selectedHostId;
			onResetClosed();
		}
	}, [
		isOpen,
		onResetClosed,
		selectedHostId
	]);
	(0, import_react.useEffect)(() => {
		if (!isOpen || previousSelectedHostIdRef.current === selectedHostId) return;
		previousSelectedHostIdRef.current = selectedHostId;
		onResetHostScopedState();
	}, [
		isOpen,
		onResetHostScopedState,
		selectedHostId
	]);
}
function AddRepoStepIndicator({ step, isAdding, onBack }) {
	if (!(step === "clone" || step === "remote" || step === "server-path" || step === "create" || step === "nested")) return null;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "-mt-1 flex min-h-5 items-center",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
			className: "inline-flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground transition-colors cursor-pointer disabled:cursor-default disabled:opacity-40",
			disabled: step === "nested" && isAdding,
			onClick: onBack,
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowLeft, { className: "size-3" }), translate("auto.components.sidebar.AddRepoStepIndicator.3bb655c117", "Back")]
		})
	});
}
function AddRepoDialogChrome({ children, isAdding, isOpen, onBack, onCloseAutoFocus, onOpenChange, step }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
		open: isOpen,
		onOpenChange,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogContent, {
			onCloseAutoFocus,
			className: `min-w-0 overflow-hidden sm:max-w-lg [&>*]:min-w-0 ${step === "nested" ? "max-h-[calc(100vh-2rem)] grid-rows-[auto_auto_minmax(0,1fr)]" : ""}`,
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AddRepoStepIndicator, {
				step,
				isAdding,
				onBack
			}), children]
		})
	});
}
function getHostStatusDetail(host) {
	if (host.compatibility?.kind === "blocked") return describeRuntimeCompatBlock(host.compatibility);
	return `${getSidebarHostHealthLabel(host.health)}${host.detail ? ` - ${host.detail}` : ""}`;
}
function AddRepoHostSelector({ hosts, selectedHostId, open, onOpenChange, onSelectHost, onConnectHost, onAddSshHost, onAddRemoteServer }) {
	const [addHostOpen, setAddHostOpen] = (0, import_react.useState)(false);
	const showHostSetupActions = Boolean(onAddSshHost || onAddRemoteServer);
	if (!shouldShowHostScopeControls(hosts) && !showHostSetupActions) return null;
	const selectedHost = hosts.find((host) => host.id === selectedHostId) ?? hosts[0];
	if (!selectedHost) return null;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex items-center gap-2 text-xs",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "font-medium text-muted-foreground",
			children: translate("auto.components.sidebar.AddRepoHostSelector.host", "Host")
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Popover, {
			open,
			onOpenChange,
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(PopoverTrigger, {
				asChild: true,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
					type: "button",
					variant: "ghost",
					role: "combobox",
					"aria-expanded": open,
					className: "h-7 min-w-0 max-w-[18rem] gap-1.5 rounded-md border border-border bg-muted/30 px-2 text-xs font-medium text-foreground hover:bg-accent hover:text-accent-foreground",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "min-w-0 truncate",
							children: selectedHost.label
						}),
						selectedHost.health !== "local" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							title: getHostStatusDetail(selectedHost),
							className: "shrink-0 text-[11px] font-normal text-muted-foreground",
							children: getSidebarHostHealthLabel(selectedHost.health)
						}) : null,
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronsUpDown, { className: "size-3.5 shrink-0 opacity-50" })
					]
				})
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PopoverContent, {
				align: "start",
				className: "w-[min(340px,calc(100vw-1rem))] min-w-[var(--radix-popover-trigger-width)] p-0",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Command, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CommandList, { children: [showHostSetupActions ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Popover, {
					open: addHostOpen,
					onOpenChange: setAddHostOpen,
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(PopoverTrigger, {
						asChild: true,
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CommandItem, {
							value: "Add remote host SSH host Orca server",
							onSelect: () => setAddHostOpen(true),
							className: "items-start gap-2 px-3 py-2 text-xs text-muted-foreground data-[state=open]:bg-accent data-[state=open]:text-accent-foreground",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Plus, { className: "mt-0.5 size-3 shrink-0" }),
								/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
									className: "min-w-0 flex-1",
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
										className: "flex min-w-0 items-center gap-2",
										children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
											className: "truncate font-medium",
											children: translate("auto.components.sidebar.AddRepoHostSelector.addRemoteHost", "Add remote host")
										})
									}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
										className: "mt-0.5 block truncate text-[11px] text-muted-foreground",
										children: translate("auto.components.sidebar.AddRepoHostSelector.addRemoteHostDetail", "SSH host or Orca server")
									})]
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronRight, { className: "mt-0.5 size-3.5 shrink-0" })
							]
						})
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(PopoverContent, {
						align: "start",
						side: "right",
						className: "w-72 p-1",
						sideOffset: 8,
						children: [onAddSshHost ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
							type: "button",
							className: "flex w-full flex-col rounded-sm px-2.5 py-2 text-left hover:bg-accent hover:text-accent-foreground focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50",
							onClick: () => {
								setAddHostOpen(false);
								onOpenChange(false);
								onAddSshHost();
							},
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "text-xs font-medium",
								children: translate("auto.components.sidebar.AddRepoHostSelector.addSshHost", "Add SSH host")
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "mt-0.5 text-[11px] text-muted-foreground",
								children: translate("auto.components.sidebar.AddRepoHostSelector.addSshHostDetail", "Use an existing machine over SSH.")
							})]
						}) : null, onAddRemoteServer ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
							type: "button",
							className: "flex w-full flex-col rounded-sm px-2.5 py-2 text-left hover:bg-accent hover:text-accent-foreground focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50",
							onClick: () => {
								setAddHostOpen(false);
								onOpenChange(false);
								onAddRemoteServer();
							},
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "text-xs font-medium",
								children: translate("auto.components.sidebar.AddRepoHostSelector.addRemoteServer", "Add remote server")
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "mt-0.5 text-[11px] text-muted-foreground",
								children: translate("auto.components.sidebar.AddRepoHostSelector.addRemoteServerDetail", "Pair with Orca running on another computer.")
							})]
						}) : null]
					})]
				}) : null, hosts.map((host) => {
					const selected = host.id === selectedHostId;
					const disabled = !canSelectAddRepoHost(host);
					const canConnect = canConnectAddRepoHost(host);
					const isConnecting = host.health === "connecting";
					return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CommandItem, {
						value: `${host.label} ${host.detail}`,
						disabled: disabled && !canConnect,
						"aria-disabled": disabled,
						onSelect: () => {
							if (disabled) return;
							onSelectHost(host.id);
							onOpenChange(false);
						},
						className: cn("items-start gap-2 px-3 py-2 text-xs", disabled && !canConnect && "cursor-not-allowed opacity-55"),
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, { className: cn("mt-0.5 size-3 text-muted-foreground", selected ? "opacity-70" : "opacity-0") }),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
								className: "min-w-0 flex-1",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "flex min-w-0 items-center gap-2",
									children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
										className: "truncate font-medium",
										children: host.label
									})
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "mt-0.5 block truncate text-[11px] text-muted-foreground",
									children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
										className: "min-w-0 flex-1 truncate",
										children: getHostStatusDetail(host)
									})
								})]
							}),
							canConnect ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
								type: "button",
								variant: "link",
								size: "xs",
								className: "ml-2 h-auto w-[5.75rem] shrink-0 justify-end gap-1 self-center px-0 py-0 text-[11px] font-normal text-muted-foreground hover:text-foreground hover:no-underline",
								disabled: isConnecting,
								onClick: (event) => {
									event.preventDefault();
									event.stopPropagation();
									onConnectHost?.(host.id);
								},
								children: [isConnecting ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-3 animate-spin" }) : null, isConnecting ? translate("auto.components.sidebar.AddRepoHostSelector.connecting", "Connecting") : translate("auto.components.sidebar.AddRepoHostSelector.connect", "Connect")]
							}) : null
						]
					}, host.id);
				})] }) })
			})]
		})]
	});
}
function AddRepoHostSelectorSlot({ hostSelection }) {
	const [addRemoteHostMode, setAddRemoteHostMode] = (0, import_react.useState)(null);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(AddRepoHostSelector, {
		hosts: hostSelection.hostOptions,
		selectedHostId: hostSelection.selectedHostId,
		open: hostSelection.hostSelectorOpen,
		onOpenChange: hostSelection.setHostSelectorOpen,
		onSelectHost: (hostId) => void hostSelection.handleSelectAddProjectHost(hostId),
		onConnectHost: (hostId) => void hostSelection.handleConnectAddProjectHost(hostId),
		onAddSshHost: () => setAddRemoteHostMode("ssh"),
		onAddRemoteServer: () => setAddRemoteHostMode("server")
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AddRemoteHostDialog, {
		mode: addRemoteHostMode,
		onOpenChange: setAddRemoteHostMode
	})] });
}
function getSelectedNestedRepoPathsInScanOrder(scan, selectedPaths) {
	return scan.repos.filter((repo) => selectedPaths.has(repo.path)).map((repo) => repo.path);
}
function trackNestedFolderOpen(args) {
	if (!args.attemptId) return;
	track("add_repo_nested_import_action", buildNestedRepoImportActionTelemetry({
		attemptId: args.attemptId,
		surface: "sidebar",
		runtimeKind: args.runtimeKind ?? args.getRuntimeKind(args.connectionId),
		action: "open_as_folder",
		foundCount: args.scan.repos.length,
		selectedCount: args.selectedCount
	}));
}
async function completeNestedFolderOpen(args) {
	trackNestedFolderOpen(args);
	args.setIsAdding(true);
	try {
		const state = useAppStore.getState();
		if (args.connectionId) {
			args.closeModal();
			state.openModal("confirm-non-git-folder", {
				folderPath: args.scan.selectedPath,
				connectionId: args.connectionId,
				runtimeEnvironmentId: args.owner
			});
			return;
		}
		const repo = await state.addNonGitFolder(args.scan.selectedPath, { runtimeEnvironmentId: args.owner ?? null });
		if (args.generation !== args.currentGeneration()) return;
		if (repo) args.closeModal();
	} catch (err) {
		if (args.generation === args.currentGeneration()) toast.error(err instanceof Error ? err.message : String(err));
	} finally {
		if (args.generation === args.currentGeneration()) args.setIsAdding(false);
	}
}
function useAddRepoNestedImportFlow({ nestedAttemptId, nestedScan, nestedSelectedPaths, nestedRuntimeKind, nestedConnectionId, nestedGroupName, nestedImportScanId, nestedRuntimeEnvironmentId, activeRuntimeEnvironmentId, closeModal, fetchWorktrees, importNestedRepos, getNestedRepoRuntimeKind, onGitRepoReady, setIsAdding }) {
	const nestedImportGenRef = (0, import_react.useRef)(0);
	const resetNestedImportFlow = (0, import_react.useCallback)(() => {
		nestedImportGenRef.current++;
	}, []);
	const trackNestedBackAction = (0, import_react.useCallback)(() => {
		if (!nestedScan || !nestedAttemptId) return;
		track("add_repo_nested_import_action", buildNestedRepoImportActionTelemetry({
			attemptId: nestedAttemptId,
			surface: "sidebar",
			runtimeKind: nestedRuntimeKind ?? getNestedRepoRuntimeKind(nestedConnectionId),
			action: "back",
			foundCount: nestedScan.repos.length,
			selectedCount: nestedSelectedPaths.size
		}));
	}, [
		getNestedRepoRuntimeKind,
		nestedAttemptId,
		nestedConnectionId,
		nestedRuntimeKind,
		nestedScan,
		nestedSelectedPaths.size
	]);
	return {
		handleImportNestedRepos: (0, import_react.useCallback)(async (mode) => {
			const attemptId = nestedAttemptId;
			if (!nestedScan || !attemptId || !shouldEmitNestedRepoImportSubmitTelemetry({
				attemptId,
				selectedCount: nestedSelectedPaths.size
			})) return;
			const foundCount = nestedScan.repos.length;
			const selectedCount = nestedSelectedPaths.size;
			const selectedProjectPaths = getSelectedNestedRepoPathsInScanOrder(nestedScan, nestedSelectedPaths);
			const runtimeKind = nestedRuntimeKind ?? getNestedRepoRuntimeKind(nestedConnectionId);
			const gen = ++nestedImportGenRef.current;
			setIsAdding(true);
			track("add_repo_nested_import_action", buildNestedRepoImportActionTelemetry({
				attemptId,
				surface: "sidebar",
				runtimeKind,
				action: mode === "group" ? "import_group" : "import_separate",
				foundCount,
				selectedCount
			}));
			let resultTracked = false;
			try {
				const result = await importNestedRepos({
					parentPath: nestedScan.selectedPath,
					groupName: nestedGroupName,
					projectPaths: selectedProjectPaths,
					...nestedConnectionId ? { connectionId: nestedConnectionId } : {},
					...nestedImportScanId ? { scanId: nestedImportScanId } : {},
					runtimeEnvironmentId: nestedRuntimeEnvironmentId,
					mode
				});
				track("add_repo_nested_import_result", buildNestedRepoImportResultTelemetry({
					attemptId,
					surface: "sidebar",
					runtimeKind,
					mode,
					foundCount,
					selectedCount,
					result
				}));
				resultTracked = true;
				if (!result) return;
				const importedRepoIds = result.projects.map((entry) => entry.projectId).filter((projectId) => typeof projectId === "string");
				const firstRepoId = importedRepoIds[0];
				if (!firstRepoId) {
					const firstFailure = result.projects.find((entry) => entry.status === "failed")?.error;
					if (gen === nestedImportGenRef.current) toast.error(translate("auto.components.sidebar.useAddRepoNestedImportFlow.1b33c5f090", "No repositories imported"), { description: firstFailure ?? void 0 });
					return;
				}
				const completionOwnerOptions = worktreeRefreshOptions(nestedConnectionId === null ? nestedRuntimeEnvironmentId : void 0, nestedConnectionId);
				for (const projectId of importedRepoIds) await fetchWorktrees(projectId, completionOwnerOptions);
				if (gen !== nestedImportGenRef.current) return;
				if (result.failedCount > 0) toast.warning(translate("auto.components.sidebar.useAddRepoNestedImportFlow.cbfbc7a797", "Some repositories could not be imported"), { description: translate("auto.components.sidebar.useAddRepoNestedImportFlow.680cac2c82", "{{value0}} failed", { value0: result.failedCount }) });
				const repo = useAppStore.getState().repos.find((entry) => entry.id === firstRepoId);
				if (repo) {
					const source = nestedConnectionId ? "ssh_remote_path" : activeRuntimeEnvironmentId?.trim() ? "runtime_server_path" : "local_folder_picker";
					await onGitRepoReady(repo.id, source, completionOwnerOptions.executionHostId);
				}
			} catch (err) {
				if (gen === nestedImportGenRef.current) toast.error(err instanceof Error ? err.message : String(err));
			} finally {
				if (!resultTracked) track("add_repo_nested_import_result", buildNestedRepoImportResultTelemetry({
					attemptId,
					surface: "sidebar",
					runtimeKind,
					mode,
					foundCount,
					selectedCount,
					result: null
				}));
				if (gen === nestedImportGenRef.current) setIsAdding(false);
			}
		}, [
			activeRuntimeEnvironmentId,
			fetchWorktrees,
			importNestedRepos,
			nestedAttemptId,
			nestedConnectionId,
			nestedGroupName,
			nestedImportScanId,
			nestedRuntimeEnvironmentId,
			nestedRuntimeKind,
			nestedScan,
			nestedSelectedPaths,
			getNestedRepoRuntimeKind,
			onGitRepoReady,
			setIsAdding
		]),
		handleOpenNestedRootFolder: (0, import_react.useCallback)(() => {
			if (!nestedScan) return Promise.resolve();
			return completeNestedFolderOpen({
				scan: nestedScan,
				generation: ++nestedImportGenRef.current,
				currentGeneration: () => nestedImportGenRef.current,
				attemptId: nestedAttemptId,
				runtimeKind: nestedRuntimeKind,
				connectionId: nestedConnectionId,
				selectedCount: nestedSelectedPaths.size,
				getRuntimeKind: getNestedRepoRuntimeKind,
				owner: nestedRuntimeEnvironmentId,
				closeModal,
				setIsAdding
			});
		}, [
			closeModal,
			getNestedRepoRuntimeKind,
			nestedAttemptId,
			nestedConnectionId,
			nestedRuntimeKind,
			nestedRuntimeEnvironmentId,
			nestedScan,
			nestedSelectedPaths.size,
			setIsAdding
		]),
		resetNestedImportFlow,
		trackNestedBackAction
	};
}
function useAddRepoNestedReviewState({ activeRuntimeEnvironmentId, cancelNestedRepoScan, setStep }) {
	const [nestedScan, setNestedScan] = (0, import_react.useState)(null);
	const [nestedSelectedPaths, setNestedSelectedPaths] = (0, import_react.useState)(/* @__PURE__ */ new Set());
	const [nestedGroupName, setNestedGroupName] = (0, import_react.useState)("");
	const [nestedConnectionId, setNestedConnectionId] = (0, import_react.useState)(null);
	const [nestedAttemptId, setNestedAttemptId] = (0, import_react.useState)(null);
	const [nestedRuntimeKind, setNestedRuntimeKind] = (0, import_react.useState)(null);
	const [nestedScanInProgress, setNestedScanInProgress] = (0, import_react.useState)(false);
	const [nestedScanId, setNestedScanId] = (0, import_react.useState)(null);
	const [nestedImportScanId, setNestedImportScanId] = (0, import_react.useState)(null);
	const [nestedRuntimeEnvironmentId, setNestedRuntimeEnvironmentId] = (0, import_react.useState)(void 0);
	const nestedScanIdRef = (0, import_react.useRef)(null);
	const nestedScanRuntimeEnvironmentIdRef = (0, import_react.useRef)(void 0);
	const getNestedRepoRuntimeKind = (0, import_react.useCallback)((connectionId) => {
		if (connectionId) return "ssh";
		return activeRuntimeEnvironmentId?.trim() ? "runtime" : "local";
	}, [activeRuntimeEnvironmentId]);
	const showNestedRepoReview = (0, import_react.useCallback)((args) => {
		setNestedScan(args.scan);
		setNestedSelectedPaths(new Set(args.scan.repos.map((repo) => repo.path)));
		setNestedGroupName(defaultProjectGroupNameForPath(args.scan.selectedPath || args.selectedPath));
		setNestedConnectionId(args.connectionId);
		setNestedAttemptId(args.attemptId);
		setNestedRuntimeKind(args.runtimeKind);
		setNestedScanInProgress(args.inProgress);
		setNestedImportScanId(args.scanId);
		setNestedRuntimeEnvironmentId(args.runtimeEnvironmentId ?? null);
		setStep("nested");
	}, [setStep]);
	const setActiveNestedScanId = (0, import_react.useCallback)((scanId, runtimeEnvironmentId) => {
		nestedScanIdRef.current = scanId;
		nestedScanRuntimeEnvironmentIdRef.current = scanId ? runtimeEnvironmentId : void 0;
		setNestedScanId(scanId);
	}, []);
	return {
		nestedScan,
		nestedSelectedPaths,
		nestedGroupName,
		nestedConnectionId,
		nestedAttemptId,
		nestedRuntimeKind,
		nestedScanInProgress,
		nestedScanId,
		nestedImportScanId,
		nestedRuntimeEnvironmentId,
		setNestedSelectedPaths,
		setNestedGroupName,
		setNestedScanInProgress,
		getNestedRepoRuntimeKind,
		showNestedRepoReview,
		setActiveNestedScanId,
		handleStopNestedScan: (0, import_react.useCallback)(() => {
			const scanId = nestedScanIdRef.current;
			if (!scanId) return;
			cancelNestedRepoScan(scanId, { runtimeEnvironmentId: nestedScanRuntimeEnvironmentIdRef.current });
		}, [cancelNestedRepoScan]),
		resetNestedRepoReviewState: (0, import_react.useCallback)(() => {
			const activeNestedScanId = nestedScanIdRef.current;
			if (activeNestedScanId) cancelNestedRepoScan(activeNestedScanId, { runtimeEnvironmentId: nestedScanRuntimeEnvironmentIdRef.current });
			setNestedScan(null);
			setNestedSelectedPaths(/* @__PURE__ */ new Set());
			setNestedGroupName("");
			setNestedConnectionId(null);
			setNestedAttemptId(null);
			setNestedRuntimeKind(null);
			setNestedScanInProgress(false);
			setNestedImportScanId(null);
			setNestedRuntimeEnvironmentId(null);
			setActiveNestedScanId(null);
		}, [cancelNestedRepoScan, setActiveNestedScanId])
	};
}
function useAddRepoRemoteNestedScan({ setActiveNestedScanId, showNestedRepoReview }) {
	return {
		showRemoteNestedRepoReview: (0, import_react.useCallback)((scan, selectedPath, connectionId, attemptId, inProgress, scanId) => {
			setActiveNestedScanId(inProgress ? scanId : null, null);
			showNestedRepoReview({
				scan,
				selectedPath,
				connectionId,
				attemptId,
				runtimeKind: "ssh",
				inProgress,
				scanId,
				runtimeEnvironmentId: null
			});
		}, [setActiveNestedScanId, showNestedRepoReview]),
		trackRemoteNestedScanResult: (0, import_react.useCallback)((scan, attemptId) => {
			track("add_repo_nested_scan_result", buildNestedRepoScanTelemetry({
				attemptId,
				surface: "sidebar",
				runtimeKind: "ssh",
				scan
			}));
		}, [])
	};
}
function useAddRepoNestedReviewController({ activeRuntimeEnvironmentId, cancelNestedRepoScan, closeModal, fetchWorktrees, importNestedRepos, onGitRepoReady, setIsAdding, setStep, reviewRuntimeEnvironmentId }) {
	const review = useAddRepoNestedReviewState({
		activeRuntimeEnvironmentId: reviewRuntimeEnvironmentId,
		cancelNestedRepoScan,
		setStep
	});
	const remote = useAddRepoRemoteNestedScan({
		setActiveNestedScanId: review.setActiveNestedScanId,
		showNestedRepoReview: review.showNestedRepoReview
	});
	const imports = useAddRepoNestedImportFlow({
		activeRuntimeEnvironmentId,
		closeModal,
		fetchWorktrees,
		importNestedRepos,
		onGitRepoReady,
		setIsAdding,
		nestedAttemptId: review.nestedAttemptId,
		nestedScan: review.nestedScan,
		nestedSelectedPaths: review.nestedSelectedPaths,
		nestedRuntimeKind: review.nestedRuntimeKind,
		nestedConnectionId: review.nestedConnectionId,
		nestedGroupName: review.nestedGroupName,
		nestedImportScanId: review.nestedImportScanId,
		nestedRuntimeEnvironmentId: review.nestedRuntimeEnvironmentId,
		getNestedRepoRuntimeKind: review.getNestedRepoRuntimeKind
	});
	return {
		...review,
		...remote,
		...imports
	};
}
function useAddRepoHostedController(hosted) {
	const storeCloseModal = useAppStore((s) => s.closeModal);
	const openSettingsPage = useAppStore((s) => s.openSettingsPage);
	const openSettingsTarget = useAppStore((s) => s.openSettingsTarget);
	const hostedOnOpenChange = hosted?.onOpenChange;
	const hostedOnProjectAdded = hosted?.onProjectAdded;
	const closeModal = (0, import_react.useMemo)(() => hostedOnOpenChange ? () => hostedOnOpenChange(false) : storeCloseModal, [hostedOnOpenChange, storeCloseModal]);
	const finishProjectAdd = (0, import_react.useMemo)(() => hostedOnOpenChange && hostedOnProjectAdded ? async (repoId) => {
		await markOnboardingProjectAdded("addedRepo");
		hostedOnOpenChange(false);
		await hostedOnProjectAdded(repoId);
	} : void 0, [hostedOnOpenChange, hostedOnProjectAdded]);
	return {
		closeModal,
		closeForFolderHandoff: (0, import_react.useMemo)(() => hostedOnOpenChange ? () => {
			hostedOnOpenChange(false);
			storeCloseModal();
		} : storeCloseModal, [hostedOnOpenChange, storeCloseModal]),
		finishProjectAdd,
		handleOpenSshSettings: (0, import_react.useCallback)(() => {
			closeModal();
			if (hostedOnOpenChange) storeCloseModal();
			openSettingsTarget({
				pane: "ssh",
				repoId: null,
				sectionId: "ssh"
			});
			openSettingsPage();
		}, [
			closeModal,
			hostedOnOpenChange,
			openSettingsPage,
			openSettingsTarget,
			storeCloseModal
		])
	};
}
function routeAddRepoBrowse(host, actions) {
	if (host?.kind === "runtime") {
		actions.browseRuntime();
		return;
	}
	if (host?.kind === "ssh") {
		actions.browseSsh(host.targetId);
		return;
	}
	if (host?.kind === "local") actions.browseLocal();
}
var AddRepoDialog_default = import_react.memo(function AddRepoDialog({ hosted }) {
	const isOpen = useAppStore((s) => hosted ? hosted.open : s.activeModal === "add-repo");
	const droppedLocalPath = useAppStore((s) => !hosted && typeof s.modalData.droppedLocalPath === "string" ? s.modalData.droppedLocalPath : "");
	const addRepoPath = useAppStore((s) => s.addRepoPath);
	const scanNestedRepos = useAppStore((s) => s.scanNestedRepos);
	const cancelNestedRepoScan = useAppStore((s) => s.cancelNestedRepoScan);
	const importNestedRepos = useAppStore((s) => s.importNestedRepos);
	const repos = useAppStore((s) => s.repos);
	const fetchWorktrees = useAppStore((s) => s.fetchWorktrees);
	const setHideDefaultBranchWorkspace = useAppStore((s) => s.setHideDefaultBranchWorkspace);
	const settings = useAppStore((s) => s.settings);
	const { closeModal, closeForFolderHandoff, finishProjectAdd, handleOpenSshSettings } = useAddRepoHostedController(hosted);
	const [step, setStep] = (0, import_react.useState)("add");
	const [isAdding, setIsAdding] = (0, import_react.useState)(false);
	const [addProjectBusyLabel, setAddProjectBusyLabel] = (0, import_react.useState)(null);
	const completeGitRepoAdd = useCompleteGitRepoAdd({
		closeModal,
		setHideDefaultBranchWorkspace,
		finishProjectAdd
	});
	const hostSelection = useAddRepoHostSelection({
		isOpen,
		setStep
	});
	const selectedRuntimeEnvironmentId = hostSelection.selectedParsedHost?.kind === "runtime" ? hostSelection.selectedParsedHost.environmentId : null;
	const { nestedScan, nestedSelectedPaths, nestedGroupName, nestedScanInProgress, nestedScanId, setNestedSelectedPaths, setNestedGroupName, setNestedScanInProgress, getNestedRepoRuntimeKind, showNestedRepoReview, setActiveNestedScanId, handleStopNestedScan, resetNestedRepoReviewState, showRemoteNestedRepoReview, trackRemoteNestedScanResult, handleImportNestedRepos, handleOpenNestedRootFolder, resetNestedImportFlow, trackNestedBackAction } = useAddRepoNestedReviewController({
		reviewRuntimeEnvironmentId: selectedRuntimeEnvironmentId,
		cancelNestedRepoScan,
		closeModal: closeForFolderHandoff,
		fetchWorktrees,
		importNestedRepos,
		onGitRepoReady: completeGitRepoAdd,
		setIsAdding,
		activeRuntimeEnvironmentId: selectedRuntimeEnvironmentId,
		setStep
	});
	const { sshTargets, selectedTargetId, remotePath, remoteError, isAddingRemote, isScanningNested: isScanningRemoteNested, setSelectedTargetId, setRemotePath, setRemoteError, resetRemoteState, handleOpenRemoteStep, handleAddRemoteRepo, handleConnectTarget, stopRemoteNestedScan } = useRemoteRepo(fetchWorktrees, setStep, closeForFolderHandoff, (repoId, executionHostId) => completeGitRepoAdd(repoId, "ssh_remote_path", executionHostId), scanNestedRepos, showRemoteNestedRepoReview, trackRemoteNestedScanResult);
	const { createName, createParent, createError, isCreating, setCreateName, setCreateParent, setCreateError, resetCreateState, handlePickParent, handleCreate } = useCreateRepo(fetchWorktrees, closeForFolderHandoff, (repoId, executionHostId) => completeGitRepoAdd(repoId, "create_project", executionHostId), {
		hostId: hostSelection.selectedHostId,
		runtimeEnvironmentId: selectedRuntimeEnvironmentId,
		sshTargetId: hostSelection.selectedSshTargetId
	});
	const { createDefaultParent, createGitAvailability, createRuntimeParentStatus, createParentDefaultPending, resetCreateDefaultState, markCreateParentTouched } = useCreateProjectDefaults({
		step,
		activeRuntimeEnvironmentId: selectedRuntimeEnvironmentId,
		sshTargetId: hostSelection.selectedSshTargetId,
		createParent,
		setCreateParent
	});
	const { cloneUrl, cloneDestination, cloneError, cloneProgress, isCloning, setCloneUrl, setCloneDestination, setCloneError, resetCloneFlow, handlePickDestination, handleClone } = useAddRepoCloneFlow({
		step,
		activeRuntimeEnvironmentId: selectedRuntimeEnvironmentId,
		sshTargetId: hostSelection.selectedSshTargetId,
		workspaceDir: settings?.workspaceDir,
		fetchWorktrees,
		onGitRepoReady: completeGitRepoAdd
	});
	const isRuntimeEnvironmentActive = Boolean(selectedRuntimeEnvironmentId);
	const selectedHostKind = hostSelection.selectedParsedHost?.kind;
	const { handleBrowse, resetLocalFolderFlow } = useAddRepoLocalFolderFlow({
		isOpen,
		droppedLocalPath,
		activeRuntimeEnvironmentId: selectedRuntimeEnvironmentId,
		addRepoPath,
		closeModal: closeForFolderHandoff,
		fetchWorktrees,
		scanNestedRepos,
		setActiveNestedScanId,
		setNestedScanInProgress,
		showNestedRepoReview,
		onGitRepoReady: completeGitRepoAdd,
		setIsAdding,
		setAddProjectBusyLabel
	});
	const { serverPath, isAddingServerPath, setServerPath, resetServerPathFlow, handleAddServerPath } = useAddRepoServerPathFlow({
		addRepoPath,
		activeRuntimeEnvironmentId: selectedRuntimeEnvironmentId,
		closeModal: closeForFolderHandoff,
		fetchWorktrees,
		getNestedRepoRuntimeKind,
		scanNestedRepos,
		setActiveNestedScanId,
		setNestedScanInProgress,
		showNestedRepoReview,
		onGitRepoReady: completeGitRepoAdd,
		setAddProjectBusyLabel
	});
	const resetState = (0, import_react.useCallback)(() => {
		window.api.repos.cloneAbort();
		resetLocalFolderFlow();
		setStep("add");
		setIsAdding(false);
		setAddProjectBusyLabel(null);
		resetServerPathFlow();
		resetCloneFlow();
		resetNestedImportFlow();
		resetNestedRepoReviewState();
		resetCreateDefaultState();
		resetCreateState();
		resetRemoteState();
	}, [
		resetCloneFlow,
		resetLocalFolderFlow,
		resetNestedRepoReviewState,
		resetCreateDefaultState,
		resetServerPathFlow,
		resetNestedImportFlow,
		resetRemoteState,
		resetCreateState
	]);
	const resetHostScopedState = (0, import_react.useCallback)(() => {
		setIsAdding(false);
		setAddProjectBusyLabel(null);
		resetLocalFolderFlow();
		resetServerPathFlow();
		resetCloneFlow();
		resetCreateDefaultState();
		resetCreateState();
		resetRemoteState();
	}, [
		resetCloneFlow,
		resetCreateDefaultState,
		resetCreateState,
		resetRemoteState,
		resetLocalFolderFlow,
		resetServerPathFlow
	]);
	useAddRepoHostChangeReset({
		isOpen,
		selectedHostId: hostSelection.selectedHostId,
		onResetClosed: resetState,
		onResetHostScopedState: resetHostScopedState
	});
	const handleBack = (0, import_react.useCallback)(() => {
		if (step === "nested") trackNestedBackAction();
		resetState();
	}, [
		resetState,
		step,
		trackNestedBackAction
	]);
	const handleOpenChange = (0, import_react.useCallback)((open) => {
		if (!open) {
			if (step === "nested" && !isAdding) trackNestedBackAction();
			closeModal();
			resetState();
		}
	}, [
		closeModal,
		isAdding,
		resetState,
		step,
		trackNestedBackAction
	]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AddRepoDialogChrome, {
		isOpen,
		step,
		isAdding,
		onBack: handleBack,
		onCloseAutoFocus: hosted?.onCloseAutoFocus,
		onOpenChange: handleOpenChange,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AddRepoDialogStepContent, {
			step,
			isRuntimeEnvironmentActive,
			activeRuntimeEnvironmentId: selectedRuntimeEnvironmentId,
			isSshLikely: false,
			repoCount: repos.length,
			isAdding,
			addProjectBusyLabel,
			nestedScanInProgress,
			nestedScanId,
			serverPath,
			isAddingServerPath,
			cloneUrl,
			cloneDestination,
			cloneError,
			cloneProgress,
			isCloning,
			sshTargets,
			selectedTargetId,
			selectedSshTargetId: hostSelection.selectedSshTargetId,
			selectedHostLabel: hostSelection.hostOptions.find((host) => host.id === hostSelection.selectedHostId)?.label ?? null,
			lockSshTargetSelection: hostSelection.selectedParsedHost?.kind === "ssh",
			remotePath,
			remoteError,
			isAddingRemote,
			isScanningRemoteNested,
			nestedScan,
			nestedSelectedPaths,
			nestedGroupName,
			createName,
			createParent,
			createError,
			isCreating,
			hostSelector: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AddRepoHostSelectorSlot, { hostSelection }),
			showRemoteAction: false,
			actionsDisabled: !hostSelection.selectedHostId,
			browseHostKind: selectedHostKind ?? "runtime",
			createDefaultParent,
			createGitAvailability,
			createRuntimeParentStatus,
			createParentDefaultPending,
			manualCreateParentEntry: isRuntimeEnvironmentActive || selectedHostKind === "ssh",
			onBrowse: () => routeAddRepoBrowse(hostSelection.selectedParsedHost, {
				browseLocal: () => void handleBrowse(),
				browseRuntime: () => setStep("server-path"),
				browseSsh: (targetId) => void handleOpenRemoteStep(targetId)
			}),
			onOpenCloneStep: () => {
				if (!hostSelection.selectedHostId) return;
				setCloneError(null);
				setStep("clone");
			},
			onOpenCreateStep: () => {
				if (!hostSelection.selectedHostId) return;
				setCreateError(null);
				setStep("create");
			},
			onOpenRemoteStep: handleOpenRemoteStep,
			onStopNestedScan: handleStopNestedScan,
			onServerPathChange: setServerPath,
			onAddServerPath: (kind) => void handleAddServerPath(kind),
			onSelectTarget: (id) => {
				setSelectedTargetId(id);
				setRemoteError(null);
			},
			onRemotePathChange: (value) => {
				setRemotePath(value);
				setRemoteError(null);
			},
			onAddRemoteRepo: handleAddRemoteRepo,
			onOpenSshSettings: handleOpenSshSettings,
			onConnectTarget: handleConnectTarget,
			onStopRemoteNestedScan: stopRemoteNestedScan,
			onCloneUrlChange: (value) => {
				setCloneUrl(value);
				setCloneError(null);
			},
			onCloneDestinationChange: (value) => {
				setCloneDestination(value);
				setCloneError(null);
			},
			onPickCloneDestination: handlePickDestination,
			onClone: handleClone,
			onNestedGroupNameChange: setNestedGroupName,
			onNestedSelectedPathsChange: setNestedSelectedPaths,
			onImportNestedRepos: (mode) => void handleImportNestedRepos(mode),
			onOpenNestedRootFolder: () => void handleOpenNestedRootFolder(),
			onCreateNameChange: (value) => {
				setCreateName(value);
				setCreateError(null);
			},
			onCreateParentChange: (value) => {
				markCreateParentTouched(value);
				setCreateParent(value);
				setCreateError(null);
			},
			onPickCreateParent: () => {
				handlePickParent().then((dir) => {
					if (dir) markCreateParentTouched(dir);
				});
			},
			onCreate: handleCreate
		})
	});
});
export { AddRepoDialog_default as default };
