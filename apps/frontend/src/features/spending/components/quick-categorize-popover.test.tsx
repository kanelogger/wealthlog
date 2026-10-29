import { fireEvent, render, screen, waitFor } from "@/test/render";
import userEvent from "@testing-library/user-event";
import { Sheet, SheetContent, SheetTitle } from "@wealthfolio/ui";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { QuickCategorizePopover } from "./quick-categorize-popover";

const originalScrollIntoView = Object.getOwnPropertyDescriptor(Element.prototype, "scrollIntoView");

vi.mock("@/hooks/use-taxonomies", () => ({
  useTaxonomy: () => ({
    data: {
      categories: Array.from({ length: 30 }, (_, index) => ({
        id: `category-${index}`,
        name: `Category ${index}`,
        sortOrder: index,
        parentId: null,
      })),
    },
  }),
}));

describe("QuickCategorizePopover inside a sheet", () => {
  beforeEach(() => {
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe = vi.fn();
        unobserve = vi.fn();
        disconnect = vi.fn();
      },
    );
    Element.prototype.scrollIntoView = vi.fn();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    if (originalScrollIntoView) {
      Object.defineProperty(Element.prototype, "scrollIntoView", originalScrollIntoView);
    } else {
      Reflect.deleteProperty(Element.prototype, "scrollIntoView");
    }
  });

  it("allows wheel scrolling in the category list and keeps selection working", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(
      <Sheet open>
        <SheetContent aria-describedby={undefined}>
          <SheetTitle>Transaction</SheetTitle>
          <QuickCategorizePopover
            trigger={<button>Choose category</button>}
            scope="expense"
            onSelect={onSelect}
          />
        </SheetContent>
      </Sheet>,
    );

    await user.click(screen.getByRole("button", { name: "Choose category" }));
    const list = await screen.findByRole("listbox");
    // jsdom has no layout; give the real scroll lock a scrollable list to inspect.
    Object.defineProperties(list, {
      scrollHeight: { configurable: true, value: 1000 },
      clientHeight: { configurable: true, value: 300 },
    });
    list.style.overflowY = "auto";

    await waitFor(() => {
      const wheel = new WheelEvent("wheel", { bubbles: true, cancelable: true, deltaY: 100 });
      fireEvent(list, wheel);
      expect(wheel.defaultPrevented).toBe(false);
    });

    await user.click(screen.getByRole("option", { name: "Category 29", exact: true }));
    expect(onSelect).toHaveBeenCalledWith("spending_categories", "category-29");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Transaction" })).toBeInTheDocument();
  });
});
