import { expect, test, type Page } from "@playwright/test";
import { BASE_URL, completeOnboardingIfNeeded } from "./helpers";

function parseMoney(text: string | null) {
  const match = text?.replace(/,/g, "").match(/-?\d+(?:\.\d+)?/);
  return match ? Number(match[0]) : NaN;
}

async function allAccountsTotal(page: Page) {
  const response = await page.request.post(`${BASE_URL}/api/v1/valuations/current/query`, {
    data: { filter: { type: "all" }, includeAccounts: false },
  });
  if (!response.ok()) return NaN;
  const valuation = (await response.json()) as {
    summary?: { totalValueBase?: number | string };
  };
  const total = Number(valuation.summary?.totalValueBase);
  return Number.isFinite(total) ? Number(total.toFixed(2)) : NaN;
}

async function expectBalanceMatchesAllAccounts(
  page: Page,
  balanceValue: ReturnType<Page["getByTestId"]>,
) {
  await expect
    .poll(
      async () => {
        const [expected, text] = await Promise.all([
          allAccountsTotal(page),
          balanceValue.textContent(),
        ]);
        return Number.isFinite(expected) ? Math.abs(parseMoney(text) - expected) : NaN;
      },
      { timeout: 30_000, intervals: [500, 1_000, 2_000] },
    )
    .toBeLessThanOrEqual(0.01);
}

