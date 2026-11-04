import { DecisionDetail } from "./decision-detail";
import { PrincipleLibrary } from "./principle-library";
import { InvestmentCases } from "./investment-cases";
import { blankPrinciple } from "./principle-types";
import { newReviewBody } from "./review-types";
import { beforeEach, expect, it, vi } from "vitest";
import {
  act,
  fireEvent,
  render,
  renderHook,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState, type ReactNode } from "react";
import { MemoryRouter } from "react-router-dom";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { PrivacyContext } from "@/context/privacy-context";
import appI18n from "@/i18n/i18n";
import { InvestmentJournalError } from "@/adapters";
import { JournalLocaleProvider } from "./locale";
import { DecisionForm, type CheckSchedule } from "./decision-form";
import { DecisionEditor } from "./decision-editor";
import { ExactInput, ExactValue } from "./controls";
import { ContextPanel } from "./context-panel";
import InvestmentPage from "./investment-page";
import {
  richContextFixture,
  datedContextFixture,
  contextFixture,
  decisionFixture,
  detailFixture,
  statusFixture,
} from "./test-fixtures";
import type { DecisionBody } from "./types";
import zh from "./locales/zh.json";
import en from "./locales/en.json";
import { useJournalWrite } from "./use-journal-write";
import { updateWriteSession, writeSessionKey } from "./write-session";

const mocks = vi.hoisted(() => ({
  native: vi.fn<(name: string, args?: Record<string, unknown>) => Promise<unknown>>(),
  desktop: true,
  accounts: [] as Record<string, unknown>[],
  assets: [] as Record<string, unknown>[],
}));
vi.mock("@/adapters", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/adapters")>()),
  get isTauri() {
    return mocks.desktop;
  },
  executeInvestmentJournal: mocks.native,
  listenInvestmentIndexUpdate: () => Promise.resolve(() => Promise.resolve()),
  getAccounts: () => Promise.resolve(mocks.accounts),
  getAssets: () => Promise.resolve(mocks.assets),
}));

function Providers({
  children,
  hidden = false,
  initialPath = "/investments?decision=new",
}: {
  children: ReactNode;
  hidden?: boolean;
  initialPath?: string;
}) {
  const [client] = useState(
    () =>
      new QueryClient({
        defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
      }),
  );
  const [privateValues, setPrivate] = useState(hidden);
  return (
    <MemoryRouter initialEntries={[initialPath]}>
      <QueryClientProvider client={client}>
        <PrivacyContext.Provider
          value={{
            isBalanceHidden: privateValues,
            toggleBalanceVisibility: () => setPrivate((v) => !v),
          }}
        >
          <JournalLocaleProvider>{children}</JournalLocaleProvider>
        </PrivacyContext.Provider>
      </QueryClientProvider>
    </MemoryRouter>
  );
}
beforeEach(() => {
  updateWriteSession(writeSessionKey(), {
    frozen: null,
    busy: false,
    uncertain: false,
    error: null,
  });
  Element.prototype.scrollIntoView = vi.fn();
  localStorage.removeItem("investment-journal-language");
  mocks.desktop = true;
  mocks.accounts = [];
  mocks.assets = [];
  mocks.native.mockReset();
  mocks.native.mockImplementation((name) => {
    return Promise.resolve(
      (() => {
        if (name === "get_investment_journal_status") return statusFixture();
        if (name === "preview_investment_decision_context" || name === "get_investment_context")
          return contextFixture();
        if (name.startsWith("list_")) return [];
        if (name === "save_investment_decision_draft")
          return {
            decisionId: "synthetic-decision",
            draftRevisionId: "synthetic-draft",
            version: 3,
            requestId: "synthetic-request",
            replayed: false,
          };
        if (name === "preview_investment_decision_confirmation")
          return {
            ledgerId: "synthetic-ledger",
            target: {
              decisionId: "synthetic-decision",
              draftRevisionId: "synthetic-draft",
              expectedVersion: 3,
            },
            revisionNumber: 1,
            body: decisionFixture(),
            nextCheckAt: "2027-01-08",
            expectedContentHash: "shown-body",
            expectedContextHash: "shown-context",
            context: contextFixture(),
          };
        if (name === "confirm_investment_decision")
          return {
            decisionId: "synthetic-decision",
            version: 4,
            requestId: "synthetic-confirm",
            replayed: false,
          };
        return {};
      })(),
    );
  });
});

it("ordinary draft saving keeps the check date and optional unknowns; date edits are explicit", async () => {
  const save = vi.fn<(body: DecisionBody, schedule: CheckSchedule, preview: boolean) => void>();
  render(
    <Providers>
      <DecisionForm
        initial={decisionFixture()}
        nextCheckAt="2027-01-08"
        busy={false}
        locked={false}
        accounts={[]}
        assets={[]}
        purposes={[]}
        principles={[]}
        tags={[]}
        changed={vi.fn()}
        save={save}
      />
    </Providers>,
  );
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "保存草稿" }));
  expect(save.mock.lastCall?.[1]).toEqual({ operation: "KEEP" });
  expect(save.mock.lastCall?.[0].waitingCost).toBeNull();
  expect(save.mock.lastCall?.[0].informationValue).toBeNull();
  fireEvent.change(screen.getByLabelText("下次检查草稿（可选）"), {
    target: { value: "2027-02-03" },
  });
  await user.click(screen.getByRole("button", { name: "保存草稿" }));
  expect(save.mock.lastCall?.[1]).toEqual({ operation: "SET", date: "2027-02-03" });
  fireEvent.change(screen.getByLabelText("下次检查草稿（可选）"), { target: { value: "" } });
  await user.click(screen.getByRole("button", { name: "保存草稿" }));
  expect(save.mock.lastCall?.[1]).toEqual({ operation: "CANCEL" });
});

it("privacy removes structured values from inputs, visible text and accessibility attributes", () => {
  const value = "9007199254740993.1234";
  const { container } = render(
    <Providers hidden>
      <ExactInput value={value} title={value} aria-valuetext={value} />
      <ExactValue value={value} />
    </Providers>,
  );
  expect(screen.getByRole("textbox")).toHaveValue("");
  expect(screen.getByRole("textbox")).toHaveAttribute("readonly");
  expect(container.innerHTML).not.toContain(value);
  expect(container.innerHTML).not.toContain("9007199254740993");
});

it("confirmation uses the exact displayed native target, body hash and context snapshot", async () => {
  const done = vi.fn();
  const detail = detailFixture();
  render(
    <Providers>
      <DecisionEditor detail={detail} draft={detail.revisions[0]} completed={done} />
    </Providers>,
  );
  await userEvent.setup().click(screen.getByRole("button", { name: "保存并预览确认" }));
  await screen.findByRole("heading", { name: /确认这个具体版本/ });
  await userEvent.setup().click(screen.getByRole("button", { name: "确认决策" }));
  await waitFor(() => expect(done).toHaveBeenCalledWith("synthetic-decision"));
  const confirmation = mocks.native.mock.calls.find(
    ([name]) => name === "confirm_investment_decision",
  )?.[1];
  expect(confirmation).toMatchObject({
    ledgerId: "synthetic-ledger",
    target: {
      decisionId: "synthetic-decision",
      draftRevisionId: "synthetic-draft",
      expectedVersion: 3,
    },
    expectedContentHash: "shown-body",
    expectedContextHash: "shown-context",
    context: contextFixture(),
  });
  expect(confirmation).not.toHaveProperty("body");
});

