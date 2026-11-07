// Per-session push mute for the remote client (plan D13: the session still shows in the panel,
// it just never pushes). The panel's row menu calls `useSessionMute()`; RemoteApp provides the
// gateway calls. The desktop GUI uses `setSessionPushMuted` from lib/tauri instead.
import { createContext, useContext } from "react";

/** Resolves true when the gateway applied the change; false means the real state is unchanged. */
export type SessionMute = (sessionRef: string, muted: boolean) => Promise<boolean>;

/** The gateway's persisted mute set, or null where this client cannot read it. */
export type SessionMuteState = () => Promise<ReadonlySet<string> | null>;

export type SessionMuteBinding = {
  set: SessionMute;
  read: SessionMuteState | null;
};

export const SessionMuteContext = createContext<SessionMuteBinding | null>(null);

/** Null where the client cannot mute (not paired): the menu hides the item. */
export function useSessionMute(): SessionMuteBinding | null {
  return useContext(SessionMuteContext);
}
