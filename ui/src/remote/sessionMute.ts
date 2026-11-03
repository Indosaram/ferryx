// Per-session push mute for the remote client (plan D13: the session still shows in the panel,
// it just never pushes). The panel's row menu calls `useSessionMute()`; RemoteApp provides the
// gateway call. The desktop GUI uses `setSessionPushMuted` from lib/tauri instead.
import { createContext, useContext } from "react";

/** Resolves true when the gateway applied the change; false means the real state is unchanged. */
export type SessionMute = (sessionRef: string, muted: boolean) => Promise<boolean>;

export const SessionMuteContext = createContext<SessionMute | null>(null);

/** Null where the client cannot mute (not paired): the menu hides the item. */
export function useSessionMute(): SessionMute | null {
  return useContext(SessionMuteContext);
}
