import { expect, type Page, test } from "@playwright/test";
import {
  BASE_URL,
  completeOnboardingIfNeeded,
  createAccount,
  fillDateField,
  gotoActivities,
  loginIfNeeded,
  openAddActivitySheet,
  selectAccountOption,
  selectActivityType,
  waitForSyncToast,
} from "./helpers";

interface AccountPayload {
  id: string;
  name: string;
  currency: string;
}

interface AssetPayload {
  id: string;
  displayCode?: string | null;
  instrumentSymbol?: string | null;
  instrumentType?: string | null;
}

interface HoldingPayload {
  quantity: number | string;
  costBasis?: { local?: number | string | null } | null;
  realizedGain?: { local?: number | string | null } | null;
  lots?: HoldingLotPayload[] | null;
}

interface HoldingLotPayload {
  quantity: number | string;
  costBasis: number | string;
  acquisitionPrice: number | string;
}

interface AssetLotPayload {
  accountId: string;
  quantity: number | string;
  remainingQuantity: number | string;
  costBasis: number | string;
  unitCost: number | string;
  isClosed: boolean;
  disposalProceeds?: number | string | null;
  disposalCostBasis?: number | string | null;
  realizedPnl?: number | string | null;
}

interface HoldingSummary {
  quantity: number;
  costBasis: number;
  realizedGain: number | null;
  lots: Array<{
    quantity: number;
    costBasis: number;
    acquisitionPrice: number;
  }>;
}

interface AssetLotSummary {
  quantity: number;
  remainingQuantity: number;
  costBasis: number;
  unitCost: number;
  isClosed: boolean;
  disposalProceeds: number | null;
  disposalCostBasis: number | null;
  realizedPnl: number | null;
}

interface ShortStateSummary {
  holding: HoldingSummary | null;
  assetLots: AssetLotSummary[];
}

const STOCK_SHORT_ACCOUNT = "E2E Stock Short Account";
const STOCK_SHORT_SYMBOL = "AAPL";
const ACCOUNT_CURRENCY = "USD";

test.describe("Stock short workflow", () => {
  test("records sell short and buy to cover with holdings and lots after each action", async ({
    page,
  }) => {
    // Multi-trade workflow with a market-sync wait after each submit — needs more
    // than the default 30s test budget.
    test.setTimeout(180000);
    await completeOnboardingIfNeeded(page);
    await loginIfNeeded(page);
    await createAccount(page, STOCK_SHORT_ACCOUNT, ACCOUNT_CURRENCY);
    const account = await getAccountByName(page, STOCK_SHORT_ACCOUNT);

    await gotoActivities(page);
    await assertBuyFormDoesNotOfferBuyShort(page);

    await submitStockTrade(page, {
      activityType: "卖出",
      intent: "卖空",
      quantity: 10,
      price: 100,
      daysAgo: 9,
      submitLabel: "卖空",
    });
    const assetId = await getAssetIdForSymbol(page, STOCK_SHORT_SYMBOL);
    await expectShortState(page, account.id, assetId, {
      holding: {
        quantity: -10,
        costBasis: -1000,
        realizedGain: null,
        lots: [{ quantity: -10, costBasis: -1000, acquisitionPrice: 100 }],
      },
      assetLots: [
        {
          quantity: -10,
          remainingQuantity: -10,
          costBasis: -1000,
          unitCost: 100,
          isClosed: false,
          disposalProceeds: null,
          disposalCostBasis: null,
          realizedPnl: null,
        },
      ],
    });

    await gotoActivities(page);
    await submitStockTrade(page, {
      activityType: "Buy",
      intent: "买入回补",
      quantity: 4,
      price: 90,
      daysAgo: 8,
      submitLabel: "买入回补",
    });
    await expectShortState(page, account.id, assetId, {
      holding: {
        quantity: -6,
        costBasis: -600,
        realizedGain: 40,
        lots: [{ quantity: -6, costBasis: -600, acquisitionPrice: 100 }],
      },
      assetLots: [
        {
          quantity: -6,
          remainingQuantity: -6,
          costBasis: -600,
          unitCost: 100,
          isClosed: false,
          disposalProceeds: -360,
          disposalCostBasis: -400,
          realizedPnl: 40,
        },
      ],
    });

    await gotoActivities(page);
    await submitStockTrade(page, {
      activityType: "Buy",
      intent: "买入回补",
      quantity: 6,
      price: 90,
      daysAgo: 7,
      submitLabel: "买入回补",
    });
    await expectShortState(page, account.id, assetId, {
      holding: null,
      assetLots: [
        {
          quantity: 0,
          remainingQuantity: 0,
          costBasis: 0,
          unitCost: 100,
          isClosed: true,
          disposalProceeds: -900,
          disposalCostBasis: -1000,
          realizedPnl: 100,
        },
      ],
    });
  });
});

