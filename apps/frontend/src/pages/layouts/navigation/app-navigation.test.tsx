import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
const platform = vi.hoisted(() => ({ desktop: false }));
vi.mock("@/adapters", () => ({
  get isTauri() {
    return platform.desktop;
  },
}));
import { isPathActive, useNavigation } from "./app-navigation";

describe("main navigation", () => {
  beforeEach(() => {
    platform.desktop = false;
    localStorage.removeItem("investment-journal-language");
  });
  it("offers activity and holdings without the removed modules", () => {
    const { result } = renderHook(() => useNavigation());
    expect(result.current.primary.map((item) => item.href)).toEqual(["/activities", "/holdings"]);
  });

  it("highlights nested activity paths", () => {
    expect(isPathActive("/activities/manage", "/activities")).toBe(true);
    expect(isPathActive("/dashboard", "/activities")).toBe(false);
  });
  it("adds investment navigation only on desktop and follows its local language", () => {
    platform.desktop = true;
    localStorage.setItem("investment-journal-language", JSON.stringify("en"));
    const { result } = renderHook(() => useNavigation());
    expect(result.current.primary.map((item) => item.href)).toEqual([
      "/activities",
      "/holdings",
      "/investments",
    ]);
    expect(result.current.primary[2].title).toBe("Investments");
  });
});
