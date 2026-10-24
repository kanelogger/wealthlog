import { expect, test } from "@playwright/test";
import { t } from "./i18n";
import { completeOnboardingIfNeeded, gotoAppPath } from "./helpers";

test("general settings stay in Simplified Chinese", async ({ page }) => {
  test.setTimeout(120000);
  await completeOnboardingIfNeeded(page);
  await gotoAppPath(page, "/settings/general");

  await expect(page.locator("html")).toHaveAttribute("lang", "zh-CN");
  await expect(page.getByRole("heading", { name: t("settings:general") })).toBeVisible();
  await expect(page.getByTestId("language-select")).toHaveCount(0);
  await expect(page.getByTestId("formatting-locale-select")).toHaveCount(0);
});
