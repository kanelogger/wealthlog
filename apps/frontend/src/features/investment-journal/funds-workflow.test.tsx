import { beforeEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState, type ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { PrivacyContext } from "@/context/privacy-context";
import { InvestmentJournalError } from "@/adapters";
import { JournalLocaleProvider } from "./locale";
import { FundAllocationEditor } from "./fund-allocation-editor";
import { FundsPage } from "./funds-page";
import { InvestmentWorkQueue } from "./investment-work-queue";
import { initialQueueFilters } from "./execution-work-types";
import { PurposeEditor } from "./purpose-editor";
import { FundReversal } from "./fund-reversal";
import { richContextFixture } from "./test-fixtures";
import { updateWriteSession, writeSessionKey } from "./write-session";
import zh from "./locales/zh.json";
import type { Purpose, Resource } from "./types";

const mocks = vi.hoisted(() => ({
  native: vi.fn<(name: string, args?: Record<string, unknown>) => Promise<unknown>>(),
}));
vi.mock("@/adapters", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/adapters")>()),
  executeInvestmentJournal: mocks.native,
  getAssets: () => Promise.resolve([]),
  getAccounts: () =>
    Promise.resolve([
      { id: "a", name: "Synthetic source", currency: "CNY", isActive: true, accountType: "CASH" },
      {
        id: "b",
        name: "Synthetic destination",
        currency: "USD",
        isActive: true,
        accountType: "CASH",
      },
    ]),
  searchActivities: () =>
    Promise.resolve({
      data: [
        {
          id: "out",
          accountId: "a",
          accountName: "Synthetic source",
          activityType: "TRANSFER_OUT",
          status: "POSTED",
          sourceGroupId: "pair",
          date: "2026-11-01",
          currency: "CNY",
          amount: "100.01",
        },
      ],
      meta: { totalRowCount: 1 },
    }),
  getTransferPairForActivity: () =>
    Promise.resolve({ transferOut: { accountId: "a" }, transferIn: { accountId: "b" } }),
}));
const resource: Resource = { kind: "CASH", accountId: "a", currency: "CNY" };
function purpose(): Purpose {
  return {
    ...richContextFixture().response.funding.data.purposes![0],
    useOn: "2028-06-01",
    liquidity: "T+1",
    targetAmount: "9007199254740993.1234",
  };
}
function Providers({ children, hidden = false }: { children: ReactNode; hidden?: boolean }) {
  const [client] = useState(
    () => new QueryClient({ defaultOptions: { queries: { retry: false } } }),
  );
  return (
    <QueryClientProvider client={client}>
      <PrivacyContext.Provider
        value={{ isBalanceHidden: hidden, toggleBalanceVisibility: vi.fn() }}
      >
        <JournalLocaleProvider>{children}</JournalLocaleProvider>
      </PrivacyContext.Provider>
    </QueryClientProvider>
  );
}
beforeEach(() => {
  localStorage.removeItem("investment-journal-language");
  updateWriteSession(writeSessionKey(), {
    frozen: null,
    busy: false,
    uncertain: false,
    error: null,
  });
  mocks.native.mockReset();
  mocks.native.mockImplementation((name) =>
    Promise.resolve(
      name === "get_investment_context"
        ? richContextFixture()
        : name === "list_fund_purposes"
          ? [purpose()]
          : name === "get_fund_reconciliation"
            ? []
            : name === "list_fund_history"
              ? []
              : name === "preview_fund_allocations"
                ? {
                    schemaVersion: 1,
                    asOfDate: "2026-11-03",
                    previewHash: "current-source-1",
                    resources: [
                      {
                        ...richContextFixture().response.funding.data.allocation!.resources[0],
                        observation: {
                          resource,
                          sources: [
                            {
                              kind: "ACTIVITY",
                              id: "synthetic-original-source",
                              version: "shown-source-v2",
                              applied: true,
                              effect: "TRADE",
                            },
                          ],
                        },
                      },
                    ],
                    sourceReferences: [
                      {
                        operationId: "prior-operation",
                        resource,
                        sourceKind: "ACTIVITY",
                        sourceId: "synthetic-original-source",
                        sourceCounterpartyId: null,
                        originVersion: "shown-source-v1",
                        currentVersion: "shown-source-v2",
                        state: "VALID",
                      },
                    ],
                  }
                : {},
    ),
  );
});

