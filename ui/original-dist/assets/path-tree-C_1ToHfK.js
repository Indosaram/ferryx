function splitPathSegments(path) {
	return path.split(/[\\/]+/).filter(Boolean);
}
export { splitPathSegments as t };
