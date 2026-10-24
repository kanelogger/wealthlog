import {
  createFormatter,
  dateFnsLocaleFor,
  parseDateTimeInTimezone,
  parseLocalizedDecimalString,
  parseLocalizedDate,
  parseLocalizedNumber,
  resolveFormattingLocale,
} from "@wealthfolio/ui";
import { zhCN } from "date-fns/locale";
import { describe, expect, it } from "vitest";

const formatting = createFormatter("zh-CN", "UTC");

describe("China formatting locale", () => {
  it("accepts only the supported inputs and resolves them to zh-CN", () => {
    expect(resolveFormattingLocale("CN")).toBe("zh-CN");
    expect(resolveFormattingLocale("zh")).toBe("zh-CN");
    expect(resolveFormattingLocale("zh-CN", "zh")).toBe("zh-CN");
    expect(formatting.locale).toBe("zh-CN");
  });

  it("rejects missing, system, and other region inputs", () => {
    expect(() => resolveFormattingLocale(undefined)).toThrow("A formatting locale is required");
    expect(() => resolveFormattingLocale(null)).toThrow("A formatting locale is required");
    for (const setting of ["system", "en-US", "US", "DE", "zh-Hant", "zh-TW", "not_a_locale"]) {
      expect(() => resolveFormattingLocale(setting)).toThrow("Invalid formatting locale");
    }
    expect(() => dateFnsLocaleFor("en-US" as "zh-CN")).toThrow("Invalid formatting locale");
  });

  it("uses Simplified Chinese calendar text and a Monday week start", () => {
    const locale = dateFnsLocaleFor("CN");
    expect(locale).toBe(zhCN);
    expect(dateFnsLocaleFor("zh")).toBe(zhCN);
    expect(locale.localize.month(0)).toBe("一月");
    expect(locale.options?.weekStartsOn).toBe(1);
  });
});

describe("China number formatting", () => {
  it("formats amounts, prices, compact values, percents, and quantities", () => {
    expect(formatting.formatDecimal(1234.56)).toBe("1,234.56");
    expect(formatting.formatAmount(-1234.56, "USD")).toBe("-US$1,234.56");
    expect(formatting.formatAmount(12.34, "GBP")).toBe("£12.34");
    expect(formatting.formatAmount(1234.56, "EUR")).toBe("€1,234.56");
    expect(formatting.formatPrice(1.4018, "CNY")).toBe("¥1.4018");
    expect(formatting.formatPrice(12, "USD")).toBe("US$12.00");
    expect(formatting.formatPrice(268.3999939, "USD")).toBe("US$268.40");
    expect(formatting.formatPrice(0.00012345, "USD", false)).toBe("0.00012345");
    expect(formatting.formatPrice(-0.000000001, "USD", false)).toBe("0.00");
    expect(formatting.formatCompactAmount(1_250_000, "USD")).toBe("US$125万");
    expect(formatting.formatCompactAmount(47_000, "USD", true, "narrowSymbol")).toBe("$4.7万");
    expect(formatting.formatPercent(-0.125)).toBe("-12.50%");
    expect(formatting.formatPercent(0, { signDisplay: "exceptZero" })).toBe("0.00%");
    expect(formatting.formatPercent(0.125, { digits: 1 })).toBe("12.5%");
    expect(formatting.formatQuantity(1234.56789)).toBe("1,234.56789");
    expect(formatting.formatRoundedAmount(1234.56, "GBp")).toBe("1,235p");
  });

  it("keeps quote units and currency minor units", () => {
    expect(formatting.formatAmount(12.34, "GBp")).toBe("12.34p");
    expect(formatting.formatAmount(12.34, "ILA")).toBe("12.34ag");
    expect(formatting.formatPrice(1.4018, "GBp")).toBe("1.4018p");
    expect(formatting.formatAmount(1234.4, "JPY")).toBe("JP¥1,234");
    expect(formatting.formatAmount(1234.4, "JPY", false)).toBe("1,234");
    expect(formatting.formatAmount(1.2345, "KWD", false)).toBe("1.235");
    expect(formatting.formatAmount(-0.4, "JPY")).not.toContain("-");
  });

  it.each(["USD", "EUR", "CNY", "JPY", "GBP", "GBp"] as const)(
    "round-trips its own %s currency output",
    (currency) => {
      expect(formatting.parseNumber(formatting.formatAmount(1234, currency))).toBe(1234);
    },
  );
});