it("editing a purpose preserves exact amounts, optional fields and the original version", async () => {
  const close = vi.fn();
  render(
    <Providers>
      <PurposeEditor initial={purpose()} currency="USD" close={close} />
    </Providers>,
  );
  fireEvent.change(screen.getByLabelText("用途名称"), { target: { value: "更新后的用途" } });
  await userEvent.click(screen.getByRole("button", { name: "保存用途" }));
  await waitFor(() => expect(close).toHaveBeenCalled());
  expect(mocks.native.mock.calls.find(([name]) => name === "save_fund_purpose")?.[1]).toMatchObject(
    {
      id: "p",
      expectedVersion: 1,
      name: "更新后的用途",
      currency: "CNY",
      targetAmount: "9007199254740993.1234",
      useOn: "2028-06-01",
      liquidity: "T+1",
      lossBudget: null,
      minimumCash: null,
      singleAssetLimitBps: null,
    },
  );
  expect(mocks.native.mock.calls.map(([name]) => name)).not.toContain("apply_fund_allocation");
});

async function fillChange() {
  await userEvent.click(screen.getByRole("button", { name: "添加用途增减" }));
  fireEvent.change(screen.getByLabelText("用途"), { target: { value: "p" } });
  fireEvent.change(screen.getByLabelText("现金归属增减"), {
    target: { value: "9007199254740993.1234" },
  });
  fireEvent.change(screen.getByLabelText("本次操作原因"), {
    target: { value: "用户明确填写的归属" },
  });
  await userEvent.click(screen.getByRole("button", { name: "预览当前资源和变动" }));
  await screen.findByRole("heading", { name: "核实用途归属变动" });
}
it("ownership preview returns to unchanged inputs and submits the exact observed hash and decimal string", async () => {
  const close = vi.fn();
  render(
    <Providers>
      <FundAllocationEditor
        resource={resource}
        capture={richContextFixture()}
        purposes={[purpose()]}
        close={close}
      />
    </Providers>,
  );
  await fillChange();
  expect(screen.getByText("9,007,199,254,740,993.1234", { exact: false })).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "返回编辑" }));
  expect(screen.getByLabelText("现金归属增减")).toHaveValue("9007199254740993.1234");
  expect(screen.getByLabelText("本次操作原因")).toHaveValue("用户明确填写的归属");
  await userEvent.click(screen.getByRole("button", { name: "预览当前资源和变动" }));
  await screen.findByRole("button", { name: "提交归属变更" });
  await userEvent.click(screen.getByRole("button", { name: "提交归属变更" }));
  await waitFor(() => expect(close).toHaveBeenCalled());
  expect(
    mocks.native.mock.calls.find(([name]) => name === "apply_fund_allocation")?.[1],
  ).toMatchObject({
    operationKind: "ALLOCATE",
    resources: [resource],
    entries: [{ purposeId: "p", resource, delta: "9007199254740993.1234" }],
    reason: "用户明确填写的归属",
    expectedPreviewHash: "current-source-1",
  });
});

it("a source conflict requires a fresh preview and preserves the proposed changes", async () => {
  render(
    <Providers>
      <FundAllocationEditor
        resource={resource}
        capture={richContextFixture()}
        purposes={[purpose()]}
        close={vi.fn()}
      />
    </Providers>,
  );
  await fillChange();
  const original = mocks.native.getMockImplementation()!;
  mocks.native.mockImplementation((name, args) =>
    name === "apply_fund_allocation"
      ? Promise.reject(new InvestmentJournalError("state_conflict", "changed"))
      : original(name, args),
  );
  await userEvent.click(screen.getByRole("button", { name: "提交归属变更" }));
  await screen.findByRole("alert");
  await userEvent.click(screen.getByRole("button", { name: "返回编辑" }));
  expect(screen.getByLabelText("现金归属增减")).toHaveValue("9007199254740993.1234");
  mocks.native.mockImplementation((name, args) =>
    name === "preview_fund_allocations"
      ? Promise.resolve({
          schemaVersion: 1,
          asOfDate: "2026-11-03",
          resources: [
            {
              observation: { resource, sources: [] },
              balance: {
                actual: "1000",
                allocated: "0",
                unallocated: "1000",
                excess: "0",
                negativeActual: "0",
              },
              purposes: {},
            },
          ],
          sourceReferences: [],
          previewHash: "current-source-2",
        })
      : original(name, args),
  );
  await userEvent.click(screen.getByRole("button", { name: "预览当前资源和变动" }));
  await screen.findByRole("button", { name: "提交归属变更" });
  await userEvent.click(screen.getByRole("button", { name: "提交归属变更" }));
  await waitFor(() =>
    expect(
      mocks.native.mock.calls.filter(([name]) => name === "apply_fund_allocation"),
    ).toHaveLength(2),
  );
  expect(mocks.native.mock.lastCall?.[1]?.expectedPreviewHash).toBe("current-source-2");
});

