function encodeDiffViewerModelKey(modelKey) {
	return encodeURIComponent(modelKey).replace(/~/g, "~7E").replace(/%/g, "~");
}
function getDiffViewerMonacoModelPathPrefixes(modelKey) {
	const encodedOwnerKey = encodeDiffViewerModelKey(modelKey);
	return {
		originalModelPathPrefix: `diff:original:${encodedOwnerKey}`,
		modifiedModelPathPrefix: `diff:modified:${encodedOwnerKey}`
	};
}
function getDiffViewerMonacoModelPaths({ modelKey, originalModelKey, modifiedModelKey, generationSuffix }) {
	const prefixes = getDiffViewerMonacoModelPathPrefixes(modelKey);
	const resolvedOriginalModelKey = encodeDiffViewerModelKey(originalModelKey ?? modelKey);
	const resolvedModifiedModelKey = encodeDiffViewerModelKey(modifiedModelKey ?? modelKey);
	return {
		originalModelPath: `${prefixes.originalModelPathPrefix}:${resolvedOriginalModelKey}${generationSuffix}`,
		modifiedModelPath: `${prefixes.modifiedModelPathPrefix}:${resolvedModifiedModelKey}${generationSuffix}`
	};
}
function disposeUnattachedDiffViewerMonacoModels(monacoRegistry, modelPaths) {
	disposeUnattachedMonacoModelPaths(monacoRegistry, [modelPaths.originalModelPath, modelPaths.modifiedModelPath]);
}
function disposeUnattachedMonacoModelPaths(monacoRegistry, modelPaths) {
	for (const modelPath of modelPaths) disposeUnattachedMonacoModel(monacoRegistry.editor.getModel(monacoRegistry.Uri.parse(modelPath)));
}
function disposeUnattachedMonacoModelsByPathPrefix(monacoRegistry, modelPathPrefix) {
	for (const model of monacoRegistry.editor.getModels()) {
		const uriString = model.uri.toString(true);
		const encodedUriString = model.uri.toString();
		if (uriString === modelPathPrefix || uriString.startsWith(`${modelPathPrefix}:`) || encodedUriString === modelPathPrefix || encodedUriString.startsWith(`${modelPathPrefix}:`)) disposeUnattachedMonacoModel(model);
	}
}
function disposeUnattachedMonacoModel(model) {
	if (!model || model.isAttachedToEditor()) return;
	model.dispose();
}
export { getDiffViewerMonacoModelPaths as a, getDiffViewerMonacoModelPathPrefixes as i, disposeUnattachedMonacoModelPaths as n, disposeUnattachedMonacoModelsByPathPrefix as r, disposeUnattachedDiffViewerMonacoModels as t };
