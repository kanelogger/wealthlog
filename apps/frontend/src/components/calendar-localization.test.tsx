import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Calendar, DatePickerInput, FormattingProvider, MonthYearPicker } from "@wealthfolio/ui";
import { describe, expect, it, vi } from "vitest";
import { MonthSwitcher } from "../features/spending/components/month-switcher";

describe("China calendar localization", () => {
  it("labels a calendar day in Simplified Chinese", () => {
    render(
      <FormattingProvider locale="zh-CN" uiLocale="zh">
        <Calendar mode="single" defaultMonth={new Date(2026, 6, 1)} />
      </FormattingProvider>,
    );

    expect(screen.getByRole("button", { name: /2026年7月10日/ })).toBeInTheDocument();
  });

  it("formats month choices in Chinese", () => {
    render(
      <FormattingProvider locale="CN" uiLocale="zh">
        <MonthYearPicker value="2026-01" maxDate="2026-12" />
      </FormattingProvider>,
    );

    expect(screen.getByRole("button", { name: "一月" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Jan" })).not.toBeInTheDocument();
  });

  it("labels the report month in Chinese and keeps the month key", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <FormattingProvider locale="zh-CN" uiLocale="zh">
        <MonthYearPicker value="2026-01" maxDate="2026-12" onChange={onChange} />
        <MonthSwitcher
          selectedMonth="2026-08"
          availableMonths={["2026-08"]}
          onMonthChange={vi.fn()}
        />
      </FormattingProvider>,
    );

    expect(screen.getByText("2026年")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "一月" }));
    expect(onChange).toHaveBeenCalledWith("2026-01");
    expect(screen.getByText(/2026年8月/)).toBeInTheDocument();
  });

  it("uses Chinese labels for the date picker", async () => {
    const user = userEvent.setup();
    render(
      <FormattingProvider locale="zh-CN" uiLocale="zh">
        <DatePickerInput value="2026-08-18" onChange={vi.fn()} />
      </FormattingProvider>,
    );

    const segments = screen.getAllByRole("spinbutton").map((segment) => segment.dataset.type);
    expect(segments).toEqual(["year", "month", "day"]);
    await user.click(screen.getByRole("button"));
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
  });
});
