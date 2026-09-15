export type MachineFilter = "all" | "paired" | "ssh";

export type MachineProjectTarget =
  | { kind: "ssh"; hostId: string }
  | { kind: "pairedDaemon"; hostId: string; generation: string };

export type RemotePage = "machines" | "access" | "details";
export type RemoteContext = {
  page: RemotePage;
  filter: MachineFilter;
  machine?: MachineProjectTarget;
};