it("source-only reconciliation keeps an empty entry list and replays an unknown outcome unchanged", async () => {
  render(
    <Providers>
      <FundAllocationEditor
        resource={resource}
        capture={richContextFixture()}
        purposes={[purpose()]}
        close={vi.fn()}
      />
    </Providers>,
  );
  fireEvent.change(screen.getByLabelText("归属操作"), { target: { value: "RECONCILE" } });
  fireEvent.change(screen.getByLabelText("本次操作原因"), {
    target: { value: "明确核对来源，不调整超额" },
  });
  await userEvent.click(screen.getByRole("button", { name: "预览当前资源和变动" }));
  await screen.findByText(/本次不写用途分录/);
  expect(screen.getAllByText(/synthetic-original-source/)).toHaveLength(2);
  expect(screen.getByText("当前已计入核算 · 已完成成交")).toBeInTheDocument();
  expect(screen.getByText("原确认版本: shown-source-v1")).toBeInTheDocument();
  expect(screen.getByText(/当前有效/)).toBeInTheDocument();
  const original = mocks.native.getMockImplementation()!;
  let calls = 0;
  mocks.native.mockImplementation((name, args) =>
    name === "apply_fund_allocation" && calls++ === 0
      ? Promise.reject(new InvestmentJournalError("unknown_result", "lost response"))
      : original(name, args),
  );
  await userEvent.click(screen.getByRole("button", { name: "提交归属变更" }));
  await screen.findByRole("button", { name: "重放同一请求" });
  expect(screen.getByRole("button", { name: "返回编辑" })).toBeDisabled();
  await userEvent.click(screen.getByRole("button", { name: "重放同一请求" }));
  await waitFor(() =>
    expect(
      mocks.native.mock.calls.filter(([name]) => name === "apply_fund_allocation"),
    ).toHaveLength(2),
  );
  const writes = mocks.native.mock.calls.filter(([name]) => name === "apply_fund_allocation");
  expect(writes[0][1]).toEqual(writes[1][1]);
  expect(writes[0][1]).toMatchObject({
    operationKind: "RECONCILE",
    resources: [resource],
    entries: [],
  });
});

it("missing source metadata cannot be submitted as a source acknowledgement", async () => {
  const original = mocks.native.getMockImplementation()!;
  mocks.native.mockImplementation((name, args) =>
    name === "preview_fund_allocations"
      ? Promise.resolve({
          schemaVersion: 1,
          asOfDate: "2026-11-03",
          resources: [richContextFixture().response.funding.data.allocation!.resources[0]],
          previewHash: "missing-sources",
        })
      : original(name, args),
  );
  render(
    <Providers>
      <FundAllocationEditor
        resource={resource}
        capture={richContextFixture()}
        purposes={[purpose()]}
        close={vi.fn()}
      />
    </Providers>,
  );
  fireEvent.change(screen.getByLabelText("归属操作"), { target: { value: "RECONCILE" } });
  fireEvent.change(screen.getByLabelText("本次操作原因"), { target: { value: "明确核对" } });
  await userEvent.click(screen.getByRole("button", { name: "预览当前资源和变动" }));
  await screen.findByText("来源清单不完整，暂不能提交；请返回后重新预览。");
  expect(screen.getByRole("button", { name: "提交归属变更" })).toBeDisabled();
  expect(mocks.native.mock.calls.some(([name]) => name === "apply_fund_allocation")).toBe(false);
});

