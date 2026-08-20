import { Ac as normalizeRelativePath } from "./store-CgXrfmaH.js";
import { t as splitPathSegments } from "./path-tree-C_1ToHfK.js";
import { t as compareFileNames } from "./file-name-sort-EN_9pleA.js";
function compareGitStatusEntries(a, b) {
	return getConflictSortRank(a) - getConflictSortRank(b) || compareFileNames(a.path, b.path);
}
function getConflictSortRank(entry) {
	if (entry.conflictStatus === "unresolved") return 0;
	if (entry.conflictStatus === "resolved_locally") return 1;
	return 2;
}
function compareTreeEntriesByPath(a, b) {
	return compareFileNames(a.path, b.path);
}
function makeDirectoryNode(area, path, name, depth) {
	return {
		type: "directory",
		key: `dir::${area}::${path}`,
		name,
		path,
		area,
		depth,
		fileCount: 0,
		children: [],
		directoryChildren: /* @__PURE__ */ new Map()
	};
}
function finalizeDirectoryNode(node, compareEntries) {
	const directories = [];
	const files = [];
	for (const child of node.children) if (child.type === "directory") directories.push(finalizeDirectoryNode(child, compareEntries));
	else files.push(child);
	directories.sort((a, b) => compareFileNames(a.name, b.name));
	files.sort((a, b) => compareEntries(a.entry, b.entry));
	const fileCount = files.length + directories.reduce((count, directory) => count + directory.fileCount, 0);
	return {
		type: "directory",
		key: node.key,
		name: node.name,
		path: node.path,
		area: node.area,
		depth: node.depth,
		fileCount,
		children: [...directories, ...files]
	};
}
function buildSourceControlTree(area, entries, compareEntries = compareTreeEntriesByPath) {
	const root = makeDirectoryNode(area, "", "", -1);
	for (const entry of entries) {
		const normalizedPath = normalizeRelativePath(entry.path);
		const segments = splitPathSegments(normalizedPath);
		if (segments.length === 0) continue;
		let parent = root;
		for (let index = 0; index < segments.length - 1; index += 1) {
			const name = segments[index];
			const path = segments.slice(0, index + 1).join("/");
			let dir = parent.directoryChildren.get(name);
			if (!dir) {
				dir = makeDirectoryNode(area, path, name, index);
				parent.directoryChildren.set(name, dir);
				parent.children.push(dir);
			}
			parent = dir;
		}
		const fileName = segments.at(-1);
		parent.children.push({
			type: "file",
			key: `${area}::${entry.path}`,
			name: fileName,
			path: normalizedPath,
			entry,
			area,
			depth: segments.length - 1
		});
	}
	return finalizeDirectoryNode(root, compareEntries).children;
}
function buildGitStatusSourceControlTree(area, entries) {
	return buildSourceControlTree(area, entries, compareGitStatusEntries);
}
function flattenSourceControlTree(nodes, collapsedDirectoryKeys) {
	const result = [];
	const visit = (node) => {
		result.push(node);
		if (node.type === "directory" && !collapsedDirectoryKeys.has(node.key)) for (const child of node.children) visit(child);
	};
	for (const node of nodes) visit(node);
	return result;
}
function compactSourceControlTree(nodes) {
	const compactNode = (node, depth) => {
		if (node.type === "file") return {
			...node,
			depth
		};
		const names = [node.name];
		let compacted = node;
		while (compacted.children.length === 1 && compacted.children[0]?.type === "directory") {
			compacted = compacted.children[0];
			names.push(compacted.name);
		}
		return {
			...compacted,
			name: names.join("/"),
			depth,
			children: compacted.children.map((child) => compactNode(child, depth + 1))
		};
	};
	return nodes.map((node) => compactNode(node, 0));
}
function namespaceSourceControlTreeDirectoryKeys(nodes, namespace) {
	const namespaceNode = (node) => {
		if (node.type === "file") return node;
		return {
			...node,
			key: `dir::${namespace}::${node.path}`,
			children: node.children.map(namespaceNode)
		};
	};
	return nodes.map(namespaceNode);
}
function applyGitStatusEntryAreasToSourceControlTree(nodes) {
	const applyEntryArea = (node) => {
		if (node.type === "file") return {
			...node,
			key: `${node.entry.area}::${node.entry.path}`,
			area: node.entry.area
		};
		return {
			...node,
			children: node.children.map(applyEntryArea)
		};
	};
	return nodes.map(applyEntryArea);
}
function collectSourceControlTreeFileEntries(node) {
	if (node.type === "file") return [node.entry];
	const entries = [];
	const collect = (child) => {
		if (child.type === "file") {
			entries.push(child.entry);
			return;
		}
		for (const grandchild of child.children) collect(grandchild);
	};
	for (const child of node.children) collect(child);
	return entries;
}
export { compactSourceControlTree as a, compareGitStatusEntries as c, collectSourceControlTreeFileEntries as i, buildGitStatusSourceControlTree as n, flattenSourceControlTree as o, buildSourceControlTree as r, namespaceSourceControlTreeDirectoryKeys as s, applyGitStatusEntryAreasToSourceControlTree as t };
