import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { toast } from "@wealthfolio/ui/components/ui/use-toast";

import { useExportData } from "./use-export-data";

const adapterMocks = vi.hoisted(() => ({
  exportDataFile: vi.fn(),
  logger: { error: vi.fn() },
}));

vi.mock("@/adapters", () => adapterMocks);
vi.mock("@wealthfolio/ui/components/ui/use-toast", () => ({ toast: vi.fn() }));

const createWrapper = () => {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
};

describe("useExportData toasts", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("shows Chinese success toast after a completed export", async () => {
    adapterMocks.exportDataFile.mockResolvedValue({
      status: "saved",
      filename: "accounts.csv",
    });
    const { result } = renderHook(() => useExportData(), { wrapper: createWrapper() });

    await act(async () => {
      await result.current.exportData({ format: "CSV", data: "accounts" });
    });

    expect(toast).toHaveBeenCalledWith({
      title: "导出完成",
      description: "文件已保存。请查看您的下载位置。",
      variant: "success",
    });
  });

  it("shows Chinese empty-dataset toast with localized type name", async () => {
    adapterMocks.exportDataFile.mockResolvedValue({ status: "empty" });
    const { result } = renderHook(() => useExportData(), { wrapper: createWrapper() });

    await act(async () => {
      await result.current.exportData({ format: "CSV", data: "accounts" });
    });

    expect(toast).toHaveBeenCalledWith({
      title: "没有可导出的内容。",
      description: "当前没有可导出的账户。",
    });
  });

  it("preserves backend error details on failure", async () => {
    adapterMocks.exportDataFile.mockRejectedValue(new Error("磁盘已满"));
    const { result } = renderHook(() => useExportData(), { wrapper: createWrapper() });

    await act(async () => {
      await result.current.exportData({ format: "CSV", data: "accounts" });
    });

    expect(toast).toHaveBeenCalledWith({
      title: "导出失败。",
      description: "磁盘已满",
      variant: "destructive",
    });
  });
});
