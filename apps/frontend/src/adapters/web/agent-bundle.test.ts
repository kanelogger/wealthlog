import { afterEach, expect, it, vi } from "vitest";
import { getAgentBundleStatus, updateAgentBundle } from "./agent-bundle";

afterEach(() => vi.unstubAllGlobals());

it("reports unsupported locally without sending an installation REST request", async () => {
  const fetch = vi.fn();
  vi.stubGlobal("fetch", fetch);
  const status = await getAgentBundleStatus();
  expect(status.supported).toBe(false);
  expect(status.cli.state).toBe("unsupported");
  for (const action of [
    "reinstall",
    "remove",
    "repair",
    "use_copy",
    "restore",
    "keep_removed",
  ] as const) {
    await expect(updateAgentBundle(action, "cli")).rejects.toThrow("当前环境不支持");
  }
  expect(fetch).not.toHaveBeenCalled();
});
