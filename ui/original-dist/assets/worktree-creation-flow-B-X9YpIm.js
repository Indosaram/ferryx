const __vite__mapDeps=(i,m=__vite__mapDeps,d=(m.f||(m.f=["./web-runtime-session-BV3mF8u4.js","./button-DszXJEV6.js","./jsx-runtime-Cv_nyRjc.js","./preload-helper-Cgw39-ka.js","./chunk-Dhmk_5SA.js","./react-Da2TLWQy.js","./store-CgXrfmaH.js","./defineProperty-BAtR-r70.js","./dist-DgqligFk.js","./react-dom-Da8MQai-.js","./plugin-manifest-Bs-50M_g.js","./useMountedRef-1omUd-IV.js","./agent-status-3vUKbY6l.js","./agent-kind-Dfx6MnkP.js","./telemetry-ZyUPyKMD.js","./agent-paste-draft-C2PA7vXu.js","./terminal-pty-input-transaction-2UskR-Bm.js","./agent-process-recognition-BB0O3DaN.js","./web-runtime-session-CN2syA39.js"])))=>i.map(i=>d[i]);
import { a as translate } from "./jsx-runtime-Cv_nyRjc.js";
import { a as ensureWorktreeHasInitialTerminal, r as activateAndRevealWorktree, wt as queueHookCommandsForFirstWorktreeTab } from "./worktree-activation-BDsaiyMf.js";
import { Fd as isNativeChatTranscriptLocalReadable, Kd as nativeChatRequiresLocalTranscript, Lu as toWebTerminalSurfaceTabId, Ou as getRuntimeEnvironmentIdForWorktree, Ud as decideInitialAgentTabViewMode, Ut as getConnectionIdFromState, Vu as createBrowserUuid, dm as PROJECT_HOST_SETUP_RUNTIME_CAPABILITY, np as assertRuntimeEnvironmentCapability, op as getActiveRuntimeTarget, t as useAppStore, ym as getProjectIdentityKey } from "./store-CgXrfmaH.js";
import { ct as toRuntimeExecutionHostId, dt as TUI_AGENT_CONFIG, lt as toSshExecutionHostId } from "./agent-status-3vUKbY6l.js";
import { n as toast } from "./dist-DgqligFk.js";
import { t as __vitePreload } from "./preload-helper-Cgw39-ka.js";
import { T as seedNativeChatLaunchDraftForAgentTab } from "./web-runtime-session-CN2syA39.js";
import { n as seedNativeChatAppliedSessionOptions, v as ensureAgentStartupInTerminal } from "./native-chat-session-option-cache-DGE3h47U.js";
import { t as queueWorkspaceActivationTerminalFocus } from "./workspace-activation-terminal-focus-CpPnzh-J.js";
import { t as getEphemeralVmRecipeResultProjectRoot } from "./ephemeral-vm-recipes-D4s3J2cQ.js";
function findPendingLinkedWorkItemCreationId(pendingCreations, request) {
	if (request.linkedIssue == null && request.linkedPR == null) return null;
	const hostId = request.workspaceRunContext?.hostId ?? null;
	return Object.values(pendingCreations).find((entry) => {
		const pending = entry.request;
		return pending.repoId === request.repoId && pending.linkedIssue === request.linkedIssue && pending.linkedPR === request.linkedPR && (pending.workspaceRunContext?.hostId ?? null) === hostId;
	})?.creationId ?? null;
}
function getCreationProgressLabel(entry) {
	if (entry.phase === "provisioning-vm") return "Provisioning VM…";
	if (entry.indeterminate) return "Setting up your workspace…";
	if (entry.phase === "preparing") return "Preparing workspace…";
	return entry.phase === "creating" ? "Creating worktree…" : "Fetching base branch…";
}
async function prepareEphemeralVmWorkspaceTarget(args) {
	const provisioned = await window.api.ephemeralVm.provision({
		repoId: args.repoId,
		recipeId: args.recipeId,
		projectId: args.projectId,
		workspaceName: args.workspaceName,
		...args.provisionId ? { provisionId: args.provisionId } : {}
	});
	if (!provisioned.ok) return {
		ok: false,
		error: provisioned.error,
		stderr: provisioned.stderr
	};
	const hostId = provisioned.connectionType === "ssh" ? toSshExecutionHostId(provisioned.sshTargetId) : toRuntimeExecutionHostId(provisioned.environment.id);
	if (provisioned.connectionType === "orca-server") try {
		await assertRuntimeEnvironmentCapability(provisioned.environment.id, PROJECT_HOST_SETUP_RUNTIME_CAPABILITY, "The recipe-created Orca server does not support project setup.");
	} catch (error) {
		await cleanupProvisionedRuntime(provisioned.runtime.id);
		return {
			ok: false,
			error: error instanceof Error ? error.message : String(error),
			stderr: provisioned.stderr
		};
	}
	let setup;
	try {
		setup = await args.setupExistingFolder({
			projectId: args.projectId,
			hostId,
			path: getEphemeralVmRecipeResultProjectRoot(provisioned.runtime.recipeResult),
			setupMethod: "imported-existing-folder"
		});
	} catch (error) {
		await cleanupProvisionedRuntime(provisioned.runtime.id);
		return {
			ok: false,
			error: error instanceof Error ? error.message : String(error),
			stderr: provisioned.stderr
		};
	}
	if (!setup) {
		await cleanupProvisionedRuntime(provisioned.runtime.id);
		return {
			ok: false,
			error: translate("auto.lib.ephemeralVmWorkspaceTarget.projectRootRegistrationFailed", "Failed to register the recipe-created project root on the runtime."),
			stderr: provisioned.stderr
		};
	}
	setup = {
		...setup,
		setup: {
			...setup.setup,
			hostId
		}
	};
	const success = {
		ok: true,
		setup,
		runtimeId: provisioned.runtime.id,
		stderr: provisioned.stderr,
		warnings: provisioned.warnings
	};
	return provisioned.connectionType === "orca-server" ? {
		...success,
		environmentId: provisioned.environment.id
	} : success;
}
async function cleanupProvisionedRuntime(runtimeId) {
	try {
		await window.api.ephemeralVm.cleanup({ runtimeId });
	} catch {}
}
var MAX_PROVISIONING_LOG_CHARS = 12e3;
async function prepareRequestForCreate(creationId, request) {
	if (!request.ephemeralVmRecipe || request.ephemeralVmRuntimeId) return request;
	const store = useAppStore.getState();
	store.updatePendingWorktreeCreation(creationId, {
		phase: "provisioning-vm",
		provisioningLog: ""
	});
	const unsubscribeProvisionEvents = window.api.ephemeralVm.onProvisionEvent?.((event) => {
		if (event.provisionId !== creationId || event.stream !== "stderr") return;
		appendProvisioningLog(creationId, event.chunk);
	});
	let preparedTarget;
	try {
		const sourceRepo = store.repos.find((repo) => repo.id === request.ephemeralVmRecipe?.sourceRepoId);
		preparedTarget = await prepareEphemeralVmWorkspaceTarget({
			repoId: request.ephemeralVmRecipe.sourceRepoId,
			recipeId: request.ephemeralVmRecipe.recipeId,
			projectId: resolvePortableEphemeralVmProjectId(sourceRepo) ?? request.ephemeralVmRecipe.projectId,
			workspaceName: request.name,
			provisionId: creationId,
			setupExistingFolder: store.setupProjectExistingFolder
		});
	} finally {
		unsubscribeProvisionEvents?.();
	}
	if (!preparedTarget.ok) {
		if (!useAppStore.getState().pendingWorktreeCreations[creationId]) return null;
		useAppStore.getState().updatePendingWorktreeCreation(creationId, {
			status: "error",
			error: preparedTarget.error
		});
		if (useAppStore.getState().activePendingCreationId !== creationId) toast.error(preparedTarget.error);
		return null;
	}
	appendProvisioningWarnings(creationId, preparedTarget.warnings);
	const preparedRequest = {
		...request,
		repoId: preparedTarget.setup.repo.id,
		...getEphemeralVmPortableBaseSelection(request),
		ephemeralVmRuntimeId: preparedTarget.runtimeId,
		...preparedTarget.environmentId ? { ephemeralVmRuntimeEnvironmentId: preparedTarget.environmentId } : {},
		workspaceRunContext: {
			kind: "workspace-run",
			projectId: preparedTarget.setup.setup.projectId,
			hostId: preparedTarget.setup.setup.hostId,
			projectHostSetupId: preparedTarget.setup.setup.id,
			repoId: preparedTarget.setup.repo.id,
			path: preparedTarget.setup.repo.path
		}
	};
	if (!useAppStore.getState().pendingWorktreeCreations[creationId]) {
		await cleanupEphemeralVmRuntimeForFailedCreate(preparedRequest);
		return null;
	}
	useAppStore.getState().updatePendingWorktreeCreation(creationId, {
		phase: "fetching",
		request: preparedRequest
	});
	return preparedRequest;
}
function getEphemeralVmPortableBaseSelection(request) {
	if (request.linkedPR !== void 0 || request.linkedGitLabMR !== void 0 || request.linkedBitbucketPR !== void 0 || request.linkedAzureDevOpsPR !== void 0 || request.linkedGiteaPR !== void 0 || Boolean(request.compareBaseRef) || Boolean(request.pushTarget) || Boolean(request.branchNameOverride)) return {
		...request.baseBranch ? { baseBranch: request.baseBranch } : {},
		...request.compareBaseRef ? { compareBaseRef: request.compareBaseRef } : {}
	};
	return {
		baseBranch: void 0,
		compareBaseRef: void 0
	};
}
function appendProvisioningWarnings(creationId, warnings) {
	if (warnings.length === 0) return;
	appendProvisioningLog(creationId, warnings.map((warning) => warning.remediation ? `Warning: ${warning.message}\n${warning.remediation}\n` : `Warning: ${warning.message}\n`).join(""));
}
function appendProvisioningLog(creationId, chunk) {
	const store = useAppStore.getState();
	const entry = store.pendingWorktreeCreations[creationId];
	if (!entry) return;
	const nextLog = `${entry.provisioningLog ?? ""}${chunk}`.slice(-MAX_PROVISIONING_LOG_CHARS);
	store.updatePendingWorktreeCreation(creationId, { provisioningLog: nextLog });
}
async function attachEphemeralVmRuntimeToWorkspace(request, workspaceId) {
	if (!request.ephemeralVmRuntimeId) return;
	try {
		await window.api.ephemeralVm.attachWorkspace({
			runtimeId: request.ephemeralVmRuntimeId,
			workspaceId
		});
		if (request.ephemeralVmRuntimeEnvironmentId) useAppStore.getState().refreshRuntimeEnvironmentStatus(request.ephemeralVmRuntimeEnvironmentId);
	} catch (error) {
		console.error("Failed to attach ephemeral VM runtime to workspace:", error);
	}
}
function resolvePortableEphemeralVmProjectId(repo) {
	if (!repo) return null;
	const key = getProjectIdentityKey(repo);
	return key.startsWith("github:") ? key : null;
}
async function cleanupEphemeralVmRuntimeForFailedCreate(request) {
	if (!request.ephemeralVmRuntimeId) return;
	try {
		await window.api.ephemeralVm.cleanup({ runtimeId: request.ephemeralVmRuntimeId });
	} catch (error) {
		console.error("Failed to clean up ephemeral VM runtime after workspace creation failed:", error);
	}
}
var MISSING_BASE_REF_ANCHOR = "could not resolve a default base ref";
function formatWorkspaceCreateError(error) {
	const message = error instanceof Error ? error.message : "Failed to create worktree.";
	if (message.toLowerCase().includes(MISSING_BASE_REF_ANCHOR)) return {
		title: translate("auto.lib.workspace.create.error.format.64555d0014", "No base branch found"),
		message: translate("auto.lib.workspace.create.error.format.37cf0bc991", "Orca could not resolve a usable base ref for this workspace."),
		help: "Create an initial commit (for example on main), or select an existing branch in Create From, then try again."
	};
	return {
		title: message,
		message
	};
}
function getWorkspaceCreateErrorToastMessage(error) {
	return error.help ? error.title : error.message;
}
function resolveLaunchAgentTabId(state, args) {
	const worktreeTabs = state.tabsByWorktree[args.worktreeId] ?? [];
	if (args.backendSpawned && args.startupTerminalTabId) return getRuntimeEnvironmentIdForWorktree(state, args.worktreeId) ? toWebTerminalSurfaceTabId(args.startupTerminalTabId) : args.startupTerminalTabId;
	return worktreeTabs.find((tab) => tab.launchAgent === args.agent)?.id ?? args.primaryTabId ?? args.startupTerminalTabId ?? null;
}
function applyBackendSpawnedDraftViewMode(args) {
	const { state, request, agent, tabId, worktreeId, backendSpawned } = args;
	if (!backendSpawned || !request.launchDraftPrompt) return;
	const desiredViewMode = decideInitialAgentTabViewMode({
		experimentalNativeChat: state.settings?.experimentalNativeChat,
		openAgentTabsInChatByDefault: state.settings?.openAgentTabsInChatByDefault,
		agent,
		promptDelivery: "draft",
		launchDraftText: request.launchDraftPrompt,
		...nativeChatRequiresLocalTranscript(agent) ? { nativeChatTranscriptIsLocalReadable: isNativeChatTranscriptLocalReadable(getConnectionIdFromState(state, worktreeId)) } : {}
	}) ?? "terminal";
	const tab = state.unifiedTabsByWorktree?.[worktreeId]?.find((tab$1) => tab$1.id === tabId);
	if (!tab && getRuntimeEnvironmentIdForWorktree(state, worktreeId)) {
		__vitePreload(async () => {
			const { setWebRuntimeTabProps } = await import("./web-runtime-session-BV3mF8u4.js");
			return { setWebRuntimeTabProps };
		}, __vite__mapDeps([0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18]), import.meta.url).then(({ setWebRuntimeTabProps }) => setWebRuntimeTabProps({
			worktreeId,
			tabId,
			viewMode: desiredViewMode
		}));
		return;
	}
	if ((tab?.viewMode ?? "terminal") !== desiredViewMode) state.setTabViewMode(tabId, desiredViewMode);
}
function applyAgentTabSeeds(args) {
	const { request, agent, tabId } = args;
	applyBackendSpawnedDraftViewMode(args);
	seedNativeChatAppliedSessionOptions(tabId, agent, request.startupPlan?.sessionOptions);
	if (request.launchDraftPrompt) seedNativeChatLaunchDraftForAgentTab({
		tabId,
		agent,
		text: request.launchDraftPrompt
	});
}
function seedAgentTabStateAfterWorktreeCreate(args) {
	const { request, worktreeId, backendSpawned } = args;
	const agent = request.agent;
	if (!request.startupPlan || !agent) return;
	const state = useAppStore.getState();
	const tabId = resolveLaunchAgentTabId(state, {
		...args,
		agent
	});
	if (tabId) {
		applyAgentTabSeeds({
			state,
			request,
			agent,
			tabId,
			worktreeId,
			backendSpawned
		});
		return;
	}
	if ((state.tabsByWorktree[worktreeId] ?? []).length > 0) return;
	queueHookCommandsForFirstWorktreeTab({
		worktreeId,
		deliver: (state$1, firstTerminalTabId) => {
			const mirroredTabId = resolveLaunchAgentTabId(state$1, {
				...args,
				agent
			});
			if (mirroredTabId) {
				applyAgentTabSeeds({
					state: state$1,
					request,
					agent,
					tabId: mirroredTabId,
					worktreeId,
					backendSpawned
				});
				return;
			}
			if ((state$1.tabsByWorktree[worktreeId] ?? []).length === 1) applyAgentTabSeeds({
				state: state$1,
				request,
				agent,
				tabId: firstTerminalTabId,
				worktreeId,
				backendSpawned
			});
		}
	});
}
function resolveBackendDraftStartup(request) {
	if (!request.startup || !request.agent || !request.launchDraftPrompt) return request.startup;
	const state = useAppStore.getState();
	const repo = state.repos.find((entry) => entry.id === request.repoId);
	const connectionId = repo ? repo.connectionId ?? null : void 0;
	const viewMode = decideInitialAgentTabViewMode({
		experimentalNativeChat: state.settings?.experimentalNativeChat,
		openAgentTabsInChatByDefault: state.settings?.openAgentTabsInChatByDefault,
		agent: request.agent,
		promptDelivery: "draft",
		launchDraftText: request.launchDraftPrompt,
		...nativeChatRequiresLocalTranscript(request.agent) ? { nativeChatTranscriptIsLocalReadable: isNativeChatTranscriptLocalReadable(connectionId) } : {}
	}) ?? "terminal";
	return {
		...request.startup,
		viewMode
	};
}
function buildWorktreeCreationStartupOpt(request, backendSpawned) {
	const plan = request.startupPlan;
	if (!plan || backendSpawned) return;
	return {
		command: plan.launchCommand,
		...plan.env ? { env: plan.env } : {},
		launchConfig: plan.launchConfig,
		...plan.launchToken ? { launchToken: plan.launchToken } : {},
		...request.agent ? { launchAgent: request.agent } : {},
		...plan.draftPrompt ? { draftPrompt: plan.draftPrompt } : {},
		...request.launchDraftPrompt ? { launchDraftText: request.launchDraftPrompt } : {},
		...plan.startupCommandDelivery ? { startupCommandDelivery: plan.startupCommandDelivery } : {},
		...request.agent === "command-code" && request.quickPrompt.trim().length > 0 ? { initialAgentStatus: {
			agent: request.agent,
			prompt: request.quickPrompt.trim()
		} } : {},
		...request.quickTelemetry ? { telemetry: request.quickTelemetry } : {}
	};
}
function getWorktreeCreationIndeterminate(request) {
	if (request.worktreeCreateProgressMode) return request.worktreeCreateProgressMode === "indeterminate";
	return getActiveRuntimeTarget(useAppStore.getState().settings).kind !== "local";
}
function getInitialWorktreeCreationPhase(request) {
	return request.ephemeralVmRecipe && !request.ephemeralVmRuntimeId ? "provisioning-vm" : "fetching";
}
function isPendingCreationSurfaceVisible(creationId) {
	const state = useAppStore.getState();
	return state.activeView === "terminal" && state.activePendingCreationId === creationId;
}
function revealPendingCreation(creationId, request, phase) {
	const store = useAppStore.getState();
	const indeterminate = getWorktreeCreationIndeterminate(request);
	store.beginPendingWorktreeCreation({
		creationId,
		phase,
		status: "creating",
		startedAt: Date.now(),
		indeterminate,
		loaderVisible: true,
		request
	});
	store.setActiveView("terminal");
	store.setSidebarOpen(true);
}
async function preflightAgentTrust(request, path, connectionId) {
	if (!request.agent || !window.api.agentTrust?.markTrusted) return;
	const preflight = TUI_AGENT_CONFIG[request.agent].preflightTrust;
	if (!preflight) return;
	try {
		await window.api.agentTrust.markTrusted({
			preset: preflight,
			workspacePath: path,
			...connectionId ? { connectionId } : {}
		});
	} catch {}
}
async function executeWorktreeCreation(creationId, request) {
	const preparedRequest = await prepareRequestForCreate(creationId, request);
	if (!preparedRequest) return;
	let result;
	try {
		const backendStartup = resolveBackendDraftStartup(preparedRequest);
		result = await useAppStore.getState().createWorktree(preparedRequest.repoId, preparedRequest.name, preparedRequest.baseBranch, preparedRequest.setupDecision, preparedRequest.sparseCheckout, preparedRequest.telemetrySource, preparedRequest.displayName, preparedRequest.linkedIssue, preparedRequest.linkedPR, preparedRequest.pushTarget, preparedRequest.agent ?? void 0, preparedRequest.linkedLinearIssue, preparedRequest.branchNameOverride, preparedRequest.workspaceStatus, preparedRequest.linkedGitLabMR, preparedRequest.linkedGitLabIssue, backendStartup, preparedRequest.pendingFirstAgentMessageRename, creationId, preparedRequest.linkedLinearIssueWorkspaceId, preparedRequest.linkedLinearIssueOrganizationUrlKey, preparedRequest.linkedBitbucketPR, preparedRequest.linkedAzureDevOpsPR, preparedRequest.linkedGiteaPR, preparedRequest.compareBaseRef, {
			...preparedRequest.linkedWorkItem !== void 0 ? { linkedWorkItem: preparedRequest.linkedWorkItem } : {},
			...preparedRequest.linkedTaskSourceContext !== void 0 ? { linkedTaskSourceContext: preparedRequest.linkedTaskSourceContext } : {},
			...!backendStartup && preparedRequest.agent && preparedRequest.launchDraftPrompt ? { startupDraft: preparedRequest.launchDraftPrompt } : {}
		});
	} catch (error) {
		if (!useAppStore.getState().pendingWorktreeCreations[creationId]) return;
		await cleanupEphemeralVmRuntimeForFailedCreate(preparedRequest);
		const message = getWorkspaceCreateErrorToastMessage(formatWorkspaceCreateError(error));
		useAppStore.getState().updatePendingWorktreeCreation(creationId, {
			status: "error",
			error: message,
			...preparedRequest.ephemeralVmRecipe ? { request } : {}
		});
		if (!isPendingCreationSurfaceVisible(creationId)) toast.error(message);
		return;
	}
	const worktree = result.worktree;
	if (!useAppStore.getState().pendingWorktreeCreations[creationId]) return;
	await attachEphemeralVmRuntimeToWorkspace(preparedRequest, worktree.id);
	const backendSpawned = result.startupTerminal?.spawned === true;
	if (preparedRequest.startupPlan && !backendSpawned && !preparedRequest.startupPlan.launchToken) preparedRequest.startupPlan.launchToken = createBrowserUuid();
	const startupOpt = buildWorktreeCreationStartupOpt(preparedRequest, backendSpawned);
	if (worktree.path) {
		const repoConnectionId = useAppStore.getState().repos.find((repo) => repo.id === worktree.repoId)?.connectionId ?? null;
		await preflightAgentTrust(preparedRequest, worktree.path, repoConnectionId);
	}
	const completionState = useAppStore.getState();
	const shouldActivateOnCompletion = completionState.pendingWorktreeCreations[creationId] !== void 0 && (isPendingCreationSurfaceVisible(creationId) || completionState.activeView === "terminal" && completionState.activePendingCreationId === null);
	let activation = false;
	let primaryTabId;
	if (shouldActivateOnCompletion) {
		activation = activateAndRevealWorktree(worktree.id, {
			sidebarRevealBehavior: "auto",
			...result.setup ? { setup: result.setup } : {},
			...result.defaultTabs ? { defaultTabs: result.defaultTabs } : {},
			...startupOpt ? { startup: startupOpt } : {},
			...preparedRequest.issueCommand ? { issueCommand: preparedRequest.issueCommand } : {},
			...backendSpawned ? { backendStartupTerminalSpawned: true } : {}
		});
		primaryTabId = activation === false ? null : activation.primaryTabId;
	} else primaryTabId = ensureWorktreeHasInitialTerminal(useAppStore.getState(), worktree.id, startupOpt, result.setup, preparedRequest.issueCommand, result.defaultTabs, {
		activateCreatedTabs: false,
		...backendSpawned ? { backendStartupTerminalSpawned: true } : {}
	});
	useAppStore.getState().removePendingWorktreeCreation(creationId, { cleanupVm: false });
	seedAgentTabStateAfterWorktreeCreate({
		request: preparedRequest,
		worktreeId: worktree.id,
		primaryTabId,
		startupTerminalTabId: result.startupTerminal?.tabId,
		backendSpawned
	});
	if (preparedRequest.startupPlan && !backendSpawned) ensureAgentStartupInTerminal({
		worktreeId: worktree.id,
		primaryTabId,
		startup: preparedRequest.startupPlan
	});
	if (shouldActivateOnCompletion && !preparedRequest.suppressTerminalFocusOnCompletion) queueWorkspaceActivationTerminalFocus(worktree.id, activation);
	if (preparedRequest.note) try {
		await useAppStore.getState().updateWorktreeMeta(worktree.id, { comment: preparedRequest.note });
	} catch {
		console.error("Failed to update worktree meta after creation");
	}
}
function runBackgroundWorktreeCreation(request) {
	const store = useAppStore.getState();
	const existingCreationId = findPendingLinkedWorkItemCreationId(store.pendingWorktreeCreations, request);
	if (existingCreationId) {
		store.setActivePendingWorktreeCreation(existingCreationId);
		store.setActiveView("terminal");
		store.setSidebarOpen(true);
		return existingCreationId;
	}
	const creationId = createBrowserUuid();
	revealPendingCreation(creationId, request, getInitialWorktreeCreationPhase(request));
	executeWorktreeCreation(creationId, request);
	return creationId;
}
function retryBackgroundWorktreeCreation(creationId) {
	const store = useAppStore.getState();
	const entry = store.pendingWorktreeCreations[creationId];
	if (!entry) return;
	store.updatePendingWorktreeCreation(creationId, {
		status: "creating",
		startedAt: Date.now(),
		phase: entry.request.ephemeralVmRecipe && !entry.request.ephemeralVmRuntimeId ? "provisioning-vm" : "fetching",
		error: void 0,
		provisioningLog: void 0
	});
	store.setActivePendingWorktreeCreation(creationId);
	store.setActiveView("terminal");
	store.setSidebarOpen(true);
	executeWorktreeCreation(creationId, entry.request);
}
export { findPendingLinkedWorkItemCreationId as a, getWorkspaceCreateErrorToastMessage as i, runBackgroundWorktreeCreation as n, getCreationProgressLabel as o, formatWorkspaceCreateError as r, retryBackgroundWorktreeCreation as t };
