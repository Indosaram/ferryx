import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { t as OpenInApplicationIcon } from "./open-in-app-catalog-DpQuLNDD.js";
import { t as ExternalLink } from "./external-link-BrcDtGAn.js";
import { t as FolderOpen } from "./folder-open-B2ZB-rfY.js";
import { Bt as showLocalPathOpenBlockedToast, t as useAppStore, zt as isLocalPathOpenBlocked } from "./store-CgXrfmaH.js";
import { n as toast } from "./dist-DgqligFk.js";
import { d as DropdownMenuSub, f as DropdownMenuSubContent, i as DropdownMenuItem, l as DropdownMenuSeparator, p as DropdownMenuSubTrigger } from "./dropdown-menu-Dth6LPK-.js";
function getLocalFileManagerLabel(userAgent) {
	const resolvedUserAgent = userAgent ?? (typeof navigator === "undefined" ? "" : navigator.userAgent);
	if (resolvedUserAgent.includes("Mac")) return "Finder";
	if (resolvedUserAgent.includes("Windows")) return "File Explorer";
	return "File Manager";
}
var VSCODE_LAUNCHER_NAMES = new Set([
	"code",
	"code-insiders",
	"code - insiders"
]);
var WINDOWS_ABSOLUTE_PATH = /^(?:[a-z]:[\\/]|\\\\)/i;
function stripMatchingQuotes(value) {
	const trimmed = value.trim();
	const quote = trimmed[0];
	if ((quote === "\"" || quote === "'") && trimmed.endsWith(quote)) return trimmed.slice(1, -1);
	return trimmed;
}
function isVsCodeLauncherExecutable(command) {
	const launcherName = (stripMatchingQuotes(command).split(/[\\/]/).at(-1) ?? "").replace(/\.(?:cmd|exe|bat)$/i, "").toLowerCase();
	return VSCODE_LAUNCHER_NAMES.has(launcherName);
}
function isVsCodeRemoteSshCommand(command) {
	const unquoted = stripMatchingQuotes(command?.trim() || "code");
	if (!/\s/.test(unquoted)) return isVsCodeLauncherExecutable(unquoted);
	return (unquoted.startsWith("/") || WINDOWS_ABSOLUTE_PATH.test(unquoted)) && isVsCodeLauncherExecutable(unquoted);
}
function getExternalEditorOpenCapability(settings, context) {
	if (settings?.activeRuntimeEnvironmentId?.trim()) return {
		allowed: false,
		reason: "remote-runtime"
	};
	if (!context.connectionId?.trim()) return {
		allowed: true,
		remote: false
	};
	return isVsCodeRemoteSshCommand(context.command) ? {
		allowed: true,
		remote: true
	} : {
		allowed: false,
		reason: "local-only-editor"
	};
}
const NO_OPEN_IN_APPLICATIONS = [];
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function getWorktreeOpenInEntries(openInApplications, fileManagerLabel) {
	return [...openInApplications.map((application) => ({
		id: application.id,
		label: application.label,
		target: "external-editor",
		command: application.command
	})), {
		id: "file-manager",
		label: fileManagerLabel,
		target: "file-manager"
	}];
}
function getOpenInEntryAvailability(entry, settings, connectionId) {
	if (entry.target === "file-manager") return isLocalPathOpenBlocked(settings, { connectionId }) ? {
		disabled: true,
		metadata: translate("auto.components.sidebar.WorktreeOpenInMenu.localOnly", "Local only")
	} : { disabled: false };
	const capability = getExternalEditorOpenCapability(settings, {
		connectionId,
		command: entry.command
	});
	if (!capability.allowed) return {
		disabled: true,
		metadata: translate("auto.components.sidebar.WorktreeOpenInMenu.localOnly", "Local only")
	};
	return capability.remote ? {
		disabled: false,
		metadata: translate("auto.components.sidebar.WorktreeOpenInMenu.remoteSsh", "Remote SSH")
	} : { disabled: false };
}
function showOpenFailureToast(result, remote) {
	if (result.reason === "remote-runtime-unsupported") {
		toast.error(translate("auto.components.sidebar.WorktreeOpenInMenu.remoteRuntimeUnsupported", "Opening this path in a local app is not available."), { description: translate("auto.components.sidebar.WorktreeOpenInMenu.remoteRuntimeUnsupportedDetail", "Switch to a local or SSH workspace, then try again.") });
		return;
	}
	if (result.reason === "ssh-target-not-found") {
		toast.error(translate("auto.components.sidebar.WorktreeOpenInMenu.sshTargetNotFound", "SSH host is no longer available."), { description: translate("auto.components.sidebar.WorktreeOpenInMenu.sshTargetNotFoundDetail", "Refresh workspaces or reconnect the host, then try again.") });
		return;
	}
	if (result.reason === "ssh-target-invalid") {
		toast.error(translate("auto.components.sidebar.WorktreeOpenInMenu.sshTargetInvalid", "SSH host configuration is incomplete."), { description: translate("auto.components.sidebar.WorktreeOpenInMenu.sshTargetInvalidDetail", "Edit or reconnect the SSH host, then try again.") });
		return;
	}
	if (result.reason === "ssh-alias-required") {
		toast.error(translate("auto.components.sidebar.WorktreeOpenInMenu.sshAliasRequired", "VS Code needs an SSH config alias for this host."), { description: translate("auto.components.sidebar.WorktreeOpenInMenu.sshAliasRequiredDetail", "Add a Host alias for {{host}}:{{port}} to your local SSH config, reconnect the workspace, then try again.", {
			host: result.host,
			port: result.port
		}) });
		return;
	}
	if (result.reason === "remote-editor-unsupported") {
		toast.error(translate("auto.components.sidebar.WorktreeOpenInMenu.remoteEditorUnsupported", "This app cannot open SSH workspaces."), { description: translate("auto.components.sidebar.WorktreeOpenInMenu.remoteEditorUnsupportedDetail", "Choose VS Code or use the app locally.") });
		return;
	}
	if (result.reason === "not-absolute") {
		toast.error(remote ? translate("auto.components.sidebar.WorktreeOpenInMenu.remotePathInvalid", "Path is not valid for the SSH host.") : translate("auto.components.sidebar.WorktreeOpenInMenu.f387af445b", "Workspace path is not a valid local path."), remote ? { description: translate("auto.components.sidebar.WorktreeOpenInMenu.remotePathInvalidDetail", "Refresh the workspace before trying again.") } : void 0);
		return;
	}
	if (result.reason === "not-found") {
		toast.error(translate("auto.components.sidebar.WorktreeOpenInMenu.3921d3d9a5", "Workspace folder was not found."), { description: translate("auto.components.sidebar.WorktreeOpenInMenu.0bed8727db", "It may have been moved or deleted. Refresh workspaces or remove it from Orca.") });
		return;
	}
	if (remote) {
		toast.error(translate("auto.components.sidebar.WorktreeOpenInMenu.remoteLaunchFailed", "Could not open the path in VS Code."), { description: translate("auto.components.sidebar.WorktreeOpenInMenu.remoteLaunchFailedDetail", "Check the VS Code command configured on this machine.") });
		return;
	}
	toast.error(translate("auto.components.sidebar.WorktreeOpenInMenu.9a5381eb09", "Could not open workspace folder."), { description: translate("auto.components.sidebar.WorktreeOpenInMenu.bd0e8159f8", "Check the editor command or file manager configuration on this machine.") });
}
function stopMenuPropagation(event) {
	event.stopPropagation();
}
function openOpenInAppsSettings() {
	const store = useAppStore.getState();
	store.openSettingsTarget({
		pane: "general",
		repoId: null,
		sectionId: "general-open-in-apps"
	});
	store.openSettingsPage();
}
async function openWorktreePath(args) {
	const settings = useAppStore.getState().settings;
	if (args.target === "file-manager") {
		if (isLocalPathOpenBlocked(settings, { connectionId: args.connectionId ?? null })) {
			showLocalPathOpenBlockedToast();
			return;
		}
	} else {
		const capability = getExternalEditorOpenCapability(settings, {
			connectionId: args.connectionId,
			command: args.command
		});
		if (!capability.allowed) {
			if (capability.reason === "remote-runtime") showOpenFailureToast({
				ok: false,
				reason: "remote-runtime-unsupported"
			}, false);
			else showOpenFailureToast({
				ok: false,
				reason: "remote-editor-unsupported"
			}, true);
			return;
		}
	}
	const result = args.target === "file-manager" ? await window.api.shell.openInFileManager(args.worktreePath) : await window.api.shell.openInExternalEditor({
		path: args.worktreePath,
		command: args.command,
		connectionId: args.connectionId
	});
	if (!result.ok) showOpenFailureToast(result, Boolean(args.connectionId?.trim()));
}
function useOpenInWorktreePath({ worktreePath, connectionId }) {
	return (0, import_react.useCallback)(async (target, command) => {
		await openWorktreePath({
			target,
			worktreePath,
			connectionId,
			command
		});
	}, [connectionId, worktreePath]);
}
function WorktreeOpenInMenuItems({ worktreePath, connectionId, disabled, labelPrefix = "" }) {
	const openInWorktreePath = useOpenInWorktreePath({
		worktreePath,
		connectionId
	});
	const openInApplications = useAppStore((s) => s.settings?.openInApplications ?? NO_OPEN_IN_APPLICATIONS);
	const settings = useAppStore((s) => s.settings);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(import_jsx_runtime.Fragment, { children: getWorktreeOpenInEntries(openInApplications, getLocalFileManagerLabel()).map((entry) => {
		const availability = getOpenInEntryAvailability(entry, settings, connectionId);
		return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuItem, {
			onClick: stopMenuPropagation,
			onSelect: () => {
				openInWorktreePath(entry.target, entry.command);
			},
			disabled: disabled || availability.disabled,
			children: [
				entry.target === "file-manager" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(FolderOpen, { className: "size-3.5" }) : entry.command ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(OpenInApplicationIcon, {
					application: { command: entry.command },
					size: 14
				}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExternalLink, { className: "size-3.5" }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
					className: "min-w-0 truncate",
					children: [labelPrefix, entry.label]
				}),
				availability.metadata ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "ml-auto shrink-0 text-[11px] text-muted-foreground",
					children: availability.metadata
				}) : null
			]
		}, entry.id);
	}) });
}
function WorktreeOpenInSubMenu({ worktreePath, connectionId, disabled }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuSub, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuSubTrigger, {
		disabled,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(FolderOpen, { className: "size-3.5" }), translate("auto.components.sidebar.WorktreeOpenInMenu.8009ab69a6", "Open in")]
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DropdownMenuSubContent, {
		className: "w-52",
		onClick: stopMenuPropagation,
		onPointerDown: stopMenuPropagation,
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(WorktreeOpenInMenuItems, {
				worktreePath,
				connectionId,
				disabled
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuSeparator, {}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DropdownMenuItem, {
				onClick: stopMenuPropagation,
				onSelect: openOpenInAppsSettings,
				disabled,
				children: translate("auto.components.sidebar.WorktreeOpenInMenu.1417fd8380", "Customize apps...")
			})
		]
	})] });
}
export { openOpenInAppsSettings as a, getLocalFileManagerLabel as c, getWorktreeOpenInEntries as i, WorktreeOpenInSubMenu as n, openWorktreePath as o, getOpenInEntryAvailability as r, NO_OPEN_IN_APPLICATIONS as s, WorktreeOpenInMenuItems as t };
