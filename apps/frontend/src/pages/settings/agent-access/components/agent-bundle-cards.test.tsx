import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { AgentBundleStatus } from "@/adapters";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  getAgentBundleStatus: vi.fn(),
  updateAgentBundle: vi.fn(),
}));
vi.mock("@/adapters", () => mocks);

import { AgentBundleCards } from "./agent-bundle-cards";

function status(supported = true): AgentBundleStatus {
  return {
    supported,
    locationSupported: supported,
    cli: {
      state: supported ? "provided_here" : "unsupported",
      path: "/Users/test/.local/bin/wealthlog",
      desired: true,
      error: null,
    },
    skill: {
      state: supported ? "provided_here" : "unsupported",
      path: "/Users/test/.agents/skills/wealthlog",
      desired: true,
      error: null,
    },
    providerApp: "/Applications/Wealthlog.app",
    currentApp: "/Applications/Wealthlog.app",
    repairApp: "/Applications/Wealthlog.app",
    version: "0.0.3",
    error: null,
    canRepair: false,
    canUseCopy: false,
    config: supported
      ? {
          cli: "/Users/test/.local/bin/wealthlog",
          appId: "local.kanelogger.wealthlog",
          ledgerId: "REPLACE_WITH_VERIFIED_LEDGER_ID",
          tokenFile: "/Users/test/.config/wealthlog-agent/token",
          timezone: "Asia/Shanghai",
          stateDir: "/Users/test/Wealthlog-Agent/.wealthlog-agent-state",
        }
      : null,
  };
}

function show() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <AgentBundleCards />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  vi.resetAllMocks();
  mocks.getAgentBundleStatus.mockResolvedValue(status());
  mocks.updateAgentBundle.mockResolvedValue(status());
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it("shows tools and offline guide without asking the service for connection state", async () => {
  const writeText = vi.fn<(text: string) => Promise<void>>().mockResolvedValue(undefined);
  vi.stubGlobal("navigator", { ...navigator, clipboard: { writeText } });
  show();
  await screen.findByText("Wealthlog CLI");
  await waitFor(() => expect(screen.getAllByText("当前副本提供")).toHaveLength(2));
  fireEvent.click(screen.getByText("接入指南（离线）"));
  expect(screen.getByText(/Python 3.9/)).toBeInTheDocument();
  fireEvent.click(screen.getByText("复制配置（不含令牌）"));
  await waitFor(() => expect(writeText).toHaveBeenCalledOnce());
  expect(JSON.parse(writeText.mock.calls[0][0])).toEqual(status().config);
  expect(writeText.mock.calls[0][0]).not.toContain("wfp_");
});

it("web mode hides every install, remove, repair and switch action", async () => {
  mocks.getAgentBundleStatus.mockResolvedValue(status(false));
  show();
  await waitFor(() => expect(screen.getAllByText("当前环境不支持")).toHaveLength(2));
  for (const name of ["重新安装", "移除", "修复安装记录", "使用此副本", "恢复管理", "保持移除"]) {
    expect(screen.queryByRole("button", { name })).not.toBeInTheDocument();
  }
  fireEvent.click(screen.getByText("接入指南（离线）"));
  expect(screen.getByText("连接 Wealthlog 与 Agent")).toBeInTheDocument();
  expect(mocks.updateAgentBundle).not.toHaveBeenCalled();
});

it("keeps the offline configuration available when clipboard access is unavailable", async () => {
  vi.stubGlobal("navigator", { ...navigator, clipboard: undefined });
  show();
  await waitFor(() => expect(screen.getAllByText("当前副本提供")).toHaveLength(2));
  fireEvent.click(screen.getByText("接入指南（离线）"));
  fireEvent.click(screen.getByText("复制配置（不含令牌）"));
  await screen.findByText("复制失败，请手工选择配置");
  expect(screen.getByText(/REPLACE_WITH_VERIFIED_LEDGER_ID/)).toBeInTheDocument();
});

