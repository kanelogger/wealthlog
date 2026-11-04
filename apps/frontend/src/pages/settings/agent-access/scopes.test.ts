import { describe, expect, it } from "vitest";
import {
  applyScopeDependencies,
  readScopesForHost,
  matchPreset,
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

  it("grants portfolio writes explicitly on desktop with matching read prerequisites", () => {
    for (const scope of ["holdings:write", "transfers:write"]) {
      expect(unavailableScopesForHost(true)).not.toContain(scope);
      expect(unavailableScopesForHost(false)).toContain(scope);
      expect(scopeAvailableOnHost(scope, false)).toBe(false);
    }
    expect(applyScopeDependencies(["holdings:write"])).toEqual([
      "accounts:read",
      "holdings:read",
      "activities:read",
      "holdings:write",
    ]);
    expect(applyScopeDependencies(["transfers:write"])).toEqual([
      "accounts:read",
      "activities:read",
      "transfers:write",
    ]);
  });

  it("keeps journal confirmation independent and desktop-only", () => {
    expect(applyScopeDependencies(["investments:write"])).toEqual([
      "accounts:read",
      "holdings:read",
      "activities:read",
      "investments:read",
      "investments:write",
    ]);
    expect(applyScopeDependencies(["investments:confirm"])).toEqual([
      "accounts:read",
      "holdings:read",
      "activities:read",
      "investments:read",
      "investments:write",
      "investments:confirm",
    ]);
    for (const scope of ["investments:read", "investments:write", "investments:confirm"]) {
      expect(unavailableScopesForHost(true)).not.toContain(scope);
      expect(unavailableScopesForHost(false)).toContain(scope);
      expect(scopeAvailableOnHost(scope, false)).toBe(false);
    }
    expect(readScopesForHost(true)).toContain("investments:read");
    expect(readScopesForHost(false)).not.toContain("investments:read");
  });

  it("recognizes old read presets without adding new token grants", () => {
    const old = [
      "accounts:read",
      "holdings:read",
      "performance:read",
      "activities:read",
      "health:read",
      "classification:read",
    ];
    const preset = matchPreset(old);
    expect(preset?.key).toBe("read-only");
    expect(preset?.scopes).toEqual(old);
    expect(old).not.toContain("investments:read");
  });

  it("keeps existing dependency rules", () => {
    expect(applyScopeDependencies(["activities:write"])).toContain("activities:draft");
    expect(applyScopeDependencies(["classification:write"])).toContain("classification:suggest");
  });
});
