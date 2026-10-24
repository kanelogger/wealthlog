import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ResetProviderHistoryDialog } from "./reset-provider-history-dialog";
import { RefreshQuotesConfirmDialog } from "./refresh-quotes-confirm-dialog";

const mocks = vi.hoisted(() => ({
  resetProviderHistory: vi.fn(),
  resetAllProviderHistory: vi.fn(),
  toast: vi.fn(),
}));
vi.mock("@/adapters", () => ({
  resetProviderHistory: mocks.resetProviderHistory,
  resetAllProviderHistory: mocks.resetAllProviderHistory,
}));
vi.mock("@wealthfolio/ui/components/ui/use-toast", () => ({
  useToast: () => ({ toast: mocks.toast }),
}));

function setup(global = false) {
  const client = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
  const onOpenChange = vi.fn();
  render(
    <QueryClientProvider client={client}>
      <ResetProviderHistoryDialog
        allAssets={global}
        assetId={global ? undefined : "asset-1"}
        assetName={global ? undefined : "Example"}
        open
        onOpenChange={onOpenChange}
      />
    </QueryClientProvider>,
  );
  return { client, onOpenChange };
}

const result = {
  assetId: "asset-1",
  source: "YAHOO",
  fromDate: "2020-01-01",
  toDate: "2026-09-17",
  insertedCount: 3,
  deletedCount: 7,
};