it("funding scopes every account and purpose, keeps unknown resources and matches reconciliation identities independent of key order", async () => {
  const context = richContextFixture();
  context.response.funding.data.allocation!.resources[0].balance.actual = null;
  const original = mocks.native.getMockImplementation()!;
  mocks.native.mockImplementation((name, args) =>
    name === "get_investment_context"
      ? Promise.resolve(context)
      : name === "get_fund_reconciliation"
        ? Promise.resolve([
            {
              id: "issue",
              kind: "SOURCE_CHANGED",
              resource: { currency: "CNY", accountId: "a", kind: "CASH" },
              sourceId: null,
              excess: null,
            },
          ])
        : original(name, args),
  );
  render(
    <Providers>
      <FundsPage />
    </Providers>,
  );
  await screen.findByText("已确认的来源发生变化");
  const call = mocks.native.mock.calls.find(([name]) => name === "get_investment_context");
  expect(call?.[1]).toEqual({ purposeIds: ["p"], accountIds: ["a", "b"] });
  const row = screen
    .getByRole("heading", { name: "Synthetic account A · CNY 现金" })
    .closest("li")!;
  expect(within(row).getByText("暂不可用")).toBeInTheDocument();
  expect(
    screen.getByRole("heading", { name: "Synthetic account B · CNY 现金" }),
  ).toBeInTheDocument();
});

it("history retains source-only receipts, pages explicitly, and masks original deltas in the DOM", async () => {
  const history = Array.from({ length: 50 }, (_, index) => ({
    id: `op-${index}`,
    operationKind: "RECONCILE",
    reason: `合成核对 ${index}`,
    correctionOf: null,
    sourceKind: null,
    sourceId: null,
    createdAt: "2026-11-03",
    createdActorKind: "PROGRAM",
    createdChannel: "CLI",
    entries: index === 0 ? [{ purposeId: "p", resource, delta: "9007199254740993.1234" }] : [],
  }));
  const original = mocks.native.getMockImplementation()!;
  mocks.native.mockImplementation((name, args) =>
    name === "list_fund_history"
      ? Promise.resolve(args?.offset === 0 ? history : [])
      : original(name, args),
  );
  const view = render(
    <Providers hidden>
      <FundsPage />
    </Providers>,
  );
  await screen.findByRole("heading", { name: "资源与核对" });
  await userEvent.click(screen.getByRole("button", { name: "归属历史" }));
  await screen.findByText("合成核对 0");
  expect(screen.getAllByText(/本次不写用途分录/)).toHaveLength(49);
  expect(view.container.innerHTML).not.toMatch(/9007199254740993|9,007,199,254,740,993/);
  await userEvent.click(screen.getByRole("button", { name: "下一页" }));
  await screen.findByText("没有符合条件的归属操作。");
  expect(
    mocks.native.mock.calls.filter(([name]) => name === "list_fund_history").at(-1)?.[1],
  ).toMatchObject({ purposeId: null, offset: 50 });
});

it("reversal displays native opposite entries and resulting ownership, then binds only the shown operation and hash", async () => {
  const original = mocks.native.getMockImplementation()!;
  const entries = [{ purposeId: "p", resource, delta: "-200.125" }];
  mocks.native.mockImplementation((name, args) =>
    name === "preview_fund_reversal"
      ? Promise.resolve({
          funding: { schemaVersion: 1, asOfDate: "2026-11-03", resources: [] },
          entries,
          balancesAfter: [{ purposeId: "p", resource, amount: "100.0000000000000000000001" }],
          pendingReconciliation: [{ id: "pending" }],
          previewHash: "native-reversal-hash",
        })
      : original(name, args),
  );
  const close = vi.fn();
  render(
    <Providers>
      <FundReversal
        item={{
          id: "original-op",
          operationKind: "ALLOCATE",
          reason: "原归属",
          createdAt: "2026-11-01",
          createdActorKind: "GUI_SESSION",
          createdChannel: "GUI",
          entries,
          sourceId: null,
          sourceKind: null,
          correctionOf: null,
        }}
        capture={richContextFixture()}
        purposes={[purpose()]}
        close={close}
      />
    </Providers>,
  );
  await screen.findByText("100.0000000000000000000001", { exact: false });
  expect(screen.getByText(/冲正不隐式确认/)).toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("本次操作原因"), { target: { value: "用户明确冲正" } });
  await userEvent.click(screen.getByRole("button", { name: "提交用途冲正" }));
  await waitFor(() => expect(close).toHaveBeenCalled());
  const submitted = mocks.native.mock.calls.find(
    ([name]) => name === "reverse_fund_operation",
  )?.[1];
  expect(submitted).toMatchObject({
    operationId: "original-op",
    reason: "用户明确冲正",
    expectedPreviewHash: "native-reversal-hash",
  });
  expect(typeof submitted?.requestKey).toBe("string");
  expect(
    mocks.native.mock.calls.some(([name]) => name.includes("trade") || name.includes("transfer")),
  ).toBe(false);
});

