import { fireEvent, render, screen } from "@testing-library/react";
import { FormattingProvider } from "@wealthfolio/ui";
import { afterEach, describe, expect, it, vi } from "vitest";

import { EventsCalendarCard } from "./events-calendar-card";
import type { EventSpendingSummary } from "../../../types/event";

vi.mock("@/hooks/use-balance-privacy", () => ({
  useBalancePrivacy: () => ({ isBalanceHidden: false }),
}));

vi.mock("../../event-dialog-provider", () => ({
  useEventDialog: () => ({ openEventDialog: vi.fn() }),
}));

describe("EventsCalendarCard", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("formats weekday content with the formatting locale", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-08-18T12:00:00Z"));

    render(
      <FormattingProvider locale="zh-CN" uiLocale="zh">
        <EventsCalendarCard events={[]} currency="EUR" selectedId={null} onSelect={vi.fn()} />
      </FormattingProvider>,
    );

    expect(screen.getByText("周日")).toBeInTheDocument();
    expect(screen.queryByText("Su")).not.toBeInTheDocument();
  });

  it("opens historical custom dates, bounds navigation, and resets when dates change", () => {
    const calendar = (start: string, end: string) => (
      <FormattingProvider locale="zh-CN" uiLocale="zh">
        <EventsCalendarCard
          events={[]}
          currency="USD"
          selectedId={null}
          onSelect={vi.fn()}
          rangeStart={new Date(start)}
          rangeEnd={new Date(end)}
          timezone="America/Los_Angeles"
        />
      </FormattingProvider>
    );
    const { rerender } = render(calendar("2025-01-01T08:00:00Z", "2025-03-01T07:59:59Z"));
    expect(screen.getByText(/2025年1月 共 0/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "上个月" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "下个月" }));
    expect(screen.getByText(/2025年2月 共 0/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "下个月" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "上个月" })).toBeEnabled();

    rerender(calendar("2024-05-01T07:00:00Z", "2024-06-01T06:59:59Z"));
    expect(screen.getByText(/2024年5月 共 0/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "上个月" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "下个月" })).toBeDisabled();
  });

  it("opens the selected event month when it loads and clamps it to the selected range", () => {
    const event: EventSpendingSummary = {
      eventId: "trip",
      eventName: "Trip",
      eventTypeId: "travel",
      eventTypeName: "Travel",
      eventTypeColor: null,
      startDate: "2025-06-15",
      endDate: "2025-06-20",
      totalSpending: 100,
      transactionCount: 1,
      currency: "USD",
      byCategory: {},
      dailySpending: {},
    };
    const calendar = (events: EventSpendingSummary[], from = "2025-01-01", to = "2025-12-31") => (
      <FormattingProvider locale="zh-CN" uiLocale="zh">
        <EventsCalendarCard
          events={events}
          currency="USD"
          selectedId={events.length ? "trip" : null}
          onSelect={vi.fn()}
          rangeStart={new Date(`${from}T00:00:00Z`)}
          rangeEnd={new Date(`${to}T23:59:59Z`)}
          timezone="UTC"
        />
      </FormattingProvider>
    );
    const { rerender } = render(calendar([]));
    expect(screen.getByText(/2025年1月 共 0/)).toBeInTheDocument();
    rerender(calendar([event]));
    expect(screen.getByText(/2025年6月 共 1/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "下个月" }));
    expect(screen.getByText(/2025年7月 共 0/)).toBeInTheDocument();
    rerender(calendar([event], "2025-07-01", "2025-12-31"));
    expect(screen.getByText(/2025年7月 共 0/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "上个月" })).toBeDisabled();
    rerender(calendar([event], "2025-01-01", "2025-05-31"));
    expect(screen.getByText(/2025年5月 共 0/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "下个月" })).toBeDisabled();
  });
});
