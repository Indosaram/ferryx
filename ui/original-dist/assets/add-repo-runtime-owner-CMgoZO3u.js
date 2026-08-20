import { Cm as projectHostSetupProjectionFromRepos, bp as getRepoHostIdentity, t as useAppStore } from "./store-CgXrfmaH.js";
import { X as LOCAL_EXECUTION_HOST_ID, ct as toRuntimeExecutionHostId, lt as toSshExecutionHostId } from "./agent-status-3vUKbY6l.js";
function repoWithCapturedOwner(repo, owner) {
	const sshConnectionId = owner.sshConnectionId?.trim();
	if (sshConnectionId) return {
		...repo,
		executionHostId: toSshExecutionHostId(sshConnectionId)
	};
	if (owner.runtimeEnvironmentId !== void 0) {
		const runtimeEnvironmentId = owner.runtimeEnvironmentId?.trim();
		return {
			...repo,
			executionHostId: runtimeEnvironmentId ? toRuntimeExecutionHostId(runtimeEnvironmentId) : LOCAL_EXECUTION_HOST_ID
		};
	}
	return repo;
}
function upsertAddedRepoWithProjectHostSetup(repo, owner = {}) {
	const state = useAppStore.getState();
	const ownedRepo = repoWithCapturedOwner(repo, owner);
	const repoIdentity = getRepoHostIdentity(ownedRepo);
	const alreadyPresent = state.repos.some((entry) => getRepoHostIdentity(entry) === repoIdentity);
	const repos = alreadyPresent ? state.repos.map((entry) => getRepoHostIdentity(entry) === repoIdentity ? ownedRepo : entry) : [...state.repos, ownedRepo];
	const projection = projectHostSetupProjectionFromRepos(repos);
	useAppStore.setState({
		repos,
		projects: projection.projects,
		projectHostSetups: projection.setups
	});
	return {
		alreadyPresent,
		repo: ownedRepo
	};
}
function capturedAddRepoExecutionHostId(owner, sshConnectionId) {
	return sshConnectionId ? toSshExecutionHostId(sshConnectionId) : owner !== void 0 ? owner ? toRuntimeExecutionHostId(owner) : LOCAL_EXECUTION_HOST_ID : void 0;
}
function worktreeRefreshOptions(owner, sshConnectionId) {
	const executionHostId = capturedAddRepoExecutionHostId(owner, sshConnectionId);
	return {
		requireAuthoritative: true,
		...executionHostId ? { executionHostId } : {}
	};
}
export { upsertAddedRepoWithProjectHostSetup as n, worktreeRefreshOptions as t };
