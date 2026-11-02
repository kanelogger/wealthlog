import { expect, Page, test } from "@playwright/test";
import { BASE_URL, loginIfNeeded, selectFirstAccount } from "./helpers";

test.describe.configure({ mode: "serial" });

test.describe("Activity Form Validation", () => {
  let page: Page;

  test.beforeAll(async ({ browser }) => {
    page = await browser.newPage();
  });

  test.afterAll(async () => {
    await page.close();
  });

  test("1. Setup: login", async () => {
    test.setTimeout(180000);
    await loginIfNeeded(page);
    await page.goto(`${BASE_URL}/activities`, { waitUntil: "domcontentloaded" });
    await expect(page).toHaveURL(/\/activities/);
  });

  test("2. Deposit — missing account and amount shows errors", async () => {
    await page.goto(`${BASE_URL}/activities/manage?type=DEPOSIT`, {
      waitUntil: "domcontentloaded",
    });
    await page.waitForTimeout(1000);

    // Submit without filling anything
    const submitButton = page.getByRole("button", { name: /添加存入/i });
    await expect(submitButton).toBeVisible({ timeout: 10000 });
    await submitButton.click();
    await page.waitForTimeout(500);

    // Both errors should be visible (exact match avoids strict mode violation with toast)
    await expect(page.getByText("请选择一个账户。", { exact: true }).first()).toBeVisible({
      timeout: 5000,
    });
    await expect(page.getByText("金额必须为数字。", { exact: true }).first()).toBeVisible({
      timeout: 5000,
    });
  });

  test("3. Deposit — explicit zero is accepted", async () => {
    await page.goto(`${BASE_URL}/activities/manage?type=DEPOSIT&redirect-to=%2Factivities`, {
      waitUntil: "domcontentloaded",
    });

    await selectFirstAccount(page);

    await page.getByTestId("amount-input").fill("0");
    await page.getByRole("button", { name: /添加存入/i }).click();

    await expect(page).toHaveURL(`${BASE_URL}/activities`);
  });

  test("4. Buy — missing symbol shows error", async () => {
    await page.goto(`${BASE_URL}/activities/manage?type=BUY`, { waitUntil: "domcontentloaded" });
    await page.waitForTimeout(1000);

    // Select an account
    await selectFirstAccount(page);

    // Fill quantity and price but NOT symbol
    const quantityInput = page.getByTestId("quantity-input");
    await quantityInput.fill("10");
    await quantityInput.blur();

    const priceInput = page.getByTestId("price-input");
    await priceInput.fill("100");
    await priceInput.blur();
    await page.waitForTimeout(200);

    const submitButton = page.getByRole("button", { name: /添加买入/i });
    await submitButton.click();
    await page.waitForTimeout(500);

    // Should show symbol-related validation error
    await expect(page.getByText("请输入代码。", { exact: true }).first()).toBeVisible({
      timeout: 5000,
    });
  });

  test("5. Buy — zero quantity shows error", async () => {
    await page.goto(`${BASE_URL}/activities/manage?type=BUY`, { waitUntil: "domcontentloaded" });
    await page.waitForTimeout(1000);

    // Select an account
    await selectFirstAccount(page);

    // Fill quantity as 0
    const quantityInput = page.getByTestId("quantity-input");
    await quantityInput.fill("0");
    await quantityInput.blur();

    const priceInput = page.getByTestId("price-input");
    await priceInput.fill("100");
    await priceInput.blur();
    await page.waitForTimeout(200);

    const submitButton = page.getByRole("button", { name: /添加买入/i });
    await submitButton.click();
    await page.waitForTimeout(500);

    // Should show quantity error
    await expect(page.getByText("数量必须大于 0。", { exact: true }).first()).toBeVisible({
      timeout: 5000,
    });
  });
});
