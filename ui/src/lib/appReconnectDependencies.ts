import { attachNativeTerminalRebind } from "./terminalEvents";
import type { AgentReconnectDependencies } from "./agentReconnect";

type BaseDependencies = Omit<AgentReconnectDependencies, "attach">;

export function createAppReconnectDependencies(
  dependencies: BaseDependencies,
): AgentReconnectDependencies {
  return {
    ...dependencies,
    attach: attachNativeTerminalRebind,
  };
}
