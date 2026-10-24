import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AccountScopeSelector } from "./account-filter-selector";

const viewport = vi.hoisted(() => ({ mobile: false }));
const inventory = vi.hoisted(() => ({
  portfolios: [{ id: "retirement", name: "Retirement" }] as
    | { id: string; name: string }[]
    | undefined,
  status: "success" as "success" | "pending" | "error",
}));
vi.mock("@/hooks", () => ({ useIsMobileViewport: () => viewport.mobile }));
vi.mock("@/hooks/use-accounts", () => ({
  useAccounts: () => ({ accounts: [{ id: "bank", name: "Everyday bank", currency: "USD" }] }),
}));
vi.mock("@/hooks/use-portfolios", () => ({
  usePortfolios: () => ({
    data: inventory.portfolios,
    isSuccess: inventory.status === "success",
    isPending: inventory.status === "pending",
  }),
}));

describe("AccountScopeSelector", () => {
  beforeEach(() => {
    viewport.mobile = false;
    inventory.portfolios = [{ id: "retirement", name: "Retirement" }];
    inventory.status = "success";
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    );
    Element.prototype.scrollIntoView = vi.fn();
  });
  it("offers all accounts and saved portfolios without individual accounts in portfolio mode", async () => {
    const onChange = vi.fn();
    render(
      <AccountScopeSelector
        value={{ type: "all" }}
        onChange={onChange}
        onManagePortfolios={vi.fn()}
        portfoliosOnly
        triggerVariant="icon"
      />,
    );
    await userEvent.click(screen.getByRole("combobox", { name: "选择投资组合: 所有账户" }));
    expect(screen.getByRole("option", { name: "所有账户" })).toBeVisible();
    expect(screen.getByRole("button", { name: "管理投资组合" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "添加投资组合" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "编辑投资组合" })).not.toBeInTheDocument();
    expect(screen.queryByRole("option", { name: /Everyday bank/ })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("option", { name: "Retirement" }));
    expect(onChange).toHaveBeenCalledWith({ type: "portfolio", portfolioId: "retirement" });
    expect(screen.queryByRole("option", { name: "Retirement" })).not.toBeInTheDocument();
  });
  it("keeps individual account selection available for existing consumers", async () => {
    render(<AccountScopeSelector value={{ type: "all" }} onChange={vi.fn()} />);
    await userEvent.click(screen.getByRole("combobox"));
    expect(screen.getByRole("option", { name: /Everyday bank/ })).toBeVisible();
  });
  it("offers a single Add portfolio action when no saved portfolios exist", async () => {
    inventory.portfolios = [];
    const onManage = vi.fn();
    render(
      <AccountScopeSelector
        value={{ type: "all" }}
        onChange={vi.fn()}
        portfoliosOnly
        triggerVariant="icon"
        onManagePortfolios={onManage}
      />,
    );
    await userEvent.click(screen.getByRole("combobox", { name: "选择投资组合: 所有账户" }));
    expect(screen.getAllByRole("option")).toHaveLength(1);
    expect(screen.queryByRole("button", { name: "管理投资组合" })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "添加投资组合" }));
    expect(onManage).toHaveBeenCalledOnce();
    expect(screen.queryByRole("option")).not.toBeInTheDocument();
  });

  it("uses an accessible mobile sheet and closes it before navigating to management", async () => {
    viewport.mobile = true;
    const onManage = vi.fn();
    render(
      <AccountScopeSelector
        value={{ type: "all" }}
        onChange={vi.fn()}
        portfoliosOnly
        triggerVariant="icon"
        onManagePortfolios={onManage}
      />,
    );
    await userEvent.click(screen.getByRole("combobox", { name: "选择投资组合: 所有账户" }));
    const sheet = screen.getByRole("dialog", { name: "选择投资组合" });
    expect(within(sheet).getByRole("option", { name: "Retirement" })).toBeVisible();
    await userEvent.click(within(sheet).getByRole("button", { name: "管理投资组合" }));
    expect(onManage).toHaveBeenCalledOnce();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it.each(["pending", "error"] as const)(
    "offers Manage portfolios when inventory is %s rather than confirmed empty",
    async (status) => {
      inventory.portfolios = undefined;
      inventory.status = status;
      render(
        <AccountScopeSelector
          value={{ type: "all" }}
          onChange={vi.fn()}
          portfoliosOnly
          triggerVariant="icon"
          onManagePortfolios={vi.fn()}
        />,
      );
      await userEvent.click(
        screen.getByRole("combobox", { name: "选择投资组合: 所有账户" }),
      );
      expect(screen.getByRole("button", { name: "管理投资组合" })).toBeVisible();
      expect(screen.queryByRole("button", { name: "添加投资组合" })).not.toBeInTheDocument();
    },
  );

  it.each([
    { mobile: false, empty: false, action: "管理投资组合" },
    { mobile: true, empty: false, action: "管理投资组合" },
    { mobile: false, empty: true, action: "添加投资组合" },
    { mobile: true, empty: true, action: "添加投资组合" },
  ])("keeps $action available during an unmatched search (mobile: $mobile)", async (scenario) => {
    viewport.mobile = scenario.mobile;
    if (scenario.empty) inventory.portfolios = [];
    const onManage = vi.fn();
    render(
      <AccountScopeSelector
        value={{ type: "all" }}
        onChange={vi.fn()}
        portfoliosOnly
        triggerVariant="icon"
        onManagePortfolios={onManage}
      />,
    );
    await userEvent.click(screen.getByRole("combobox", { name: "选择投资组合: 所有账户" }));
    await userEvent.type(screen.getByPlaceholderText("搜索投资组合…"), "unmatched xyz");
    expect(screen.getByText("无结果。")).toBeVisible();
    expect(screen.getByRole("separator")).toBeVisible();
    expect(screen.queryByRole("option", { name: "所有账户" })).not.toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "Retirement" })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: scenario.action }));
    expect(onManage).toHaveBeenCalledOnce();
    expect(screen.queryByRole("option")).not.toBeInTheDocument();
  });

  it.each([false, true])(
    "keeps matching results above the keyboard-accessible footer (mobile: %s)",
    async (mobile) => {
      viewport.mobile = mobile;
      const onChange = vi.fn();
      const onManage = vi.fn();
      render(
        <AccountScopeSelector
          value={{ type: "all" }}
          onChange={onChange}
          portfoliosOnly
          triggerVariant="icon"
          onManagePortfolios={onManage}
        />,
      );
      await userEvent.click(
        screen.getByRole("combobox", { name: "选择投资组合: 所有账户" }),
      );
      await userEvent.type(screen.getByPlaceholderText("搜索投资组合…"), "Retirement");
      const result = screen.getByRole("option", { name: "Retirement" });
      const separator = screen.getByRole("separator");
      const manage = screen.getByRole("button", { name: "管理投资组合" });
      expect(
        result.compareDocumentPosition(separator) & Node.DOCUMENT_POSITION_FOLLOWING,
      ).toBeTruthy();
      expect(
        separator.compareDocumentPosition(manage) & Node.DOCUMENT_POSITION_FOLLOWING,
      ).toBeTruthy();
      expect(screen.getByRole("listbox")).not.toContainElement(manage);
      await userEvent.tab();
      expect(manage).toHaveFocus();
      await userEvent.keyboard("{Enter}");
      expect(onManage).toHaveBeenCalledOnce();
      expect(onChange).not.toHaveBeenCalled();
      expect(screen.queryByRole("option")).not.toBeInTheDocument();
    },
  );
});
