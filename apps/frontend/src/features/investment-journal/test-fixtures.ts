// Synthetic UI fixtures. Never connect these values to a user's ledger.
import {
  emptyDecision,
  type Block,
  type ContextCapture,
  type DecisionBody,
  type DecisionDetail,
  type JournalStatus,
  type Resource,
} from "./types";

export function contextFixture(): ContextCapture {
  const block = <T>(data: T): Block<T> => ({
    scope: {},
    asOfDate: "2026-11-03",
    sourceDataAsOf: "2026-11-03",
    currency: "CNY",
    quality: "COMPLETE",
    stale: false,
    issues: [],
    data,
  });
  return {
    schemaVersion: 1,
    sourceVersion: "synthetic-source",
    response: {
      schemaVersion: 1,
      asOfDate: "2026-11-03",
      currency: "CNY",
      query: { purposeIds: [], accountIds: [] },
      funding: block({
        purposes: [],
        allocation: { resources: [] },
        risks: [],
        reconciliation: [],
        decisionReferences: { accounts: [], assets: [] },
      }),
      recentCashFlow: block({
        convertedOrdinary: { currency: "CNY", income: "3200", expense: "950.125" },
        internalCashTransfers: [],
        purposeFlows: {
          allocationIn: [],
          allocationOut: [],
          tradeConversions: [],
          adjustments: [],
        },
      }),
      household: block({
        date: "2026-11-03",
        currency: "CNY",
        totalAssets: "9007199254741003.1234",
        financialAssets: "9007199254740993.1234",
        otherAssets: "10",
        totalLiabilities: "0",
        netWorth: "9007199254741003.1234",
        items: [
          {
            id: "synthetic-cash",
            name: "合成现金",
            category: "cash",
            currency: "CNY",
            value: "9007199254740993.1234",
            substituteValue: null,
            valuationDate: "2026-11-03",
            stale: false,
            basis: "BALANCE",
            issues: [],
          },
        ],
      }),
    },
  };
}
export function decisionFixture(): DecisionBody {
  return {
    ...emptyDecision(),
    title: "等待费用核实",
    action: "WAIT",
    thesis: "用户提供的理由",
    options: [
      {
        action: "立即买入",
        description: "先核实费用后再决定",
        advantages: null,
        disadvantages: null,
        notSelectedReason: null,
      },
    ],
  };
}
export function detailFixture(): DecisionDetail {
  return {
    decisionId: "synthetic-decision",
    title: "等待费用核实",
    draftTitle: "等待费用核实",
    lifecycle: "DRAFT",
    executionState: "NOT_APPLICABLE",
    currentRevisionId: null,
    currentRevisionNumber: null,
    draftRevisionId: "synthetic-draft",
    draftRevisionNumber: 1,
    nextReviewAt: null,
    version: 2,
    revisions: [
      {
        id: "synthetic-draft",
        revisionNumber: 1,
        status: "DRAFT",
        body: decisionFixture(),
        nextCheckAt: "2027-01-08",
        context: null,
        confirmedAt: null,
        confirmedActorKind: null,
        contentOrigin: "UNSPECIFIED",
      },
    ],
  };
}
export function statusFixture(): JournalStatus {
  return {
    ledgerId: "synthetic-ledger",
    ready: true,
    asOfDate: "2026-11-03",
    timezone: "Asia/Shanghai",
    enabledAt: null,
    enabledOn: null,
    onboardingCompletedAt: "2026-11-03T00:00:00Z",
    projection: {
      pendingSources: 0,
      backfillComplete: { ACTIVITY: true, MANUAL_TRADE: true, SNAPSHOT: true },
    },
  };
}

