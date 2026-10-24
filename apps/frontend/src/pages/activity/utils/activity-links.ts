import type { ActivityDetails } from "@/lib/types";
import { formatDateISO } from "@/lib/utils";

const DATE_KEY_PATTERN = /^\d{4}-\d{2}-\d{2}$/;

export function buildActivityFilterUrl(activity: ActivityDetails): string {
  const params = new URLSearchParams();

  if (activity.accountId) {
    params.set("account", activity.accountId);
  }

  const dateKey = toActivityDateKey(activity.date);
  if (dateKey) {
    params.set("from", dateKey);
    params.set("to", dateKey);
  }

  if (activity.activityType) {
    params.set("types", activity.activityType);
  }

  const searchTerm = getActivitySearchTerm(activity);
  if (searchTerm) {
    params.set("q", searchTerm);
  }

  const query = params.toString();
  return query ? `/activities?${query}` : "/activities";
}

function getActivitySearchTerm(activity: ActivityDetails): string | null {
  const candidates = [activity.assetSymbol, activity.assetName, activity.assetId];
  const value = candidates.find((candidate) => candidate?.trim());
  return value?.trim() ?? null;
}

function toActivityDateKey(value: Date | string | null | undefined): string | null {
  if (!value) return null;

  if (value instanceof Date) {
    return Number.isNaN(value.getTime()) ? null : formatDateISO(value);
  }

  const trimmed = value.trim();
  if (!trimmed) return null;

  const datePart = trimmed.split("T")[0];
  if (DATE_KEY_PATTERN.test(datePart)) {
    return datePart;
  }

  const parsed = new Date(trimmed);
  return Number.isNaN(parsed.getTime()) ? null : parsed.toISOString().slice(0, 10);
}
