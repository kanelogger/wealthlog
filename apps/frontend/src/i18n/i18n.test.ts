import { describe, expect, it } from "vitest";

import { createI18n } from "./create-i18n";
import i18n from "./i18n";

describe("Simplified Chinese initialization", () => {
  it("stays on zh when an old language cache is present", () => {
    localStorage.setItem("wealthfolio-language", "zh-Hant");
    const instance = createI18n();

    expect(instance.language).toBe("zh");
    expect(instance.t("common:welcome")).toBe("欢迎");
    expect(i18n.language).toBe("zh");
    expect(document.documentElement.lang).not.toBe("zh-Hant");
  });
});
