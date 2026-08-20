const fileNameCollator = new Intl.Collator("en", { numeric: true });
function compareFileNames(a, b) {
	const primary = fileNameCollator.compare(a, b);
	if (primary !== 0) return primary;
	return a < b ? -1 : a > b ? 1 : 0;
}
function sortDirEntries(entries) {
	return entries.sort((a, b) => {
		if (a.isDirectory !== b.isDirectory) return a.isDirectory ? -1 : 1;
		return compareFileNames(a.name, b.name);
	});
}
export { sortDirEntries as n, compareFileNames as t };
