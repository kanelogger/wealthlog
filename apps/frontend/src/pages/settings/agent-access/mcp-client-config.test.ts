import { describe, expect, it, vi } from "vitest";
import { buildClientConfig, CLIENT_PRESETS } from "./mcp-client-config";

vi.mock("@/adapters", () => ({ isWeb: false }));
vi.mock("@/features/profiles/session", () => ({ selectedProfileId: () => null }));

describe("MCP client branding", () => {
  it.each(CLIENT_PRESETS)("uses Wealthlog in $id configurations", ({ id }) => {
    const config = buildClientConfig(id, "http://localhost:1234/mcp");
    expect(config).not.toMatch(/wealthfolio/i);
    if (id !== "generic") expect(config).toMatch(/wealthlog/i);
    expect(JSON.parse(config)).toBeTruthy();
  });
});
