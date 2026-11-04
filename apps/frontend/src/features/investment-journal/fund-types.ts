import type { Exact, Resource, ResourceStatus } from "./types";

export function sameResource(a: Resource, b: Resource) {
  return (
    a.accountId === b.accountId &&
    (a.kind === "CASH"
      ? b.kind === "CASH" && a.currency === b.currency
      : b.kind === "SECURITY" && a.assetId === b.assetId)
  );
}

export interface AllocationPreview {
  schemaVersion: 1;
  asOfDate: string;
  resources: ResourceStatus[];
  previewHash: string;
  sourceReferences: {
    operationId: string;
    resource: Resource;
    sourceKind: string;
    sourceId: string;
    sourceCounterpartyId: string | null;
    originVersion: string | null;
    currentVersion: string | null;
    state: "VALID" | "INVALID" | "DELETED" | "UNAVAILABLE";
  }[];
}
export interface AllocationDelta {
  purposeId: string;
  resource: Resource;
  delta: Exact;
}
export interface FundIssue {
  id: string;
  kind:
    | "PURPOSE_EXCESS"
    | "RESOURCE_UNAVAILABLE"
    | "SOURCE_CHANGED"
    | "SOURCE_INVALIDATED"
    | "CORPORATE_ACTION"
    | "CALIBRATION"
    | "TRANSFER_CHANGED";
  resource: Resource;
  sourceId: string | null;
  excess: Exact | null;
}
export interface FundHistoryItem {
  id: string;
  operationKind: string;
  reason: string;
  correctionOf: string | null;
  sourceKind: string | null;
  sourceId: string | null;
  createdAt: string;
  createdActorKind: "GUI_SESSION" | "PROGRAM";
  createdChannel: string;
  entries: AllocationDelta[];
}
export interface FundReversalPreview {
  funding: AllocationPreview;
  entries: AllocationDelta[];
  balancesAfter: { purposeId: string; resource: Resource; amount: Exact }[];
  pendingReconciliation: FundIssue[];
  previewHash: string;
}