describe("重置数据源历史", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.resetProviderHistory.mockResolvedValue(result);
  });

  it("describes replacement and preserves prices until confirmation; cancellation sends nothing", () => {
    const { onOpenChange } = setup();
    expect(screen.getByText(/新历史可能更短|可能较短/)).toHaveTextContent(
      "手动价格和券商价格将保留",
    );
    expect(mocks.resetProviderHistory).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(mocks.resetProviderHistory).not.toHaveBeenCalled();
  });

  it("submits only the selected asset once and disables repeat submission while pending", async () => {
    let finish!: (value: typeof result) => void;
    mocks.resetProviderHistory.mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    setup();
    const confirm = screen.getByRole("button", { name: "重置数据源历史" });
    fireEvent.click(confirm);
    fireEvent.click(confirm);
    await waitFor(() => expect(mocks.resetProviderHistory).toHaveBeenCalledTimes(1));
    expect(mocks.resetProviderHistory).toHaveBeenCalledWith("asset-1");
    expect(screen.getByRole("button", { name: "正在重置…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "取消" })).toBeDisabled();
    await act(async () => {
      finish(result);
      await Promise.resolve();
    });
  });

  it("can reset the same asset again after a successful controlled close and reopen", async () => {
    const client = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    const onOpenChange = vi.fn();
    const dialog = (open: boolean) => (
      <QueryClientProvider client={client}>
        <ResetProviderHistoryDialog
          assetId="asset-1"
          assetName="Example"
          open={open}
          onOpenChange={onOpenChange}
        />
      </QueryClientProvider>
    );
    const { rerender } = render(dialog(true));
    fireEvent.click(screen.getByRole("button", { name: "重置数据源历史" }));
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    rerender(dialog(false));
    expect(
      screen.queryByRole("button", { name: "重置数据源历史" }),
    ).not.toBeInTheDocument();
    rerender(dialog(true));
    const confirm = screen.getByRole("button", { name: "重置数据源历史" });
    expect(confirm).toBeEnabled();
    fireEvent.click(confirm);
    await waitFor(() => expect(mocks.resetProviderHistory).toHaveBeenCalledTimes(2));
    expect(mocks.resetProviderHistory).toHaveBeenNthCalledWith(1, "asset-1");
    expect(mocks.resetProviderHistory).toHaveBeenNthCalledWith(2, "asset-1");
    expect(mocks.resetAllProviderHistory).not.toHaveBeenCalled();
  });

  it("reports saved prices and requested recalculation, and refreshes caches", async () => {
    const { client, onOpenChange } = setup();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    fireEvent.click(screen.getByRole("button", { name: "重置数据源历史" }));
    await waitFor(() =>
      expect(mocks.toast).toHaveBeenCalledWith(
        expect.objectContaining({
          title: "数据源历史已替换",
          description: "价格已替换。已请求重新计算投资组合。",
        }),
      ),
    );
    expect(invalidate).toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "重置数据源历史" })).toBeEnabled();
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it.each([
    "Already refreshing this asset",
    "Invalid provider history: invalid returned rows",
    "Asset or provider settings changed during fetching; history was not replaced",
    "Provider network connection failed before replacement",
  ])("shows backend rejection: %s", async (message) => {
    mocks.resetProviderHistory.mockRejectedValue(
      Object.assign(new Error(message), { outcomeUnknown: false }),
    );
    setup();
    fireEvent.click(screen.getByRole("button", { name: "重置数据源历史" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(message);
    expect(mocks.toast).not.toHaveBeenCalled();
    expect(screen.queryByText(/价格可能已被替换/)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "重置数据源历史" })).toBeEnabled();
    expect(mocks.resetProviderHistory).toHaveBeenCalledTimes(1);
  });

  it.each([
    "Request timed out",
    "Internal Server Error",
    "Reset completion could not be confirmed. Reload quotes before retrying.",
  ])("does not retry an uncertain response or claim a rollback: %s", async (message) => {
    mocks.resetProviderHistory.mockRejectedValue(
      Object.assign(new Error(message), { outcomeUnknown: true }),
    );
    setup();
    fireEvent.click(screen.getByRole("button", { name: "重置数据源历史" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "价格可能已被替换",
    );
    expect(screen.getByRole("button", { name: "重置数据源历史" })).toBeDisabled();
    expect(mocks.resetProviderHistory).toHaveBeenCalledTimes(1);
  });

  it("ordinary refresh confirms merging instead of deletion", () => {
    render(
      <RefreshQuotesConfirmDialog
        open
        onOpenChange={vi.fn()}
        onConfirm={vi.fn()}
        assetName="Example"
      />,
    );
    expect(screen.getByText(/与现有价格合并/)).toHaveTextContent("旧历史");
    expect(screen.queryByText(/delete and replace/)).not.toBeInTheDocument();
  });
});

describe("Reset all provider history", () => {
  beforeEach(() => vi.clearAllMocks());
  it("does not interpret a missing asset as authorization to reset everything", async () => {
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <ResetProviderHistoryDialog open onOpenChange={vi.fn()} />
      </QueryClientProvider>,
    );
    fireEvent.click(screen.getByRole("button", { name: "重置数据源历史" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("未选择资产。");
    expect(mocks.resetAllProviderHistory).not.toHaveBeenCalled();
    expect(mocks.resetProviderHistory).not.toHaveBeenCalled();
  });
  it("confirms the global scope and cancels without sending a request", () => {
    setup(true);
    expect(screen.getByText(/所有符合条件的资产/)).toHaveTextContent(
      "成功的替换不会回滚",
    );
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(mocks.resetAllProviderHistory).not.toHaveBeenCalled();
  });
  it("reports partial failure by asset and prevents repeating successful resets", async () => {
    mocks.resetAllProviderHistory.mockResolvedValue({
      results: [result],
      failures: [{ assetId: "failed-asset", error: "Invalid history" }],
      skipped: [{ assetId: "manual-asset", reason: "Manual prices" }],
    });
    const { onOpenChange } = setup(true);
    fireEvent.click(screen.getByRole("button", { name: "重置数据源历史" }));
    const status = await screen.findByRole("status");
    expect(status).toHaveTextContent("已替换：1。失败：1。已跳过：1。");
    expect(status).toHaveTextContent("failed-asset: Invalid history");
    expect(status).toHaveTextContent("manual-asset: Manual prices");
    expect(status).toHaveTextContent("已请求重新计算投资组合");
    expect(
      screen.queryByRole("button", { name: "重置数据源历史" }),
    ).not.toBeInTheDocument();
    expect(mocks.resetAllProviderHistory).toHaveBeenCalledTimes(1);
    expect(mocks.resetProviderHistory).not.toHaveBeenCalled();
    expect(onOpenChange).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "关闭" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
  it("never automatically retries a global reset after an interrupted response", async () => {
    mocks.resetAllProviderHistory.mockRejectedValue(
      Object.assign(new Error("Network connection lost"), { outcomeUnknown: true }),
    );
    setup(true);
    fireEvent.click(screen.getByRole("button", { name: "重置数据源历史" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "价格可能已被替换",
    );
    expect(screen.getByRole("button", { name: "重置数据源历史" })).toBeDisabled();
    expect(mocks.resetAllProviderHistory).toHaveBeenCalledTimes(1);
  });
});
