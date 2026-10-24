import { describe, expect, it } from "vitest";

import type { Account } from "@/lib/types";
import {
  canAddHoldings,
  getAllowedActivityTypes,
  getActivityRestrictionLevel,
  restrictionAllowsType,
} from "./activity-restrictions";

function holdingsAccount(providerAccountId?: string): Account {
  return {
    id: "account",
    name: "account",
    accountType: "SECURITIES",
    balance: 0,
    currency: "USD",
    isDefault: false,
    isActive: true,
    isArchived: false,
    trackingMode: "HOLDINGS",
    createdAt: new Date("2026-08-05T00:00:00Z"),
    updatedAt: new Date("2026-08-05T00:00:00Z"),
    providerAccountId,
  };
}

describe("canAddHoldings", () => {
  it("allows manual holdings accounts", () => {
    expect(canAddHoldings(holdingsAccount())).toBe(true);
  });

  it("allows manual holdings updates on accounts with historical provider metadata", () => {
    expect(canAddHoldings(holdingsAccount("provider-account"))).toBe(true);
  });
});

describe("manual holdings activity types", () => {
  it("allows recording a split, which applies to the snapshot quantities", () => {
    expect(getAllowedActivityTypes(holdingsAccount())).toContain("SPLIT");
    expect(restrictionAllowsType("limited", "SPLIT")).toBe(true);
  });

  it("keeps trades on the account page's holdings update", () => {
    expect(getAllowedActivityTypes(holdingsAccount())).not.toContain("BUY");
    expect(restrictionAllowsType("limited", "SELL")).toBe(false);
  });

  it("allows cash and split entries on accounts with historical provider metadata", () => {
    const account = holdingsAccount("provider-account");
    expect(getAllowedActivityTypes(account)).toContain("DEPOSIT");
    expect(getAllowedActivityTypes(account)).toContain("SPLIT");
    expect(getAllowedActivityTypes(account)).not.toContain("BUY");
    expect(getActivityRestrictionLevel(account)).toBe("limited");
  });
});
