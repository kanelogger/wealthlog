import { expect, test } from "@playwright/test";

for (const width of [320, 1280]) {
  test(`separate monthly drivers fit at ${width}px in zh`, async ({ page }) => {
    await page.setViewportSize({ width, height: 1100 });
    await page.goto(`/e2e/net-worth/?language=zh`);
    const card = page.getByRole("region", { name: "Monthly pace card" });
    for (const label of ["投入", "其他资产", "投资收益"]) {
      await expect(card.getByText(label)).toBeVisible();
    }
    await expect(card).toContainText("自");
    await page.evaluate(() => document.fonts.ready);
    expect(
      await card.evaluate((root) => {
        const errors: string[] = [];
        const bounds = root.getBoundingClientRect();
        const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
        while (walker.nextNode()) {
          const node = walker.currentNode;
          if (!node.textContent?.trim()) continue;
          const range = document.createRange();
          range.selectNodeContents(node);
          for (const rect of range.getClientRects()) {
            if (rect.left < bounds.left || rect.right > bounds.right)
              errors.push(node.textContent);
          }
        }
        return errors;
      }),
    ).toEqual([]);
  });
}