it("an unknown save locks editing and explicitly replays the same request", async () => {
  const normal = mocks.native.getMockImplementation()!;
  let attempts = 0;
  mocks.native.mockImplementation(async (name, args) => {
    if (name === "save_investment_decision_draft" && attempts++ === 0)
      throw new InvestmentJournalError("unknown_result", "unknown");
    return normal(name, args);
  });
  const detail = detailFixture();
  render(
    <Providers>
      <DecisionEditor detail={detail} draft={detail.revisions[0]} completed={vi.fn()} />
    </Providers>,
  );
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "保存草稿" }));
  await screen.findByRole("button", { name: "重放同一请求" });
  expect(screen.getByRole("button", { name: "保存草稿" })).toBeDisabled();
  await user.click(screen.getByRole("button", { name: "重放同一请求" }));
  await screen.findByText("草稿已保存");
  const writes = mocks.native.mock.calls.filter(
    ([name]) => name === "save_investment_decision_draft",
  );
  expect(writes).toHaveLength(2);
  expect(writes[0][1]).toEqual(writes[1][1]);
});

it("module English is reachable without changing the rest of the Chinese application", async () => {
  expect(Object.keys(en).sort()).toEqual(Object.keys(zh).sort());
  render(
    <Providers>
      <InvestmentPage />
    </Providers>,
  );
  const language = await screen.findByLabelText("投资界面语言");
  await userEvent.setup().selectOptions(language, "en");
  expect(screen.getByRole("heading", { name: "Investments" })).toBeVisible();
  expect(screen.getByRole("button", { name: "Save draft" })).toBeVisible();
  expect(appI18n.language).toBe("zh");
});

it("removing evidence keeps the next row's probability attached to that row", async () => {
  const body = decisionFixture();
  body.options = [];
  body.evidence = [25, 40].map((percent) => ({
    informationKind: "HYPOTHESIS",
    content: "Synthetic scenario",
    source: null,
    probabilityLowerBps: percent * 100,
    probabilityUpperBps: percent * 100,
    probabilityBasis: "Synthetic basis",
    purposeId: null,
    assumedLossAmount: null,
    lossCurrency: null,
    lossBasis: null,
  }));
  render(
    <Providers>
      <DecisionForm
        initial={body}
        nextCheckAt={null}
        busy={false}
        locked={false}
        accounts={[]}
        assets={[]}
        purposes={[]}
        principles={[]}
        tags={[]}
        changed={vi.fn()}
        save={vi.fn()}
      />
    </Providers>,
  );
  await userEvent.setup().click(screen.getAllByRole("button", { name: "移除" })[0]);
  expect(screen.getAllByLabelText("概率下限 %（可选）")[0]).toHaveValue("40");
});

it("unverified comparisons stay unknown and never display a pass or a fabricated zero", () => {
  const context = contextFixture();
  context.response.funding.data.purposes = [
    {
      id: "purpose",
      name: "Synthetic purpose",
      category: "CUSTOM",
      currency: "CNY",
      targetAmount: null,
      minimumCash: "100",
      lossBudget: "50",
      singleAssetLimitBps: 5000,
      isArchived: false,
      version: 1,
    },
  ];
  context.response.funding.data.risks = [
    {
      purposeId: "purpose",
      currency: "CNY",
      cashValue: null,
      minimumCash: "100",
      minimumCashStatus: "UNABLE_TO_DETERMINE",
      singleAssetLimitStatus: "UNABLE_TO_DETERMINE",
      lossBudget: "50",
      lossComparisonStatus: "NO_SCENARIO_PROVIDED",
      quality: "PARTIAL",
    },
  ];
  context.response.funding.data.decisionAssessment = {
    plannedFunding: [
      {
        purposeId: "purpose",
        resource: { kind: "CASH", accountId: "synthetic", currency: "CNY" },
        planned: "12.345678901234",
        purposeAmountBefore: null,
        purposeAmountAfter: null,
        shortfall: null,
        unallocatedAvailable: null,
        status: "UNABLE_TO_DETERMINE",
      },
    ],
    lossScenarios: [
      {
        purposeId: "purpose",
        currency: "CNY",
        lossBudget: "50",
        assumedLossAmount: null,
        hypothesisBasis: null,
        status: "NO_SCENARIO_PROVIDED",
      },
    ],
  };
  render(
    <Providers>
      <ContextPanel capture={context} />
    </Providers>,
  );
  expect(screen.getAllByText(/无法判断/).length).toBeGreaterThan(0);
  expect(screen.getByText("尚未填写比较情景")).toBeVisible();
  expect(screen.queryByText("计划在当前用途持有量内")).not.toBeInTheDocument();
  expect(screen.queryByText("所填情景金额未超过预算")).not.toBeInTheDocument();
});

it("one failed native block preserves known siblings and does not fabricate zero counts", () => {
  const context = contextFixture();
  context.response.funding.quality = "UNAVAILABLE";
  context.response.funding.data = {};
  context.response.recentCashFlow.quality = "UNAVAILABLE";
  context.response.recentCashFlow.data = {};
  render(
    <Providers>
      <ContextPanel capture={context} />
    </Providers>,
  );
  expect(screen.getAllByText(zh.unavailable).length).toBeGreaterThan(1);
  for (const label of [zh.total_assets, zh.net_worth]) {
    expect(screen.getByText(label).parentElement).toHaveTextContent("9,007,199,254,741,003.1234");
  }
  expect(
    screen.getByText(new RegExp(zh.internal_transfers + ": " + zh.unavailable)),
  ).toBeInTheDocument();
});

it("preview return retains the saved body, tags and check date", async () => {
  const original = mocks.native.getMockImplementation()!;
  let saved = decisionFixture();
  let date = "2027-01-08";
  mocks.native.mockImplementation((name, args) => {
    if (name === "save_investment_decision_draft") {
      saved = structuredClone(args?.body as DecisionBody);
      const schedule = args?.checkSchedule as CheckSchedule;
      if (schedule.operation === "SET") date = schedule.date;
    }
    if (name === "preview_investment_decision_confirmation")
      return original(name, args).then((value) => ({
        ...(value as object),
        body: saved,
        nextCheckAt: date,
      }));
    return original(name, args);
  });
  const detail = detailFixture();
  render(
    <Providers>
      <DecisionEditor detail={detail} draft={detail.revisions[0]} completed={vi.fn()} />
    </Providers>,
  );
  fireEvent.change(screen.getByLabelText(zh.title), {
    target: { value: "Unsaved thought retained" },
  });
  fireEvent.change(screen.getByLabelText(zh.scenario_tags), { target: { value: "Alpha, Beta" } });
  fireEvent.change(screen.getByLabelText(zh.next_check), { target: { value: "2027-02-10" } });
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: zh.save_preview }));
  await screen.findByRole("heading", { name: /确认这个具体版本/ });
  await user.click(screen.getByRole("button", { name: zh.return_edit }));
  expect(screen.getByLabelText(zh.title)).toHaveValue("Unsaved thought retained");
  expect(screen.getByLabelText(zh.scenario_tags)).toHaveValue("Alpha, Beta");
  expect(screen.getByLabelText(zh.next_check)).toHaveValue("2027-02-10");
  await user.click(screen.getByRole("button", { name: zh.save_draft }));
  expect(
    mocks.native.mock.calls
      .filter(([name]) => name === "save_investment_decision_draft")
      .at(-1)?.[1]?.checkSchedule,
  ).toEqual({ operation: "KEEP" });
});

