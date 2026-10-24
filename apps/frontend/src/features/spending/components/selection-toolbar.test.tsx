import { render, screen } from "@/test/render";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { SelectionToolbar } from "./selection-toolbar";

function renderToolbar(overrides: Partial<React.ComponentProps<typeof SelectionToolbar>> = {}) {
  const props = {
    rowCount: 3,
    selectionMode: false,
    onEnterSelectionMode: vi.fn(),
    onExitSelectionMode: vi.fn(),
    allVisibleSelected: false,
    someVisibleSelected: false,
    onToggleSelectAllVisible: vi.fn(),
    ...overrides,
  };
  render(<SelectionToolbar {...props} />);
  return props;
}

describe("SelectionToolbar", () => {
  /**
   * This is the only way into selection mode on mobile, so gating it on more
   * than one row put bulk actions out of reach whenever a filter matched a
   * single transaction.
   */
  it("offers Select for a single row", async () => {
    const user = userEvent.setup();
    const props = renderToolbar({ rowCount: 1 });

    await user.click(screen.getByRole("button", { name: "选择" }));

    expect(props.onEnterSelectionMode).toHaveBeenCalledTimes(1);
  });

  it("renders nothing when there are no rows", () => {
    renderToolbar({ rowCount: 0 });

    expect(screen.queryByRole("button", { name: "选择" })).not.toBeInTheDocument();
  });

  it("swaps Select for the select-all control and Cancel once selecting", async () => {
    const user = userEvent.setup();
    const props = renderToolbar({ selectionMode: true });

    expect(screen.queryByRole("button", { name: "选择" })).not.toBeInTheDocument();
    expect(screen.getByText("全选")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "取消" }));

    expect(props.onExitSelectionMode).toHaveBeenCalledTimes(1);
  });

  it("labels the select-all control by what clicking it will do", () => {
    renderToolbar({ selectionMode: true, allVisibleSelected: true });

    expect(
      screen.getByRole("checkbox", { name: "取消选择所有可见交易" }),
    ).toBeInTheDocument();
  });
});