test("dashboard portfolio selection, creation, persistence, deletion and mobile layout", async ({
  page,
}, testInfo) => {
  test.setTimeout(180000);
  await completeOnboardingIfNeeded(page);
  const accountIds: string[] = [];
  for (const [name, amount] of [
    ["Portfolio filter savings", 1000000],
    ["Portfolio filter checking", 200],
  ] as const) {
    const response = await page.request.post(`${BASE_URL}/api/v1/accounts`, {
      data: {
        name,
        accountType: "CASH",
        currency: "CAD",
        isDefault: false,
        isActive: true,
        trackingMode: "TRANSACTIONS",
      },
    });
    expect(response.ok()).toBe(true);
    const { id } = (await response.json()) as { id: string };
    accountIds.push(id);
    const deposit = await page.request.post(`${BASE_URL}/api/v1/activities`, {
      data: {
        accountId: id,
        activityType: "DEPOSIT",
        activityDate: "2026-05-01T12:00:00Z",
        amount,
        currency: "CAD",
        fee: 0,
      },
    });
    expect(deposit.ok()).toBe(true);
  }
  await page.goto(`${BASE_URL}/dashboard`, { waitUntil: "domcontentloaded" });
  const filter = page.getByRole("combobox", { name: /^选择投资组合:/ });
  // Mobile keeps neighbouring views mounted; scope assertions to the investment headline.
  const headlineContainer = page
    .getByRole("button", { name: /^(隐藏余额|显示余额)$/ })
    .locator("..");
  const balanceValue = headlineContainer.getByTestId("portfolio-balance-value");
  await expect(filter).toHaveAccessibleName("选择投资组合: 所有账户");
  await expect(
    headlineContainer.locator("..").getByText("所有账户", { exact: true }),
  ).toHaveCount(0);
  // Suite-shared DB: "所有账户" includes prior fixtures — match the valuation API.
  await expectBalanceMatchesAllAccounts(page, balanceValue);
  const privacy = page.getByRole("button", { name: /^(隐藏余额|显示余额)$/ });
  expect(await privacy.evaluate((element) => element.closest("a"))).toBeNull();
  await privacy.focus();
  await expect(privacy).toBeFocused();
  // Focus used to open the update hover card after its 700ms delay.
  await page.waitForTimeout(800);
  await expect(page.getByRole("button", { name: "更新价格", exact: true })).toHaveCount(0);
  await privacy.press("Enter");
  await expect(headlineContainer.getByTestId("portfolio-balance")).toContainText("•••");
  await privacy.press("Enter");
  await expectBalanceMatchesAllAccounts(page, balanceValue);
  await headlineContainer.getByTestId("portfolio-balance").hover();
  await expect(page.getByRole("button", { name: "更新价格", exact: true })).toBeVisible();
  await page.mouse.move(0, 0);
  await expect(page.getByRole("button", { name: "更新价格", exact: true })).not.toBeVisible();
  await filter.click();
  await page.getByRole("button", { name: "添加投资组合" }).click();
  await expect(page).toHaveURL(`${BASE_URL}/settings/portfolios`);
  await expect(page.getByRole("dialog")).toHaveCount(0);
  // Empty state and page header both expose the same CTA.
  await page.getByRole("button", { name: "添加投资组合", exact: true }).first().click();
  const dialog = page.getByRole("dialog", { name: "新建投资组合" });
  await dialog.getByLabel("名称", { exact: true }).fill("Dashboard savings");
  await dialog
    .locator("label")
    .filter({ hasText: "Portfolio filter savings" })
    .getByRole("checkbox")
    .click();
  await dialog.getByRole("button", { name: "保存", exact: true }).click();
  await expect(dialog).not.toBeVisible();
  await page.goto(`${BASE_URL}/dashboard`, { waitUntil: "domcontentloaded" });
  await expect(filter).toHaveAccessibleName("选择投资组合: 所有账户");
  await filter.click();
  await expect(page.getByRole("button", { name: "添加投资组合" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "编辑投资组合" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "管理投资组合" })).toBeVisible();
  await page.getByRole("option", { name: "Dashboard savings" }).click();
  await expect(filter).toHaveAccessibleName("选择投资组合: Dashboard savings");
  await expect(
    headlineContainer.locator("..").getByText("Dashboard savings", { exact: true }),
  ).toBeVisible();
  await expect(balanceValue).toContainText("1,000,000.00", {
    timeout: 30000,
  });
  await expect(page.locator(`a[href="/accounts/${accountIds[0]}"]`)).toBeVisible();
  await expect(page.locator(`a[href="/accounts/${accountIds[1]}"]`)).toHaveCount(0);
  await page.screenshot({ path: testInfo.outputPath("desktop-dashboard.png") });
  await filter.click();
  await expect
    .poll(() => page.getByRole("dialog").evaluate((element) => getComputedStyle(element).opacity))
    .toBe("1");
  const optionIcon = await page
    .getByRole("option", { name: "Dashboard savings" })
    .locator("svg")
    .first()
    .boundingBox();
  const footerIcon = await page
    .getByRole("button", { name: "管理投资组合" })
    .locator("svg")
    .first()
    .boundingBox();
  expect(optionIcon).not.toBeNull();
  expect(footerIcon).not.toBeNull();
  expect(footerIcon!.x).toBeCloseTo(optionIcon!.x, 0);
  await page.screenshot({ path: testInfo.outputPath("desktop-portfolio-filter.png") });
  await page.getByPlaceholder("搜索投资组合…").fill("unmatched xyz");
  await expect(page.getByText("无结果。", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "管理投资组合" })).toBeVisible();
  await page.getByPlaceholder("搜索投资组合…").fill("Dashboard savings");
  await expect(page.getByRole("option", { name: "Dashboard savings" })).toBeVisible();
  await page.keyboard.press("Tab");
  await expect(page.getByRole("button", { name: "管理投资组合" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(`${BASE_URL}/settings/portfolios`);
  await page.getByRole("button", { name: "打开", exact: true }).click();
  await page.getByRole("menuitem", { name: "编辑", exact: true }).click();
  const editor = page.getByRole("dialog", { name: "编辑投资组合" });
  await editor
    .locator("label")
    .filter({ hasText: "Portfolio filter checking" })
    .getByRole("checkbox")
    .click();
  await editor.getByRole("button", { name: "保存", exact: true }).click();
  await expect(editor).not.toBeVisible();
  await page.goto(`${BASE_URL}/dashboard`, { waitUntil: "domcontentloaded" });
  await expect(filter).toHaveAccessibleName("选择投资组合: Dashboard savings");
  await expect(balanceValue).toContainText("1,000,200.00");
  await expect(page.locator(`a[href="/accounts/${accountIds[1]}"]`)).toBeVisible();
  await filter.click();
  await page.getByRole("button", { name: "管理投资组合" }).click();
  await expect(page).toHaveURL(`${BASE_URL}/settings/portfolios`);
  await page.getByRole("button", { name: "打开", exact: true }).click();
  await page.getByRole("menuitem", { name: "编辑", exact: true }).click();
  await editor
    .locator("label")
    .filter({ hasText: "Portfolio filter checking" })
    .getByRole("checkbox")
    .click();
  await editor.getByRole("button", { name: "保存", exact: true }).click();
  await expect(editor).not.toBeVisible();
  await page.goto(`${BASE_URL}/dashboard`, { waitUntil: "domcontentloaded" });
  await expect(balanceValue).toContainText("1,000,000.00");
  await page.reload();
  await expect(filter).toHaveAccessibleName("选择投资组合: Dashboard savings");
  await filter.click();
  await page.getByRole("option", { name: "所有账户" }).click();
  await expectBalanceMatchesAllAccounts(page, balanceValue);
  await filter.click();
  await page.getByRole("option", { name: "Dashboard savings" }).click();

  await page.setViewportSize({ width: 320, height: 740 });
  await expect(filter).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath("mobile-dashboard.png") });
  await filter.click();
  const sheet = page.getByRole("dialog", { name: "选择投资组合" });
  await expect(sheet).toBeVisible();
  await expect(sheet.getByRole("heading", { name: "选择投资组合" })).toBeFocused();
  await expect(async () => {
    const sheetBounds = await sheet.boundingBox();
    expect(sheetBounds).not.toBeNull();
    expect(sheetBounds?.width).toBeLessThanOrEqual(320);
    expect(sheetBounds?.y).toBeGreaterThanOrEqual(0);
    expect((sheetBounds?.y ?? 0) + (sheetBounds?.height ?? 0)).toBeLessThanOrEqual(741);
  }).toPass();
  await page.screenshot({ path: testInfo.outputPath("mobile-portfolio-sheet.png") });
  await sheet.getByPlaceholder("搜索投资组合…").fill("unmatched xyz");
  await expect(sheet.getByText("无结果。", { exact: true })).toBeVisible();
  await expect(sheet.getByRole("button", { name: "管理投资组合" })).toBeVisible();
  await sheet.getByRole("button", { name: "管理投资组合" }).click();
  await expect(sheet).not.toBeVisible();
  await expect(page).toHaveURL(`${BASE_URL}/settings/portfolios`);
  await page.goto(`${BASE_URL}/dashboard`, { waitUntil: "domcontentloaded" });
  await expect(filter).toHaveAccessibleName("选择投资组合: Dashboard savings");
  await filter.click();
  await sheet.getByRole("option", { name: "所有账户" }).click();
  await expect(sheet).not.toBeVisible();
  await expect(
    headlineContainer.locator("..").getByText("所有账户", { exact: true }),
  ).toHaveCount(0);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.getByRole("button", { name: "隐藏余额", exact: true }).click();
  await expect(headlineContainer.getByTestId("portfolio-balance")).toContainText("•••");
  await page.getByRole("button", { name: "显示余额", exact: true }).click();
  await filter.click();
  await page.getByRole("option", { name: "Dashboard savings" }).click();

  const portfolios = await page.request.get(`${BASE_URL}/api/v1/portfolios`);
  const saved = ((await portfolios.json()) as { id: string; name: string }[]).find(
    (item) => item.name === "Dashboard savings",
  );
  expect(saved).toBeDefined();
  const card = await page.request.post(`${BASE_URL}/api/v1/accounts`, {
    data: {
      name: "Portfolio filter credit card",
      accountType: "CREDIT_CARD",
      currency: "CAD",
      isDefault: false,
      isActive: true,
      trackingMode: "TRANSACTIONS",
    },
  });
  expect(card.ok()).toBe(true);
  const { id: cardId } = (await card.json()) as { id: string };
  const changed = await page.request.put(`${BASE_URL}/api/v1/portfolios/${saved?.id}`, {
    data: { ...saved, accountIds: [cardId] },
  });
  expect(changed.ok()).toBe(true);
  await page.reload();
  await expect(filter).toHaveAccessibleName("选择投资组合: Dashboard savings");
  // CAD base currency formats as CA$ under zh-CN; USD may still show $.
  await expect(balanceValue).toHaveText(/^(?:CA\$|\$|C\$)0\.00$/, { timeout: 30000 });
  await expect(page.getByText("此投资组合中没有可显示的账户。")).toBeVisible();
  await expect(page.getByRole("link", { name: "管理投资组合" })).toHaveAttribute(
    "href",
    "/settings/portfolios",
  );
  await expect(page.getByRole("link", { name: "添加您的第一个账户" })).toHaveCount(0);
  const deleted = await page.request.delete(`${BASE_URL}/api/v1/portfolios/${saved?.id}`);
  expect(deleted.ok()).toBe(true);
  await page.reload();
  await expect(filter).toHaveAccessibleName("选择投资组合: 所有账户");
  await expectBalanceMatchesAllAccounts(page, balanceValue);
  const longName = "Long-term savings and fixed income — retirement investment portfolio";
  const longPortfolio = await page.request.post(`${BASE_URL}/api/v1/portfolios`, {
    data: { name: longName, accountIds: [accountIds[0]] },
  });
  expect(longPortfolio.ok()).toBe(true);
  await page.reload();
  await filter.click();
  await page.getByRole("option", { name: longName }).click();
  for (const viewport of [
    { width: 1440, height: 900 },
    { width: 320, height: 740 },
    { width: 390, height: 844 },
  ]) {
    await page.setViewportSize(viewport);
    await expect(filter).toHaveAccessibleName(`选择投资组合: ${longName}`);
    await expect(balanceValue).toContainText("1,000,000.00");
    await expect(async () => {
      const layout = await page
        .getByRole("button", { name: "隐藏余额", exact: true })
        .evaluate((element) => {
          const eye = element.getBoundingClientRect();
          const balance = element
            .parentElement!.querySelector('[data-testid="portfolio-balance"]')!
            .getBoundingClientRect();
          return {
            centerDifference: Math.abs(eye.y + eye.height / 2 - balance.y - balance.height / 2),
            gap: eye.x - balance.right,
            overflow: document.documentElement.scrollWidth > innerWidth,
            nestedInAnchor: Boolean(element.closest("a")),
          };
        });
      expect(layout.centerDifference).toBeLessThan(2);
      expect(layout.gap).toBeCloseTo(8, 0);
      expect(layout.overflow).toBe(false);
      expect(layout.nestedInAnchor).toBe(false);
    }).toPass();
    await page.screenshot({ path: testInfo.outputPath(`long-name-${viewport.width}.png`) });
  }
});