it("removes, refreshes, reinstalls and previews record repair with source and endpoints", async () => {
  show();
  await waitFor(() => expect(screen.getAllByText("当前副本提供")).toHaveLength(2));
  const removed = status();
  removed.cli = { ...removed.cli, state: "removed", desired: false };
  mocks.getAgentBundleStatus.mockResolvedValue(removed);
  fireEvent.click(screen.getAllByRole("button", { name: "移除" })[0]);
  await waitFor(() => expect(screen.getByText("已移除")).toBeInTheDocument());
  expect(mocks.updateAgentBundle).toHaveBeenCalledWith("remove", "cli");
  mocks.getAgentBundleStatus.mockResolvedValue(status());
  fireEvent.click(screen.getAllByRole("button", { name: "重新安装" })[0]);
  await waitFor(() => expect(screen.queryByText("已移除")).not.toBeInTheDocument());
  expect(mocks.updateAgentBundle).toHaveBeenCalledWith("reinstall", "cli");
  const damaged = status();
  damaged.canRepair = true;
  damaged.skill.state = "needs_repair";
  mocks.getAgentBundleStatus.mockResolvedValue(damaged);
  vi.spyOn(document, "hasFocus").mockReturnValue(true);
  fireEvent.focus(window);
  await screen.findByRole("button", { name: "修复安装记录" });
  fireEvent.click(screen.getByRole("button", { name: "修复安装记录" }));
  expect(screen.getByText(/将使用的应用/)).toHaveTextContent("/Applications/Wealthlog.app");
  fireEvent.click(screen.getByRole("button", { name: "确认修复" }));
  await waitFor(() => expect(mocks.updateAgentBundle).toHaveBeenCalledWith("repair", undefined));
});

it("hides repair and copy actions when healthy and shows Chinese failure guidance", async () => {
  const value = status();
  value.cli.state = "failed";
  value.cli.error = "Cannot publish: permission denied";
  mocks.getAgentBundleStatus.mockResolvedValue(value);
  show();
  await screen.findByText("失败");
  expect(screen.queryByRole("button", { name: "修复安装记录" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "使用此副本" })).not.toBeInTheDocument();
  expect(screen.getByRole("alert")).toHaveTextContent("工具操作未完成");
  expect(screen.getByText("查看错误详情")).toBeInTheDocument();
});

it("polls every two seconds only while visible and focused and stops on unmount", async () => {
  const focus = vi.spyOn(document, "hasFocus").mockReturnValue(true);
  const visibility = vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
  const view = show();
  await waitFor(() => expect(screen.getAllByText("当前副本提供")).toHaveLength(2));
  vi.useFakeTimers();
  // Re-arm the interval under the fake clock using a remount.
  view.unmount();
  const mounted = show();
  await act(() => vi.advanceTimersByTimeAsync(0));
  mocks.getAgentBundleStatus.mockClear();
  const next = status();
  next.skill.state = "pending_validation";
  mocks.getAgentBundleStatus.mockResolvedValue(next);
  await act(() => vi.advanceTimersByTimeAsync(2000));
  expect(mocks.getAgentBundleStatus).toHaveBeenCalledOnce();
  await act(() => vi.advanceTimersByTimeAsync(1));
  expect(screen.getByText("待校验")).toBeInTheDocument();
  focus.mockReturnValue(false);
  await act(() => vi.advanceTimersByTimeAsync(4000));
  expect(mocks.getAgentBundleStatus).toHaveBeenCalledOnce();
  focus.mockReturnValue(true);
  visibility.mockReturnValue("hidden");
  await act(() => vi.advanceTimersByTimeAsync(4000));
  expect(mocks.getAgentBundleStatus).toHaveBeenCalledOnce();
  mounted.unmount();
  await act(() => vi.advanceTimersByTimeAsync(4000));
  expect(mocks.getAgentBundleStatus).toHaveBeenCalledOnce();
});
