import { fireEvent, render, screen, waitFor } from "@/test/render";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { createOrdinaryActivity, updateActivity, updateOrdinaryActivity } from "@/adapters";
import type { Activity } from "@/lib/types";
import { CashActivityForm } from "./cash-activity-form";

vi.mock("@/adapters", () => ({
  createActivity: vi.fn(),
  updateActivity: vi.fn(),
  createOrdinaryActivity: vi.fn(),
  updateOrdinaryActivity: vi.fn(),
  logger: { error: vi.fn(), warn: vi.fn(), info: vi.fn(), debug: vi.fn() },
}));
vi.mock("@/hooks/use-accounts", () => {
  const accounts = [
    { id: "cash", name: "Daily Cash", currency: "CAD", accountType: "CASH", isActive: true },
  ];
  return { useAccounts: () => ({ accounts }) };
});
vi.mock("@/hooks/use-settings", () => {
  const data = { baseCurrency: "CAD" };
  return { useSettings: () => ({ data }) };
});
vi.mock("@/hooks/use-platform", () => ({ useIsMobileViewport: () => false }));
vi.mock("@/hooks/use-taxonomies", () => {
  const result = { data: null };
  return { useTaxonomy: () => result };
});
const spendingSettings = vi.hoisted(() => ({
  current: { enabled: true, accountIds: ["cash"] },
}));
vi.mock("../hooks/use-spending-settings", () => ({
  useSpendingSettings: () => ({ settings: spendingSettings.current }),
}));
vi.mock("../hooks/use-spending-events", () => {
  const empty = { data: [] };
  return { useSpendingEvents: () => empty, useEventTypes: () => empty };
});
vi.mock("./event-dialog-provider", () => ({
  useEventDialog: () => ({ openEventDialog: vi.fn(), openEventTypeDialog: vi.fn() }),
}));

function wrapper({ children }: { children: ReactNode }) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
}

const ordinaryActivity = {
  id: "act-ordinary",
  accountId: "cash",
  activityType: "WITHDRAWAL",
  activityDate: "2026-07-13T12:00:00.000Z",
  amount: "12.50",
  currency: "USD",
  fxRate: "1.37",
  notes: "Lunch",
  status: "POSTED",
  version: 1,
} as unknown as Activity;

const writeResult = {
  activity: ordinaryActivity,
  version: 2,
  requestId: "req-1",
  replayed: false,
};

describe("CashActivityForm ordinary branch", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    spendingSettings.current = { enabled: true, accountIds: ["cash"] };
    vi.mocked(updateOrdinaryActivity).mockResolvedValue(writeResult);
    vi.mocked(createOrdinaryActivity).mockResolvedValue(writeResult);
  });

  it("sends the version and FX rate through the unified update", async () => {
    const user = userEvent.setup();
    render(<CashActivityForm open onOpenChange={vi.fn()} activity={ordinaryActivity} />, {
      wrapper,
    });
    await user.click(await screen.findByRole("button", { name: /更新/ }));
    await waitFor(() => expect(updateOrdinaryActivity).toHaveBeenCalled());
    expect(vi.mocked(updateOrdinaryActivity).mock.calls[0][0]).toMatchObject({
      activityId: "act-ordinary",
      expectedVersion: 1,
      fxRate: "1.37",
      amount: "12.5",
    });
  });

  it("replays the frozen update after spending stops covering the account", async () => {
    vi.mocked(updateOrdinaryActivity).mockRejectedValueOnce(new TypeError("Failed to fetch"));
    const user = userEvent.setup();
    const { rerender } = render(
      <CashActivityForm open onOpenChange={vi.fn()} activity={ordinaryActivity} />,
      { wrapper },
    );
    await user.click(await screen.findByRole("button", { name: /更新/ }));
    await waitFor(() => expect(updateOrdinaryActivity).toHaveBeenCalledTimes(1));
    const first = vi.mocked(updateOrdinaryActivity).mock.calls[0][0];

    // The user disables spending (or moves the account out) before retrying.
    spendingSettings.current = { enabled: false, accountIds: [] };
    rerender(<CashActivityForm open onOpenChange={vi.fn()} activity={ordinaryActivity} />);
    await user.click(screen.getByRole("button", { name: /更新/ }));

    await waitFor(() => expect(updateOrdinaryActivity).toHaveBeenCalledTimes(2));
    expect(vi.mocked(updateOrdinaryActivity).mock.calls[1][0]).toEqual(first);
    expect(updateActivity).not.toHaveBeenCalled();
  });

  it("omits an unchanged category so saving notes keeps its splits", async () => {
    const user = userEvent.setup();
    render(
      <CashActivityForm
        open
        onOpenChange={vi.fn()}
        activity={
          {
            ...ordinaryActivity,
            categoryTaxonomyId: "spending_categories",
            categoryId: "food",
          } as unknown as Activity
        }
      />,
      { wrapper },
    );
    await user.click(await screen.findByRole("button", { name: /更新/ }));
    await waitFor(() => expect(updateOrdinaryActivity).toHaveBeenCalled());
    const payload = vi.mocked(updateOrdinaryActivity).mock.calls[0][0] as unknown as Record<
      string,
      unknown
    >;
    expect(payload).not.toHaveProperty("taxonomyId");
    expect(payload).not.toHaveProperty("categoryId");
  });

  it("starts a new request key when the form is reopened", async () => {
    const user = userEvent.setup();
    const { rerender } = render(
      <CashActivityForm open onOpenChange={vi.fn()} activity={ordinaryActivity} />,
      { wrapper },
    );
    await user.click(await screen.findByRole("button", { name: /更新/ }));
    await waitFor(() => expect(updateOrdinaryActivity).toHaveBeenCalledTimes(1));
    const firstKey = vi.mocked(updateOrdinaryActivity).mock.calls[0][0].requestKey;

    rerender(<CashActivityForm open={false} onOpenChange={vi.fn()} activity={ordinaryActivity} />);
    rerender(<CashActivityForm open onOpenChange={vi.fn()} activity={ordinaryActivity} />);
    await user.click(await screen.findByRole("button", { name: /更新/ }));
    await waitFor(() => expect(updateOrdinaryActivity).toHaveBeenCalledTimes(2));
    const secondKey = vi.mocked(updateOrdinaryActivity).mock.calls[1][0].requestKey;
    expect(secondKey).not.toBe(firstKey);
  });

  it("freezes the payload after an unknown result and resends it unchanged", async () => {
    vi.mocked(updateOrdinaryActivity).mockRejectedValueOnce(new TypeError("Failed to fetch"));
    const user = userEvent.setup();
    render(<CashActivityForm open onOpenChange={vi.fn()} activity={ordinaryActivity} />, {
      wrapper,
    });
    await user.click(await screen.findByRole("button", { name: /更新/ }));
    await waitFor(() => expect(updateOrdinaryActivity).toHaveBeenCalledTimes(1));
    const first = vi.mocked(updateOrdinaryActivity).mock.calls[0][0];

    const notes = screen.getByRole("textbox", { name: /备注/ });
    fireEvent.change(notes, { target: { value: "changed after timeout" } });
    await user.click(screen.getByRole("button", { name: /更新/ }));
    await waitFor(() => expect(updateOrdinaryActivity).toHaveBeenCalledTimes(2));
    const second = vi.mocked(updateOrdinaryActivity).mock.calls[1][0];
    expect(second).toEqual(first);
  });
});
