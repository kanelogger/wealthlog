import { renderHook } from "@testing-library/react";
import { FormattingProvider } from "@wealthfolio/ui";
import type { ReactNode } from "react";
import { describe, expect, it } from "vitest";

import { useMonthCalendar } from "./use-month-calendar";

function wrapper(locale: "zh-CN") {
  return function LocaleWrapper({ children }: { children: ReactNode }) {
    return <FormattingProvider locale={locale}>{children}</FormattingProvider>;
  };
}

describe("useMonthCalendar", () => {
  const cursor = new Date(2026, 5, 15, 12);

  it("uses Simplified Chinese month text and a Monday-first calendar", () => {
    const { result } = renderHook(() => useMonthCalendar([], cursor), {
      wrapper: wrapper("zh-CN"),
    });

    expect(result.current.monthLabel).toBe(
      new Intl.DateTimeFormat("zh-CN", {
        month: "long",
        year: "numeric",
        timeZone: "UTC",
      }).format(new Date(Date.UTC(2026, 5, 15))),
    );
    expect(result.current.weekStartsOn).toBe(1);
    expect(result.current.weeks[0].days[0].getDay()).toBe(1);
  });
});
