import {
  createContext,
  useContext,
  useEffect,
  useState,
  type PropsWithChildren,
  type ReactElement,
} from "react";

const NativeTerminalVisibilityContext = createContext(true);
const DIALOG_SELECTOR = '[role="dialog"]';

function isDialogSurfaceVisible(): boolean {
  return typeof document !== "undefined" && document.querySelector(DIALOG_SELECTOR) !== null;
}

export function NativeTerminalVisibilityProvider({
  visible,
  children,
}: PropsWithChildren<{ visible: boolean }>): ReactElement {
  return (
    <NativeTerminalVisibilityContext.Provider value={visible}>
      {children}
    </NativeTerminalVisibilityContext.Provider>
  );
}

/**
 * Native compositor child views live above WKWebView on macOS, so DOM z-index cannot
 * cover them. The active terminal therefore has to relinquish its native surface while
 * any modal/dialog surface (Settings, New Tab menu, etc.) is mounted. The semantic dialog
 * selector keeps this independent from dialog implementation classes while the context
 * supplies an explicit visibility override for other owners/tests.
 */
export function useNativeTerminalVisibility(): boolean {
  const ownerVisible = useContext(NativeTerminalVisibilityContext);
  const [dialogOpen, setDialogOpen] = useState(isDialogSurfaceVisible);

  useEffect(() => {
    if (typeof document === "undefined" || typeof MutationObserver === "undefined") {
      return;
    }

    const update = () => setDialogOpen(isDialogSurfaceVisible());
    update();

    const observer = new MutationObserver(update);
    observer.observe(document.body, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, []);

  return ownerVisible && !dialogOpen;
}
