function pathSeparatorFor(pathValue) {
	return pathValue.includes("\\") ? "\\" : "/";
}
function trimTrailingSeparators(pathValue) {
	const trimmed = pathValue.replace(/[\\/]+$/, "");
	if (trimmed === "" && pathValue.startsWith("/")) return "/";
	if (/^[A-Za-z]:$/.test(trimmed)) return `${trimmed}${pathSeparatorFor(pathValue)}`;
	return trimmed;
}
function joinCreateProjectPath(parentPath, childName) {
	const parent = trimTrailingSeparators(parentPath.trim());
	const child = childName.trim().replace(/^[\\/]+/, "");
	if (!parent || !child) return parent || child;
	const separator = pathSeparatorFor(parent);
	if (parent === "/" || /^[A-Za-z]:[\\/]$/.test(parent)) return `${parent}${child}`;
	return `${parent}${separator}${child}`;
}
function getDefaultCreateProjectParent(homeDir) {
	const trimmedHomeDir = trimTrailingSeparators(homeDir.trim());
	if (!trimmedHomeDir) return "";
	return joinCreateProjectPath(joinCreateProjectPath(trimmedHomeDir, "orca"), "projects");
}
function formatCreateProjectParentSummary({ parent, defaultParent, runtimeEnvironmentId, isRemoteHost, missingLocationLabel = "location not selected", missingServerLocationLabel = "host folder not selected" }) {
	const trimmedParent = parent.trim();
	if (!trimmedParent) return runtimeEnvironmentId || isRemoteHost ? missingServerLocationLabel : missingLocationLabel;
	if (defaultParent && trimmedParent === defaultParent && !runtimeEnvironmentId && !isRemoteHost) return "~/orca/projects";
	return trimmedParent;
}
export { getDefaultCreateProjectParent as n, joinCreateProjectPath as r, formatCreateProjectParentSummary as t };
