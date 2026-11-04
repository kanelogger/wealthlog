import type { AllocationDelta, FundIssue, FundReversalPreview } from "./fund-types";

export interface Execution {
  id: string;
  accountId: string;
  assetId: string;
  side: "BUY" | "SELL";
  occurredAt: string;
  sourceId: string;
  sourceVersion?: string;
  quantity: string | null;
  netCash: string | null;
  settlementCurrency: string | null;
  fee: string | null;
  tax: string | null;
  activityCurrency: string;
}
export interface ExecutionSource {
  execution: Execution;
  sourceState: "VALID" | "INVALID" | "DELETED" | "UNAVAILABLE";
  currentSourceVersion: string | null;
  projectionPending: boolean;
  activeLinks: {
    id: string;
    sourceVersion: string;
    snapshot: {
      execution: Execution;
      decision: { revisionNumber: number; confirmedAt: string };
    };
  }[];
}
export interface QueueItem {
  id: string;
  kind: string;
  category?: "FUND_RECONCILIATION";
  execution?: Execution;
  source?: ExecutionSource;
  decisionId?: string;
  revisionId?: string;
  revisionNumber?: number;
  title?: string;
  dueOn?: string;
  excess?: FundIssue["excess"];
  resource?: FundIssue["resource"];
}
export interface WorkQueue {
  complete: boolean;
  items: QueueItem[];
  asOfDate: string;
}
export const initialQueueFilters = { historical: false, search: "", page: 0 };

export interface ExecutionResolutionDraft {
  executionId: string;
  resolution: "ADJUST_OWNERSHIP" | "NO_OWNERSHIP_CHANGE";
  entries: AllocationDelta[];
}
export interface ExecutionReconciliationPreview extends Omit<
  FundReversalPreview,
  "pendingReconciliation"
> {
  source: ExecutionSource;
  resolution: ExecutionResolutionDraft["resolution"];
}
