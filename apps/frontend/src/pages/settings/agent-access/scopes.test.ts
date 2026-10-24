import { describe, expect, it } from "vitest";
import {
  applyScopeDependencies,
  scopeAvailableOnHost,
  unavailableScopesForHost,
  WEB_UNAVAILABLE_SCOPES,
} from "./scopes";

describe("agent-access scopes", () => {
  it("offers ordinary:write only on the desktop host", () => {
    expect(unavailableScopesForHost(true)).not.toContain("ordinary:write");
    expect(unavailableScopesForHost(false)).toContain("ordinary:write");
    expect(WEB_UNAVAILABLE_SCOPES).toContain("ordinary:write");
    expect(scopeAvailableOnHost("ordinary:write", true)).toBe(true);
    expect(scopeAvailableOnHost("ordinary:write", false)).toBe(false);
  });

  it("adds the read dependencies required for ordinary writes", () => {
    expect(applyScopeDependencies(["ordinary:write"])).toEqual([
      "accounts:read",
      "activities:read",
      "ordinary:write",
    ]);
  });

  it("keeps existing dependency rules", () => {
    expect(applyScopeDependencies(["activities:write"])).toContain("activities:draft");
    expect(applyScopeDependencies(["classification:write"])).toContain(
      "classification:suggest",
    );
  });
});
