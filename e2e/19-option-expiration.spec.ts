import { expect, test } from "@playwright/test";
import { BASE_URL, completeOnboardingIfNeeded, createAccount } from "./helpers";

test("option expiration preserves month/day while entering the year", async ({ page }) => {
  test.setTimeout(180000);
  await completeOnboardingIfNeeded(page);
  await createAccount(page, "Option expiration regression", "USD");
  await page.goto(`${BASE_URL}/activities/manage?type=BUY`, { waitUntil: "domcontentloaded" });
  await page.getByRole("button", { name: "期权", exact: true }).click();

  // The first date field is the activity timestamp; the last is option expiration.
  // zh-CN DatePicker spinbutton names are 年/月/日 (not Year/Month/Day).
  const month = page.getByRole("spinbutton", { name: /^月/ }).last();
  const day = page.getByRole("spinbutton", { name: /^日/ }).last();
  const year = page.getByRole("spinbutton", { name: /^年/ }).last();
  await month.click();
  await page.keyboard.type("12");
  await day.click();
  await page.keyboard.type("31");
  await year.click();
  await page.keyboard.type("2");
  await expect(month).toHaveAttribute("aria-valuenow", "12");
  await expect(day).toHaveAttribute("aria-valuenow", "31");
  await page.keyboard.type("027");
  await page.keyboard.press("Tab");
  await expect(year).toHaveAttribute("aria-valuenow", "2027");
  await expect(month).toHaveAttribute("aria-valuenow", "12");
  await expect(day).toHaveAttribute("aria-valuenow", "31");
});