it("an unresolved write survives its editor unmount and is replayed by a fresh owner", async () => {
  let attempts = 0;
  mocks.native.mockImplementation(() =>
    ++attempts === 1
      ? Promise.reject(new InvestmentJournalError("unknown_result", "unknown"))
      : Promise.resolve({ decisionId: "recovered", replayed: true }),
  );
  const done = vi.fn();
  const first = renderHook(() => useJournalWrite());
  await act(() =>
    first.result.current.run(
      "save_investment_decision_draft",
      { body: decisionFixture(), requestKey: "frozen-after-unmount" },
      done,
    ),
  );
  expect(first.result.current.uncertain).toBe(true);
  first.unmount();
  const next = renderHook(() => useJournalWrite());
  expect(next.result.current.uncertain).toBe(true);
  await act(() => next.result.current.retry());
  expect(mocks.native.mock.calls[0][1]).toEqual(mocks.native.mock.calls[1][1]);
  expect(done).not.toHaveBeenCalled();
  expect(next.result.current.uncertain).toBe(false);
});

it("amendment saves keep their original optimistic version when parent metadata changes", async () => {
  const detail = detailFixture();
  detail.currentRevisionId = detail.draftRevisionId;
  detail.draftRevisionId = null;
  detail.revisions[0].status = "CONFIRMED";
  const view = render(
    <Providers>
      <DecisionEditor detail={detail} completed={vi.fn()} />
    </Providers>,
  );
  view.rerender(
    <Providers>
      <DecisionEditor detail={{ ...detail, version: 99 }} completed={vi.fn()} />
    </Providers>,
  );
  await userEvent.setup().click(screen.getByRole("button", { name: zh.save_draft }));
  await waitFor(() =>
    expect(
      mocks.native.mock.calls.some(([name]) => name === "save_investment_decision_draft"),
    ).toBe(true),
  );
  expect(
    mocks.native.mock.calls.find(([name]) => name === "save_investment_decision_draft")?.[1]
      ?.expectedVersion,
  ).toBe(2);
});

it("custom confirmation errors focus and describe their relevant input", async () => {
  const body = decisionFixture();
  body.action = null;
  render(
    <Providers>
      <DecisionForm
        initial={body}
        nextCheckAt={null}
        busy={false}
        locked={false}
        accounts={[]}
        assets={[]}
        purposes={[]}
        principles={[]}
        tags={[]}
        changed={vi.fn()}
        save={vi.fn()}
      />
    </Providers>,
  );
  await userEvent.setup().click(screen.getByRole("button", { name: zh.save_preview }));
  const action = screen.getByLabelText(zh.action);
  expect(action).toHaveFocus();
  expect(action).toHaveAttribute("aria-invalid", "true");
  expect(action.getAttribute("aria-describedby")).toBe(screen.getByRole("alert").id);
  expect(screen.getByLabelText(zh.next_check)).toHaveAccessibleDescription(zh.check_hint);
});

it("resource names, diagnostic reasons and original-currency transfer amounts are inspectable", async () => {
  render(
    <Providers>
      <ContextPanel capture={richContextFixture()} />
    </Providers>,
  );
  expect(screen.getByText("Synthetic account A \u00b7 CNY \u73b0\u91d1")).toBeInTheDocument();
  expect(
    screen.getByText(
      "Synthetic account B \u00b7 Synthetic Equity B \u00b7 \u8bc1\u5238\u6570\u91cf",
    ),
  ).toBeInTheDocument();
  await userEvent.setup().click(screen.getByText(zh.quality_issues.replace("{{count}}", "1")));
  expect(screen.getByText(new RegExp(zh.issue_quote))).toBeVisible();
  const details = screen.getAllByText(zh.details);
  await userEvent.setup().click(details[0]);
  expect(screen.getByText("700.1234")).toBeVisible();
  expect(screen.getByText("99.12345")).toBeVisible();
});

it("shows exact account values and grouped purpose flows in the frozen context under privacy", async () => {
  const capture = richContextFixture();
  capture.response.funding.data.accountValuations = {
    accounts: [
      {
        accountId: "a",
        baseCurrency: "CNY",
        cashBalanceBase: "5801.123456",
        investmentMarketValueBase: null,
        sourceDataAsOf: "2026-10-12",
        calculatedAt: "2026-11-03T01:23:45Z",
        stale: true,
        issues: [{ code: "MISSING_QUOTE", assetId: "stock-a", currency: "CNY" }],
      },
    ],
  };
  const row = {
    operationId: "synthetic-operation",
    occurredOn: "2026-11-01",
    entry: {
      resource: { kind: "CASH" as const, accountId: "a", currency: "CNY" },
      delta: "61.234567",
      purposeId: "p",
    },
  };
  capture.response.recentCashFlow.data.purposeFlows = {
    allocationIn: [row],
    allocationOut: [{ ...row, entry: { ...row.entry, delta: "-7.123456" } }],
    tradeConversions: [{ ...row, entry: { ...row.entry, delta: "-12.654321" } }],
    adjustments: [],
  };
  const show = render(
    <Providers>
      <ContextPanel capture={capture} frozen />
    </Providers>,
  );
  const account = within(
    screen.getByRole("heading", { name: "Synthetic account A估值" }).parentElement!,
  );
  expect(account.getByText("5,801.123456")).toBeVisible();
  expect(account.getByText(zh.unavailable)).toBeVisible();
  expect(account.getByText(/2026-10-12/)).toHaveTextContent(zh.stale);
  expect(
    account.getByText(zh.valuation_calculated_at.replace("{{date}}", "2026-11-03T01:23:45Z")),
  ).toBeVisible();
  expect(screen.getByText(zh.total_assets)).toBeVisible();
  show.unmount();
  const hidden = render(
    <Providers hidden>
      <ContextPanel capture={capture} frozen />
    </Providers>,
  );
  await userEvent.click(screen.getAllByText(zh.details)[0]);
  for (const label of [zh.purpose_flow_in, zh.purpose_flow_out, zh.purpose_flow_trades]) {
    const group = screen.getByRole("heading", { name: label }).closest("section")!;
    expect(within(group).getByText(/Synthetic purpose/)).toBeVisible();
  }
  expect(hidden.container.innerHTML).not.toMatch(/5,?801\.123456|61\.234567|7\.123456|12\.654321/);
});

