import { render, screen, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
import { VelocityCard } from "./velocity-card";

vi.mock("@/components/dashboard-card", () => ({
  DashboardCard: ({
    children,
    title,
    meta,
  }: {
    children: ReactNode;
    title: string;
    meta?: string;
  }) => (
    <section aria-label={title}>
      <span>{meta}</span>
      {children}
    </section>
  ),
}));

vi.mock("./compact-amount", () => ({
  CompactAmount: ({ value }: { value: number }) => <span>{value}</span>,
}));

const base = {
  netChange: 0,
  portfolioGains: 0,
  otherAssetChanges: 0,
  contributions: 0,
  equityBuilt: 0,
  perMonth: 0,
  months: 4,
  startDate: "2024-01-01",
};

describe("VelocityCard", () => {
  it("renders the localized period separately from the drivers heading", () => {
    render(
      <VelocityCard
        velocity={{
          ...base,
          netChange: 300,
          portfolioGains: 100,
          contributions: 100,
          equityBuilt: 100,
          perMonth: 100,
          months: 3,
        }}
        currency="USD"
        periodLabel="过去 3 个月"
      />,
    );

    const card = screen.getByRole("region", { name: "月度节奏" });
    expect(within(card).getByText("过去 3 个月")).toBeInTheDocument();
    expect(within(card).getByText("变化驱动因素")).toBeInTheDocument();
    expect(within(card).queryByText("Drivers of past 3 months change")).not.toBeInTheDocument();
    expect(card).toHaveTextContent(/自 .*2024/);
  });

  it("lists moving drivers largest first with bars scaled to the largest", () => {
    render(
      <VelocityCard
        velocity={{
          ...base,
          netChange: 1100,
          portfolioGains: 300,
          otherAssetChanges: -1200,
          contributions: 2000,
          perMonth: 275,
        }}
        currency="EUR"
        periodLabel="YTD"
      />,
    );
    const labels = screen
      .getAllByText(/^(投资收益|其他资产|投入|偿还债务)$/)
      .map((element) => element.textContent);
    expect(labels).toEqual(["投入", "其他资产", "投资收益"]);

    const row = (label: string) => screen.getByText(label).parentElement!.parentElement!;
    const bar = (label: string) =>
      row(label).querySelector<HTMLElement>("[style*='width']")!.style.width;
    expect(row("投入")).toHaveTextContent("+500");
    expect(row("其他资产")).toHaveTextContent("-300");
    expect(row("投资收益")).toHaveTextContent("+75");
    expect(bar("投入")).toBe("100%");
    expect(bar("其他资产")).toBe("60%");
    expect(bar("投资收益")).toBe("15%");
  });

  it("names drivers by their direction", () => {
    render(
      <VelocityCard
        velocity={{
          ...base,
          netChange: -160,
          portfolioGains: -100,
          contributions: -50,
          equityBuilt: -10,
          perMonth: -40,
        }}
        currency="USD"
        periodLabel="YTD"
      />,
    );
    expect(screen.getByText("投资亏损")).toBeInTheDocument();
    expect(screen.getByText("取出")).toBeInTheDocument();
    expect(screen.getByText("新增债务")).toBeInTheDocument();
  });

  it("shows a sole driver as the whole change instead of repeating the headline", () => {
    render(
      <VelocityCard
        velocity={{ ...base, netChange: 400, portfolioGains: 400, perMonth: 100 }}
        currency="USD"
        periodLabel="ALL"
      />,
    );
    const row = screen.getByText("投资收益").parentElement!;
    expect(row).toHaveTextContent("100%");
    expect(row.querySelector("[style*='width']")).toBeNull();
    expect(screen.queryByText("其他资产")).not.toBeInTheDocument();
  });

  it.each([
    [150, "· 12 个月平均值的 1.5 倍"],
    [-150, null],
  ])("compares with the trailing year only for same-signed paces (%s)", (trailing, text) => {
    render(
      <VelocityCard
        velocity={{ ...base, netChange: 900, portfolioGains: 900, perMonth: 225 }}
        trailingYearMonthly={trailing}
        currency="USD"
        periodLabel="3M"
      />,
    );
    const card = screen.getByRole("region", { name: "月度节奏" });
    if (text) expect(card).toHaveTextContent(text);
    else expect(card).not.toHaveTextContent("12 个月平均值");
  });

  it("hides the drivers section when nothing moved", () => {
    render(<VelocityCard velocity={base} currency="USD" periodLabel="1M" />);
    expect(screen.queryByText("变化驱动因素")).not.toBeInTheDocument();
  });
});
