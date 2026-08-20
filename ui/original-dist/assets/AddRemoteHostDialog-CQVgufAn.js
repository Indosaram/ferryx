import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { l as createLucideIcon, n as cn, t as Button } from "./button-DszXJEV6.js";
import { t as useAppStore } from "./store-CgXrfmaH.js";
import { n as toast } from "./dist-DgqligFk.js";
import { t as Checkbox } from "./checkbox-PAbetBh2.js";
import { t as Label } from "./label-D-n9s_wS.js";
import { t as Badge } from "./badge-BBptl5GG.js";
import { a as getEditingTargetFromSshConfigHost, c as isRelayGracePeriodValid, n as EMPTY_FORM, o as getSshTargetDraftConnectionFields, r as applyParsedSshHostInput, s as hasAdvancedConnectionValues, t as SshHostAdvancedFields, u as parseRelayGracePeriodSeconds } from "./SshHostAdvancedFields-DHVKhP0i.js";
import { a as DialogFooter, i as DialogDescription, o as DialogHeader, r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
import { t as Input } from "./input-DV5rpysh.js";
import { a as SSH_CONFIG_HOST_RESULT_LIMIT, r as MAX_SSH_RELAY_GRACE_PERIOD_SECONDS } from "./ssh-types-Caw2Ltsn.js";
import { i as parseHostAccessLink, n as translateRemotePairingEndpointKind, r as translateRemotePairingFailureDescription, t as translateHostAccessLinkError } from "./remote-pairing-copy-BzsTvPoC.js";
var Lightbulb = createLucideIcon("lightbulb", [
	["path", {
		d: "M15 14c.2-1 .7-1.7 1.5-2.5 1-.9 1.5-2.2 1.5-3.5A6 6 0 0 0 6 8c0 1 .2 2.2 1.5 3.5.7.7 1.3 1.5 1.5 2.5",
		key: "1gvzjb"
	}],
	["path", {
		d: "M9 18h6",
		key: "x1upvd"
	}],
	["path", {
		d: "M10 22h4",
		key: "ceow96"
	}]
]);
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var EMPTY_CONFIG_HOSTS = [];
function AddRemoteHostSshConfigPicker({ hosts = EMPTY_CONFIG_HOSTS, totalHostCount = 0, newHostCount = 0, matchesTruncated = false, isLoading, isBulkImporting, resolvingAlias = null, loadError, onSelect, onQueryChange, onRetry, onBack, onAddAllToOrca }) {
	const [query, setQuery] = (0, import_react.useState)("");
	const queryTimer = (0, import_react.useRef)(null);
	(0, import_react.useEffect)(() => () => {
		if (queryTimer.current) clearTimeout(queryTimer.current);
	}, []);
	const isResolving = resolvingAlias != null;
	const filterDisabled = isBulkImporting || isResolving;
	const picksDisabled = isBulkImporting || isResolving;
	const canAddAll = !isLoading && !isBulkImporting && !isResolving && loadError == null && newHostCount > 0;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex min-h-0 flex-1 flex-col gap-3",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, {
				className: "text-left",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, { children: translate("auto.components.sidebar.AddRemoteHostDialog.sshConfigPickerTitle", "Choose from ~/.ssh/config") }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, { children: translate("auto.components.sidebar.AddRemoteHostDialog.sshConfigPickerDescription", "Pick a host to fill the form, or add every new host to Orca’s host list.") })]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
				value: query,
				onChange: (event) => {
					const value = event.target.value;
					setQuery(value);
					if (queryTimer.current) clearTimeout(queryTimer.current);
					queryTimer.current = setTimeout(() => onQueryChange(value), 200);
				},
				placeholder: translate("auto.components.sidebar.AddRemoteHostDialog.sshConfigPickerFilter", "Filter hosts…"),
				autoFocus: true,
				disabled: filterDisabled,
				"aria-label": translate("auto.components.sidebar.AddRemoteHostDialog.sshConfigPickerFilter", "Filter hosts…")
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "scrollbar-sleek min-h-0 flex-1 overflow-y-auto rounded-md border border-border bg-card",
				children: loadError != null ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "px-3 py-8 text-center text-sm text-muted-foreground",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", { children: loadError }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						type: "button",
						variant: "outline",
						size: "sm",
						className: "mt-3",
						onClick: onRetry,
						disabled: isLoading || isBulkImporting,
						children: translate("auto.components.sidebar.AddRemoteHostDialog.sshConfigPickerRetry", "Try again")
					})]
				}) : isLoading && hosts.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "px-3 py-8 text-center text-sm text-muted-foreground",
					children: translate("auto.components.sidebar.AddRemoteHostDialog.sshConfigPickerLoading", "Reading ~/.ssh/config…")
				}) : hosts.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "px-3 py-8 text-center text-sm text-muted-foreground",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "font-medium text-foreground",
						children: totalHostCount === 0 ? translate("auto.components.sidebar.AddRemoteHostDialog.sshConfigPickerEmpty", "No hosts in ~/.ssh/config") : translate("auto.components.sidebar.AddRemoteHostDialog.sshConfigPickerNoMatch", "No matching hosts")
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "mt-1",
						children: totalHostCount === 0 ? translate("auto.components.sidebar.AddRemoteHostDialog.sshConfigPickerEmptyHint", "Add a Host entry there, or go back and type the details manually.") : translate("auto.components.sidebar.AddRemoteHostDialog.sshConfigPickerNoMatchHint", "Try another filter, or go back and type manually.")
					})]
				}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("ul", {
					"aria-label": translate("auto.components.sidebar.AddRemoteHostDialog.sshConfigPickerHostsLabel", "SSH config hosts"),
					"aria-busy": isLoading || isResolving,
					className: cn("divide-y divide-border/70", isLoading && "opacity-60"),
					children: hosts.map((host) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("li", { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
						type: "button",
						disabled: picksDisabled || host.alreadyInOrca,
						className: cn("flex w-full items-start justify-between gap-3 px-3 py-2.5 text-left", "hover:bg-accent focus-visible:bg-accent focus-visible:outline-none", "disabled:cursor-not-allowed disabled:opacity-50"),
						onClick: () => onSelect(host),
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "min-w-0 flex-1",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "truncate text-sm font-medium",
									children: host.alias
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "truncate text-xs text-muted-foreground",
									children: host.username ? `${host.username}@${host.hostname}:${host.port}` : `${host.hostname}:${host.port}`
								}),
								host.identityFile != null && host.identityFile !== "" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "mt-0.5 truncate font-mono text-[11px] text-muted-foreground/80",
									children: host.identityFile
								}) : null
							]
						}), resolvingAlias === host.alias ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "mt-0.5 shrink-0 text-[10.5px] text-muted-foreground",
							children: translate("auto.components.sidebar.AddRemoteHostDialog.sshConfigPickerResolving", "Reading…")
						}) : host.alreadyInOrca ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
							variant: "outline",
							className: "mt-0.5 shrink-0 border-emerald-500/40 text-[10.5px] text-emerald-400",
							children: translate("auto.components.sidebar.AddRemoteHostDialog.sshConfigPickerInOrca", "In Orca")
						}) : host.previouslyRemoved ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
							variant: "outline",
							className: "mt-0.5 shrink-0 text-[10.5px] text-muted-foreground",
							children: translate("auto.components.sidebar.AddRemoteHostDialog.sshConfigPickerPreviouslyRemoved", "Removed from Orca")
						}) : null]
					}) }, host.alias))
				})
			}),
			matchesTruncated ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				className: "text-xs text-muted-foreground",
				children: translate("auto.components.sidebar.AddRemoteHostDialog.sshConfigPickerMoreResults", "Showing the first {{value0}} matches. Narrow your filter to find more.", { value0: 100 })
			}) : null,
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex flex-col gap-2 border-t border-border pt-3 sm:flex-row sm:items-center sm:justify-between",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					type: "button",
					variant: "secondary",
					disabled: !canAddAll,
					onClick: onAddAllToOrca,
					className: "w-full sm:w-auto",
					children: isBulkImporting ? translate("auto.components.sidebar.AddRemoteHostDialog.sshConfigPickerAddingAll", "Adding hosts…") : newHostCount > 0 ? translate("auto.components.sidebar.AddRemoteHostDialog.sshConfigPickerAddAll", "Add all {{value0}} to Orca", { value0: newHostCount }) : totalHostCount > 0 ? translate("auto.components.sidebar.AddRemoteHostDialog.sshConfigPickerNoNewHosts", "No new hosts to add") : translate("auto.components.sidebar.AddRemoteHostDialog.sshConfigPickerAddAllEmpty", "Add all to Orca")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					type: "button",
					variant: "outline",
					onClick: onBack,
					disabled: isBulkImporting,
					className: "w-full sm:w-auto",
					children: translate("auto.components.sidebar.AddRemoteHostDialog.sshConfigPickerBack", "Back")
				})]
			})
		]
	});
}
function SshHostFields({ form, disabled, preferAdvancedOpen = false, configIdentityAlias = null, onFormChange, onSubmit }) {
	const [advancedOpen, setAdvancedOpen] = (0, import_react.useState)(preferAdvancedOpen);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("form", {
		className: "grid gap-3 sm:grid-cols-2",
		onSubmit: (event) => {
			event.preventDefault();
			onSubmit();
		},
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-1.5",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
					htmlFor: "add-ssh-label",
					children: translate("auto.components.sidebar.AddRemoteHostDialog.label", "Label")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
					id: "add-ssh-label",
					value: form.label,
					disabled,
					onChange: (event) => onFormChange((draft) => ({
						...draft,
						label: event.target.value
					})),
					placeholder: translate("auto.components.sidebar.AddRemoteHostDialog.sshLabelPlaceholder", "Dev box")
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-1.5",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
					htmlFor: "add-ssh-host",
					children: translate("auto.components.sidebar.AddRemoteHostDialog.sshHost", "Host or alias")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
					id: "add-ssh-host",
					value: form.host,
					disabled,
					autoFocus: true,
					onBlur: () => onFormChange(applyParsedSshHostInput),
					onChange: (event) => onFormChange((draft) => ({
						...draft,
						host: event.target.value
					})),
					placeholder: translate("auto.components.sidebar.AddRemoteHostDialog.sshHostPlaceholder", "deploy@server:22")
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-1.5",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
					htmlFor: "add-ssh-username",
					children: translate("auto.components.sidebar.AddRemoteHostDialog.username", "Username")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
					id: "add-ssh-username",
					value: form.username,
					disabled,
					onChange: (event) => onFormChange((draft) => ({
						...draft,
						username: event.target.value
					})),
					placeholder: translate("auto.components.sidebar.AddRemoteHostDialog.usernamePlaceholder", "deploy")
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-1.5",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
					htmlFor: "add-ssh-port",
					children: translate("auto.components.sidebar.AddRemoteHostDialog.port", "Port")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
					id: "add-ssh-port",
					value: form.port,
					disabled,
					type: "number",
					min: 1,
					max: 65535,
					onChange: (event) => onFormChange((draft) => ({
						...draft,
						port: event.target.value
					})),
					placeholder: "22"
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-1.5 sm:col-span-2",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
						htmlFor: "add-ssh-identity-file",
						children: translate("auto.components.sidebar.AddRemoteHostDialog.identityFile", "Identity file")
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
						id: "add-ssh-identity-file",
						value: form.identityFile,
						disabled,
						onChange: (event) => onFormChange((draft) => ({
							...draft,
							identityFile: event.target.value
						})),
						placeholder: translate("auto.components.sidebar.AddRemoteHostDialog.identityFilePlaceholder", "~/.ssh/id_ed25519 (optional)")
					}),
					configIdentityAlias && form.identityFile.trim() === "" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "text-xs text-muted-foreground",
						children: translate("auto.components.sidebar.AddRemoteHostDialog.identityFileFromConfigHint", "Left empty on purpose: Orca uses every key ~/.ssh/config resolves for {{value0}}. Type a path to use just that key.", { value0: configIdentityAlias })
					}) : null
				]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SshHostAdvancedFields, {
				open: advancedOpen,
				onOpenChange: setAdvancedOpen,
				form,
				disabled,
				onFormChange
			})
		]
	});
}
function RemoteServerFields({ name, pairingCode, parsedLink, disabled, onNameChange, onPairingCodeChange, allowLoopback, onAllowLoopbackChange, onSubmit }) {
	const inputError = pairingCode.trim() !== "" && !parsedLink.ok;
	const loopbackBlocked = parsedLink.ok && parsedLink.value.endpointKind === "loopback" && !allowLoopback;
	const pairingCodeDescriptionId = inputError ? "add-server-pairing-code-error" : loopbackBlocked ? "add-server-loopback-blocked" : "add-server-pairing-code-help";
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("form", {
		className: "space-y-3",
		onSubmit: (event) => {
			event.preventDefault();
			onSubmit();
		},
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-1.5",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
					htmlFor: "add-server-name",
					children: translate("auto.components.sidebar.AddRemoteHostDialog.serverName", "Name in Orca")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
					id: "add-server-name",
					value: name,
					disabled,
					autoFocus: true,
					onChange: (event) => onNameChange(event.target.value),
					placeholder: translate("auto.components.sidebar.AddRemoteHostDialog.serverNamePlaceholder", "Dev box")
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-1.5",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
						htmlFor: "add-server-pairing-code",
						children: translate("auto.components.sidebar.AddRemoteHostDialog.pairingCode", "Access link")
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
						id: "add-server-pairing-code",
						"aria-invalid": inputError || loopbackBlocked,
						"aria-describedby": pairingCodeDescriptionId,
						value: pairingCode,
						disabled,
						onChange: (event) => onPairingCodeChange(event.target.value),
						placeholder: translate("auto.components.sidebar.AddRemoteHostDialog.pairingCodePlaceholder", "orca://pair?code=..."),
						className: "font-mono"
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						id: "add-server-pairing-code-help",
						className: "text-xs text-muted-foreground",
						children: translate("auto.components.sidebar.AddRemoteHostDialog.pairingHelpSuffix", "Create this under Settings → Remote Orca Servers → Share this host on the other computer.")
					}),
					inputError ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						id: "add-server-pairing-code-error",
						role: "alert",
						className: "text-xs text-destructive",
						children: parsedLink.ok ? null : translateHostAccessLinkError(parsedLink.kind)
					}) : null
				]
			}),
			parsedLink.ok ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-1 rounded-md border border-border/60 p-3",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-center gap-2 text-xs font-medium",
						children: [translate("auto.components.sidebar.AddRemoteHostDialog.linkDestination", "Link destination"), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
							variant: "outline",
							children: translateRemotePairingEndpointKind(parsedLink.value.endpointKind)
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "font-mono text-sm",
						children: parsedLink.value.displayEndpoint
					}),
					parsedLink.value.endpointKind === "loopback" ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("label", {
						className: "mt-2 flex items-start gap-2 text-xs",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Checkbox, {
							checked: allowLoopback,
							disabled,
							onCheckedChange: (checked) => onAllowLoopbackChange(checked === true)
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "block font-medium",
							children: translate("auto.components.sidebar.AddRemoteHostDialog.sshTunnel", "I am using an SSH tunnel")
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-muted-foreground",
							children: translate("auto.components.sidebar.AddRemoteHostDialog.sshTunnelHelp", "Otherwise, this link points back to this device and cannot identify the other computer.")
						})] })]
					}) : null
				]
			}) : null,
			loopbackBlocked ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				id: "add-server-loopback-blocked",
				role: "alert",
				className: "text-xs text-destructive",
				children: translate("auto.components.sidebar.AddRemoteHostDialog.loopbackBlocked", "Enable the SSH tunnel override or create a new link using the other host’s Tailscale or LAN address.")
			}) : null
		]
	});
}
function AddRemoteHostSshFormPanel({ form, disabled, preferAdvancedOpen, configIdentityAlias, onFormChange, onSubmit, onCancel, onFillFromConfig }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
		/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, { children: translate("auto.components.sidebar.AddRemoteHostDialog.sshTitle", "Add SSH host") }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, { children: translate("auto.components.sidebar.AddRemoteHostDialog.sshDescription", "Add a persistent machine you can log into over SSH.") })] }),
		/* @__PURE__ */ (0, import_jsx_runtime.jsx)(SshHostFields, {
			form,
			disabled,
			preferAdvancedOpen,
			configIdentityAlias,
			onFormChange,
			onSubmit
		}),
		/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogFooter, {
			className: "sm:justify-between",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				type: "button",
				variant: "link",
				className: "h-auto self-center justify-start p-0 text-xs text-muted-foreground hover:text-foreground",
				onClick: onFillFromConfig,
				disabled,
				children: translate("auto.components.sidebar.AddRemoteHostDialog.fillFromSshConfig", "Fill from ~/.ssh/config…")
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex flex-col-reverse gap-2 sm:flex-row sm:justify-end",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					type: "button",
					variant: "outline",
					onClick: onCancel,
					disabled,
					children: translate("auto.components.sidebar.AddRemoteHostDialog.cancel", "Cancel")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					type: "button",
					onClick: onSubmit,
					disabled,
					children: disabled ? translate("auto.components.sidebar.AddRemoteHostDialog.saving", "Saving...") : translate("auto.components.sidebar.AddRemoteHostDialog.save", "Save")
				})]
			})]
		})
	] });
}
function AddRemoteHostServerFormPanel({ name, pairingCode, parsedLink, allowLoopback, disabled, canSubmit, onNameChange, onPairingCodeChange, onAllowLoopbackChange, onSubmit, onCancel }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
		/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, { children: translate("auto.components.sidebar.AddRemoteHostDialog.serverTitle", "Add remote server") }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, { children: translate("auto.components.sidebar.AddRemoteHostDialog.serverDescription", "Pair with Orca running on another computer.") })] }),
		/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RemoteServerFields, {
			name,
			pairingCode,
			parsedLink,
			disabled,
			onNameChange,
			onPairingCodeChange,
			allowLoopback,
			onAllowLoopbackChange,
			onSubmit
		}),
		/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogFooter, {
			className: "sm:justify-between",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex flex-col-reverse gap-2 sm:flex-row sm:justify-end",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					type: "button",
					variant: "outline",
					onClick: onCancel,
					disabled,
					children: translate("auto.components.sidebar.AddRemoteHostDialog.cancel", "Cancel")
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					type: "button",
					onClick: onSubmit,
					disabled: disabled || !canSubmit,
					children: disabled ? translate("auto.components.sidebar.AddRemoteHostDialog.saving", "Saving...") : translate("auto.components.sidebar.AddRemoteHostDialog.save", "Save")
				})]
			})]
		})
	] });
}
function normalizeSshConfigAlias(alias) {
	return alias ? alias.trim().toLowerCase() : "";
}
function isDuplicateSshTargetAlias({ existingTargets, configHost, label, host }) {
	const alias = normalizeSshConfigAlias(configHost) || normalizeSshConfigAlias(label) || normalizeSshConfigAlias(host);
	if (!alias) return false;
	return existingTargets.some((target) => getOccupiedAliases(target).includes(alias));
}
function getOccupiedAliases(target) {
	const occupied = [target.configHost, target.label].map(normalizeSshConfigAlias).filter(Boolean);
	return occupied.length > 0 ? occupied : [normalizeSshConfigAlias(target.host)].filter(Boolean);
}
async function saveNewSshHostFromForm({ form, ssh, recordSshRepoReadoptions, setSshTargetsMetadata, recordFeatureInteraction }) {
	const { host, configHost, username, port } = getSshTargetDraftConnectionFields(form);
	if (!host) {
		toast.error(translate("auto.components.sidebar.AddRemoteHostDialog.sshHostRequired", "Host or SSH config alias is required."));
		return "validation-failed";
	}
	if (Number.isNaN(port) || port < 1 || port > 65535) {
		toast.error(translate("auto.components.sidebar.AddRemoteHostDialog.sshPortInvalid", "Port must be between 1 and 65535."));
		return "validation-failed";
	}
	const graceSeconds = parseRelayGracePeriodSeconds(form);
	if (!isRelayGracePeriodValid(form, graceSeconds)) {
		toast.error(translate("auto.components.sidebar.AddRemoteHostDialog.sshRelayGraceInvalid", "Terminal timeout must be between 60 and {{value0}} seconds.", { value0: MAX_SSH_RELAY_GRACE_PERIOD_SECONDS }));
		return "validation-failed";
	}
	const identityFile = form.identityFile.trim() || void 0;
	const proxyCommand = form.proxyCommand.trim() || void 0;
	const jumpHost = form.jumpHost.trim() || void 0;
	const systemSshConnectionReuse = form.systemSshConnectionReuse ? void 0 : false;
	const target = {
		label: form.label.trim() || (username ? `${username}@${host}` : configHost || host),
		configHost,
		host,
		port,
		username,
		...form.gssapiAuthentication ? { gssapiAuthentication: true } : {},
		relayGracePeriodSeconds: graceSeconds,
		...identityFile ? { identityFile } : {},
		...proxyCommand ? { proxyCommand } : {},
		...jumpHost ? { jumpHost } : {},
		...systemSshConnectionReuse === false ? { systemSshConnectionReuse } : {}
	};
	try {
		if (isDuplicateSshTargetAlias({
			existingTargets: await ssh.listTargets(),
			configHost: target.configHost,
			label: target.label,
			host: target.host
		})) {
			toast.error(translate("auto.components.sidebar.AddRemoteHostDialog.sshAlreadyExists", "That SSH host is already in Orca."));
			return "validation-failed";
		}
		recordSshRepoReadoptions((await ssh.addTarget({ target })).repoReadoptions);
		setSshTargetsMetadata(await ssh.listTargets());
		recordFeatureInteraction("ssh");
		toast.success(translate("auto.components.sidebar.AddRemoteHostDialog.sshSaved", "SSH host added."));
		return "saved";
	} catch (error) {
		toast.error(error instanceof Error ? error.message : translate("auto.components.sidebar.AddRemoteHostDialog.sshSaveFailed", "Failed to add SSH host."));
		return "failed";
	}
}
async function prefillFormFromSshConfigHost(host, ssh) {
	if (typeof ssh.resolveConfigHost !== "function") throw new Error(translate("auto.components.sidebar.AddRemoteHostDialog.sshConfigPickerRestartRequired", "Restart Orca to finish applying the SSH config picker update."));
	const resolved = await ssh.resolveConfigHost({ alias: host.alias });
	if (!resolved) return null;
	const form = getEditingTargetFromSshConfigHost(resolved);
	return {
		form,
		preferAdvancedOpen: hasAdvancedConnectionValues(form)
	};
}
async function addAllSshConfigHostsToOrca({ ssh, recordSshRepoReadoptions, setSshTargetsMetadata, recordFeatureInteraction }) {
	try {
		const result = await ssh.importConfig();
		recordSshRepoReadoptions(result.repoReadoptions);
		setSshTargetsMetadata(await ssh.listTargets());
		recordFeatureInteraction("ssh");
		if (result.targets.length === 0) {
			toast(translate("auto.components.sidebar.AddRemoteHostDialog.sshImportAlreadySynced", "~/.ssh/config already in sync."));
			return { kind: "already-synced" };
		}
		toast.success(translate("auto.components.sidebar.AddRemoteHostDialog.sshImportSynced", "Added {{value0}} host{{value1}} to Orca.", {
			value0: result.targets.length,
			value1: result.targets.length > 1 ? "s" : ""
		}));
		return {
			kind: "added",
			count: result.targets.length
		};
	} catch (error) {
		toast.error(error instanceof Error ? error.message : translate("auto.components.sidebar.AddRemoteHostDialog.sshImportFailed", "Failed to import SSH config."));
		return { kind: "failed" };
	}
}
async function loadSshConfigHostsForPicker(ssh, args) {
	try {
		const result = normalizeSshConfigHostListResult(await ssh.listConfigHosts(args));
		if (!result) throw new Error("Invalid SSH config host response");
		return {
			ok: true,
			result
		};
	} catch (error) {
		return {
			ok: false,
			error: error instanceof Error ? error.message : translate("auto.components.sidebar.AddRemoteHostDialog.sshConfigPickerLoadFailed", "Failed to read ~/.ssh/config.")
		};
	}
}
function normalizeSshConfigHostListResult(value) {
	if (Array.isArray(value)) {
		const hosts = value.slice(0, 100);
		return {
			hosts,
			totalHostCount: value.length,
			newHostCount: value.filter((host) => typeof host === "object" && host !== null && host.alreadyInOrca === false).length,
			matchCount: value.length,
			hasMore: value.length > hosts.length
		};
	}
	if (!value || typeof value !== "object") return null;
	const result = value;
	return Array.isArray(result.hosts) && typeof result.totalHostCount === "number" && typeof result.newHostCount === "number" && typeof result.matchCount === "number" && typeof result.hasMore === "boolean" ? result : null;
}
function AddRemoteHostDialog({ mode, onOpenChange }) {
	const open = mode !== null;
	const [renderMode, setRenderMode] = (0, import_react.useState)(mode ?? "ssh");
	if (mode !== null && mode !== renderMode) setRenderMode(mode);
	const [sshForm, setSshForm] = (0, import_react.useState)(EMPTY_FORM);
	const [sshView, setSshView] = (0, import_react.useState)("form");
	const [configHosts, setConfigHosts] = (0, import_react.useState)([]);
	const [configHostCount, setConfigHostCount] = (0, import_react.useState)(0);
	const [newConfigHostCount, setNewConfigHostCount] = (0, import_react.useState)(0);
	const [configHostMatchesTruncated, setConfigHostMatchesTruncated] = (0, import_react.useState)(false);
	const [isLoadingConfigHosts, setIsLoadingConfigHosts] = (0, import_react.useState)(false);
	const [resolvingConfigAlias, setResolvingConfigAlias] = (0, import_react.useState)(null);
	const [isBulkImporting, setIsBulkImporting] = (0, import_react.useState)(false);
	const [configHostsError, setConfigHostsError] = (0, import_react.useState)(null);
	const [preferAdvancedOpen, setPreferAdvancedOpen] = (0, import_react.useState)(false);
	const [configFilledAlias, setConfigFilledAlias] = (0, import_react.useState)(null);
	const [serverName, setServerName] = (0, import_react.useState)("");
	const [pairingCode, setPairingCode] = (0, import_react.useState)("");
	const [allowLoopback, setAllowLoopback] = (0, import_react.useState)(false);
	const [isSaving, setIsSaving] = (0, import_react.useState)(false);
	const configSearchGeneration = (0, import_react.useRef)(0);
	const configSearchQuery = (0, import_react.useRef)("");
	const configResolveGeneration = (0, import_react.useRef)(0);
	const parsedServerLink = (0, import_react.useMemo)(() => parseHostAccessLink(pairingCode), [pairingCode]);
	const serverFormCanSubmit = serverName.trim() !== "" && parsedServerLink.ok && (parsedServerLink.value.endpointKind !== "loopback" || allowLoopback);
	const setSshTargetsMetadata = useAppStore((s) => s.setSshTargetsMetadata);
	const recordSshRepoReadoptions = useAppStore((s) => s.recordSshRepoReadoptions);
	const setRuntimeEnvironments = useAppStore((s) => s.setRuntimeEnvironments);
	const setRuntimeEnvironmentStatus = useAppStore((s) => s.setRuntimeEnvironmentStatus);
	const recordFeatureInteraction = useAppStore((s) => s.recordFeatureInteraction);
	const busy = isSaving || isBulkImporting || resolvingConfigAlias !== null;
	const invalidatePendingConfigResolve = () => {
		configResolveGeneration.current += 1;
		setResolvingConfigAlias(null);
	};
	const reset = () => {
		setSshForm(EMPTY_FORM);
		setSshView("form");
		setConfigHosts([]);
		setConfigHostCount(0);
		setNewConfigHostCount(0);
		setConfigHostMatchesTruncated(false);
		setConfigHostsError(null);
		configSearchQuery.current = "";
		invalidatePendingConfigResolve();
		setPreferAdvancedOpen(false);
		setConfigFilledAlias(null);
		setIsBulkImporting(false);
		setServerName("");
		setPairingCode("");
		setAllowLoopback(false);
	};
	const close = () => {
		if (isSaving || isBulkImporting) return;
		reset();
		onOpenChange(null);
	};
	const saveSshHost = async () => {
		setIsSaving(true);
		try {
			if (await saveNewSshHostFromForm({
				form: sshForm,
				ssh: window.api.ssh,
				recordSshRepoReadoptions,
				setSshTargetsMetadata,
				recordFeatureInteraction
			}) === "saved") {
				reset();
				onOpenChange(null);
			}
		} finally {
			setIsSaving(false);
		}
	};
	const loadSshConfigHosts = async (query = "", options) => {
		configSearchQuery.current = query;
		const generation = configSearchGeneration.current + 1;
		configSearchGeneration.current = generation;
		setIsLoadingConfigHosts(true);
		setConfigHostsError(null);
		const result = await loadSshConfigHostsForPicker(window.api.ssh, {
			query,
			...options?.refresh ? { refresh: true } : {}
		});
		if (generation !== configSearchGeneration.current) return;
		if (result.ok) {
			setConfigHosts(result.result.hosts);
			setConfigHostCount(result.result.totalHostCount);
			setNewConfigHostCount(result.result.newHostCount);
			setConfigHostMatchesTruncated(result.result.hasMore);
		} else {
			setConfigHosts([]);
			setConfigHostsError(result.error);
		}
		setIsLoadingConfigHosts(false);
	};
	const openSshConfigPicker = async () => {
		setSshView("config-picker");
		await loadSshConfigHosts("", { refresh: true });
	};
	const leaveSshConfigPicker = () => {
		invalidatePendingConfigResolve();
		setSshView("form");
	};
	const selectSshConfigHost = async (host) => {
		const generation = configResolveGeneration.current + 1;
		configResolveGeneration.current = generation;
		setResolvingConfigAlias(host.alias);
		const isStale = () => generation !== configResolveGeneration.current;
		let resolved;
		try {
			resolved = await prefillFormFromSshConfigHost(host, window.api.ssh);
		} catch (error) {
			if (isStale()) return;
			setResolvingConfigAlias(null);
			toast.error(error instanceof Error ? error.message : translate("auto.components.sidebar.AddRemoteHostDialog.sshConfigPickerResolveFailed", "Failed to resolve that SSH config host."));
			return;
		}
		if (isStale()) return;
		setResolvingConfigAlias(null);
		if (!resolved) {
			toast.error(translate("auto.components.sidebar.AddRemoteHostDialog.sshConfigPickerResolveFailed", "Failed to resolve that SSH config host."));
			return;
		}
		const { form, preferAdvancedOpen: openAdvanced } = resolved;
		setSshForm(form);
		setPreferAdvancedOpen(openAdvanced);
		setConfigFilledAlias(host.alias);
		setSshView("form");
		recordFeatureInteraction("ssh");
		toast.success(translate("auto.components.sidebar.AddRemoteHostDialog.sshConfigPickerFilled", "Filled from {{value0}}. Review and Save.", { value0: host.alias }));
	};
	const addAllConfigHostsToOrca = async () => {
		setIsBulkImporting(true);
		try {
			const result = await addAllSshConfigHostsToOrca({
				ssh: window.api.ssh,
				recordSshRepoReadoptions,
				setSshTargetsMetadata,
				recordFeatureInteraction
			});
			if (result.kind === "added") {
				reset();
				onOpenChange(null);
				return;
			}
			if (result.kind === "already-synced") await loadSshConfigHosts(configSearchQuery.current);
		} finally {
			setIsBulkImporting(false);
		}
	};
	const saveRemoteServer = async () => {
		const trimmedName = serverName.trim();
		const trimmedPairingCode = pairingCode.trim();
		if (!trimmedName || !trimmedPairingCode) {
			toast.error(translate("auto.components.sidebar.AddRemoteHostDialog.serverFieldsRequired", "Server name and pairing code are required."));
			return;
		}
		if (!parsedServerLink.ok) {
			toast.error(translateHostAccessLinkError(parsedServerLink.kind));
			return;
		}
		if (parsedServerLink.value.endpointKind === "loopback" && !allowLoopback) {
			toast.error(translate("auto.components.sidebar.AddRemoteHostDialog.loopbackBlocked", "Enable the SSH tunnel override or create a new link using the other host’s Tailscale or LAN address."));
			return;
		}
		setIsSaving(true);
		try {
			const result = await window.api.runtimeEnvironments.verifyAndAddFromPairingCode({
				name: trimmedName,
				pairingCode: trimmedPairingCode,
				allowLoopback
			});
			if (!result.ok) {
				toast.error(result.kind === "environment-save-failed" ? result.message : translateRemotePairingFailureDescription(result.kind, parsedServerLink.value.displayEndpoint));
				return;
			}
			setRuntimeEnvironments(await window.api.runtimeEnvironments.list());
			setRuntimeEnvironmentStatus(result.environment.id, {
				status: result.runtimeStatus,
				checkedAt: Date.now()
			});
			toast.success(translate("auto.components.sidebar.AddRemoteHostDialog.serverSaved", "Remote server added."));
			reset();
			onOpenChange(null);
		} catch (error) {
			toast.error(error instanceof Error ? error.message : translate("auto.components.sidebar.AddRemoteHostDialog.serverSaveFailed", "Failed to add remote server."));
		} finally {
			setIsSaving(false);
		}
	};
	const showSshConfigPicker = renderMode === "ssh" && sshView === "config-picker";
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
		open,
		onOpenChange: (nextOpen) => {
			if (!nextOpen) close();
		},
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogContent, {
			className: showSshConfigPicker ? "flex max-h-[min(90vh,560px)] flex-col gap-0 overflow-hidden sm:max-w-xl" : "scrollbar-sleek max-h-[min(90vh,560px)] overflow-y-auto sm:max-w-xl",
			children: showSshConfigPicker ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "flex min-h-0 flex-1 flex-col",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AddRemoteHostSshConfigPicker, {
					hosts: configHosts,
					totalHostCount: configHostCount,
					newHostCount: newConfigHostCount,
					matchesTruncated: configHostMatchesTruncated,
					isLoading: isLoadingConfigHosts,
					isBulkImporting,
					resolvingAlias: resolvingConfigAlias,
					loadError: configHostsError,
					onSelect: (host) => void selectSshConfigHost(host),
					onQueryChange: (query) => void loadSshConfigHosts(query),
					onRetry: () => void loadSshConfigHosts(configSearchQuery.current, { refresh: true }),
					onBack: leaveSshConfigPicker,
					onAddAllToOrca: () => void addAllConfigHostsToOrca()
				})
			}) : renderMode === "ssh" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AddRemoteHostSshFormPanel, {
				form: sshForm,
				disabled: busy,
				preferAdvancedOpen,
				configIdentityAlias: configFilledAlias,
				onFormChange: setSshForm,
				onSubmit: () => void saveSshHost(),
				onCancel: close,
				onFillFromConfig: () => void openSshConfigPicker()
			}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AddRemoteHostServerFormPanel, {
				name: serverName,
				pairingCode,
				parsedLink: parsedServerLink,
				allowLoopback,
				disabled: busy,
				canSubmit: serverFormCanSubmit,
				onNameChange: setServerName,
				onPairingCodeChange: (value) => {
					setPairingCode(value);
					setAllowLoopback(false);
				},
				onAllowLoopbackChange: setAllowLoopback,
				onSubmit: () => void saveRemoteServer(),
				onCancel: close
			})
		})
	});
}
export { Lightbulb as n, AddRemoteHostDialog as t };
