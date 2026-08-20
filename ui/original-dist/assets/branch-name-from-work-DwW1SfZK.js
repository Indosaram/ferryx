import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import { l as createLucideIcon, t as Button } from "./button-DszXJEV6.js";
import { _u as getRuntimeEnvironmentIdForRepo, t as useAppStore } from "./store-CgXrfmaH.js";
import { st as parseExecutionHostId } from "./agent-status-3vUKbY6l.js";
import { t as Input } from "./input-DV5rpysh.js";
import { i as isRuntimeRepoRefSearchQueryWithinLimit, r as searchRuntimeRepoBaseRefs, t as getRuntimeRepoBaseRefDefault } from "./runtime-repo-client-Cii78G29.js";
import { t as MARINE_CREATURES } from "./marine-creatures-C7R2Pjk9.js";
var CloudUpload = createLucideIcon("cloud-upload", [
	["path", {
		d: "M12 13v8",
		key: "1l5pq0"
	}],
	["path", {
		d: "M4 14.899A7 7 0 1 1 15.71 8h1.79a4.5 4.5 0 0 1 2.5 8.242",
		key: "1pljnt"
	}],
	["path", {
		d: "m8 17 4-4 4 4",
		key: "1quai1"
	}]
]);
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
function BaseRefPicker({ repoId, hostId, currentBaseRef, onSelect, onUsePrimary }) {
	const focusedRuntimeEnvironmentId = useAppStore((state) => getRuntimeEnvironmentIdForRepo(state, repoId));
	const selectedHost = parseExecutionHostId(hostId);
	const activeRuntimeEnvironmentId = hostId ? selectedHost?.kind === "runtime" ? selectedHost.environmentId : null : focusedRuntimeEnvironmentId;
	const [defaultBaseRef, setDefaultBaseRef] = (0, import_react.useState)(null);
	const [remoteCount, setRemoteCount] = (0, import_react.useState)(0);
	const [baseRefQuery, setBaseRefQuery] = (0, import_react.useState)("");
	const [baseRefResults, setBaseRefResults] = (0, import_react.useState)([]);
	const [isSearchingBaseRefs, setIsSearchingBaseRefs] = (0, import_react.useState)(false);
	const baseRefResultsListRef = (0, import_react.useRef)(null);
	(0, import_react.useEffect)(() => {
		const el = baseRefResultsListRef.current;
		if (!el) return;
		const onWheel = (event) => {
			if (el.scrollHeight <= el.clientHeight) return;
			event.preventDefault();
			el.scrollTop += event.deltaY;
		};
		el.addEventListener("wheel", onWheel, { passive: false });
		return () => el.removeEventListener("wheel", onWheel);
	}, [baseRefResults.length]);
	(0, import_react.useEffect)(() => {
		let stale = false;
		const loadDefaultBaseRef = async () => {
			try {
				const result = await getRuntimeRepoBaseRefDefault({ activeRuntimeEnvironmentId }, repoId, hostId);
				if (!stale) {
					setDefaultBaseRef(result.defaultBaseRef);
					setRemoteCount(result.remoteCount);
				}
			} catch (err) {
				console.error("[BaseRefPicker] getBaseRefDefault failed", err);
				if (!stale) {
					setDefaultBaseRef(null);
					setRemoteCount(0);
				}
			}
		};
		setBaseRefQuery("");
		setBaseRefResults([]);
		setDefaultBaseRef(null);
		setRemoteCount(0);
		loadDefaultBaseRef();
		return () => {
			stale = true;
		};
	}, [
		activeRuntimeEnvironmentId,
		hostId,
		repoId
	]);
	(0, import_react.useEffect)(() => {
		if (!isRuntimeRepoRefSearchQueryWithinLimit(baseRefQuery)) {
			setBaseRefResults([]);
			setIsSearchingBaseRefs(false);
			return;
		}
		const trimmedQuery = baseRefQuery.trim();
		if (trimmedQuery.length < 2) {
			setBaseRefResults([]);
			setIsSearchingBaseRefs(false);
			return;
		}
		let stale = false;
		setIsSearchingBaseRefs(true);
		const timer = window.setTimeout(() => {
			searchRuntimeRepoBaseRefs({ activeRuntimeEnvironmentId }, repoId, trimmedQuery, 20, hostId).then((results) => {
				if (!stale) setBaseRefResults(results);
			}).catch((err) => {
				console.error("[BaseRefPicker] searchBaseRefs failed", err);
				if (!stale) setBaseRefResults([]);
			}).finally(() => {
				if (!stale) setIsSearchingBaseRefs(false);
			});
		}, 200);
		return () => {
			stale = true;
			window.clearTimeout(timer);
		};
	}, [
		activeRuntimeEnvironmentId,
		baseRefQuery,
		hostId,
		repoId
	]);
	const effectiveBaseRef = currentBaseRef ?? defaultBaseRef;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-2.5",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex flex-wrap items-center justify-between gap-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "text-sm font-medium text-foreground",
						children: effectiveBaseRef ?? translate("auto.components.settings.BaseRefPicker.ee110e1830", "No default base ref")
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "text-xs text-muted-foreground",
						children: currentBaseRef ? translate("auto.components.settings.BaseRefPicker.2f3cda96f5", "Pinned for this repo") : defaultBaseRef ? translate("auto.components.settings.BaseRefPicker.086ce7f369", "Following primary branch ({{value0}})", { value0: defaultBaseRef }) : translate("auto.components.settings.BaseRefPicker.9a14ec7400", "Pick a base branch below")
					}),
					remoteCount > 1 ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
						className: "text-xs text-muted-foreground",
						children: [
							translate("auto.components.settings.BaseRefPicker.a5c16712c1", "Multiple remotes detected. Type a remote name (e.g."),
							" ",
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("code", { children: translate("auto.components.settings.BaseRefPicker.915ad97875", "upstream") }),
							translate("auto.components.settings.BaseRefPicker.80f7c82303", ") or a full ref (e.g."),
							" ",
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("code", { children: translate("auto.components.settings.BaseRefPicker.b468f46726", "upstream/main") }),
							translate("auto.components.settings.BaseRefPicker.ade9a5bb03", ") to scope results.")
						]
					}) : null
				] }), onUsePrimary && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					variant: "outline",
					size: "sm",
					onClick: onUsePrimary,
					disabled: !currentBaseRef,
					children: translate("auto.components.settings.BaseRefPicker.773a5687a3", "Use Primary")
				})]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
				value: baseRefQuery,
				onChange: (e) => setBaseRefQuery(e.target.value),
				placeholder: translate("auto.components.settings.BaseRefPicker.7db7fb87e5", "Search branches by name..."),
				className: "max-w-md"
			}),
			isSearchingBaseRefs ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				className: "text-xs text-muted-foreground",
				children: translate("auto.components.settings.BaseRefPicker.a4a9372eb2", "Searching branches...")
			}) : null,
			!isSearchingBaseRefs && baseRefQuery.trim().length >= 2 ? baseRefResults.length > 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				ref: baseRefResultsListRef,
				className: "max-h-[min(12rem,40vh)] overflow-y-auto overflow-x-hidden rounded-md border border-border/50 scrollbar-sleek",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "p-1",
					children: baseRefResults.map((ref) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
						onClick: () => {
							setBaseRefQuery("");
							setBaseRefResults([]);
							onSelect(ref);
						},
						className: `flex w-full items-center justify-between rounded-sm px-3 py-2 text-left text-sm transition-colors hover:bg-muted/60 ${effectiveBaseRef === ref ? "bg-accent text-accent-foreground" : "text-foreground"}`,
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "truncate",
							children: ref
						}), effectiveBaseRef === ref ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-[10px] uppercase tracking-[0.18em]",
							children: translate("auto.components.settings.BaseRefPicker.d166ff883d", "Current")
						}) : null]
					}, ref))
				})
			}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				className: "text-xs text-muted-foreground",
				children: translate("auto.components.settings.BaseRefPicker.1b8e54151f", "No matching branches found.")
			}) : null
		]
	});
}
new Set(MARINE_CREATURES.map((name) => name.toLowerCase()));
function humanizeBranchSlug(slug) {
	const joined = slug.split("-").filter(Boolean).join(" ");
	if (!joined) return "";
	return joined.charAt(0).toUpperCase() + joined.slice(1);
}
function buildBranchNamePrompt(context, customPrompt = "") {
	const sections = [];
	const prompt = customPrompt.trim();
	if (prompt) sections.push(prompt, "");
	sections.push(prompt ? "Generate a git branch name that summarizes the coding task described below." : "Generate a short git branch name that summarizes the coding task described below.", "Output ONLY the branch name on a single line, nothing else.", "");
	sections.push("User request:", context.firstPrompt.trim());
	const assistant = context.assistantMessage?.trim();
	if (assistant) sections.push("", "Agent's initial response:", assistant);
	return sections.join("\n");
}
export { CloudUpload as i, humanizeBranchSlug as n, BaseRefPicker as r, buildBranchNamePrompt as t };