async function assertBuyFormDoesNotOfferBuyShort(page: Page) {
  await openStockActivityForm(page, "Buy");
  const dialog = activityDialog(page);

  await expect(dialog.getByRole("button", { name: "添加买入", exact: true })).toBeVisible();
  // Buy form must not offer short-sell intent (English ghost or Chinese 卖空).
  await expect(dialog.getByRole("button", { name: "Buy Short", exact: true })).toBeHidden();
  await expect(dialog.getByRole("button", { name: "卖空", exact: true })).toBeHidden();
  await expect(dialog.getByRole("button", { name: "买入回补", exact: true })).toBeHidden();

  await dialog.getByRole("button", { name: "取消", exact: true }).click();
  await expect(page.getByRole("heading", { name: "添加活动" })).not.toBeVisible({
    timeout: 10000,
  });
}

async function submitStockTrade(
  page: Page,
  trade: {
    activityType: "Buy" | "卖出";
    intent: "卖空" | "买入回补";
    quantity: number;
    price: number;
    daysAgo: number;
    submitLabel: "卖空" | "买入回补";
  },
) {
  await openStockActivityForm(page, trade.activityType);
  const dialog = activityDialog(page);

  await fillDateField(page, trade.daysAgo);

  const tradeTypeGroup = dialog.getByRole("group", { name: "交易类型" });
  await expect(tradeTypeGroup).toBeVisible();
  if (trade.activityType === "Buy") {
    await expect(dialog.getByRole("button", { name: "卖空", exact: true })).toBeHidden();
  }
  await tradeTypeGroup.getByRole("button", { name: trade.intent, exact: true }).click();

  await dialog.getByTestId("quantity-input").fill(String(trade.quantity));
  await dialog.getByTestId("price-input").fill(String(trade.price));
  await dialog.getByTestId("fee-input").fill("0");
  // Notes live inside the collapsible "高级与备注" section — expand it first.
  const notesInput = dialog.getByTestId("notes-input");
  if (!(await notesInput.isVisible().catch(() => false))) {
    await dialog.getByTestId("advanced-options-button").click();
    await expect(notesInput).toBeVisible({ timeout: 5000 });
  }
  await notesInput.fill(`${trade.intent} ${trade.quantity} ${STOCK_SHORT_SYMBOL}`);

  const submitButton = dialog.locator('button[type="submit"]');
  await expect(submitButton).toContainText(trade.submitLabel);
  await submitButton.click();
  await expect(page.getByRole("heading", { name: "添加活动" })).not.toBeVisible({
    timeout: 10000,
  });
  await waitForSyncToast(page, 30000);
}

async function openStockActivityForm(page: Page, activityType: "Buy" | "卖出") {
  await openAddActivitySheet(page);
  await selectActivityType(page, activityType);

  const dialog = activityDialog(page);
  await selectAccount(dialog, page, STOCK_SHORT_ACCOUNT, ACCOUNT_CURRENCY);
  await searchAndSelectSymbol(dialog, page, STOCK_SHORT_SYMBOL);
}

async function selectAccount(
  dialog: ReturnType<typeof activityDialog>,
  page: Page,
  accountName: string,
  currency: string,
) {
  await selectAccountOption(page, accountName, currency, dialog.getByTestId("account-select"));
}

