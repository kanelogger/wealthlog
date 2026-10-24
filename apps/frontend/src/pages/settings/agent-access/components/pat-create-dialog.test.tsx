import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { PatCreateDialog } from "./pat-create-dialog";

vi.mock("@wealthfolio/ui/components/ui/use-toast", () => ({ toast: vi.fn() }));

function scopeCheckbox(label: string) {
  // Buttons expose `aria-label = "<label>: <description>"`; match on the label.
  return screen.getByRole("checkbox", { name: new RegExp(`^${label}`) });
}

describe("PatCreateDialog scopes", () => {
  it("creates a read-only token with retained scopes and no financial planning option", async () => {
    const onCreate = vi.fn().mockResolvedValue("wfp_secret");
    render(<PatCreateDialog open onOpenChange={vi.fn()} onCreate={onCreate} isCreating={false} />);

    expect(scopeCheckbox("持仓与价值")).toHaveAttribute("aria-checked", "true");
    expect(screen.queryByRole("checkbox", { name: /财务规划/ })).not.toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "名称" }), {
      target: { value: "Reader" },
    });
    fireEvent.click(screen.getByRole("button", { name: "创建令牌" }));

    await waitFor(() =>
      expect(onCreate).toHaveBeenCalledWith({
        name: "Reader",
        expiresAt: expect.any(String),
        scopes: [
          "accounts:read",
          "holdings:read",
          "performance:read",
          "activities:read",
          "health:read",
          "classification:read",
        ],
      }),
    );
  });

  it("hides the disabled formal-write scopes and keeps draft and suggest", () => {
    render(
      <PatCreateDialog
        open
        onOpenChange={vi.fn()}
        onCreate={vi.fn().mockResolvedValue("wfp_secret")}
        isCreating={false}
      />,
    );

    // Formal-write scopes cannot be granted for a new token this release.
    expect(screen.queryByRole("checkbox", { name: /活动——提交\/写入/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: /分类——应用\/写入/ })).not.toBeInTheDocument();

    // Drafts and classification suggestions remain selectable independently.
    const draft = scopeCheckbox("活动——草稿");
    const suggest = scopeCheckbox("分类——建议");
    fireEvent.click(draft);
    fireEvent.click(suggest);
    expect(draft).toHaveAttribute("aria-checked", "true");
    expect(suggest).toHaveAttribute("aria-checked", "true");
  });
});
