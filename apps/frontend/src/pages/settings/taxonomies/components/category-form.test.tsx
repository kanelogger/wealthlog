import { render, screen, waitFor } from "@/test/render";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TaxonomyCategory } from "@/lib/types";
import { CategoryForm } from "./category-form";

const update = vi.fn();
vi.mock("@/hooks/use-taxonomies", () => ({
  useCreateCategory: () => ({ mutateAsync: vi.fn(), isPending: false }),
  useUpdateCategory: () => ({ mutateAsync: update, isPending: false }),
  useDeleteCategory: () => ({ mutateAsync: vi.fn(), isPending: false }),
}));
const category: TaxonomyCategory = {
  id: "STOCK_PREFERRED",
  taxonomyId: "instrument_type",
  name: "Preferred Stock",
  key: "STOCK_PREFERRED",
  color: "#4385be",
  description: "",
  sortOrder: 1,
  createdAt: "",
  updatedAt: "",
};

function showForm() {
  return render(
    <CategoryForm
      category={category}
      categoryLabel="优先股"
      taxonomyId="instrument_type"
      isSystem
      onClose={vi.fn()}
    />,
  );
}

describe("translated category form", () => {
  beforeEach(() => update.mockReset().mockResolvedValue(undefined));

  it("shows Chinese and preserves the stored name when editing another field", async () => {
    const user = userEvent.setup();
    showForm();
    expect(screen.getByRole("heading", { name: "优先股" })).toBeInTheDocument();
    expect(screen.getByDisplayValue("优先股")).toBeInTheDocument();
    await user.type(screen.getByRole("textbox", { name: "描述" }), "备注");
    await user.click(screen.getByRole("button", { name: "保存更改" }));
    await waitFor(() =>
      expect(update).toHaveBeenCalledWith(
        expect.objectContaining({ name: "Preferred Stock", description: "备注" }),
      ),
    );
  });

  it("saves an explicitly edited name", async () => {
    const user = userEvent.setup();
    showForm();
    const name = screen.getByDisplayValue("优先股");
    await user.clear(name);
    await user.type(name, "我的优先股");
    await user.click(screen.getByRole("button", { name: "保存更改" }));
    await waitFor(() =>
      expect(update).toHaveBeenCalledWith(expect.objectContaining({ name: "我的优先股" })),
    );
  });
});
