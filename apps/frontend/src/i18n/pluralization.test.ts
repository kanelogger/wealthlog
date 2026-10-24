import { createI18n } from "./create-i18n";
import { NAMESPACES } from "./locales";
import { resources } from "./resources";
import { describe, expect, it } from "vitest";

const i18n = createI18n();

function collectKeys(node: Record<string, unknown>, prefix: string, out: string[]) {
  for (const [key, value] of Object.entries(node)) {
    const path = prefix ? `${prefix}.${key}` : key;
    if (value !== null && typeof value === "object" && !Array.isArray(value)) {
      collectKeys(value as Record<string, unknown>, path, out);
    } else {
      out.push(path);
    }
  }
}

describe("Simplified Chinese plurals and interpolation", () => {
  it("lets i18next choose the Chinese plural form", () => {
    expect(new Intl.PluralRules("zh").resolvedOptions().pluralCategories).toEqual(["other"]);
    for (const count of [0, 1, 2, 1_000_000]) {
      const message = i18n.t("common:globalEvents.priceUpdateFailed", { count });
      expect(message).toBe(`${count} 项资产的价格更新失败`);
      expect(message).not.toContain("globalEvents");
    }
  });

  it("interpolates profile names through i18next", () => {
    expect(i18n.t("common:profiles.openingProfile", { name: "主账本" })).toBe("正在打开 主账本…");
  });
});

describe("dynamic translation keys", () => {
  const branches = [
    ["holdings", "instrument_types"],
    ["ui", "interval"],
    ["settings", "agentAccess.preset"],
    ["settings", "agentAccess.scope"],
  ] as const;

  it("resolves every key under the dynamic branches", () => {
    const missing: string[] = [];
    for (const [namespace, branch] of branches) {
      const root = resources.zh[namespace] as Record<string, unknown>;
      let node: unknown = root;
      for (const part of branch.split(".")) {
        node = (node as Record<string, unknown>)[part];
      }
      const keys: string[] = [];
      collectKeys(node as Record<string, unknown>, branch, keys);
      for (const key of keys) {
        const resolved = i18n.t(`${namespace}:${key}`);
        if (resolved === `${namespace}:${key}` || String(resolved).trim() === "") {
          missing.push(`${namespace}:${key}`);
        }
      }
    }
    expect(missing).toEqual([]);
    expect(NAMESPACES).toContain("holdings");
  });
});
