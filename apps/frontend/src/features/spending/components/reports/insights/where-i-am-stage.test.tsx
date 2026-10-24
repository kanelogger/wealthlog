import { render, screen } from "@testing-library/react";
import { FormattingProvider } from "@wealthfolio/ui";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";
import { WhereIAmStage } from "./where-i-am-stage";
import type { MonthlyReport } from "../../../types/report";
import { comparisonRange } from "../../../lib/reports-period";
import { spendingRangeToReportsRange } from "../../../lib/date-range-params";

vi.mock("@/hooks/use-balance-privacy", () => ({
  useBalancePrivacy: () => ({ isBalanceHidden: false }),
}));

vi.mock("../../../hooks/use-spending-settings", () => ({
  useSpendingSettings: () => ({ excludedCategoryIds: [] }),
}));

function report(outflow: number): MonthlyReport {
  const summary = { income: 0, outflow, saved: 0, net: -outflow, count: 1 };
  return {
    baseCurrency: "USD",
    current: summary,
    prior: summary,
    spendingBreakdown: [],
    incomeBreakdown: [],
    savingsBreakdown: [],
    byDay: [],
    byDayByCategory: [],
  };
}

const timezone = "America/Toronto";
const range = spendingRangeToReportsRange(
  { from: new Date(2025, 2, 8), to: new Date(2025, 2, 10) },
  timezone,
);

function setup(custom: boolean) {
  render(
    <MemoryRouter>
      <FormattingProvider locale="zh-CN" timezone={timezone}>
        <WhereIAmStage
          range={range}
          priorRange={custom ? comparisonRange(range, "prior", timezone)! : undefined}
          currentReport={report(300)}
          priorReport={report(150)}
          months={[]}
          taxonomyCategories={[]}
          incomeCategories={[]}
          savingsCategories={[]}
          budget={undefined}
          currency="USD"
          isLoading={false}
        />
      </FormattingProvider>
    </MemoryRouter>,
  );
}

describe("Where I am comparison labels", () => {
  it("labels a custom span as a period and shows the actual prior dates across DST", () => {
    setup(true);
    expect(screen.getByText("本周期已花费")).toBeInTheDocument();
    expect(screen.getByText(/对比 2025年3月5日 – 2025年3月7日/)).toBeInTheDocument();
    expect(screen.queryByText("本月已花费")).not.toBeInTheDocument();
    expect(screen.queryByText(/对比 二月/)).not.toBeInTheDocument();
  });
  it("retains month labels for preset month selections", () => {
    setup(false);
    expect(screen.getByText("本月已花费")).toBeInTheDocument();
    expect(screen.getByText(/对比 二月/)).toBeInTheDocument();
  });
});
