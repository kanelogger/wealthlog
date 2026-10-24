import type { AgentBundleAction, AgentBundleStatus, AgentBundleTool } from "../agent-bundle-types";
import { invoke } from "./core";

export const getAgentBundleStatus = () => invoke<AgentBundleStatus>("agent_bundle_status");

export const updateAgentBundle = (action: AgentBundleAction, tool?: AgentBundleTool) =>
  invoke<AgentBundleStatus>("agent_bundle_action", { action, tool });
