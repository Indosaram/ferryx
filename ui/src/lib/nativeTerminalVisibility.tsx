import {
  createContext,
  useContext,
  useEffect,
  useState,
  type PropsWithChildren,
  type ReactElement,
} from "react";

const NativeTerminalVisibilityContext = createContext(true);
const YIELDING_SURFACE_SELECTOR = '[role="dialog"], [role="search"]';

function isYieldingSurfaceVisible(): boolean {
  return (
    typeof document !== "undefined" &&
    document.querySelector(YIELDING_SURFACE_SELECTOR) !== null
  );
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
 * any modal/dialog or overlay surface (Settings, New Tab menu, Search overlay, etc.)
 * is mounted. The semantic selector keeps this independent from dialog implementation
 * classes while the context supplies an explicit visibility override for other owners/tests.
 */
export function useNativeTerminalVisibility(): boolean {
  const ownerVisible = useContext(NativeTerminalVisibilityContext);
  const [surfaceOpen, setSurfaceOpen] = useState(isYieldingSurfaceVisible);

  useEffect(() => {
    if (typeof document === "undefined" || typeof MutationObserver === "undefined") {
      return;
    }

    const update = () => setSurfaceOpen(isYieldingSurfaceVisible());
    update();

    const observer = new MutationObserver(update);
    observer.observe(document.body, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, []);

  return ownerVisible && !surfaceOpen;
}
