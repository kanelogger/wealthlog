import { describe, expect, it } from "vitest";
import { createI18n } from "@/i18n/create-i18n";
import activitySeed from "../../../../../../crates/storage-sqlite/migrations/2026-05-25-000001_spending_module/up.sql?raw";
import seed from "../../../../../../crates/storage-sqlite/migrations/2026-01-01-000002_taxonomies/up.sql?raw";
import { getCategoryLabel } from "@/lib/taxonomy-labels";

const i18n = createI18n();

describe("built-in category labels", () => {
  it.each([
    ["EQUITY_SECURITY", "Stocks", "股票"],
    ["STOCK_PREFERRED", "Preferred Stock", "优先股"],
    ["DEBT_SECURITY", "Bonds", "债券"],
    ["BOND_MUNICIPAL", "Municipal Bond", "市政债券"],
  ])("translates %s without mutating the category", (id, name, label) => {
    const category = Object.freeze({ id, name, taxonomyId: "instrument_type" });
    expect(getCategoryLabel(category, true, i18n.t)).toBe(label);
    expect(category.name).toBe(name);
  });

  it("preserves user categories and unknown IDs", () => {
    const category = { id: "STOCK_PREFERRED", name: "My stocks", taxonomyId: "my_groups" };
    expect(getCategoryLabel(category, false, i18n.t)).toBe(category.name);
    expect(getCategoryLabel({ ...category, taxonomyId: "custom_groups" }, true, i18n.t)).toBe(
      category.name,
    );
    expect(
      getCategoryLabel({ ...category, taxonomyId: "instrument_type", id: "new" }, true, i18n.t),
    ).toBe(category.name);
    expect(getCategoryLabel({ ...category, taxonomyId: "instrument_type" }, true, i18n.t)).toBe(
      category.name,
    );
  });

  it("covers every seeded asset category with a Chinese label", () => {
    const rows = [
      ...(seed + activitySeed).matchAll(
        /\('([^']+)',\s*'(instrument_type|asset_classes|industries_gics|regions|risk_category|spending_categories|income_sources|savings_categories)',\s*(?:NULL|'[^']*'),\s*'((?:[^']|'')*)'/g,
      ),
    ];
    expect(rows.length).toBeGreaterThan(600);
    for (const [, id, taxonomyId, name] of rows) {
      const label = getCategoryLabel({ id, taxonomyId, name }, true, i18n.t);
      expect(label, `${taxonomyId}/${id}`).toMatch(/[\u3400-\u9fff]/);
    }
  });
});
