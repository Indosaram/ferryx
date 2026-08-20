import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { a as translate, t as require_jsx_runtime } from "./jsx-runtime-Cv_nyRjc.js";
import "./button-DszXJEV6.js";
import { t as Plus } from "./plus-Db0kWPVa.js";
import { t as useAppStore } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./es2015-B5WZ-7WO.js";
import { f as ContextMenuTrigger, i as ContextMenuLabel, n as ContextMenuContent, r as ContextMenuItem, t as ContextMenu } from "./context-menu-D4RKI7hR.js";
import "./useMountedRef-1omUd-IV.js";
import { t as getRepoHeaderCreateState } from "./repo-header-create-state-CAuZRBON.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = /* @__PURE__ */ __toESM(require_jsx_runtime());
var FOLDER_PROJECT_PREFIX = "folder-workspace:";
function AgentMapProjectContextMenu({ request, onOpenChange }) {
	const triggerRef = (0, import_react.useRef)(null);
	const repos = useAppStore((state) => state.repos);
	const projectGroups = useAppStore((state) => state.projectGroups);
	const target = (0, import_react.useMemo)(() => {
		if (request.projectId.startsWith(FOLDER_PROJECT_PREFIX)) {
			const groupId = request.projectId.slice(17);
			const groups = projectGroups.filter((group) => group.id === groupId);
			return groups.length === 1 ? {
				kind: "folder",
				group: groups[0]
			} : null;
		}
		const owners = repos.filter((repo$1) => repo$1.id === request.projectId);
		return owners.length === 1 ? {
			kind: "repo",
			repo: owners[0]
		} : null;
	}, [
		projectGroups,
		repos,
		request.projectId
	]);
	const repo = target?.kind === "repo" ? target.repo : null;
	const sshStatus = useAppStore((state) => repo?.connectionId ? state.sshConnectionStates.get(repo.connectionId)?.status ?? null : null);
	const openModal = useAppStore((state) => state.openModal);
	(0, import_react.useEffect)(() => {
		if (!target) {
			onOpenChange?.(false);
			return;
		}
		triggerRef.current?.dispatchEvent(new MouseEvent("contextmenu", {
			bubbles: true,
			cancelable: true,
			clientX: request.clientX,
			clientY: request.clientY,
			button: 2
		}));
	}, [
		onOpenChange,
		request,
		target
	]);
	if (!target) return null;
	const label = target.kind === "repo" ? target.repo.displayName : target.group.name;
	const createState = target.kind === "repo" ? getRepoHeaderCreateState({
		repo: target.repo,
		label,
		sshStatus
	}) : {
		disabled: false,
		tooltip: translate("auto.components.sidebar.repo.header.create.state.62e71f2d5d", "Create workspace for {{value0}}", { value0: label }),
		ariaLabel: translate("auto.components.sidebar.repo.header.create.state.62e71f2d5d", "Create workspace for {{value0}}", { value0: label })
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "pointer-events-none absolute inset-0",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ContextMenu, {
			onOpenChange,
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ContextMenuTrigger, {
				asChild: true,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					ref: triggerRef,
					"aria-hidden": true
				})
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ContextMenuContent, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ContextMenuLabel, { children: label }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(ContextMenuItem, {
				disabled: createState.disabled,
				"aria-label": createState.ariaLabel,
				onSelect: () => {
					openModal("new-workspace-composer", target.kind === "repo" ? {
						initialRepoId: target.repo.id,
						telemetrySource: "sidebar"
					} : {
						initialProjectGroupId: target.group.id,
						telemetrySource: "sidebar"
					});
				},
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Plus, {}), createState.tooltip]
			})] })]
		})
	});
}
export { AgentMapProjectContextMenu };
