import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";

import { calculateRebalancePlan as calculateTauriRebalancePlan } from "./tauri";
import { invoke } from "./web/core";

const { platformInvokeMock } = vi.hoisted(() => ({
  platformInvokeMock: vi.fn(),
}));

vi.mock("#platform", () => ({ invoke: platformInvokeMock }));

// Adapter tests exercise transport behavior inside an admitted profile.
beforeEach(async () => {
  const { installProfileSession } = await import("@/features/profiles/session");
  installProfileSession({ profileId: "test-profile", scopeId: "test-scope" });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("rebalance eligibility transport", () => {
  it("canonicalizes the Tauri shared request before web transport", async () => {
    const mock = stubFetch({});
    platformInvokeMock
      .mockReset()
      .mockImplementation((command, payload) => invoke(command, payload));

    await calculateTauriRebalancePlan("target-1", 100, { type: "all" }, "cash_flow_only", [
      "asset-z",
      "asset-a",
      "asset-z",
    ]);

    const { body } = lastCall(mock);
    const parsed = JSON.parse(body as string) as { eligibleAssetIds?: unknown };
    expect(parsed.eligibleAssetIds).toEqual(["asset-a", "asset-z"]);
  });

  it("omits the allowlist when no restriction is supplied", async () => {
    const mock = stubFetch({});
    await invoke("calculate_rebalance_plan", {
      targetId: "target-1",
      availableCash: 100,
      filter: { type: "all" },
      scenarioMode: "cash_flow_only",
    });

    const { body } = lastCall(mock);
    expect(JSON.parse(body as string)).not.toHaveProperty("eligibleAssetIds");
  });
});

describe("adapter command parity", () => {
  it("routes allocation drilldown requests with all required filters", async () => {
    const response = new Response(JSON.stringify({ holdings: [] }), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(response);
    vi.stubGlobal("fetch", fetchMock);

    await invoke("get_holdings_by_allocation", {
      filter: { type: "all" },
      taxonomyId: "asset_classes",
      categoryId: "EQUITY",
    });

    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe("/api/v1/allocations/holdings/query");
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body as string)).toEqual({
      filter: { type: "all" },
      taxonomyId: "asset_classes",
      categoryId: "EQUITY",
    });
  });

  it("routes historical exchange-rate batches with the request payload", async () => {
    const response = new Response(JSON.stringify([]), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(response);
    vi.stubGlobal("fetch", fetchMock);
    const request = {
      pairs: [{ fromCurrency: "USD", toCurrency: "EUR", date: "2026-05-18" }],
    };

    await invoke("get_exchange_rates_for_dates", { request });

    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe("/api/v1/exchange-rates/historical");
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body as string)).toEqual(request);
  });
});

// ─── Scope routing coverage ───────────────────────────────────────────────────

