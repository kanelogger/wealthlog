import { expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/plugin-log", () => ({
  error: vi.fn(),
  warn: vi.fn(),
  info: vi.fn(),
  debug: vi.fn(),
  trace: vi.fn(),
}));
vi.mock("@/features/profiles/session", () => ({
  profileScope: () => {
    throw new Error("No open ledger");
  },
  revokeProfileSession: vi.fn(),
}));

import { getAgentBundleStatus, updateAgentBundle } from "./agent-bundle";

it("uses application commands without requiring an open ledger", async () => {
  mocks.invoke.mockResolvedValue({ supported: true });
  await expect(getAgentBundleStatus()).resolves.toEqual({ supported: true });
  await updateAgentBundle("reinstall", "skill");
  expect(mocks.invoke).toHaveBeenCalledWith("agent_bundle_status", undefined);
  expect(mocks.invoke).toHaveBeenCalledWith("agent_bundle_action", {
    action: "reinstall",
    tool: "skill",
  });
});
