// Shared chip contract (frozen in .omo/plans/chat-t3-parity-20261125.md). Byte-identical in every lane.
export type ChipKind = "thread" | "model" | "skill" | "pr" | "file" | "terminal";
export interface ChipItem {
  readonly id: string;
  readonly kind: ChipKind;
  readonly label: string;
  readonly detail?: string;
}
export interface ChipSources {
  readonly threads?: (query: string) => Promise<readonly ChipItem[]>;
  readonly pullRequests?: (query: string) => Promise<readonly ChipItem[]>;
  readonly models?: readonly ChipItem[];
  readonly skills?: readonly ChipItem[];
}
export type PermissionMode = "supervised" | "auto-accept-edits" | "auto" | "full-access";
export interface PermissionControl {
  readonly modes: readonly PermissionMode[];
  readonly current: PermissionMode | null;
  readonly onChange: (mode: PermissionMode) => void;
}
export interface ExecutionModeControl {
  readonly modes: readonly string[];
  readonly current: string | null;
  readonly onChange: (mode: string) => void;
}
export interface AgentControl {
  readonly agents: readonly string[];
  readonly current: string | null;
  readonly onChange: (agent: string) => void;
}
export interface ThinkingControl {
  readonly levels: readonly string[];
  readonly current: string | null;
  readonly onChange: (level: string) => void;
}
export interface FastModeControl {
  readonly enabled: boolean;
  readonly onChange: (enabled: boolean) => void;
}
export interface AdditionalDirsControl {
  readonly dirs: readonly string[];
  readonly onAdd: (dir: string) => void;
  readonly onRemove: (dir: string) => void;
}
export type QueueDeliveryMode = "all" | "one-at-a-time";
export interface QueueModeControl {
  readonly steeringMode: QueueDeliveryMode;
  readonly followUpMode: QueueDeliveryMode;
  readonly onChangeSteering: (mode: QueueDeliveryMode) => void;
  readonly onChangeFollowUp: (mode: QueueDeliveryMode) => void;
}
export interface CitationInsert {
  readonly messageId: string;
  readonly text: string;
}