function stubFetch(body: unknown = []) {
  const res = new Response(JSON.stringify(body), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
  const mock = vi.fn<typeof fetch>().mockResolvedValue(res);
  vi.stubGlobal("fetch", mock);
  return mock;
}

function lastCall(mock: ReturnType<typeof stubFetch>) {
  const [url, init] = mock.mock.calls[0] as [string, RequestInit];
  return { url, method: (init as RequestInit & { method: string }).method, body: init.body };
}

describe("scope-based routing — get_holdings", () => {
  it("all → POST /holdings/query", async () => {
    const mock = stubFetch();
    await invoke("get_holdings", { filter: { type: "all" } });
    const { url, method, body } = lastCall(mock);
    expect(url).toBe("/api/v1/holdings/query");
    expect(method).toBe("POST");
    expect(JSON.parse(body as string)).toEqual({ filter: { type: "all" } });
  });

  it("account → GET /holdings?accountId=...", async () => {
    const mock = stubFetch();
    await invoke("get_holdings", { filter: { type: "account", accountId: "acc_1" } });
    const { url, method } = lastCall(mock);
    expect(url).toBe("/api/v1/holdings?accountId=acc_1");
    expect(method).toBe("GET");
  });

  it("portfolio → POST /holdings/query", async () => {
    const mock = stubFetch();
    await invoke("get_holdings", { filter: { type: "portfolio", portfolioId: "pf_1" } });
    const { url, method, body } = lastCall(mock);
    expect(url).toBe("/api/v1/holdings/query");
    expect(method).toBe("POST");
    expect(JSON.parse(body as string)).toEqual({
      filter: { type: "portfolio", portfolioId: "pf_1" },
    });
  });

  it("accounts → POST /holdings/query", async () => {
    const mock = stubFetch();
    await invoke("get_holdings", {
      filter: { type: "accounts", accountIds: ["acc_1", "acc_2"] },
    });
    const { url, method, body } = lastCall(mock);
    expect(url).toBe("/api/v1/holdings/query");
    expect(method).toBe("POST");
    expect(JSON.parse(body as string)).toEqual({
      filter: { type: "accounts", accountIds: ["acc_1", "acc_2"] },
    });
  });
});

describe("scope-based routing — get_holdings_list", () => {
  it("all → POST /holdings/list/query", async () => {
    const mock = stubFetch();
    await invoke("get_holdings_list", { filter: { type: "all" } });
    const { url, method, body } = lastCall(mock);
    expect(url).toBe("/api/v1/holdings/list/query");
    expect(method).toBe("POST");
    expect(JSON.parse(body as string)).toEqual({ filter: { type: "all" } });
  });

  it("account → GET /holdings/list?accountId=...", async () => {
    const mock = stubFetch();
    await invoke("get_holdings_list", { filter: { type: "account", accountId: "acc_1" } });
    const { url, method } = lastCall(mock);
    expect(url).toBe("/api/v1/holdings/list?accountId=acc_1");
    expect(method).toBe("GET");
  });

  it("account with closed positions → GET with includeClosed", async () => {
    const mock = stubFetch();
    await invoke("get_holdings_list", {
      filter: { type: "account", accountId: "acc_1" },
      includeClosed: true,
    });
    const { url, method } = lastCall(mock);
    expect(url).toBe("/api/v1/holdings/list?accountId=acc_1&includeClosed=true");
    expect(method).toBe("GET");
  });

  it("portfolio → POST /holdings/list/query", async () => {
    const mock = stubFetch();
    await invoke("get_holdings_list", { filter: { type: "portfolio", portfolioId: "pf_1" } });
    const { url, method, body } = lastCall(mock);
    expect(url).toBe("/api/v1/holdings/list/query");
    expect(method).toBe("POST");
    expect(JSON.parse(body as string)).toEqual({
      filter: { type: "portfolio", portfolioId: "pf_1" },
    });
  });

  it("accounts → POST /holdings/list/query", async () => {
    const mock = stubFetch();
    await invoke("get_holdings_list", {
      filter: { type: "accounts", accountIds: ["acc_1", "acc_2"] },
    });
    const { url, method, body } = lastCall(mock);
    expect(url).toBe("/api/v1/holdings/list/query");
    expect(method).toBe("POST");
    expect(JSON.parse(body as string)).toEqual({
      filter: { type: "accounts", accountIds: ["acc_1", "acc_2"] },
    });
  });

  it("all with closed positions → POST with includeClosed", async () => {
    const mock = stubFetch();
    await invoke("get_holdings_list", {
      filter: { type: "all" },
      includeClosed: true,
    });
    const { url, method, body } = lastCall(mock);
    expect(url).toBe("/api/v1/holdings/list/query");
    expect(method).toBe("POST");
    expect(JSON.parse(body as string)).toEqual({
      filter: { type: "all" },
      includeClosed: true,
    });
  });
});

describe("scope-based routing — get_portfolio_allocations", () => {
  it("all → POST /allocations/query", async () => {
    const mock = stubFetch();
    await invoke("get_portfolio_allocations", { filter: { type: "all" } });
    const { url, method, body } = lastCall(mock);
    expect(url).toBe("/api/v1/allocations/query");
    expect(method).toBe("POST");
    expect(JSON.parse(body as string)).toEqual({ filter: { type: "all" } });
  });

  it("account → GET /allocations?accountId=...", async () => {
    const mock = stubFetch();
    await invoke("get_portfolio_allocations", {
      filter: { type: "account", accountId: "acc_1" },
    });
    const { url, method } = lastCall(mock);
    expect(url).toBe("/api/v1/allocations?accountId=acc_1");
    expect(method).toBe("GET");
  });

  it("portfolio → POST /allocations/query", async () => {
    const mock = stubFetch();
    await invoke("get_portfolio_allocations", {
      filter: { type: "portfolio", portfolioId: "pf_1" },
    });
    const { url, method, body } = lastCall(mock);
    expect(url).toBe("/api/v1/allocations/query");
    expect(method).toBe("POST");
    expect(JSON.parse(body as string)).toEqual({
      filter: { type: "portfolio", portfolioId: "pf_1" },
    });
  });

  it("accounts → POST /allocations/query", async () => {
    const mock = stubFetch();
    await invoke("get_portfolio_allocations", {
      filter: { type: "accounts", accountIds: ["acc_1", "acc_2"] },
    });
    const { url, method, body } = lastCall(mock);
    expect(url).toBe("/api/v1/allocations/query");
    expect(method).toBe("POST");
    expect(JSON.parse(body as string)).toEqual({
      filter: { type: "accounts", accountIds: ["acc_1", "acc_2"] },
    });
  });
});

describe("scope-based routing — get_holdings_by_allocation", () => {
  const drilldown = { taxonomyId: "asset_classes", categoryId: "EQUITY" };

  it("all → POST /allocations/holdings/query", async () => {
    const mock = stubFetch();
    await invoke("get_holdings_by_allocation", { filter: { type: "all" }, ...drilldown });
    const { url, method, body } = lastCall(mock);
    expect(url).toBe("/api/v1/allocations/holdings/query");
    expect(method).toBe("POST");
    expect(JSON.parse(body as string)).toEqual({ filter: { type: "all" }, ...drilldown });
  });

  it("account → GET /allocations/holdings?...", async () => {
    const mock = stubFetch();
    await invoke("get_holdings_by_allocation", {
      filter: { type: "account", accountId: "acc_1" },
      ...drilldown,
    });
    const { url, method } = lastCall(mock);
    expect(url).toBe(
      "/api/v1/allocations/holdings?accountId=acc_1&taxonomyId=asset_classes&categoryId=EQUITY",
    );
    expect(method).toBe("GET");
  });

  it("portfolio → POST /allocations/holdings/query", async () => {
    const mock = stubFetch();
    await invoke("get_holdings_by_allocation", {
      filter: { type: "portfolio", portfolioId: "pf_1" },
      ...drilldown,
    });
    const { url, method, body } = lastCall(mock);
    expect(url).toBe("/api/v1/allocations/holdings/query");
    expect(method).toBe("POST");
    expect(JSON.parse(body as string)).toEqual({
      filter: { type: "portfolio", portfolioId: "pf_1" },
      ...drilldown,
    });
  });

  it("accounts → POST /allocations/holdings/query", async () => {
    const mock = stubFetch();
    await invoke("get_holdings_by_allocation", {
      filter: { type: "accounts", accountIds: ["acc_1", "acc_2"] },
      ...drilldown,
    });
    const { url, method, body } = lastCall(mock);
    expect(url).toBe("/api/v1/allocations/holdings/query");
    expect(method).toBe("POST");
    expect(JSON.parse(body as string)).toEqual({
      filter: { type: "accounts", accountIds: ["acc_1", "acc_2"] },
      ...drilldown,
    });
  });
});

describe("scope-based routing — get_income_summary", () => {
  it("all → POST /income/summary/query", async () => {
    const mock = stubFetch();
    await invoke("get_income_summary", { filter: { type: "all" } });
    const { url, method, body } = lastCall(mock);
    expect(url).toBe("/api/v1/income/summary/query");
    expect(method).toBe("POST");
    expect(JSON.parse(body as string)).toEqual({ filter: { type: "all" } });
  });

  it("account → GET /income/summary?accountId=...", async () => {
    const mock = stubFetch();
    await invoke("get_income_summary", { filter: { type: "account", accountId: "acc_1" } });
    const { url, method } = lastCall(mock);
    expect(url).toBe("/api/v1/income/summary?accountId=acc_1");
    expect(method).toBe("GET");
  });

  it("portfolio → POST /income/summary/query", async () => {
    const mock = stubFetch();
    await invoke("get_income_summary", { filter: { type: "portfolio", portfolioId: "pf_1" } });
    const { url, method, body } = lastCall(mock);
    expect(url).toBe("/api/v1/income/summary/query");
    expect(method).toBe("POST");
    expect(JSON.parse(body as string)).toEqual({
      filter: { type: "portfolio", portfolioId: "pf_1" },
    });
  });

  it("accounts → POST /income/summary/query", async () => {
    const mock = stubFetch();
    await invoke("get_income_summary", {
      filter: { type: "accounts", accountIds: ["acc_1", "acc_2"] },
    });
    const { url, method, body } = lastCall(mock);
    expect(url).toBe("/api/v1/income/summary/query");
    expect(method).toBe("POST");
    expect(JSON.parse(body as string)).toEqual({
      filter: { type: "accounts", accountIds: ["acc_1", "acc_2"] },
    });
  });
});

describe("scope-based routing — performance filters", () => {
  const commands = [
    { command: "calculate_performance_history", path: "/api/v1/performance/history" },
    { command: "calculate_performance_summary", path: "/api/v1/performance/summary" },
  ] as const;
  const scopes = [
    { name: "all", filter: { type: "all" } },
    { name: "portfolio", filter: { type: "portfolio", portfolioId: "pf_1" } },
    { name: "accounts", filter: { type: "accounts", accountIds: ["acc_1", "acc_2"] } },
  ] as const;

  for (const { command, path } of commands) {
    for (const { name, filter } of scopes) {
      it(`${command} sends ${name} filter`, async () => {
        const mock = stubFetch();
        await invoke(command, {
          itemType: "account",
          itemId: "portfolio:all",
          startDate: "2026-01-01",
          endDate: "2026-01-31",
          trackingMode: "TRANSACTIONS",
          filter,
        });

        const { url, method, body } = lastCall(mock);
        expect(url).toBe(path);
        expect(method).toBe("POST");
        expect(JSON.parse(body as string)).toEqual({
          itemType: "account",
          itemId: "portfolio:all",
          startDate: "2026-01-01",
          endDate: "2026-01-31",
          trackingMode: "TRANSACTIONS",
          filter,
        });
      });
    }
  }

  it("get_performance_summaries sends batch scopes", async () => {
    const mock = stubFetch();
    await invoke("get_performance_summaries", {
      scopes: [{ accountIds: ["acc_2", "acc_1"] }],
      startDate: "2026-01-01",
      endDate: "2026-01-31",
      profile: "summary",
    });

    const { url, method, body } = lastCall(mock);
    expect(url).toBe("/api/v1/performance/summaries");
    expect(method).toBe("POST");
    expect(JSON.parse(body as string)).toEqual({
      scopes: [{ accountIds: ["acc_2", "acc_1"] }],
      startDate: "2026-01-01",
      endDate: "2026-01-31",
      profile: "summary",
    });
  });
});
