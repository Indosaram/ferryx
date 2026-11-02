// Seeded `phone-nav` state for real-browser QA of the phone push/pop motion and edge swipe back
// (no network). A minimal home with a resume row and a Session screen, both driven by the real
// usePhoneNavigation, runPhoneTransition and useEdgeSwipeBack (forced on, since a QA browser
// is neither a standalone PWA nor the Tauri shell). `window.__phoneNavQa.transitions` counts
// the view transitions that actually started.
import React, { useEffect } from "react";

import { useEdgeSwipeBack } from "../remote/home/useEdgeSwipeBack";
import { usePhoneNavigation } from "../remote/home/usePhoneNavigation";

export const PHONE_NAV_QA_STATE = "phone-nav";

if (typeof window !== "undefined" && !window.__phoneNavQa) window.__phoneNavQa = { transitions: 0 };

export const PhoneNavQaState: React.FC = () => {
  const nav = usePhoneNavigation(true);
  const sessionRef = useEdgeSwipeBack({ enabled: false, forceEnabled: true, onBack: nav.back });
  useEffect(() => {
    document.documentElement.dataset.theme = "dark";
  }, []);
  return (
    <div className="remote-app-root flex h-[100dvh] w-full flex-col overflow-hidden bg-chat-screen text-chat-foreground" data-screen={nav.screen.kind}>
      {nav.screen.kind === "home" ? (
        <div data-testid="mobile-home" data-destination={nav.destination} className="flex min-h-0 flex-1 flex-col gap-3 p-4 pt-[max(env(safe-area-inset-top),1rem)]">
          <h1 className="text-lg font-semibold">Supervisor</h1>
          <button
            type="button"
            data-testid="mobile-home-resume"
            onClick={nav.openSession}
            className="flex min-h-11 items-center justify-between rounded-lg border border-chat-border bg-chat-surface px-3 text-left"
          >
            <span>
              <span className="block text-sm font-medium">feature/mobile-chat</span>
              <span className="block text-xs text-chat-foreground-secondary">maho-workspace</span>
            </span>
            <span aria-hidden="true">&rsaquo;</span>
          </button>
        </div>
      ) : (
        <div ref={sessionRef} data-testid="phone-nav-session" className="flex min-h-0 flex-1 flex-col bg-chat-screen">
          <header className="flex h-14 shrink-0 items-center gap-2 border-b border-chat-border px-2 pt-[env(safe-area-inset-top)]">
            <button type="button" data-testid="phone-nav-back" onClick={nav.back} className="min-h-11 rounded px-3">
              Back
            </button>
            <span className="truncate text-sm font-medium">feature/mobile-chat</span>
          </header>
          <div className="flex-1 overflow-y-auto p-4 text-sm text-chat-foreground-secondary">
            Session screen. Swipe from the left edge to go back.
          </div>
        </div>
      )}
    </div>
  );
};