it.each(["existing", "new"] as const)(
  "freezes %s cash transfer shares and the native preview hash",
  async (mode) => {
    const fallback = mocks.native.getMockImplementation()!;
    const share = "31.123456789012345678";
    const preview = {
      previewHash: "transfer-proof",
      transfer: {
        sourceResource: resource,
        destinationResource: { kind: "CASH", accountId: "b", currency: "USD" },
        sourceAmount: "100.01",
        destinationAmount: "14.0123",
        occurredAt: "2026-11-01",
      },
      shares: [{ purposeId: purpose().id, sourceAmount: share, destinationAmount: "4.3609" }],
      funding: { resources: [], sourceReferences: [] },
      balancesAfter: [],
      pendingReconciliation: [],
    };
    mocks.native.mockImplementation(async (name, args) =>
      name === "preview_cash_transfer_ownership" || name === "preview_existing_transfer_ownership"
        ? preview
        : fallback(name, args),
    );
    render(
      <Providers>
        <FundsPage />
      </Providers>,
    );
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: zh.operation_TRANSFER }));
    expect(screen.getByRole("heading", { name: zh.operation_TRANSFER })).toHaveFocus();
    if (mode === "new") {
      await user.selectOptions(screen.getByLabelText(zh.transfer_mode), "new");
      await user.selectOptions(screen.getByLabelText(zh.transfer_out), "a");
      await user.selectOptions(screen.getByLabelText(zh.transfer_in), "b");
      fireEvent.change(screen.getByLabelText(zh.transfer_source_amount), {
        target: { value: "100.01" },
      });
      fireEvent.change(screen.getByLabelText(zh.transfer_destination_amount), {
        target: { value: "14.0123" },
      });
    } else {
      await screen.findByRole("option", { name: /Synthetic source.*100.01/ });
      await user.selectOptions(screen.getByLabelText(zh.transfer_select), "out");
    }
    await user.selectOptions(screen.getByLabelText(zh.purpose), purpose().id);
    fireEvent.change(screen.getByLabelText(zh.transfer_share_amount), { target: { value: share } });
    fireEvent.change(screen.getByLabelText(zh.operation_reason), {
      target: { value: "Synthetic bank receipt" },
    });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: zh.preview_ownership })).toBeEnabled(),
    );
    await user.click(screen.getByRole("button", { name: zh.preview_ownership }));
    expect(await screen.findByText("4.3609")).toBeVisible();
    const commit =
      mode === "new" ? "create_cash_transfer_with_ownership" : "apply_existing_transfer_ownership";
    const read =
      mode === "new" ? "preview_cash_transfer_ownership" : "preview_existing_transfer_ownership";
    expect(mocks.native.mock.calls.some(([name]) => name === commit)).toBe(false);
    await user.click(
      screen.getByRole("button", {
        name: mode === "new" ? zh.transfer_submit_new : zh.transfer_submit,
      }),
    );
    expect(await screen.findByText(zh.transfer_saved)).toHaveFocus();
    await user.click(screen.getByRole("button", { name: zh.transfer_back }));
    expect(screen.getByRole("button", { name: zh.operation_TRANSFER })).toHaveFocus();
    const previewArgs = mocks.native.mock.calls.find(([name]) => name === read)?.[1];
    const commitArgs = mocks.native.mock.calls.find(([name]) => name === commit)?.[1];
    const key =
      mode === "new"
        ? (previewArgs?.transfer as { requestKey?: unknown } | undefined)?.requestKey
        : commitArgs?.requestKey;
    expect(typeof key).toBe("string");
    expect(commitArgs).toMatchObject({
      ...previewArgs,
      expectedPreviewHash: "transfer-proof",
      reason: "Synthetic bank receipt",
      shares: [{ purposeId: purpose().id, sourceAmount: share }],
      ...(mode === "new"
        ? {
            transfer: {
              sourceCurrency: "CNY",
              destinationCurrency: "USD",
              sourceAmount: "100.01",
              destinationAmount: "14.0123",
            },
          }
        : { activityId: "out" }),
    });
  },
);

