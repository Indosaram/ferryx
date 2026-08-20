function compareWorktreeDisplayName(a, b) {
	return (a.displayName ?? "").localeCompare(b.displayName ?? "");
}
export { compareWorktreeDisplayName as t };
