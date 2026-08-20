var pendingEditorFlushes = /* @__PURE__ */ new Map();
function registerPendingEditorFlush(fileId, flush) {
	pendingEditorFlushes.set(fileId, flush);
	return () => {
		if (pendingEditorFlushes.get(fileId) === flush) pendingEditorFlushes.delete(fileId);
	};
}
function flushPendingEditorChange(fileId) {
	pendingEditorFlushes.get(fileId)?.();
}
export { registerPendingEditorFlush as n, flushPendingEditorChange as t };
