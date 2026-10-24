import { act, fireEvent, render, screen } from "@testing-library/react";
import { FormattingProvider } from "@wealthfolio/ui";
import { cloneElement, isValidElement, type ReactNode } from "react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import type { BudgetCategoryRow, BudgetGroupRow, BudgetSnapshot } from "../../../types/budget";
import type { MonthlyReport } from "../../../types/report";
import { MoneyFlowSection } from "./money-flow-section";

const privacy = vi.hoisted(() => ({ isBalanceHidden: false }));
vi.mock("@/hooks/use-balance-privacy", () => ({
  useBalancePrivacy: () => privacy,
}));

// jsdom has no layout: give the chart a fixed size so the Sankey draws nodes.
vi.mock("recharts", async (importOriginal) => {
  const actual = await importOriginal<typeof import("recharts")>();
  return {
    ...actual,
    ResponsiveContainer: ({ children }: { children: ReactNode }) =>
      isValidElement(children)
        ? cloneElement(children as React.ReactElement<{ width: number; height: number }>, {
            width: 900,
            height: 500,
          })
        : null,
  };
});

// jsdom has no canvas either: label measuring falls back to an average advance.
// Nor does it tell keyboard from mouse focus: `:focus-visible` follows `focusModality`.
let focusModality: "keyboard" | "mouse" = "keyboard";
beforeAll(() => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
  const matches = Element.prototype.matches;
  vi.spyOn(Element.prototype, "matches").mockImplementation(function (this: Element, selector) {
    if (selector === ":focus-visible") {
      return focusModality === "keyboard" && this === document.activeElement;
    }
    return matches.call(this, selector);
  });
});

afterEach(() => {
  privacy.isBalanceHidden = false;
  focusModality = "keyboard";
});

const spend = (categoryId: string, name: string, actual: number): BudgetCategoryRow => ({
  taxonomyId: "spending_categories",
  categoryId,
  groupId: "needs",
  parentId: null,
  name,
  color: "#A35742",
  icon: null,
  target: 0,
  actual,
  rolloverIn: 0,
  rolloverOut: 0,
  remaining: 0,
  overspent: false,
  hasDefaultTarget: false,
  hasMonthOverride: false,
  rolloverEnabled: false,
});

const needs: BudgetGroupRow = {
  group: {
    id: "needs",
    name: "Needs",
    key: "needs",
    color: "#4F6B92",
    icon: null,
    sortOrder: 0,
    isSystem: true,
    createdAt: "",
    updatedAt: "",
  },
  categoryTargetTotal: 0,
  buffer: 0,
  plannedTotal: 0,
  actual: 1618,
  rolloverIn: 0,
  rolloverOut: 0,
  remaining: 0,
  overspent: false,
  rolloverEnabled: false,
  categories: [spend("cat_housing", "Housing", 1498), spend("cat_bills", "Bills", 120)],
};

// The section reads only the computed group rows of the budget snapshot.
const budget = { computed: { groupRows: [needs] } } as unknown as BudgetSnapshot;

function report(income: number): MonthlyReport {
  const summary = { income, outflow: 1618, saved: 0, net: income - 1618, count: 3 };
  return {
    baseCurrency: "USD",
    current: summary,
    prior: summary,
    spendingBreakdown: [],
    incomeBreakdown: income
      ? [
          {
            taxonomyId: "income_sources",
            categoryId: "cat_income_salary",
            amount: income,
            count: 1,
          },
        ]
      : [],
    savingsBreakdown: [],
    byDay: [],
    byDayByCategory: [],
  };
}

function setup({
  income = 2500,
  onCategoryClick = vi.fn(),
  groups = budget,
  isMobile = false,
}: {
  income?: number;
  onCategoryClick?: (id: string) => void;
  groups?: BudgetSnapshot;
  isMobile?: boolean;
} = {}) {
  const view = render(
    <FormattingProvider locale="zh-CN">
      <MoneyFlowSection
        periodLabel="April"
        currentReport={report(income)}
        budget={groups}
        incomeCategories={[
          {
            id: "cat_income_salary",
            taxonomyId: "income_sources",
            parentId: null,
            name: "Salary",
            key: "salary",
            color: "#5A7A3E",
            sortOrder: 0,
            createdAt: "",
            updatedAt: "",
          },
        ]}
        savingsCategories={[]}
        currency="USD"
        isLoading={false}
        isMobile={isMobile}
        onCategoryClick={onCategoryClick}
      />
    </FormattingProvider>,
  );
  return { onCategoryClick, ...view };
}