async function searchAndSelectSymbol(
  dialog: ReturnType<typeof activityDialog>,
  page: Page,
  symbol: string,
) {
  const escapedSymbol = escapeRegExp(symbol);
  const exactSymbolPattern = new RegExp(`^${escapedSymbol}$`, "i");
  const symbolCombobox = dialog.getByRole("combobox").filter({ hasText: /选择代码/ });
  await symbolCombobox.click();

  const searchInput = page.getByPlaceholder("搜索代码");
  await expect(searchInput).toBeVisible({ timeout: 5000 });
  await searchInput.fill(symbol);

  const suggestions = page.getByRole("listbox", { name: /Suggestions/i });
  await expect(suggestions).toBeVisible({ timeout: 10000 });
  const symbolOption = suggestions
    .getByRole("option")
    .filter({
      has: page.locator("span.font-mono").filter({ hasText: exactSymbolPattern }),
      hasNotText: /创建自定义\(手动\)/i,
    })
    .first();
  await expect(symbolOption).toBeVisible({ timeout: 30000 });
  await symbolOption.click();
}

async function getAccountByName(page: Page, accountName: string): Promise<AccountPayload> {
  const accounts = await apiGet<AccountPayload[]>(page, "/accounts");
  const account = accounts.find((candidate) => candidate.name === accountName);
  expect(account, `Expected account ${accountName} to exist`).toBeTruthy();
  return account!;
}

async function getAssetIdForSymbol(page: Page, symbol: string): Promise<string> {
  const assets = await apiGet<AssetPayload[]>(page, "/assets");
  const asset = assets.find(
    (candidate) =>
      candidate.displayCode === symbol ||
      (candidate.instrumentSymbol === symbol && candidate.instrumentType === "EQUITY"),
  );
  expect(asset, `Expected ${symbol} asset to exist`).toBeTruthy();
  return asset!.id;
}

async function expectShortState(
  page: Page,
  accountId: string,
  assetId: string,
  expected: ShortStateSummary,
) {
  await expect
    .poll(() => getShortState(page, accountId, assetId), {
      timeout: 20000,
      intervals: [500, 1000, 2000],
    })
    .toEqual(expected);
}

async function getShortState(
  page: Page,
  accountId: string,
  assetId: string,
): Promise<ShortStateSummary> {
  const holding = await apiGet<HoldingPayload | null>(
    page,
    `/holdings/item?accountId=${encodeURIComponent(accountId)}&assetId=${encodeURIComponent(assetId)}`,
  );
  const assetLots = await apiGet<AssetLotPayload[]>(
    page,
    `/holdings/lots?assetId=${encodeURIComponent(assetId)}&includeSnapshotPositions=false`,
  );

  return {
    holding: summarizeHolding(holding),
    assetLots: assetLots.filter((lot) => lot.accountId === accountId).map(summarizeAssetLot),
  };
}

function summarizeHolding(holding: HoldingPayload | null): HoldingSummary | null {
  if (!holding) return null;

  return {
    quantity: toRoundedNumber(holding.quantity),
    costBasis: toRoundedNumber(holding.costBasis?.local),
    realizedGain: toRoundedNullableNumber(holding.realizedGain?.local),
    lots: (holding.lots ?? []).map((lot) => ({
      quantity: toRoundedNumber(lot.quantity),
      costBasis: toRoundedNumber(lot.costBasis),
      acquisitionPrice: toRoundedNumber(lot.acquisitionPrice),
    })),
  };
}

function summarizeAssetLot(lot: AssetLotPayload): AssetLotSummary {
  return {
    quantity: toRoundedNumber(lot.quantity),
    remainingQuantity: toRoundedNumber(lot.remainingQuantity),
    costBasis: toRoundedNumber(lot.costBasis),
    unitCost: toRoundedNumber(lot.unitCost),
    isClosed: lot.isClosed,
    disposalProceeds: toRoundedNullableNumber(lot.disposalProceeds),
    disposalCostBasis: toRoundedNullableNumber(lot.disposalCostBasis),
    realizedPnl: toRoundedNullableNumber(lot.realizedPnl),
  };
}

async function apiGet<T>(page: Page, path: string): Promise<T> {
  const response = await page.request.get(`${BASE_URL}/api/v1${path}`);
  expect(response.ok(), `GET ${path} failed with ${response.status()}`).toBeTruthy();
  return response.json() as Promise<T>;
}

function activityDialog(page: Page) {
  return page
    .getByRole("dialog")
    .filter({ has: page.getByRole("heading", { name: "添加活动" }) });
}

function toRoundedNumber(value: number | string | null | undefined): number {
  return Number(Number(value ?? 0).toFixed(4));
}

function toRoundedNullableNumber(value: number | string | null | undefined): number | null {
  if (value === null || value === undefined) return null;
  return toRoundedNumber(value);
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
