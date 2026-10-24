import { renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { useNameComparator } from "./use-name-comparator";

vi.mock("@wealthfolio/ui", () => ({
  useLocalizationSettings: () => ({ uiLocale: "zh" }),
}));

describe("useNameComparator", () => {
  it("sorts names with Simplified Chinese collation", () => {
    const { result } = renderHook(() => useNameComparator());
    const names = ["王", "陈", "李", "张"];

    expect([...names].sort(result.current)).toEqual(
      [...names].sort(new Intl.Collator("zh").compare),
    );
  });
});
