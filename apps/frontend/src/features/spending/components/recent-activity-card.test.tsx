import { render, screen } from "@/test/render";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { RecentActivityCard } from "./recent-activity-card";
import type { Activity } from "@/lib/types";

vi.mock("@tanstack/react-query", () => ({
  useQueries: vi.fn(() => []),
}));

function renderRecentActivityCard(activities: Activity[] = []) {
  return render(
    <MemoryRouter>
      <RecentActivityCard activities={activities} categoriesMeta={new Map()} />
    </MemoryRouter>,
  );
}

describe("RecentActivityCard", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("shows the standard empty state without a setup link", () => {
    renderRecentActivityCard();

    expect(screen.getByText("没有近期活动。")).toBeInTheDocument();
    expect(screen.queryByText("未选择支出账户。")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("link", { name: "打开支出设置 →" }),
    ).not.toBeInTheDocument();
  });

  it("labels each activity with its own currency, not the base currency", () => {
    const activity: Activity = {
      id: "1",
      accountId: "acc-1",
      activityDate: new Date().toISOString().slice(0, 10),
      activityType: "WITHDRAWAL",
      amount: "100",
      currency: "EUR",
      notes: "Coffee",
    } as unknown as Activity;

    renderRecentActivityCard([activity]);

    const row = screen.getByText("Coffee").closest("a");
    expect(row).toBeInTheDocument();
    expect(row?.textContent).toContain("€");
    expect(row?.textContent).not.toContain("$");
  });
});
