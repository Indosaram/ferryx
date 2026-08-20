import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { n as cn, t as Button } from "./button-DszXJEV6.js";
import { t as Check } from "./check-Lb2n4tDb.js";
import { t as ChevronDown } from "./chevron-down-BRkP96Md.js";
import { t as CircleAlert } from "./circle-alert-keRTpMg-.js";
import { t as LoaderCircle } from "./loader-circle-CRZpWdsi.js";
import { t as Plus } from "./plus-Db0kWPVa.js";
import { t as ShieldCheck } from "./shield-check-CksLJk1J.js";
import { Cr as addMobilePairingCustomAddress, Dr as parseManualNetworkAddress, Er as removeMobilePairingCustomAddress, Tr as normalizeMobilePairingCustomAddresses, t as useAppStore, wr as normalizeMobilePairingCustomAddress } from "./store-CgXrfmaH.js";
import { t as X } from "./x-BrGKE4uz.js";
import { n as toast } from "./dist-DgqligFk.js";
import { t as Label } from "./label-D-n9s_wS.js";
import { i as PopoverTrigger, r as PopoverContent, t as Popover } from "./popover-CgR1mzy7.js";
import { i as TooltipTrigger, n as TooltipContent, t as Tooltip } from "./tooltip-DPmd1AoJ.js";
import { t as useMountedRef } from "./useMountedRef-1omUd-IV.js";
import { t as Badge } from "./badge-BBptl5GG.js";
import { i as CommandGroup, o as CommandItem, s as CommandList, t as Command } from "./command-D8Tw17HJ.js";
import { a as DialogFooter, i as DialogDescription, o as DialogHeader, r as DialogContent, s as DialogTitle, t as Dialog } from "./dialog-BbelfMSB.js";
import { t as Input } from "./input-DV5rpysh.js";
import { t as isTailnetIPv4Address } from "./tailnet-address-B7o4AlJU.js";
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function IosBrandIcon({ className } = {}) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("svg", {
		className: className ?? "mp-platform-brand-icon",
		viewBox: "0 0 24 24",
		"aria-hidden": "true",
		focusable: "false",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M12.152 6.896c-.948 0-2.415-1.078-3.96-1.04-2.04.027-3.91 1.183-4.961 3.014-2.117 3.675-.546 9.103 1.519 12.09 1.013 1.454 2.208 3.09 3.792 3.039 1.52-.065 2.09-.987 3.935-.987 1.831 0 2.35.987 3.96.948 1.637-.026 2.676-1.48 3.676-2.948 1.156-1.688 1.636-3.325 1.662-3.415-.039-.013-3.182-1.221-3.22-4.857-.026-3.04 2.48-4.494 2.597-4.559-1.429-2.09-3.623-2.324-4.39-2.376-2-.156-3.675 1.09-4.61 1.09zM15.53 3.83c.843-1.012 1.4-2.427 1.245-3.83-1.207.052-2.662.805-3.532 1.818-.78.896-1.454 2.338-1.273 3.714 1.338.104 2.715-.688 3.559-1.701" })
	});
}
function AndroidLogo({ className } = {}) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("svg", {
		className: className ?? "mp-platform-brand-icon",
		viewBox: "0 0 24 24",
		"aria-hidden": "true",
		focusable: "false",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M18.4395 5.5586c-.675 1.1664-1.352 2.3318-2.0274 3.498-.0366-.0155-.0742-.0286-.1113-.043-1.8249-.6957-3.484-.8-4.42-.787-1.8551.0185-3.3544.4643-4.2597.8203-.084-.1494-1.7526-3.021-2.0215-3.4864a1.1451 1.1451 0 0 0-.1406-.1914c-.3312-.364-.9054-.4859-1.379-.203-.475.282-.7136.9361-.3886 1.5019 1.9466 3.3696-.0966-.2158 1.9473 3.3593.0172.031-.4946.2642-1.3926 1.0177C2.8987 12.176.452 14.772 0 18.9902h24c-.119-1.1108-.3686-2.099-.7461-3.0683-.7438-1.9118-1.8435-3.2928-2.7402-4.1836a12.1048 12.1048 0 0 0-2.1309-1.6875c.6594-1.122 1.312-2.2559 1.9649-3.3848.2077-.3615.1886-.7956-.0079-1.1191a1.1001 1.1001 0 0 0-.8515-.5332c-.5225-.0536-.9392.3128-1.0488.5449zm-.0391 8.461c.3944.5926.324 1.3306-.1563 1.6503-.4799.3197-1.188.0985-1.582-.4941-.3944-.5927-.324-1.3307.1563-1.6504.4727-.315 1.1812-.1086 1.582.4941zM7.207 13.5273c.4803.3197.5506 1.0577.1563 1.6504-.394.5926-1.1038.8138-1.584.4941-.48-.3197-.5503-1.0577-.1563-1.6504.4008-.6021 1.1087-.8106 1.584-.4941z" })
	});
}
var import_react = /* @__PURE__ */ __toESM(require_react());
function CustomAddressDialog({ open, onOpenChange, initialValue, validate, copy, inputId, onConfirm }) {
	const [value, setValue] = (0, import_react.useState)(initialValue ?? "");
	const [submitting, setSubmitting] = (0, import_react.useState)(false);
	const [confirmationFailed, setConfirmationFailed] = (0, import_react.useState)(false);
	(0, import_react.useEffect)(() => {
		if (open) setValue(initialValue ?? "");
	}, [open, initialValue]);
	const close = () => {
		setSubmitting(false);
		setConfirmationFailed(false);
		onOpenChange(false);
	};
	const handleOpenChange = (nextOpen) => {
		if (nextOpen) onOpenChange(true);
		else if (!submitting) close();
	};
	const parsed = validate(value);
	const showInvalid = value.trim() !== "" && !parsed.ok;
	const submit = async () => {
		if (!parsed.ok || submitting) return;
		setSubmitting(true);
		setConfirmationFailed(false);
		try {
			if (await onConfirm(parsed.value) !== false) close();
			else setConfirmationFailed(true);
		} catch {
			setConfirmationFailed(true);
		} finally {
			setSubmitting(false);
		}
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Dialog, {
		open,
		onOpenChange: handleOpenChange,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogContent, {
			className: "sm:max-w-md",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogTitle, { children: copy.title }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DialogDescription, { children: copy.description })] }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "space-y-2",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Label, {
							htmlFor: inputId,
							children: copy.inputLabel
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
							id: inputId,
							autoFocus: true,
							value,
							disabled: submitting,
							"aria-invalid": showInvalid,
							placeholder: copy.placeholder,
							onChange: (e) => {
								setValue(e.target.value);
								setConfirmationFailed(false);
							},
							onKeyDown: (e) => {
								if (e.key === "Enter") {
									e.preventDefault();
									submit();
								}
							}
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "text-xs text-muted-foreground",
							children: copy.hint
						}),
						confirmationFailed ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "text-xs text-destructive",
							role: "alert",
							children: copy.confirmationError ?? copy.hint
						}) : null
					]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(DialogFooter, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					type: "button",
					variant: "outline",
					disabled: submitting,
					onClick: close,
					children: copy.cancel
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
					type: "button",
					disabled: !parsed.ok || submitting,
					onClick: () => void submit(),
					children: [submitting ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, {
						className: "size-4 animate-spin",
						"aria-hidden": true
					}) : null, copy.confirm]
				})] })
			]
		})
	});
}
var EMPTY_ADDRESS_OPTIONS = [];
function AddressPickerItem({ option, selected, commandValue, onSelect, onRemove, removeLabel }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "group relative",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CommandItem, {
			value: commandValue,
			onSelect,
			"data-current": selected ? "true" : void 0,
			className: cn("peer min-w-0", onRemove && "pr-8", selected && "bg-accent"),
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, {
				className: cn("size-3.5 shrink-0", !selected && "invisible"),
				"aria-hidden": true
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "min-w-0 flex-1 truncate",
				children: option.label
			})]
		}), onRemove ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Tooltip, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipTrigger, {
			asChild: true,
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				type: "button",
				variant: "ghost",
				size: "icon-xs",
				"aria-label": removeLabel,
				onKeyDown: (event) => {
					if (event.key === "Enter" || event.key === " ") event.stopPropagation();
				},
				onClick: (event) => {
					event.stopPropagation();
					onRemove();
				},
				className: "absolute top-1/2 right-1 -translate-y-1/2 text-muted-foreground opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 peer-data-[selected=true]:opacity-100 hover:text-destructive focus-visible:opacity-100",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { "aria-hidden": true })
			})
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TooltipContent, {
			side: "top",
			sideOffset: 4,
			children: removeLabel
		})] }) : null]
	});
}
function AddressPicker({ options, customOptions = EMPTY_ADDRESS_OPTIONS, value, valueIsCustom, onValueChange, onCustomValueChange, onCustomRemove, beforeCustomConfirm, formatCustomLabel, addCustomLabel, customSectionLabel, removeCustomLabel, customDialogCopy, validateCustom, customInputId, placeholder, triggerAriaLabel, disabled = false, className, id }) {
	const [pickerOpen, setPickerOpen] = (0, import_react.useState)(false);
	const [dialogOpen, setDialogOpen] = (0, import_react.useState)(false);
	const [commandValue, setCommandValue] = (0, import_react.useState)("");
	const [listId, setListId] = (0, import_react.useState)();
	const listRef = (0, import_react.useRef)(null);
	const restoreFocusAfterRemovalRef = (0, import_react.useRef)(false);
	const typeaheadRef = (0, import_react.useRef)({
		query: "",
		updatedAt: 0
	});
	const handleListRef = (0, import_react.useCallback)((node) => {
		listRef.current = node;
		setListId(node?.id);
	}, []);
	const isCustomSelection = value !== void 0 && value !== "" && (valueIsCustom ?? !options.some((option) => option.value === value));
	const displayedCustomOptions = (0, import_react.useMemo)(() => {
		if (!isCustomSelection || value === void 0 || customOptions.some((option) => option.value === value)) return customOptions;
		return [...customOptions, {
			value,
			label: formatCustomLabel(value)
		}];
	}, [
		customOptions,
		formatCustomLabel,
		isCustomSelection,
		value
	]);
	const selectedOption = (isCustomSelection ? displayedCustomOptions.find((option) => option.value === value) : options.find((option) => option.value === value)) ?? displayedCustomOptions.find((option) => option.value === value);
	const selectedCommandValue = value ? `${isCustomSelection ? "custom" : "detected"}:${value}` : "";
	const customValueChange = onCustomValueChange ?? onValueChange;
	const firstCommandValue = selectedCommandValue || (options[0] ? `detected:${options[0].value}` : displayedCustomOptions[0] ? `custom:${displayedCustomOptions[0].value}` : "add-custom-address");
	(0, import_react.useEffect)(() => {
		if (!pickerOpen) return;
		if (!(commandValue === "add-custom-address" || options.some((option) => commandValue === `detected:${option.value}`) || displayedCustomOptions.some((option) => commandValue === `custom:${option.value}`))) setCommandValue(firstCommandValue);
	}, [
		commandValue,
		displayedCustomOptions,
		firstCommandValue,
		options,
		pickerOpen
	]);
	(0, import_react.useEffect)(() => {
		if (!pickerOpen || !restoreFocusAfterRemovalRef.current) return;
		restoreFocusAfterRemovalRef.current = false;
		listRef.current?.focus();
	}, [displayedCustomOptions, pickerOpen]);
	(0, import_react.useEffect)(() => {
		if (!pickerOpen) return;
		const frame = window.requestAnimationFrame(() => {
			const list = listRef.current;
			const activeOption = list?.querySelector("[cmdk-item][aria-selected=\"true\"]");
			if (list && activeOption?.id) list.setAttribute("aria-activedescendant", activeOption.id);
		});
		return () => window.cancelAnimationFrame(frame);
	}, [
		commandValue,
		displayedCustomOptions,
		options,
		pickerOpen
	]);
	const handlePickerOpenChange = (nextOpen) => {
		typeaheadRef.current = {
			query: "",
			updatedAt: 0
		};
		if (nextOpen) setCommandValue(firstCommandValue);
		setPickerOpen(nextOpen);
	};
	const handleCommandKeyDown = (event) => {
		if (event.key === " ") {
			event.preventDefault();
			listRef.current?.querySelector("[cmdk-item][aria-selected=\"true\"]")?.click();
			return;
		}
		if (event.key.length !== 1 || event.altKey || event.ctrlKey || event.metaKey || event.nativeEvent.isComposing) return;
		event.preventDefault();
		const now = Date.now();
		const previous = typeaheadRef.current;
		const query = now - previous.updatedAt > 700 ? event.key : previous.query + event.key;
		typeaheadRef.current = {
			query,
			updatedAt: now
		};
		const prefix = ([...query].every((character) => character === query[0]) ? event.key : query).toLocaleLowerCase();
		const items = [
			...options.map((option) => ({
				command: `detected:${option.value}`,
				label: option.label
			})),
			...displayedCustomOptions.map((option) => ({
				command: `custom:${option.value}`,
				label: option.label
			})),
			{
				command: "add-custom-address",
				label: addCustomLabel
			}
		];
		const currentIndex = items.findIndex((item) => item.command === commandValue);
		const nextItem = [...items.slice(currentIndex + 1), ...items.slice(0, currentIndex + 1)].find((item) => item.label.toLocaleLowerCase().startsWith(prefix));
		if (nextItem) setCommandValue(nextItem.command);
	};
	const selectValue = (next, custom) => {
		setPickerOpen(false);
		if (custom) customValueChange(next);
		else onValueChange(next);
	};
	const handleCustomConfirm = async (next) => {
		if (beforeCustomConfirm && !await beforeCustomConfirm(next)) return false;
		customValueChange(next);
		return true;
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Popover, {
		open: pickerOpen,
		onOpenChange: handlePickerOpenChange,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(PopoverTrigger, {
			asChild: true,
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
				id,
				type: "button",
				variant: "outline",
				size: "sm",
				role: "combobox",
				"aria-controls": pickerOpen ? listId : void 0,
				"aria-expanded": pickerOpen,
				"aria-label": triggerAriaLabel,
				disabled,
				className: cn("w-fit min-w-0 justify-between px-3 font-normal", className),
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "min-w-0 flex-1 truncate text-left",
					children: selectedOption?.label ?? placeholder
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronDown, {
					className: "size-4 shrink-0 text-muted-foreground",
					"aria-hidden": true
				})]
			})
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(PopoverContent, {
			align: "start",
			sideOffset: 4,
			className: "w-[var(--radix-popover-trigger-width)] min-w-[14rem] p-0",
			onOpenAutoFocus: (event) => {
				event.preventDefault();
				listRef.current?.focus();
			},
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Command, {
				shouldFilter: false,
				loop: true,
				value: commandValue,
				onValueChange: setCommandValue,
				onKeyDown: handleCommandKeyDown,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CommandList, {
					ref: handleListRef,
					label: triggerAriaLabel,
					className: "max-h-72 py-1",
					children: [
						options.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CommandGroup, { children: options.map((option) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AddressPickerItem, {
							option,
							selected: !isCustomSelection && option.value === value,
							commandValue: `detected:${option.value}`,
							onSelect: () => selectValue(option.value, false)
						}, option.value)) }) : null,
						displayedCustomOptions.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CommandGroup, {
							heading: customSectionLabel,
							className: cn(options.length > 0 && "border-t border-border pt-1"),
							children: displayedCustomOptions.map((option) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AddressPickerItem, {
								option,
								selected: isCustomSelection && option.value === value,
								commandValue: `custom:${option.value}`,
								onSelect: () => selectValue(option.value, true),
								onRemove: onCustomRemove ? () => {
									restoreFocusAfterRemovalRef.current = true;
									onCustomRemove(option.value);
								} : void 0,
								removeLabel: removeCustomLabel?.(option.value)
							}, option.value))
						}) : null,
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CommandGroup, {
							className: cn((options.length > 0 || displayedCustomOptions.length > 0) && "border-t border-border pt-1"),
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CommandItem, {
								value: "add-custom-address",
								onSelect: () => {
									setPickerOpen(false);
									setDialogOpen(true);
								},
								className: "text-muted-foreground data-[selected=true]:text-foreground",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Plus, {
									className: "size-3.5",
									"aria-hidden": true
								}), addCustomLabel]
							})
						})
					]
				})
			})
		})]
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CustomAddressDialog, {
		open: dialogOpen,
		onOpenChange: setDialogOpen,
		initialValue: isCustomSelection ? value : void 0,
		validate: validateCustom,
		copy: customDialogCopy,
		inputId: customInputId,
		onConfirm: handleCustomConfirm
	})] });
}
function formatCustomAddressLabel(address) {
	return translate("auto.components.mobile.NetworkInterfacePicker.custom-option", "{{address}} (custom)", { address });
}
function NetworkInterfacePicker({ networkInterfaces, customAddresses, selectedAddress, selectedAddressIsCustom, onSelectedAddressChange, onCustomAddressSelect, onCustomAddressRemove, beforeCustomAddressChange, disabled = false, className, id }) {
	const options = (0, import_react.useMemo)(() => networkInterfaces.map((iface) => ({
		value: iface.address,
		label: `${iface.address} (${iface.name})`
	})), [networkInterfaces]);
	const customOptions = (0, import_react.useMemo)(() => customAddresses.map((address) => ({
		value: address,
		label: formatCustomAddressLabel(address)
	})), [customAddresses]);
	const placeholder = options.length > 0 || customOptions.length > 0 ? translate("auto.components.mobile.NetworkInterfacePicker.no-address-selected", "No address selected") : translate("auto.components.settings.MobileNetworkInterfaceSection.b2c384cfd6", "No interfaces found");
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AddressPicker, {
		options,
		customOptions,
		value: selectedAddress,
		valueIsCustom: selectedAddressIsCustom,
		onValueChange: onSelectedAddressChange,
		onCustomValueChange: onCustomAddressSelect,
		onCustomRemove: onCustomAddressRemove,
		beforeCustomConfirm: beforeCustomAddressChange,
		disabled,
		className,
		id,
		formatCustomLabel: formatCustomAddressLabel,
		customSectionLabel: translate("auto.components.mobile.NetworkInterfacePicker.custom-section", "Custom"),
		removeCustomLabel: (address) => translate("auto.components.mobile.NetworkInterfacePicker.remove-custom", "Remove {{address}}", { address }),
		addCustomLabel: translate("auto.components.mobile.NetworkInterfacePicker.add-custom", "Add custom address…"),
		placeholder,
		triggerAriaLabel: translate("auto.components.mobile.NetworkInterfacePicker.trigger-label", "Network address to advertise"),
		customInputId: "custom-network-address-input",
		validateCustom: (input) => {
			const parsed = parseManualNetworkAddress(input);
			return parsed.ok ? {
				ok: true,
				value: parsed.address
			} : { ok: false };
		},
		customDialogCopy: {
			title: translate("auto.components.mobile.CustomNetworkAddressDialog.title", "Custom network address"),
			description: translate("auto.components.mobile.CustomNetworkAddressDialog.description", "Advertise an address your phone can reach — for example a Tailscale hostname, IP address, or reverse-proxy URL."),
			inputLabel: translate("auto.components.mobile.CustomNetworkAddressDialog.label", "Address"),
			placeholder: translate("auto.components.mobile.CustomNetworkAddressDialog.placeholder", "home.example.com:8443 or https://example.com/orca"),
			hint: translate("auto.components.mobile.CustomNetworkAddressDialog.hint", "Enter an IPv4/IPv6 address, hostname, or full HTTP(S)/WebSocket URL. Ports are optional."),
			cancel: translate("auto.components.mobile.CustomNetworkAddressDialog.cancel", "Cancel"),
			confirm: translate("auto.components.mobile.CustomNetworkAddressDialog.use", "Use address"),
			confirmationError: translate("auto.components.mobile.CustomNetworkAddressDialog.confirmationError", "This address could not produce a scannable pairing code. Check the address and try again.")
		}
	});
}
function MobilePairingPathOption({ selected, onSelect, title, description, trailing, tabIndex, disabled = false, positionInSet, setSize, optionRef }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		ref: optionRef,
		role: "radio",
		tabIndex,
		"aria-checked": selected,
		"aria-disabled": disabled,
		"aria-posinset": positionInSet,
		"aria-setsize": setSize,
		onClick: disabled ? void 0 : onSelect,
		onKeyDown: (event) => {
			if (disabled) return;
			if (event.key === " " || event.key === "Enter") {
				event.preventDefault();
				onSelect();
			}
		},
		className: cn("flex cursor-pointer items-start gap-3 px-3 py-2.5 outline-none transition-colors", "focus-visible:bg-accent/50 focus-visible:ring-[3px] focus-visible:ring-ring/50", disabled && "cursor-not-allowed opacity-60", selected ? "bg-accent/40" : "hover:bg-accent/20"),
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: cn("mt-0.5 flex size-3.5 shrink-0 items-center justify-center rounded-full border", selected ? "border-foreground bg-foreground" : "border-muted-foreground/40"),
			"aria-hidden": true,
			children: selected ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "size-1.5 rounded-full bg-background" }) : null
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "min-w-0 flex-1",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex flex-wrap items-center gap-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "text-sm font-medium leading-none",
					children: title
				}), trailing]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				className: "mt-1 text-xs text-muted-foreground",
				children: description
			})]
		})]
	});
}
function relayStatusLabel(status) {
	if (status === "registered") return translate("auto.components.settings.MobilePairingConnectionOptions.ready", "Ready");
	if (status === "connecting") return translate("auto.components.settings.MobilePairingConnectionOptions.connecting", "Connecting");
	if (status === "standby") return translate("auto.components.settings.MobilePairingConnectionOptions.available", "Available");
	if (status === "draining") return translate("auto.components.settings.MobilePairingConnectionOptions.reconnecting", "Reconnecting");
	return translate("auto.components.settings.MobilePairingConnectionOptions.unavailable", "Unavailable");
}
function MobilePairingConnectionOptions({ value, onChange, compact = false, relayMintFailed = false, relayMintRetrying = false }) {
	const authStatus = useAppStore((state) => state.orcaProfileAuthStatus);
	const connecting = useAppStore((state) => state.orcaProfileConnecting);
	const connect = useAppStore((state) => state.connectCurrentOrcaProfile);
	const fetchAuthStatus = useAppStore((state) => state.fetchOrcaProfileAuthStatus);
	const [relayStatus, setRelayStatus] = (0, import_react.useState)("offline");
	const signedIn = authStatus?.state === "connected";
	const reconnectRequired = authStatus?.state === "reconnect-required";
	const configured = authStatus?.configured !== false;
	const needsSignIn = value === "automatic" && !signedIn && configured;
	const relayUnavailable = !signedIn && !configured;
	const relayDisabled = relayMintRetrying || relayUnavailable;
	const optionRefs = (0, import_react.useRef)({
		automatic: null,
		"local-only": null
	});
	const handleArrowKeys = (event) => {
		if (![
			"ArrowUp",
			"ArrowDown",
			"ArrowLeft",
			"ArrowRight"
		].includes(event.key)) return;
		const target = event.target;
		if (!(target instanceof HTMLElement) || target.getAttribute("role") !== "radio") return;
		if (relayDisabled && value !== "automatic") return;
		event.preventDefault();
		const next = relayDisabled || value === "automatic" ? "local-only" : "automatic";
		onChange(next);
		optionRefs.current[next]?.focus();
	};
	(0, import_react.useEffect)(() => {
		if (!authStatus) fetchAuthStatus();
	}, [authStatus, fetchAuthStatus]);
	(0, import_react.useEffect)(() => {
		let receivedEvent = false;
		let active = true;
		const unsubscribe = window.api.mobile.onRelayStatusChanged((status) => {
			receivedEvent = true;
			if (active) setRelayStatus(status);
		});
		window.api.mobile.getRelayStatus().then(({ status }) => {
			if (active && !receivedEvent) setRelayStatus(status);
		}).catch(() => {});
		return () => {
			active = false;
			unsubscribe();
		};
	}, []);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: cn("space-y-2", compact && "space-y-1.5"),
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			role: "radiogroup",
			"aria-label": translate("auto.components.settings.MobilePairingConnectionOptions.pathGroup", "How the phone reaches this computer"),
			onKeyDown: handleArrowKeys,
			className: "overflow-hidden rounded-md border border-border",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(MobilePairingPathOption, {
					selected: value === "automatic",
					tabIndex: value === "automatic" && !relayDisabled ? 0 : -1,
					disabled: relayDisabled,
					positionInSet: 1,
					setSize: 2,
					optionRef: (el) => {
						optionRefs.current.automatic = el;
					},
					onSelect: () => onChange("automatic"),
					title: translate("auto.components.settings.MobilePairingConnectionOptions.anywhereTitle", "Orca Relay"),
					description: relayUnavailable ? translate("auto.components.settings.MobilePairingConnectionOptions.relayUnavailable", "Orca Relay isn’t available in this build. Use LAN.") : translate("auto.components.settings.MobilePairingConnectionOptions.anywhereDescription", "Phone can be on cellular or any Wi‑Fi. Sign-in required for Relay only."),
					trailing: relayUnavailable ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
						variant: "outline",
						className: "text-[11px]",
						children: translate("auto.components.settings.MobilePairingConnectionOptions.unavailable", "Unavailable")
					}) : signedIn && value === "automatic" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
						variant: "outline",
						className: "text-[11px]",
						children: relayMintRetrying ? translate("auto.components.settings.MobilePairingConnectionOptions.retrying", "Retrying") : relayMintFailed ? translate("auto.components.settings.MobilePairingConnectionOptions.unavailable", "Unavailable") : relayStatusLabel(relayStatus)
					}) : null
				}),
				needsSignIn ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					onKeyDown: (event) => {
						if ([
							"ArrowUp",
							"ArrowDown",
							"ArrowLeft",
							"ArrowRight"
						].includes(event.key)) event.stopPropagation();
					},
					className: "flex flex-wrap items-center justify-between gap-2 border-t border-border/60 bg-accent/40 py-2.5 pl-10 pr-3",
					"data-testid": "anywhere-sign-in-panel",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "min-w-0 flex-1 text-xs text-muted-foreground",
						children: translate("auto.components.settings.MobilePairingConnectionOptions.signInRequired", "Relay only — LAN does not need an account.")
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
						type: "button",
						size: "sm",
						className: "shrink-0",
						disabled: connecting,
						onClick: () => {
							onChange("automatic");
							connect();
						},
						children: [connecting ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "animate-spin" }) : null, reconnectRequired ? translate("auto.components.settings.MobilePairingConnectionOptions.signInAgain", "Sign in again for Relay") : translate("auto.components.settings.MobilePairingConnectionOptions.signIn", "Sign in for Relay")]
					})]
				}) : null,
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "border-t border-border" }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(MobilePairingPathOption, {
					selected: value === "local-only",
					tabIndex: value === "local-only" || relayDisabled ? 0 : -1,
					positionInSet: 2,
					setSize: 2,
					optionRef: (el) => {
						optionRefs.current["local-only"] = el;
					},
					onSelect: () => onChange("local-only"),
					title: translate("auto.components.settings.MobilePairingConnectionOptions.localTitle", "LAN"),
					description: translate("auto.components.settings.MobilePairingConnectionOptions.localDescription", "Phone must be on this Wi‑Fi or connected through Tailscale. No account needed.")
				})
			]
		})
	});
}
function MobileRelayBetaNotice({ className }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
		className: cn("text-[11px] text-muted-foreground", className),
		children: translate("auto.components.settings.MobileRelayBetaNotice.notice", "Orca Relay is in beta.")
	});
}
function MobileRelayMintFailureNotice({ failure, onUseLan, onRetry, onCopyDiagnostics, className, compact = false, busy = false }) {
	const providerMissing = failure.stage === "provider_missing";
	const [showBusyFeedback, setShowBusyFeedback] = (0, import_react.useState)(false);
	(0, import_react.useEffect)(() => {
		if (!busy) {
			setShowBusyFeedback(false);
			return;
		}
		const timer = window.setTimeout(() => setShowBusyFeedback(true), 200);
		return () => window.clearTimeout(timer);
	}, [busy]);
	const visibleBusy = busy && showBusyFeedback;
	const title = visibleBusy ? translate("auto.components.mobile.MobileRelayMintFailureNotice.retryingTitle", "Retrying Orca Relay…") : providerMissing ? translate("auto.components.mobile.MobileRelayMintFailureNotice.unavailableTitle", "Orca Relay isn’t available on this desktop.") : translate("auto.components.mobile.MobileRelayMintFailureNotice.title", "Couldn’t create a Relay pairing code.");
	const body = visibleBusy ? translate("auto.components.mobile.MobileRelayMintFailureNotice.retryingBody", "Creating a new pairing code. This can take a moment over a remote connection.") : providerMissing ? translate("auto.components.mobile.MobileRelayMintFailureNotice.unavailableBody", "Use LAN to pair over Tailscale or the same Wi‑Fi.") : translate("auto.components.mobile.MobileRelayMintFailureNotice.body", "Retry, or use LAN to pair over Tailscale or the same Wi‑Fi.");
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: cn("flex w-full min-w-0 items-start gap-2 rounded-lg border p-3 text-xs", visibleBusy ? "border-border bg-muted/40 text-foreground" : "border-destructive/30 bg-destructive/10 text-destructive", className),
		"data-testid": "relay-mint-failure-notice",
		children: [visibleBusy ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, {
			className: "mt-0.5 size-3.5 shrink-0 animate-spin",
			"aria-hidden": true
		}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CircleAlert, {
			className: "mt-0.5 size-3.5 shrink-0",
			"aria-hidden": true
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "min-w-0 flex-1 space-y-2",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
				className: "min-w-0",
				role: visibleBusy ? "status" : "alert",
				"aria-live": visibleBusy ? "polite" : "assertive",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "font-medium",
						children: title
					}),
					" ",
					body
				]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex flex-wrap gap-2",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						type: "button",
						size: compact ? "xs" : "sm",
						onClick: onUseLan,
						children: translate("auto.components.mobile.MobileRelayMintFailureNotice.useLan", "Use LAN")
					}),
					!providerMissing ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
						type: "button",
						size: compact ? "xs" : "sm",
						variant: "outline",
						onClick: onRetry,
						disabled: busy,
						className: "w-28",
						children: [visibleBusy ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "animate-spin" }) : null, visibleBusy ? translate("auto.components.mobile.MobileRelayMintFailureNotice.retrying", "Retrying…") : translate("auto.components.mobile.MobileRelayMintFailureNotice.retry", "Retry Relay")]
					}) : null,
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						type: "button",
						size: compact ? "xs" : "sm",
						variant: "ghost",
						onClick: onCopyDiagnostics,
						children: translate("auto.components.mobile.MobileRelayMintFailureNotice.copyDiagnostics", "Copy diagnostics")
					})
				]
			})]
		})]
	});
}
function WindowsFirewallNotice({ pairingReady, address, usingRelay = false, className }) {
	const [status, setStatus] = (0, import_react.useState)(null);
	const [repairing, setRepairing] = (0, import_react.useState)(false);
	const mountedRef = useMountedRef();
	const inspectIdRef = (0, import_react.useRef)(0);
	const inspect = (0, import_react.useCallback)(async () => {
		const inspectId = ++inspectIdRef.current;
		if (!pairingReady) {
			setStatus(null);
			return null;
		}
		try {
			const next = await window.api.mobile.getWindowsFirewallStatus(address ? { address } : void 0);
			if (mountedRef.current && inspectIdRef.current === inspectId) setStatus(next);
			return next;
		} catch {
			if (mountedRef.current && inspectIdRef.current === inspectId) setStatus(null);
			return null;
		}
	}, [
		address,
		mountedRef,
		pairingReady
	]);
	(0, import_react.useEffect)(() => {
		inspect();
		window.addEventListener("focus", inspect);
		return () => window.removeEventListener("focus", inspect);
	}, [inspect]);
	if (!status?.supported) return null;
	const firewallStatus = status;
	const networkIsPublic = firewallStatus.networkCategory === "public";
	const blockingRuleDetected = firewallStatus.blockingRuleDetected;
	if (!pairingReady || firewallStatus.networkCategory === "domain") return null;
	if (!networkIsPublic && (!firewallStatus.privateFirewallEnabled || firewallStatus.ruleAllowed && !blockingRuleDetected)) return null;
	async function repair() {
		setRepairing(true);
		try {
			const result = await window.api.mobile.repairWindowsFirewall();
			if (!mountedRef.current) return;
			if (result.ok) {
				const next = await inspect();
				if (!mountedRef.current) return;
				if (next?.supported && (!next.privateFirewallEnabled || next.ruleAllowed && !next.blockingRuleDetected && next.inspectionAvailable)) {
					toast.success(translate("auto.components.mobile.WindowsFirewallNotice.repair-success", "Windows Firewall now allows Orca Mobile on private networks"));
					return;
				}
				if (!next) setStatus(firewallStatus);
				toast.error(translate("auto.components.mobile.WindowsFirewallNotice.repair-unverified", "Windows Firewall access could not be verified"));
				return;
			}
			if (result.reason !== "cancelled") toast.error(translate("auto.components.mobile.WindowsFirewallNotice.repair-failed", "Could not update the Windows Firewall rules"));
		} catch {
			if (mountedRef.current) toast.error(translate("auto.components.mobile.WindowsFirewallNotice.repair-failed", "Could not update the Windows Firewall rules"));
		} finally {
			if (mountedRef.current) setRepairing(false);
		}
	}
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: cn("rounded-lg border border-border bg-muted/40 p-3", className),
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex items-start gap-2.5",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CircleAlert, { className: "mt-0.5 size-4 shrink-0 text-muted-foreground" }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "min-w-0 flex-1 space-y-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "space-y-1",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "text-sm font-medium",
							children: networkIsPublic ? translate("auto.components.mobile.WindowsFirewallNotice.public-title", "Windows marks this network as public") : blockingRuleDetected ? translate("auto.components.mobile.WindowsFirewallNotice.blocked-title", "Windows may be blocking Orca Mobile") : translate("auto.components.mobile.WindowsFirewallNotice.missing-title", "Allow phone connections through Windows Firewall")
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "text-xs text-muted-foreground",
							children: networkIsPublic ? translate("auto.components.mobile.WindowsFirewallNotice.public-description", "Change this trusted Wi-Fi network to Private before allowing Orca Mobile connections.") : blockingRuleDetected ? translate("auto.components.mobile.WindowsFirewallNotice.blocked-description", "An existing inbound Block rule can override the pairing exception. Repair removes conflicting TCP rules for this Orca app, then allows port {{port}} on Private networks.", { port: firewallStatus.port }) : translate("auto.components.mobile.WindowsFirewallNotice.missing-description", "Windows may block the pairing server. Add a rule for this Orca app and TCP port {{port}} on Private networks.", { port: firewallStatus.port })
						}),
						usingRelay ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "text-xs text-muted-foreground",
							children: translate("auto.components.mobile.WindowsFirewallNotice.relay-note", "Pairing still works over Orca Relay — allowing this only adds the faster local connection.")
						}) : null
					]
				}), networkIsPublic ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					type: "button",
					size: "sm",
					variant: "outline",
					onClick: () => void window.api.mobile.openWindowsNetworkSettings(),
					children: translate("auto.components.mobile.WindowsFirewallNotice.open-settings", "Open network settings")
				}) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
					type: "button",
					size: "sm",
					onClick: () => void repair(),
					disabled: repairing,
					children: [repairing ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "animate-spin" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ShieldCheck, { "aria-hidden": "true" }), repairing ? translate("auto.components.mobile.WindowsFirewallNotice.waiting", "Waiting for Windows…") : blockingRuleDetected ? translate("auto.components.mobile.WindowsFirewallNotice.repair", "Repair firewall access") : translate("auto.components.mobile.WindowsFirewallNotice.allow", "Allow phone connections")]
				})]
			})]
		})
	});
}
function resolveMobilePairingConnectionMode(saved) {
	return saved === "local-only" ? "local-only" : "automatic";
}
function canMintMobilePairingOffer(args) {
	return !(args.connectionMode === "automatic" && !args.signedIn);
}
function useMobilePairingConnectionMode() {
	const savedConnectionMode = useAppStore((s) => s.settings?.mobilePairingConnectionMode);
	const [connectionMode, setConnectionMode] = (0, import_react.useState)(() => resolveMobilePairingConnectionMode(savedConnectionMode));
	(0, import_react.useEffect)(() => {
		setConnectionMode(resolveMobilePairingConnectionMode(savedConnectionMode));
	}, [savedConnectionMode]);
	return [connectionMode, setConnectionMode];
}
const MOBILE_PAIRING_DEVICE_POLL_MS = 3e3;
function shouldPollMobilePairingDevices({ deviceCountAtQr, currentDeviceCount, visibilityState, focused }) {
	return deviceCountAtQr !== null && currentDeviceCount <= deviceCountAtQr && visibilityState === "visible" && focused;
}
function useMobilePairingDevicePolling({ deviceCountAtQr, currentDeviceCount, loadDevices }) {
	(0, import_react.useEffect)(() => {
		if (deviceCountAtQr === null || currentDeviceCount > deviceCountAtQr) return;
		let stopped = false;
		let pollInFlight = false;
		let timeoutId = null;
		const clearPendingPoll = () => {
			if (timeoutId !== null) {
				window.clearTimeout(timeoutId);
				timeoutId = null;
			}
		};
		const canPoll = () => shouldPollMobilePairingDevices({
			deviceCountAtQr,
			currentDeviceCount,
			visibilityState: document.visibilityState,
			focused: document.hasFocus()
		});
		const scheduleNextPoll = () => {
			clearPendingPoll();
			if (stopped || !canPoll()) return;
			timeoutId = window.setTimeout(() => {
				timeoutId = null;
				if (stopped || !canPoll()) return;
				if (pollInFlight) {
					scheduleNextPoll();
					return;
				}
				pollInFlight = true;
				loadDevices().finally(() => {
					pollInFlight = false;
					scheduleNextPoll();
				});
			}, MOBILE_PAIRING_DEVICE_POLL_MS);
		};
		const resumePolling = () => {
			if (!canPoll()) {
				clearPendingPoll();
				return;
			}
			if (pollInFlight) return;
			pollInFlight = true;
			loadDevices().finally(() => {
				pollInFlight = false;
				scheduleNextPoll();
			});
		};
		scheduleNextPoll();
		window.addEventListener("focus", resumePolling);
		document.addEventListener("visibilitychange", resumePolling);
		return () => {
			stopped = true;
			clearPendingPoll();
			window.removeEventListener("focus", resumePolling);
			document.removeEventListener("visibilitychange", resumePolling);
		};
	}, [
		deviceCountAtQr,
		currentDeviceCount,
		loadDevices
	]);
}
var VIRTUAL_BRIDGE_INTERFACE_PATTERN = /^(?:docker|br-|virbr|vmnet|vboxnet|veth|lxcbr|cni|flannel|cali|bridge)|VMware Network Adapter|VirtualBox Host-Only/i;
var HYPER_V_INTERFACE_PATTERN = /^vEthernet /i;
var HOST_LOCAL_HYPER_V_INTERFACE_PATTERN = /^vEthernet \((?:Default Switch|WSL(?: \(Hyper-V firewall\))?)\)$/i;
function isVirtualBridgeInterface(name, hasDefaultRoute) {
	if (HOST_LOCAL_HYPER_V_INTERFACE_PATTERN.test(name)) return true;
	if (HYPER_V_INTERFACE_PATTERN.test(name)) return hasDefaultRoute !== true;
	return VIRTUAL_BRIDGE_INTERFACE_PATTERN.test(name);
}
function selectAutoAdvertisedPairingAddress(interfaces) {
	const advertisable = interfaces.filter((iface) => !isVirtualBridgeInterface(iface.name, iface.hasDefaultRoute));
	return advertisable.find((iface) => isTailnetIPv4Address(iface.address))?.address ?? advertisable[0]?.address;
}
function selectRefreshedNetworkAddress(currentAddress, interfaces, currentAddressIsManual = false, currentAddressWasExplicitlySelected = false) {
	if (interfaces.length === 0) return currentAddressIsManual || currentAddressWasExplicitlySelected ? currentAddress : void 0;
	if (currentAddressIsManual) return currentAddress;
	const currentInterface = interfaces.find((iface) => iface.address === currentAddress);
	if (currentInterface && (currentAddressWasExplicitlySelected || !isVirtualBridgeInterface(currentInterface.name, currentInterface.hasDefaultRoute))) return currentAddress;
	return selectAutoAdvertisedPairingAddress(interfaces);
}
function useMobilePairingCustomAddress() {
	return normalizeMobilePairingCustomAddress(useAppStore((state) => state.settings?.mobilePairingCustomAddress)) ?? void 0;
}
function useMobilePairingCustomAddresses() {
	const savedAddresses = useAppStore((state) => state.settings?.mobilePairingCustomAddresses);
	const savedAddress = useAppStore((state) => state.settings?.mobilePairingCustomAddress);
	return (0, import_react.useMemo)(() => {
		const normalized = normalizeMobilePairingCustomAddresses(savedAddresses);
		return typeof savedAddress === "string" ? addMobilePairingCustomAddress(normalized, savedAddress) : normalized;
	}, [savedAddress, savedAddresses]);
}
function haveSameAddresses(left, right) {
	return left.length === right.length && left.every((address, index) => address === right[index]);
}
function useMobilePairingAddressPreference(args) {
	const { networkInterfaces, onSelectionInvalidated } = args;
	const updateSettings = useAppStore((state) => state.updateSettings);
	const savedCustomAddress = useMobilePairingCustomAddress();
	const savedCustomAddresses = useMobilePairingCustomAddresses();
	const [selectedAddress, setSelectedAddress] = (0, import_react.useState)(savedCustomAddress);
	const [selectedAddressIsCustom, setSelectedAddressIsCustom] = (0, import_react.useState)(savedCustomAddress !== void 0);
	const [customAddresses, setCustomAddresses] = (0, import_react.useState)(savedCustomAddresses);
	const selectedAddressRef = (0, import_react.useRef)(selectedAddress);
	const selectedAddressIsManualRef = (0, import_react.useRef)(savedCustomAddress !== void 0);
	const selectedAddressWasExplicitlySelectedRef = (0, import_react.useRef)(savedCustomAddress !== void 0);
	const customAddressesRef = (0, import_react.useRef)(savedCustomAddresses);
	const observedCustomAddressRef = (0, import_react.useRef)(savedCustomAddress);
	const pendingCustomAddressWritesRef = (0, import_react.useRef)([]);
	const selectAddressAfterRefresh = (0, import_react.useCallback)((interfaces) => {
		const nextAddress = selectRefreshedNetworkAddress(selectedAddressRef.current, interfaces, selectedAddressIsManualRef.current, selectedAddressWasExplicitlySelectedRef.current);
		if (nextAddress === selectedAddressRef.current) return;
		const hadSelection = selectedAddressRef.current !== void 0;
		selectedAddressRef.current = nextAddress;
		selectedAddressIsManualRef.current = false;
		selectedAddressWasExplicitlySelectedRef.current = false;
		setSelectedAddress(nextAddress);
		setSelectedAddressIsCustom(false);
		if (hadSelection) onSelectionInvalidated({
			address: nextAddress,
			source: "refresh"
		});
	}, [onSelectionInvalidated]);
	const commitAddress = (0, import_react.useCallback)((address, isManual) => {
		const addressChanged = selectedAddressRef.current !== address;
		if (!addressChanged && selectedAddressIsManualRef.current === isManual && selectedAddressWasExplicitlySelectedRef.current) return;
		selectedAddressRef.current = address;
		selectedAddressIsManualRef.current = isManual;
		selectedAddressWasExplicitlySelectedRef.current = true;
		setSelectedAddress(address);
		setSelectedAddressIsCustom(isManual);
		const customAddress = isManual ? address : void 0;
		const pendingWrites = pendingCustomAddressWritesRef.current;
		if (customAddress !== (pendingWrites.length > 0 ? pendingWrites.at(-1) : observedCustomAddressRef.current)) {
			pendingWrites.push(customAddress);
			if (customAddress) {
				const nextCustomAddresses = addMobilePairingCustomAddress(customAddressesRef.current, customAddress);
				customAddressesRef.current = nextCustomAddresses;
				setCustomAddresses(nextCustomAddresses);
				updateSettings({
					mobilePairingCustomAddress: customAddress,
					mobilePairingCustomAddresses: nextCustomAddresses
				});
			} else updateSettings({ mobilePairingCustomAddress: null });
		}
		if (addressChanged) onSelectionInvalidated({
			address,
			source: "user"
		});
	}, [onSelectionInvalidated, updateSettings]);
	const selectAddress = (0, import_react.useCallback)((address) => {
		commitAddress(address, !networkInterfaces.some((iface) => iface.address === address));
	}, [commitAddress, networkInterfaces]);
	const selectCustomAddress = (0, import_react.useCallback)((address) => commitAddress(address, true), [commitAddress]);
	const removeCustomAddress = (0, import_react.useCallback)((address) => {
		const nextCustomAddresses = removeMobilePairingCustomAddress(customAddressesRef.current, address);
		if (haveSameAddresses(nextCustomAddresses, customAddressesRef.current)) return;
		customAddressesRef.current = nextCustomAddresses;
		setCustomAddresses(nextCustomAddresses);
		if (!(selectedAddressIsManualRef.current && selectedAddressRef.current === address)) {
			updateSettings({ mobilePairingCustomAddresses: nextCustomAddresses });
			return;
		}
		const nextAddress = selectRefreshedNetworkAddress(void 0, networkInterfaces);
		const addressChanged = selectedAddressRef.current !== nextAddress;
		selectedAddressRef.current = nextAddress;
		selectedAddressIsManualRef.current = false;
		selectedAddressWasExplicitlySelectedRef.current = false;
		setSelectedAddress(nextAddress);
		setSelectedAddressIsCustom(false);
		pendingCustomAddressWritesRef.current.push(void 0);
		updateSettings({
			mobilePairingCustomAddress: null,
			mobilePairingCustomAddresses: nextCustomAddresses
		});
		if (addressChanged) onSelectionInvalidated({
			address: nextAddress,
			source: "user"
		});
	}, [
		networkInterfaces,
		onSelectionInvalidated,
		updateSettings
	]);
	(0, import_react.useEffect)(() => {
		if (savedCustomAddress === observedCustomAddressRef.current) return;
		observedCustomAddressRef.current = savedCustomAddress;
		const pendingWrites = pendingCustomAddressWritesRef.current;
		const acknowledgedWriteIndex = pendingWrites.indexOf(savedCustomAddress);
		if (acknowledgedWriteIndex !== -1) {
			pendingWrites.splice(0, acknowledgedWriteIndex + 1);
			return;
		}
		pendingWrites.length = 0;
		const nextAddress = savedCustomAddress ?? selectRefreshedNetworkAddress(void 0, networkInterfaces);
		const addressChanged = selectedAddressRef.current !== nextAddress;
		selectedAddressRef.current = nextAddress;
		selectedAddressIsManualRef.current = savedCustomAddress !== void 0;
		selectedAddressWasExplicitlySelectedRef.current = savedCustomAddress !== void 0;
		setSelectedAddress(nextAddress);
		setSelectedAddressIsCustom(savedCustomAddress !== void 0);
		if (addressChanged) onSelectionInvalidated({
			address: nextAddress,
			source: "external"
		});
	}, [
		networkInterfaces,
		onSelectionInvalidated,
		savedCustomAddress
	]);
	(0, import_react.useEffect)(() => {
		if (pendingCustomAddressWritesRef.current.length > 0) return;
		if (haveSameAddresses(savedCustomAddresses, customAddressesRef.current)) return;
		customAddressesRef.current = savedCustomAddresses;
		setCustomAddresses(savedCustomAddresses);
	}, [savedCustomAddresses]);
	return {
		selectedAddress,
		selectedAddressIsCustom,
		customAddresses,
		selectAddress,
		selectCustomAddress,
		removeCustomAddress,
		selectAddressAfterRefresh
	};
}
export { WindowsFirewallNotice as a, MobilePairingConnectionOptions as c, AndroidLogo as d, IosBrandIcon as f, canMintMobilePairingOffer as i, NetworkInterfacePicker as l, useMobilePairingDevicePolling as n, MobileRelayMintFailureNotice as o, useMobilePairingConnectionMode as r, MobileRelayBetaNotice as s, useMobilePairingAddressPreference as t, AddressPicker as u };