it("blocks a historical correction without source evidence and commits only its frozen signed entries", async () => {
  const fallback = mocks.native.getMockImplementation()!;
  const history = [
    {
      id: "old-ownership",
      operationKind: "TRADE",
      reason: "Synthetic original ownership",
      createdAt: "2026-11-03",
      createdActorKind: "PROGRAM",
      createdChannel: "CLI",
      entries: [{ purposeId: purpose().id, resource, delta: "200" }],
    },
  ];
  const delta = "-19.123456789012345678";
  const entries = [{ purposeId: purpose().id, resource, delta }];
  const anchor = {
    operationId: "old-ownership",
    resource,
    sourceKind: "MANUAL_TRADE",
    sourceId: "synthetic-trade",
    sourceCounterpartyId: null,
    originVersion: "original-v1",
    currentVersion: "current-v2",
    state: "VALID",
  };
  let attempts = 0;
  mocks.native.mockImplementation(async (command, args) => {
    if (command === "list_fund_history") return history;
    if (command === "preview_fund_correction") {
      return {
        previewHash: "shown-correction",
        entries,
        funding: { resources: [], sourceReferences: [] },
        balancesAfter: [],
        pendingReconciliation: [],
        ...(attempts++ ? { sourceAnchors: [anchor] } : {}),
      };
    }
    return fallback(command, args);
  });
  render(
    <Providers>
      <FundsPage />
    </Providers>,
  );
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: zh.ownership_history }));
  await user.click(await screen.findByRole("button", { name: zh.correct_ownership }));
  await user.selectOptions(screen.getByLabelText(zh.purpose), purpose().id);
  const picker = screen.getByLabelText<HTMLSelectElement>(zh.correction_resource);
  await user.selectOptions(picker, picker.options[1].value);
  fireEvent.change(screen.getByLabelText(zh.correction_delta), { target: { value: delta } });
  fireEvent.change(screen.getByLabelText(zh.operation_reason), { target: { value: "   " } });
  expect(screen.getByRole("button", { name: zh.preview_ownership })).toBeDisabled();
  fireEvent.change(screen.getByLabelText(zh.operation_reason), {
    target: { value: "Synthetic correction" },
  });
  await user.click(screen.getByRole("button", { name: zh.preview_ownership }));
  await screen.findByText(zh.source_list_unavailable);
  expect(screen.getByRole("button", { name: zh.submit_correction })).toBeDisabled();
  await user.click(screen.getByRole("button", { name: zh.return_edit }));
  expect(screen.getByLabelText(zh.correction_delta)).toHaveValue(delta);
  await user.click(screen.getByRole("button", { name: zh.preview_ownership }));
  await screen.findByText(/current-v2/);
  expect(mocks.native.mock.calls.some(([command]) => command === "correct_fund_operation")).toBe(
    false,
  );
  await user.click(screen.getByRole("button", { name: zh.submit_correction }));
  await screen.findByText(zh.correction_saved);
  const args = mocks.native.mock.calls.find(
    ([command]) => command === "correct_fund_operation",
  )?.[1];
  expect(args).toMatchObject({
    operationId: "old-ownership",
    entries,
    reason: "Synthetic correction",
    expectedPreviewHash: "shown-correction",
  });
  expect(typeof args?.requestKey).toBe("string");
});

