import { REPORTS_PERIODS, type ReportsPeriod } from "./reports-period";

export const INSIGHTS_PERIOD_STORAGE_KEY = "spending-insights-period";
export const INSIGHTS_PERIOD_UPDATED_AT_STORAGE_KEY = "spending-insights-period-updated-at";
export const DASHBOARD_PERIOD_UPDATED_AT_STORAGE_KEY = "spending-dashboard-period-updated-at";

export function normalizeReportsPeriod(value: string | null | undefined): ReportsPeriod | null {
  if (!value) return null;
  if (REPORTS_PERIODS.includes(value as ReportsPeriod)) return value as ReportsPeriod;
  return null;
}

export function periodPreferenceTimestamp(): string {
  return String(Date.now());
}

export function shouldPreferDashboardPeriod(opts: {
  persistedInsightPeriod: string | null | undefined;
  dashboardUpdatedAt: string | null | undefined;
  insightUpdatedAt: string | null | undefined;
}): boolean {
  if (!normalizeReportsPeriod(opts.persistedInsightPeriod)) return true;
  return timestampValue(opts.dashboardUpdatedAt) > timestampValue(opts.insightUpdatedAt);
}

function timestampValue(value: string | null | undefined): number {
  const timestamp = Number(value);
  return Number.isFinite(timestamp) ? timestamp : 0;
}
