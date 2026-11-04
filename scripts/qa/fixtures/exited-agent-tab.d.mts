/** Types for the exited-agent tab fixture. The implementation stays plain ESM for the QA runner. */

export interface ExitedAgentTabFixture {
  id: "{{tabId}}";
  label: "{{label}}";
  agentType: "{{agentType}}";
  worktreeLabel: "{{worktreeLabel}}";
  sessionId: "{{sessionId}}";
  running: false;
  providerSession: {
    key: "{{providerKey}}";
    id: "{{providerSessionId}}";
    transcriptPath: "{{transcriptPath}}";
    launcher: "{{launcher}}";
  };
}

export interface ExitedAgentTabValues {
  tabId?: string;
  label?: string;
  agentType?: string;
  worktreeLabel?: string;
  sessionId?: string;
  providerKey?: "session_id" | "conversation_id";
  providerSessionId?: string;
  transcriptPath?: string;
  launcher?: "mhc";
}

export interface ExitedAgentTabPublication {
  id?: string;
  label?: string;
  agentType?: string;
  worktreeLabel?: string;
  sessionId?: string;
  running: false;
  providerSession?: {
    key: "session_id" | "conversation_id";
    id: string;
    transcriptPath?: string;
    launcher?: "mhc";
  };
}

export declare function loadExitedAgentTabFixture(): ExitedAgentTabFixture;

export declare function fillExitedAgentTab(values?: ExitedAgentTabValues): ExitedAgentTabPublication;