it("pages 1,024 historical trades and takes the keyboard-selected source to the exact decision", async () => {
  const detail = detailFixture();
  const confirmed = {
    ...detail.revisions[0],
    id: "confirmed",
    revisionNumber: 1,
    status: "CONFIRMED",
  };
  Object.assign(detail, {
    lifecycle: "CONFIRMED",
    currentRevisionId: confirmed.id,
    currentRevisionNumber: 1,
    draftRevisionId: null,
    revisions: [confirmed],
  });
  mocks.accounts = [
    { id: "a", name: "Synthetic primary" },
    { id: "b", name: "Synthetic secondary" },
  ];
  mocks.assets = [{ id: "equity", name: "Synthetic equity", displayCode: "SYN" }];
  const items = Array.from({ length: 1024 }, (_, index) => ({
    id: `item-${index}`,
    kind: "UNLINKED_EXECUTION",
    execution: {
      id: `execution-${index}`,
      accountId: index === 1023 ? "b" : "a",
      assetId: "equity",
      side: "BUY",
      occurredAt: "2026-10-01",
      sourceId: `synthetic-source-${index}`,
      quantity: "1",
    },
  }));
  const fallback = mocks.native.getMockImplementation()!;
  mocks.native.mockImplementation(async (name, args) => {
    if (name === "get_investment_work_queue")
      return {
        complete: true,
        asOfDate: "2026-11-03",
        items: args?.includeHistoricalExecutions ? items : [],
      };
    if (name === "list_investment_decisions")
      return [{ ...detail, title: "Synthetic target decision" }];
    if (name === "get_investment_decision") return detail;
    return fallback(name, args);
  });
  render(
    <Providers initialPath="/investments?view=queue">
      <InvestmentPage />
    </Providers>,
  );
  const user = userEvent.setup();
  await user.click(await screen.findByLabelText(zh.execution_include_history));
  await screen.findByText("第 1／41 页 · 1024 条结果");
  expect(screen.getAllByRole("button", { name: zh.queue_pick_decision })).toHaveLength(25);
  await user.click(screen.getByRole("button", { name: zh.next }));
  await screen.findByText("第 2／41 页 · 1024 条结果");
  expect(screen.getByText(/synthetic-source-25$/)).toBeVisible();
  await user.type(screen.getByRole("searchbox", { name: zh.queue_filter }), "Synthetic secondary");
  expect(screen.getAllByRole("button", { name: zh.queue_pick_decision })).toHaveLength(1);
  await user.tab();
  await user.tab();
  const choose = screen.getByRole("button", { name: zh.queue_pick_decision });
  expect(choose).toHaveFocus();
  expect(choose).toHaveAccessibleDescription(/SYN.*Synthetic secondary.*synthetic-source-1023/);
  await user.keyboard("{Enter}");
  await user.click(await screen.findByRole("button", { name: /Synthetic target decision/ }));
  expect(await screen.findByLabelText(zh.existing_execution)).toHaveValue("execution-1023");
});

for (const language of ["zh", "en"] as const) {
  it(`keeps FX dates distinct from snapshot and activity dates under privacy in ${language}`, async () => {
    localStorage.setItem("investment-journal-language", JSON.stringify(language));
    const copy = language === "zh" ? zh : en;
    const view = render(
      <Providers hidden>
        <ContextPanel capture={datedContextFixture()} />
      </Providers>,
    );
    for (const details of screen.getAllByText(copy.details)) await userEvent.click(details);
    expect(
      screen.getAllByText(copy.fx_source_date.replace("{{date}}", "2026-07-26")).length,
    ).toBeGreaterThanOrEqual(2);
    expect(screen.getByText(copy.dated_fx_sources)).toBeVisible();
    expect(
      screen.getByText(new RegExp(copy.activity_day.replace("{{date}}", "2026-11-02"))),
    ).toBeVisible();
    await userEvent.click(screen.getByText(copy.quality_issues.replace("{{count}}", "2")));
    expect(screen.getAllByText(new RegExp(copy.issue_projection))[0]).toBeVisible();
    expect(view.container.innerHTML).not.toMatch(
      /9007199254740993|9,007,199,254,740,993|700\.1234|99\.12345/,
    );
  });
}

it("live context can refresh after a read error while a frozen context exposes no refresh control", async () => {
  const retry = vi.fn();
  const view = render(
    <Providers>
      <ContextPanel capture={contextFixture()} error retry={retry} />
    </Providers>,
  );
  expect(screen.getByRole("alert")).toHaveTextContent(zh.context_error);
  await userEvent.click(screen.getByRole("button", { name: zh.refresh_context }));
  expect(retry).toHaveBeenCalledTimes(1);
  view.rerender(
    <Providers>
      <ContextPanel capture={contextFixture()} frozen retry={retry} />
    </Providers>,
  );
  expect(screen.queryByRole("button", { name: zh.refresh_context })).not.toBeInTheDocument();
});

it("first amendment save keeps the preview owner alive across detail refetch", async () => {
  const normal = mocks.native.getMockImplementation()!;
  const model = detailFixture();
  model.currentRevisionId = "confirmed";
  model.draftRevisionId = null;
  model.lifecycle = "CONFIRMED";
  model.revisions[0].id = "confirmed";
  model.revisions[0].status = "CONFIRMED";
  model.revisions[0].context = contextFixture();
  let finishPreview: (value: unknown) => void = () => {
    throw new Error("preview not started");
  };
  const preview = new Promise((resolve) => {
    finishPreview = resolve;
  });
  mocks.native.mockImplementation((name, args) => {
    if (name === "get_investment_decision") return Promise.resolve(structuredClone(model));
    if (name === "save_investment_decision_draft") {
      model.draftRevisionId = "synthetic-draft";
      model.version = 3;
      model.revisions.push({
        ...model.revisions[0],
        id: "synthetic-draft",
        revisionNumber: 2,
        status: "DRAFT",
        body: args?.body as DecisionBody,
        context: null,
      });
    }
    if (name === "preview_investment_decision_confirmation") return preview;
    return normal(name, args);
  });
  render(
    <Providers>
      <DecisionDetail id="synthetic-decision" />
    </Providers>,
  );
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: zh.amend }));
  await user.click(screen.getByRole("button", { name: zh.save_preview }));
  await waitFor(() =>
    expect(
      mocks.native.mock.calls.filter(([name]) => name === "get_investment_decision").length,
    ).toBeGreaterThan(1),
  );
  await act(async () => {
    finishPreview(await normal("preview_investment_decision_confirmation", {}));
  });
  expect(await screen.findByRole("heading", { name: /确认这个具体版本/ })).toBeVisible();
});

