import { renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { isPathActive, useNavigation } from "./app-navigation";

describe("main navigation", () => {
  it("offers activity and holdings without the removed modules", () => {
    const { result } = renderHook(() => useNavigation());
    expect(result.current.primary.map((item) => item.href)).toEqual(["/activities", "/holdings"]);
  });

  it("highlights nested activity paths", () => {
    expect(isPathActive("/activities/manage", "/activities")).toBe(true);
    expect(isPathActive("/dashboard", "/activities")).toBe(false);
  });
});
