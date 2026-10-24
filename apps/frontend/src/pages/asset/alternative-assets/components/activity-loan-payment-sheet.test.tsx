import type { ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@/test/render";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ActivityDetails } from "@/lib/types";
import { ActivityLoanPaymentSheet } from "./activity-loan-payment-sheet";

const mocks = vi.hoisted(() => ({
  link: vi.fn<(activityId: string, link: unknown) => Promise<void>>(),
}));
vi.mock("@/adapters", () => ({ linkLoanPayment: mocks.link }));
vi.mock("@/hooks/use-accounts", () => ({
  useAccounts: () => ({
    accounts: [
      { id: "chequing", name: "Chequing", accountType: "CASH" },
      { id: "card", name: "Card", accountType: "CREDIT_CARD" },
    ],
  }),
}));
const projection = {
  version: 1,
  annualRate: 4,
  paymentAmount: 100,
  frequency: "monthly",
  firstPaymentDate: "2026-02-01",
};
vi.mock("@/hooks/use-alternative-assets", () => ({
  useAlternativeHoldings: () => ({
    data: [
      {
        id: "mortgage",
        name: "抵押贷款",
        kind: "LIABILITY",
        currency: "CAD",
        metadata: { loan_projection: projection },
      },
      { id: "manual", name: "手动", kind: "LIABILITY", currency: "CAD", metadata: {} },
      {
        id: "usd",
        name: "US loan",
        kind: "LIABILITY",
        currency: "USD",
        metadata: { loan_projection: projection },
      },
    ],
  }),
}));
vi.mock("@wealthfolio/ui", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@wealthfolio/ui")>()),
  ResponsiveSelect: ({
    value,
    onValueChange,
    options,
    "aria-label": label,
  }: {
    value: string;
    onValueChange: (value: string) => void;
    options: { value: string; label: string }[];
    "aria-label"?: string;
  }) => (
    <select
      aria-label={label}
      value={value}
      onChange={(event) => onValueChange(event.target.value)}
    >
      <option value="" />
      {options.map((option) => (
        <option key={option.value} value={option.value}>
          {option.label}
        </option>
      ))}
    </select>
  ),
}));

const withdrawal = (overrides: Partial<ActivityDetails> = {}) =>
  ({
    id: "act",
    activityType: "WITHDRAWAL",
    accountId: "chequing",
    currency: "CAD",
    amount: "600",
    date: "2026-03-01T12:00:00Z",
    ...overrides,
  }) as unknown as ActivityDetails;

const show = (activity: ActivityDetails, onChanged = vi.fn()) =>
  render(
    <ActivityLoanPaymentSheet
      activity={activity}
      open
      onOpenChange={vi.fn()}
      onChanged={onChanged}
    />,
    {
      wrapper: ({ children }: { children: ReactNode }) => (
        <QueryClientProvider client={new QueryClient()}>{children}</QueryClientProvider>
      ),
    },
  );

beforeEach(() => {
  vi.clearAllMocks();
  mocks.link.mockResolvedValue(undefined);
});

describe("linking a withdrawal from its account", () => {
  it("offers calculated loans in the withdrawal's currency and links as extra", async () => {
    show(withdrawal());
    const loan = screen.getByRole("combobox", { name: "贷款" });
    expect([...loan.querySelectorAll("option")].map((option) => option.textContent)).toEqual([
      "",
      "抵押贷款",
    ]);
    fireEvent.change(loan, { target: { value: "mortgage" } });
    const escrow = screen.getByLabelText("每期税费和保险");
    expect(escrow).toHaveAttribute("placeholder", "贷款的通常金额");
    fireEvent.change(screen.getByRole("combobox", { name: "计入" }), {
      target: { value: "extra" },
    });
    // Extra principal includes no escrow unless one is entered.
    expect(escrow).toHaveAttribute("placeholder", "除非填写，否则为无");
    fireEvent.click(screen.getByRole("button", { name: "关联" }));
    await waitFor(() =>
      expect(mocks.link).toHaveBeenCalledWith("act", {
        type: "link",
        loanId: "mortgage",
        appliesTo: "extra",
      }),
    );
  });

  it("unlinks a withdrawal already counted on a loan and reports the change", async () => {
    const onChanged = vi.fn();
    show(withdrawal({ metadata: { loan_payment: { loan_id: "mortgage" } } }), onChanged);
    expect(screen.getByText("已计为抵押贷款的还款")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "取消关联" }));
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect(mocks.link).toHaveBeenCalledWith("act", { type: "unlink" });
  });

  it("explains that only cash accounts pay loans", () => {
    show(withdrawal({ accountId: "card" }));
    expect(screen.queryByRole("combobox", { name: "贷款" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "关联" })).not.toBeInTheDocument();
  });

  it("shows a refusal from the backend", async () => {
    mocks.link.mockRejectedValueOnce("LOAN_PAYMENT_NOT_ELIGIBLE");
    const onChanged = vi.fn();
    show(withdrawal(), onChanged);
    fireEvent.change(screen.getByRole("combobox", { name: "贷款" }), {
      target: { value: "mortgage" },
    });
    fireEvent.click(screen.getByRole("button", { name: "关联" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "只有贷款币种现金账户中的已入账取款才能支付此贷款。",
    );
    expect(onChanged).not.toHaveBeenCalled();
  });

  it("offers to replace an extra repayment recorded for the same money", async () => {
    mocks.link.mockRejectedValueOnce("LOAN_PAYMENT_DUPLICATES_EVENT");
    const onChanged = vi.fn();
    show(withdrawal(), onChanged);
    fireEvent.change(screen.getByRole("combobox", { name: "贷款" }), {
      target: { value: "mortgage" },
    });
    const escrow = screen.getByLabelText("每期税费和保险");
    expect(escrow).toHaveAttribute("placeholder", "贷款的通常金额");
    fireEvent.click(screen.getByRole("button", { name: "关联" }));
    const replace = await screen.findByRole("button", { name: "改用这笔取款" });
    // The replacement is all extra principal, so no escrow is assumed.
    expect(escrow).toHaveAttribute("placeholder", "除非填写，否则为无");
    fireEvent.click(replace);
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect(mocks.link).toHaveBeenLastCalledWith("act", {
      type: "link",
      loanId: "mortgage",
      replaceEvent: true,
    });
  });

  it("drops the replace offer once the link it answers changes", async () => {
    mocks.link.mockRejectedValueOnce("LOAN_PAYMENT_DUPLICATES_EVENT");
    show(withdrawal());
    const loan = screen.getByRole("combobox", { name: "贷款" });
    fireEvent.change(loan, { target: { value: "mortgage" } });
    fireEvent.click(screen.getByRole("button", { name: "关联" }));
    await screen.findByRole("button", { name: "改用这笔取款" });
    fireEvent.change(loan, { target: { value: "" } });
    expect(screen.queryByRole("button", { name: "改用这笔取款" })).toBeNull();
  });
});
