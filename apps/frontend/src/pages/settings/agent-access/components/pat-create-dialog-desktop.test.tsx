import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

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
});
