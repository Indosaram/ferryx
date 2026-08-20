import { o as __toESM } from "./chunk-Dhmk_5SA.js";
import { t as require_react } from "./react-Da2TLWQy.js";
import { _r as slugByRepoId, fr as nextRepoSlugFailureRetryDelay, gr as settingsForRepoOwner, hr as repoUpstreamIdentityKey, mr as rememberRepoSlug, op as getActiveRuntimeTarget, pr as readRepoSlugCache, rp as callRuntimeRpc, t as useAppStore, ur as deleteRepoSlugCacheKey, vr as slugCacheKey, wm as githubRepoIdentityKey } from "./store-CgXrfmaH.js";
var import_react = /* @__PURE__ */ __toESM(require_react());
var slugResolutionInFlight = /* @__PURE__ */ new Map();
var slugResolutionGeneration = /* @__PURE__ */ new Map();
function invalidateSlugResolution(cacheKey) {
	slugResolutionInFlight.delete(cacheKey);
	slugResolutionGeneration.set(cacheKey, (slugResolutionGeneration.get(cacheKey) ?? 0) + 1);
}
function clearRepoSlugCacheEntry(repoId) {
	const suffix = `:${repoId}`;
	const keys = /* @__PURE__ */ new Set();
	for (const key of slugByRepoId.keys()) if (key.endsWith(suffix)) keys.add(key);
	for (const key of slugResolutionInFlight.keys()) if (key.endsWith(suffix)) keys.add(key);
	for (const key of keys) {
		deleteRepoSlugCacheKey(key);
		invalidateSlugResolution(key);
	}
}
async function resolveRepoSlug(repo, settings) {
	const cacheKey = slugCacheKey(repo.id, settings);
	const cached = readRepoSlugCache(cacheKey);
	if (cached.hit) return cached.value;
	const inFlight = slugResolutionInFlight.get(cacheKey);
	if (inFlight) return inFlight;
	const generation = slugResolutionGeneration.get(cacheKey) ?? 0;
	const resolution = (async () => {
		const commit = (value) => {
			if ((slugResolutionGeneration.get(cacheKey) ?? 0) === generation) rememberRepoSlug(cacheKey, value);
			return value;
		};
		try {
			const target = getActiveRuntimeTarget(settings);
			const result = target.kind === "environment" ? await callRuntimeRpc(target, "github.repoSlug", { repo: repo.id }, { timeoutMs: 3e4 }) : await window.api.gh.repoSlug({
				repoPath: repo.path,
				repoId: repo.id
			});
			if (!result) return commit(null);
			return commit(githubRepoIdentityKey(result));
		} catch {
			return commit(null);
		}
	})();
	slugResolutionInFlight.set(cacheKey, resolution);
	try {
		return await resolution;
	} finally {
		if (slugResolutionInFlight.get(cacheKey) === resolution) slugResolutionInFlight.delete(cacheKey);
	}
}
async function buildIndex(repos, settings) {
	const liveKeys = new Set(repos.map((r) => slugCacheKey(r.id, settingsForRepoOwner(r, settings))));
	for (const key of slugByRepoId.keys()) if (!liveKeys.has(key)) {
		deleteRepoSlugCacheKey(key);
		invalidateSlugResolution(key);
	}
	const next = /* @__PURE__ */ new Map();
	const upstreamNext = /* @__PURE__ */ new Map();
	const results = await Promise.all(repos.map(async (r) => ({
		repo: r,
		slug: await resolveRepoSlug(r, settingsForRepoOwner(r, settings))
	})));
	for (const { repo, slug } of results) {
		if (slug) next.set(slug, [...next.get(slug) ?? [], repo]);
		const upstreamKey = repoUpstreamIdentityKey(repo, slug);
		if (upstreamKey && upstreamKey !== slug) upstreamNext.set(upstreamKey, [...upstreamNext.get(upstreamKey) ?? [], repo]);
	}
	return {
		index: next,
		upstreamIndex: upstreamNext,
		retryDelayMs: nextRepoSlugFailureRetryDelay(liveKeys)
	};
}
function useRepoSlugIndex() {
	const repos = useAppStore((s) => s.repos);
	const settings = useAppStore((s) => s.settings);
	const [index, setIndex] = (0, import_react.useState)(() => /* @__PURE__ */ new Map());
	const [upstreamIndex, setUpstreamIndex] = (0, import_react.useState)(() => /* @__PURE__ */ new Map());
	const [ready, setReady] = (0, import_react.useState)(false);
	const [retryGeneration, setRetryGeneration] = (0, import_react.useState)(0);
	const [retryDelayMs, setRetryDelayMs] = (0, import_react.useState)(null);
	const generationRef = (0, import_react.useRef)(0);
	(0, import_react.useEffect)(() => {
		const gen = ++generationRef.current;
		setReady(false);
		setRetryDelayMs(null);
		buildIndex(repos, settings).then(({ index: next, upstreamIndex: nextUpstream, retryDelayMs: nextRetryDelayMs }) => {
			if (gen !== generationRef.current) return;
			setIndex(next);
			setUpstreamIndex(nextUpstream);
			setReady(true);
			setRetryDelayMs(nextRetryDelayMs);
		});
		return () => {
			generationRef.current += 1;
		};
	}, [
		repos,
		retryGeneration,
		settings
	]);
	(0, import_react.useEffect)(() => {
		if (retryDelayMs === null) return;
		const retryTimer = setTimeout(() => setRetryGeneration((value) => value + 1), retryDelayMs);
		return () => {
			clearTimeout(retryTimer);
		};
	}, [retryDelayMs]);
	return (0, import_react.useMemo)(() => {
		const lookupSlugMatches = (slug, host) => {
			const [owner, repo] = slug?.split("/") ?? [];
			if (!owner || !repo) return {
				origin: [],
				upstream: []
			};
			const key = githubRepoIdentityKey({
				owner,
				repo,
				host
			});
			return {
				origin: index.get(key) ?? [],
				upstream: upstreamIndex.get(key) ?? []
			};
		};
		return {
			lookupSlugMatches,
			lookupSlug: (slug, host) => {
				const { origin, upstream } = lookupSlugMatches(slug, host);
				return origin.length > 0 ? origin : upstream;
			},
			ready
		};
	}, [
		index,
		upstreamIndex,
		ready
	]);
}
export { useRepoSlugIndex as n, clearRepoSlugCacheEntry as t };
