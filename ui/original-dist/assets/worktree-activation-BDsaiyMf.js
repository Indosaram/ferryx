import { a as translate } from "./jsx-runtime-Cv_nyRjc.js";
import { l as createLucideIcon } from "./button-DszXJEV6.js";
import { c as ConductorProgressIcon, l as ConductorReviewIcon, n as getWorkspaceStatusVisualMeta, s as ConductorDoneIcon } from "./workspace-status-wl52y3xd.js";
import { t as CircleX } from "./circle-x-Cl9fp3Vy.js";
import { t as List } from "./list-BWhHuDP9.js";
import { t as Pin } from "./pin-K26SGNXp.js";
import { Ar as resolveRuntimePaneTitleLeafId, Ec as basename, Eu as getExecutionHostIdForWorktree, Fd as isNativeChatTranscriptLocalReadable, Fu as isWebTerminalSurfaceTabId, Gp as getEffectiveProjectGroupManualRank, Hi as setWorktreeNavActivator, Hp as parseWslUncPath, Ip as isWindowsAbsolutePathLike, Jc as parsePaneKey, Ju as findFolderWorkspaceOwner, Lp as normalizeRuntimePathForComparison, Mm as DEFAULT_SHOW_SLEEPING_WORKSPACES, Ou as getRuntimeEnvironmentIdForWorktree, Sc as normalizeWorkspaceCreatorProvenance, Uc as branchName, Ui as setWorktreeNavViewActivator, Ul as getLegacyGitHubPRCacheKey, Vl as getGitHubPRCacheKey, Vp as isWslUncPath, Wd as initialAgentTabViewModeProps, Wp as UNGROUPED_PROJECT_GROUP_KEY, _h as resolveTuiAgentLaunchEnv, bg as getWorkspaceStatusFromGroupKey, bo as reconcileTabOrder, cc as resolveLocalWindowsAgentStartupShell, da as getLocalProjectExecutionRuntimeContext, ef as buildAgentResumeStartupPlan, fd as folderWorkspaceActivationBlocked, fm as RUNTIME_PROTOCOL_VERSION, gh as resolveTuiAgentLaunchArgs, hd as folderWorkspaceKey, hp as evaluateRuntimeCompat, j as getHostDisplayLabelOverrides, md as getFolderWorkspacePathStatusTitle, oa as agentEntryCompletionAt, of as encodePowerShellCommand, pd as getFolderWorkspacePathStatusDescription, qc as parseLegacyNumericPaneKey, t as useAppStore, um as MIN_COMPATIBLE_RUNTIME_SERVER_VERSION, vd as parseWorkspaceKey, vf as agentProviderSessionsEqual, vg as cloneDefaultWorkspaceStatuses, xc as folderWorkspaceToWorktree, xg as getWorkspaceStatusGroupKey, yg as getWorkspaceStatus } from "./store-CgXrfmaH.js";
import { $ as getRepoExecutionHostId, Q as getLocalExecutionHostLabel, X as LOCAL_EXECUTION_HOST_ID, Y as ALL_EXECUTION_HOSTS_SCOPE, Z as getExecutionHostLabel, a as isExplicitAgentStatusFresh, ct as toRuntimeExecutionHostId, et as getSettingsFocusedExecutionHostId, f as isFreshNonDoneAgentStatus, i as classifyTitleActivity, lt as toSshExecutionHostId, nt as isRuntimeOwnedSshTargetId, rt as normalizeExecutionHostId, st as parseExecutionHostId, tt as getWorktreeExecutionHostId, u as AGENT_STATUS_STALE_AFTER_MS, v as tabHasLivePty } from "./agent-status-3vUKbY6l.js";
import { n as toast } from "./dist-DgqligFk.js";
import { n as tuiAgentToAgentKind, t as agentKindToTuiAgent } from "./agent-kind-Dfx6MnkP.js";
import { i as getWorktreeMapFromState, r as getRepoMapFromState, t as getAllWorktreesFromState, v as getProjectHostSetupProjectionFromState } from "./selectors-XOBeaOSb.js";
import { d as isWebRuntimeSessionActive, n as activateWebRuntimeSessionWorktree, u as createWebRuntimeSessionTerminal } from "./web-runtime-session-CN2syA39.js";
import { C as beginWebRuntimeWakeTerminalRespawn, l as getLastKnownHostTerminalTabCount, w as endWebRuntimeWakeTerminalRespawn } from "./web-session-tabs-sync-CYKZbAxS.js";
import { m as CLIENT_PLATFORM, n as seedNativeChatAppliedSessionOptions } from "./native-chat-session-option-cache-DGE3h47U.js";
import { i as resolveWindowsShellOverride } from "./windows-pty-compatibility-XujC9UTf.js";
import { t as getConnectionId } from "./connection-context-BUPsamzR.js";
import { t as migrationUnsupportedToAgentStatusEntry } from "./migration-unsupported-agent-entry-BJ_0rXR-.js";
import { n as getLineageRenderInfo, t as getCyclicProjectedWorktreeLineageIds } from "./worktree-lineage-projection-CS7n_mKq.js";
import { t as getWorktreeGitIdentityDisplay } from "./worktree-git-identity-display-B29QxW_l.js";
var FolderTree = createLucideIcon("folder-tree", [
	["path", {
		d: "M20 10a1 1 0 0 0 1-1V6a1 1 0 0 0-1-1h-2.5a1 1 0 0 1-.8-.4l-.9-1.2A1 1 0 0 0 15 3h-2a1 1 0 0 0-1 1v5a1 1 0 0 0 1 1Z",
		key: "hod4my"
	}],
	["path", {
		d: "M20 21a1 1 0 0 0 1-1v-3a1 1 0 0 0-1-1h-2.9a1 1 0 0 1-.88-.55l-.42-.85a1 1 0 0 0-.92-.6H13a1 1 0 0 0-1 1v5a1 1 0 0 0 1 1Z",
		key: "w4yl2u"
	}],
	["path", {
		d: "M3 5a2 2 0 0 0 2 2h3",
		key: "f2jnh7"
	}],
	["path", {
		d: "M3 3v13a2 2 0 0 0 2 2h3",
		key: "k8epm1"
	}]
]);
function shouldAutoCreateInitialTerminal(renderableTabCount) {
	return renderableTabCount === 0;
}
var WINDOWS_RUNNER_PATH_CMD_GUARD_PATTERN = /[%&|<>^()!,;=$`]/;
function windowsRunnerPathNeedsCmdGuard(runnerScriptPath) {
	return WINDOWS_RUNNER_PATH_CMD_GUARD_PATTERN.test(runnerScriptPath);
}
function buildWindowsCmdRunnerDelayedLaunchCommand(runnerScriptPath) {
	return `powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand ${encodePowerShellCommand([
		`$runner = ${quotePowerShellString$1(runnerScriptPath)}`,
		"if ([string]::IsNullOrEmpty($runner)) { exit 1 }",
		"$processInfo = [System.Diagnostics.ProcessStartInfo]::new()",
		"$processInfo.FileName = $env:ComSpec",
		"if (-not $processInfo.FileName) { $processInfo.FileName = 'cmd.exe' }",
		"$processInfo.Arguments = '/d /s /v:on /c \"\"!ORCA_SETUP_RUNNER!\"\"'",
		"$processInfo.UseShellExecute = $false",
		"$processInfo.EnvironmentVariables[\"ORCA_SETUP_RUNNER\"] = $runner",
		"$process = [System.Diagnostics.Process]::Start($processInfo)",
		"$process.WaitForExit()",
		"exit $process.ExitCode"
	].join("; "))}`;
}
function quotePowerShellString$1(value) {
	return `'${value.replace(/'/g, "''")}'`;
}
function buildSetupRunnerCommand$1(runnerScriptPath, platform, shell) {
	return resolveSetupRunnerCommand(runnerScriptPath, platform, shell).command;
}
function getSetupRunnerCommandPlatformForPath(runnerScriptPath, fallbackPlatform) {
	if (isWindowsAbsolutePathLike(runnerScriptPath)) return "windows";
	if (runnerScriptPath.startsWith("/")) return "posix";
	return fallbackPlatform;
}
function resolveSetupRunnerCommand(runnerScriptPath, platform, shell) {
	if (platform === "windows") {
		if (isWslUncPath$1(runnerScriptPath)) {
			const linuxPath = wslUncToLinuxPath(runnerScriptPath);
			return {
				command: `bash ${quotePosixArg$1(linuxPath)}`,
				runnerScriptPathForShell: linuxPath,
				shell: "posix"
			};
		}
		if (runnerScriptPath.startsWith("/") && !isWindowsAbsolutePathLike(runnerScriptPath)) return {
			command: `bash ${quotePosixArg$1(runnerScriptPath)}`,
			runnerScriptPathForShell: runnerScriptPath,
			shell: "posix"
		};
		if (!isWindowsCmdRunnerPath(runnerScriptPath) && (shell?.family === "posix" || /\.sh$/i.test(runnerScriptPath))) {
			if (isWslExecutable(shell?.executable)) {
				const wslPath = nativeWindowsPathToWslShellPath(runnerScriptPath);
				return {
					command: `bash ${quotePosixArg$1(wslPath)}`,
					runnerScriptPathForShell: wslPath,
					shell: "posix"
				};
			}
			const posixPath = nativeWindowsPathToPosixShellPath(runnerScriptPath);
			return {
				command: `bash ${quotePosixArg$1(posixPath)}`,
				runnerScriptPathForShell: posixPath,
				shell: "posix"
			};
		}
		return {
			command: shell?.family === "posix" || windowsRunnerPathNeedsCmdGuard(runnerScriptPath) ? buildWindowsCmdRunnerDelayedLaunchCommand(runnerScriptPath) : `cmd.exe /c ${quoteWindowsArg(runnerScriptPath)}`,
			runnerScriptPathForShell: runnerScriptPath,
			shell: "windows"
		};
	}
	return {
		command: `bash ${quotePosixArg$1(runnerScriptPath)}`,
		runnerScriptPathForShell: runnerScriptPath,
		shell: "posix"
	};
}
function isWindowsCmdRunnerPath(runnerScriptPath) {
	return /\.(cmd|bat)$/i.test(runnerScriptPath);
}
function isWslUncPath$1(path) {
	const normalized = path.replace(/\\/g, "/");
	return /^\/\/(wsl\.localhost|wsl\$)\//i.test(normalized);
}
function wslUncToLinuxPath(windowsPath) {
	return windowsPath.replace(/\\/g, "/").match(/^\/\/(wsl\.localhost|wsl\$)\/[^/]+(\/.*)?$/i)?.[2] || "/";
}
function quotePosixArg$1(value) {
	if (/^[A-Za-z0-9_./:-]+$/.test(value)) return value;
	return `'${value.replace(/'/g, `'\\''`)}'`;
}
function quoteWindowsArg(value) {
	return `"${value.replace(/"/g, "\"\"")}"`;
}
function nativeWindowsPathToPosixShellPath(value) {
	const driveMatch = value.match(/^([A-Za-z]):[\\/](.*)$/);
	if (driveMatch) return `/${driveMatch[1].toLowerCase()}/${driveMatch[2].replace(/\\/g, "/")}`;
	return value.replace(/\\/g, "/");
}
function nativeWindowsPathToWslShellPath(value) {
	const driveMatch = value.match(/^([A-Za-z]):[\\/](.*)$/);
	if (driveMatch) return `/mnt/${driveMatch[1].toLowerCase()}/${driveMatch[2].replace(/\\/g, "/")}`;
	return value.replace(/\\/g, "/");
}
function isWslExecutable(value) {
	const basename$1 = value?.trim().replaceAll("\\", "/").split("/").pop()?.toLowerCase() ?? "";
	return basename$1 === "wsl.exe" || basename$1 === "wsl";
}
function buildSetupRunnerCommand(runnerScriptPath, shell) {
	return buildSetupRunnerCommand$1(runnerScriptPath, getSetupRunnerCommandPlatformForPath(runnerScriptPath, navigator.userAgent.includes("Windows") ? "windows" : "posix"), shell);
}
var DEFAULT_WAIT_TIMEOUT_SECONDS = 7200;
const SETUP_AGENT_SEQUENCE_STARTUP_COMMAND_ENV = "ORCA_SEQUENCED_STARTUP_COMMAND";
const SETUP_AGENT_SEQUENCE_STARTUP_SCRIPT_ENV = "ORCA_SEQUENCED_STARTUP_SCRIPT";
function createSetupAgentSequenceNonce() {
	const cryptoApi = globalThis.crypto;
	if (typeof cryptoApi?.randomUUID === "function") return cryptoApi.randomUUID();
	return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}
