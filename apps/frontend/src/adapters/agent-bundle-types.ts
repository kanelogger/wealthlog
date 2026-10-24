export type AgentBundleTool = "cli" | "skill";
export type AgentBundleAction =
  | "reinstall"
  | "remove"
  | "repair"
  | "use_copy"
  | "restore"
  | "keep_removed";

export type AgentBundleToolState =
  | "provided_here"
  | "provided_elsewhere"
  | "user_managed"
  | "removed"
  | "installing"
  | "pending_validation"
  | "needs_repair"
  | "state_conflict"
  | "conflict"
  | "failed"
  | "unsupported_location"
  | "unsupported";

export interface AgentBundleStatus {
  supported: boolean;
  locationSupported: boolean;
  cli: AgentBundleEntry;
  skill: AgentBundleEntry;
  providerApp: string | null;
  currentApp: string | null;
  repairApp: string | null;
  version: string | null;
  config: {
    cli: string;
    appId: string;
    ledgerId: string;
    tokenFile: string;
    timezone: string;
    stateDir: string;
  } | null;
  error: string | null;
  canRepair: boolean;
  canUseCopy: boolean;
}

export interface AgentBundleEntry {
  state: AgentBundleToolState;
  path: string;
  desired: boolean;
  error: string | null;
}
