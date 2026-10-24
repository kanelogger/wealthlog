import { AccountType } from "@/lib/constants";

import { cashActivityFlowMetadata, resolveCashActivitySubtype } from "./cash-activity-form-utils";

describe("resolveCashActivitySubtype", () => {
  it("sets reimbursement subtype for new cash-account credits", () => {
    expect(
      resolveCashActivitySubtype({
        activityType: "CREDIT",
        accountType: AccountType.CASH,
      }),
    ).toBe("REIMBURSEMENT");
  });

  it("preserves existing cash-account credit subtypes on edit", () => {
    expect(
      resolveCashActivitySubtype({
        activityType: "CREDIT",
        accountType: AccountType.CASH,
        existingActivityType: "CREDIT",
        existingSubtype: "BONUS",
      }),
    ).toBe("BONUS");
  });

  it("does not assign reimbursement subtype to credit-card credits", () => {
    expect(
      resolveCashActivitySubtype({
        activityType: "CREDIT",
        accountType: AccountType.CREDIT_CARD,
        existingActivityType: "CREDIT",
        existingSubtype: "BONUS",
      }),
    ).toBeNull();
  });
});

describe("cashActivityFlowMetadata", () => {
  it("marks CREDIT/REIMBURSEMENT as crossing the performance boundary", () => {
    expect(cashActivityFlowMetadata("CREDIT", "REIMBURSEMENT")).toEqual({
      flow: { is_external: true },
    });
  });

  it.each(["REFUND", "REBATE"])("does not infer CREDIT/%s as external", (subtype) => {
    expect(cashActivityFlowMetadata("CREDIT", subtype)).toBeUndefined();
  });

  it("does not mark unrelated cash activity types as external", () => {
    expect(cashActivityFlowMetadata("CREDIT", "BONUS")).toBeUndefined();
    expect(cashActivityFlowMetadata("WITHDRAWAL", "REFUND")).toBeUndefined();
  });

  it("preserves existing metadata when adding the reimbursement boundary marker", () => {
    expect(
      cashActivityFlowMetadata("CREDIT", "REIMBURSEMENT", {
        raw_type: "merchant_refund",
        flow: { confidence: 0.9 },
      }),
    ).toEqual({
      raw_type: "merchant_refund",
      flow: { confidence: 0.9, is_external: true },
    });
  });

  it.each([true, false])("preserves an explicit %s boundary override", (isExternal) => {
    expect(
      cashActivityFlowMetadata("CREDIT", "REFUND", {
        raw_type: "merchant_refund",
        flow: { confidence: 0.9, is_external: isExternal },
      }),
    ).toEqual({
      raw_type: "merchant_refund",
      flow: { confidence: 0.9, is_external: isExternal },
    });
  });

  it("preserves an explicit false BONUS override instead of restoring the subtype default", () => {
    expect(
      cashActivityFlowMetadata("CREDIT", "BONUS", {
        flow: { is_external: false },
      }),
    ).toEqual({
      flow: { is_external: false },
    });
  });

  it("removes only the stale boundary marker when the activity stops being a credit", () => {
    expect(
      cashActivityFlowMetadata("DEPOSIT", "BONUS", {
        raw_type: "cash_bonus",
        flow: { confidence: 0.9, is_external: true },
      }),
    ).toEqual({
      raw_type: "cash_bonus",
      flow: { confidence: 0.9 },
    });
  });

  it("returns empty metadata when the stale marker was the only value", () => {
    expect(
      cashActivityFlowMetadata("DEPOSIT", "BONUS", {
        flow: { is_external: true },
      }),
    ).toEqual({});
  });
});