function createSequencedSetupAgentCommands(args) {
	const nonce = args.nonce ?? createSetupAgentSequenceNonce();
	const resolution = resolveSetupRunnerCommand(args.runnerScriptPath, args.platform, args.shell);
	const posixGateForWindowsRunner = resolution.shell === "windows" && args.shell?.family === "posix";
	const markerPath = `${posixGateForWindowsRunner ? nativeWindowsPathToPosixShellPath(resolution.runnerScriptPathForShell) : resolution.runnerScriptPathForShell}.${nonce}.done`;
	const waitTimeoutSeconds = args.waitTimeoutSeconds ?? DEFAULT_WAIT_TIMEOUT_SECONDS;
	if (resolution.shell === "windows" && !posixGateForWindowsRunner) return {
		setupCommand: buildWindowsSetupCommand(resolution.runnerScriptPathForShell, markerPath, nonce),
		startupCommand: buildWindowsStartupCommand(markerPath, nonce, waitTimeoutSeconds),
		startupEnv: { [SETUP_AGENT_SEQUENCE_STARTUP_COMMAND_ENV]: args.startupCommand }
	};
	const startupScript = buildPosixStartupScript(args.startupCommand, markerPath, nonce, waitTimeoutSeconds);
	return {
		setupCommand: buildPosixSetupCommand(resolution.command, markerPath, nonce),
		startupCommand: `bash -lc 'eval "$${SETUP_AGENT_SEQUENCE_STARTUP_SCRIPT_ENV}"'`,
		startupEnv: {
			[SETUP_AGENT_SEQUENCE_STARTUP_COMMAND_ENV]: args.startupCommand,
			[SETUP_AGENT_SEQUENCE_STARTUP_SCRIPT_ENV]: startupScript
		}
	};
}
function buildPosixSetupCommand(setupCommand, markerPath, nonce) {
	const marker = quotePosixArg(markerPath);
	const tmp = quotePosixArg(`${markerPath}.tmp`);
	const nonceValue = quotePosixArg(nonce);
	return `bash -lc ${quotePosixArg([
		`rm -f ${marker} ${tmp} 2>/dev/null`,
		`( ${setupCommand} )`,
		"status=$?",
		`printf '%s:%s\\n' ${nonceValue} "$status" > ${tmp}`,
		`mv -f ${tmp} ${marker}`,
		"exit \"$status\""
	].join("; "))}`;
}
function buildPosixStartupScript(startupCommand, markerPath, nonce, waitTimeoutSeconds) {
	const marker = quotePosixArg(markerPath);
	const tmp = quotePosixArg(`${markerPath}.tmp`);
	const nonceValue = quotePosixArg(nonce);
	const timeout = Math.max(1, Math.floor(waitTimeoutSeconds));
	const startupSuccessCommand = buildPosixStartupSuccessCommand(startupCommand);
	return [
		`deadline=$((SECONDS + ${timeout}));`,
		"echo \"Waiting for setup to finish before starting agent...\" >&2;",
		"while :; do",
		`if [ -f ${marker} ]; then`,
		`IFS=: read -r seen status < ${marker} || true;`,
		`if [ "$seen" = ${nonceValue} ]; then`,
		`rm -f ${marker} ${tmp} 2>/dev/null;`,
		`if [ "$status" = "0" ]; then if [ -n "\${${SETUP_AGENT_SEQUENCE_STARTUP_COMMAND_ENV}:-}" ]; then eval "\$${SETUP_AGENT_SEQUENCE_STARTUP_COMMAND_ENV}"; exit "$?"; else ${startupSuccessCommand}; fi; fi;`,
		"echo \"Setup failed; skipping agent startup.\" >&2;",
		"exit \"${status:-1}\";",
		"fi;",
		"fi;",
		"if [ \"$SECONDS\" -ge \"$deadline\" ]; then",
		"echo \"Timed out waiting for setup before starting agent.\" >&2;",
		"exit 124;",
		"fi;",
		"sleep 1;",
		"done"
	].join(" ");
}
function buildPosixStartupSuccessCommand(startupCommand) {
	if (hasUnquotedPosixCommandSeparator(startupCommand) || hasLeadingPosixEnvAssignment(startupCommand)) return `eval ${quotePosixArg(startupCommand)}; exit "$?"`;
	return `exec ${startupCommand}`;
}
function hasLeadingPosixEnvAssignment(command) {
	return /^[A-Za-z_][A-Za-z0-9_]*=/.test(command.trimStart());
}
function hasUnquotedPosixCommandSeparator(command) {
	let quote = null;
	let escaped = false;
	for (const char of command) {
		if (escaped) {
			escaped = false;
			continue;
		}
		if (char === "\\") {
			escaped = true;
			continue;
		}
		if (quote) {
			if (char === quote) quote = null;
			continue;
		}
		if (char === "'" || char === "\"") {
			quote = char;
			continue;
		}
		if (char === ";" || char === "&" || char === "|" || char === "\n" || char === "\r") return true;
	}
	return false;
}
function buildWindowsSetupCommand(runnerScriptPath, markerPath, nonce) {
	return encodePowerShellInvocation([
		`$runner = ${quotePowerShellString(runnerScriptPath)}`,
		`$marker = ${quotePowerShellString(markerPath)}`,
		"$tmp = $marker + \".tmp\"",
		`$nonce = ${quotePowerShellString(nonce)}`,
		"Remove-Item -LiteralPath $marker, $tmp -Force -ErrorAction SilentlyContinue",
		"$processInfo = [System.Diagnostics.ProcessStartInfo]::new()",
		"$processInfo.FileName = $env:ComSpec",
		"$processInfo.Arguments = '/d /s /v:on /c \"\"!ORCA_SETUP_RUNNER!\"\"'",
		"$processInfo.UseShellExecute = $false",
		"$processInfo.EnvironmentVariables[\"ORCA_SETUP_RUNNER\"] = $runner",
		"$process = [System.Diagnostics.Process]::Start($processInfo)",
		"$process.WaitForExit()",
		"$setupStatus = $process.ExitCode",
		"$utf8 = [System.Text.UTF8Encoding]::new($false)",
		"[System.IO.File]::WriteAllText($tmp, ($nonce + \":\" + $setupStatus + [Environment]::NewLine), $utf8)",
		"Move-Item -LiteralPath $tmp -Destination $marker -Force",
		"exit $setupStatus"
	].join("; "));
}
function buildWindowsStartupCommand(markerPath, nonce, waitTimeoutSeconds) {
	const timeout = Math.max(1, Math.floor(waitTimeoutSeconds));
	return encodePowerShellInvocation([
		`$marker = ${quotePowerShellString(markerPath)}`,
		"if ([string]::IsNullOrWhiteSpace($marker)) {",
		"  [Console]::Error.WriteLine(\"Missing setup marker path.\")",
		"  exit 1",
		"}",
		"$tmp = $marker + \".tmp\"",
		`$nonce = ${quotePowerShellString(nonce)}`,
		`$deadline = (Get-Date).AddSeconds(${timeout})`,
		"[Console]::Error.WriteLine(\"Waiting for setup to finish before starting agent...\")",
		"while ($true) {",
		"  if (Test-Path -LiteralPath $marker) {",
		"    $content = Get-Content -LiteralPath $marker -TotalCount 1",
		"    if ($content -match \"^([0-9A-Za-z_-]+):([0-9]+)$\" -and $Matches[1] -eq $nonce) {",
		"      $setupStatus = [int]$Matches[2]",
		"      Remove-Item -LiteralPath $marker, $tmp -Force -ErrorAction SilentlyContinue",
		"      if ($setupStatus -ne 0) {",
		"        [Console]::Error.WriteLine(\"Setup failed; skipping agent startup.\")",
		"        exit $setupStatus",
		"      }",
		`      $startup = $env:${SETUP_AGENT_SEQUENCE_STARTUP_COMMAND_ENV}`,
		"      if ([string]::IsNullOrWhiteSpace($startup)) {",
		"        [Console]::Error.WriteLine(\"Missing sequenced startup command.\")",
		"        exit 1",
		"      }",
		"      Invoke-Expression $startup",
		"      if ($global:LASTEXITCODE -ne $null) { exit $global:LASTEXITCODE }",
		"      if (-not $?) { exit 1 }",
		"      exit 0",
		"    }",
		"  }",
		"  if ((Get-Date) -ge $deadline) {",
		"    [Console]::Error.WriteLine(\"Timed out waiting for setup before starting agent.\")",
		"    exit 124",
		"  }",
		"  Start-Sleep -Seconds 1",
		"}"
	].join("; "));
}
function encodePowerShellInvocation(script) {
	return `powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand ${encodePowerShellCommand(script)}`;
}
function quotePosixArg(value) {
	if (/^[A-Za-z0-9_./:-]+$/.test(value)) return value;
	return `'${value.replace(/'/g, `'\\''`)}'`;
}
function quotePowerShellString(value) {
	return `'${value.replace(/'/g, "''")}'`;
}
function getProviderSessionClaimKey(record) {
	const base = `${record.worktreeId}\0${record.agent}\0${record.providerSession.key}\0${record.providerSession.id}`;
	return record.agent === "pi" || record.agent === "prime-agent" ? `${base}\0${record.providerSession.transcriptPath ?? ""}` : base;
}
function isPassiveCompletedHibernationEvidence(record) {
	return record.origin !== "quit" && record.origin !== "live" && record.state === "done";
}
function getLegacyPaneTabId(record) {
	const legacy = parseLegacyNumericPaneKey(record.paneKey);
	if (!legacy || record.tabId && record.tabId !== legacy.tabId) return null;
	return record.tabId ?? legacy.tabId;
}
function getLegacyProviderSessionKeysForTab(state, worktreeId, tabId) {
	const keys = /* @__PURE__ */ new Set();
	for (const record of Object.values(state.sleepingAgentSessionsByPaneKey)) if (record.worktreeId === worktreeId && getLegacyPaneTabId(record) === tabId) keys.add(getProviderSessionClaimKey(record));
	return keys;
}
function layoutContainsLeaf(node, leafId) {
	return Boolean(node && (node.type === "leaf" ? node.leafId === leafId : layoutContainsLeaf(node.first, leafId) || layoutContainsLeaf(node.second, leafId)));
}
function hasMatchingStablePaneLayout(tabId, leafId, terminalLayoutsByTabId) {
	return layoutContainsLeaf(terminalLayoutsByTabId[tabId]?.root, leafId);
}
function hasRestorableStablePanePty(tab, tabId, leafId, ptyIdsByTabId, terminalLayoutsByTabId) {
	const layout = terminalLayoutsByTabId[tabId];
	const hasLeafPty = Boolean(layout?.ptyIdsByLeafId?.[leafId]);
	const isSingleLeafLayout = layout?.root?.type === "leaf" && layout.root.leafId === leafId;
	return Boolean(hasLeafPty || isSingleLeafLayout && (tab.ptyId || (ptyIdsByTabId[tabId]?.length ?? 0) > 0));
}
function stablePaneHasLivePty(tabId, leafId, ptyIdsByTabId, layout) {
	const livePtyIds = ptyIdsByTabId[tabId] ?? [];
	if (livePtyIds.length === 0) return false;
	const leafPtyId = layout?.ptyIdsByLeafId?.[leafId];
	if (leafPtyId) return livePtyIds.includes(leafPtyId);
	return layout?.root?.type === "leaf" && layout.root.leafId === leafId;
}
function paneWillConnectOnActivation(worktreeId, tabId, state) {
	if (state.activeWorktreeId !== worktreeId) return false;
	return !isWebTerminalSurfaceTabId(tabId);
}
function recordPaneIsOwnedByPreservedPane(record, state) {
	const worktreeTabs = state.tabsByWorktree[record.worktreeId] ?? [];
	const stable = parsePaneKey(record.paneKey);
	if (stable) {
		if (record.tabId && record.tabId !== stable.tabId) return false;
		const tabId$1 = record.tabId ?? stable.tabId;
		const tab$1 = worktreeTabs.find((candidate) => candidate.id === tabId$1) ?? null;
		if (!tab$1 || !hasMatchingStablePaneLayout(tabId$1, stable.leafId, state.terminalLayoutsByTabId)) return false;
		if (isPassiveCompletedHibernationEvidence(record)) return true;
		if (stablePaneHasLivePty(tabId$1, stable.leafId, state.ptyIdsByTabId, state.terminalLayoutsByTabId[tabId$1])) return true;
		return hasRestorableStablePanePty(tab$1, tabId$1, stable.leafId, state.ptyIdsByTabId, state.terminalLayoutsByTabId) && paneWillConnectOnActivation(record.worktreeId, tabId$1, state);
	}
	const tabId = getLegacyPaneTabId(record);
	if (!tabId) return false;
	const tab = worktreeTabs.find((candidate) => candidate.id === tabId) ?? null;
	const providerKeys = getLegacyProviderSessionKeysForTab(state, record.worktreeId, tabId);
	return Boolean(tab && (tab.ptyId || (state.ptyIdsByTabId[tab.id]?.length ?? 0) > 0) && providerKeys.size === 1 && paneWillConnectOnActivation(record.worktreeId, tabId, state));
}
function resolveResumeLaunchPlatform(args) {
	if (args.projectRuntime?.status === "repair-required") return args.projectRuntime.repair.preferredRuntime.kind === "wsl" ? "linux" : CLIENT_PLATFORM;
	if (args.projectRuntime?.status === "resolved" && args.projectRuntime.runtime.kind === "wsl") return "linux";
	if (args.connectionId || args.worktreePath && isWslUncPath(args.worktreePath)) return "linux";
	return CLIENT_PLATFORM;
}
function resolveAgentResumeLaunchTarget(args) {
	const platform = resolveResumeLaunchPlatform(args);
	return {
		platform,
		shell: resolveLocalWindowsAgentStartupShell({
			platform,
			isRemote: Boolean(args.connectionId) || parseExecutionHostId(args.executionHostId)?.kind !== "local",
			terminalWindowsShell: resolveWindowsShellOverride(args.tabShellOverride, args.terminalWindowsShell)
		})
	};
}
function getResumeLaunchTarget(worktreeId) {
	const state = useAppStore.getState();
	const worktree = state.getKnownWorktreeById(worktreeId);
	const repo = worktree ? state.repos.find((entry) => entry.id === worktree.repoId) : null;
	return resolveAgentResumeLaunchTarget({
		projectRuntime: getLocalProjectExecutionRuntimeContext(state, worktreeId),
		connectionId: repo?.connectionId,
		executionHostId: getExecutionHostIdForWorktree(state, worktreeId),
		worktreePath: worktree?.path,
		terminalWindowsShell: state.settings?.terminalWindowsShell
	});
}
function appendTabToWorktreeOrder(worktreeId, tabId) {
	const state = useAppStore.getState();
	const termIds = (state.tabsByWorktree[worktreeId] ?? []).map((tab) => tab.id);
	const editorIds = state.openFiles.filter((file) => file.worktreeId === worktreeId).map((f) => f.id);
	const browserIds = (state.browserTabsByWorktree?.[worktreeId] ?? []).map((tab) => tab.id);
	const order = reconcileTabOrder(state.tabBarOrderByWorktree[worktreeId], termIds, editorIds, browserIds).filter((id) => id !== tabId);
	order.push(tabId);
	state.setTabBarOrder(worktreeId, order);
}
function launchSleepingAgentSession(record, options) {
	const state = useAppStore.getState();
	const launchConfig = record.launchConfig;
	const resumeTarget = getResumeLaunchTarget(record.worktreeId);
	const startupPlan = buildAgentResumeStartupPlan({
		agent: record.agent,
		providerSession: record.providerSession,
		cmdOverrides: state.settings?.agentCmdOverrides ?? {},
		agentArgs: launchConfig !== void 0 ? launchConfig.agentArgs : resolveTuiAgentLaunchArgs(record.agent, state.settings?.agentDefaultArgs),
		agentEnv: launchConfig !== void 0 ? launchConfig.agentEnv : resolveTuiAgentLaunchEnv(record.agent, state.settings?.agentDefaultEnv),
		...launchConfig?.agentCommand ? { agentCommand: launchConfig.agentCommand } : {},
		...launchConfig?.ompResumeFilePath ? { ompResumeFilePath: launchConfig.ompResumeFilePath } : {},
		platform: resumeTarget.platform,
		shell: resumeTarget.shell
	});
	if (!startupPlan) {
		toast.error(translate("auto.lib.resume.sleeping.agent.session.f235f604fd", "This agent session cannot be resumed."));
		return false;
	}
	const tab = state.createTab(record.worktreeId, void 0, void 0, {
		launchAgent: record.agent,
		...options?.suppressNavigation ? {
			activate: false,
			recordInteraction: false
		} : {}
	});
	state.queueTabStartupCommand(tab.id, {
		command: startupPlan.launchCommand,
		...startupPlan.env ? { env: startupPlan.env } : {},
		launchConfig: startupPlan.launchConfig,
		resumeProviderSession: record.providerSession,
		launchAgent: record.agent,
		...launchConfig ? { agentArgsOverride: launchConfig.agentArgs } : {},
		...startupPlan.startupCommandDelivery ? { startupCommandDelivery: startupPlan.startupCommandDelivery } : {},
		showSessionRestoredBanner: true,
		telemetry: {
			agent_kind: tuiAgentToAgentKind(record.agent),
			launch_source: "sidebar",
			request_kind: "resume"
		}
	});
	state.claimAutomaticAgentResume(tab.id, {
		worktreeId: record.worktreeId,
		launchAgent: record.agent,
		providerSession: record.providerSession
	});
	state.clearSleepingAgentSession(record.paneKey);
	if (!options?.suppressNavigation) state.setActiveTabType("terminal");
	appendTabToWorktreeOrder(record.worktreeId, tab.id);
	options?.onSessionLaunched?.(tab.id);
	return true;
}
function clearPassiveCompletedRecordsForClaimKey(records, claimKey, keepPaneKey) {
	const state = useAppStore.getState();
	for (const record of records) {
		if (record.paneKey === keepPaneKey || !isPassiveCompletedHibernationEvidence(record)) continue;
		if (getProviderSessionClaimKey(record) === claimKey) state.clearSleepingAgentSession(record.paneKey);
	}
}
function getCurrentPaneOwnedClaimKeys(records) {
	const state = useAppStore.getState();
	const keys = /* @__PURE__ */ new Set();
	for (const record of records) {
		if (state.sleepingAgentSessionsByPaneKey[record.paneKey] !== record || isInvalidWorktreeActivationRecord(record) || isPassiveCompletedHibernationEvidence(record)) continue;
		if (recordPaneIsOwnedByPreservedPane(record, state)) keys.add(getProviderSessionClaimKey(record));
	}
	return keys;
}
function getNewestActiveRecordsByClaimKey(records) {
	const newestRecords = /* @__PURE__ */ new Map();
	for (const record of records) {
		const claimKey = getProviderSessionClaimKey(record);
		const current = newestRecords.get(claimKey);
		if (!current || record.capturedAt > current.capturedAt || record.capturedAt === current.capturedAt && record.updatedAt > current.updatedAt) newestRecords.set(claimKey, record);
	}
	return newestRecords;
}
function getAgentStatusTabId(entry) {
	if (entry.tabId) return entry.tabId;
	const separatorIndex = entry.paneKey.indexOf(":");
	return separatorIndex === -1 ? null : entry.paneKey.slice(0, separatorIndex);
}
function activeOrQueuedResumeClaimsProviderSession(record, state, samePaneOwnsRecovery) {
	const worktreeTabIds = new Set((state.tabsByWorktree[record.worktreeId] ?? []).map((tab) => tab.id));
	for (const entry of Object.values(state.agentStatusByPaneKey)) {
		if (samePaneOwnsRecovery && entry.paneKey === record.paneKey) continue;
		if (worktreeTabIds.has(getAgentStatusTabId(entry) ?? "") && entry.worktreeId === record.worktreeId && entry.agentType === record.agent && entry.state !== "done" && agentProviderSessionsEqual(record.agent, entry.providerSession, record.providerSession)) return true;
	}
	for (const [tabId, startup] of Object.entries(state.pendingStartupByTabId)) if (worktreeTabIds.has(tabId) && startup.launchAgent === record.agent && agentProviderSessionsEqual(record.agent, startup.resumeProviderSession, record.providerSession)) return true;
	for (const [tabId, claim] of Object.entries(state.automaticAgentResumeClaimsByTabId)) if (worktreeTabIds.has(tabId) && claim.worktreeId === record.worktreeId && claim.launchAgent === record.agent && agentProviderSessionsEqual(record.agent, claim.providerSession, record.providerSession)) return true;
	return false;
}
function isInvalidWorktreeActivationRecord(record) {
	if (!record.origin && record.state === "done") return true;
	return record.state !== "done" && record.capturedAt - record.updatedAt > 18e5;
}
function resumeSleepingAgentSessionsForWorktree(worktreeId, options) {
	const state = useAppStore.getState();
	const worktreeRecords = Object.values(state.sleepingAgentSessionsByPaneKey).filter((record) => record.worktreeId === worktreeId).sort((a, b) => a.capturedAt - b.capturedAt || a.updatedAt - b.updatedAt);
	const activeWorktreeRecords = worktreeRecords.filter((record) => !isInvalidWorktreeActivationRecord(record)).filter((record) => !isPassiveCompletedHibernationEvidence(record));
	const activeClaimKeys = new Set(activeWorktreeRecords.map(getProviderSessionClaimKey));
	const newestActiveRecordByClaimKey = getNewestActiveRecordsByClaimKey(activeWorktreeRecords);
	const freshlyLaunchedClaimKeys = /* @__PURE__ */ new Set();
	let launched = 0;
	for (const record of worktreeRecords) {
		const currentState = useAppStore.getState();
		if (currentState.sleepingAgentSessionsByPaneKey[record.paneKey] !== record) continue;
		const claimKey = getProviderSessionClaimKey(record);
		if (options?.skipClaimKeys?.has(claimKey)) continue;
		if (record.automaticResumeBlockedBy === "legacy-orchestration-worker") continue;
		if (isInvalidWorktreeActivationRecord(record)) {
			state.clearSleepingAgentSession(record.paneKey);
			continue;
		}
		const isPaneOwned = recordPaneIsOwnedByPreservedPane(record, currentState);
		if (isPassiveCompletedHibernationEvidence(record)) {
			if (!isPaneOwned || activeClaimKeys.has(claimKey)) state.clearSleepingAgentSession(record.paneKey);
			continue;
		}
		if (activeOrQueuedResumeClaimsProviderSession(record, currentState, isPaneOwned)) {
			state.clearSleepingAgentSession(record.paneKey);
			continue;
		}
		if (getCurrentPaneOwnedClaimKeys(activeWorktreeRecords).has(claimKey)) {
			if (!isPaneOwned) state.clearSleepingAgentSession(record.paneKey);
			continue;
		}
		if (freshlyLaunchedClaimKeys.has(claimKey)) {
			state.clearSleepingAgentSession(record.paneKey);
			continue;
		}
		if (newestActiveRecordByClaimKey.get(claimKey) !== record) {
			state.clearSleepingAgentSession(record.paneKey);
			continue;
		}
		if (isPaneOwned) continue;
		if (launchSleepingAgentSession(record, options)) {
			launched += 1;
			freshlyLaunchedClaimKeys.add(claimKey);
			clearPassiveCompletedRecordsForClaimKey(worktreeRecords, claimKey, record.paneKey);
		}
	}
	return launched;
}
var pendingHookCommandDeliveries = /* @__PURE__ */ new Map();
var unsubscribePendingHookCommandDeliveries = null;
function queueHookCommandsForFirstWorktreeTab(delivery) {
	const queued = pendingHookCommandDeliveries.get(delivery.worktreeId);
	if (queued) queued.push(delivery);
	else pendingHookCommandDeliveries.set(delivery.worktreeId, [delivery]);
	ensurePendingHookCommandSubscription();
	flushPendingHookCommandDeliveries();
}
function ensurePendingHookCommandSubscription() {
	if (unsubscribePendingHookCommandDeliveries) return;
	const initial = useAppStore.getState();
	let previousTabsByWorktree = initial.tabsByWorktree;
	let previousWorktreesByRepo = initial.worktreesByRepo;
	let previousDetectedWorktreesByRepo = initial.detectedWorktreesByRepo;
	let previousFolderWorkspaces = initial.folderWorkspaces;
	let previousWorktreeLookup = initial.getKnownWorktreeById;
	unsubscribePendingHookCommandDeliveries = useAppStore.subscribe((state) => {
		if (state.tabsByWorktree === previousTabsByWorktree && state.worktreesByRepo === previousWorktreesByRepo && state.detectedWorktreesByRepo === previousDetectedWorktreesByRepo && state.folderWorkspaces === previousFolderWorkspaces && state.getKnownWorktreeById === previousWorktreeLookup) return;
		previousTabsByWorktree = state.tabsByWorktree;
		previousWorktreesByRepo = state.worktreesByRepo;
		previousDetectedWorktreesByRepo = state.detectedWorktreesByRepo;
		previousFolderWorkspaces = state.folderWorkspaces;
		previousWorktreeLookup = state.getKnownWorktreeById;
		flushPendingHookCommandDeliveries();
	});
}
function stopPendingHookCommandSubscriptionIfIdle() {
	if (pendingHookCommandDeliveries.size > 0 || !unsubscribePendingHookCommandDeliveries) return;
	unsubscribePendingHookCommandDeliveries();
	unsubscribePendingHookCommandDeliveries = null;
}
function flushPendingHookCommandDeliveries() {
	const state = useAppStore.getState();
	for (const [worktreeId, deliveries] of pendingHookCommandDeliveries) {
		const firstTerminalTabId = state.tabsByWorktree[worktreeId]?.[0]?.id;
		if (!firstTerminalTabId) {
			if (!state.getKnownWorktreeById(worktreeId)) pendingHookCommandDeliveries.delete(worktreeId);
			continue;
		}
		pendingHookCommandDeliveries.delete(worktreeId);
		for (const delivery of deliveries) delivery.deliver(state, firstTerminalTabId);
	}
	stopPendingHookCommandSubscriptionIfIdle();
}
const IDLE = {
	cls: 4,
	attentionTimestamp: 0
};
function hasFreshAttributedAgentStatus(agentStatusByPaneKey, now, tabsByWorktree) {
	const freshUnstampedTabIds = /* @__PURE__ */ new Set();
	for (const entry of Object.values(agentStatusByPaneKey ?? {})) {
		const parsed = parsePaneKey(entry.paneKey);
		if (parsed === null || !isExplicitAgentStatusFresh(entry, now, 18e5)) continue;
		if (entry.worktreeId) return true;
		freshUnstampedTabIds.add(parsed.tabId);
	}
	if (freshUnstampedTabIds.size === 0) return false;
	return Object.values(tabsByWorktree).some((tabs) => tabs.some((tab) => freshUnstampedTabIds.has(tab.id)));
}
function mostRecentAttentionInHistory(history) {
	let max = 0;
	for (const h of history) {
		if (h.state === "done" && h.interrupted) continue;
		if (h.state === "done" || h.state === "blocked" || h.state === "waiting") {
			if (!Number.isFinite(h.startedAt)) continue;
			if (h.startedAt > max) max = h.startedAt;
		}
	}
	return max > 0 ? max : null;
}
function resolveAttention(panes, now) {
	let bestCls = 4;
	let bestTs = 0;
	let bestCause;
	for (const pane of panes) {
		let cls;
		let ts;
		let cause;
		if (pane.kind === "hook") {
			const entry = pane.entry;
			if (!isExplicitAgentStatusFresh(entry, now, 18e5)) continue;
			if (!Number.isFinite(entry.stateStartedAt)) continue;
			if (entry.state === "blocked" || entry.state === "waiting") {
				cls = 1;
				ts = entry.stateStartedAt;
				cause = entry.state;
			} else if (entry.state === "done") {
				const completedAt = agentEntryCompletionAt(entry);
				if (completedAt === null) continue;
				if (now - completedAt > 18e5) continue;
				cls = 2;
				ts = completedAt;
			} else {
				cls = 3;
				const prior = mostRecentAttentionInHistory(entry.stateHistory);
				if (prior === null) ts = entry.stateStartedAt;
				else if (entry.agentType === "command-code") ts = Math.max(prior, entry.stateStartedAt);
				else ts = prior;
			}
		} else if (pane.status === "permission") {
			cls = 1;
			ts = now;
			cause = "title-heuristic";
		} else if (pane.status === "working") {
			cls = 3;
			ts = pane.worktreeLastActivityAt;
		} else continue;
		if (cls < bestCls || cls === bestCls && ts > bestTs) {
			bestCls = cls;
			bestTs = ts;
			bestCause = cause;
		}
	}
	return bestCls === 1 && bestCause ? {
		cls: bestCls,
		attentionTimestamp: bestTs,
		cause: bestCause
	} : {
		cls: bestCls,
		attentionTimestamp: bestTs
	};
}
function buildExplicitEntriesByTabId(agentStatusByPaneKey, migrationUnsupportedByPtyId) {
	const byTab = /* @__PURE__ */ new Map();
	const entries = [...Object.values(agentStatusByPaneKey ?? {}), ...Object.values(migrationUnsupportedByPtyId ?? {}).flatMap((entry) => {
		const agentEntry = migrationUnsupportedToAgentStatusEntry(entry);
		return agentEntry ? [agentEntry] : [];
	})];
	if (entries.length === 0) return byTab;
	for (const entry of entries) {
		const parsed = parsePaneKey(entry.paneKey);
		if (!parsed) continue;
		const bucket = byTab.get(parsed.tabId);
		if (bucket) bucket.push(entry);
		else byTab.set(parsed.tabId, [entry]);
	}
	return byTab;
}
function buildExplicitEntriesByWorktreeId(agentStatusByPaneKey) {
	const byWorktree = /* @__PURE__ */ new Map();
	for (const entry of Object.values(agentStatusByPaneKey ?? {})) {
		if (!entry.worktreeId || !parsePaneKey(entry.paneKey)) continue;
		const bucket = byWorktree.get(entry.worktreeId);
		if (bucket) bucket.push(entry);
		else byWorktree.set(entry.worktreeId, [entry]);
	}
	return byWorktree;
}
function leafIdFromPaneKey(paneKey) {
	return parsePaneKey(paneKey)?.leafId ?? null;
}
function collectTabPaneInputs(tab, worktreeLastActivityAt, sources, now) {
	const panes = [];
	const hookLeafIds = /* @__PURE__ */ new Set();
	for (const entry of sources.entriesByTabId.get(tab.id) ?? []) {
		panes.push({
			kind: "hook",
			entry
		});
		if (!entry.restoredUnconfirmed && !isExplicitAgentStatusFresh(entry, now, 18e5)) continue;
		const leafId = leafIdFromPaneKey(entry.paneKey);
		if (leafId !== null) hookLeafIds.add(leafId);
	}
	if (!tabHasLivePty(sources.ptyIdsByTabId, tab.id)) return panes;
	const paneTitles = sources.runtimePaneTitlesByTabId[tab.id];
	if (!paneTitles || Object.keys(paneTitles).length === 0) {
		if (hookLeafIds.size === 0) panes.push({
			kind: "title",
			status: classifyTitleActivity(tab.title),
			worktreeLastActivityAt
		});
		return panes;
	}
	const tabLayout = sources.terminalLayoutsByTabId?.[tab.id];
	const paneTitleEntries = Object.entries(paneTitles);
	for (const [runtimePaneId, title] of paneTitleEntries) {
		const leafId = resolveRuntimePaneTitleLeafId(tabLayout, runtimePaneId);
		const hasSingleUnmappedHook = leafId === null && hookLeafIds.size === 1 && paneTitleEntries.length === 1;
		if (leafId !== null && hookLeafIds.has(leafId) || hasSingleUnmappedHook) continue;
		panes.push({
			kind: "title",
			status: classifyTitleActivity(title),
			worktreeLastActivityAt
		});
	}
	return panes;
}
function buildAttentionByWorktree(worktrees, tabsByWorktree, agentStatusByPaneKey, runtimePaneTitlesByTabId, ptyIdsByTabId, now, migrationUnsupportedByPtyId, terminalLayoutsByTabId) {
	const byTab = buildExplicitEntriesByTabId(agentStatusByPaneKey, migrationUnsupportedByPtyId);
	const byAttributedWorktree = buildExplicitEntriesByWorktreeId(agentStatusByPaneKey);
	const mirroredTabIds = new Set(Object.values(tabsByWorktree ?? {}).flatMap((tabs) => tabs.map((tab) => tab.id)));
	const paneSources = {
		entriesByTabId: byTab,
		ptyIdsByTabId,
		runtimePaneTitlesByTabId,
		terminalLayoutsByTabId
	};
	const result = /* @__PURE__ */ new Map();
	for (const worktree of worktrees) {
		const tabs = tabsByWorktree?.[worktree.id] ?? [];
		const panes = (byAttributedWorktree.get(worktree.id) ?? []).filter((entry) => {
			const parsed = parsePaneKey(entry.paneKey);
			return parsed !== null && !mirroredTabIds.has(parsed.tabId);
		}).map((entry) => ({
			kind: "hook",
			entry
		}));
		if (tabs.length === 0) {
			result.set(worktree.id, resolveAttention(panes, now));
			continue;
		}
		for (const tab of tabs) panes.push(...collectTabPaneInputs(tab, worktree.lastActivityAt, paneSources, now));
		result.set(worktree.id, resolveAttention(panes, now));
	}
	return result;
}
const CREATE_GRACE_MS = 300 * 1e3;
function effectiveRecentActivity(worktree, now) {
	const { lastActivityAt, createdAt } = worktree;
	if (createdAt === void 0 || now >= createdAt + 3e5) return lastActivityAt;
	return Math.max(lastActivityAt, createdAt + CREATE_GRACE_MS);
}
function getWorktreeSortLabel(worktree) {
	const displayName = typeof worktree.displayName === "string" ? worktree.displayName.trim() : "";
	if (displayName) return displayName;
	return (typeof worktree.path === "string" ? basename(worktree.path).trim() : "") || worktree.id;
}
function compareWorktreeSortLabel(a, b) {
	return getWorktreeSortLabel(a).localeCompare(getWorktreeSortLabel(b));
}
function buildWorktreeComparator(sortBy, repoMap, now, attentionByWorktree) {
	return (a, b) => {
		switch (sortBy) {
			case "name": return compareWorktreeSortLabel(a, b);
			case "smart": {
				const aw = attentionByWorktree.get(a.id) ?? IDLE;
				const bw = attentionByWorktree.get(b.id) ?? IDLE;
				return aw.cls - bw.cls || bw.attentionTimestamp - aw.attentionTimestamp || effectiveRecentActivity(b, now) - effectiveRecentActivity(a, now) || compareWorktreeSortLabel(a, b);
			}
			case "recent": return effectiveRecentActivity(b, now) - effectiveRecentActivity(a, now) || compareWorktreeSortLabel(a, b);
			case "repo": {
				const ra = repoMap.get(a.repoId)?.displayName ?? "";
				const rb = repoMap.get(b.repoId)?.displayName ?? "";
				const cmp = ra.localeCompare(rb);
				return cmp !== 0 ? cmp : compareWorktreeSortLabel(a, b);
			}
			case "manual": return (b.manualOrder ?? b.sortOrder) - (a.manualOrder ?? a.sortOrder) || compareWorktreeSortLabel(a, b);
		}
	};
}
function sortWorktreesSmart(worktrees, tabsByWorktree, repoMap, agentStatusByPaneKey, runtimePaneTitlesByTabId, ptyIdsByTabId, migrationUnsupportedByPtyId, terminalLayoutsByTabId) {
	const hasAnyLivePty = Object.values(tabsByWorktree).flat().some((tab) => tabHasLivePty(ptyIdsByTabId, tab.id));
	const now = Date.now();
	if (!hasAnyLivePty && !hasFreshAttributedAgentStatus(agentStatusByPaneKey, now, tabsByWorktree)) return [...worktrees].sort((a, b) => b.sortOrder - a.sortOrder || compareWorktreeSortLabel(a, b));
	const attentionByWorktree = buildAttentionByWorktree(worktrees, tabsByWorktree, agentStatusByPaneKey, runtimePaneTitlesByTabId, ptyIdsByTabId, now, migrationUnsupportedByPtyId, terminalLayoutsByTabId);
	return [...worktrees].sort(buildWorktreeComparator("smart", repoMap, now, attentionByWorktree));
}
function parseAgentStatusPaneIdentity(paneKey) {
	if (!paneKey) return null;
	const parsed = parsePaneKey(paneKey);
	if (parsed) return {
		tabId: parsed.tabId,
		paneId: parsed.leafId
	};
	const legacy = parseLegacyNumericPaneKey(paneKey);
	return legacy ? {
		tabId: legacy.tabId,
		paneId: legacy.numericPaneId
	} : null;
}
function resolveAgentStatusWorktreeId(entry, worktreeIdByTabId, orchestration = entry.orchestration) {
	const paneIdentity = parseAgentStatusPaneIdentity(entry.paneKey);
	const parentIdentity = parseAgentStatusPaneIdentity(orchestration?.parentPaneKey);
	return worktreeIdByTabId.get(paneIdentity?.tabId ?? "") ?? entry.worktreeId ?? worktreeIdByTabId.get(parentIdentity?.tabId ?? "") ?? null;
}
function mergeAgentStatusOrchestration(entry, runtimeOrchestration) {
	if (!entry.orchestration) return runtimeOrchestration;
	if (!runtimeOrchestration || entry.orchestration.taskId !== runtimeOrchestration.taskId || entry.orchestration.dispatchId !== runtimeOrchestration.dispatchId) return entry.orchestration;
	return {
		...entry.orchestration,
		...runtimeOrchestration
	};
}
function getWorktreeIdsWithLiveAgent(agentStatusByPaneKey, tabsByWorktree, now) {
	return new Set(getLiveAgentStatusByWorktreeId(agentStatusByPaneKey, tabsByWorktree, now).keys());
}
function getLiveAgentStatusByWorktreeId(agentStatusByPaneKey, tabsByWorktree, now) {
	const entries = Object.values(agentStatusByPaneKey ?? {}).filter((entry) => isFreshNonDoneAgentStatus(entry, now));
	if (entries.length === 0) return /* @__PURE__ */ new Map();
	const worktreeIdByTabId = /* @__PURE__ */ new Map();
	for (const [worktreeId, tabs] of Object.entries(tabsByWorktree ?? {})) for (const tab of tabs) worktreeIdByTabId.set(tab.id, worktreeId);
	const result = /* @__PURE__ */ new Map();
	for (const entry of entries) {
		const worktreeId = resolveAgentStatusWorktreeId(entry, worktreeIdByTabId);
		if (worktreeId) {
			const status = entry.state === "working" ? "working" : "permission";
			if (status === "permission" || !result.has(worktreeId)) result.set(worktreeId, status);
		}
	}
	return result;
}
function hasActiveWorkspaceActivity(worktreeId, tabsByWorktree, ptyIdsByTabId, browserTabsByWorktree, worktreeIdsWithLiveAgent) {
	const tabs = tabsByWorktree?.[worktreeId] ?? [];
	const hasLiveTerminal = ptyIdsByTabId != null && tabs.some((tab) => tabHasLivePty(ptyIdsByTabId, tab.id));
	const hasBrowser = (browserTabsByWorktree?.[worktreeId] ?? []).length > 0;
	const hasLiveAgent = worktreeIdsWithLiveAgent.has(worktreeId);
	return hasLiveTerminal || hasBrowser || hasLiveAgent;
}
function isInactiveWorkspace(worktreeId, tabsByWorktree, ptyIdsByTabId, browserTabsByWorktree, worktreeIdsWithLiveAgent) {
	return !hasActiveWorkspaceActivity(worktreeId, tabsByWorktree, ptyIdsByTabId, browserTabsByWorktree, worktreeIdsWithLiveAgent);
}
function getRepoDisplayLabelKey(item) {
	return `${getRepoExecutionHostId(item)}::${item.path}`;
}
function normalizePathSegments(path) {
	return path.replace(/\\/g, "/").replace(/\/+$/g, "").split("/").filter(Boolean);
}
function labelForDepth(item, depth) {
	const segments = normalizePathSegments(item.path);
	const suffix = segments.slice(Math.max(0, segments.length - depth));
	if (suffix.length === 0) return item.displayName;
	suffix[suffix.length - 1] = item.displayName;
	return suffix.join("/");
}
function hasDuplicateLabels(labels) {
	return new Set(labels).size !== labels.length;
}
function getRepoDisplayLabelsByPath(items) {
	const labels = /* @__PURE__ */ new Map();
	const itemsByName = /* @__PURE__ */ new Map();
	for (const item of items) {
		const displayName = item.displayName || item.path;
		labels.set(getRepoDisplayLabelKey(item), displayName);
		const colliding = itemsByName.get(displayName) ?? [];
		colliding.push({
			...item,
			displayName
		});
		itemsByName.set(displayName, colliding);
	}
	for (const collidingItems of itemsByName.values()) {
		if (collidingItems.length < 2) continue;
		const maxDepth = Math.max(...collidingItems.map((item) => normalizePathSegments(item.path).length));
		let depth = 1;
		let nextLabels = collidingItems.map((item) => labelForDepth(item, depth));
		while (depth < maxDepth && hasDuplicateLabels(nextLabels)) {
			depth += 1;
			nextLabels = collidingItems.map((item) => labelForDepth(item, depth));
		}
		collidingItems.forEach((item, index) => {
			labels.set(getRepoDisplayLabelKey(item), nextLabels[index] ?? item.displayName);
		});
	}
	return labels;
}
function getPinnedWorktreeDisplayPolicy(settings) {
	return settings?.showPinnedWorktreesInGroups === true ? "duplicate-in-groups" : "single-location";
}
function buildPendingCreationRow(creation, repoMap) {
	return {
		type: "pending-creation",
		key: `pending:${creation.creationId}`,
		creationId: creation.creationId,
		repo: repoMap.get(creation.repoId)
	};
}
var projectGroupingIndexCache = /* @__PURE__ */ new WeakMap();
function isDistinctUserCheckout(setup) {
	return setup.setupMethod !== "provisioned" && setup.kind !== "folder";
}
function getProjectSetupSurfaceKey(setup) {
	return `${setup.projectId}::${setup.hostId}::${getExecutionSurface(setup)}::${getPathSurface(setup)}`;
}
function getExecutionSurface(setup) {
	const connectionId = setup.connectionId?.trim();
	if (connectionId) return toSshExecutionHostId(connectionId);
	return setup.executionHostId?.trim() || setup.hostId;
}
function getCheckoutIdentity(setup) {
	return normalizeRuntimePathForComparison(setup.path.trim()) || setup.repoId || setup.id;
}
function getPathSurface(setup) {
	const wslPath = parseWslUncPath(setup.path);
	if (wslPath) return `wsl:${wslPath.distro.toLowerCase()}`;
	if (isWindowsAbsolutePathLike(setup.path)) return "windows-host";
	return "default";
}
function buildProjectGroupingIndex(model) {
	if (!model) return null;
	const cached = projectGroupingIndexCache.get(model);
	if (cached !== void 0) return cached;
	const projects = model.projects ?? [];
	const projectHostSetups = model.projectHostSetups ?? [];
	if (projects.length === 0 || projectHostSetups.length === 0) {
		projectGroupingIndexCache.set(model, null);
		return null;
	}
	const checkoutsByProjectSurface = /* @__PURE__ */ new Map();
	for (const setup of projectHostSetups) {
		if (!isDistinctUserCheckout(setup)) continue;
		const key = getProjectSetupSurfaceKey(setup);
		const existing = checkoutsByProjectSurface.get(key);
		if (existing) existing.add(getCheckoutIdentity(setup));
		else checkoutsByProjectSurface.set(key, new Set([getCheckoutIdentity(setup)]));
	}
	const surfaceKeysRequiringSetupGroups = /* @__PURE__ */ new Set();
	for (const [surfaceKey, checkouts] of checkoutsByProjectSurface) if (checkouts.size > 1) surfaceKeysRequiringSetupGroups.add(surfaceKey);
	const index = {
		projectById: new Map(projects.map((project) => [project.id, project])),
		setupByRepoId: new Map(projectHostSetups.map((setup) => [setup.repoId, setup])),
		surfaceKeysRequiringSetupGroups
	};
	projectGroupingIndexCache.set(model, index);
	return index;
}
function getProjectGroupingForRepo(repoId, repoMap, projectIndex) {
	const repo = repoMap.get(repoId);
	const setup = projectIndex?.setupByRepoId.get(repoId);
	const project = setup ? projectIndex?.projectById.get(setup.projectId) : void 0;
	if (!setup || !project) return {
		key: `repo:${repoId}`,
		label: repo?.displayName ?? "Unknown",
		repo
	};
	if (projectIndex?.surfaceKeysRequiringSetupGroups.has(getProjectSetupSurfaceKey(setup)) && isDistinctUserCheckout(setup)) return {
		key: `project:${project.id}::setup:${repoId}`,
		label: repo?.displayName ?? setup.displayName,
		repo,
		projectId: project.id
	};
	return {
		key: `project:${project.id}`,
		label: project.displayName,
		repo,
		projectId: project.id
	};
}
function getProjectHeaderRevealTarget(repoId, repoMap, projectGrouping) {
	return getProjectGroupingForRepo(repoId, repoMap, buildProjectGroupingIndex(projectGrouping));
}
function addRepoIdToGroup(group, repoId) {
	group.repoIds.add(repoId);
}
const PR_GROUP_ORDER = [
	"done",
	"in-review",
	"in-progress",
	"closed"
];
const PR_GROUP_META = {
	done: {
		get label() {
			return translate("auto.components.sidebar.worktree.list.groups.5076efc3d2", "Done");
		},
		icon: ConductorDoneIcon,
		tone: "text-[#c7a594]"
	},
	"in-review": {
		get label() {
			return translate("auto.components.sidebar.worktree.list.groups.6798dc7c94", "In review");
		},
		icon: ConductorReviewIcon,
		tone: "text-[#16a34a]"
	},
	"in-progress": {
		get label() {
			return translate("auto.components.sidebar.worktree.list.groups.7c2f009786", "In progress");
		},
		icon: ConductorProgressIcon,
		tone: "text-[#d4a300]"
	},
	closed: {
		get label() {
			return translate("auto.components.sidebar.worktree.list.groups.682ed5d551", "Closed");
		},
		icon: CircleX,
		tone: "text-zinc-600 dark:text-zinc-300"
	}
};
const PROJECT_GROUP_META = {
	tone: "text-foreground",
	icon: FolderTree
};
function getProjectGroupHeaderKey(groupId) {
	return groupId ? `project-group:${groupId}` : UNGROUPED_PROJECT_GROUP_KEY;
}
const PINNED_GROUP_KEY = "pinned";
const PINNED_GROUP_META = {
	get label() {
		return translate("auto.components.sidebar.worktree.list.groups.4aeefc5996", "Pinned");
	},
	tone: "text-foreground",
	icon: Pin
};
const ALL_GROUP_KEY = "all";
const ALL_GROUP_META = {
	get label() {
		return translate("auto.components.sidebar.worktree.list.groups.0ed04075b8", "All");
	},
	tone: "text-foreground",
	icon: List
};
const LINEAGE_GROUP_PREFIX = "lineage:";
function getLineageGroupKey(worktreeId) {
	return `${LINEAGE_GROUP_PREFIX}${worktreeId}`;
}
function getPRGroupKey(worktree, repoMap, prCache, settings) {
	const repo = repoMap.get(worktree.repoId);
	const branch = branchName(worktree.branch);
	const repoScopedCacheKey = repo && branch ? getGitHubPRCacheKey(repo.path, repo.id, branch, settings, repo.connectionId, repo.executionHostId, true) : "";
	const canUseLegacyPRCache = repo !== void 0 && !repo.connectionId && !repo.executionHostId;
	const legacyRepoScopedCacheKey = canUseLegacyPRCache && branch ? getLegacyGitHubPRCacheKey(repo.path, repo.id, branch) : "";
	const legacyPathScopedCacheKey = canUseLegacyPRCache && branch ? getLegacyGitHubPRCacheKey(repo.path, void 0, branch) : "";
	const pr = (prCache ? (repoScopedCacheKey ? prCache[repoScopedCacheKey] : void 0) ?? (legacyRepoScopedCacheKey ? prCache[legacyRepoScopedCacheKey] : void 0) ?? (legacyPathScopedCacheKey ? prCache[legacyPathScopedCacheKey] : void 0) : void 0)?.data;
	if (!pr) return "in-progress";
	if (pr.state === "merged") return "done";
	if (pr.state === "closed") return "closed";
	if (pr.state === "draft") return "in-progress";
	return "in-review";
}
function emitPinnedGroup(worktrees, repoMap, defaultHostId, collapsedGroups, renderedNaturalAnchorRepoIds, importedWorktreesByRepo, allowImportedFallback, result) {
	const pinned = worktrees.filter((w) => w.isPinned);
	if (pinned.length === 0) return;
	const hostWorktreeCounts = /* @__PURE__ */ new Map();
	const hostWorktreeIds = /* @__PURE__ */ new Map();
	const pinnedRepoOrder = [];
	const seenPinnedRepoIds = /* @__PURE__ */ new Set();
	for (const worktree of pinned) {
		const hostId = getWorktreeExecutionHostId(worktree, repoMap.get(worktree.repoId), defaultHostId);
		hostWorktreeCounts.set(hostId, (hostWorktreeCounts.get(hostId) ?? 0) + 1);
		const hostIds = hostWorktreeIds.get(hostId) ?? [];
		hostIds.push(worktree.id);
		hostWorktreeIds.set(hostId, hostIds);
		if (!seenPinnedRepoIds.has(worktree.repoId)) {
			pinnedRepoOrder.push(worktree.repoId);
			seenPinnedRepoIds.add(worktree.repoId);
		}
	}
	result.push({
		type: "header",
		key: PINNED_GROUP_KEY,
		label: PINNED_GROUP_META.label,
		count: pinned.length,
		tone: PINNED_GROUP_META.tone,
		icon: PINNED_GROUP_META.icon,
		hostWorktreeCounts,
		hostWorktreeIds,
		worktreeIds: pinned.map((worktree) => worktree.id)
	});
	if (collapsedGroups.has("pinned")) for (const repoId of pinnedRepoOrder) {
		const candidate = importedWorktreesByRepo.get(repoId);
		if (allowImportedFallback && candidate && !renderedNaturalAnchorRepoIds.has(repoId)) result.push(buildImportedWorktreesCardRow(candidate, "pinned-fallback"));
	}
	else {
		const lastPinnedIndexByRepoId = /* @__PURE__ */ new Map();
		pinned.forEach((worktree, index) => lastPinnedIndexByRepoId.set(worktree.repoId, index));
		for (const [index, worktree] of pinned.entries()) {
			result.push(buildWorktreeRow(worktree, repoMap, {
				rowKey: `${PINNED_GROUP_KEY}:${worktree.id}`,
				sectionKey: PINNED_GROUP_KEY,
				depth: 0,
				groupDepth: 0,
				lineageTrail: [],
				isLastLineageChild: false,
				lineageChildCount: 0,
				lineageCollapsed: false
			}));
			const candidate = importedWorktreesByRepo.get(worktree.repoId);
			if (allowImportedFallback && candidate && !renderedNaturalAnchorRepoIds.has(worktree.repoId) && lastPinnedIndexByRepoId.get(worktree.repoId) === index) result.push(buildImportedWorktreesCardRow(candidate, "pinned-fallback"));
		}
	}
}
function buildImportedWorktreesCardRow(candidate, placement) {
	return {
		type: "imported-worktrees-card",
		key: `imported-worktrees-card:${placement}:${candidate.repo.id}`,
		repo: candidate.repo,
		hiddenWorktrees: candidate.hiddenWorktrees,
		placement
	};
}
function buildNewExternalWorktreesInboxRow(candidate) {
	return {
		type: "new-external-worktrees-inbox",
		key: `new-external-worktrees-inbox:${candidate.repo.id}`,
		repo: candidate.repo,
		inboxWorktrees: candidate.inboxWorktrees
	};
}
function buildWorktreeRow(worktree, repoMap, options) {
	return {
		type: "item",
		rowKey: options.rowKey,
		sectionKey: options.sectionKey,
		worktree,
		repo: repoMap.get(worktree.repoId),
		depth: options.depth,
		groupDepth: options.groupDepth,
		lineageTrail: options.lineageTrail,
		isLastLineageChild: options.isLastLineageChild,
		lineageChildCount: options.lineageChildCount,
		...options.hostContextLabel ? { hostContextLabel: options.hostContextLabel } : {},
		...options.lineageChildCount > 0 ? { lineageGroupKey: getLineageGroupKey(worktree.id) } : {},
		...options.lineageChildCount > 0 ? { lineageCollapsed: options.lineageCollapsed } : {}
	};
}
function appendWorktreeRows(result, worktrees, repoMap, lineageById, worktreeMap, options) {
	const { nestLineage, collapsedGroups, groupDepth, sectionKey, hostContextLabelByRepoId, hostContextLabelByWorktreeId, cyclicLineageIds } = options;
	if (!nestLineage) {
		for (const worktree of worktrees) result.push(buildWorktreeRow(worktree, repoMap, {
			rowKey: `${sectionKey}:${worktree.id}`,
			sectionKey,
			depth: 0,
			groupDepth,
			lineageTrail: [],
			isLastLineageChild: false,
			lineageChildCount: 0,
			lineageCollapsed: false,
			hostContextLabel: hostContextLabelByWorktreeId?.get(worktree.id) ?? hostContextLabelByRepoId?.get(worktree.repoId)
		}));
		return;
	}
	const visibleIds = new Set(worktrees.map((worktree) => worktree.id));
	const childrenByParentId = /* @__PURE__ */ new Map();
	const childIds = /* @__PURE__ */ new Set();
	for (const worktree of worktrees) {
		const lineage = getLineageRenderInfo(worktree, lineageById, worktreeMap, cyclicLineageIds);
		if (lineage.state !== "valid" || !visibleIds.has(lineage.parent.id)) continue;
		childIds.add(worktree.id);
		const children = childrenByParentId.get(lineage.parent.id) ?? [];
		children.push(worktree);
		childrenByParentId.set(lineage.parent.id, children);
	}
	const emitted = /* @__PURE__ */ new Set();
	const emit = (worktree, depth, lineageTrail, isLastChild) => {
		if (emitted.has(worktree.id)) return;
		const children = childrenByParentId.get(worktree.id) ?? [];
		const lineageGroupKey = getLineageGroupKey(worktree.id);
		const lineageCollapsed = collapsedGroups.has(lineageGroupKey);
		emitted.add(worktree.id);
		result.push(buildWorktreeRow(worktree, repoMap, {
			rowKey: `${sectionKey}:${worktree.id}`,
			sectionKey,
			depth,
			groupDepth,
			lineageTrail,
			isLastLineageChild: isLastChild,
			lineageChildCount: children.length,
			lineageCollapsed,
			hostContextLabel: hostContextLabelByWorktreeId?.get(worktree.id) ?? hostContextLabelByRepoId?.get(worktree.repoId)
		}));
		if (lineageCollapsed) return;
		children.forEach((child, index) => {
			emit(child, depth + 1, [...lineageTrail, index < children.length - 1], index === children.length - 1);
		});
	};
	const roots = worktrees.filter((worktree) => !childIds.has(worktree.id));
	for (const [index, worktree] of roots.entries()) emit(worktree, 0, [], index === roots.length - 1);
	if (roots.length === 0) {
		for (const worktree of worktrees) if (!emitted.has(worktree.id)) emit(worktree, 0, [], true);
	}
}
function getRepoHostLabel(repoId, repoMap, projectIndex, hostLabelById) {
	const setup = projectIndex?.setupByRepoId.get(repoId);
	if (setup) return hostLabelById?.get(setup.hostId) ?? getExecutionHostLabel(setup.hostId);
	const repo = repoMap.get(repoId);
	if (!repo) return null;
	const hostId = getRepoExecutionHostId(repo);
	return hostLabelById?.get(hostId) ?? getExecutionHostLabel(hostId);
}
function getMixedHostContextLabels(group, repoMap, projectIndex, hostLabelById) {
	const labelsByRepoId = /* @__PURE__ */ new Map();
	const uniqueLabels = /* @__PURE__ */ new Set();
	for (const repoId of group.repoIds) {
		const label = getRepoHostLabel(repoId, repoMap, projectIndex, hostLabelById);
		if (!label) continue;
		labelsByRepoId.set(repoId, label);
		uniqueLabels.add(label);
	}
	return uniqueLabels.size > 1 ? labelsByRepoId : void 0;
}
function getMixedWorktreeHostContextLabels(worktrees, repoMap, hostLabelById, defaultHostId) {
	const labelsByWorktreeId = /* @__PURE__ */ new Map();
	const uniqueHostIds = /* @__PURE__ */ new Set();
	for (const worktree of worktrees) {
		const hostId = getWorktreeExecutionHostId(worktree, repoMap.get(worktree.repoId), defaultHostId);
		uniqueHostIds.add(hostId);
		labelsByWorktreeId.set(worktree.id, hostLabelById?.get(hostId) ?? getExecutionHostLabel(hostId));
	}
	return uniqueHostIds.size > 1 ? labelsByWorktreeId : void 0;
}
function getHostWorktreeCounts(worktrees, repoMap, defaultHostId) {
	if (worktrees.length === 0) return;
	const counts = /* @__PURE__ */ new Map();
	const seenWorktreeIds = /* @__PURE__ */ new Set();
	for (const worktree of worktrees) {
		if (seenWorktreeIds.has(worktree.id)) continue;
		seenWorktreeIds.add(worktree.id);
		const hostId = getWorktreeExecutionHostId(worktree, repoMap.get(worktree.repoId), defaultHostId);
		counts.set(hostId, (counts.get(hostId) ?? 0) + 1);
	}
	return counts;
}
function getHostWorktreeIds(worktrees, repoMap, defaultHostId) {
	if (worktrees.length === 0) return;
	const idsByHost = /* @__PURE__ */ new Map();
	const seenWorktreeIds = /* @__PURE__ */ new Set();
	for (const worktree of worktrees) {
		if (seenWorktreeIds.has(worktree.id)) continue;
		seenWorktreeIds.add(worktree.id);
		const hostId = getWorktreeExecutionHostId(worktree, repoMap.get(worktree.repoId), defaultHostId);
		const ids = idsByHost.get(hostId) ?? [];
		ids.push(worktree.id);
		idsByHost.set(hostId, ids);
	}
	return idsByHost;
}
function getRenderedNaturalAnchorRepoIds({ groupBy, worktrees, repoMap, prCache, collapsedGroups, workspaceStatuses, settings, projectGrouping }) {
	const renderedRepoIds = /* @__PURE__ */ new Set();
	if (groupBy === "none") {
		if (!collapsedGroups.has("all")) for (const worktree of worktrees) renderedRepoIds.add(worktree.repoId);
		return renderedRepoIds;
	}
	if (groupBy === "repo") {
		for (const worktree of worktrees) renderedRepoIds.add(worktree.repoId);
		return renderedRepoIds;
	}
	for (const worktree of worktrees) {
		const groupKey = getGroupKeyForWorktree(groupBy, worktree, repoMap, prCache, workspaceStatuses, settings, projectGrouping);
		if (groupKey && !collapsedGroups.has(groupKey)) renderedRepoIds.add(worktree.repoId);
	}
	return renderedRepoIds;
}
function orderMainWorktreeFirst(worktrees) {
	const mainWorktrees = worktrees.filter((worktree) => worktree.isMainWorktree);
	if (mainWorktrees.length === 0) return worktrees;
	return [...mainWorktrees, ...worktrees.filter((worktree) => !worktree.isMainWorktree)];
}
function withRepoSectionDisplayLabels(entries) {
	const repos = entries.map((entry) => entry[1].repo).filter((repo) => repo !== void 0);
	if (repos.length < 2) return [...entries];
	const labelsByPath = getRepoDisplayLabelsByPath(repos);
	return entries.map(([key, group]) => [key, group.repo ? {
		...group,
		label: labelsByPath.get(getRepoDisplayLabelKey(group.repo)) ?? group.label
	} : group]);
}
function recentRankForEntry(entry) {
	let max = Number.NEGATIVE_INFINITY;
	for (const worktree of entry[1].items) if (worktree.lastActivityAt > max) max = worktree.lastActivityAt;
	if (max !== Number.NEGATIVE_INFINITY) return {
		hasActivity: true,
		ts: max
	};
	const addedAt = entry[1].repo?.addedAt;
	return {
		hasActivity: false,
		ts: typeof addedAt === "number" ? addedAt : Number.NEGATIVE_INFINITY
	};
}
function compareRecentRank(a, b) {
	if (a.hasActivity !== b.hasActivity) return a.hasActivity ? -1 : 1;
	return b.ts - a.ts;
}
function manualRankForEntry(entry, repoOrder) {
	const key = entry[0];
	const repoIds = entry[1].repoIds.size > 0 ? [...entry[1].repoIds] : [key.startsWith("repo:") ? key.slice(5) : key];
	let rank = Number.POSITIVE_INFINITY;
	for (const repoId of repoIds) {
		const repoRank = repoOrder?.get(repoId);
		if (repoRank !== void 0 && repoRank < rank) rank = repoRank;
	}
	return rank;
}
function getManualOrderAnchorRepo(group, repoMap, repoOrder) {
	let anchor = group.repo;
	let anchorRank = anchor ? repoOrder?.get(anchor.id) ?? Number.POSITIVE_INFINITY : void 0;
	for (const repoId of group.repoIds) {
		const repo = repoMap.get(repoId);
		if (!repo) continue;
		const rank = repoOrder?.get(repoId) ?? Number.POSITIVE_INFINITY;
		if (!anchor || rank < (anchorRank ?? Number.POSITIVE_INFINITY)) {
			anchor = repo;
			anchorRank = rank;
		}
	}
	return anchor;
}
function sortProjectEntries(entries, projectOrderBy, repoOrder) {
	if (projectOrderBy === "recent") return [...entries].sort((a, b) => {
		const byRecent = compareRecentRank(recentRankForEntry(a), recentRankForEntry(b));
		if (byRecent !== 0) return byRecent;
		const ma = manualRankForEntry(a, repoOrder);
		const mb = manualRankForEntry(b, repoOrder);
		if (ma !== mb) return ma - mb;
		return a[1].label.localeCompare(b[1].label);
	});
	if (!repoOrder) return entries;
	return [...entries].sort((a, b) => {
		const ra = manualRankForEntry(a, repoOrder);
		const rb = manualRankForEntry(b, repoOrder);
		if (ra !== rb) return ra - rb;
		return a[1].label.localeCompare(b[1].label);
	});
}
function buildRows(groupBy, worktrees, repoMap, prCache, collapsedGroups, repoOrder, workspaceStatuses = cloneDefaultWorkspaceStatuses(), projectOrderBy = "manual", lineageById = {}, worktreeMap = new Map(worktrees.map((worktree) => [worktree.id, worktree])), nestLineage = false, settings, projectGroups = [], placeholderRepoIds = /* @__PURE__ */ new Set(), importedWorktreesByRepo = /* @__PURE__ */ new Map(), newExternalWorktreesInboxByRepo = /* @__PURE__ */ new Map(), pendingCreations = [], projectGrouping, folderWorkspaces = [], hostLabelById, defaultHostId = LOCAL_EXECUTION_HOST_ID, pinnedDisplayPolicy = getPinnedWorktreeDisplayPolicy(settings)) {
	const result = [];
	const projectIndex = buildProjectGroupingIndex(projectGrouping);
	const cyclicLineageIds = nestLineage ? getCyclicProjectedWorktreeLineageIds(lineageById, worktreeMap) : /* @__PURE__ */ new Set();
	const pendingByRepo = /* @__PURE__ */ new Map();
	for (const creation of pendingCreations) {
		const list = pendingByRepo.get(creation.repoId) ?? [];
		list.push(creation);
		pendingByRepo.set(creation.repoId, list);
	}
	if (groupBy !== "repo" && pendingCreations.length > 0) for (const creation of pendingCreations) result.push(buildPendingCreationRow(creation, repoMap));
	const naturalWorktrees = pinnedDisplayPolicy === "duplicate-in-groups" ? worktrees : worktrees.filter((worktree) => !worktree.isPinned);
	const mixedWorktreeHostContextLabels = getMixedWorktreeHostContextLabels(naturalWorktrees, repoMap, hostLabelById, defaultHostId);
	emitPinnedGroup(worktrees, repoMap, defaultHostId, collapsedGroups, getRenderedNaturalAnchorRepoIds({
		groupBy,
		worktrees: naturalWorktrees,
		repoMap,
		prCache,
		collapsedGroups,
		workspaceStatuses,
		settings,
		projectGrouping
	}), importedWorktreesByRepo, groupBy !== "repo", result);
	if (groupBy === "none") {
		if (naturalWorktrees.length > 0) {
			result.push({
				type: "header",
				key: "all",
				label: ALL_GROUP_META.label,
				count: naturalWorktrees.length,
				tone: ALL_GROUP_META.tone,
				icon: ALL_GROUP_META.icon,
				hostWorktreeCounts: getHostWorktreeCounts(naturalWorktrees, repoMap, defaultHostId),
				hostWorktreeIds: getHostWorktreeIds(naturalWorktrees, repoMap, defaultHostId),
				worktreeIds: naturalWorktrees.map((worktree) => worktree.id)
			});
			if (!collapsedGroups.has("all")) appendWorktreeRows(result, naturalWorktrees, repoMap, lineageById, worktreeMap, {
				nestLineage,
				collapsedGroups,
				groupDepth: 0,
				sectionKey: "all",
				hostContextLabelByWorktreeId: mixedWorktreeHostContextLabels,
				cyclicLineageIds
			});
		}
		return result;
	}
	const grouped = /* @__PURE__ */ new Map();
	for (const w of naturalWorktrees) {
		let key;
		let label;
		let repo;
		if (groupBy === "repo") {
			const grouping = getProjectGroupingForRepo(w.repoId, repoMap, projectIndex);
			key = grouping.key;
			label = grouping.label;
			repo = grouping.repo;
		} else if (groupBy === "workspace-status") {
			const workspaceStatus = getWorkspaceStatus(w, workspaceStatuses);
			key = getWorkspaceStatusGroupKey(workspaceStatus);
			label = workspaceStatuses.find((status) => status.id === workspaceStatus)?.label ?? workspaceStatus;
		} else {
			const prGroup = getPRGroupKey(w, repoMap, prCache, settings);
			key = `pr:${prGroup}`;
			label = PR_GROUP_META[prGroup].label;
		}
		if (!grouped.has(key)) grouped.set(key, {
			label,
			items: [],
			repo,
			repoIds: /* @__PURE__ */ new Set()
		});
		const group = grouped.get(key);
		group.items.push(w);
		addRepoIdToGroup(group, w.repoId);
	}
	if (groupBy === "repo") for (const repoId of placeholderRepoIds) {
		const grouping = getProjectGroupingForRepo(repoId, repoMap, projectIndex);
		if (!grouping.repo) continue;
		const key = grouping.key;
		if (!grouped.has(key)) grouped.set(key, {
			label: grouping.label,
			items: [],
			repo: grouping.repo,
			repoIds: new Set([repoId])
		});
		else addRepoIdToGroup(grouped.get(key), repoId);
	}
	if (groupBy === "repo") for (const [repoId, candidate] of importedWorktreesByRepo) {
		const grouping = getProjectGroupingForRepo(repoId, repoMap, projectIndex);
		const key = grouping.key;
		if (!grouped.has(key)) grouped.set(key, {
			label: grouping.label,
			items: [],
			repo: grouping.repo ?? candidate.repo,
			repoIds: new Set([repoId])
		});
		else if (grouped.has(key)) addRepoIdToGroup(grouped.get(key), repoId);
	}
	if (groupBy === "repo") for (const [repoId, candidate] of newExternalWorktreesInboxByRepo) {
		const grouping = getProjectGroupingForRepo(repoId, repoMap, projectIndex);
		const key = grouping.key;
		if (!grouped.has(key)) grouped.set(key, {
			label: grouping.label,
			items: [],
			repo: grouping.repo ?? candidate.repo,
			repoIds: new Set([repoId])
		});
		else if (grouped.has(key)) addRepoIdToGroup(grouped.get(key), repoId);
	}
	if (groupBy === "repo") for (const repoId of pendingByRepo.keys()) {
		const grouping = getProjectGroupingForRepo(repoId, repoMap, projectIndex);
		const key = grouping.key;
		if (!grouped.has(key)) grouped.set(key, {
			label: grouping.label,
			items: [],
			repo: grouping.repo,
			repoIds: new Set([repoId])
		});
		else addRepoIdToGroup(grouped.get(key), repoId);
	}
	const orderedGroups = [];
	if (groupBy === "pr-status") for (const prGroup of PR_GROUP_ORDER) {
		const key = `pr:${prGroup}`;
		const group = grouped.get(key);
		if (group) orderedGroups.push([key, group]);
	}
	else if (groupBy === "workspace-status") for (const status of workspaceStatuses) {
		const key = getWorkspaceStatusGroupKey(status.id);
		const group = grouped.get(key);
		if (group) orderedGroups.push([key, group]);
	}
	else {
		for (const group of grouped.values()) group.repo = getManualOrderAnchorRepo(group, repoMap, repoOrder);
		const entries = sortProjectEntries(Array.from(grouped.entries()), projectOrderBy, repoOrder);
		for (const entry of entries) orderedGroups.push(entry);
	}
	const appendOrderedGroups = (groupsToAppend, projectGroupDepth = 0) => {
		for (const [key, group] of groupsToAppend) {
			const isCollapsed = collapsedGroups.has(key);
			const repo = group.repo;
			const header = groupBy === "repo" ? {
				type: "header",
				key,
				label: group.label,
				count: group.items.length,
				tone: PROJECT_GROUP_META.tone,
				icon: PROJECT_GROUP_META.icon,
				repo,
				projectGroupDepth
			} : groupBy === "workspace-status" ? (() => {
				const workspaceStatus = getWorkspaceStatusFromGroupKey(key, workspaceStatuses) ?? workspaceStatuses[0]?.id ?? "in-progress";
				const definition = workspaceStatuses.find((status) => status.id === workspaceStatus);
				const meta = getWorkspaceStatusVisualMeta(definition ?? workspaceStatus);
				return {
					type: "header",
					key,
					label: definition?.label ?? workspaceStatus,
					count: group.items.length,
					tone: meta.tone,
					icon: meta.icon,
					hostWorktreeCounts: getHostWorktreeCounts(group.items, repoMap, defaultHostId),
					hostWorktreeIds: getHostWorktreeIds(group.items, repoMap, defaultHostId),
					worktreeIds: group.items.map((worktree) => worktree.id)
				};
			})() : (() => {
				const meta = PR_GROUP_META[key.replace(/^pr:/, "")];
				return {
					type: "header",
					key,
					label: meta.label,
					count: group.items.length,
					tone: meta.tone,
					icon: meta.icon,
					hostWorktreeCounts: getHostWorktreeCounts(group.items, repoMap, defaultHostId),
					hostWorktreeIds: getHostWorktreeIds(group.items, repoMap, defaultHostId),
					worktreeIds: group.items.map((worktree) => worktree.id)
				};
			})();
			result.push(header);
			if (!isCollapsed) {
				if (groupBy === "repo") {
					const repoIds = group.repoIds.size > 0 ? [...group.repoIds] : repo ? [repo.id] : key.startsWith("repo:") ? [key.slice(5)] : [];
					for (const repoId of repoIds) {
						const candidate = importedWorktreesByRepo.get(repoId);
						if (candidate) result.push(buildImportedWorktreesCardRow(candidate, "repo-group"));
					}
					for (const repoId of repoIds) {
						const candidate = newExternalWorktreesInboxByRepo.get(repoId);
						if (candidate) result.push(buildNewExternalWorktreesInboxRow(candidate));
					}
					for (const repoId of repoIds) for (const creation of pendingByRepo.get(repoId) ?? []) result.push(buildPendingCreationRow(creation, repoMap));
				}
				const items = groupBy === "repo" ? orderMainWorktreeFirst(group.items) : group.items;
				const hostContextLabelByRepoId = groupBy === "repo" ? getMixedHostContextLabels(group, repoMap, projectIndex, hostLabelById) : void 0;
				const hostContextLabelByWorktreeId = groupBy === "repo" ? void 0 : mixedWorktreeHostContextLabels;
				if (groupBy === "repo") appendWorktreeRows(result, items, repoMap, lineageById, worktreeMap, {
					nestLineage,
					collapsedGroups,
					groupDepth: projectGroupDepth,
					sectionKey: key,
					hostContextLabelByRepoId,
					hostContextLabelByWorktreeId,
					cyclicLineageIds
				});
				else appendWorktreeRows(result, items, repoMap, lineageById, worktreeMap, {
					nestLineage,
					collapsedGroups,
					groupDepth: projectGroupDepth,
					sectionKey: key,
					hostContextLabelByRepoId,
					hostContextLabelByWorktreeId,
					cyclicLineageIds
				});
			}
		}
	};
	if (groupBy !== "repo" || projectGroups.length === 0) {
		appendOrderedGroups(groupBy === "repo" ? withRepoSectionDisplayLabels(orderedGroups) : orderedGroups);
		return result;
	}
	const groupByProjectGroupId = /* @__PURE__ */ new Map();
	for (const entry of orderedGroups) {
		const projectGroupId = entry[1].repo?.projectGroupId ?? null;
		const list = groupByProjectGroupId.get(projectGroupId) ?? [];
		list.push(entry);
		groupByProjectGroupId.set(projectGroupId, list);
	}
	const sortRepoEntriesWithinGroup = (entries) => {
		if (projectOrderBy === "recent") return [...entries].sort((left, right) => compareRecentRank(recentRankForEntry(left), recentRankForEntry(right)));
		return [...entries].sort((left, right) => {
			return getEffectiveProjectGroupManualRank(left[1].repo, repoOrder) - getEffectiveProjectGroupManualRank(right[1].repo, repoOrder);
		});
	};
	const projectGroupsById = new Map(projectGroups.map((group) => [group.id, group]));
	const folderWorkspacesByProjectGroupId = /* @__PURE__ */ new Map();
	for (const workspace of folderWorkspaces) {
		if (!projectGroupsById.get(workspace.projectGroupId)?.parentPath) continue;
		const list = folderWorkspacesByProjectGroupId.get(workspace.projectGroupId) ?? [];
		list.push(workspace);
		folderWorkspacesByProjectGroupId.set(workspace.projectGroupId, list);
	}
	for (const list of folderWorkspacesByProjectGroupId.values()) list.sort((left, right) => {
		const leftOrder = left.manualOrder ?? left.sortOrder;
		return (right.manualOrder ?? right.sortOrder) - leftOrder || left.name.localeCompare(right.name);
	});
	const childGroupsByParentId = /* @__PURE__ */ new Map();
	for (const group of projectGroups) {
		const parentId = group.parentGroupId && projectGroupsById.has(group.parentGroupId) ? group.parentGroupId : null;
		const children = childGroupsByParentId.get(parentId) ?? [];
		children.push(group);
		childGroupsByParentId.set(parentId, children);
	}
	for (const groups of childGroupsByParentId.values()) groups.sort((left, right) => left.tabOrder - right.tabOrder || left.name.localeCompare(right.name));
	const getProjectGroupSubtreeCount = (groupId) => {
		const directCount = groupByProjectGroupId.get(groupId)?.length ?? 0;
		const folderWorkspaceCount = folderWorkspacesByProjectGroupId.get(groupId)?.length ?? 0;
		return (childGroupsByParentId.get(groupId) ?? []).reduce((count, child) => count + getProjectGroupSubtreeCount(child.id), directCount + folderWorkspaceCount);
	};
	const appendProjectGroup = (projectGroup, depth) => {
		const repoEntries = sortRepoEntriesWithinGroup(groupByProjectGroupId.get(projectGroup.id) ?? []);
		const childGroups = childGroupsByParentId.get(projectGroup.id) ?? [];
		const key = getProjectGroupHeaderKey(projectGroup.id);
		result.push({
			type: "header",
			key,
			label: projectGroup.name,
			count: getProjectGroupSubtreeCount(projectGroup.id),
			tone: PROJECT_GROUP_META.tone,
			icon: PROJECT_GROUP_META.icon,
			projectGroup,
			projectGroupDepth: depth
		});
		if (!collapsedGroups.has(key)) {
			for (const folderWorkspace of folderWorkspacesByProjectGroupId.get(projectGroup.id) ?? []) result.push({
				type: "folder-workspace",
				key: `folder-workspace:${folderWorkspace.id}`,
				folderWorkspace,
				projectGroup,
				depth: 0,
				groupDepth: depth + 1
			});
			appendOrderedGroups(withRepoSectionDisplayLabels(repoEntries), depth + 1);
			for (const childGroup of childGroups) appendProjectGroup(childGroup, depth + 1);
		}
		groupByProjectGroupId.delete(projectGroup.id);
	};
	for (const projectGroup of childGroupsByParentId.get(null) ?? []) appendProjectGroup(projectGroup, 0);
	const remainingRepoEntries = [...groupByProjectGroupId.get(null) ?? []];
	for (const [projectGroupId, entries] of groupByProjectGroupId) {
		if (projectGroupId === null || projectGroupsById.has(projectGroupId)) continue;
		remainingRepoEntries.push(...entries);
	}
	appendOrderedGroups(withRepoSectionDisplayLabels(sortRepoEntriesWithinGroup(remainingRepoEntries)), 0);
	return result;
}
function getGroupKeyForWorktree(groupBy, worktree, repoMap, prCache, workspaceStatuses = cloneDefaultWorkspaceStatuses(), settings, projectGrouping) {
	if (groupBy === "none") return "all";
	if (groupBy === "workspace-status") return getWorkspaceStatusGroupKey(getWorkspaceStatus(worktree, workspaceStatuses));
	if (groupBy === "repo") return getProjectGroupingForRepo(worktree.repoId, repoMap, buildProjectGroupingIndex(projectGrouping)).key;
	return `pr:${getPRGroupKey(worktree, repoMap, prCache, settings)}`;
}
function getGroupKeysForWorktree(groupBy, worktree, repoMap, prCache, workspaceStatuses = cloneDefaultWorkspaceStatuses(), settings, projectGroups = [], projectGrouping) {
	const groupKey = getGroupKeyForWorktree(groupBy, worktree, repoMap, prCache, workspaceStatuses, settings, projectGrouping);
	if (!groupKey) return [];
	if (groupBy !== "repo") return [groupKey];
	const repo = repoMap.get(worktree.repoId);
	const groupIds = [];
	const groupsById = new Map(projectGroups.map((group) => [group.id, group]));
	const visited = /* @__PURE__ */ new Set();
	let currentGroupId = repo?.projectGroupId ?? null;
	while (currentGroupId && !visited.has(currentGroupId)) {
		const group = groupsById.get(currentGroupId);
		if (!group) break;
		visited.add(currentGroupId);
		groupIds.unshift(currentGroupId);
		const parentId = group.parentGroupId ?? null;
		currentGroupId = parentId && groupsById.has(parentId) ? parentId : null;
	}
	return [...groupIds.map((id) => getProjectGroupHeaderKey(id)), groupKey];
}
function getRepoHostId(repo, defaultHostId) {
	if (repo?.connectionId || repo?.executionHostId) return getRepoExecutionHostId(repo);
	return defaultHostId;
}
function getSshHostId(connectionId) {
	return `ssh:${encodeURIComponent(connectionId)}`;
}
function getFolderWorkspaceHostId(folderWorkspace, projectGroup, defaultHostId) {
	const connectionId = folderWorkspace.connectionId ?? projectGroup.connectionId;
	return connectionId ? getSshHostId(connectionId) : defaultHostId;
}
function getRowHostId(row, defaultHostId) {
	switch (row.type) {
		case "item": return getWorktreeExecutionHostId(row.worktree, row.repo, defaultHostId);
		case "pending-creation":
		case "imported-worktrees-card":
		case "new-external-worktrees-inbox": return getRepoHostId(row.repo, defaultHostId);
		case "folder-workspace": return getFolderWorkspaceHostId(row.folderWorkspace, row.projectGroup, defaultHostId);
		case "header": return row.repo ? getRepoHostId(row.repo, defaultHostId) : null;
	}
}
function getFallbackHost(hostId) {
	const isLocal = hostId === LOCAL_EXECUTION_HOST_ID;
	return {
		id: hostId,
		kind: isLocal ? "local" : hostId.startsWith("ssh:") ? "ssh" : "runtime",
		label: isLocal ? getLocalExecutionHostLabel() : hostId,
		detail: isLocal ? "This computer" : "Host",
		health: isLocal ? "local" : "available"
	};
}
function countWorktreeRows(rows) {
	let count = 0;
	const seenWorktreeIds = /* @__PURE__ */ new Set();
	let pendingHeader = null;
	let pendingHeaderHadItems = false;
	const flushHeader = () => {
		if (pendingHeader && !pendingHeaderHadItems) if (pendingHeader.worktreeIds) {
			for (const worktreeId of pendingHeader.worktreeIds) if (!seenWorktreeIds.has(worktreeId)) {
				count += 1;
				seenWorktreeIds.add(worktreeId);
			}
		} else count += pendingHeader.count;
		pendingHeader = null;
		pendingHeaderHadItems = false;
	};
	for (const row of rows) {
		if (row.type === "header") {
			flushHeader();
			pendingHeader = row;
			continue;
		}
		if (row.type === "item") {
			if (!seenWorktreeIds.has(row.worktree.id)) {
				count += 1;
				seenWorktreeIds.add(row.worktree.id);
			}
			pendingHeaderHadItems = pendingHeader !== null;
		}
	}
	flushHeader();
	return count;
}
function localizePendingRowsForHost(rows, hostId) {
	const localized = [];
	for (const row of rows) {
		if (!row.hostWorktreeCounts) {
			localized.push(row);
			continue;
		}
		const count = row.hostWorktreeCounts.get(hostId);
		if (count !== void 0 && count > 0) localized.push({
			...row,
			count,
			hostId,
			worktreeIds: row.hostWorktreeIds?.get(hostId) ?? row.worktreeIds
		});
	}
	return localized;
}
function getPendingRowsKey(rows) {
	return rows.map((pendingRow) => `${pendingRow.key}:${pendingRow.count}:${pendingRow.worktreeIds?.join(",") ?? ""}`).join("\0");
}
function addHostSectionRows(args) {
	const visibleHostIds = args.visibleWorkspaceHostIds ?? (args.workspaceHostScope === "all" ? null : [args.workspaceHostScope]);
	if (args.preferProjectGrouping && args.workspaceHostScope === "all" && !args.visibleWorkspaceHostIds) return [...args.rows];
	if (visibleHostIds && visibleHostIds.length <= 1 || args.hostOptions.length <= 1) return [...args.rows];
	const hostOptionsById = new Map(args.hostOptions.map((host) => [host.id, host]));
	const rowsByHostId = /* @__PURE__ */ new Map();
	const globalRows = [];
	let pendingRows = [];
	let pendingRowsWereUsed = false;
	const pendingRowsKeyByHostId = /* @__PURE__ */ new Map();
	const flushUnusedPendingRows = () => {
		if (pendingRows.length === 0 || pendingRowsWereUsed) return;
		if (!pendingRows.some((row) => row.hostWorktreeCounts)) {
			globalRows.push(...pendingRows);
			return;
		}
		for (const row of pendingRows) for (const [hostId, count] of row.hostWorktreeCounts ?? []) {
			if (count <= 0) continue;
			const hostRows = rowsByHostId.get(hostId) ?? [];
			const hostIds = row.hostWorktreeIds?.get(hostId);
			hostRows.push({
				...row,
				count,
				hostId,
				worktreeIds: hostIds ?? row.worktreeIds
			});
			rowsByHostId.set(hostId, hostRows);
		}
	};
	for (const row of args.rows) {
		const rowHostId = getRowHostId(row, args.defaultHostId);
		if (rowHostId) {
			const hostRows = rowsByHostId.get(rowHostId) ?? [];
			if (pendingRows.length > 0) {
				const localizedPendingRows = localizePendingRowsForHost(pendingRows, rowHostId);
				const pendingRowsKey = getPendingRowsKey(localizedPendingRows);
				if (localizedPendingRows.length > 0 && pendingRowsKeyByHostId.get(rowHostId) !== pendingRowsKey) {
					hostRows.push(...localizedPendingRows);
					pendingRowsKeyByHostId.set(rowHostId, pendingRowsKey);
				}
				pendingRowsWereUsed = pendingRowsWereUsed || localizedPendingRows.length > 0;
			}
			hostRows.push(row);
			rowsByHostId.set(rowHostId, hostRows);
			continue;
		}
		if (row.type === "header") {
			flushUnusedPendingRows();
			pendingRows = [row];
			pendingRowsWereUsed = false;
		} else globalRows.push(row);
	}
	flushUnusedPendingRows();
	const hostOrder = [];
	for (const host of args.hostOptions) if (rowsByHostId.has(host.id)) hostOrder.push(host.id);
	for (const hostId of rowsByHostId.keys()) if (!hostOptionsById.has(hostId)) hostOrder.push(hostId);
	if (rowsByHostId.size <= 1) return [...args.rows];
	const result = [...globalRows];
	for (const hostId of hostOrder) {
		const hostRows = rowsByHostId.get(hostId);
		if (!hostRows || hostRows.length === 0) continue;
		const host = hostOptionsById.get(hostId) ?? getFallbackHost(hostId);
		const collapsed = args.forceCollapseHosts || (args.collapsedHostKeys?.has(`host:${host.id}`) ?? false);
		result.push({
			type: "host-header",
			key: `host:${host.id}`,
			hostId: host.id,
			kind: host.kind,
			label: host.label,
			detail: host.detail,
			health: host.health,
			compatibility: host.compatibility,
			connectionStatus: host.connectionStatus,
			collapsed,
			count: countWorktreeRows(hostRows)
		});
		if (!collapsed) result.push(...hostRows);
	}
	return result;
}
function orderHostSectionOptions(hostOptions, workspaceHostOrder = []) {
	if (workspaceHostOrder.length === 0 || hostOptions.length <= 1) return [...hostOptions];
	const hostById = new Map(hostOptions.map((host) => [host.id, host]));
	const ordered = [];
	const seen = /* @__PURE__ */ new Set();
	for (const hostId of workspaceHostOrder) {
		const host = hostById.get(hostId);
		if (!host || seen.has(host.id)) continue;
		ordered.push(host);
		seen.add(host.id);
	}
	for (const host of hostOptions) {
		if (seen.has(host.id)) continue;
		ordered.push(host);
	}
	return ordered;
}
function normalizeHostPart(value) {
	const trimmed = value?.trim();
	return trimmed ? trimmed : null;
}
function runtimeCompatibility(status) {
	if (!status) return null;
	return evaluateRuntimeCompat({
		clientProtocolVersion: 3,
		minCompatibleServerProtocolVersion: 2,
		serverProtocolVersion: status.runtimeProtocolVersion ?? status.protocolVersion,
		serverMinCompatibleClientProtocolVersion: status.minCompatibleRuntimeClientVersion ?? status.minCompatibleMobileVersion
	});
}
function runtimeHealth(status, compatibility) {
	if (!status) return "disconnected";
	if (!compatibility) return "available";
	return compatibility.kind === "blocked" ? "blocked" : "available";
}
function runtimeControlHealth(remoteControl) {
	switch (remoteControl?.state) {
		case "awaiting_authenticated":
		case "awaiting_ready":
		case "reconnecting": return "connecting";
		case "closed": return remoteControl.lastError ? "error" : "disconnected";
		case "ready": return null;
		case void 0: return null;
	}
}
function sshHealth(state) {
	switch (state?.status) {
		case "connected": return "available";
		case "connecting":
		case "deploying-relay":
		case "reconnecting": return "connecting";
		case "auth-failed":
		case "error":
		case "reconnection-failed": return "error";
		case "disconnected":
		case void 0: return "disconnected";
	}
}
function setHost(hosts, entry) {
	const existing = hosts.get(entry.id);
	if (!existing) {
		hosts.set(entry.id, entry);
		return;
	}
	if (existing.health !== "disconnected") return;
	hosts.set(entry.id, {
		...entry,
		label: existing.label,
		source: existing.source ?? entry.source
	});
}
function addRuntimeHost(hosts, environmentId, label, source, statusByEnvironmentId) {
	const hostId = toRuntimeExecutionHostId(environmentId);
	const runtimeStatus = statusByEnvironmentId?.get(environmentId);
	const status = runtimeStatus?.status;
	const compatibility = runtimeCompatibility(status);
	setHost(hosts, {
		id: hostId,
		kind: "runtime",
		label,
		detail: "Orca server",
		health: runtimeControlHealth(status?.remoteControl) ?? runtimeHealth(status, compatibility),
		compatibility: compatibility ?? void 0,
		capabilities: status?.capabilities,
		appVersion: runtimeStatus?.appVersion ?? status?.appVersion ?? null,
		protocolVersion: status?.runtimeProtocolVersion ?? status?.protocolVersion ?? null,
		minCompatibleClientVersion: status?.minCompatibleRuntimeClientVersion ?? status?.minCompatibleMobileVersion ?? null,
		platform: status?.hostPlatform ?? null,
		remoteControlState: status?.remoteControl ?? null,
		...source ? { source } : {}
	});
}
function buildExecutionHostRegistry(args) {
	const hosts = /* @__PURE__ */ new Map();
	hosts.set(LOCAL_EXECUTION_HOST_ID, {
		id: LOCAL_EXECUTION_HOST_ID,
		kind: "local",
		label: getLocalExecutionHostLabel(),
		detail: "This computer",
		health: "local"
	});
	for (const environment of args.runtimeEnvironments ?? []) {
		const environmentId = normalizeHostPart(environment.id);
		if (!environmentId) continue;
		addRuntimeHost(hosts, environmentId, normalizeHostPart(environment.name) ?? environmentId, environment.source, args.runtimeStatusByEnvironmentId);
	}
	for (const environmentId of args.runtimeStatusByEnvironmentId?.keys() ?? []) addRuntimeHost(hosts, environmentId, environmentId, void 0, args.runtimeStatusByEnvironmentId);
	const parsedFocusedHost = parseExecutionHostId(getSettingsFocusedExecutionHostId(args.settings));
	if (parsedFocusedHost?.kind === "runtime" && args.hostSource !== "configured-only") addRuntimeHost(hosts, parsedFocusedHost.environmentId, parsedFocusedHost.environmentId, void 0, args.runtimeStatusByEnvironmentId);
	const sshTargetIds = /* @__PURE__ */ new Set();
	if (args.hostSource !== "configured-only") for (const repo of args.repos) {
		const parsedHost = parseExecutionHostId(repo.executionHostId);
		if (parsedHost?.kind === "runtime") addRuntimeHost(hosts, parsedHost.environmentId, parsedHost.environmentId, void 0, args.runtimeStatusByEnvironmentId);
		if (parsedHost?.kind === "ssh" && !isRuntimeOwnedSshTargetId(parsedHost.targetId)) sshTargetIds.add(parsedHost.targetId);
	}
	for (const targetId of args.sshTargetLabels?.keys() ?? []) {
		const normalized = normalizeHostPart(targetId);
		if (normalized && !isRuntimeOwnedSshTargetId(normalized)) sshTargetIds.add(normalized);
	}
	if (args.hostSource !== "configured-only") for (const repo of args.repos) {
		const targetId = normalizeHostPart(repo.connectionId);
		if (targetId && !isRuntimeOwnedSshTargetId(targetId)) sshTargetIds.add(targetId);
	}
	for (const targetId of sshTargetIds) {
		const state = args.sshConnectionStates?.get(targetId);
		setHost(hosts, {
			id: toSshExecutionHostId(targetId),
			kind: "ssh",
			label: args.sshTargetLabels?.get(targetId) || targetId,
			detail: "SSH",
			health: sshHealth(state),
			connectionStatus: state?.status
		});
	}
	const overrides = args.hostLabelOverrides;
	if (!overrides || overrides.size === 0) return [...hosts.values()];
	return [...hosts.values()].map((host) => {
		const label = overrides.get(host.id);
		return label ? {
			...host,
			label
		} : host;
	});
}
function buildSidebarHostOptions(args) {
	const configuredSshTargetIds = new Set(args.sshTargetLabels.keys());
	const projectSshTargetIds = /* @__PURE__ */ new Set();
	for (const repo of args.repos) {
		if (repo.connectionId?.trim()) projectSshTargetIds.add(repo.connectionId.trim());
		if (repo.executionHostId?.startsWith("ssh:")) projectSshTargetIds.add(decodeURIComponent(repo.executionHostId.slice(4)));
	}
	const activeRuntimeHostId = args.settings?.activeRuntimeEnvironmentId?.trim() ? `runtime:${encodeURIComponent(args.settings.activeRuntimeEnvironmentId.trim())}` : null;
	return buildExecutionHostRegistry({
		repos: args.repos,
		settings: args.settings,
		sshTargetLabels: args.sshTargetLabels,
		sshConnectionStates: args.sshConnectionStates,
		runtimeEnvironments: args.runtimeEnvironments,
		runtimeStatusByEnvironmentId: args.runtimeStatusByEnvironmentId,
		hostLabelOverrides: args.hostLabelOverrides
	}).map((host) => {
		if (host.kind === "local") return {
			...host,
			presence: "local"
		};
		if (host.kind === "ssh") {
			const targetId = decodeURIComponent(host.id.slice(4));
			return {
				...host,
				presence: configuredSshTargetIds.has(targetId) ? "configured" : projectSshTargetIds.has(targetId) ? "project" : "active"
			};
		}
		return {
			...host,
			presence: host.id === activeRuntimeHostId ? "active" : "project"
		};
	});
}
function shouldShowHostScopeControls(hosts) {
	return hosts.some((host) => host.id !== LOCAL_EXECUTION_HOST_ID);
}
function buildSidebarHostScopeOptions(hosts) {
	return [{
		id: "all",
		label: translate("auto.components.sidebar.sidebarHostOptions.3e102f111c", "All hosts"),
		detail: hosts.map((host) => host.label).join(", "),
		health: "mixed"
	}, ...hosts.map((host) => ({
		id: host.id,
		label: host.label,
		detail: host.detail,
		health: host.health
	}))];
}
function getSidebarHostVisibilityLabel(visibleHostIds, hosts) {
	if (!visibleHostIds || visibleHostIds.length === hosts.length) return translate("auto.components.sidebar.sidebarHostOptions.3e102f111c", "All hosts");
	if (visibleHostIds.length === 1) return hosts.find((host) => host.id === visibleHostIds[0])?.label ?? "Hosts";
	return translate("auto.components.sidebar.sidebarHostOptions.visibleHostsCount", "{{value0}} hosts", { value0: visibleHostIds.length });
}
function getSidebarHostHealthLabel(health) {
	switch (health) {
		case "local": return "Local";
		case "available": return "Connected";
		case "connecting": return "Connecting";
		case "blocked": return "Update needed";
		case "disconnected": return "Disconnected";
		case "error": return "Needs attention";
		case "mixed": return "Mixed";
	}
}
var EDGE_ZONE_PX = 56;
var MAX_OUTSIDE_EDGE_PX = 48;
var MAX_SCROLL_SPEED_PX_PER_SECOND = 960;
var MAX_FRAME_MS = 32;
var DROP_BOUNDS_PADDING_PX = 8;
function getWorktreeSidebarDragAutoscroll(args) {
	const { point, containerRect } = args;
	if (point.clientX < containerRect.left || point.clientX > containerRect.right) return null;
	const maxScrollTop = Math.max(0, args.scrollHeight - args.clientHeight);
	if (maxScrollTop <= 0) return null;
	const scrollTop = Math.max(0, Math.min(maxScrollTop, args.scrollTop));
	const elapsedMs = Math.max(0, Math.min(MAX_FRAME_MS, args.elapsedMs));
	if (elapsedMs <= 0) return null;
	const edge = getVerticalEdgeIntensity(point.clientY, containerRect);
	if (!edge) return null;
	const nextScrollTop = Math.max(0, Math.min(maxScrollTop, scrollTop + edge.direction * edge.intensity * MAX_SCROLL_SPEED_PX_PER_SECOND * (elapsedMs / 1e3)));
	return nextScrollTop === scrollTop ? null : { scrollTop: nextScrollTop };
}
function getWorktreeSidebarBoundaryDrop(args) {
	if (args.localY < args.firstRect.top - DROP_BOUNDS_PADDING_PX) {
		if (args.firstRect.groupIndex === 0 && args.localY >= args.firstRect.top - EDGE_ZONE_PX) return {
			kind: "drop",
			dropIndex: 0,
			indicatorY: Math.max(0, args.firstRect.top - 3)
		};
		return { kind: "outside" };
	}
	if (args.localY > args.lastRect.bottom + DROP_BOUNDS_PADDING_PX) {
		const lastGroupIndex = args.sourceGroupSize - 1;
		if (args.lastRect.groupIndex === lastGroupIndex && args.localY <= args.lastRect.bottom + EDGE_ZONE_PX) return {
			kind: "drop",
			dropIndex: args.sourceGroupSize,
			indicatorY: args.lastRect.bottom + 3
		};
		return { kind: "outside" };
	}
	return { kind: "inside" };
}
function getWorktreeSidebarDragRectsForGroup(container, groupKey) {
	const containerRect = container.getBoundingClientRect();
	const rects = [];
	container.querySelectorAll("[data-worktree-drag-id]").forEach((element) => {
		if (element.getAttribute("data-worktree-drag-group-key") !== groupKey) return;
		const worktreeId = element.getAttribute("data-worktree-drag-id");
		const rawGroupIndex = element.getAttribute("data-worktree-drag-group-index");
		const groupIndex = rawGroupIndex === null ? NaN : Number(rawGroupIndex);
		if (!worktreeId || !Number.isFinite(groupIndex)) return;
		const rect = element.getBoundingClientRect();
		const virtualRow = element.closest("[data-worktree-virtual-row]");
		const virtualRowStart = getWorktreeVirtualRowStart(virtualRow);
		const top = virtualRow && virtualRowStart !== null ? virtualRowStart + rect.top - virtualRow.getBoundingClientRect().top : rect.top - containerRect.top + container.scrollTop;
		rects.push({
			worktreeId,
			groupIndex,
			top,
			bottom: top + rect.height
		});
	});
	rects.sort((a, b) => a.top - b.top);
	return rects;
}
function getWorktreeVirtualRowStart(virtualRow) {
	if (!virtualRow) return null;
	const rawStart = virtualRow.getAttribute("data-worktree-virtual-row-start");
	if (rawStart === null) return null;
	const start = Number(rawStart);
	return Number.isFinite(start) ? start : null;
}
function refreshWorktreeSidebarDragSession(args) {
	const sourceGroup = args.groups.find((group) => group.key === args.session.sourceGroupKey);
	if (!sourceGroup || !sourceGroup.worktreeIds.includes(args.session.draggingWorktreeId)) return null;
	const sourceUnitGroup = args.unitGroups.find((group) => group.key === args.session.sourceGroupKey);
	if (!sourceUnitGroup) return null;
	const sourceGroupIds = new Set(sourceGroup.worktreeIds);
	const sourceUnitIds = new Set(sourceUnitGroup.worktreeIds);
	if (args.session.reorderUnitDraggedIds.some((worktreeId) => !sourceUnitIds.has(worktreeId) && !sourceGroupIds.has(worktreeId))) return null;
	return {
		...args.session,
		rects: args.rects
	};
}
function getVerticalEdgeIntensity(clientY, containerRect) {
	if (clientY < containerRect.top - MAX_OUTSIDE_EDGE_PX) return null;
	if (clientY > containerRect.bottom + MAX_OUTSIDE_EDGE_PX) return null;
	if (clientY <= containerRect.top + EDGE_ZONE_PX) return {
		direction: -1,
		intensity: Math.min(1, (containerRect.top + EDGE_ZONE_PX - clientY) / EDGE_ZONE_PX)
	};
	if (clientY >= containerRect.bottom - EDGE_ZONE_PX) return {
		direction: 1,
		intensity: Math.min(1, (clientY - (containerRect.bottom - EDGE_ZONE_PX)) / EDGE_ZONE_PX)
	};
	return null;
}
var INDICATOR_GAP_PX = 4;
function computeWorktreeSidebarHeaderDropPreview(args) {
	if (args.rects.length === 0 || args.headerCount === 0) return null;
	const localY = args.pointerY - args.containerTop + args.scrollTop;
	if (args.contentBottom !== void 0 && localY > args.contentBottom) return null;
	const first = args.rects[0];
	const last = args.rects.at(-1);
	const lastBoundaryBottom = Math.max(last.bottom, last.sectionBottom ?? last.bottom);
	const boundaryDrop = getWorktreeSidebarBoundaryDrop({
		localY,
		firstRect: {
			worktreeId: args.getId(first),
			groupIndex: first.headerIndex,
			top: first.top,
			bottom: first.bottom
		},
		lastRect: {
			worktreeId: args.getId(last),
			groupIndex: last.headerIndex,
			top: last.top,
			bottom: lastBoundaryBottom
		},
		sourceGroupSize: args.headerCount
	});
	if (boundaryDrop.kind === "outside") return null;
	if (boundaryDrop.kind === "drop") return {
		dropIndex: boundaryDrop.dropIndex,
		dropIndicatorY: Math.max(args.scrollTop, boundaryDrop.indicatorY)
	};
	const hoveredRect = args.rects.find((rect) => localY >= rect.top && localY <= rect.bottom);
	if (hoveredRect) {
		const mid = (hoveredRect.top + hoveredRect.bottom) / 2;
		const dropIndex = localY < mid ? hoveredRect.headerIndex : hoveredRect.headerIndex + 1;
		const nextRect = localY < mid ? hoveredRect : args.rects.find((rect) => rect.headerIndex >= dropIndex);
		const indicatorY = nextRect ? Math.max(0, nextRect.top - INDICATOR_GAP_PX) : Math.max(hoveredRect.bottom, hoveredRect.sectionBottom ?? hoveredRect.bottom) + INDICATOR_GAP_PX;
		return {
			dropIndex,
			dropIndicatorY: Math.max(args.scrollTop, indicatorY)
		};
	}
	const boundary = pickNearestHeaderBoundarySlot(args.rects, localY);
	if (!boundary) return null;
	return {
		dropIndex: boundary.dropIndex,
		dropIndicatorY: Math.max(args.scrollTop, boundary.indicatorY)
	};
}
function pickNearestHeaderBoundarySlot(rects, localY) {
	let prevRect;
	let nextRect;
	for (const rect of rects) if (rect.top <= localY) prevRect = rect;
	else if (nextRect === void 0) nextRect = rect;
	const afterPrev = prevRect ? {
		dropIndex: prevRect.headerIndex + 1,
		indicatorY: Math.max(prevRect.bottom, prevRect.sectionBottom ?? prevRect.bottom) + INDICATOR_GAP_PX
	} : null;
	const beforeNext = nextRect ? {
		dropIndex: nextRect.headerIndex,
		indicatorY: Math.max(0, nextRect.top - INDICATOR_GAP_PX)
	} : null;
	if (!afterPrev) return beforeNext;
	if (!beforeNext) return afterPrev;
	return Math.abs(localY - beforeNext.indicatorY) <= Math.abs(localY - afterPrev.indicatorY) ? beforeNext : afterPrev;
}
function getProjectHeaderDragBucketKey(repo) {
	return repo.projectGroupId ? `group:${repo.projectGroupId}` : "ungrouped";
}
function getSidebarOrderedRepoHeaderIdsByBucket(rows) {
	const buckets = /* @__PURE__ */ new Map();
	for (const row of rows) {
		if (row.type !== "header" || !row.repo) continue;
		const bucketKey = getProjectHeaderDragBucketKey(row.repo);
		const list = buckets.get(bucketKey) ?? [];
		list.push(row.repo.id);
		buckets.set(bucketKey, list);
	}
	return buckets;
}
function getLogicalRepoOrderRankById(orderedRepoIds) {
	const rankById = /* @__PURE__ */ new Map();
	orderedRepoIds.forEach((repoId, index) => {
		if (!rankById.has(repoId)) rankById.set(repoId, index);
	});
	return rankById;
}
function getProjectGroupOrderForSidebarDrop(args) {
	const ordered = args.siblings.slice();
	if (ordered.length === 0) return 0;
	const getEffectiveOrder = (repo, fallbackIndex) => {
		if (!repo) return;
		return getEffectiveProjectGroupManualRank(repo, args.repoOrderRankById, fallbackIndex);
	};
	const before = getEffectiveOrder(ordered[args.dropIndex - 1], args.dropIndex - 1);
	const after = getEffectiveOrder(ordered[args.dropIndex], args.dropIndex);
	if (before === void 0 && after === void 0) return 0;
	if (before === void 0) return after !== void 0 ? after - 1 : 0;
	if (after === void 0) return before + 1;
	if (after > before) return before + (after - before) / 2;
	return before + 1;
}
function mapSidebarProjectHeaderDropIndexToSiblingInsertIndex(args) {
	const adjustedDropIndex = args.sourceIndex >= 0 && args.sidebarDropIndex > args.sourceIndex ? args.sidebarDropIndex - 1 : args.sidebarDropIndex;
	return Math.max(0, Math.min(args.siblingCount, adjustedDropIndex));
}
function getVirtualRowStart(virtualRow) {
	if (!virtualRow) return null;
	const rawStart = virtualRow.getAttribute("data-worktree-virtual-row-start");
	if (rawStart === null) return null;
	const start = Number(rawStart);
	return Number.isFinite(start) ? start : null;
}
function getOptionalNumberAttribute(element, attribute) {
	const rawValue = element.getAttribute(attribute);
	if (rawValue === null) return;
	const value = Number(rawValue);
	return Number.isFinite(value) ? value : void 0;
}
function measureProjectHeaderDragRects(container, bucketKey) {
	const containerRect = container.getBoundingClientRect();
	const rects = [];
	container.querySelectorAll("[data-repo-header-id]").forEach((element) => {
		const repoId = element.getAttribute("data-repo-header-id");
		const elementBucketKey = element.getAttribute("data-repo-header-bucket");
		const rawHeaderIndex = element.getAttribute("data-repo-header-index");
		const headerIndex = rawHeaderIndex === null ? NaN : Number(rawHeaderIndex);
		if (!repoId || !elementBucketKey || !Number.isFinite(headerIndex)) return;
		if (bucketKey !== void 0 && elementBucketKey !== bucketKey) return;
		const rect = element.getBoundingClientRect();
		const virtualRow = element.closest("[data-worktree-virtual-row]");
		const virtualRowStart = getVirtualRowStart(virtualRow);
		const top = virtualRow && virtualRowStart !== null ? virtualRowStart + rect.top - virtualRow.getBoundingClientRect().top : rect.top - containerRect.top + container.scrollTop;
		rects.push({
			repoId,
			bucketKey: elementBucketKey,
			headerIndex,
			top,
			bottom: top + rect.height,
			sectionBottom: getOptionalNumberAttribute(element, "data-repo-header-section-end")
		});
	});
	rects.sort((left, right) => left.top - right.top);
	return rects;
}
function mapSidebarRepoDropIndexToAllRepoInsertAt(sidebarDropIndex, sidebarRepoHeaderIds, allRepoIds) {
	if (sidebarRepoHeaderIds.length === 0) return 0;
	if (sidebarDropIndex <= 0) return allRepoIds.indexOf(sidebarRepoHeaderIds[0]);
	if (sidebarDropIndex >= sidebarRepoHeaderIds.length) {
		const lastId = sidebarRepoHeaderIds.at(-1);
		return allRepoIds.indexOf(lastId) + 1;
	}
	return allRepoIds.indexOf(sidebarRepoHeaderIds[sidebarDropIndex]);
}
function computeProjectHeaderDropPreview(args) {
	const { rects, sidebarRepoHeaderIds } = args;
	return computeWorktreeSidebarHeaderDropPreview({
		pointerY: args.pointerY,
		containerTop: args.containerTop,
		scrollTop: args.scrollTop,
		rects,
		headerCount: sidebarRepoHeaderIds.length,
		getId: (rect) => rect.repoId,
		contentBottom: args.contentBottom
	});
}
function applyAllRepoInsertAt(allRepoIds, draggedRepoId, insertAt) {
	if (!allRepoIds.includes(draggedRepoId) || insertAt < 0 || insertAt > allRepoIds.length) return null;
	const draggedBlock = allRepoIds.filter((repoId) => repoId === draggedRepoId);
	const adjustedInsertAt = insertAt - allRepoIds.slice(0, insertAt).filter((repoId) => repoId === draggedRepoId).length;
	const next = allRepoIds.filter((repoId) => repoId !== draggedRepoId);
	next.splice(adjustedInsertAt, 0, ...draggedBlock);
	if (next.every((repoId, index) => repoId === allRepoIds[index])) return null;
	return next;
}
function getPreferredWorktreeRows(rows, pinnedDisplayPolicy) {
	if (pinnedDisplayPolicy === "single-location") {
		const seen$1 = /* @__PURE__ */ new Set();
		return rows.filter((row) => {
			if (seen$1.has(row.worktree.id)) return false;
			seen$1.add(row.worktree.id);
			return true;
		});
	}
	const preferredRows = [];
	const seen = /* @__PURE__ */ new Set();
	for (const row of rows) {
		if (row.sectionKey === "pinned" || seen.has(row.worktree.id)) continue;
		preferredRows.push(row);
		seen.add(row.worktree.id);
	}
	for (const row of rows) {
		if (seen.has(row.worktree.id)) continue;
		preferredRows.push(row);
		seen.add(row.worktree.id);
	}
	return preferredRows;
}
function getRenderedWorktreesInSidebarOrder(rows, pinnedDisplayPolicy) {
	const itemRows = rows.filter((row) => row.type === "item");
	const preferredRowKeys = new Set(getPreferredWorktreeRows(itemRows, pinnedDisplayPolicy).map((row) => row.rowKey));
	const renderedWorktrees = [];
	for (const row of rows) if (row.type === "item" && preferredRowKeys.has(row.rowKey)) renderedWorktrees.push(row.worktree);
	else if (row.type === "folder-workspace") renderedWorktrees.push(folderWorkspaceToWorktree(row.folderWorkspace));
	return renderedWorktrees;
}
const EMPTY_WORKTREE_LIST_REVIEW_CACHE_INPUTS = Object.freeze({
	prCache: null,
	hostedReviewCache: null
});
function selectWorktreeListReviewCacheInputs(state, groupBy, cardProperties) {
	const hasFolderWorkspaces = state.folderWorkspaces.length > 0;
	const newCardStyle = state.settings?.experimentalNewWorktreeCardStyle === true;
	const folderCardsNeedReview = hasFolderWorkspaces && (newCardStyle ? cardProperties.includes("status") : cardProperties.includes("pr"));
	const needsPrCache = groupBy === "pr-status" || folderCardsNeedReview;
	const needsHostedReviewCache = newCardStyle && folderCardsNeedReview;
	if (!needsPrCache && !needsHostedReviewCache) return EMPTY_WORKTREE_LIST_REVIEW_CACHE_INPUTS;
	return {
		prCache: needsPrCache ? state.prCache : null,
		hostedReviewCache: needsHostedReviewCache ? state.hostedReviewCache : null
	};
}
function getVisibleSidebarHostIdSet(visibleWorkspaceHostIds, workspaceHostScope) {
	const visibleHostIds = visibleWorkspaceHostIds ?? (workspaceHostScope === "all" ? null : [workspaceHostScope]);
	return visibleHostIds ? new Set(visibleHostIds) : null;
}
function filterProjectGroupsForVisibleHosts(projectGroups, visibleHostIdSet, defaultHostId) {
	if (!visibleHostIdSet) return projectGroups;
	return projectGroups.filter((group) => visibleHostIdSet.has(getProjectGroupExecutionHostIdForRows(group, defaultHostId)));
}
function filterFolderWorkspacesForVisibleHosts(folderWorkspaces, projectGroups, visibleHostIdSet, defaultHostId) {
	if (!visibleHostIdSet) return folderWorkspaces;
	const projectGroupById = new Map(projectGroups.map((group) => [group.id, group]));
	return folderWorkspaces.filter((folderWorkspace) => visibleHostIdSet.has(getFolderWorkspaceExecutionHostIdForRows({
		folderWorkspace,
		projectGroup: projectGroupById.get(folderWorkspace.projectGroupId),
		defaultHostId
	})));
}
function getProjectGroupExecutionHostIdForRows(group, defaultHostId) {
	const executionHostId = normalizeExecutionHostId(group.executionHostId);
	if (executionHostId) return executionHostId;
	return group.connectionId ? toSshExecutionHostId(group.connectionId) : defaultHostId;
}
function getFolderWorkspaceExecutionHostIdForRows({ folderWorkspace, projectGroup, defaultHostId }) {
	const explicitFolderHostId = normalizeExecutionHostId(folderWorkspace.executionHostId);
	if (explicitFolderHostId) return explicitFolderHostId;
	if (projectGroup) {
		const explicitProjectGroupHostId = normalizeExecutionHostId(projectGroup.executionHostId);
		if (explicitProjectGroupHostId) return explicitProjectGroupHostId;
		const projectGroupHostId = getProjectGroupExecutionHostIdForRows(projectGroup, defaultHostId);
		if (projectGroupHostId !== defaultHostId || !folderWorkspace.connectionId) return projectGroupHostId;
	}
	return folderWorkspace.connectionId ? toSshExecutionHostId(folderWorkspace.connectionId) : defaultHostId;
}
function getRuntimeEnvironmentIdForFolderPathStatusHost(hostId) {
	const parsed = parseExecutionHostId(hostId);
	return parsed?.kind === "runtime" ? parsed.environmentId : null;
}
function getProjectGroupExecutionHostIdForFolderPathStatus(group) {
	const executionHostId = normalizeExecutionHostId(group.executionHostId);
	if (executionHostId) return executionHostId;
	return group.connectionId ? toSshExecutionHostId(group.connectionId) : "local";
}
function getFolderPathStatusRouteOptionsForRows({ request, projectGroupsById, folderWorkspacesById }) {
	const folderWorkspace = request.scope === "folder-workspace" ? folderWorkspacesById.get(request.folderWorkspaceId) : void 0;
	const group = request.scope === "project-group" ? projectGroupsById.get(request.projectGroupId) : projectGroupsById.get(folderWorkspace?.projectGroupId ?? "");
	if (!group) return;
	return { runtimeEnvironmentId: getRuntimeEnvironmentIdForFolderPathStatusHost(request.scope === "project-group" ? getProjectGroupExecutionHostIdForFolderPathStatus(group) : getFolderWorkspaceExecutionHostIdForRows({
		folderWorkspace: folderWorkspace ?? {
			connectionId: null,
			executionHostId: null
		},
		projectGroup: group,
		defaultHostId: getProjectGroupExecutionHostIdForFolderPathStatus(group)
	})) };
}
var EMPTY_REPO_ID_SET = Object.freeze(/* @__PURE__ */ new Set());
var EMPTY_IMPORTED_BY_REPO = Object.freeze(/* @__PURE__ */ new Map());
var EMPTY_INBOX_BY_REPO = Object.freeze(/* @__PURE__ */ new Map());
var EMPTY_PENDING_CREATIONS = Object.freeze([]);
function computeRenderedSidebarWorktreeOrder(state, visibleWorktrees) {
	const defaultHostId = getSettingsFocusedExecutionHostId(state.settings);
	const pinnedDisplayPolicy = getPinnedWorktreeDisplayPolicy(state.settings);
	const projection = getProjectHostSetupProjectionFromState(state);
	const visibleHostIdSet = getVisibleSidebarHostIdSet(state.visibleWorkspaceHostIds, state.workspaceHostScope);
	const projectGroups = state.projectGroups ?? [];
	const { prCache } = selectWorktreeListReviewCacheInputs(state, state.groupBy, state.worktreeCardProperties);
	const rows = buildRows(state.groupBy, [...visibleWorktrees], getRepoMapFromState(state), prCache, state.collapsedGroups, getLogicalRepoOrderRankById(state.repos.map((repo) => repo.id)), state.workspaceStatuses, state.projectOrderBy, state.worktreeLineageById, getWorktreeMapFromState(state), true, state.settings, filterProjectGroupsForVisibleHosts(projectGroups, visibleHostIdSet, defaultHostId), EMPTY_REPO_ID_SET, EMPTY_IMPORTED_BY_REPO, EMPTY_INBOX_BY_REPO, EMPTY_PENDING_CREATIONS, {
		projects: projection.projects,
		projectHostSetups: projection.setups
	}, filterFolderWorkspacesForVisibleHosts(state.folderWorkspaces, projectGroups, visibleHostIdSet, defaultHostId), void 0, defaultHostId, pinnedDisplayPolicy);
	const sectionRows = state.workspaceHostScope !== "all" || state.visibleWorkspaceHostIds != null ? addHostSectionRows({
		rows,
		hostOptions: orderHostSectionOptions(buildSidebarHostOptions({
			repos: state.repos,
			sshTargetLabels: state.sshTargetLabels,
			sshConnectionStates: state.sshConnectionStates,
			settings: state.settings,
			runtimeEnvironments: state.runtimeEnvironments,
			runtimeStatusByEnvironmentId: state.runtimeStatusByEnvironmentId,
			hostLabelOverrides: getHostDisplayLabelOverrides(state.settings)
		}), state.workspaceHostOrder),
		workspaceHostScope: state.workspaceHostScope,
		visibleWorkspaceHostIds: state.visibleWorkspaceHostIds,
		defaultHostId,
		collapsedHostKeys: state.collapsedGroups,
		forceCollapseHosts: false,
		preferProjectGrouping: true
	}) : rows;
	return Array.from(new Set(getRenderedWorktreesInSidebarOrder(sectionRows, pinnedDisplayPolicy).map((worktree) => worktree.id)));
}
const EMPTY_PAIRED_DEVICE_IDS_BY_ENVIRONMENT = /* @__PURE__ */ new Map();
function getPairedDeviceIdsByEnvironment(environments, statuses) {
	const result = /* @__PURE__ */ new Map();
	for (const environment of environments) {
		const deviceId = statuses.get(environment.id)?.status?.pairedDeviceId ?? environment.pairedDeviceId;
		if (deviceId) result.set(environment.id, deviceId);
	}
	return result;
}
function isWorkspaceFromOtherDevice(worktree, pairedDeviceIdsByEnvironment) {
	const creator = normalizeWorkspaceCreatorProvenance(worktree.creatorProvenance);
	if (!creator) return false;
	const environmentId = worktree.runtimeOwnerEnvironmentId;
	if (!environmentId) return creator.kind !== "host";
	const pairedDeviceId = pairedDeviceIdsByEnvironment.get(environmentId);
	if (!pairedDeviceId) return false;
	return creator.kind !== "paired-device" || creator.deviceId !== pairedDeviceId;
}
function isFolderWorkspaceFromOtherDevice(workspace, pairedDeviceIdsByEnvironment) {
	return isWorkspaceFromOtherDevice(folderWorkspaceToWorktree(workspace), pairedDeviceIdsByEnvironment);
}
function filterFolderWorkspacesFromOtherDevices(workspaces, pairedDeviceIdsByEnvironment) {
	return workspaces.filter((workspace) => !isFolderWorkspaceFromOtherDevice(workspace, pairedDeviceIdsByEnvironment));
}
function isDefaultBranchWorkspace(worktree) {
	return worktree.isMainWorktree && worktree.branch.trim() !== "";
}
function isSleepingSweepExemptWorkspace(worktree, alwaysShowDefaultBranchWorkspace) {
	return alwaysShowDefaultBranchWorkspace !== false && worktree.isMainWorktree;
}
function isSleepingSweepExemptionNarrowingList(showSleepingWorkspaces, alwaysShowDefaultBranchWorkspace) {
	return !showSleepingWorkspaces && alwaysShowDefaultBranchWorkspace === false;
}
function isAutomationGeneratedWorkspace(worktree) {
	return worktree.automationProvenance?.kind === "created-by-automation";
}
function isCliCreatedWorkspace(worktree) {
	return worktree.cliProvenance?.kind === "created-by-cli";
}
function isDetachedHeadWorkspace(worktree) {
	return getWorktreeGitIdentityDisplay(worktree)?.kind === "detached";
}
function sidebarHasActiveFilters(state) {
	return state.showSleepingWorkspaces !== true || state.filterRepoIds.length > 0 || state.hideDefaultBranchWorkspace || state.hideAutomationGeneratedWorkspaces || state.hideCliCreatedWorkspaces || state.hideDetachedHeadWorkspaces || state.hideWorkspacesFromOtherDevices || state.alwaysShowDefaultBranchWorkspace === false || state.visibleWorkspaceHostIds != null || state.workspaceHostScope != null && state.workspaceHostScope !== "all";
}
function computeClearFilterActions(state) {
	return {
		resetShowSleepingWorkspaces: state.showSleepingWorkspaces !== true,
		resetFilterRepoIds: state.filterRepoIds.length > 0,
		resetHideDefaultBranchWorkspace: state.hideDefaultBranchWorkspace,
		resetHideAutomationGeneratedWorkspaces: state.hideAutomationGeneratedWorkspaces,
		resetHideCliCreatedWorkspaces: state.hideCliCreatedWorkspaces,
		resetHideDetachedHeadWorkspaces: state.hideDetachedHeadWorkspaces,
		resetHideWorkspacesFromOtherDevices: state.hideWorkspacesFromOtherDevices,
		resetAlwaysShowDefaultBranchWorkspace: state.alwaysShowDefaultBranchWorkspace === false,
		resetVisibleWorkspaceHostIds: state.visibleWorkspaceHostIds != null || state.workspaceHostScope != null && state.workspaceHostScope !== "all"
	};
}
function computeVisibleWorktreeIds(worktreesByRepo, sortedIds, opts) {
	let all = getAllWorktreesFromState({ worktreesByRepo });
	all = all.filter((w) => !w.isArchived);
	const lineageAncestorById = new Map(all.map((w) => [w.id, w]));
	if (opts.hideWorkspacesFromOtherDevices) all = all.filter((worktree) => !isWorkspaceFromOtherDevice(worktree, opts.pairedDeviceIdsByEnvironment));
	if (opts.hideDefaultBranchWorkspace) all = all.filter((w) => !isDefaultBranchWorkspace(w));
	if (opts.hideAutomationGeneratedWorkspaces) all = all.filter((w) => !isAutomationGeneratedWorkspace(w));
	if (opts.hideCliCreatedWorkspaces) all = all.filter((w) => !isCliCreatedWorkspace(w));
	if (opts.hideDetachedHeadWorkspaces) all = all.filter((w) => !isDetachedHeadWorkspace(w));
	const visibleHostIds = opts.visibleWorkspaceHostIds ?? (opts.workspaceHostScope === "all" ? null : [opts.workspaceHostScope]);
	if (visibleHostIds) {
		const visibleHostIdSet = new Set(visibleHostIds);
		all = all.filter((w) => {
			const repo = opts.repoMap.get(w.repoId);
			if (!repo) return false;
			const hostId = getWorktreeExecutionHostId(w, repo, opts.defaultHostId);
			return visibleHostIdSet.has(hostId);
		});
	}
	if (opts.filterRepoIds.length > 0) {
		const selectedRepoIds = new Set(opts.filterRepoIds);
		all = all.filter((w) => selectedRepoIds.has(w.repoId));
	}
	if (!opts.showSleepingWorkspaces) all = all.filter((w) => isSleepingSweepExemptWorkspace(w, opts.alwaysShowDefaultBranchWorkspace) || !isInactiveWorkspace(w.id, opts.tabsByWorktree, opts.ptyIdsByTabId, opts.browserTabsByWorktree, opts.worktreeIdsWithLiveAgent));
	if (opts.forcedVisibleWorktreeIds && opts.forcedVisibleWorktreeIds.length > 0) {
		const includedIds = new Set(all.map((worktree) => worktree.id));
		for (const worktreeId of opts.forcedVisibleWorktreeIds) {
			const worktree = lineageAncestorById.get(worktreeId);
			if (worktree && !includedIds.has(worktreeId)) {
				includedIds.add(worktreeId);
				all.push(worktree);
			}
		}
	}
	const orderIndex = new Map(sortedIds.map((id, i) => [id, i]));
	all.sort((a, b) => {
		return (orderIndex.get(a.id) ?? Infinity) - (orderIndex.get(b.id) ?? Infinity);
	});
	const visibleIds = all.map((w) => w.id);
	return opts.injectLineageAncestors === false ? visibleIds : addVisibleLineageAncestors(visibleIds, lineageAncestorById, opts.worktreeLineageById);
}
function addVisibleLineageAncestors(ids, worktreeById, lineageById) {
	const result = [];
	const included = /* @__PURE__ */ new Set();
	const visiting = /* @__PURE__ */ new Set();
	const cyclicLineageIds = getCyclicProjectedWorktreeLineageIds(lineageById, worktreeById);
	const addWithAncestors = (id) => {
		if (included.has(id) || visiting.has(id)) return;
		const worktree = worktreeById.get(id);
		if (!worktree) return;
		visiting.add(id);
		const lineage = getLineageRenderInfo(worktree, lineageById, worktreeById, cyclicLineageIds);
		if (lineage.state === "valid") addWithAncestors(lineage.parent.id);
		visiting.delete(id);
		if (!included.has(id)) {
			included.add(id);
			result.push(id);
		}
	};
	for (const id of ids) addWithAncestors(id);
	return result;
}
var _publishedVisibleIds = null;
function setVisibleWorktreeIds(ids) {
	_publishedVisibleIds = ids;
}
function getVisibleWorktreeIds() {
	if (_publishedVisibleIds) return _publishedVisibleIds;
	const state = useAppStore.getState();
	const allWorktrees = getAllWorktreesFromState(state).filter((w) => !w.isArchived);
	const repoMap = getRepoMapFromState(state);
	let sortedIds;
	if (state.sortBy === "smart") sortedIds = sortWorktreesSmart(allWorktrees, state.tabsByWorktree, repoMap, state.agentStatusByPaneKey, state.runtimePaneTitlesByTabId, state.ptyIdsByTabId, state.migrationUnsupportedByPtyId, state.terminalLayoutsByTabId).map((w) => w.id);
	else sortedIds = [...allWorktrees].sort(buildWorktreeComparator(state.sortBy, repoMap, Date.now(), /* @__PURE__ */ new Map())).map((w) => w.id);
	const visibleIds = computeVisibleWorktreeIds(state.worktreesByRepo, sortedIds, {
		filterRepoIds: state.filterRepoIds,
		showSleepingWorkspaces: state.showSleepingWorkspaces,
		tabsByWorktree: state.tabsByWorktree,
		ptyIdsByTabId: state.ptyIdsByTabId,
		browserTabsByWorktree: state.browserTabsByWorktree,
		worktreeIdsWithLiveAgent: getWorktreeIdsWithLiveAgent(state.agentStatusByPaneKey, state.tabsByWorktree, Date.now()),
		hideDefaultBranchWorkspace: state.hideDefaultBranchWorkspace,
		hideAutomationGeneratedWorkspaces: state.hideAutomationGeneratedWorkspaces,
		hideCliCreatedWorkspaces: state.hideCliCreatedWorkspaces,
		hideDetachedHeadWorkspaces: state.hideDetachedHeadWorkspaces,
		hideWorkspacesFromOtherDevices: state.hideWorkspacesFromOtherDevices,
		pairedDeviceIdsByEnvironment: state.hideWorkspacesFromOtherDevices ? getPairedDeviceIdsByEnvironment(state.runtimeEnvironments, state.runtimeStatusByEnvironmentId) : EMPTY_PAIRED_DEVICE_IDS_BY_ENVIRONMENT,
		alwaysShowDefaultBranchWorkspace: state.alwaysShowDefaultBranchWorkspace,
		repoMap,
		workspaceHostScope: state.workspaceHostScope,
		visibleWorkspaceHostIds: state.visibleWorkspaceHostIds,
		defaultHostId: getSettingsFocusedExecutionHostId(state.settings),
		worktreeLineageById: state.worktreeLineageById
	});
	const worktreeMap = getWorktreeMapFromState(state);
	return computeRenderedSidebarWorktreeOrder(state, visibleIds.map((id) => worktreeMap.get(id)).filter((w) => w != null));
}
function resolveStartupLaunchDraftText(startup) {
	return startup?.draftPrompt ?? startup?.launchDraftText;
}
function draftViewModeProps(draftText) {
	return draftText == null ? {} : {
		promptDelivery: "draft",
		launchDraftText: draftText
	};
}
function getSetupRunnerCommandPlatformForLaunch(setup) {
	return getSetupRunnerCommandPlatformForPath(setup.runnerScriptPath, navigator.userAgent.includes("Windows") ? "windows" : "posix");
}
function ensureFolderWorkspaceInitialTerminal(folderWorkspace, startup) {
	return ensureWorktreeHasInitialTerminal(useAppStore.getState(), folderWorkspaceKey(folderWorkspace.id), startup, void 0, void 0, void 0);
}
function activateAndRevealFolderWorkspace(folderWorkspaceId, opts) {
	const state = useAppStore.getState();
	const folderWorkspaceOwner = findFolderWorkspaceOwner(state, folderWorkspaceId, opts?.executionHostId);
	const folderWorkspace = state.folderWorkspaces.find((workspace) => workspace === folderWorkspaceOwner);
	if (!folderWorkspace) return false;
	const runtimeEnvironmentId = opts && "runtimeEnvironmentId" in opts ? opts.runtimeEnvironmentId ?? null : getRuntimeEnvironmentIdForWorktree(state, folderWorkspaceKey(folderWorkspaceId));
	const pathStatus = state.getFreshFolderWorkspacePathStatus({
		scope: "folder-workspace",
		folderWorkspaceId
	}, { runtimeEnvironmentId });
	if (folderWorkspaceActivationBlocked(pathStatus)) {
		const title = getFolderWorkspacePathStatusTitle(pathStatus) ?? translate("auto.lib.worktree.activation.cannotOpenFolderWorkspace", "Cannot open folder workspace");
		toast.error(title, { description: getFolderWorkspacePathStatusDescription(pathStatus) ?? folderWorkspace.folderPath });
		return false;
	}
	if (state.activeView !== "terminal") state.setActiveView("terminal");
	state.setActiveFolderWorkspace(folderWorkspaceId, opts?.executionHostId);
	const workspaceKey = folderWorkspaceKey(folderWorkspaceId);
	state.markWorktreeVisited(workspaceKey);
	if (!state.isNavigatingHistory) state.recordWorktreeVisit(workspaceKey);
	resumeSleepingAgentSessionsForWorktree(workspaceKey);
	const primaryTabId = ensureFolderWorkspaceInitialTerminal(folderWorkspace, opts?.startup);
	if (opts?.sidebarRevealBehavior) state.revealWorktreeInSidebar(workspaceKey, { behavior: opts.sidebarRevealBehavior });
	else state.revealWorktreeInSidebar(workspaceKey);
	return { primaryTabId };
}
function activateAndRevealWorktree(worktreeId, opts) {
	const state = useAppStore.getState();
	const wt = state.getKnownWorktreeById(worktreeId, opts?.executionHostId);
	if (!wt) return false;
	const isPlainAlreadyActiveTerminal = !Boolean(opts?.startup || opts?.setup || opts?.defaultTabs || opts?.issueCommand) && state.activeRepoId === wt.repoId && state.activeWorktreeId === worktreeId && state.activeWorkspaceExecutionHostId === (opts?.executionHostId ?? null) && state.activeView === "terminal";
	if (wt.repoId !== state.activeRepoId) state.setActiveRepo(wt.repoId);
	if (state.activeView !== "terminal") state.setActiveView("terminal");
	state.setActiveWorktree(worktreeId, opts?.executionHostId);
	const ownerRuntimeEnvironmentId = getRuntimeEnvironmentIdForWorktree(useAppStore.getState(), wt.id);
	if (opts?.notifyHostRuntime !== false && isWebRuntimeSessionActive(ownerRuntimeEnvironmentId)) activateWebRuntimeSessionWorktree({
		worktreeId,
		environmentId: ownerRuntimeEnvironmentId
	});
	if (!isPlainAlreadyActiveTerminal) state.markWorktreeVisited(worktreeId);
	if (!isPlainAlreadyActiveTerminal && !state.isNavigatingHistory) state.recordWorktreeVisit(worktreeId);
	resumeSleepingAgentSessionsForWorktree(worktreeId);
	const primaryTabId = ensureWorktreeHasInitialTerminal(useAppStore.getState(), worktreeId, opts?.startup, opts?.setup, opts?.issueCommand, opts?.defaultTabs, opts?.backendStartupTerminalSpawned ? { backendStartupTerminalSpawned: true } : void 0);
	if (primaryTabId && opts?.initialCwd) useAppStore.getState().queueTabInitialCwd(primaryTabId, opts.initialCwd);
	if (state.filterRepoIds.length > 0 && !state.filterRepoIds.includes(wt.repoId)) state.setFilterRepoIds([]);
	if (state.hideAutomationGeneratedWorkspaces && wt.automationProvenance?.kind === "created-by-automation") state.setHideAutomationGeneratedWorkspaces(false);
	if (state.hideCliCreatedWorkspaces && wt.cliProvenance?.kind === "created-by-cli") state.setHideCliCreatedWorkspaces(false);
	if (state.hideDetachedHeadWorkspaces && isDetachedHeadWorkspace(wt)) state.setHideDetachedHeadWorkspaces(false);
	if (opts?.revealInSidebar !== false) if (opts?.sidebarRevealBehavior) state.revealWorktreeInSidebar(worktreeId, { behavior: opts.sidebarRevealBehavior });
	else state.revealWorktreeInSidebar(worktreeId);
	if (opts?.notifyHostRuntime !== false && !opts?.backendStartupTerminalSpawned) ensureWebRuntimeWorktreeTerminalAfterWake(worktreeId);
	return { primaryTabId };
}
function ensureWebRuntimeWorktreeTerminalAfterWake(worktreeId) {
	const state = useAppStore.getState();
	const worktree = state.getKnownWorktreeById(worktreeId);
	if (!worktree) return;
	const runtimeEnvironmentId = getRuntimeEnvironmentIdForWorktree(state, worktree.id);
	if (!runtimeEnvironmentId || !isWebRuntimeSessionActive(runtimeEnvironmentId)) return;
	const tabs = state.tabsByWorktree[worktreeId] ?? [];
	if (tabs.some((tab) => tabHasLivePty(state.ptyIdsByTabId, tab.id))) return;
	if (tabs.some((tab) => isWebTerminalSurfaceTabId(tab.id))) return;
	if (getLastKnownHostTerminalTabCount(runtimeEnvironmentId, worktreeId) > 0) return;
	const { renderableTabCount } = state.reconcileWorktreeTabModel(worktreeId);
	if (tabs.length > 0 && renderableTabCount === 0) return;
	if (!beginWebRuntimeWakeTerminalRespawn(worktreeId)) return;
	createWebRuntimeSessionTerminal({
		worktreeId,
		environmentId: runtimeEnvironmentId,
		activate: true,
		selectWorktree: false
	}).finally(() => {
		endWebRuntimeWakeTerminalRespawn(worktreeId);
	});
}
function ensureWorktreeHasInitialTerminal(store, worktreeId, startup, setup, issueCommand, defaultTabs, opts) {
	const { renderableTabCount } = store.reconcileWorktreeTabModel(worktreeId);
	const ownerState = store.settings !== void 0 || store.repos !== void 0 || store.worktreesByRepo !== void 0 ? store : useAppStore.getState();
	let sequencedStartup = startup;
	let wrappedSetupCommandStr;
	if (startup && setup?.waitForAgentStartup === true) {
		const platform = getSetupRunnerCommandPlatformForLaunch(setup);
		const sequenced = createSequencedSetupAgentCommands({
			runnerScriptPath: setup.runnerScriptPath,
			startupCommand: startup.command,
			platform,
			shell: setup.shell
		});
		sequencedStartup = {
			...startup,
			command: sequenced.startupCommand,
			...sequenced.startupEnv ? { env: {
				...startup.env,
				...sequenced.startupEnv
			} } : {}
		};
		wrappedSetupCommandStr = sequenced.setupCommand;
	}
	const backendStartupTerminalSpawned = opts?.backendStartupTerminalSpawned === true;
	if (backendStartupTerminalSpawned || isWebRuntimeSessionActive(getRuntimeEnvironmentIdForWorktree(ownerState, worktreeId))) {
		const existingTerminalTabId = store.tabsByWorktree[worktreeId]?.[0]?.id;
		if (existingTerminalTabId && (setup || issueCommand)) {
			queueSetupAndIssueCommands(store, worktreeId, existingTerminalTabId, setup, issueCommand, wrappedSetupCommandStr, opts);
			return existingTerminalTabId;
		}
		if (existingTerminalTabId && backendStartupTerminalSpawned) return existingTerminalTabId;
		if (setup || issueCommand) queueHookCommandsForFirstWorktreeTab({
			worktreeId,
			deliver: (state, firstTerminalTabId) => queueSetupAndIssueCommands(state, worktreeId, firstTerminalTabId, setup, issueCommand, wrappedSetupCommandStr, opts)
		});
		return null;
	}
	if (!shouldAutoCreateInitialTerminal(renderableTabCount)) {
		const existingTerminalTabId = store.tabsByWorktree[worktreeId]?.[0]?.id;
		if (existingTerminalTabId && (setup || issueCommand)) {
			queueSetupAndIssueCommands(store, worktreeId, existingTerminalTabId, setup, issueCommand, wrappedSetupCommandStr, opts);
			return existingTerminalTabId;
		}
		return null;
	}
	const templatedTabId = applyDefaultTerminalTabs(store, worktreeId, sequencedStartup, setup, issueCommand, defaultTabs, wrappedSetupCommandStr, opts);
	if (templatedTabId) return templatedTabId;
	const launchAgent = sequencedStartup?.launchAgent ?? (sequencedStartup?.telemetry ? agentKindToTuiAgent(sequencedStartup.telemetry.agent_kind) ?? void 0 : void 0);
	const terminalTab = store.createTab(worktreeId, void 0, void 0, {
		pendingActivationSpawn: true,
		...launchAgent ? {
			launchAgent,
			...initialAgentTabViewModeProps(store.settings ?? null, {
				agent: launchAgent,
				...draftViewModeProps(resolveStartupLaunchDraftText(sequencedStartup)),
				nativeChatTranscriptIsLocalReadable: isNativeChatTranscriptLocalReadable(getConnectionId(worktreeId))
			})
		} : {},
		...opts?.activateCreatedTabs === false ? { activate: false } : {}
	});
	if (opts?.activateCreatedTabs !== false) store.setActiveTab(terminalTab.id);
	if (sequencedStartup) {
		if (launchAgent) seedNativeChatAppliedSessionOptions(terminalTab.id, launchAgent, sequencedStartup.sessionOptions);
		store.queueTabStartupCommand(terminalTab.id, sequencedStartup);
	}
	queueSetupAndIssueCommands(store, worktreeId, terminalTab.id, setup, issueCommand, wrappedSetupCommandStr, opts);
	return terminalTab.id;
}
function applyDefaultTerminalTabs(store, worktreeId, startup, setup, issueCommand, defaultTabs, wrappedSetupCommandStr, opts) {
	if (!defaultTabs || store.defaultTerminalTabsAppliedByWorktreeId[worktreeId]) return null;
	store.markDefaultTerminalTabsApplied(worktreeId);
	if (defaultTabs.tabs.length === 0) return null;
	let firstTabId = null;
	for (const [index, template] of defaultTabs.tabs.entries()) {
		const isStartupTab = index === 0 && startup !== void 0;
		const launchAgent = isStartupTab && startup?.launchAgent ? startup.launchAgent : isStartupTab && startup?.telemetry ? agentKindToTuiAgent(startup.telemetry.agent_kind) ?? void 0 : void 0;
		const tab = store.createTab(worktreeId, void 0, void 0, {
			pendingActivationSpawn: true,
			recordInteraction: false,
			...launchAgent ? {
				launchAgent,
				...initialAgentTabViewModeProps(store.settings ?? null, {
					agent: launchAgent,
					...draftViewModeProps(isStartupTab ? resolveStartupLaunchDraftText(startup) : void 0),
					nativeChatTranscriptIsLocalReadable: isNativeChatTranscriptLocalReadable(getConnectionId(worktreeId))
				})
			} : {},
			...opts?.activateCreatedTabs === false ? { activate: false } : {}
		});
		if (index === 0) firstTabId = tab.id;
		if (template.title) store.setTabCustomTitle(tab.id, template.title, { recordInteraction: false });
		if (template.color) store.setTabColor(tab.id, template.color);
		const templateCommand = template.command?.trim();
		if (templateCommand && defaultTabs.runCommands && !(index === 0 && startup)) store.queueTabStartupCommand(tab.id, { command: templateCommand });
	}
	if (!firstTabId) return null;
	if (opts?.activateCreatedTabs !== false) store.setActiveTab(firstTabId);
	if (startup) {
		const startupAgent = startup.launchAgent ?? (startup.telemetry ? agentKindToTuiAgent(startup.telemetry.agent_kind) ?? void 0 : void 0);
		if (startupAgent) seedNativeChatAppliedSessionOptions(firstTabId, startupAgent, startup.sessionOptions);
		store.queueTabStartupCommand(firstTabId, startup);
	}
	queueSetupAndIssueCommands(store, worktreeId, firstTabId, setup, issueCommand, wrappedSetupCommandStr, opts);
	return firstTabId;
}
function queueSetupAndIssueCommands(store, worktreeId, terminalTabId, setup, issueCommand, wrappedSetupCommandStr, opts) {
	if (setup) {
		const mode = useAppStore.getState().settings?.setupScriptLaunchMode ?? "new-tab";
		const setupCommand = {
			command: wrappedSetupCommandStr ?? setup.command ?? buildSetupRunnerCommand(setup.runnerScriptPath, setup.shell),
			env: setup.envVars
		};
		if (mode === "new-tab") {
			const setupTab = store.createTab(worktreeId, void 0, void 0, {
				recordInteraction: false,
				...opts?.activateCreatedTabs === false ? { activate: false } : {}
			});
			if (opts?.activateCreatedTabs !== false) store.setActiveTab(terminalTabId);
			store.setTabCustomTitle(setupTab.id, "Setup", { recordInteraction: false });
			store.queueTabStartupCommand(setupTab.id, setupCommand);
		} else store.queueTabSetupSplit(terminalTabId, {
			...setupCommand,
			direction: mode === "split-horizontal" ? "horizontal" : "vertical"
		});
	}
	if (issueCommand) {
		const queuedIssueCommand = "runnerScriptPath" in issueCommand ? {
			command: buildSetupRunnerCommand(issueCommand.runnerScriptPath, issueCommand.shell),
			env: issueCommand.envVars
		} : {
			command: issueCommand.command,
			env: issueCommand.env
		};
		store.queueTabIssueCommandSplit(terminalTabId, queuedIssueCommand);
	}
}
function activateAndRevealWorkspace(workspaceId) {
	const workspaceScope = parseWorkspaceKey(workspaceId);
	if (workspaceScope?.type === "folder") return activateAndRevealFolderWorkspace(workspaceScope.folderWorkspaceId);
	return activateAndRevealWorktree(workspaceId);
}
setWorktreeNavActivator(activateAndRevealWorkspace);
setWorktreeNavViewActivator((entry) => {
	if (entry === "automations") {
		useAppStore.getState().setActiveView(entry);
		return;
	}
	if (entry === "tasks") {
		useAppStore.setState((state) => ({
			activeView: "tasks",
			githubTaskDrawerWorkItem: null,
			taskPageData: {
				...state.taskPageData,
				openGitHubWorkItem: void 0,
				openGitHubSourceContext: void 0,
				openGitHubInitialTab: void 0,
				openGitLabWorkItem: void 0,
				openGitLabSourceContext: void 0,
				openLinearIssue: void 0,
				openLinearSourceContext: void 0,
				openJiraIssue: void 0,
				openJiraSourceContext: void 0
			}
		}));
		return;
	}
	if (entry.source === "github") {
		useAppStore.setState((state) => ({
			activeView: "tasks",
			taskPageData: {
				...state.taskPageData,
				taskSource: "github",
				preselectedRepoId: entry.workItem.repoId,
				openGitHubWorkItem: entry.workItem,
				openGitHubSourceContext: entry.sourceContext,
				openGitHubInitialTab: entry.initialTab,
				openGitLabWorkItem: void 0,
				openGitLabSourceContext: void 0,
				openLinearIssue: void 0,
				openLinearSourceContext: void 0,
				openJiraIssue: void 0,
				openJiraSourceContext: void 0
			}
		}));
		return;
	}
	if (entry.source === "gitlab") {
		useAppStore.setState((state) => ({
			activeView: "tasks",
			githubTaskDrawerWorkItem: null,
			taskPageData: {
				...state.taskPageData,
				taskSource: "gitlab",
				preselectedRepoId: entry.workItem.repoId,
				openGitHubWorkItem: void 0,
				openGitHubSourceContext: void 0,
				openGitHubInitialTab: void 0,
				openGitLabWorkItem: entry.workItem,
				openGitLabSourceContext: entry.sourceContext,
				openLinearIssue: void 0,
				openLinearSourceContext: void 0,
				openJiraIssue: void 0,
				openJiraSourceContext: void 0
			}
		}));
		return;
	}
	if (entry.source === "jira") {
		useAppStore.setState((state) => ({
			activeView: "tasks",
			githubTaskDrawerWorkItem: null,
			taskPageData: {
				...state.taskPageData,
				taskSource: "jira",
				openGitHubWorkItem: void 0,
				openGitHubSourceContext: void 0,
				openGitHubInitialTab: void 0,
				openGitLabWorkItem: void 0,
				openGitLabSourceContext: void 0,
				openLinearIssue: void 0,
				openLinearSourceContext: void 0,
				openJiraIssue: entry.issue,
				openJiraSourceContext: entry.sourceContext
			}
		}));
		return;
	}
	useAppStore.setState((state) => ({
		activeView: "tasks",
		githubTaskDrawerWorkItem: null,
		taskPageData: {
			...state.taskPageData,
			taskSource: "linear",
			openGitHubWorkItem: void 0,
			openGitHubSourceContext: void 0,
			openGitHubInitialTab: void 0,
			openGitLabWorkItem: void 0,
			openGitLabSourceContext: void 0,
			openLinearIssue: entry.issue,
			openLinearSourceContext: entry.sourceContext,
			openJiraIssue: void 0,
			openJiraSourceContext: void 0
		}
	}));
});
export { ALL_GROUP_KEY as $, getRenderedWorktreesInSidebarOrder as A, buildSetupRunnerCommand$1 as At, computeWorktreeSidebarHeaderDropPreview as B, filterFolderWorkspacesForVisibleHosts as C, resolveAttention as Ct, getVisibleSidebarHostIdSet as D, getProviderSessionClaimKey as Dt, getProjectGroupExecutionHostIdForRows as E, resolveAgentResumeLaunchTarget as Et, getProjectHeaderDragBucketKey as F, buildSidebarHostOptions as G, getWorktreeSidebarDragAutoscroll as H, getSidebarOrderedRepoHeaderIdsByBucket as I, getSidebarHostVisibilityLabel as J, buildSidebarHostScopeOptions as K, mapSidebarProjectHeaderDropIndexToSiblingInsertIndex as L, computeProjectHeaderDropPreview as M, shouldAutoCreateInitialTerminal as Mt, getLogicalRepoOrderRankById as N, FolderTree as Nt, selectWorktreeListReviewCacheInputs as O, isPassiveCompletedHibernationEvidence as Ot, getProjectGroupOrderForSidebarDrop as P, addHostSectionRows as Q, mapSidebarRepoDropIndexToAllRepoInsertAt as R, isWorkspaceFromOtherDevice as S, hasFreshAttributedAgentStatus as St, getFolderPathStatusRouteOptionsForRows as T, resumeSleepingAgentSessionsForWorktree as Tt, getWorktreeSidebarDragRectsForGroup as U, getWorktreeSidebarBoundaryDrop as V, refreshWorktreeSidebarDragSession as W, buildExecutionHostRegistry as X, shouldShowHostScopeControls as Y, orderHostSectionOptions as Z, sidebarHasActiveFilters as _, sortWorktreesSmart as _t, ensureWorktreeHasInitialTerminal as a, getProjectGroupHeaderKey as at, getPairedDeviceIdsByEnvironment as b, buildExplicitEntriesByTabId as bt, computeVisibleWorktreeIds as c, getRepoDisplayLabelsByPath as ct, isCliCreatedWorkspace as d, isInactiveWorkspace as dt, PINNED_GROUP_KEY as et, isDefaultBranchWorkspace as f, mergeAgentStatusOrchestration as ft, setVisibleWorktreeIds as g, compareWorktreeSortLabel as gt, isSleepingSweepExemptionNarrowingList as h, buildWorktreeComparator as ht, ensureWebRuntimeWorktreeTerminalAfterWake as i, getPinnedWorktreeDisplayPolicy as it, applyAllRepoInsertAt as j, getSetupRunnerCommandPlatformForPath as jt, getPreferredWorktreeRows as k, recordPaneIsOwnedByPreservedPane as kt, getVisibleWorktreeIds as l, getLiveAgentStatusByWorktreeId as lt, isSleepingSweepExemptWorkspace as m, resolveAgentStatusWorktreeId as mt, activateAndRevealWorkspace as n, getGroupKeysForWorktree as nt, resolveStartupLaunchDraftText as o, getProjectHeaderRevealTarget as ot, isDetachedHeadWorkspace as p, parseAgentStatusPaneIdentity as pt, getSidebarHostHealthLabel as q, activateAndRevealWorktree as r, getLineageGroupKey as rt, computeClearFilterActions as s, getRepoDisplayLabelKey as st, activateAndRevealFolderWorkspace as t, buildRows as tt, isAutomationGeneratedWorkspace as u, getWorktreeIdsWithLiveAgent as ut, EMPTY_PAIRED_DEVICE_IDS_BY_ENVIRONMENT as v, IDLE as vt, filterProjectGroupsForVisibleHosts as w, queueHookCommandsForFirstWorktreeTab as wt, isFolderWorkspaceFromOtherDevice as x, collectTabPaneInputs as xt, filterFolderWorkspacesFromOtherDevices as y, buildAttentionByWorktree as yt, measureProjectHeaderDragRects as z };
