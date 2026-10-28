import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@/test/render";
import type { ReactNode } from "react";
import { expect, it, vi } from "vitest";
import { updateBudgetGroup } from "../adapters/budget";
import { updateEventType } from "../adapters/events";
import { useBudgetMutations } from "./use-budget";
import { useEventTypeMutations } from "./use-spending-events";
import { QueryKeys } from "@/lib/query-keys";
vi.mock("../adapters/budget", () => ({ updateBudgetGroup: vi.fn(() => Promise.resolve()) }));
vi.mock("../adapters/events", () => ({ updateEventType: vi.fn(() => Promise.resolve()) }));

it("preserves raw names on unrelated budget group and event type edits", async () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const group = { id: "032ecb02-5912-42e8-9724-2cd566fc08d5", isSystem: true, name: "Needs" };
  const event = { id: "255600dc-c2a3-47e0-979d-a602c21fc337", name: "Travel" };
  client.setQueryData([QueryKeys.SPENDING_BUDGET, null], { state: { groups: [group] } });
  client.setQueryData([QueryKeys.SPENDING_EVENT_TYPES], [event]);
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  const { result } = renderHook(
    () => ({ budget: useBudgetMutations(), events: useEventTypeMutations() }),
    { wrapper },
  );
  await act(async () => {
    await result.current.budget.updateGroup.mutateAsync({
      id: group.id,
      patch: { name: "必要开支", color: "#123456" },
    });
    await result.current.events.update.mutateAsync({
      id: event.id,
      patch: { name: "旅行", color: "#123456" },
    });
  });
  expect(updateBudgetGroup).toHaveBeenLastCalledWith(
    group.id,
    { name: "Needs", color: "#123456" },
    undefined,
  );
  expect(updateEventType).toHaveBeenLastCalledWith(event.id, { name: "Travel", color: "#123456" });
  await act(async () => {
    await result.current.budget.updateGroup.mutateAsync({
      id: group.id,
      patch: { name: "My Needs" },
    });
    await result.current.events.update.mutateAsync({ id: event.id, patch: { name: "My Travel" } });
  });
  expect(updateBudgetGroup).toHaveBeenLastCalledWith(group.id, { name: "My Needs" }, undefined);
  expect(updateEventType).toHaveBeenLastCalledWith(event.id, { name: "My Travel" });
});