describe("China amount parsing", () => {
  it("accepts yuan prefixes, signs, grouping, and full-width digits", () => {
    expect(parseLocalizedNumber("¥1,234.56", "zh-CN")).toBe(1234.56);
    expect(parseLocalizedNumber("￥1,234.56", "zh")).toBe(1234.56);
    expect(parseLocalizedNumber("CN¥1,234.56", "CN")).toBe(1234.56);
    expect(parseLocalizedNumber("CNY 1,234.56", "zh-CN")).toBe(1234.56);
    expect(parseLocalizedNumber("-¥1,234.56", "zh-CN")).toBe(-1234.56);
    expect(parseLocalizedNumber("−CN¥1234.50", "zh-CN")).toBe(-1234.5);
    expect(parseLocalizedNumber("１，２３４．５６", "zh-CN")).toBe(1234.56);
    expect(formatting.parseNumber("１，２３４．５６")).toBe(1234.56);
    expect(parseLocalizedDecimalString("123456789012345678.12345678", "zh-CN")).toBe(
      "123456789012345678.12345678",
    );
    expect(parseLocalizedNumber(".5", "zh-CN")).toBe(0.5);
    expect(parseLocalizedNumber("1.", "zh-CN")).toBe(1);
  });

  it("rejects illegal separators and non-numeric text", () => {
    expect(parseLocalizedNumber("1,234.5.6", "zh-CN")).toBeUndefined();
    expect(parseLocalizedNumber("1,23", "zh-CN")).toBeUndefined();
    expect(parseLocalizedNumber("1.234,56", "zh-CN")).toBeUndefined();
    expect(parseLocalizedNumber("abc123", "zh-CN")).toBeUndefined();
    expect(parseLocalizedDecimalString("1,23", "zh-CN")).toBeUndefined();
  });
});

describe("China dates", () => {
  it("keeps a calendar date stable across timezones", () => {
    const honolulu = createFormatter("zh-CN", "Pacific/Honolulu");
    expect(
      honolulu.formatCalendarDate("2026-07-10", {
        year: "numeric",
        month: "2-digit",
        day: "2-digit",
      }),
    ).toBe("2026/07/10");
    expect(formatting.formatCalendarDate("2026-01-01", { month: "long" })).toBe("一月");
    expect(
      formatting.formatCalendarDateRange("2026-07-01", "2026-07-03", {
        month: "short",
        day: "numeric",
      }),
    ).toBe(
      new Intl.DateTimeFormat("zh-CN", {
        month: "short",
        day: "numeric",
        timeZone: "UTC",
      }).formatRange(new Date(Date.UTC(2026, 6, 1)), new Date(Date.UTC(2026, 6, 3))),
    );
  });

  it("parses ISO and Chinese calendar text without shifting the day", () => {
    expect(parseLocalizedDate("2026-07-10", "zh-CN")?.getDate()).toBe(10);
    expect(parseLocalizedDate("2026/08/18", "zh-CN")).toEqual(new Date(2026, 7, 18));
    expect(parseLocalizedDate("２０２６/８/１８", "zh-CN")).toEqual(new Date(2026, 7, 18));
    expect(parseLocalizedDate("2026-07-10T00:00:00.000Z", "zh-CN")?.toISOString()).toBe(
      "2026-07-10T00:00:00.000Z",
    );
    const rendered = formatting.formatCalendarDate("2023-01-03");
    expect(rendered).toBe("2023年1月3日");
    expect(formatting.parseDate(rendered)?.getMonth()).toBe(0);
    expect(formatting.parseDate(rendered)?.getDate()).toBe(3);
  });

  it("converts timestamps by timezone and keeps number formatting independent", () => {
    const losAngeles = createFormatter("zh-CN", "America/Los_Angeles");
    const tokyo = createFormatter("zh-CN", "Asia/Tokyo");
    const options = {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      hourCycle: "h23",
    } as const;
    expect(losAngeles.formatDateTime("2026-07-10T01:30:00.000Z", options)).toBe("2026/07/09 18:30");
    expect(tokyo.formatDateTime("2026-07-10T01:30:00.000Z", options)).toBe("2026/07/10 10:30");
    expect(losAngeles.formatDecimal(1234.5)).toBe("1,234.5");
    expect(
      losAngeles.formatDate("2026-07-10T00:00:00.000Z", {
        year: "numeric",
        month: "short",
        day: "numeric",
        timeZone: "UTC",
      }),
    ).toBe("2026年7月10日");
  });

  it("interprets wall-clock datetimes in the configured timezone", () => {
    expect(parseDateTimeInTimezone("2026-08-18 09:00", "Asia/Tokyo")?.toISOString()).toBe(
      "2026-08-18T00:00:00.000Z",
    );
    expect(parseDateTimeInTimezone("2026-08-18T09:00:00-04:00", "Asia/Tokyo")?.toISOString()).toBe(
      "2026-08-18T13:00:00.000Z",
    );
  });
});
