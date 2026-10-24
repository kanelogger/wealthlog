import { afterEach, describe, expect, it, vi } from "vitest";

import { periodLabel, periodToReportsRange } from "./reports-period";

describe("reports periods", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("resolves LAST_MONTH to the previous full calendar month", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-06-10T12:00:00.000Z"));

    const range = periodToReportsRange("LAST_MONTH", "UTC");

    expect(range.start.toISOString()).toBe("2026-05-01T00:00:00.000Z");
    expect(range.end.toISOString()).toBe("2026-05-31T23:59:59.999Z");
    expect(range.days).toBe(31);
    expect(range.months).toBe(1);
    expect(periodLabel("LAST_MONTH")).toBe("上月");
  });
});
