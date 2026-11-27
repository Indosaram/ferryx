import { useCallback, useState } from "react";
import { MessageSquare } from "lucide-react";

import {
  ANNOUNCE_LEVELS,
  DEFAULT_ANNOUNCE_LEVEL,
  loadAnnounceLevel,
  loadToolCallVisibility,
  saveAnnounceLevel,
  saveToolCallVisibility,
  TOOL_CALL_VISIBILITIES,
  type ToolCallVisibility,
} from "../../lib/paneChatPreferences";
import type { AnnounceLevel } from "../../remote/chat/messages/announcer";
import { SettingRow, SettingsGroup, SettingsHeading } from "./primitives";

/**
 * Settings -> Chat: the two pane-chat preferences the transcript reads (t3code
 * docs/user/activity-log.md, "Hide tool calls" and "Screen reader announcements").
 *
 * Both are device-local, like the reference: they live under `ferryx.*` keys in this window's
 * storage, and a mounted pane re-reads them the moment one changes. The copy is the reference's
 * own wording, because the promise it makes ("approvals, questions, and errors always show") is
 * what the transcript implements.
 */

const TOOL_CALL_VISIBILITY_LABEL: Record<ToolCallVisibility, string> = {
  all: "Show all tool calls",
  "hide-successful": "Hide successful",
  "hide-all": "Hide all",
  zen: "Zen",
};

const TOOL_CALL_VISIBILITY_DESCRIPTION: Record<ToolCallVisibility, string> = {
  all: "Commands, file edits, searches, and MCP tool calls stay in the conversation.",
  "hide-successful": "A call shows while it runs and stays only if it fails.",
  "hide-all": "Every call is hidden and Thinking shows while the agent works.",
  zen: "In-between messages are hidden too: each turn shows your message and the agent's last reply.",
};

const ANNOUNCE_LEVEL_LABEL: Record<AnnounceLevel, string> = {
  "status-only": "Status only",
  "status-and-messages": "Status and messages",
  everything: "Everything",
};

const ANNOUNCE_LEVEL_DESCRIPTION: Record<AnnounceLevel, string> = {
  "status-only": "Says when a turn starts, needs your approval or input, and ends.",
  "status-and-messages": "Also reads each message the agent writes and then keeps working after.",
  everything: "Also says the step the agent is on, such as the command it is running.",
};

export function ChatSection() {
  const [toolCallVisibility, setToolCallVisibility] = useState<ToolCallVisibility>(() =>
    loadToolCallVisibility(),
  );
  const [announceLevel, setAnnounceLevel] = useState<AnnounceLevel>(() => loadAnnounceLevel());

  const changeVisibility = useCallback((next: ToolCallVisibility) => {
    setToolCallVisibility(next);
    saveToolCallVisibility(next);
  }, []);
  const changeAnnounceLevel = useCallback((next: AnnounceLevel) => {
    setAnnounceLevel(next);
    saveAnnounceLevel(next);
  }, []);

  return (
    <section aria-labelledby="settings-chat-heading">
      <SettingsHeading
        icon={<MessageSquare />}
        title="Chat"
        description="How much of an agent's work the conversation shows, and what a screen reader hears."
      />
      <h2 id="settings-chat-heading" className="sr-only">
        Chat
      </h2>

      <SettingsGroup
        title="Tool calls in chat"
        description="Applies to this browser or device only. Approvals, questions, and errors always show."
      >
        {TOOL_CALL_VISIBILITIES.map((mode) => (
          <SettingRow
            key={mode}
            label={TOOL_CALL_VISIBILITY_LABEL[mode]}
            description={TOOL_CALL_VISIBILITY_DESCRIPTION[mode]}
          >
            <input
              type="radio"
              name="pane-chat-tool-call-visibility"
              aria-label={TOOL_CALL_VISIBILITY_LABEL[mode]}
              data-testid={`pane-chat-tool-call-visibility-${mode}`}
              className="size-4"
              checked={toolCallVisibility === mode}
              onChange={() => changeVisibility(mode)}
            />
          </SettingRow>
        ))}
      </SettingsGroup>

      <SettingsGroup
        title="Screen reader announcements"
        description="What a screen reader hears while the open thread's turn runs. Applies to this browser or device only."
      >
        {ANNOUNCE_LEVELS.map((level) => (
          <SettingRow
            key={level}
            label={ANNOUNCE_LEVEL_LABEL[level]}
            description={
              level === DEFAULT_ANNOUNCE_LEVEL
                ? `${ANNOUNCE_LEVEL_DESCRIPTION[level]} (default)`
                : ANNOUNCE_LEVEL_DESCRIPTION[level]
            }
          >
            <input
              type="radio"
              name="pane-chat-announce-level"
              aria-label={ANNOUNCE_LEVEL_LABEL[level]}
              data-testid={`pane-chat-announce-level-${level}`}
              className="size-4"
              checked={announceLevel === level}
              onChange={() => changeAnnounceLevel(level)}
            />
          </SettingRow>
        ))}
      </SettingsGroup>
    </section>
  );
}
