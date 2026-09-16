import { attachNativeTerminalRebind } from "./terminalEvents";
import { withAgentConflictAdoption } from "./agentConflictAdoption";
import type { AgentReconnectDependencies } from "./agentReconnect";

type BaseDependencies = Omit<AgentReconnectDependencies, "attach">;

export function createAppReconnectDependencies(
  dependencies: BaseDependencies,
): AgentReconnectDependencies {
  return withAgentConflictAdoption({
    ...dependencies,
    attach: attachNativeTerminalRebind,
  });
}