export function richContextFixture(): ContextCapture {
  const capture = contextFixture();
  const data = capture.response.funding.data;
  data.decisionReferences = {
    accounts: [
      { id: "a", name: "Synthetic account A", currency: "CNY" },
      { id: "b", name: "Synthetic account B", currency: "CNY" },
    ],
    assets: [
      { id: "stock-a", name: "Synthetic Equity A", displayCode: "A", currency: "CNY" },
      { id: "stock-b", name: "Synthetic Equity B", displayCode: "B", currency: "CNY" },
    ],
  };
  data.purposes = [
    {
      id: "p",
      name: "Synthetic purpose",
      category: "CUSTOM",
      currency: "CNY",
      targetAmount: null,
      minimumCash: null,
      lossBudget: null,
      singleAssetLimitBps: null,
      isArchived: false,
      version: 1,
    },
  ];
  data.allocation = {
    resources: [
      { kind: "CASH", accountId: "a", currency: "CNY" },
      { kind: "CASH", accountId: "b", currency: "CNY" },
      { kind: "SECURITY", accountId: "a", assetId: "stock-a" },
      { kind: "SECURITY", accountId: "b", assetId: "stock-b" },
    ].map((resource) => ({
      observation: { resource: resource as Resource, sourceVersion: "synthetic" },
      balance: {
        actual: "1000.125",
        allocated: "200",
        unallocated: "800.125",
        excess: "0",
        negativeActual: "0",
      },
      purposes: { p: "200" },
    })),
  };
  capture.response.funding.quality = "PARTIAL";
  capture.response.funding.issues = [
    { code: "MISSING_QUOTE", sourceId: "stock-b", currency: "CNY" },
  ];
  capture.response.recentCashFlow.data.internalCashTransfers = [
    {
      outActivityId: "out",
      occurredAt: "2026-11-02",
      sourceResource: { kind: "CASH", accountId: "a", currency: "CNY" },
      destinationResource: { kind: "CASH", accountId: "b", currency: "USD" },
      sourceAmount: "700.1234",
      destinationAmount: "99.12345",
    },
  ];
  capture.response.recentCashFlow.data.ordinary = {
    currencies: [
      {
        currency: "CNY",
        income: "3200",
        expense: "950.125",
        knownIncome: "3200",
        knownExpense: "950.125",
      },
    ],
  };
  return capture;
}

export function datedContextFixture(): ContextCapture {
  const capture = richContextFixture();
  const old = "2026-07-26";
  for (const block of [
    capture.response.funding,
    capture.response.recentCashFlow,
    capture.response.household,
  ]) {
    block.sourceDataAsOf = old;
    block.stale = true;
  }
  capture.response.household.data.items![0].fxSourceDate = old;
  capture.response.household.data.items![0].stale = true;
  capture.response.household.data.items![0].name = "合成外币资产";
  capture.response.household.data.items![0].category = "properties";
  capture.response.household.data.items![0].basis = "MARKET_QUOTE";
  capture.response.household.data.otherAssets = capture.response.household.data.items![0].value;
  capture.response.household.data.financialAssets = null;
  capture.response.household.data.totalAssets = null;
  capture.response.household.data.netWorth = null;
  capture.response.household.data.items!.push({
    id: "pending-cash",
    name: "合成待更新现金",
    category: "cash",
    currency: "CNY",
    value: null,
    substituteValue: null,
    valuationDate: "2026-11-03",
    fxSourceDate: null,
    stale: false,
    basis: "BALANCE",
    issues: [{ code: "PROJECTION_NOT_CURRENT" }],
  });
  capture.response.household.quality = "PARTIAL";
  capture.response.household.issues.push({
    code: "PROJECTION_NOT_CURRENT",
    sourceId: "a",
    currency: "CNY",
  });
  capture.response.funding.data.risks = [
    {
      purposeId: "p",
      currency: "CNY",
      cashValue: "700",
      minimumCash: "100",
      minimumCashStatus: "AT_OR_ABOVE_MINIMUM",
      singleAssetLimitStatus: "NOT_CONFIGURED",
      lossBudget: null,
      lossComparisonStatus: "NO_SCENARIO_PROVIDED",
      quality: "COMPLETE",
      fxSourceDate: old,
    },
  ];
  capture.response.funding.issues.push({
    code: "PROJECTION_NOT_CURRENT",
    sourceId: "a",
    currency: "CNY",
  });
  capture.response.recentCashFlow.data.convertedOrdinary!.fxSources = [
    { sourceCurrency: "EUR", activityDate: "2026-11-02", sourceDate: old },
  ];
  return capture;
}
