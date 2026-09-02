import React, { useEffect, useState } from "react";
import {
  CircleCheckIcon,
  InfoIcon,
  Loader2Icon,
  OctagonXIcon,
  TriangleAlertIcon,
} from "lucide-react";
import { toast, Toaster as Sonner, type ToasterProps } from "sonner";

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

const Toaster = ({ ...props }: ToasterProps) => {
  const theme = useToastTheme();

  return (
    <Sonner
      theme={theme}
      position="bottom-right"
      closeButton
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
  );
};

export { Toaster, toast };
export type { ToasterProps };
