import { r as activateAndRevealWorktree } from "./worktree-activation-BDsaiyMf.js";
import { $a as isClipboardTextByteLengthOverLimit, Oa as focusTerminalTabSurface, Ou as getRuntimeEnvironmentIdForWorktree, So as resolveUnifiedTabLabel, Tc as resolveWorktreeDisplayName, t as useAppStore, xo as resolveTerminalTabTitle, zm as ORCA_BROWSER_BLANK_URL } from "./store-CgXrfmaH.js";
import { d as isWebRuntimeSessionActive, t as activateWebRuntimeSessionTab } from "./web-runtime-session-CN2syA39.js";
import { t as compareBaseSensitivityLocaleText } from "./locale-text-collators-CA-Yns-8.js";
import { t as getEditorDisplayLabel } from "./editor-labels-CLrKaT36.js";
const BROWSER_PALETTE_QUERY_MAX_BYTES = 2 * 1024;
function isBrowserPaletteQueryTooLarge(query, maxBytes = BROWSER_PALETTE_QUERY_MAX_BYTES) {
	return isClipboardTextByteLengthOverLimit(query, maxBytes);
}
function compareText$2(a, b) {
	return compareBaseSensitivityLocaleText(a, b);
}
function isBlankBrowserUrl(url) {
	return url === "about:blank" || url === "data:text/html,";
}
function formatBrowserPaletteUrl(url) {
	if (isBlankBrowserUrl(url)) return "New Tab";
	try {
		const parsed = new URL(url);
		return `${parsed.host}${parsed.pathname === "/" ? "" : parsed.pathname}${parsed.search}${parsed.hash}`;
	} catch {
		return url;
	}
}
function findRange$2(text, query) {
	if (!query) return null;
	const start = text.toLowerCase().indexOf(query);
	if (start === -1) return null;
	return {
		start,
		end: start + query.length
	};
}
function compareEmptyQueryResults$2(a, b) {
	if (a.isCurrentPage !== b.isCurrentPage) return a.isCurrentPage ? -1 : 1;
	if (a.isCurrentWorktree !== b.isCurrentWorktree) return a.isCurrentWorktree ? -1 : 1;
	if (a.score !== b.score) return a.score - b.score;
	const secondaryCmp = compareText$2(a.secondaryText, b.secondaryText);
	if (secondaryCmp !== 0) return secondaryCmp;
	return compareText$2(a.title, b.title);
}
function scoreBrowserPageMatch({ fieldWeight, matchIndex, entry }) {
	let score = fieldWeight + matchIndex + entry.worktreeSortIndex * 100;
	if (entry.isCurrentPage) score -= 40;
	else if (entry.isCurrentWorktree) score -= 10;
	return score;
}
function searchBrowserPages(entries, query) {
	if (isBrowserPaletteQueryTooLarge(query)) return [];
	const trimmedQuery = query.trim().toLowerCase();
	const results = [];
	for (const entry of entries) {
		const formattedUrl = formatBrowserPaletteUrl(entry.page.url);
		const title = entry.page.title || formattedUrl;
		const fallbackSecondaryText = formattedUrl;
		const worktreeName = resolveWorktreeDisplayName(entry.worktree);
		const baseResult = {
			pageId: entry.page.id,
			workspaceId: entry.workspace.id,
			worktreeId: entry.worktree.id,
			title,
			workspaceLabel: entry.workspace.label ?? null,
			repoName: entry.repoName,
			worktreeName,
			isCurrentPage: entry.isCurrentPage,
			isCurrentWorktree: entry.isCurrentWorktree
		};
		if (!trimmedQuery) {
			results.push({
				...baseResult,
				secondaryText: fallbackSecondaryText,
				workspaceRange: null,
				titleRange: null,
				secondaryRange: null,
				repoRange: null,
				worktreeRange: null,
				score: entry.isCurrentPage ? -2 : entry.isCurrentWorktree ? -1 : entry.worktreeSortIndex * 100
			});
			continue;
		}
		const titleRange = findRange$2(title, trimmedQuery);
		if (titleRange) {
			results.push({
				...baseResult,
				secondaryText: fallbackSecondaryText,
				workspaceRange: null,
				titleRange,
				secondaryRange: null,
				repoRange: null,
				worktreeRange: null,
				score: scoreBrowserPageMatch({
					fieldWeight: 0,
					matchIndex: titleRange.start,
					entry
				})
			});
			continue;
		}
		const formattedUrlRange = findRange$2(formattedUrl, trimmedQuery);
		if (formattedUrlRange) {
			results.push({
				...baseResult,
				secondaryText: formattedUrl,
				workspaceRange: null,
				titleRange: null,
				secondaryRange: formattedUrlRange,
				repoRange: null,
				worktreeRange: null,
				score: scoreBrowserPageMatch({
					fieldWeight: 20,
					matchIndex: formattedUrlRange.start,
					entry
				})
			});
			continue;
		}
		const rawUrlRange = findRange$2(entry.page.url, trimmedQuery);
		if (rawUrlRange) {
			results.push({
				...baseResult,
				secondaryText: entry.page.url,
				workspaceRange: null,
				titleRange: null,
				secondaryRange: rawUrlRange,
				repoRange: null,
				worktreeRange: null,
				score: scoreBrowserPageMatch({
					fieldWeight: 24,
					matchIndex: rawUrlRange.start,
					entry
				})
			});
			continue;
		}
		const workspaceRange = findRange$2(entry.workspace.label ?? "", trimmedQuery);
		if (workspaceRange) {
			results.push({
				...baseResult,
				secondaryText: fallbackSecondaryText,
				workspaceRange,
				titleRange: null,
				secondaryRange: null,
				repoRange: null,
				worktreeRange: null,
				score: scoreBrowserPageMatch({
					fieldWeight: 32,
					matchIndex: workspaceRange.start,
					entry
				})
			});
			continue;
		}
		const worktreeRange = findRange$2(worktreeName, trimmedQuery);
		if (worktreeRange) {
			results.push({
				...baseResult,
				secondaryText: fallbackSecondaryText,
				workspaceRange: null,
				titleRange: null,
				secondaryRange: null,
				repoRange: null,
				worktreeRange,
				score: scoreBrowserPageMatch({
					fieldWeight: 40,
					matchIndex: worktreeRange.start,
					entry
				})
			});
			continue;
		}
		const repoRange = findRange$2(entry.repoName, trimmedQuery);
		if (repoRange) results.push({
			...baseResult,
			secondaryText: fallbackSecondaryText,
			workspaceRange: null,
			titleRange: null,
			secondaryRange: null,
			repoRange,
			worktreeRange: null,
			score: scoreBrowserPageMatch({
				fieldWeight: 60,
				matchIndex: repoRange.start,
				entry
			})
		});
	}
	return results.sort((a, b) => {
		if (!trimmedQuery) return compareEmptyQueryResults$2(a, b);
		if (a.score !== b.score) return a.score - b.score;
		return compareEmptyQueryResults$2(a, b);
	});
}
function activateBrowserPagePaletteResult({ executionHostId, pageId, workspaceId, worktreeId }) {
	const initialState = useAppStore.getState();
	const page = (initialState.browserPagesByWorkspace[workspaceId] ?? []).find((candidate) => candidate.id === pageId);
	const workspace = (initialState.browserTabsByWorktree[worktreeId] ?? []).find((candidate) => candidate.id === workspaceId);
	const worktree = initialState.getKnownWorktreeById(worktreeId, executionHostId);
	if (!worktree) return {
		status: "failed",
		reason: "missing-worktree"
	};
	if (!page || !workspace) return {
		status: "failed",
		reason: "missing-page"
	};
	const focusTarget = isBlankBrowserUrl(page.url) ? "address-bar" : "webview";
	const targetHostId = executionHostId ?? worktree.hostId;
	if (!activateAndRevealWorktree(worktree.id, targetHostId ? { executionHostId: targetHostId } : {})) return {
		status: "failed",
		reason: "missing-worktree"
	};
	const state = useAppStore.getState();
	state.setActiveBrowserTab(workspace.id);
	state.setActiveBrowserPage(workspace.id, pageId);
	return {
		status: "activated",
		pageId,
		focusTarget
	};
}
function activateSimulatorTabPaletteResult({ executionHostId, tabId, worktreeId }) {
	const initialState = useAppStore.getState();
	const tab = (initialState.unifiedTabsByWorktree[worktreeId] ?? []).find((candidate) => candidate.id === tabId && candidate.contentType === "simulator");
	if (!tab) return {
		status: "failed",
		reason: "missing-tab"
	};
	const worktree = initialState.getKnownWorktreeById(worktreeId, executionHostId);
	if (!worktree) return {
		status: "failed",
		reason: "missing-worktree"
	};
	const targetHostId = executionHostId ?? worktree.hostId;
	if (!activateAndRevealWorktree(worktree.id, targetHostId ? { executionHostId: targetHostId } : {})) return {
		status: "failed",
		reason: "missing-worktree"
	};
	const state = useAppStore.getState();
	state.focusGroup(worktreeId, tab.groupId);
	state.activateTab(tab.id);
	state.setActiveTab(tab.id);
	state.setActiveTabType("simulator");
	return {
		status: "activated",
		tabId: tab.id
	};
}
function validateTarget(state, result) {
	if (!state.getKnownWorktreeById(result.worktreeId, result.executionHostId)) return "missing-worktree";
	if (!(state.groupsByWorktree[result.worktreeId] ?? []).find((candidate) => candidate.id === result.groupId)) return "missing-group";
	if (!(state.unifiedTabsByWorktree[result.worktreeId] ?? []).find((candidate) => candidate.id === result.tabId && candidate.entityId === result.entityId && candidate.groupId === result.groupId && candidate.worktreeId === result.worktreeId && candidate.contentType === result.contentType)) return "missing-tab";
	if (result.contentType !== "terminal" && !state.openFiles.some((file) => file.id === result.entityId && file.worktreeId === result.worktreeId)) return "missing-file";
	return null;
}
function activateWorkspaceTabPaletteResult(result) {
	const initialState = useAppStore.getState();
	const initialFailure = validateTarget(initialState, result);
	if (initialFailure) return {
		status: "failed",
		reason: initialFailure
	};
	const executionHostId = result.executionHostId ?? initialState.getKnownWorktreeById(result.worktreeId)?.hostId;
	if (!(executionHostId ? activateAndRevealWorktree(result.worktreeId, { executionHostId }) : activateAndRevealWorktree(result.worktreeId))) return {
		status: "failed",
		reason: "missing-worktree"
	};
	const state = useAppStore.getState();
	const finalFailure = validateTarget(state, result);
	if (finalFailure) return {
		status: "failed",
		reason: finalFailure
	};
	const runtimeEnvironmentId = getRuntimeEnvironmentIdForWorktree(state, result.worktreeId);
	state.focusGroup(result.worktreeId, result.groupId);
	state.activateTab(result.tabId);
	if (result.contentType === "terminal") {
		if (isWebRuntimeSessionActive(runtimeEnvironmentId)) activateWebRuntimeSessionTab({
			worktreeId: result.worktreeId,
			tabId: result.entityId,
			environmentId: runtimeEnvironmentId
		});
		state.setActiveTab(result.entityId);
		state.setActiveTabType("terminal");
		focusTerminalTabSurface(result.entityId);
		return { status: "activated" };
	}
	state.setActiveFile(result.entityId);
	state.setActiveTabType("editor");
	return { status: "activated" };
}
function buildSearchableBrowserPages({ worktrees, repoMap, worktreeOrder, browserTabsByWorktree, browserPagesByWorkspace, activeBrowserTabId, activeWorktreeId, activeTabType }) {
	const entries = [];
	for (const worktree of worktrees) {
		const repoName = repoMap.get(worktree.repoId)?.displayName ?? "";
		const worktreeSortIndex = worktreeOrder.get(worktree.id) ?? Number.MAX_SAFE_INTEGER;
		for (const workspace of browserTabsByWorktree[worktree.id] ?? []) for (const page of browserPagesByWorkspace[workspace.id] ?? []) entries.push({
			page,
			workspace,
			worktree,
			repoName,
			worktreeSortIndex,
			isCurrentPage: activeTabType === "browser" && workspace.id === activeBrowserTabId && workspace.activePageId === page.id,
			isCurrentWorktree: activeWorktreeId === worktree.id
		});
	}
	return entries;
}
function selectPaletteTypeAliasMatch(aliases, lowercasedQuery) {
	if (!lowercasedQuery) return null;
	let best = null;
	for (const alias of aliases) {
		const start = alias.toLowerCase().indexOf(lowercasedQuery);
		if (start === -1) continue;
		if (!best || start < best.range.start) best = {
			text: alias,
			range: {
				start,
				end: start + lowercasedQuery.length
			}
		};
	}
	return best;
}
const SIMULATOR_PALETTE_QUERY_MAX_BYTES = 2 * 1024;
var SIMULATOR_TYPE_SEARCH_ALIASES = [
	"mobile emulator tab",
	"mobile emulator",
	"ios simulator",
	"emulator"
];
function isSimulatorPaletteQueryTooLarge(query, maxBytes = SIMULATOR_PALETTE_QUERY_MAX_BYTES) {
	return isClipboardTextByteLengthOverLimit(query, maxBytes);
}
function compareText$1(a, b) {
	return compareBaseSensitivityLocaleText(a, b);
}
function findRange$1(text, query) {
	if (!query) return null;
	const start = text.toLowerCase().indexOf(query);
	if (start === -1) return null;
	return {
		start,
		end: start + query.length
	};
}
function compareEmptyQueryResults$1(a, b) {
	if (a.isCurrentTab !== b.isCurrentTab) return a.isCurrentTab ? -1 : 1;
	if (a.isCurrentWorktree !== b.isCurrentWorktree) return a.isCurrentWorktree ? -1 : 1;
	if (a.score !== b.score) return a.score - b.score;
	const worktreeCmp = compareText$1(a.worktreeName, b.worktreeName);
	if (worktreeCmp !== 0) return worktreeCmp;
	return compareText$1(a.title, b.title);
}
function scoreSimulatorTabMatch({ fieldWeight, matchIndex, entry }) {
	let score = fieldWeight + matchIndex + entry.worktreeSortIndex * 100;
	if (entry.isCurrentTab) score -= 40;
	else if (entry.isCurrentWorktree) score -= 10;
	return score;
}
function getActiveUnifiedTabId$1({ worktreeId, activeWorktreeId, activeTabType, activeGroupIdByWorktree, groupsByWorktree }) {
	if (activeWorktreeId !== worktreeId || activeTabType !== "simulator") return null;
	const activeGroupId = activeGroupIdByWorktree[worktreeId];
	return (activeGroupId ? (groupsByWorktree[worktreeId] ?? []).find((group) => group.id === activeGroupId) : void 0)?.activeTabId ?? null;
}
function buildSearchableSimulatorTabs({ worktrees, repoMap, worktreeOrder, unifiedTabsByWorktree, activeGroupIdByWorktree, groupsByWorktree, activeWorktreeId, activeTabType }) {
	const entries = [];
	for (const worktree of worktrees) {
		const repoName = repoMap.get(worktree.repoId)?.displayName ?? "";
		const worktreeSortIndex = worktreeOrder.get(worktree.id) ?? Number.MAX_SAFE_INTEGER;
		const activeUnifiedTabId = getActiveUnifiedTabId$1({
			worktreeId: worktree.id,
			activeWorktreeId,
			activeTabType,
			activeGroupIdByWorktree,
			groupsByWorktree
		});
		const tabs = unifiedTabsByWorktree[worktree.id] ?? [];
		for (const tab of tabs) {
			if (tab.contentType !== "simulator") continue;
			entries.push({
				tab,
				worktree,
				repoName,
				worktreeSortIndex,
				isCurrentTab: activeUnifiedTabId === tab.id,
				isCurrentWorktree: activeWorktreeId === worktree.id
			});
		}
	}
	return entries;
}
function searchSimulatorTabs(entries, query) {
	if (isSimulatorPaletteQueryTooLarge(query)) return [];
	const trimmedQuery = query.trim().toLowerCase();
	const results = [];
	for (const entry of entries) {
		const title = entry.tab.label || "Mobile Emulator";
		const secondaryText = "";
		const worktreeName = resolveWorktreeDisplayName(entry.worktree);
		const baseResult = {
			executionHostId: entry.worktree.hostId,
			tabId: entry.tab.id,
			worktreeId: entry.worktree.id,
			groupId: entry.tab.groupId,
			title,
			secondaryText,
			repoName: entry.repoName,
			worktreeName,
			isCurrentTab: entry.isCurrentTab,
			isCurrentWorktree: entry.isCurrentWorktree
		};
		if (!trimmedQuery) {
			results.push({
				...baseResult,
				titleRange: null,
				secondaryRange: null,
				repoRange: null,
				worktreeRange: null,
				score: entry.isCurrentTab ? -2 : entry.isCurrentWorktree ? -1 : entry.worktreeSortIndex * 100
			});
			continue;
		}
		const titleRange = findRange$1(title, trimmedQuery);
		if (titleRange) {
			results.push({
				...baseResult,
				titleRange,
				secondaryRange: null,
				repoRange: null,
				worktreeRange: null,
				score: scoreSimulatorTabMatch({
					fieldWeight: 0,
					matchIndex: titleRange.start,
					entry
				})
			});
			continue;
		}
		const typeAliasHit = selectPaletteTypeAliasMatch(SIMULATOR_TYPE_SEARCH_ALIASES, trimmedQuery);
		if (typeAliasHit) {
			results.push({
				...baseResult,
				titleRange: null,
				secondaryRange: null,
				repoRange: null,
				worktreeRange: null,
				typeAliasMatch: typeAliasHit,
				score: scoreSimulatorTabMatch({
					fieldWeight: 20,
					matchIndex: typeAliasHit.range.start,
					entry
				})
			});
			continue;
		}
		const worktreeRange = findRange$1(worktreeName, trimmedQuery);
		if (worktreeRange) {
			results.push({
				...baseResult,
				titleRange: null,
				secondaryRange: null,
				repoRange: null,
				worktreeRange,
				score: scoreSimulatorTabMatch({
					fieldWeight: 40,
					matchIndex: worktreeRange.start,
					entry
				})
			});
			continue;
		}
		const repoRange = findRange$1(entry.repoName, trimmedQuery);
		if (repoRange) results.push({
			...baseResult,
			titleRange: null,
			secondaryRange: null,
			repoRange,
			worktreeRange: null,
			score: scoreSimulatorTabMatch({
				fieldWeight: 60,
				matchIndex: repoRange.start,
				entry
			})
		});
	}
	return results.sort((a, b) => {
		if (!trimmedQuery) return compareEmptyQueryResults$1(a, b);
		if (a.score !== b.score) return a.score - b.score;
		return compareEmptyQueryResults$1(a, b);
	});
}
function normalizeText(value) {
	return value?.trim() ?? "";
}
function addText(target, value) {
	const trimmed = normalizeText(value);
	if (trimmed) target.push(trimmed);
}
function addProviderSession(target, providerSession) {
	if (!providerSession) return;
	addText(target, providerSession.key);
	addText(target, providerSession.id);
}
function getPaneKeyTabId(paneKey) {
	const separator = paneKey.indexOf(":");
	if (separator <= 0 || separator !== paneKey.lastIndexOf(":")) return null;
	return paneKey.slice(0, separator);
}
function collectLiveMetadata(entry) {
	const textParts = [];
	const snippetCandidates = [];
	addText(textParts, entry.orchestration?.displayName);
	addText(snippetCandidates, entry.orchestration?.displayName);
	addText(textParts, entry.orchestration?.taskTitle);
	addText(snippetCandidates, entry.orchestration?.taskTitle);
	addText(textParts, entry.prompt);
	addText(snippetCandidates, entry.prompt);
	addText(textParts, entry.agentType);
	addText(textParts, entry.state);
	addText(textParts, entry.terminalTitle);
	addText(snippetCandidates, entry.terminalTitle);
	addProviderSession(textParts, entry.providerSession);
	for (const historyEntry of entry.stateHistory) {
		addText(textParts, historyEntry.prompt);
		addText(snippetCandidates, historyEntry.prompt);
	}
	return {
		textParts,
		snippetCandidates
	};
}
function collectSleepingMetadata(record) {
	const textParts = [];
	const snippetCandidates = [];
	addText(textParts, record.prompt);
	addText(snippetCandidates, record.prompt);
	addText(textParts, record.agent);
	addText(textParts, record.state);
	addText(textParts, record.terminalTitle);
	addText(snippetCandidates, record.terminalTitle);
	addProviderSession(textParts, record.providerSession);
	return {
		textParts,
		snippetCandidates
	};
}
function pushToIndex(index, tabId, entry) {
	let bucket = index.get(tabId);
	if (!bucket) {
		bucket = [];
		index.set(tabId, bucket);
	}
	bucket.push(entry);
}
function buildAgentMetadataTabIndex(state) {
	const index = /* @__PURE__ */ new Map();
	const seenPaneKeys = /* @__PURE__ */ new Set();
	for (const [paneKey, entry] of Object.entries(state.agentStatusByPaneKey)) {
		const tabId = entry.tabId || getPaneKeyTabId(paneKey);
		if (!tabId) continue;
		seenPaneKeys.add(paneKey);
		pushToIndex(index, tabId, {
			paneKey,
			worktreeId: entry.worktreeId,
			metadata: {
				paneKey,
				...collectLiveMetadata(entry)
			}
		});
	}
	for (const [paneKey, retained] of Object.entries(state.retainedAgentsByPaneKey)) {
		if (seenPaneKeys.has(paneKey)) continue;
		const tabId = retained.entry.tabId ?? retained.tab.id ?? getPaneKeyTabId(paneKey);
		if (!tabId) continue;
		seenPaneKeys.add(paneKey);
		const meta = collectLiveMetadata(retained.entry);
		addText(meta.textParts, retained.tab.title);
		addText(meta.snippetCandidates, retained.tab.title);
		pushToIndex(index, tabId, {
			paneKey,
			worktreeId: retained.worktreeId,
			metadata: {
				paneKey,
				...meta
			}
		});
	}
	for (const [paneKey, record] of Object.entries(state.sleepingAgentSessionsByPaneKey)) {
		if (seenPaneKeys.has(paneKey)) continue;
		const tabId = record.tabId || getPaneKeyTabId(paneKey);
		if (!tabId) continue;
		pushToIndex(index, tabId, {
			paneKey,
			worktreeId: record.worktreeId,
			metadata: {
				paneKey,
				...collectSleepingMetadata(record)
			}
		});
	}
	return index;
}
function collectAgentMetadataFromIndex(index, terminalTabId, worktreeId) {
	const entries = index.get(terminalTabId);
	if (!entries) return [];
	return entries.filter((e) => !e.worktreeId || e.worktreeId === worktreeId).map((e) => e.metadata);
}
function compareText(a, b) {
	return compareBaseSensitivityLocaleText(a, b);
}
function findRange(text, query) {
	if (!query) return null;
	const start = text.toLowerCase().indexOf(query);
	if (start === -1) return null;
	return {
		start,
		end: start + query.length
	};
}
function compareEmptyQueryResults(a, b) {
	if (a.isCurrentTab !== b.isCurrentTab) return a.isCurrentTab ? -1 : 1;
	if (a.isCurrentWorktree !== b.isCurrentWorktree) return a.isCurrentWorktree ? -1 : 1;
	if (a.score !== b.score) return a.score - b.score;
	const worktreeCmp = compareText(a.worktreeName, b.worktreeName);
	if (worktreeCmp !== 0) return worktreeCmp;
	return compareText(a.title, b.title);
}
function scoreWorkspaceTabMatch({ fieldWeight, matchIndex, entry }) {
	let score = fieldWeight + matchIndex + entry.worktreeSortIndex * 100 + entry.groupSortIndex * 10 + entry.tabSortIndex;
	if (entry.isCurrentTab) score -= 40;
	else if (entry.isCurrentWorktree) score -= 10;
	return score;
}
function getBestAgentSnippet(entry, query) {
	for (const metadata of entry.agentMetadata) for (const snippet of metadata.snippetCandidates) {
		const range = findRange(snippet, query);
		if (range) return {
			text: snippet,
			range
		};
	}
	for (const metadata of entry.agentMetadata) for (const text of metadata.textParts) {
		const range = findRange(text, query);
		if (range) return {
			text,
			range
		};
	}
	return null;
}
function searchWorkspaceTabs(entries, query) {
	const trimmedQuery = query.trim().toLowerCase();
	const results = [];
	for (const entry of entries) {
		const worktreeName = resolveWorktreeDisplayName(entry.worktree);
		const baseResult = {
			tabId: entry.tab.id,
			entityId: entry.tab.entityId,
			worktreeId: entry.worktree.id,
			groupId: entry.tab.groupId,
			contentType: entry.tab.contentType,
			title: entry.title,
			secondaryText: entry.secondaryText,
			repoName: entry.repoName,
			worktreeName,
			isCurrentTab: entry.isCurrentTab,
			isCurrentWorktree: entry.isCurrentWorktree
		};
		if (!trimmedQuery) {
			results.push({
				...baseResult,
				titleRange: null,
				secondaryRange: null,
				repoRange: null,
				worktreeRange: null,
				score: entry.isCurrentTab ? -2 : entry.isCurrentWorktree ? -1 : entry.worktreeSortIndex * 100 + entry.groupSortIndex * 10 + entry.tabSortIndex
			});
			continue;
		}
		const titleRange = findRange(entry.titleSearchText, trimmedQuery);
		if (titleRange) {
			results.push({
				...baseResult,
				titleRange,
				secondaryRange: null,
				repoRange: null,
				worktreeRange: null,
				score: scoreWorkspaceTabMatch({
					fieldWeight: 0,
					matchIndex: titleRange.start,
					entry
				})
			});
			continue;
		}
		let secondaryMatch = null;
		for (const secondaryText of entry.secondarySearchTexts) {
			const range = findRange(secondaryText, trimmedQuery);
			if (range) {
				secondaryMatch = {
					text: secondaryText,
					range
				};
				break;
			}
		}
		if (secondaryMatch) {
			results.push({
				...baseResult,
				secondaryText: secondaryMatch.text,
				titleRange: null,
				secondaryRange: secondaryMatch.range,
				repoRange: null,
				worktreeRange: null,
				score: scoreWorkspaceTabMatch({
					fieldWeight: 20,
					matchIndex: secondaryMatch.range.start,
					entry
				})
			});
			continue;
		}
		const typeAliasHit = selectPaletteTypeAliasMatch(entry.typeSearchAliases ?? [], trimmedQuery);
		if (typeAliasHit) {
			results.push({
				...baseResult,
				titleRange: null,
				secondaryRange: null,
				repoRange: null,
				worktreeRange: null,
				typeAliasMatch: typeAliasHit,
				score: scoreWorkspaceTabMatch({
					fieldWeight: 25,
					matchIndex: typeAliasHit.range.start,
					entry
				})
			});
			continue;
		}
		const agentMatch = getBestAgentSnippet(entry, trimmedQuery);
		if (agentMatch) {
			results.push({
				...baseResult,
				secondaryText: agentMatch.text,
				titleRange: null,
				secondaryRange: agentMatch.range,
				repoRange: null,
				worktreeRange: null,
				score: scoreWorkspaceTabMatch({
					fieldWeight: 30,
					matchIndex: agentMatch.range.start,
					entry
				})
			});
			continue;
		}
		const worktreeRange = findRange(worktreeName, trimmedQuery);
		if (worktreeRange) {
			results.push({
				...baseResult,
				titleRange: null,
				secondaryRange: null,
				repoRange: null,
				worktreeRange,
				score: scoreWorkspaceTabMatch({
					fieldWeight: 40,
					matchIndex: worktreeRange.start,
					entry
				})
			});
			continue;
		}
		const repoRange = findRange(entry.repoName, trimmedQuery);
		if (repoRange) results.push({
			...baseResult,
			titleRange: null,
			secondaryRange: null,
			repoRange,
			worktreeRange: null,
			score: scoreWorkspaceTabMatch({
				fieldWeight: 60,
				matchIndex: repoRange.start,
				entry
			})
		});
	}
	return results.sort((a, b) => {
		if (!trimmedQuery) return compareEmptyQueryResults(a, b);
		if (a.score !== b.score) return a.score - b.score;
		return compareEmptyQueryResults(a, b);
	});
}
const TERMINAL_TYPE_SEARCH_ALIASES = ["terminal tab", "terminal"];
function getActiveUnifiedTabId({ worktreeId, activeWorktreeId, activeTabType, activeGroupIdByWorktree, groupsByWorktree }) {
	if (activeWorktreeId !== worktreeId) return null;
	const activeGroupId = activeGroupIdByWorktree[worktreeId];
	const activeUnifiedTabId = (activeGroupId ? (groupsByWorktree[worktreeId] ?? []).find((group) => group.id === activeGroupId) : void 0)?.activeTabId ?? null;
	return activeTabType === "terminal" || activeTabType === "editor" ? activeUnifiedTabId : null;
}
function isCurrentWorkspaceTab({ tab, activeWorktreeId, activeTabType, activeTabId, activeTabIdByWorktree, activeFileId, activeFileIdByWorktree, activeTabTypeByWorktree, activeUnifiedTabId }) {
	if (tab.worktreeId !== activeWorktreeId) return false;
	const visibleType = tab.contentType === "terminal" ? "terminal" : "editor";
	if ((activeTabTypeByWorktree[tab.worktreeId] ?? activeTabType) !== visibleType || activeUnifiedTabId !== tab.id) return false;
	if (visibleType === "terminal") return (activeTabIdByWorktree[tab.worktreeId] ?? activeTabId) === tab.entityId;
	return (activeFileIdByWorktree[tab.worktreeId] ?? activeFileId) === tab.entityId;
}
function isWorkspaceTabContentType(contentType) {
	return contentType === "terminal" || contentType === "editor" || contentType === "diff" || contentType === "conflict-review" || contentType === "check-details";
}
function buildSearchableWorkspaceTabs({ worktrees, repoMap, worktreeOrder, unifiedTabsByWorktree, tabsByWorktree, openFiles, agentStatusByPaneKey, retainedAgentsByPaneKey, sleepingAgentSessionsByPaneKey, activeGroupIdByWorktree, groupsByWorktree, activeWorktreeId, activeTabType, activeTabId, activeTabIdByWorktree, activeFileId, activeFileIdByWorktree, activeTabTypeByWorktree, generatedTitlesEnabled }) {
	const entries = [];
	const openFilesById = new Map(openFiles.map((file) => [file.id, file]));
	const agentIndex = buildAgentMetadataTabIndex({
		agentStatusByPaneKey,
		retainedAgentsByPaneKey,
		sleepingAgentSessionsByPaneKey
	});
	for (const worktree of worktrees) {
		const repoName = repoMap.get(worktree.repoId)?.displayName ?? "";
		const worktreeSortIndex = worktreeOrder.get(worktree.id) ?? Number.MAX_SAFE_INTEGER;
		const activeUnifiedTabId = getActiveUnifiedTabId({
			worktreeId: worktree.id,
			activeWorktreeId,
			activeTabType,
			activeGroupIdByWorktree,
			groupsByWorktree
		});
		const groups = groupsByWorktree[worktree.id] ?? [];
		const groupOrder = new Map(groups.map((group, index) => [group.id, index]));
		const tabOrder = /* @__PURE__ */ new Map();
		for (const group of groups) group.tabOrder.forEach((tabId, index) => tabOrder.set(tabId, index));
		const terminalTabs = new Map((tabsByWorktree[worktree.id] ?? []).map((tab) => [tab.id, tab]));
		for (const rawTab of unifiedTabsByWorktree[worktree.id] ?? []) {
			if (!isWorkspaceTabContentType(rawTab.contentType)) continue;
			const tab = rawTab;
			const isCurrentTab = isCurrentWorkspaceTab({
				tab,
				activeWorktreeId,
				activeTabType,
				activeTabId,
				activeTabIdByWorktree,
				activeFileId,
				activeFileIdByWorktree,
				activeTabTypeByWorktree,
				activeUnifiedTabId
			});
			const baseEntry = {
				tab,
				worktree,
				repoName,
				worktreeSortIndex,
				groupSortIndex: groupOrder.get(tab.groupId) ?? Number.MAX_SAFE_INTEGER,
				tabSortIndex: tabOrder.get(tab.id) ?? tab.sortOrder,
				isCurrentTab,
				isCurrentWorktree: activeWorktreeId === worktree.id
			};
			if (tab.contentType === "terminal") {
				const terminalTab = terminalTabs.get(tab.entityId);
				const terminalTitle = terminalTab ? resolveTerminalTabTitle(terminalTab, generatedTitlesEnabled, "Terminal") : "Terminal";
				const title$1 = resolveUnifiedTabLabel({
					...tab,
					customLabel: tab.customLabel ?? terminalTab?.customTitle ?? null,
					quickCommandLabel: tab.quickCommandLabel ?? terminalTab?.quickCommandLabel,
					generatedLabel: tab.generatedLabel ?? terminalTab?.generatedTitle
				}, generatedTitlesEnabled, terminalTitle);
				entries.push({
					...baseEntry,
					title: title$1,
					secondaryText: "",
					titleSearchText: title$1,
					secondarySearchTexts: [],
					typeSearchAliases: TERMINAL_TYPE_SEARCH_ALIASES,
					agentMetadata: collectAgentMetadataFromIndex(agentIndex, tab.entityId, worktree.id)
				});
				continue;
			}
			const file = openFilesById.get(tab.entityId);
			if (!file || file.worktreeId !== worktree.id) continue;
			const title = getEditorDisplayLabel(file);
			entries.push({
				...baseEntry,
				title,
				secondaryText: file.relativePath,
				titleSearchText: title,
				secondarySearchTexts: [file.relativePath, file.filePath],
				agentMetadata: []
			});
		}
	}
	return entries;
}
export { buildSearchableBrowserPages as a, activateBrowserPagePaletteResult as c, searchSimulatorTabs as i, searchBrowserPages as l, searchWorkspaceTabs as n, activateWorkspaceTabPaletteResult as o, buildSearchableSimulatorTabs as r, activateSimulatorTabPaletteResult as s, buildSearchableWorkspaceTabs as t };