it("reads each alternative's original reason in the confirmed decision detail", async () => {
  const detail = detailFixture();
  const revision = detail.revisions[0];
  const providedReason = "用户希望保留流动资金";
  revision.body.options.push({
    ...revision.body.options[0],
    action: "分批买入",
    notSelectedReason: providedReason,
  });
  revision.status = "CONFIRMED";
  Object.assign(detail, {
    lifecycle: "CONFIRMED",
    currentRevisionId: revision.id,
    currentRevisionNumber: revision.revisionNumber,
    draftRevisionId: null,
    draftRevisionNumber: null,
  });
  const fallback = mocks.native.getMockImplementation()!;
  mocks.native.mockImplementation(async (name, args) =>
    name === "get_investment_decision" ? detail : fallback(name, args),
  );
  render(
    <Providers>
      <DecisionDetail id={detail.decisionId} />
    </Providers>,
  );

  for (const [action, expected] of [
    ["立即买入", zh.not_recorded],
    ["分批买入", providedReason],
  ]) {
    const option = (await screen.findByText(action, { selector: "dt" })).closest("dl")!;
    const reasonLabel = within(option).getByText(zh.not_selected_reason);
    expect(reasonLabel.nextElementSibling).toBeVisible();
    expect(reasonLabel.nextElementSibling?.textContent).toBe(expected);
  }
});

it("alternative recovery describes Add when empty and marks its first blank action", async () => {
  const body = decisionFixture();
  body.options = [];
  render(
    <Providers>
      <DecisionForm
        initial={body}
        nextCheckAt={null}
        busy={false}
        locked={false}
        accounts={[]}
        assets={[]}
        purposes={[]}
        principles={[]}
        tags={[]}
        changed={vi.fn()}
        save={vi.fn()}
      />
    </Providers>,
  );
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: zh.save_preview }));
  const add = screen.getByRole("button", { name: zh.add_alternative });
  expect(add).toHaveFocus();
  expect(add.getAttribute("aria-describedby")).toBe(screen.getByRole("alert").id);
  await user.click(add);
  await user.click(screen.getByRole("button", { name: zh.save_preview }));
  const action = screen.getByLabelText(zh.alternative_action);
  expect(action).toHaveFocus();
  expect(action).toHaveAttribute("aria-invalid", "true");
  expect(action.getAttribute("aria-describedby")).toBe(screen.getByRole("alert").id);
});

it("records a ledger trade against the confirmed version and replays the frozen fact-only request", async () => {
  const detail = detailFixture();
  const body = { ...decisionFixture(), action: "BUY" as const, assetId: "stock" };
  Object.assign(detail, {
    lifecycle: "CONFIRMED",
    executionState: "OPEN",
    currentRevisionId: "confirmed",
    currentRevisionNumber: 1,
    draftRevisionNumber: 2,
    revisions: [
      { ...detail.revisions[0], id: "confirmed", status: "CONFIRMED", body },
      { ...detail.revisions[0], revisionNumber: 2 },
    ],
  });
  const progress = {
    quantityForPlannedAsset: null,
    settlementCashForPlanCurrency: null,
    remainingQuantity: null,
    remainingCashAmount: null,
    quantityUnitStatus: "UNVERIFIED_CONTEXT",
  };
  mocks.accounts = [
    {
      id: "account",
      name: "Synthetic securities",
      accountType: "SECURITIES",
      trackingMode: "TRANSACTIONS",
      currency: "CNY",
      isActive: true,
    },
  ];
  mocks.assets = [
    { id: "stock", kind: "INVESTMENT", instrumentType: "EQUITY", name: "Synthetic equity" },
  ];
  const preview = {
    schemaVersion: 1,
    previewHash: "frozen-preview",
    ownershipMode: "RECONCILE_LATER",
    decision: { id: "confirmed", revisionNumber: 1, body, progress },
    trade: {
      automaticAllocationEligible: false,
      activityCurrency: "CNY",
      settlementCurrency: null,
      netCash: null,
      fee: "0",
      tax: null,
      cashAdjustment: null,
      changes: [],
      issues: ["MissingFinalCash"],
    },
    funding: null,
    ownership: null,
    pendingReconciliation: [],
  };
  const fallback = mocks.native.getMockImplementation()!;
  let attempts = 0;
  mocks.native.mockImplementation(async (command, args) => {
    switch (command) {
      case "get_investment_decision":
        return detail;
      case "get_investment_execution_progress":
        return progress;
      case "preview_decision_ledger_trade":
        return preview;
      case "record_decision_ledger_trade":
        if (++attempts === 1)
          throw new InvestmentJournalError("unknown_result", "Synthetic response lost");
        return { decisionRevisionId: "confirmed", ownershipStatus: "RECONCILIATION_REQUIRED" };
      default:
        return fallback(command, args);
    }
  });
  render(
    <Providers>
      <DecisionDetail id={detail.decisionId} />
    </Providers>,
  );
  const user = userEvent.setup();
  await user.click(
    await screen.findByRole("button", {
      name: zh.record_confirmed_trade.replace("{{number}}", "1"),
    }),
  );
  await user.selectOptions(await screen.findByLabelText(zh.trade_account), "account");
  await user.click(screen.getByRole("button", { name: zh.continue_trade }));
  const quantity = "12345.0000000000001";
  fireEvent.change(await screen.findByLabelText(zh.actual_quantity), {
    target: { value: quantity },
  });
  fireEvent.change(screen.getByLabelText(zh.trade_fee), { target: { value: "0" } });
  fireEvent.change(screen.getByLabelText(zh.operation_reason), {
    target: { value: "Synthetic evidence" },
  });
  await user.click(screen.getByRole("button", { name: zh.preview_actual_trade }));
  expect(await screen.findByRole("radio", { name: zh.apply_trade_ownership })).toBeDisabled();
  expect(screen.getByRole("button", { name: zh.review_trade_submission })).toBeDisabled();
  await user.click(screen.getByRole("radio", { name: zh.defer_trade_ownership }));
  await user.click(screen.getByRole("button", { name: zh.review_trade_submission }));
  expect(mocks.native.mock.calls.filter(([name]) => name.startsWith("record_"))).toHaveLength(0);
  await user.click(screen.getByRole("button", { name: zh.submit_actual_trade }));
  await user.click(await screen.findByRole("button", { name: zh.replay }));
  await screen.findByText(zh.trade_recorded.replace("{{number}}", "1"));
  const writes = mocks.native.mock.calls.filter(
    ([name]) => name === "record_decision_ledger_trade",
  );
  expect(writes).toHaveLength(2);
  expect(writes[0][1]).toEqual(writes[1][1]);
  expect(writes[0][1]).toMatchObject({
    decision: { decisionId: detail.decisionId, revisionId: "confirmed", deviationReason: null },
    trade: {
      accountId: "account",
      assetId: "stock",
      quantity,
      fee: "0",
      tax: null,
      amount: null,
      fxRate: null,
    },
    ownership: { mode: "RECONCILE_LATER" },
    expectedPreviewHash: "frozen-preview",
    reason: "Synthetic evidence",
  });
});

