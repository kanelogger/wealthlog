import type { Account, Asset } from "@/lib/types";
import type { DecisionBody, Exact, Resource } from "./types";
import type { AllocationDelta, AllocationPreview, FundIssue } from "./fund-types";

export interface TradeBinding {
  decisionId: string;
  revisionId: string;
  revisionNumber: number;
  body: DecisionBody;
}
export interface ExecutionProgress {
  quantityForPlannedAsset: Exact | null;
  settlementCashForPlanCurrency: Exact | null;
  remainingQuantity: Exact | null;
  remainingCashAmount: Exact | null;
  quantityUnitDate?: string | null;
  quantityUnitStatus?: string;
}
export interface ManualAccountState {
  accountId: string;
  currency: string;
  trackingMode: string;
  asOfDate: string;
  snapshotDate: string | null;
  hasSnapshot: boolean;
  stateVersion: string;
  cashBalances: Record<string, Exact>;
  positions: { assetId: string; quantity: Exact; currency: string }[];
}
export interface ActualTradeFields {
  assetId: string;
  side: "BUY" | "SELL";
  activityDate: string;
  quantity: string;
  price: string;
  amount: string;
  fee: string;
  tax: string;
  currency: string;
  fxRate: string;
  observedCashBalance: string;
  notes: string;
  reason: string;
  deviationReason: string;
}
export interface TradeShare {
  purposeId: string;
  quantity: Exact;
  netCash: Exact;
  topUps: { resource: Resource; amount: Exact }[];
}
export interface TradePreview {
  schemaVersion: 1;
  previewHash: string;
  ownershipMode: "APPLY" | "RECONCILE_LATER";
  decision: { id: string; revisionNumber: number; body: DecisionBody; progress: ExecutionProgress };
  trade: {
    automaticAllocationEligible: boolean;
    activityCurrency: string;
    settlementCurrency: string | null;
    netCash: Exact | null;
    fee: Exact | null;
    tax: Exact | null;
    cashAdjustment: Exact | null;
    changes: { resource: Resource; delta: Exact }[];
    issues: string[];
  };
  funding: AllocationPreview | null;
  ownership: {
    topUpEntries: AllocationDelta[];
    conversionEntries: AllocationDelta[];
    balancesAfter: { purposeId: string; resource: Resource; amount: Exact }[];
    charges: { purposeId: string; currency: string; fee: Exact | null; tax: Exact | null }[];
  } | null;
  pendingReconciliation: FundIssue[];
}
export interface PreparedTrade {
  account: Account;
  asset: Asset;
  fields: ActualTradeFields;
  args: {
    decision: { decisionId: string; revisionId: string; deviationReason: string | null };
    trade: Record<string, unknown>;
  };
  facts: TradePreview;
}
export interface TradeReceipt {
  executionId: string;
  executionLinkId: string;
  decisionRevisionId: string;
  allocationOperationId: string | null;
  ownershipStatus: "APPLIED" | "RECONCILIATION_REQUIRED";
  replayed?: boolean;
}
