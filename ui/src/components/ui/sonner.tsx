import React, { useEffect, useState } from "react";
import {
  CircleCheckIcon,
  InfoIcon,
  ListXIcon,
  Loader2Icon,
  OctagonXIcon,
  TriangleAlertIcon,
} from "lucide-react";
import { toast, Toaster as Sonner, useSonner, type ToasterProps } from "sonner";

import {
  APPEARANCE_SETTINGS_EVENT,
  type AppearanceSettingsState,
  type AppearanceTheme,
  loadAppearanceSettings,
} from "../../lib/appearanceSettings";

function mapToastTheme(theme: AppearanceTheme | string | undefined): ToasterProps["theme"] {
  if (theme === "light") return "light";
  if (theme === "system") return "system";
  return "dark";
}

export function useToastTheme(): ToasterProps["theme"] {
  const [theme, setTheme] = useState<ToasterProps["theme"]>(() => {
    return mapToastTheme(loadAppearanceSettings().theme);
  });

  useEffect(() => {
    const handleAppearance = (event: Event) => {
      const detail = (event as CustomEvent<AppearanceSettingsState>).detail;
      setTheme(mapToastTheme(detail?.theme));
    };
    const handleStorage = (event: StorageEvent) => {
      if (event.key === "ferryx.settings.appearance") {
        setTheme(mapToastTheme(loadAppearanceSettings().theme));
      }
    };
    window.addEventListener(APPEARANCE_SETTINGS_EVENT, handleAppearance);
    window.addEventListener("storage", handleStorage);
    return () => {
      window.removeEventListener(APPEARANCE_SETTINGS_EVENT, handleAppearance);
      window.removeEventListener("storage", handleStorage);
    };
  }, []);

  return theme;
}

// Mirrors sonner's viewport-offset defaults (desktop 24px, mobile 16px at <= 600px width)
// and reserves one row beside the toast stack so the clear-all pill never overlaps toasts.
const TOAST_EDGE_OFFSET_PX = 24;
const TOAST_EDGE_OFFSET_MOBILE_PX = 16;
const TOAST_CLEAR_ROW_PX = 36;
export const TOAST_PHONE_QUERY = "(max-width: 600px)";
const TOAST_CLEAR_ROW_PHONE_PX = 52;
const SAFE_TOP = "env(safe-area-inset-top, 0px)";

/**
 * Phones get toasts at the top, below the notch: the bottom of a phone screen belongs to the
 * tab bar, the composer and the keyboard, and a toast there covers the control the user needs.
 */
export function useToastOnPhone(): boolean {
  const [phone, setPhone] = useState(() =>
    typeof window !== "undefined" && typeof window.matchMedia === "function"
      ? window.matchMedia(TOAST_PHONE_QUERY).matches === true
      : false,
  );
  useEffect(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") return;
    const query = window.matchMedia(TOAST_PHONE_QUERY);
    if (typeof query.addEventListener !== "function") return;
    setPhone(query.matches === true);
    const onChange = (event: MediaQueryListEvent) => setPhone(event.matches === true);
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);
  return phone;
}

const Toaster = ({ ...props }: ToasterProps) => {
  const theme = useToastTheme();
  const phone = useToastOnPhone();
  const { toasts } = useSonner();
  const hasToasts = toasts.length > 0;

  return (
    <div data-native-terminal-yield="off" className="selectable">
      <Sonner
        theme={theme}
        position={phone ? "top-center" : "bottom-right"}
        closeButton
        offset={{
          bottom: TOAST_EDGE_OFFSET_PX + TOAST_CLEAR_ROW_PX,
          right: TOAST_EDGE_OFFSET_PX,
        }}
        mobileOffset={{
          top: `calc(${SAFE_TOP} + ${TOAST_EDGE_OFFSET_MOBILE_PX + TOAST_CLEAR_ROW_PHONE_PX}px)`,
          right: TOAST_EDGE_OFFSET_MOBILE_PX,
          left: TOAST_EDGE_OFFSET_MOBILE_PX,
        }}
        toastOptions={{ className: "font-sans text-sm", ...props.toastOptions }}
        className="toaster group"
        icons={{
          success: <CircleCheckIcon className="size-4" />,
          info: <InfoIcon className="size-4" />,
          warning: <TriangleAlertIcon className="size-4" />,
          error: <OctagonXIcon className="size-4" />,
          loading: <Loader2Icon className="size-4 animate-spin" />,
          ...props.icons,
        }}
        style={
          {
            "--normal-bg": "var(--popover)",
            "--normal-text": "var(--popover-foreground)",
            "--normal-border": "var(--border)",
            "--border-radius": "var(--radius)",
            "--width": "min(26rem, calc(100vw - 2rem))",
            ...props.style,
          } as React.CSSProperties
        }
        {...props}
      />
      {hasToasts ? (
        <button
          type="button"
          className="toast-clear-all transition hover:brightness-110 active:brightness-95"
          data-testid="toast-clear-all"
          aria-label="Dismiss all notifications"
          title="Dismiss all notifications"
          onClick={() => toast.dismiss()}
          data-placement={phone ? "phone-top" : "desktop-bottom"}
          style={{
            position: "fixed",
            ...(phone
              ? { top: `calc(${SAFE_TOP} + ${TOAST_EDGE_OFFSET_MOBILE_PX}px)`, right: TOAST_EDGE_OFFSET_MOBILE_PX, minHeight: 44 }
              : { right: TOAST_EDGE_OFFSET_PX, bottom: TOAST_EDGE_OFFSET_PX }),
            zIndex: 1000000000,
            display: "inline-flex",
            alignItems: "center",
            gap: 6,
            padding: "5px 12px",
            borderRadius: 9999,
            border: "1px solid var(--border)",
            background: "var(--popover)",
            color: "var(--popover-foreground)",
            font: "inherit",
            fontSize: 12,
            lineHeight: "16px",
            cursor: "pointer",
            boxShadow: "0 6px 16px rgb(0 0 0 / 0.28)",
          }}
        >
          <ListXIcon className="size-3.5" aria-hidden />
          Clear all
        </button>
      ) : null}
    </div>
  );
};

export { Toaster, toast };
export type { ToasterProps };
