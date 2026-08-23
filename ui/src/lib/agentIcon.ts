import {
  Bird,
  Bot,
  Boxes,
  Braces,
  BrainCircuit,
  CircleDot,
  Code2,
  Cpu,
  Gem,
  Github,
  Hand,
  MessageCircle,
  Moon,
  MousePointer2,
  Network,
  Rocket,
  ScrollText,
  Smartphone,
  Sparkles,
  TerminalSquare,
  WandSparkles,
  Workflow,
  Zap,
  type LucideIcon,
} from "lucide-react";

export const GENERIC_AGENT_ICON: LucideIcon = Bot;

export const AGENT_ICON_BY_TYPE: Record<string, LucideIcon> = {
  antigravity: Rocket,
  omo: Workflow,
  claude: Sparkles,
  codex: Code2,
  opencode: Braces,
  omp: Boxes,
  pi: CircleDot,
  aider: WandSparkles,
  cursor: MousePointer2,
  gemini: Gem,
  grok: Zap,
  devin: BrainCircuit,
  droid: Smartphone,
  hermes: ScrollText,
  kimi: Moon,
  goose: Bird,
  cline: TerminalSquare,
  codebuff: Cpu,
  rovo: Network,
  openclaw: Hand,
  copilot: Github,
  mimo: MessageCircle,
  generic: GENERIC_AGENT_ICON,
};

export function resolveAgentIcon(agentType?: string | null): LucideIcon {
  const normalized = agentType?.trim().toLowerCase();
  return (normalized && AGENT_ICON_BY_TYPE[normalized]) || GENERIC_AGENT_ICON;
}
