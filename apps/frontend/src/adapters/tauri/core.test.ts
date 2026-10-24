import { afterEach, expect, it, vi } from "vitest";
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
  profileScope: () => "profile",
  revokeProfileSession: vi.fn(),
}));
import { invoke } from "./core";
afterEach(() => {
  vi.useRealTimers();
  vi.resetAllMocks();
});
it("preserves the ordinary deadline for unrelated commands", async () => {
  vi.useFakeTimers();
  mocks.invoke.mockReturnValue(Promise.withResolvers<unknown>().promise);
  const result = invoke("get_accounts");
  const assertion = expect(result).rejects.toThrow('Command "get_accounts" timed out');
  await vi.advanceTimersByTimeAsync(300_001);
  await assertion;
});
