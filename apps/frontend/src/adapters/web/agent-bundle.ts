import type { AgentBundleAction, AgentBundleStatus, AgentBundleTool } from "../agent-bundle-types";

export const getAgentBundleStatus = (): Promise<AgentBundleStatus> =>
  Promise.resolve({
    supported: false,
    locationSupported: false,
    cli: { state: "unsupported", path: "", desired: false, error: null },
    skill: { state: "unsupported", path: "", desired: false, error: null },
    providerApp: null,
    currentApp: null,
    repairApp: null,
    version: null,
    config: null,
    error: null,
    canRepair: false,
    canUseCopy: false,
  });

export const updateAgentBundle = (
  _action: AgentBundleAction,
  _tool?: AgentBundleTool,
): Promise<AgentBundleStatus> =>
  Promise.reject(new Error("当前环境不支持安装工具，请使用 macOS 桌面应用。"));
