/** Native values stay decimal strings. Display and form code must never infer zero. */
export type Exact = string;
export type Action = "BUY" | "SELL" | "HOLD" | "WAIT" | "RESEARCH" | "OTHER";
export type Origin = "USER_PROVIDED" | "ASSISTED_DRAFT" | "UNSPECIFIED";
export type Resource =
  | { kind: "CASH"; accountId: string; currency: string }
  | { kind: "SECURITY"; accountId: string; assetId: string };
export interface DecisionOption {
  action: string;
  description: string;
  advantages: string | null;
  disadvantages: string | null;
  notSelectedReason: string | null;
}
export function hasOptionContent(option: DecisionOption) {
  return [
    option.action,
    option.description,
    option.advantages,
    option.disadvantages,
    option.notSelectedReason,
  ].some((value) => value?.trim());
}
export interface Evidence {
  informationKind: "FACT" | "HYPOTHESIS" | "UNKNOWN";
  content: string;
  source: string | null;
  probabilityLowerBps: number | null;
  probabilityUpperBps: number | null;
  probabilityBasis: string | null;
  purposeId: string | null;
  assumedLossAmount: Exact | null;
  lossCurrency: string | null;
  lossBasis: string | null;
}
export interface Funding {
  purposeId: string;
  resource: Resource;
  planned: Exact;
}
export interface DecisionBody {
  schemaVersion: 1;
  title: string;
  action: Action | null;
  assetId: string | null;
  plannedAmount: Exact | null;
  plannedQuantity: Exact | null;
  currency: string | null;
  thesis: string;
  acceptedDisadvantages: string | null;
  informationValue: string | null;
  waitingCost: string | null;
  declaredAt: string | null;
  fundingStatus: "UNKNOWN" | "PLANNED";
  scenarioTags: string[];
  purposeIds: string[];
  principleRevisionIds?: string[];
  options: DecisionOption[];
  evidence: Evidence[];
  funding: Funding[];
  contentOrigin: Origin;
}
export interface Block<T = Record<string, unknown>> {
  scope: Record<string, unknown>;
  asOfDate: string;
  sourceDataAsOf: string | null;
  currency: string;
  quality: "COMPLETE" | "PARTIAL" | "UNAVAILABLE";
  stale: boolean;
  issues: { code: string; sourceId: string | null; currency: string | null }[];
  data: T;
}
export interface Household {
  date: string;
  currency: string;
  totalAssets: Exact | null;
  financialAssets: Exact | null;
  otherAssets: Exact | null;
  totalLiabilities: Exact | null;
  netWorth: Exact | null;
  items: {
    id: string;
    name: string;
    category: string;
    currency: string;
    value: Exact | null;
    substituteValue: Exact | null;
    valuationDate: string | null;
    fxSourceDate?: string | null;
    stale: boolean;
    basis: string;
    issues: { code: string }[];
  }[];
}
export interface ContextCapture {
  schemaVersion: 1;
  sourceVersion: string;
  response: {
    schemaVersion: 1;
    asOfDate: string;
    currency: string;
    query: { accountIds: string[]; purposeIds: string[] };
    funding: Block<
      Partial<{
        purposes: Purpose[];
        allocation: { resources: ResourceStatus[] };
        accountValuations: {
          accounts: {
            accountId: string;
            baseCurrency: string;
            cashBalanceBase: Exact | null;
            investmentMarketValueBase: Exact | null;
            sourceDataAsOf: string | null;
            calculatedAt: string;
            stale: boolean;
            issues: { code: string; assetId: string | null; currency: string | null }[];
          }[];
        } | null;
        risks: {
          purposeId: string;
          currency: string;
          cashValue: Exact | null;
          minimumCash: Exact | null;
          minimumCashStatus: string;
          singleAssetLimitStatus: string;
          lossBudget: Exact | null;
          lossComparisonStatus: string;
          quality: string;
          fxSourceDate?: string | null;
        }[];
        reconciliation: { kind: string }[];
        decisionAssessment?: {
          plannedFunding: {
            purposeId: string;
            resource: Resource;
            planned: Exact;
            purposeAmountBefore: Exact | null;
            purposeAmountAfter: Exact | null;
            shortfall: Exact | null;
            unallocatedAvailable: Exact | null;
            status: string;
          }[];
          lossScenarios: {
            purposeId: string;
            currency: string | null;
            lossBudget: Exact | null;
            assumedLossAmount: Exact | null;
            hypothesisBasis: string | null;
            status: string;
          }[];
        };
        principleVersions?: { id: string; revisionNumber: number; body: { name: string } }[];
        resourceReferences?: {
          accounts: { id: string; name: string; currency: string }[];
          assets: {
            id: string;
            name: string | null;
            displayCode: string | null;
            currency: string;
          }[];
        };
        decisionReferences?: {
          accounts: { id: string; name: string; currency: string }[];
          assets: {
            id: string;
            name: string | null;
            displayCode: string | null;
            currency: string;
          }[];
        };
      }>
    >;
    recentCashFlow: Block<
      Partial<{
        convertedOrdinary: {
          currency: string;
          income: Exact | null;
          expense: Exact | null;
          fxSources?: { sourceCurrency: string; activityDate: string; sourceDate: string }[];
        };
        internalCashTransfers: {
          outActivityId: string;
          occurredAt: string;
          sourceResource: Resource;
          destinationResource: Resource;
          sourceAmount: Exact;
          destinationAmount: Exact;
        }[];
        ordinary?: {
          currencies: {
            currency: string;
            income: Exact | null;
            expense: Exact | null;
            knownIncome: Exact;
            knownExpense: Exact;
          }[];
        };
        purposeFlows: Record<
          "allocationIn" | "allocationOut" | "tradeConversions" | "adjustments",
          {
            operationId: string;
            occurredOn: string;
            entry: { purposeId?: string; resource: Resource; delta: Exact };
          }[]
        >;
      }>
    >;
    household: Block<Partial<Household>>;
  };
}
export interface ResourceStatus {
  observation: {
    resource: Resource;
    sourceVersion?: string;
    sources?: { kind: string; id: string; version: string; applied: boolean; effect: string }[];
  };
  balance: {
    actual: Exact | null;
    allocated: Exact;
    unallocated: Exact | null;
    excess: Exact | null;
    negativeActual: Exact | null;
  };
  purposes: Record<string, Exact>;
}
export interface Purpose {
  id: string;
  name: string;
  category: string;
  currency: string;
  targetAmount: Exact | null;
  useOn?: string | null;
  liquidity?: string | null;
  minimumCash: Exact | null;
  lossBudget: Exact | null;
  singleAssetLimitBps: number | null;
  isArchived: boolean;
  version: number;
}
export interface PrincipleSummary {
  principleId: string;
  name: string;
  currentRevisionId: string | null;
  currentRevisionNumber: number | null;
  draftRevisionId: string | null;
  isArchived: boolean;
  version: number;
}
export interface DecisionSummary {
  decisionId: string;
  title: string;
  draftTitle: string | null;
  lifecycle: "DRAFT" | "CONFIRMED" | "ARCHIVED";
  executionState: "OPEN" | "CLOSED" | "NOT_APPLICABLE";
  currentRevisionId: string | null;
  currentRevisionNumber: number | null;
  draftRevisionId: string | null;
  draftRevisionNumber: number | null;
  nextReviewAt: string | null;
  version: number;
}
export interface Revision {
  id: string;
  revisionNumber: number;
  status: "DRAFT" | "CONFIRMED" | "DISCARDED";
  body: DecisionBody;
  nextCheckAt: string | null;
  context: ContextCapture | null;
  confirmedAt: string | null;
  confirmedActorKind: "GUI_SESSION" | "PROGRAM" | null;
  contentOrigin: Origin;
}
export interface DecisionDetail extends DecisionSummary {
  closeReason?: string | null;
  revisions: Revision[];
}
export interface DecisionTarget {
  decisionId: string;
  draftRevisionId: string;
  expectedVersion: number;
}
export interface ConfirmationPreview {
  ledgerId: string;
  target: DecisionTarget;
  revisionNumber: number;
  body: DecisionBody;
  nextCheckAt: string | null;
  expectedContentHash: string;
  expectedContextHash: string;
  context: ContextCapture;
}
export interface JournalStatus {
  ledgerId: string;
  ready: boolean;
  asOfDate: string;
  timezone: string;
  enabledAt: string | null;
  enabledOn: string | null;
  onboardingCompletedAt: string | null;
  projection: {
    pendingSources: number;
    backfillComplete: Record<string, boolean>;
    error?: string | null;
  };
}
export interface WriteResult {
  requestId: string;
  replayed: boolean;
  decisionId?: string;
  draftRevisionId?: string;
  currentRevisionId?: string;
  revisionNumber?: number;
  version?: number;
}
export function emptyDecision(): DecisionBody {
  return {
    schemaVersion: 1,
    title: "",
    action: null,
    assetId: null,
    plannedAmount: null,
    plannedQuantity: null,
    currency: null,
    thesis: "",
    acceptedDisadvantages: null,
    informationValue: null,
    waitingCost: null,
    declaredAt: null,
    fundingStatus: "UNKNOWN",
    scenarioTags: [],
    purposeIds: [],
    principleRevisionIds: [],
    options: [],
    evidence: [],
    funding: [],
    contentOrigin: "UNSPECIFIED",
  };
}
