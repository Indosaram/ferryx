import { useCallback, useEffect, useState } from "react";
import {
  loadAnnounceLevel,
  loadToolCallVisibility,
  subscribePaneChatPreferences,
} from "./paneChatPreferences";
import type { ToolCallVisibility } from "./paneChatPreferences";
import type { AnnounceLevel } from "../remote/chat/messages/announcer";

export interface PaneChatPreferences {
  readonly toolCallVisibility: ToolCallVisibility;
  readonly announceLevel: AnnounceLevel;
}

function readPreferences(): PaneChatPreferences {
  return { toolCallVisibility: loadToolCallVisibility(), announceLevel: loadAnnounceLevel() };
}

/**
 * The pane chat preferences (t3code Settings -> Chat) as a live value: device-local storage,
 * re-read when the Settings control writes it in this window or another window writes it.
 * One hook for the pane and its owner, so the persisted setting and what a pane draws cannot
 * drift apart.
 */
export function usePaneChatPreferences(): PaneChatPreferences {
  const read = useCallback(readPreferences, []);
  const [preferences, setPreferences] = useState(read);
  useEffect(() => subscribePaneChatPreferences(() => setPreferences(read())), [read]);
  return preferences;
}
