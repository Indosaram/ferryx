export function installContextMenuGuard(isDesktop: boolean): () => void {
  if (!isDesktop) return () => {};

  const handleContextMenu = (event: MouseEvent) => {
    const target = event.target as HTMLElement | null;
    const isInput = target && (
      target.tagName === "INPUT" ||
      target.tagName === "TEXTAREA" ||
      target.isContentEditable ||
      Boolean(target.closest("input, textarea, [contenteditable='true']"))
    );
    if (isInput) return; // Allow native text edit / spellcheck context menu
    if (import.meta.env.DEV && event.altKey) return; // Alt+RightClick in DEV allows Inspect Element
    event.preventDefault();
  };
  window.addEventListener("contextmenu", handleContextMenu);
  return () => window.removeEventListener("contextmenu", handleContextMenu);
}
