import { expect, test } from "@playwright/test";

for (const width of [320, 1280]) {
  test(`attention actions fit and link an existing property at ${width}px in zh`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 1100 });
    await page.goto(`/e2e/net-worth/?attention=1&language=zh`);
    const card = page.getByRole("region", { name: "需要关注" });
    await card.scrollIntoViewIfNeeded();
    await expect(card.getByRole("link")).toHaveCount(2);
    const linkText = "为Home Mortgage关联房产";
    if (width === 320) {
      await card.getByRole("button", { name: linkText, exact: true }).click();
      await page.getByRole("dialog").getByRole("button", { name: "Home", exact: true }).click();
    } else {
      await card.getByRole("combobox").click();
      await page.getByRole("option", { name: "Home", exact: true }).click();
    }
    await expect(card.getByRole("button", { name: /添加房产/ })).toHaveCount(0);
    await expect(card.getByRole("link")).toHaveCount(2);

    // Reset the fixture to inspect the creation action as well.
    await page.reload();
    await card.scrollIntoViewIfNeeded();
    await page.evaluate(() => document.fonts.ready);
    expect(
      await card.evaluate((root) => {
        const bounds = root.getBoundingClientRect();
        const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
        const overflow: string[] = [];
        while (walker.nextNode()) {
          const node = walker.currentNode;
          if (!node.textContent?.trim()) continue;
          // Select triggers deliberately truncate long localized labels.
          if (node.parentElement?.closest('[role="combobox"], .truncate')) continue;
          const range = document.createRange();
          range.selectNodeContents(node);
          for (const rect of range.getClientRects()) {
            if (rect.width && (rect.left < bounds.left - 1 || rect.right > bounds.right + 1))
              overflow.push(node.textContent ?? "");
          }
        }
        return overflow;
      }),
    ).toEqual([]);
    await card.screenshot({ path: test.info().outputPath("attention.png") });
    const addText = "为Home Mortgage添加房产";
    if (width === 320) {
      await card.getByRole("button", { name: linkText, exact: true }).click();
      await page.getByRole("dialog").getByRole("button", { name: addText, exact: true }).click();
    } else {
      await card.getByRole("combobox").click();
      await page.getByRole("option", { name: addText, exact: true }).click();
    }
    await expect(page.getByText("Property creation requested")).toBeVisible();
  });
}

test("offers creation when no property exists", async ({ page }) => {
  await page.goto("/e2e/net-worth/?attention=1&property=0&language=zh");
  const card = page.getByRole("region", { name: "需要关注" });
  await expect(card.getByRole("combobox")).toHaveCount(0);
  await expect(
    card.getByRole("button", { name: "为Home Mortgage添加房产" }),
  ).toBeVisible();
});

test("offers a property for an untyped legacy liability with no recorded property", async ({
  page,
}) => {
  await page.goto("/e2e/net-worth/?attention=1&property=0&untyped=1&language=zh");
  const card = page.getByRole("region", { name: "需要关注" });
  await expect(card.getByText("关联现有房产不会改变您的净资产。")).toHaveCount(0);
  await card.getByRole("button", { name: "为Home Mortgage添加房产" }).click();
  await expect(page.getByText("Property creation requested")).toBeVisible();
});
