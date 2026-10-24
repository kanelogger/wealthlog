import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";

import { HoldingType } from "@/lib/constants";
import type { Holding } from "@/lib/types";
import { PlannerInput } from "./rebalance-tab";
import { EligibleHoldingsSelector } from "./eligible-holdings-selector";
import { getEligibleHoldings, groupEligibleHoldings } from "./eligible-holdings";

if (typeof ResizeObserver === "undefined") {
  globalThis.ResizeObserver = class {
    observe() {
      return undefined;
    }
    unobserve() {
      return undefined;
    }
    disconnect() {
      return undefined;
    }
  } as typeof ResizeObserver;
}
if (!HTMLElement.prototype.scrollIntoView) {
  HTMLElement.prototype.scrollIntoView = () => undefined;
}

function holding(
  assetId: string,
  symbol: string,
  name: string,
  instrumentType?: string,
  holdingType: Holding["holdingType"] = HoldingType.SECURITY,
  currency = "USD",
  exchangeMic?: string,
): Holding {
  return {
    id: `${assetId}-${symbol}`,
    accountId: "account-1",
    holdingType,
    instrument: {
      id: assetId,
      symbol,
      name,
      currency,
      quoteMode: "MARKET",
      exchangeMic,
      instrumentType,
    },
  } as Holding;
}

function Harness({ holdings }: { holdings: Holding[] }) {
  const [excludedAssetIds, setExcludedAssetIds] = useState<Set<string>>(new Set());
  return (
    <EligibleHoldingsSelector
      holdings={holdings}
      excludedAssetIds={excludedAssetIds}
      onToggle={(assetId) =>
        setExcludedAssetIds((previous) => {
          const next = new Set(previous);
          if (next.has(assetId)) next.delete(assetId);
          else next.add(assetId);
          return next;
        })
      }
      onSelectAll={() => setExcludedAssetIds(new Set())}
      onClear={() =>
        setExcludedAssetIds(new Set(getEligibleHoldings(holdings).map((row) => row.assetId)))
      }
    />
  );
}

describe("EligibleHoldingsSelector", () => {
  const holdings = [
    holding("asset-bond", "BND", "Bond fund", "BOND"),
    holding("asset-vti", "VTI", "Vanguard Total Stock", "EQUITY"),
    holding("asset-vti", "VTI", "Vanguard Total Stock", "EQUITY", HoldingType.SECURITY),
    holding("asset-unknown", "MYST", "Mystery asset"),
    holding("cash-usd", "USD", "现金", undefined, HoldingType.CASH),
  ];

  it("selects each unique non-cash instrument initially and groups it by type", () => {
    render(<Harness holdings={holdings} />);

    expect(screen.getByRole("button", { name: /可买入的持仓/ })).toHaveTextContent(
      "已选择全部持仓",
    );

    expect(groupEligibleHoldings(getEligibleHoldings(holdings)).map((group) => group.key)).toEqual([
      "EQUITY",
      "BOND",
      "OTHER",
    ]);

    expect(screen.queryByText("股票")).not.toBeInTheDocument();
  });

  it("announces selection state on the trigger and rows", async () => {
    const user = userEvent.setup();
    render(<Harness holdings={holdings} />);

    expect(
      screen.getByRole("button", { name: /可买入的持仓.*已选择全部持仓/ }),
    ).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: /可买入的持仓.*已选择全部持仓/ }),
    );
    const vti = screen.getByRole("option", { name: /VTI.*已选择/ });
    expect(vti).toHaveAccessibleName(/已选择/);

    await user.click(vti);
    expect(screen.getByRole("option", { name: /VTI.*未选择/ })).toBeInTheDocument();
  });

  it("opens grouped rows, searches by name, and toggles individual holdings", async () => {
    const user = userEvent.setup();
    render(<Harness holdings={holdings} />);
    await user.click(screen.getByRole("button", { name: /可买入的持仓/ }));

    expect(screen.getByText("股票")).not.toBeInstanceOf(HTMLButtonElement);
    expect(screen.getByText("债券")).toBeInTheDocument();
    expect(screen.getByText("其他")).toBeInTheDocument();

    const search = screen.getByPlaceholderText("搜索持仓");
    await user.type(search, "Vanguard");
    expect(screen.getByText("VTI")).toBeInTheDocument();
    expect(screen.queryByText("BND")).not.toBeInTheDocument();

    await user.click(screen.getByRole("option", { name: /VTI.*Vanguard Total Stock/ }));
    expect(screen.getByRole("button", { name: /可买入的持仓/ })).toHaveTextContent(
      "已选择 2 / 3",
    );
  });

  it("disambiguates duplicate labels and reaches each row from the keyboard", async () => {
    const user = userEvent.setup();
    render(
      <Harness
        holdings={[
          holding("asset-xnas", "ABC", "Acme Corp", "EQUITY", HoldingType.SECURITY, "USD", "XNAS"),
          holding("asset-xtse", "ABC", "Acme Corp", "EQUITY", HoldingType.SECURITY, "CAD", "XTSE"),
        ]}
      />,
    );

    await user.click(screen.getByRole("button", { name: /可买入的持仓/ }));
    expect(
      screen.getByRole("option", { name: /ABC.*Acme Corp.*XNAS.*USD.*已选择/ }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("option", { name: /ABC.*Acme Corp.*XTSE.*CAD.*已选择/ }),
    ).toBeInTheDocument();
    expect(screen.getByText("XNAS · USD")).toBeInTheDocument();
    expect(screen.getByText("XTSE · CAD")).toBeInTheDocument();
    expect(
      screen
        .getAllByRole("option")
        .filter((option) => option.getAttribute("aria-selected") === "true"),
    ).toHaveLength(1);

    await user.click(screen.getByPlaceholderText("搜索持仓"));
    await user.keyboard("{ArrowDown}{Enter}");

    expect(
      screen.getByRole("option", { name: /ABC.*Acme Corp.*XTSE.*CAD.*未选择/ }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /可买入的持仓/ })).toHaveTextContent(
      "已选择 1 / 2",
    );
  });

  it("supports Clear and Select all and explains the empty state", async () => {
    const user = userEvent.setup();
    render(<Harness holdings={holdings} />);
    await user.click(screen.getByRole("button", { name: /可买入的持仓/ }));
    await user.click(screen.getByRole("button", { name: "清除" }));

    expect(screen.getByRole("button", { name: /可买入的持仓/ })).toHaveTextContent(
      "已选择 0 / 3",
    );
    expect(
      screen.getByText("请至少选择一项持仓后再计算计划。"),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "全选" }));
    expect(screen.getByRole("button", { name: /可买入的持仓/ })).toHaveTextContent(
      "已选择全部持仓",
    );
  });

  it("disables Calculate and Enter when no holding is eligible", () => {
    const onCalculate = vi.fn();
    render(
      <PlannerInput
        description=""
        cashValue="100"
        availableCash={100}
        currency="USD"
        onCashChange={vi.fn()}
        onCalculate={onCalculate}
        hasPlan={false}
        isCalculating={false}
        isSourceLoading={false}
        hasEligibleHoldings={false}
        eligibleHoldingsSelector={<p>请至少选择一项持仓后再计算计划。</p>}
      />,
    );

    const calculate = screen.getByRole("button", { name: "计算方案" });
    expect(calculate).toBeDisabled();
    expect(
      screen.getByText("请至少选择一项持仓后再计算计划。"),
    ).toBeInTheDocument();

    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
    expect(onCalculate).not.toHaveBeenCalled();
  });
});
