const DASHBOARD_BUCKET_ORDER = [
	"attention",
	"working",
	"done",
	"idle"
];
const DASHBOARD_MAX_LABEL_LENGTH = 1024;
const DASHBOARD_MAX_LAUNCH_WORKTREES = 500;
const DASHBOARD_MAX_MAP_WORKSPACES = 2e3;
function dashboardCardDisplayState(card) {
	return card.dotState === "done" && !card.unseen ? "idle" : card.dotState;
}
const EMPTY_DASHBOARD_SNAPSHOT = {
	generatedAt: 0,
	cards: [],
	workspaces: [],
	filterOptions: {
		projects: [],
		workspaceStatuses: []
	},
	launchableAgentsByWorktreeId: {},
	repoIconsByRepoId: {}
};
export { EMPTY_DASHBOARD_SNAPSHOT as a, DASHBOARD_MAX_MAP_WORKSPACES as i, DASHBOARD_MAX_LABEL_LENGTH as n, dashboardCardDisplayState as o, DASHBOARD_MAX_LAUNCH_WORKTREES as r, DASHBOARD_BUCKET_ORDER as t };
