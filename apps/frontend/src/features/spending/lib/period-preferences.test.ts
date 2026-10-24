import { describe, expect, it } from "vitest";

import { normalizeReportsPeriod, shouldPreferDashboardPeriod } from "./period-preferences";

describe("spending period preferences", () => {
  it("normalizes legacy dashboard month periods for insights", () => {
    expect(normalizeReportsPeriod("3M")).toBe("3M");
    expect(normalizeReportsPeriod("LAST_MONTH")).toBe("LAST_MONTH");
    expect(normalizeReportsPeriod("1M")).toBeNull();
  });

  it("uses the dashboard period when no valid insight period is stored", () => {
    expect(
      shouldPreferDashboardPeriod({
        persistedInsightPeriod: null,
        dashboardUpdatedAt: "0",
        insightUpdatedAt: "100",
      }),
    ).toBe(true);
  });

  it("keeps a newer insight period when returning from the dashboard", () => {
    expect(
      shouldPreferDashboardPeriod({
        persistedInsightPeriod: "6M",
        dashboardUpdatedAt: "100",
        insightUpdatedAt: "200",
      }),
    ).toBe(false);
  });

  it("uses the dashboard period after the dashboard selector changes", () => {
    expect(
      shouldPreferDashboardPeriod({
        persistedInsightPeriod: "6M",
        dashboardUpdatedAt: "300",
        insightUpdatedAt: "200",
      }),
    ).toBe(true);
  });
});
