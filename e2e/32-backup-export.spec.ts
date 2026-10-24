import { expect, test } from "@playwright/test";
import { completeOnboardingIfNeeded, gotoAppPath } from "./helpers";

test.describe.configure({ mode: "serial" });

test("creates and exports a local database backup without restore controls", async ({ page }) => {
  test.setTimeout(120_000);
  await completeOnboardingIfNeeded(page);
  await gotoAppPath(page, "/settings/exports");

  await page.getByRole("tab", { name: "备份" }).click();
  const manager = page.getByRole("region", { name: "本地备份" });
  await expect(manager).toBeVisible();
  await expect(manager.getByRole("button", { name: /restore|import/i })).toHaveCount(0);

  await manager.getByRole("button", { name: "立即备份" }).click();
  await expect(page.getByText("备份已保存")).toBeVisible();

  const backupName = /^导出 wealthfolio_backup_\d{8}_\d{6}(?:_[a-z0-9]+)*\.db$/;
  const exportButton = manager.getByRole("button", { name: backupName }).first();
  await expect(exportButton).toBeVisible();
  await exportButton.click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toContainText("导出备份");
  await dialog.getByRole("radio", { name: "不设密码" }).click();
  const download = page.waitForEvent("download");
  await dialog.getByRole("button", { name: "不设密码并导出" }).click();
  expect((await download).suggestedFilename()).toMatch(/^wealthfolio-\d{8}-\d{6}\.db$/);
});