it.each(["NO_OWNERSHIP_CHANGE", "ADJUST_OWNERSHIP"] as const)(
  "acknowledges the current execution source with explicit %s",
  async (resolution) => {
    const fallback = mocks.native.getMockImplementation()!;
    const execution = {
      id: "indexed-trade",
      accountId: "a",
      assetId: "stock",
      side: "BUY",
      occurredAt: "2026-11-01",
      sourceId: "synthetic-source",
      sourceVersion: "fresh-v2",
      quantity: "4.25",
      netCash: null,
      settlementCurrency: null,
      fee: "0",
      tax: null,
      activityCurrency: "CNY",
    };
    const source = {
      execution,
      sourceState: "INVALID",
      currentSourceVersion: "latest-v3",
      projectionPending: false,
      activeLinks: [
        {
          id: "link",
          sourceVersion: "original-v1",
          snapshot: {
            execution: { ...execution, quantity: "2.125" },
            decision: { revisionNumber: 1, confirmedAt: "2026-11-01T00:00:00Z" },
          },
        },
      ],
    };
    let contextReads = 0;
    mocks.native.mockImplementation(async (name, args) => {
      if (name === "get_investment_context" && contextReads++ === 0)
        throw new Error("Synthetic resource load failure");
      if (name === "get_investment_work_queue")
        return {
          asOfDate: "2026-11-04",
          complete: false,
          items: [{ id: "source-changed", kind: "EXECUTION_SOURCE_CHANGED", source }],
        };
      if (name === "preview_investment_execution_reconciliation")
        return {
          source,
          resolution,
          entries: args?.entries,
          funding: { resources: [], sourceReferences: [] },
          balancesAfter: [],
          previewHash: "exact-source-preview",
        };
      return fallback(name, args);
    });
    render(
      <Providers>
        <InvestmentWorkQueue
          filters={initialQueueFilters}
          onFiltersChange={vi.fn()}
          openDecision={vi.fn()}
          openFunds={vi.fn()}
          openDecisions={vi.fn()}
        />
      </Providers>,
    );
    const user = userEvent.setup();
    await screen.findByText(zh.queue_incomplete);
    await user.click(await screen.findByRole("button", { name: zh.execution_reconcile }));
    expect(screen.getByText(/original-v1/)).toBeVisible();
    expect(screen.getByText(new RegExp(zh.execution_indexed_version + ".*fresh-v2"))).toBeVisible();
    expect(screen.getByText(new RegExp(zh.source_current_version + ".*latest-v3"))).toBeVisible();
    await screen.findByText(zh.execution_resources_failed);
    expect(screen.getByRole("radio", { name: zh.execution_ack_only })).toBeEnabled();
    expect(screen.getByRole("radio", { name: zh.execution_adjust })).toBeDisabled();
    if (resolution === "ADJUST_OWNERSHIP") {
      await user.click(screen.getByRole("button", { name: zh.retry }));
      await waitFor(() =>
        expect(screen.getByRole("radio", { name: zh.execution_adjust })).toBeEnabled(),
      );
    }
    await user.click(
      screen.getByRole("radio", {
        name: resolution === "ADJUST_OWNERSHIP" ? zh.execution_adjust : zh.execution_ack_only,
      }),
    );
    const entries =
      resolution === "ADJUST_OWNERSHIP"
        ? [{ purposeId: purpose().id, resource, delta: "-1.125" }]
        : [];
    if (resolution === "ADJUST_OWNERSHIP") {
      await user.selectOptions(screen.getByLabelText(zh.purpose), purpose().id);
      const picker = screen.getByLabelText<HTMLSelectElement>(zh.correction_resource);
      await user.selectOptions(picker, picker.options[1].value);
      fireEvent.change(screen.getByLabelText(zh.correction_delta), { target: { value: "-1.125" } });
    }
    fireEvent.change(screen.getByLabelText(zh.operation_reason), {
      target: { value: "Synthetic correction evidence" },
    });
    await user.click(screen.getByRole("button", { name: zh.execution_review_sources }));
    const confirm = await screen.findByRole("button", { name: zh.execution_submit_reconciliation });
    expect(
      mocks.native.mock.calls.some(([name]) => name === "reconcile_investment_execution"),
    ).toBe(false);
    await user.click(confirm);
    await screen.findByText(zh.execution_reconciliation_saved);
    expect(
      mocks.native.mock.calls.find(([name]) => name === "reconcile_investment_execution")?.[1],
    ).toMatchObject({
      draft: { executionId: "indexed-trade", resolution, entries },
      expectedPreviewHash: "exact-source-preview",
      reason: "Synthetic correction evidence",
    });
  },
);
