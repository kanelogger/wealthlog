import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import type { ActionPaletteGroup } from "@/components/action-palette";
import { DashboardActions } from "./dashboard-actions";

const mocks = vi.hoisted(() => ({
  navigate: vi.fn(),
  update: vi.fn(),
  rebuild: vi.fn(),
  verify: vi.fn(),
}));
vi.mock("react-router-dom", () => ({ useNavigate: () => mocks.navigate }));
vi.mock("@/components/action-palette", () => ({
  ActionPalette: ({ groups }: { groups: ActionPaletteGroup[] }) => (
    <div>
      {groups.flatMap((group) =>
        group.items.map((item) => (
          <button key={item.label} onClick={item.onClick}>
            {item.label}
          </button>
        )),
      )}
    </div>
  ),
}));
vi.mock("@/hooks/use-calculate-portfolio", () => ({
  useUpdatePortfolioMutation: () => ({ mutate: mocks.update }),
  useRecalculatePortfolioMutation: () => ({ mutate: mocks.rebuild }),
}));
vi.mock("@/hooks/use-health", () => ({ useRunHealthChecks: () => ({ mutate: mocks.verify }) }));
vi.mock("@/features/devices-sync", () => ({ syncService: {} }));
vi.mock("@/features/devices-sync/hooks", () => ({
  useSyncStatus: () => ({ syncState: "disabled" }),
}));
vi.mock("@/features/wealthfolio-connect", () => ({ hasBrokerSync: () => false }));
vi.mock("@/features/wealthfolio-connect/hooks", () => ({
  useSyncBrokerData: () => ({ mutate: vi.fn() }),
}));
vi.mock("@/features/wealthfolio-connect/providers/wealthfolio-connect-provider", () => ({
  useWealthfolioConnect: () => ({ isEnabled: false, isConnected: false }),
}));

beforeEach(() => vi.clearAllMocks());

it("opens asset management from Net Worth and omits full-history refresh", () => {
  const addAsset = vi.fn();
  const addLiability = vi.fn();
  render(<DashboardActions onAddAsset={addAsset} onAddLiability={addLiability} />);
  fireEvent.click(screen.getByRole("button", { name: "管理" }));
  expect(mocks.navigate).toHaveBeenCalledWith("/holdings?tab=assets");
  expect(screen.queryByRole("button", { name: /刷新全部历史/ })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "添加资产" }));
  fireEvent.click(screen.getByRole("button", { name: "添加负债" }));
  expect(addAsset).toHaveBeenCalledOnce();
  expect(addLiability).toHaveBeenCalledOnce();
  fireEvent.click(screen.getByRole("button", { name: /更新价格/ }));
  fireEvent.click(screen.getByRole("button", { name: "校验数据" }));
  expect(mocks.update).toHaveBeenCalledOnce();
  expect(mocks.verify).toHaveBeenCalledOnce();
});

it("preserves the Investments menu", () => {
  render(<DashboardActions />);
  expect(screen.queryByRole("button", { name: "管理" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: /刷新全部历史/ }));
  expect(mocks.rebuild).toHaveBeenCalledOnce();
});
