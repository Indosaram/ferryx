import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn, t as Button } from "./button-DszXJEV6.js";
import { t as ChevronDown } from "./chevron-down-BRkP96Md.js";
import { G as buildSidebarHostOptions, K as buildSidebarHostScopeOptions } from "./worktree-activation-BDsaiyMf.js";
import { j as getHostDisplayLabelOverrides, t as useAppStore } from "./store-CgXrfmaH.js";
import { t as Label } from "./label-D-n9s_wS.js";
import { s as SettingsSwitch } from "./SettingsFormControls-C0chb_HE.js";
import { t as Input } from "./input-DV5rpysh.js";
import { n as CollapsibleContent, r as CollapsibleTrigger, t as Collapsible } from "./collapsible-raq6sIQA.js";
import { i as MIN_SSH_RELAY_GRACE_PERIOD_SECONDS, n as DEFAULT_SSH_RELAY_GRACE_PERIOD_SECONDS, r as MAX_SSH_RELAY_GRACE_PERIOD_SECONDS, t as DEFAULT_BOUNDED_SSH_RELAY_GRACE_PERIOD_SECONDS } from "./ssh-types-Caw2Ltsn.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
function useSidebarHostScopeOptions() {
	const repos = useAppStore((s) => s.repos);
	const sshTargetLabels = useAppStore((s) => s.sshTargetLabels);
	const sshConnectionStates = useAppStore((s) => s.sshConnectionStates);
	const settings = useAppStore((s) => s.settings);
	const runtimeEnvironments = useAppStore((s) => s.runtimeEnvironments);
	const runtimeStatusByEnvironmentId = useAppStore((s) => s.runtimeStatusByEnvironmentId);
	const hostLabelOverrides = (0, import_react.useMemo)(() => getHostDisplayLabelOverrides(settings), [settings]);
	const hostOptions = (0, import_react.useMemo)(() => buildSidebarHostOptions({
		repos,
		sshTargetLabels,
		sshConnectionStates,
		settings,
		runtimeEnvironments,
		runtimeStatusByEnvironmentId,
		hostLabelOverrides
	}), [
		repos,
		sshTargetLabels,
		sshConnectionStates,
		settings,
		runtimeEnvironments,
		runtimeStatusByEnvironmentId,
		hostLabelOverrides
	]);
	return {
		hostOptions,
		hostScopeOptions: (0, import_react.useMemo)(() => buildSidebarHostScopeOptions(hostOptions), [hostOptions])
	};
}
const EMPTY_FORM = {
	label: "",
	configHost: "",
	host: "",
	port: "22",
	username: "",
	identityFile: "",
	gssapiAuthentication: false,
	proxyCommand: "",
	jumpHost: "",
	systemSshConnectionReuse: true,
	relayGracePeriodSeconds: String(DEFAULT_BOUNDED_SSH_RELAY_GRACE_PERIOD_SECONDS),
	relayKeepAliveUntilReset: true
};
function getEditingTargetForSshTarget(target) {
	const configHost = target.configHost && target.configHost !== target.host ? target.configHost : "";
	return {
		label: target.label,
		configHost,
		host: target.host,
		port: String(target.port),
		username: target.username,
		identityFile: target.identityFile ?? "",
		gssapiAuthentication: target.gssapiAuthentication === true,
		proxyCommand: target.proxyCommand ?? "",
		jumpHost: target.jumpHost ?? "",
		systemSshConnectionReuse: target.systemSshConnectionReuse !== false,
		relayGracePeriodSeconds: String(target.relayGracePeriodSeconds === 0 ? DEFAULT_BOUNDED_SSH_RELAY_GRACE_PERIOD_SECONDS : target.relayGracePeriodSeconds ?? 86400),
		relayKeepAliveUntilReset: (target.relayGracePeriodSeconds ?? 0) === 0
	};
}
function getEditingTargetFromSshConfigHost(host) {
	const configHost = host.alias !== host.hostname ? host.alias : "";
	return {
		...EMPTY_FORM,
		label: host.alias,
		configHost,
		host: host.hostname,
		port: String(host.port),
		username: host.username,
		identityFile: "",
		gssapiAuthentication: host.gssapiAuthentication === true,
		proxyCommand: host.proxyCommand ?? "",
		jumpHost: host.jumpHost ?? ""
	};
}
function parseSshHostInput(rawInput) {
	const input = rawInput.trim();
	if (!input) return null;
	if (/^ssh:\/\//i.test(input)) return parseSshUrl(input);
	const atIndex = input.lastIndexOf("@");
	const username = atIndex > 0 ? input.slice(0, atIndex).trim() : void 0;
	const parsed = parseHostAndOptionalPort(atIndex > 0 ? input.slice(atIndex + 1).trim() : input);
	if (!parsed.host) return null;
	return {
		host: parsed.host,
		username,
		port: parsed.port,
		invalidPort: parsed.invalidPort,
		configHost: parsed.host
	};
}
function applyParsedSshHostInput(draft) {
	const parsed = parseSshHostInput(draft.host);
	if (!parsed || parsed.invalidPort) return draft;
	return {
		...draft,
		host: parsed.host,
		configHost: draft.configHost.trim() || parsed.configHost,
		username: draft.username.trim() || parsed.username || "",
		port: parsed.port !== void 0 && isDefaultPortDraft(draft.port) ? String(parsed.port) : draft.port
	};
}
function getSshTargetDraftConnectionFields(draft) {
	const parsed = parseSshHostInput(draft.host);
	const host = parsed?.host ?? draft.host.trim();
	const configHost = draft.configHost.trim() || parsed?.configHost || host;
	const username = draft.username.trim() || parsed?.username || "";
	const parsedPort = Number.parseInt(draft.port, 10);
	return {
		host,
		configHost,
		username,
		port: parsed?.invalidPort === true ? NaN : parsed?.port !== void 0 && isDefaultPortDraft(draft.port) ? parsed.port : parsedPort
	};
}
function hasAdvancedConnectionValues(form) {
	return form.proxyCommand.trim().length > 0 || form.jumpHost.trim().length > 0 || !form.systemSshConnectionReuse;
}
function isSshTargetFormDirty(current, baseline) {
	return current.label !== baseline.label || current.configHost !== baseline.configHost || current.host !== baseline.host || current.port !== baseline.port || current.username !== baseline.username || current.identityFile !== baseline.identityFile || current.gssapiAuthentication !== baseline.gssapiAuthentication || current.proxyCommand !== baseline.proxyCommand || current.jumpHost !== baseline.jumpHost || current.systemSshConnectionReuse !== baseline.systemSshConnectionReuse || current.relayGracePeriodSeconds !== baseline.relayGracePeriodSeconds || current.relayKeepAliveUntilReset !== baseline.relayKeepAliveUntilReset;
}
function parseRelayGracePeriodSeconds(draft) {
	return draft.relayKeepAliveUntilReset ? 0 : Number.parseInt(draft.relayGracePeriodSeconds, 10);
}
function isRelayGracePeriodValid(draft, graceSeconds) {
	return draft.relayKeepAliveUntilReset || !Number.isNaN(graceSeconds) && graceSeconds >= 60 && graceSeconds <= 604800;
}
function parseSshUrl(input) {
	try {
		const url = new URL(input);
		if (url.protocol !== "ssh:" || !url.hostname) return null;
		const host = url.hostname.replace(/^\[|\]$/g, "");
		const port = url.port ? parsePort(url.port) : void 0;
		if (url.port && port === void 0) return {
			host,
			username: decodeSshUrlUsername(url.username),
			configHost: host,
			invalidPort: true
		};
		return {
			host,
			username: decodeSshUrlUsername(url.username),
			port,
			configHost: host
		};
	} catch {
		return parseSshUrlWithInvalidPort(input);
	}
}
function parseSshUrlWithInvalidPort(input) {
	const match = input.match(/^ssh:\/\/(?:([^@/?#]*)@)?(\[[^\]]+\]|[^:/?#]+):([^/?#]*)(?:[/?#]|$)/i);
	if (!match) return null;
	const rawHost = match[2];
	const host = rawHost.startsWith("[") && rawHost.endsWith("]") ? rawHost.slice(1, -1) : rawHost;
	if (parsePort(match[3]) !== void 0) return null;
	return {
		host,
		username: decodeSshUrlUsername(match[1] ?? ""),
		configHost: host,
		invalidPort: true
	};
}
function decodeSshUrlUsername(value) {
	if (!value) return;
	try {
		return decodeURIComponent(value);
	} catch {
		return value;
	}
}
function parseHostAndOptionalPort(input) {
	if (input.startsWith("[")) {
		const closeIndex = input.indexOf("]");
		if (closeIndex > 1) {
			const host = input.slice(1, closeIndex);
			const suffix = input.slice(closeIndex + 1);
			if (suffix.startsWith(":")) {
				const port = parsePort(suffix.slice(1));
				return port === void 0 ? {
					host,
					invalidPort: true
				} : {
					host,
					port
				};
			}
			return { host };
		}
	}
	const firstColon = input.indexOf(":");
	if (firstColon !== -1 && firstColon === input.lastIndexOf(":")) {
		const host = input.slice(0, firstColon);
		const port = parsePort(input.slice(firstColon + 1));
		if (host) return port === void 0 ? {
			host,
			invalidPort: true
		} : {
			host,
			port
		};
	}
	return { host: input };
}
function parsePort(value) {
	if (!/^\d+$/.test(value)) return;
	const port = Number(value);
	return isValidPort(port) ? port : void 0;
}
function isValidPort(port) {
	return Number.isInteger(port) && port >= 1 && port <= 65535;
}
function isDefaultPortDraft(value) {
	const trimmed = value.trim();
	return trimmed === "" || trimmed === "22";
}
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function SshHostAdvancedFields({ open, onOpenChange, form, disabled, onFormChange }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Collapsible, {
		open,
		onOpenChange,
		className: "col-span-2 sm:col-span-2",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CollapsibleTrigger, {
			asChild: true,
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
				type: "button",
				variant: "ghost",
				size: "sm",
				className: "px-2 text-xs",
				children: [translate("auto.components.sidebar.AddRemoteHostDialog.advanced", "Advanced"), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronDown, { className: cn("size-4 transition-transform", open && "rotate-180") })]
			})
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CollapsibleContent, {
			className: "collapsible-height-content",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-4 pt-3",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "space-y-1.5",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
								htmlFor: "add-ssh-proxy-command",
								children: translate("auto.components.settings.SshTargetForm.c7d0e18ecb", "Proxy Command")
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
								id: "add-ssh-proxy-command",
								value: form.proxyCommand,
								disabled,
								onChange: (e) => onFormChange((f) => ({
									...f,
									proxyCommand: e.target.value
								})),
								placeholder: translate("auto.components.settings.SshTargetForm.f42d844544", "e.g. cloudflared access ssh --hostname %h")
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "text-[11px] text-muted-foreground",
								children: translate("auto.components.settings.SshTargetForm.3b01ca44a0", "Optional. Used for tunneling (e.g. Cloudflare Access, ProxyCommand).")
							})
						]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "space-y-1.5",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
								htmlFor: "add-ssh-jump-host",
								children: translate("auto.components.settings.SshTargetForm.b2ab248ded", "Jump Host")
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
								id: "add-ssh-jump-host",
								value: form.jumpHost,
								disabled,
								onChange: (e) => onFormChange((f) => ({
									...f,
									jumpHost: e.target.value
								})),
								placeholder: translate("auto.components.settings.SshTargetForm.11bcb4507a", "bastion.example.com")
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "text-[11px] text-muted-foreground",
								children: translate("auto.components.settings.SshTargetForm.feae1d1e69", "Optional. Equivalent to ProxyJump / ssh -J.")
							})
						]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-start justify-between gap-4 py-1 text-xs",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "min-w-0 flex-1 space-y-0.5",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
								className: "text-xs font-medium",
								children: translate("auto.components.settings.SshTargetForm.8c922dffba", "Reuse SSH connection for faster setup")
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "text-muted-foreground",
								children: translate("auto.components.settings.SshTargetForm.53e9aabfc0", "Uses OpenSSH multiplexing when available. Turn off for hosts with custom SSH restrictions.")
							})]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsSwitch, {
							checked: form.systemSshConnectionReuse,
							disabled,
							onChange: () => onFormChange((f) => ({
								...f,
								systemSshConnectionReuse: !f.systemSshConnectionReuse
							})),
							ariaLabel: translate("auto.components.settings.SshTargetForm.8c922dffba", "Reuse SSH connection for faster setup")
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-start justify-between gap-4 py-1 text-xs",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "min-w-0 flex-1 space-y-0.5",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
								className: "text-xs font-medium",
								children: translate("auto.components.settings.SshTargetForm.71fc546097", "Keep terminals alive until reset")
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "text-muted-foreground",
								children: translate("auto.components.settings.SshTargetForm.b574994adc", "Use End Remote Terminals or Reset Relay when you want to stop them.")
							})]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsSwitch, {
							checked: form.relayKeepAliveUntilReset,
							disabled,
							onChange: () => onFormChange((f) => ({
								...f,
								relayKeepAliveUntilReset: !f.relayKeepAliveUntilReset
							})),
							ariaLabel: translate("auto.components.settings.SshTargetForm.71fc546097", "Keep terminals alive until reset")
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "space-y-1.5",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
								htmlFor: "add-ssh-relay-grace-period",
								className: "text-xs text-muted-foreground",
								children: translate("auto.components.settings.SshTargetForm.55c56cf2c7", "Timeout after disconnect (seconds)")
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
								id: "add-ssh-relay-grace-period",
								type: form.relayKeepAliveUntilReset ? "text" : "number",
								value: form.relayKeepAliveUntilReset ? translate("auto.components.settings.SshTargetForm.7c13f58c91", "Until reset") : form.relayGracePeriodSeconds,
								disabled: disabled || form.relayKeepAliveUntilReset,
								onChange: (e) => onFormChange((f) => ({
									...f,
									relayGracePeriodSeconds: e.target.value
								})),
								placeholder: String(DEFAULT_BOUNDED_SSH_RELAY_GRACE_PERIOD_SECONDS),
								min: 60,
								max: MAX_SSH_RELAY_GRACE_PERIOD_SECONDS
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "text-[11px] text-muted-foreground",
								children: translate("auto.components.settings.SshTargetForm.1b19b00e93", "Bounded timeouts must be between 60 seconds and 7 days.")
							})
						]
					})
				]
			})
		})]
	});
}
export { getEditingTargetFromSshConfigHost as a, isRelayGracePeriodValid as c, useSidebarHostScopeOptions as d, getEditingTargetForSshTarget as i, isSshTargetFormDirty as l, EMPTY_FORM as n, getSshTargetDraftConnectionFields as o, applyParsedSshHostInput as r, hasAdvancedConnectionValues as s, SshHostAdvancedFields as t, parseRelayGracePeriodSeconds as u };