it("links existing evidence to the selected confirmed version, then unlinks by the recorded timestamp", async () => {
  const detail = detailFixture();
  const body = { ...decisionFixture(), action: "BUY" as const, assetId: "stock" };
  const revision = {
    ...detail.revisions[0],
    id: "confirmed",
    revisionNumber: 1,
    status: "CONFIRMED",
    body,
  };
  Object.assign(detail, {
    lifecycle: "CONFIRMED",
    executionState: "OPEN",
    currentRevisionId: "confirmed",
    currentRevisionNumber: 1,
    draftRevisionNumber: 2,
    revisions: [revision, { ...detail.revisions[0], revisionNumber: 2 }],
  });
  const execution = {
    id: "existing",
    sourceId: "synthetic-activity",
    accountId: "a",
    assetId: "stock",
    side: "BUY",
    occurredAt: "2026-11-03",
    quantity: "1.000000000001",
    netCash: null,
    settlementCurrency: null,
    activityCurrency: "CNY",
    fee: "0",
    tax: null,
  };
  const snapshot = {
    execution,
    decision: { id: "confirmed", revisionNumber: 1, confirmedAt: "2026-11-03T12:00:00Z" },
  };
  const linkedAt = "2026-11-04T01:00:00Z";
  const evidence = { snapshot, temporalClassification: "UNDETERMINED", sourceVersion: "source-v1" };
  let linkAttempts = 0;
  let unlinkAttempts = 0;
  let links: Record<string, unknown>[] = [];
  const fallback = mocks.native.getMockImplementation()!;
  mocks.native.mockImplementation(async (command, args) => {
    switch (command) {
      case "get_investment_decision":
        return detail;
      case "list_investment_execution_links":
        return links;
      case "get_investment_work_queue":
        return {
          complete: true,
          items: args?.includeHistoricalExecutions
            ? [{ id: "unlinked", kind: "UNLINKED_EXECUTION", execution }]
            : [],
        };
      case "preview_investment_execution_link":
        return { ...evidence, target: args, previewHash: "link-proof" };
      case "link_investment_execution":
        if (linkAttempts++ === 0)
          throw new InvestmentJournalError("unknown_result", "Synthetic response lost");
        links = [
          {
            ...evidence,
            id: "link",
            revisionNumber: 1,
            linkedAt,
            unlinkedAt: null,
            deviationReason: "Synthetic deviation",
          },
        ];
        return {};
      case "unlink_investment_execution":
        if (unlinkAttempts++ === 0)
          throw new InvestmentJournalError("state_conflict", "Synthetic stale write");
        links = [{ ...links[0], unlinkedAt: "2026-11-04T02:00:00Z", unlinkReason: args?.reason }];
        return {};
      default:
        return fallback(command, args);
    }
  });
  render(
    <Providers>
      <DecisionDetail id={detail.decisionId} />
    </Providers>,
  );
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: zh.link_execution }));
  await screen.findByText(zh.no_execution_candidates);
  await user.click(screen.getByLabelText(zh.execution_include_history));
  await screen.findByRole("option", { name: /synthetic-activity/ });
  await user.selectOptions(screen.getByLabelText(zh.existing_execution), "existing");
  fireEvent.change(screen.getByLabelText(zh.deviation_reason), {
    target: { value: "Synthetic deviation" },
  });
  await user.click(screen.getByRole("button", { name: zh.review_execution_link }));
  await screen.findByText(new RegExp(zh.execution_timing_unknown));
  expect(mocks.native.mock.calls.some(([command]) => command === "link_investment_execution")).toBe(
    false,
  );
  const linkButton = screen.getByRole("button", { name: zh.submit_execution_link });
  const section = within(linkButton.closest("section")!);
  await user.click(linkButton);
  await user.click(await section.findByRole("button", { name: zh.replay }));
  const linkWrites = mocks.native.mock.calls.filter(
    ([command]) => command === "link_investment_execution",
  );
  expect(linkWrites).toHaveLength(2);
  expect(linkWrites[0][1]).toEqual(linkWrites[1][1]);
  await user.click(await screen.findByRole("button", { name: zh.unlink_execution }));
  await screen.findByText(zh.unlink_execution_hint);
  fireEvent.change(screen.getByLabelText(zh.operation_reason), {
    target: { value: "Synthetic reason" },
  });
  await user.click(screen.getByRole("button", { name: zh.unlink_execution }));
  await section.findByText(zh.error_conflict);
  expect(screen.getAllByText(zh.error_conflict)).toHaveLength(1);
  await user.click(screen.getByRole("button", { name: zh.execution_reload_link }));
  await waitFor(() =>
    expect(screen.getByRole("button", { name: zh.unlink_execution })).toBeEnabled(),
  );
  fireEvent.change(screen.getByLabelText(zh.operation_reason), {
    target: { value: "Synthetic reason updated" },
  });
  await user.click(screen.getByRole("button", { name: zh.unlink_execution }));
  await screen.findByText(/Synthetic reason updated/);
  const writes = mocks.native.mock.calls;
  expect(writes.find(([command]) => command === "link_investment_execution")?.[1]).toMatchObject({
    target: {
      decisionRevisionId: "confirmed",
      executionId: "existing",
      deviationReason: "Synthetic deviation",
    },
    expectedPreviewHash: "link-proof",
  });
  expect(writes.find(([command]) => command === "unlink_investment_execution")?.[1]).toMatchObject({
    linkId: "link",
    expectedLinkedAt: linkedAt,
    reason: "Synthetic reason",
  });
});

