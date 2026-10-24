import { fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { DeleteProfileDialog } from "./delete-profile-dialog";
vi.mock("@/adapters", () => ({ isWeb: true, backupDatabase: vi.fn() }));
vi.mock("@/pages/settings/exports/backup-export-dialog", () => ({
  BackupExportDialog: () => null,
}));
const profile = { id: "a", name: "Personal", avatarId: "clay-pebble-animated", lockEnabled: false };
it("requires the exact profile name and explains server scope", () => {
  const onDelete = vi.fn();
  render(<DeleteProfileDialog profile={profile} onClose={vi.fn()} onDelete={onDelete} />);
  expect(screen.getByRole("heading", { name: /从此服务器/ })).toBeInTheDocument();
  const button = screen.getByRole("button", { name: "删除个人资料" });
  expect(button).toBeDisabled();
  fireEvent.change(screen.getByLabelText("输入 Personal 以确认"), {
    target: { value: "Personal" },
  });
  fireEvent.click(button);
  expect(onDelete).toHaveBeenCalledWith("Personal", "");
});
it("requires fresh proof for a protected profile", () => {
  const onDelete = vi.fn();
  render(
    <DeleteProfileDialog
      profile={{ ...profile, lockEnabled: true }}
      onClose={vi.fn()}
      onDelete={onDelete}
    />,
  );
  fireEvent.change(screen.getByLabelText("输入 Personal 以确认"), {
    target: { value: "Personal" },
  });
  const button = screen.getByRole("button", { name: "删除个人资料" });
  expect(button).toBeDisabled();
  fireEvent.change(screen.getByLabelText("当前密码或恢复代码"), {
    target: { value: "a long password" },
  });
  fireEvent.click(button);
  expect(onDelete).toHaveBeenCalledWith("Personal", "a long password");
});
it("shows a readable credential error beside the password and clears it when editing", () => {
  render(
    <DeleteProfileDialog
      profile={{ ...profile, lockEnabled: true }}
      error="PROFILE_PASSWORD_INVALID: The password or recovery code is incorrect."
      onClose={vi.fn()}
      onDelete={vi.fn()}
    />,
  );
  const input = screen.getByLabelText("当前密码或恢复代码");
  expect(input).toHaveAttribute("aria-invalid", "true");
  expect(input).toHaveAccessibleDescription(
    "密码或恢复代码不正确。请重试。",
  );
  expect(screen.queryByText(/PROFILE_PASSWORD_INVALID/)).not.toBeInTheDocument();
  fireEvent.change(input, { target: { value: "corrected password" } });
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});
it.each([
  ["PROFILE_COOLDOWN: Try again in 30 seconds.", "尝试次数过多。请在 30 秒后重试。"],
  [
    "PROFILE_UNAVAILABLE: internal database path",
    "无法删除此个人资料。请重试。",
  ],
])("makes deletion errors readable: %s", (error, message) => {
  render(
    <DeleteProfileDialog profile={profile} error={error} onClose={vi.fn()} onDelete={vi.fn()} />,
  );
  expect(screen.getByRole("alert")).toHaveTextContent(message);
  expect(screen.queryByText(/PROFILE_/)).not.toBeInTheDocument();
});
