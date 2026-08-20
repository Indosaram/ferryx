import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import "./button-DszXJEV6.js";
import { Qp as sanitizeRepoIcon, t as useAppStore } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import { n as toast } from "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./useMountedRef-1omUd-IV.js";
import { n as useSystemPrefersDark } from "./use-system-prefers-dark-QSo6mmSW.js";
import { n as Theme, r as emoji_picker_react_esm_default, t as EmojiStyle } from "./emoji-picker-react.esm-BhgCXOhg.js";
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function RepositoryIconEmojiPicker({ selectedEmoji, onSetIcon }) {
	const settingsTheme = useAppStore((state) => state.settings?.theme ?? "system");
	const systemPrefersDark = useSystemPrefersDark();
	const isDarkTheme = settingsTheme === "dark" || settingsTheme === "system" && systemPrefersDark;
	const handleEmojiClick = (emojiData) => {
		const repoIcon = sanitizeRepoIcon({
			type: "emoji",
			emoji: emojiData.emoji
		});
		if (!repoIcon) {
			toast.error(translate("auto.components.settings.RepositoryIconPicker.emojiTooLongForRepoIcon", "This emoji can't be used as a repo icon."));
			return;
		}
		onSetIcon(repoIcon);
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "repo-icon-emoji-picker overflow-hidden rounded-md border border-border",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(emoji_picker_react_esm_default, {
			autoFocusSearch: false,
			emojiStyle: EmojiStyle.NATIVE,
			height: 340,
			width: "100%",
			lazyLoadEmojis: true,
			onEmojiClick: handleEmojiClick,
			previewConfig: { showPreview: true },
			searchPlaceholder: translate("auto.components.settings.RepositoryIconPicker.searchEmojiPlaceholder", "Search emoji"),
			theme: isDarkTheme ? Theme.DARK : Theme.LIGHT
		})
	}), selectedEmoji ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
		className: "mt-2 text-[11px] text-muted-foreground",
		children: translate("auto.components.settings.RepositoryIconPicker.currentEmojiSelection", "Current: {{value0}}", { value0: selectedEmoji })
	}) : null] });
}
export { RepositoryIconEmojiPicker };
