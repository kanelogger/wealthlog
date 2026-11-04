import { beforeEach, expect, it, vi } from "vitest";
const listeners = vi.hoisted(() => new Map<string, (event: unknown) => void>());
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((name: string, callback: (event: unknown) => void) => {
    listeners.set(name, callback);
    return Promise.resolve(vi.fn());
  }),
}));
beforeEach(() => {
  vi.resetModules();
  listeners.clear();
});
it("drops queued data from an older scope, including after a local lock", async () => {
  const session = await import("@/features/profiles/session");
  session.installProfileSession({ profileId: "B", scopeId: "scope-b" });
  const { listenPortfolioUpdateError } = await import("./events");
  const handler = vi.fn();
  await listenPortfolioUpdateError(handler);
  const deliver = (scopeId: string) =>
    listeners.get("portfolio:update-error")!({
      event: "portfolio:update-error",
      payload: { scopeId, data: "private" },
      id: 1,
    });
  deliver("scope-a");
  expect(handler).not.toHaveBeenCalled();
  deliver("scope-b");
  expect(handler).toHaveBeenCalledWith({
    event: "portfolio:update-error",
    payload: "private",
    id: 1,
  });
  session.revokeProfileSession();
  deliver("scope-b");
  expect(handler).toHaveBeenCalledTimes(1);
});

it("investment index notifications retain the profile scope boundary", async () => {
  const session = await import("@/features/profiles/session");
  session.installProfileSession({ profileId: "B", scopeId: "scope-b" });
  const { listenInvestmentIndexUpdate } = await import("./events");
  const handler = vi.fn();
  await listenInvestmentIndexUpdate(handler);
  const deliver = (scopeId: string) =>
    listeners.get("investment:index-updated")!({
      event: "investment:index-updated",
      payload: { scopeId, data: { complete: true } },
      id: 2,
    });
  deliver("scope-a");
  expect(handler).not.toHaveBeenCalled();
  deliver("scope-b");
  expect(handler).toHaveBeenCalledTimes(1);
  session.revokeProfileSession();
  deliver("scope-b");
  expect(handler).toHaveBeenCalledTimes(1);
});
