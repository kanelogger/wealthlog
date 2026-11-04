import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { CreateAgentAccessTokenInput } from "@/adapters";

vi.mock("@/adapters", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/adapters")>();
  return { ...actual, isTauri: true };
});

vi.mock("@wealthfolio/ui/components/ui/use-toast", () => ({ toast: vi.fn() }));

import { PatCreateDialog } from "./pat-create-dialog";

describe("PatCreateDialog desktop scopes", () => {
  it("offers ordinary:write and still hides the retired write scopes", () => {
    render(
      <PatCreateDialog
        open
        onOpenChange={vi.fn()}
        onCreate={vi.fn().mockResolvedValue("wfp_secret")}
        isCreating={false}
      />,
    );
    expect(screen.getByRole("checkbox", { name: /普通收支——正式写入/ })).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: /内部现金转账——记账/ })).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: /手动持仓与现金——更新/ })).toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: /活动——提交\/写入/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: /分类——应用\/写入/ })).not.toBeInTheDocument();
  });
  it("ordinary investment writes do not grant confirmation or fact writes", async () => {
    const onCreate = vi
      .fn<(input: CreateAgentAccessTokenInput) => Promise<string>>()
      .mockResolvedValue("synthetic-token");
    render(<PatCreateDialog open onOpenChange={vi.fn()} onCreate={onCreate} isCreating={false} />);
    const write = screen.getByRole("checkbox", { name: /投资记录——编辑与起草/ });
    const confirm = screen.getByRole("checkbox", { name: /投资记录——确认与发布/ });
    expect(confirm).toHaveAttribute("aria-checked", "false");
    fireEvent.click(write);
    expect(confirm).toHaveAttribute("aria-checked", "false");
    fireEvent.change(screen.getByRole("textbox", { name: "名称" }), {
      target: { value: "Journal writer" },
    });
    fireEvent.click(screen.getByRole("button", { name: "创建令牌" }));
    await waitFor(() => expect(onCreate).toHaveBeenCalledOnce());
    const scopes = onCreate.mock.calls[0][0].scopes;
    expect(scopes).toContain("investments:write");
    expect(scopes).not.toContain("investments:confirm");
    expect(scopes).not.toContain("holdings:write");
    expect(scopes).not.toContain("transfers:write");
  });

  it("explicit confirmation includes its dependencies and can be removed", () => {
    render(<PatCreateDialog open onOpenChange={vi.fn()} onCreate={vi.fn()} isCreating={false} />);
    const confirm = screen.getByRole("checkbox", { name: /投资记录——确认与发布/ });
    const write = screen.getByRole("checkbox", { name: /投资记录——编辑与起草/ });
    fireEvent.click(confirm);
    expect(confirm).toHaveAttribute("aria-checked", "true");
    expect(write).toHaveAttribute("aria-checked", "true");
    expect(write).toBeDisabled();
    fireEvent.click(confirm);
    expect(confirm).toHaveAttribute("aria-checked", "false");
    expect(write).toHaveAttribute("aria-checked", "false");
    expect(write).not.toBeDisabled();
  });
});
