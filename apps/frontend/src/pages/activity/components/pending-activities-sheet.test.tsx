import { fireEvent, render, screen, waitFor } from "@/test/render";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  confirmPendingActivity,
  getAccounts,
  listPendingActivities,
  updatePendingActivity,
} from "@/adapters";
import type { PendingActivity } from "@/lib/types";
import { PendingActivitiesSheet } from "./pending-activities-sheet";

vi.mock("@/adapters", () => ({
  getAccounts: vi.fn(),
  listPendingActivities: vi.fn(),
  updatePendingActivity: vi.fn(),
  confirmPendingActivity: vi.fn(),
  rejectPendingActivity: vi.fn(),
}));
vi.mock("sonner", () => ({ toast: { error: vi.fn(), success: vi.fn() } }));

const entry: PendingActivity = {
  id: "pend-1",
  status: "pending",
  accountId: "cash",
  activityType: "WITHDRAWAL",
  activityDate: "2026-04-05",
  currency: "EUR",
  amount: "12.50",
  fee: null,
  tax: null,
  notes: "Lunch",
  createdBy: "pat-1",
  confirmedBy: null,
  committedActivityId: null,
  version: 1,
  createdAt: "2026-04-05T00:00:00Z",
  updatedAt: "2026-04-05T00:00:00Z",
};

function wrapper({ children }: { children: ReactNode }) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
}

describe("PendingActivitiesSheet", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(getAccounts).mockResolvedValue([
      {
        id: "cash",
        name: "Daily Cash",
        currency: "EUR",
        accountType: "CASH",
        isArchived: false,
      },
    ] as never);
    vi.mocked(listPendingActivities).mockResolvedValue([entry]);
    vi.mocked(updatePendingActivity).mockResolvedValue({ ...entry, version: 2, amount: "40.00" });
    vi.mocked(confirmPendingActivity).mockResolvedValue({
      pending: { ...entry, status: "committed", version: 3 },
      activity: null,
      replayed: false,
    });
  });

  it("saves the edited amount before confirming and uses the new version", async () => {
    render(<PendingActivitiesSheet open onOpenChange={vi.fn()} />, { wrapper });

    const amount = await screen.findByLabelText("金额");
    fireEvent.change(amount, { target: { value: "40.00" } });
    fireEvent.click(screen.getByRole("button", { name: /确认入账/ }));

    await waitFor(() => expect(updatePendingActivity).toHaveBeenCalled());
    expect(vi.mocked(updatePendingActivity).mock.calls[0]).toEqual([
      "pend-1",
      expect.objectContaining({ expectedVersion: 1, amount: "40.00", accountId: "cash" }),
    ]);
    await waitFor(() =>
      expect(vi.mocked(confirmPendingActivity)).toHaveBeenCalledWith("pend-1", 2),
    );
  });

  it("retries confirm with the version the failed attempt saved", async () => {
    vi.mocked(listPendingActivities)
      .mockResolvedValueOnce([entry])
      .mockResolvedValue([{ ...entry, version: 2 }]);
    vi.mocked(confirmPendingActivity).mockRejectedValueOnce(new Error("offline"));

    render(<PendingActivitiesSheet open onOpenChange={vi.fn()} />, { wrapper });
    const button = await screen.findByRole("button", { name: /确认入账/ });
    fireEvent.click(button);

    await waitFor(() =>
      expect(vi.mocked(confirmPendingActivity)).toHaveBeenCalledWith("pend-1", 2),
    );
    // Let the failed-confirm refetch settle.
    await waitFor(() => expect(listPendingActivities).toHaveBeenCalledTimes(2));

    fireEvent.click(screen.getByRole("button", { name: /确认入账/ }));
    await waitFor(() => expect(vi.mocked(updatePendingActivity)).toHaveBeenCalledTimes(2));
    expect(vi.mocked(updatePendingActivity).mock.calls[1][1]).toEqual(
      expect.objectContaining({ expectedVersion: 2 }),
    );
    await waitFor(() =>
      expect(vi.mocked(confirmPendingActivity)).toHaveBeenLastCalledWith("pend-1", 2),
    );
  });
});
