import { ActivityType } from "@/lib/constants";
import type { ActivityDetails } from "@/lib/types";
import { describe, expect, it } from "vitest";
import { buildActivityFilterUrl } from "./activity-links";

describe("buildActivityFilterUrl", () => {
  it("links asset activities to Activity page with account, date, type, and symbol search", () => {
    const url = buildActivityFilterUrl(
      createActivity({
        accountId: "account-1",
        activityType: ActivityType.BUY,
        date: new Date("2024-08-18T14:30:00.000Z"),
        assetSymbol: "AAPL",
      }),
    );

    expect(url).toBe(
      "/activities?account=account-1&from=2024-08-18&to=2024-08-18&types=BUY&q=AAPL",
    );
  });

  it("links cash-only activities without a search query", () => {
    const url = buildActivityFilterUrl(
      createActivity({
        accountId: "account-1",
        activityType: ActivityType.DEPOSIT,
        date: "2026-03-06T14:00:00+00:00" as unknown as Date,
        assetSymbol: "",
        assetId: "",
      }),
    );

    expect(url).toBe("/activities?account=account-1&from=2026-03-06&to=2026-03-06&types=DEPOSIT");
  });
});

function createActivity(overrides: Partial<ActivityDetails> = {}): ActivityDetails {
  return {
    id: "activity-1",
    activityType: ActivityType.BUY,
    date: new Date("2024-08-18T14:30:00.000Z"),
    quantity: "1",
    unitPrice: "1",
    amount: "1",
    fee: "0",
    currency: "USD",
    needsReview: false,
    createdAt: new Date("2024-08-18T14:30:00.000Z"),
    assetId: "AAPL",
    updatedAt: new Date("2024-08-18T14:30:00.000Z"),
    accountId: "account-1",
    accountName: "Test Account",
    accountCurrency: "USD",
    assetSymbol: "AAPL",
    ...overrides,
  };
}