describe("MoneyFlowSection", () => {
  it("draws income into groups and categories, with the rest left over", () => {
    setup();

    expect(screen.getByRole("heading", { name: "资金流向" })).toBeInTheDocument();
    expect(screen.getByText("Salary")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Needs，US$1,618" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Housing，US$1,498" })).toBeInTheDocument();
    expect(screen.getByText("剩余")).toBeInTheDocument();
    expect(screen.getByText("US$882")).toBeInTheDocument();
  });

  it("opens a category's transactions", () => {
    const { onCategoryClick } = setup();

    fireEvent.click(screen.getByRole("button", { name: "Housing，US$1,498" }));

    expect(onCategoryClick).toHaveBeenCalledWith("cat_housing");
  });

  it("zooms into a group, focuses the way back, and returns focus to the group on Escape", () => {
    setup();

    fireEvent.click(screen.getByRole("button", { name: "Needs，US$1,618" }));

    expect(screen.getByRole("heading", { name: "Needs" })).toBeInTheDocument();
    const back = screen.getByRole("button", { name: "全部流向" });
    expect(back).toHaveFocus();
    expect(screen.queryByText("Salary")).not.toBeInTheDocument();

    fireEvent.keyDown(back, { key: "Escape" });

    expect(screen.getByRole("heading", { name: "资金流向" })).toBeInTheDocument();
    expect(screen.getByText("Salary")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Needs，US$1,618" })).toHaveFocus();
  });

  it("leaves focus alone when the zoom is left with the mouse", () => {
    setup();

    fireEvent.click(screen.getByRole("button", { name: "Needs，US$1,618" }));
    fireEvent.click(screen.getByRole("button", { name: "全部流向" }), { detail: 1 });

    expect(screen.getByRole("heading", { name: "资金流向" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Needs，US$1,618" })).not.toHaveFocus();
  });

  it("opens a category from the keyboard", () => {
    const { onCategoryClick } = setup();

    fireEvent.keyDown(screen.getByRole("button", { name: "Bills，US$120" }), { key: "Enter" });

    expect(onCategoryClick).toHaveBeenCalledWith("cat_bills");
  });

  it("is a single Tab stop navigated with the arrow keys", () => {
    const { container } = setup();

    const stops = container.querySelectorAll('[aria-label^="资金流向图"] [tabindex="0"]');
    expect(stops).toHaveLength(1);
    const salary = screen.getByRole("img", { name: "Salary，US$2,500" });
    expect(stops[0]).toBe(salary);

    salary.focus();
    fireEvent.keyDown(salary, { key: "ArrowRight" });
    const total = screen.getByRole("img", { name: "总额，US$2,500" });
    expect(total).toHaveFocus();

    fireEvent.keyDown(total, { key: "ArrowRight" });
    const needs = screen.getByRole("button", { name: "Needs，US$1,618" });
    expect(needs).toHaveFocus();

    fireEvent.keyDown(needs, { key: "ArrowDown" });
    expect(screen.getByRole("img", { name: "剩余，US$882" })).toHaveFocus();
    expect(
      container.querySelectorAll('[aria-label^="资金流向图"] [tabindex="0"]'),
    ).toHaveLength(1);

    fireEvent.keyDown(screen.getByRole("img", { name: "剩余，US$882" }), { key: "ArrowLeft" });
    expect(total).toHaveFocus();
  });

  it("brings back the focused node's tooltip after the mouse passes over another", () => {
    setup();

    const leftOver = screen.getByRole("img", { name: "剩余，US$882" });
    act(() => leftOver.focus());
    const housing = screen.getByRole("button", { name: "Housing，US$1,498" });
    fireEvent.mouseEnter(housing);
    expect(screen.getByRole("tooltip")).toHaveTextContent("Housing");

    fireEvent.mouseLeave(housing);

    expect(screen.getByRole("tooltip")).toHaveTextContent("剩余");
    expect(leftOver).toHaveAttribute("aria-describedby", screen.getByRole("tooltip").id);
  });

  it("does not pin the tooltip of a node focused by a mouse click", () => {
    setup();
    focusModality = "mouse";

    const leftOver = screen.getByRole("img", { name: "剩余，US$882" });
    fireEvent.mouseEnter(leftOver);
    act(() => leftOver.focus());
    fireEvent.mouseLeave(leftOver);

    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("leaves modified arrows to the browser", () => {
    setup();

    const salary = screen.getByRole("img", { name: "Salary，US$2,500" });
    act(() => salary.focus());
    const altRight = fireEvent.keyDown(salary, { key: "ArrowRight", altKey: true });

    expect(altRight).toBe(true);
    expect(salary).toHaveFocus();
    // A plain arrow with nowhere to go stays in the chart instead of scrolling the page.
    expect(fireEvent.keyDown(salary, { key: "ArrowUp" })).toBe(false);
  });

  it("describes the total as money out when nothing came in", () => {
    setup({ income: 0 });

    act(() => screen.getByRole("img", { name: "总额，US$1,618" }).focus());

    expect(screen.getByRole("tooltip")).toHaveTextContent(
      "退款前的全部支出与预留金额。",
    );
  });

  it("explains a synthetic node to keyboard users", () => {
    setup();

    const leftOver = screen.getByRole("img", { name: "剩余，US$882" });
    act(() => leftOver.focus());

    const tooltip = screen.getByRole("tooltip");
    expect(leftOver).toHaveAttribute("aria-describedby", tooltip.id);
    expect(tooltip).toHaveTextContent("已流入但未支出或预留的资金。");
  });

  it("draws net refunds and a shortfall as sources", () => {
    const refunded = {
      computed: {
        groupRows: [
          {
            ...needs,
            categories: [...needs.categories, spend("cat_deposit", "存入", -200)],
          },
        ],
      },
    } as unknown as BudgetSnapshot;
    setup({ income: 1000, groups: refunded });

    expect(screen.getByRole("img", { name: "净退款，US$200" })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "缺口，US$418" })).toBeInTheDocument();

    fireEvent.mouseEnter(screen.getByRole("button", { name: "Needs，US$1,618" }));
    expect(screen.getByRole("tooltip")).toHaveTextContent(
      "未扣除 US$200.00 净退款。",
    );
  });

  it("masks every amount in privacy mode", () => {
    privacy.isBalanceHidden = true;
    const { container } = setup();

    expect(screen.getByRole("button", { name: "Needs，••••" })).toBeInTheDocument();
    expect(container.textContent).not.toMatch(/\$\d/);
  });

  it("shows tooltips on keyboard focus at narrow widths too", () => {
    setup({ isMobile: true });

    const total = screen.getByRole("img", { name: "总额，US$2,500" });
    act(() => total.focus());

    const tooltip = screen.getByRole("tooltip");
    expect(total).toHaveAttribute("aria-describedby", tooltip.id);
    expect(tooltip).toHaveTextContent("流入加上净退款和任何缺口，始终等于流出。");
  });

  it("stops at groups on mobile and shows a tooltip on tap", () => {
    setup({ isMobile: true });

    expect(screen.queryByRole("button", { name: "Housing，US$1,498" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Needs，US$1,618" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("img", { name: "总额，US$2,500" }));
    expect(screen.getByRole("tooltip")).toHaveTextContent(
      "流入加上净退款和任何缺口，始终等于流出。",
    );
  });

  it("notes when no income was recorded", () => {
    setup({ income: 0 });

    expect(screen.getByText("此周期内所选账户没有收入。")).toBeInTheDocument();
    expect(screen.queryByText("缺口")).not.toBeInTheDocument();
  });

  it("renders nothing when no money moved", () => {
    const empty = { computed: { groupRows: [] } } as unknown as BudgetSnapshot;
    setup({ income: 0, groups: empty });

    expect(screen.queryByRole("heading", { name: "资金流向" })).not.toBeInTheDocument();
  });
});
