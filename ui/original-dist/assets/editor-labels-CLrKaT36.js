import { Ec as basename } from "./store-CgXrfmaH.js";
function getBaseLabel(file, variant) {
	switch (variant) {
		case "fullPath": return file.filePath;
		case "relativePath": return file.relativePath;
		case "fileName": return basename(file.relativePath);
	}
}
var DIFF_SOURCE_LABELS = {
	staged: "staged diff",
	unstaged: "diff",
	branch: "branch diff",
	commit: "commit diff"
};
function getEditorDisplayLabel(file, variant = "fileName") {
	if (file.mode === "conflict-review") return "Conflict Review";
	if (file.mode === "check-details") return file.checkRunDetails?.check.name ?? getBaseLabel(file, variant);
	if (file.mode === "markdown-preview") return `${getBaseLabel(file, variant)} (preview)`;
	if (file.mode !== "diff") return getBaseLabel(file, variant);
	const source = file.diffSource;
	if (source === "combined-all") return "All Changes";
	if (source === "combined-uncommitted") return file.combinedAreaFilter ? getBaseLabel(file, variant) : "Uncommitted Changes";
	if (source === "combined-branch") return `Branch Changes (${file.branchCompare?.baseRef ?? "base"})`;
	if (source === "combined-commit") return file.commitCompare?.subject ? `Commit ${file.commitCompare.compareRef}: ${file.commitCompare.subject}` : `Commit ${file.commitCompare?.compareRef ?? "diff"}`;
	return `${getBaseLabel(file, variant)} (${(source && DIFF_SOURCE_LABELS[source]) ?? "diff"})`;
}
export { getEditorDisplayLabel as t };