it("saves a cutoff-bound review draft, then submits only the frozen native preview", async () => {
  const detail = detailFixture();
  const decision = {
    ...detail.revisions[0],
    id: "confirmed",
    revisionNumber: 1,
    status: "CONFIRMED",
    confirmedAt: "2026-11-01T00:00:00Z",
  };
  Object.assign(detail, {
    lifecycle: "CONFIRMED",
    currentRevisionId: "confirmed",
    currentRevisionNumber: 1,
    draftRevisionId: null,
    revisions: [decision],
  });
  const facts = {
    query: {
      decisionRevisionId: "confirmed",
      observationCutoff: "2026-11-02",
      observationTimezone: "Asia/Shanghai",
    },
    quality: "PARTIAL",
    issues: [],
    subjectMarket: null,
    sourceVersion: "frozen-facts",
    observations: {
      decision: { ...decision, contextAtConfirmation: null },
      executions: [],
      principles: [],
    },
  };
  const fallback = mocks.native.getMockImplementation()!;
  let savedBody: unknown;
  let submits = 0;
  mocks.native.mockImplementation(async (command, args) => {
    if (command === "get_investment_decision") return detail;
    if (command === "list_investment_scenario_tags") return ["Synthetic category"];
    if (command === "save_investment_review_draft") {
      savedBody = args?.body;
      return { reviewId: "review", rootId: "review", version: 1, revisionNumber: 2 };
    }
    if (command === "preview_investment_review_submission")
      return {
        ledgerId: "synthetic-ledger",
        target: args,
        body: savedBody,
        facts,
        expectedContentHash: "frozen-content",
        expectedFactsHash: "frozen-facts-hash",
      };
    if (command === "submit_investment_review" && submits++ === 0)
      throw new InvestmentJournalError("unknown_result", "Synthetic lost receipt");
    return fallback(command, args);
  });
  render(
    <Providers>
      <DecisionDetail id={detail.decisionId} />
    </Providers>,
  );
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: zh.review_new }));
  await screen.findByRole("button", { name: "Synthetic category" });
  const categories = screen.getByLabelText(zh.scenario_tags);
  for (const comma of [",", "，"]) {
    fireEvent.change(categories, { target: { value: `First${comma} S` } });
    await user.click(screen.getByRole("button", { name: "Synthetic category" }));
    expect(categories).toHaveValue(`First${comma} Synthetic category`);
  }
  fireEvent.change(screen.getByLabelText(zh.review_cutoff), { target: { value: "2026-11-02" } });
  fireEvent.change(screen.getByLabelText(zh.review_judgment), {
    target: { value: "Synthetic original judgment" },
  });
  fireEvent.change(screen.getByLabelText(zh.review_execution_assessment), {
    target: { value: "Synthetic execution observations" },
  });
  fireEvent.change(screen.getByLabelText(zh.review_explanations), {
    target: { value: "Alternative explanation" },
  });
  expect(screen.getByRole("button", { name: zh.review_submission_preview })).toBeDisabled();
  await user.click(screen.getByRole("button", { name: zh.save_draft }));
  await screen.findByText(zh.saved);
  expect(screen.getByLabelText(zh.review_cutoff)).toHaveAttribute("readonly");
  await user.click(screen.getByRole("button", { name: zh.review_submission_preview }));
  await screen.findByText(zh.review_quality_partial);
  expect(screen.getByText(zh.review_record_version.replace("{{number}}", "2"))).toBeVisible();
  expect(
    screen.getByRole("heading", {
      name: new RegExp(zh.review_decision_version.replace("{{number}}", "1")),
    }),
  ).toBeVisible();
  expect(screen.getByText("Alternative explanation")).toBeVisible();
  expect(mocks.native.mock.calls.some(([name]) => name === "submit_investment_review")).toBe(false);
  await user.click(screen.getByRole("button", { name: zh.review_submit }));
  await user.click(await screen.findByRole("button", { name: zh.replay }));
  await screen.findByText(zh.review_saved_submission);
  const saves = mocks.native.mock.calls.filter(([name]) => name === "save_investment_review_draft");
  expect(saves).toHaveLength(1);
  expect(saves[0][1]).toMatchObject({
    decisionRevisionId: "confirmed",
    observationCutoff: "2026-11-02",
    body: { scenarioTags: ["First", "Synthetic category"] },
  });
  const writes = mocks.native.mock.calls.filter(([name]) => name === "submit_investment_review");
  expect(writes).toHaveLength(2);
  expect(writes[0][1]).toEqual(writes[1][1]);
  expect(writes[0][1]).toMatchObject({
    ledgerId: "synthetic-ledger",
    target: { reviewId: "review", expectedVersion: 1 },
    expectedContentHash: "frozen-content",
    expectedFactsHash: "frozen-facts-hash",
    facts,
  });
});

it("opens an exact due review and keeps reminder changes separate from draft checks", async () => {
  const detail = detailFixture();
  const decision = {
    ...detail.revisions[0],
    id: "confirmed",
    revisionNumber: 1,
    status: "CONFIRMED",
  };
  Object.assign(detail, {
    lifecycle: "CONFIRMED",
    currentRevisionId: "confirmed",
    currentRevisionNumber: 1,
    nextReviewAt: "2026-11-03",
    revisions: [decision, detail.revisions[0]],
  });
  const fallback = mocks.native.getMockImplementation()!;
  mocks.native.mockImplementation(async (command, args) => {
    if (command === "get_investment_decision") return detail;
    if (command === "update_investment_decision") {
      const mutation = args?.mutation as { operation: string; date?: string };
      detail.version++;
      detail.nextReviewAt =
        mutation.operation === "SET_REVIEW_DATE" ? (mutation.date ?? null) : null;
      return { decisionId: detail.decisionId, version: detail.version };
    }
    return fallback(command, args);
  });
  render(
    <Providers>
      <DecisionDetail
        id={detail.decisionId}
        task="REVIEW_DUE"
        taskRevision="confirmed"
        taskDate="2026-11-03"
      />
    </Providers>,
  );
  const user = userEvent.setup();
  expect(await screen.findByRole("heading", { name: /决策复盘/ })).toHaveFocus();
  const initialVersion = detail.version;
  await user.click(screen.getByRole("button", { name: zh.review_reminder }));
  fireEvent.change(screen.getByLabelText(zh.review_reminder_date), {
    target: { value: "2026-11-12" },
  });
  await user.click(screen.getByRole("button", { name: zh.review_save_reminder }));
  await screen.findByText(zh.review_current_reminder.replace("{{date}}", "2026-11-12"));
  await user.click(screen.getByRole("button", { name: zh.review_reminder }));
  await user.click(screen.getByRole("button", { name: zh.review_cancel_reminder }));
  await screen.findByText(zh.review_no_reminder);
  const writes = mocks.native.mock.calls.filter(([name]) => name === "update_investment_decision");
  expect(writes).toHaveLength(2);
  expect(writes[0][1]).toMatchObject({
    expectedVersion: initialVersion,
    mutation: { operation: "SET_REVIEW_DATE", date: "2026-11-12" },
  });
  expect(writes[1][1]).toMatchObject({
    expectedVersion: initialVersion + 1,
    mutation: { operation: "CANCEL_REVIEW" },
  });
  expect(mocks.native.mock.calls.some(([name]) => name === "save_investment_decision_draft")).toBe(
    false,
  );
});

it("never opens a replacement draft from a stale due-task link", async () => {
  const detail = detailFixture();
  const fallback = mocks.native.getMockImplementation()!;
  mocks.native.mockImplementation(async (name, args) =>
    name === "get_investment_decision" ? detail : fallback(name, args),
  );
  render(
    <Providers>
      <DecisionDetail
        id={detail.decisionId}
        task="DRAFT_CHECK_DUE"
        taskRevision="replaced"
        taskDate="2027-01-08"
      />
    </Providers>,
  );
  await screen.findByText(zh.queue_task_changed);
  expect(screen.queryByRole("button", { name: zh.save_draft })).not.toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: zh.continue_draft }));
  expect(await screen.findByRole("button", { name: zh.save_draft })).toBeVisible();
});

