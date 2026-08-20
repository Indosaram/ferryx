import { t as useAppStore } from "./store-CgXrfmaH.js";
import { d as formatKeybinding, f as formatKeybindingList, p as getEffectiveKeybindingsForAction, v as isDoubleTapBinding } from "./plugin-manifest-Bs-50M_g.js";
import { t as getShortcutPlatform } from "./shortcut-platform-BbPBGzth.js";
function formatShortcutLabel(actionId, overrides) {
	const platform = getShortcutPlatform();
	return formatKeybindingList(getEffectiveKeybindingsForAction(actionId, platform, overrides), platform);
}
function formatPrimaryShortcutLabel(actionId, overrides) {
	const platform = getShortcutPlatform();
	const [binding] = getEffectiveKeybindingsForAction(actionId, platform, overrides);
	return binding ? formatKeybindingList([binding], platform) : "Unassigned";
}
function useShortcutLabel(actionId) {
	return formatShortcutLabel(actionId, useAppStore((state) => state.keybindings));
}
function formatOptionalShortcutLabel(actionId, overrides) {
	const platform = getShortcutPlatform();
	const bindings = getEffectiveKeybindingsForAction(actionId, platform, overrides);
	if (bindings.length === 0) return null;
	return formatKeybindingList(bindings, platform);
}
function useOptionalShortcutLabel(actionId) {
	return formatOptionalShortcutLabel(actionId, useAppStore((state) => state.keybindings));
}
function formatShortcutKeyComboDetails(actionId, overrides) {
	const platform = getShortcutPlatform();
	return getEffectiveKeybindingsForAction(actionId, platform, overrides).map((binding) => ({
		keys: formatKeybinding(binding, platform),
		doubleTap: isDoubleTapBinding(binding)
	}));
}
function useShortcutKeyComboDetails(actionId) {
	return formatShortcutKeyComboDetails(actionId, useAppStore((state) => state.keybindings));
}
function useShortcutKeyDetails(actionId) {
	return useShortcutKeyComboDetails(actionId)[0] ?? {
		keys: [],
		doubleTap: false
	};
}
export { useShortcutKeyComboDetails as a, useOptionalShortcutLabel as i, formatShortcutKeyComboDetails as n, useShortcutKeyDetails as o, formatShortcutLabel as r, useShortcutLabel as s, formatPrimaryShortcutLabel as t };
