import { describe, expect, it } from "vitest";
import i18nextConfig from "../../i18next.config";
import { NAMESPACES, SUPPORTED_LOCALE_CODES, UI_LANGUAGE } from "./locales";
import { resources } from "./resources";

const catalogDirectories = Object.keys(
  import.meta.glob("./locales/*/common.json", { eager: true }),
).map((path) => path.replace("./locales/", "").replace("/common.json", ""));

describe("supported locales", () => {
  it("keeps i18next-cli on the single Simplified Chinese catalog", () => {
    expect([...i18nextConfig.locales]).toEqual([UI_LANGUAGE]);
    expect(SUPPORTED_LOCALE_CODES).toEqual([UI_LANGUAGE]);
  });

  it("ships only the Simplified Chinese catalog", () => {
    expect(catalogDirectories).toEqual([UI_LANGUAGE]);
    expect(Object.keys(resources[UI_LANGUAGE]).sort()).toEqual([...NAMESPACES].sort());
  });
});