it("publishes the exact principle draft and pins a source review without submitting it", async () => {
  const effective = {
    id: "effective",
    revisionNumber: 1,
    status: "EFFECTIVE",
    body: { ...blankPrinciple(), name: "Synthetic current rule", content: "Original rule" },
  };
  const detail = {
    principleId: "principle",
    name: effective.body.name,
    version: 4,
    currentRevisionId: effective.id,
    currentRevisionNumber: 1,
    draftRevisionId: null,
    isArchived: false,
    revisions: [effective],
  };
  const sourceBody = {
    ...newReviewBody(),
    judgment: "Source draft judgment",
    executionAssessment: "Source draft execution",
  };
  const sourceReview = {
    id: "draft-review",
    rootId: "draft-review",
    revisionNumber: 2,
    status: "DRAFT",
    observationCutoff: "2026-11-03",
    observationTimezone: "Asia/Shanghai",
    body: sourceBody,
    factsSnapshot: null,
  };
  const sourceContext = {
    sourceReview: { ...sourceReview, statusAtCapture: "DRAFT" },
    sourceReference: null,
    sourceStatusIsVerification: false,
  };
  let savedBody: unknown;
  const fallback = mocks.native.getMockImplementation()!;
  mocks.native.mockImplementation(async (command, args) => {
    switch (command) {
      case "list_investment_principles":
        return [detail];
      case "get_investment_principle":
        return detail;
      case "get_investment_review":
        return sourceReview;
      case "save_investment_principle_draft":
        savedBody = args?.body;
        return {
          principleId: "principle",
          draftRevisionId: "new-draft",
          revisionNumber: 2,
          version: 5,
        };
      case "preview_investment_principle_publication":
        return {
          ledgerId: "synthetic-ledger",
          target: args,
          body: savedBody,
          revisionNumber: 2,
          expectedContentHash: "principle-content",
          expectedSourceContextHash: "principle-source",
          sourceContext,
        };
      default:
        return fallback(command, args);
    }
  });
  render(
    <Providers>
      <PrincipleLibrary clearSourceReview={vi.fn()} />
    </Providers>,
  );
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: zh.principle_view }));
  await screen.findByText("Original rule");
  await user.click(screen.getByRole("button", { name: zh.principle_amend }));
  fireEvent.change(screen.getByLabelText(zh.principle_rule), {
    target: { value: "Revised synthetic rule" },
  });
  fireEvent.change(screen.getByLabelText(zh.principle_source_review), {
    target: { value: "draft-review" },
  });
  await user.click(screen.getByRole("button", { name: zh.save_draft }));
  await screen.findByText(zh.saved);
  expect(screen.getByRole("button", { name: zh.principle_publication_preview })).toBeDisabled();
  fireEvent.change(screen.getByLabelText(zh.principle_revision_reason), {
    target: { value: "New receipt guidance" },
  });
  await user.click(screen.getByRole("button", { name: zh.save_draft }));
  await user.click(screen.getByRole("button", { name: zh.principle_publication_preview }));
  await screen.findByText("Source draft judgment");
  expect(screen.getByText(zh.principle_source_status_hint)).toBeVisible();
  expect(
    mocks.native.mock.calls.some(([command]) => command === "publish_investment_principle"),
  ).toBe(false);
  await user.click(screen.getByRole("button", { name: zh.principle_publish }));
  await screen.findByText(new RegExp(zh.principle_published));
  const calls = mocks.native.mock.calls;
  expect(
    calls.find(([command]) => command === "save_investment_principle_draft")?.[1],
  ).toMatchObject({
    principleId: "principle",
    expectedDraftRevisionId: null,
    expectedVersion: 4,
    body: { sourceReviewId: "draft-review" },
  });
  expect(calls.find(([command]) => command === "publish_investment_principle")?.[1]).toMatchObject({
    ledgerId: "synthetic-ledger",
    target: { principleId: "principle", draftRevisionId: "new-draft", expectedVersion: 5 },
    expectedContentHash: "principle-content",
    expectedSourceContextHash: "principle-source",
    sourceContext,
  });
  expect(calls.some(([command]) => command === "submit_investment_review")).toBe(false);
});

it("reads a historical case with exact tag intersection and no inferred principle relationship", async () => {
  const detail = detailFixture();
  const old = {
    ...detail.revisions[0],
    id: "confirmed-old",
    revisionNumber: 1,
    status: "CONFIRMED",
    body: {
      ...decisionFixture(),
      title: "Historical case",
      thesis: "Original historical rationale",
    },
    context: null,
  };
  const current = {
    ...old,
    id: "current",
    revisionNumber: 2,
    body: { ...old.body, thesis: "Current rationale" },
  };
  Object.assign(detail, {
    lifecycle: "CONFIRMED",
    currentRevisionId: current.id,
    currentRevisionNumber: 2,
    revisions: [old, current],
  });
  const fallback = mocks.native.getMockImplementation()!;
  mocks.native.mockImplementation(async (command, args) => {
    if (command === "search_investment_cases")
      return [
        {
          kind: "DECISION",
          id: old.id,
          decisionId: detail.decisionId,
          title: old.body.title,
          body: old.body,
          scenarioTags: ["one, part", "two"],
          principleRelations: [],
          confirmedAt: "2026-11-01",
          revisionNumber: 1,
        },
      ];
    if (command === "get_investment_decision") return detail;
    return fallback(command, args);
  });
  render(
    <Providers>
      <InvestmentCases initialTags={["one, part", "two"]} clearPrinciple={vi.fn()} />
    </Providers>,
  );
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: /查看此版本.*Historical case/ }));
  expect(await screen.findByText("Original historical rationale")).toBeVisible();
  expect(screen.queryByText("Current rationale")).not.toBeInTheDocument();
  expect(screen.getByText(zh.case_no_relation)).toBeVisible();
  expect(
    mocks.native.mock.calls.find(([command]) => command === "search_investment_cases")?.[1],
  ).toMatchObject({ scenarioTags: ["one, part", "two"], principleRevisionId: null });
});

it("requires explicit reason to close decision execution and reopens by current version", async () => {
  const detail = detailFixture();
  const decision = {
    ...detail.revisions[0],
    id: "confirmed",
    revisionNumber: 1,
    status: "CONFIRMED",
    body: { ...decisionFixture(), action: "BUY" },
  };
  Object.assign(detail, {
    lifecycle: "CONFIRMED",
    currentRevisionId: "confirmed",
    currentRevisionNumber: 1,
    executionState: "OPEN",
    revisions: [decision, detail.revisions[0]],
  });
  const initialVersion = detail.version;
  const fallback = mocks.native.getMockImplementation()!;
  mocks.native.mockImplementation(async (command, args) => {
    if (command === "get_investment_decision") return detail;
    if (command === "update_investment_decision") {
      const mutation = args?.mutation as { operation: "CLOSE" | "REOPEN"; reason?: string };
      detail.executionState = mutation.operation === "CLOSE" ? "CLOSED" : "OPEN";
      detail.closeReason = mutation.reason ?? null;
      detail.version++;
      return { decisionId: detail.decisionId, version: detail.version };
    }
    return fallback(command, args);
  });
  render(
    <Providers>
      <DecisionDetail id={detail.decisionId} />
    </Providers>,
  );
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: zh.execution_close }));
  expect(screen.getByRole("button", { name: zh.execution_confirm_close })).toBeDisabled();
  fireEvent.change(screen.getByLabelText(zh.execution_close_reason), {
    target: { value: "Synthetic close reason" },
  });
  await user.click(screen.getByRole("button", { name: zh.execution_confirm_close }));
  await user.click(await screen.findByRole("button", { name: zh.execution_reopen }));
  expect(screen.getByText(/Synthetic close reason/)).toBeVisible();
  await user.click(screen.getByRole("button", { name: zh.execution_confirm_reopen }));
  await screen.findByRole("button", { name: zh.execution_close });
  const writes = mocks.native.mock.calls.filter(
    ([command]) => command === "update_investment_decision",
  );
  expect(writes[0][1]).toMatchObject({
    expectedVersion: initialVersion,
    mutation: { operation: "CLOSE", reason: "Synthetic close reason" },
  });
  expect(writes[1][1]).toMatchObject({
    expectedVersion: initialVersion + 1,
    mutation: { operation: "REOPEN" },
  });
});
