import type { HostPlatform } from "../../lib/platform";

/**
 * Official install command per built-in agent, keyed by the catalog name in
 * `AGENT_CANDIDATES`. `windows` is set only where the Windows command differs;
 * `windows: null` means the vendor ships no native Windows installer.
 */
type AgentInstallCommand = {
  default: string;
  windows?: string | null;
};

export const AGENT_INSTALL_COMMANDS: Readonly<Record<string, AgentInstallCommand>> = {
  claude: {
    default: "curl -fsSL https://claude.ai/install.sh | bash",
    windows: "irm https://claude.ai/install.ps1 | iex",
  },
  codex: { default: "npm install -g @openai/codex" },
  opencode: {
    default: "curl -fsSL https://opencode.ai/install | bash",
    windows: "npm install -g opencode-ai",
  },
  aider: { default: "python -m pip install aider-install && aider-install" },
  "cursor-agent": {
    default: "curl https://cursor.com/install -fsS | bash",
    windows: null,
  },
  droid: {
    default: "curl -fsSL https://app.factory.ai/cli | sh",
    windows: "irm https://app.factory.ai/cli/windows | iex",
  },
  crush: { default: "npm install -g @charmland/crush" },
  copilot: { default: "npm install -g @github/copilot" },
};

/** The install command to show for `name` on `platform`, or null when there is none. */
export function getAgentInstallCommand(
  name: string,
  platform: HostPlatform,
): string | null {
  if (!Object.prototype.hasOwnProperty.call(AGENT_INSTALL_COMMANDS, name)) return null;
  const entry = AGENT_INSTALL_COMMANDS[name];
  if (platform === "windows" && entry.windows !== undefined) return entry.windows;
  return entry.default;
}
