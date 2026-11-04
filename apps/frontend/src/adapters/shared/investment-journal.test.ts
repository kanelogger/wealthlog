import { beforeEach, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  desktop: true,
  invoke:
    vi.fn<
      (
        command: string,
        payload?: { command: string; args: Record<string, unknown> },
      ) => Promise<unknown>
    >(),
}));
vi.mock("./platform", () => ({
  get isTauri() {
    return mocks.desktop;
  },
  invoke: mocks.invoke,
}));

import { executeInvestmentJournal } from "./investment-journal";

beforeEach(() => {
  mocks.desktop = true;
  mocks.invoke.mockReset();
});

it("forwards financial strings, exact captures and a frozen request key unchanged", async () => {
  const args = {
    target: { decisionId: "decision", draftRevisionId: "draft", expectedVersion: 2 },
    expectedContentHash: "content",
    expectedContextHash: "context",
    context: { funding: { amount: "9007199254740993.1234", unavailable: null } },
    requestKey: "frozen",
  };
  const result = { requestId: "native-request", replayed: false };
  mocks.invoke.mockResolvedValue(result);
  expect(await executeInvestmentJournal("confirm_investment_decision", args)).toBe(result);
  expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith("execute_investment_journal", {
    command: "confirm_investment_decision",
    args,
  });
  expect(mocks.invoke.mock.calls[0][1]?.args).toBe(args);
});

it("keeps stable native conflict codes and never automatically retries a write", async () => {
  mocks.invoke.mockRejectedValue({
    code: "state_conflict",
    message: "Preview again",
    retryable: false,
  });
  await expect(
    executeInvestmentJournal("apply_fund_allocation", { requestKey: "same-key" }),
  ).rejects.toMatchObject({ code: "state_conflict", retryable: false });
  expect(mocks.invoke).toHaveBeenCalledTimes(1);
});

it("an unknown outcome retains the caller's frozen payload for explicit replay", async () => {
  const args = { requestKey: "same-key", expectedPreviewHash: "same-preview" };
  mocks.invoke.mockRejectedValueOnce(new Error("connection lost"));
  await expect(executeInvestmentJournal("apply_fund_allocation", args)).rejects.toMatchObject({
    code: "unknown_result",
    retryable: false,
  });
  expect(mocks.invoke).toHaveBeenCalledTimes(1);
  mocks.invoke.mockResolvedValueOnce({ replayed: true });
  expect(await executeInvestmentJournal("apply_fund_allocation", args)).toEqual({ replayed: true });
  expect(mocks.invoke.mock.calls[0][1]?.args).toBe(mocks.invoke.mock.calls[1][1]?.args);
});

it.each(["get_investment_context", "confirm_investment_decision"] as const)(
  "web rejects %s locally without invoking any platform or HTTP endpoint",
  async (command) => {
    mocks.desktop = false;
    await expect(executeInvestmentJournal(command, {})).rejects.toMatchObject({
      code: "desktop_only",
    });
    expect(mocks.invoke).not.toHaveBeenCalled();
  },
);
