import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { HoldingType, QuoteMode } from "@/lib/constants";
import type { Holding } from "@/lib/types";

import { HoldingsTableMobile } from "./holdings-table-mobile";

vi.mock("@/components/ticker-avatar", () => ({
  TickerAvatar: () => <div data-testid="ticker-avatar" />,
}));

vi.mock("@/hooks/use-balance-privacy", () => ({
  useBalancePrivacy: () => ({ isBalanceHidden: false }),
}));

vi.mock("react-router-dom", async (importOriginal) => ({
  ...(await importOriginal<typeof import("react-router-dom")>()),
  useNavigate: () => vi.fn(),
}));

vi.mock("./holdings-mobile-filter-sheet", () => ({
  HoldingsMobileFilterSheet: () => null,
}));

const cashHolding: Holding = {
  id: "cash-usd",
  accountId: "account-1",
  holdingType: HoldingType.CASH,
  instrument: {
    id: "cash:USD",
    symbol: "USD",
    name: "Cash (USD)",
    currency: "USD",
    quoteMode: QuoteMode.MANUAL,
  },
  quantity: 250,
  localCurrency: "USD",
  baseCurrency: "USD",
  marketValue: { local: 250, base: 250 },
  weight: 0.25,
  asOfDate: "2026-08-18",
};

describe("HoldingsTableMobile", () => {
  it("keeps its controls available while holdings load", () => {
    render(
      <HoldingsTableMobile
        holdings={[]}
        isLoading
        selectedTypes={[]}
        setSelectedTypes={vi.fn()}
        accountFilter={{ type: "all" }}
        onAccountScopeChange={vi.fn()}
        accounts={[]}
        portfolios={[]}
        toolbarActions={<button type="button">打开操作</button>}
      />,
    );

    expect(screen.getByPlaceholderText("搜索...")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "打开持仓筛选" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "打开操作" })).toBeInTheDocument();
    expect(screen.queryByText("未找到持仓")).not.toBeInTheDocument();
  });

  it("shows portfolio weight below a cash balance", () => {
    render(
      <HoldingsTableMobile
        holdings={[cashHolding]}
        isLoading={false}
        selectedTypes={[]}
        setSelectedTypes={vi.fn()}
        accountFilter={{ type: "all" }}
        onAccountScopeChange={vi.fn()}
        accounts={[]}
        portfolios={[]}
        showSearch={false}
        showFilterButton={false}
      />,
    );

    expect(screen.getByText("权重 25.00%")).toBeInTheDocument();
  });

  it("suggests changing filters when closed positions are hidden", () => {
    render(
      <HoldingsTableMobile
        holdings={[]}
        isLoading={false}
        selectedTypes={[]}
        setSelectedTypes={vi.fn()}
        accountFilter={{ type: "all" }}
        onAccountScopeChange={vi.fn()}
        accounts={[]}
        portfolios={[]}
        hasHiddenPositions
      />,
    );

    expect(screen.getByText("请尝试调整您的搜索或筛选条件。")).toBeInTheDocument();
    expect(
      screen.queryByText("添加活动以在此处查看您的持仓。"),
    ).not.toBeInTheDocument();
  });
});
