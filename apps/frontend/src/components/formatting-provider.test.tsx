import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {
  AmountDisplay,
  DatePickerInput,
  FormattingProvider,
  useAmountFormatting,
  useDateFormatting,
  useDateFnsLocale,
  useLocalizationSettings,
  useNumberFormatting,
  type SupportedFormattingLocale,
} from "@wealthfolio/ui";
import { type ReactElement, useState } from "react";
import { describe, expect, it } from "vitest";

function LocaleSwitcher() {
  const [locale, setLocale] = useState<SupportedFormattingLocale>("zh");
  return (
    <FormattingProvider locale={locale} timezone="UTC">
      <AmountDisplay value={1234.56} currency="EUR" />
      <button type="button" onClick={() => setLocale("CN")}>
        {locale}
      </button>
    </FormattingProvider>
  );
}

function FormattingConsumer() {
  void {
    ...useLocalizationSettings(),
    ...useAmountFormatting(),
    ...useNumberFormatting(),
    ...useDateFormatting(),
  };
  return null;
}

function AmountFormattingConsumer() {
  useAmountFormatting();
  return null;
}

const observedAmountServices: unknown[] = [];
function AmountServiceConsumer() {
  observedAmountServices.push(useAmountFormatting());
  return null;
}

const observedServices: { amount: unknown; number: unknown; date: unknown }[] = [];
function ServiceIdentityConsumer() {
  observedServices.push({
    amount: useAmountFormatting(),
    number: useNumberFormatting(),
    date: useDateFormatting(),
  });
  return null;
}

function RegionalNumber() {
  return <span>{useNumberFormatting().formatDecimal(1234.56)}</span>;
}

function RegionalCalendar() {
  const locale = useDateFnsLocale();
  return <span>{`${locale.localize.month(0)}:${locale.options?.weekStartsOn}`}</span>;
}

function RegionalFormattedMonth() {
  const formatting = useDateFormatting();
  return <span>{formatting.formatCalendarDate("2026-01-01", { month: "long" })}</span>;
}

function UILocaleConsumer() {
  return <span>{useLocalizationSettings().uiLocale}</span>;
}

describe("FormattingProvider", () => {
  it("keeps China formatting when the supported input changes", async () => {
    const { user } = setupUser(<LocaleSwitcher />);
    expect(screen.getByText("€1,234.56")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "zh" }));

    expect(screen.getByText("€1,234.56")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "CN" })).toBeInTheDocument();
  });

  it("preserves finance privacy masking", () => {
    render(
      <FormattingProvider locale="zh-CN">
        <AmountDisplay value={1234.56} currency="USD" isHidden />
      </FormattingProvider>,
    );
    expect(screen.getByText("••••")).toBeInTheDocument();
  });

  it("uses China formatting for numbers and calendar date content", () => {
    render(
      <FormattingProvider locale="CN" uiLocale="zh">
        <RegionalNumber />
        <RegionalCalendar />
        <RegionalFormattedMonth />
      </FormattingProvider>,
    );
    expect(screen.getByText("1,234.56")).toBeInTheDocument();
    expect(screen.getByText("一月:1")).toBeInTheDocument();
    expect(screen.getByText("一月")).toBeInTheDocument();
  });

  it("keeps the interface language on zh", () => {
    render(
      <FormattingProvider locale="zh-CN" uiLocale="zh">
        <UILocaleConsumer />
        <RegionalNumber />
        <DatePickerInput value="2026-08-18" onChange={() => undefined} />
      </FormattingProvider>,
    );

    expect(screen.getByText("zh")).toBeInTheDocument();
    expect(screen.getByText("1,234.56")).toBeInTheDocument();
    expect(screen.getAllByRole("spinbutton").map((segment) => segment.dataset.type)).toEqual([
      "year",
      "month",
      "day",
    ]);
  });

  it("reuses the provider-owned finance service across consumers", () => {
    observedAmountServices.length = 0;
    render(
      <FormattingProvider locale="CN" uiLocale="zh">
        <AmountServiceConsumer />
        <AmountServiceConsumer />
      </FormattingProvider>,
    );
    expect(observedAmountServices).toHaveLength(2);
    expect(observedAmountServices[0]).toBe(observedAmountServices[1]);
  });

  it("keeps amount and number services stable across timezone-only updates", () => {
    observedServices.length = 0;
    const { rerender } = render(
      <FormattingProvider locale="CN" uiLocale="zh" timezone="UTC">
        <ServiceIdentityConsumer />
      </FormattingProvider>,
    );
    const initial = observedServices.at(-1)!;

    rerender(
      <FormattingProvider locale="CN" uiLocale="zh" timezone="America/Toronto">
        <ServiceIdentityConsumer />
      </FormattingProvider>,
    );
    const updated = observedServices.at(-1)!;

    expect(updated.amount).toBe(initial.amount);
    expect(updated.number).toBe(initial.number);
    expect(updated.date).not.toBe(initial.date);
  });

  it("keeps public components and hooks usable without a provider", () => {
    render(
      <>
        <FormattingConsumer />
        <AmountFormattingConsumer />
        <AmountDisplay value={1234.56} currency="USD" />
      </>,
    );
    expect(screen.getByText("US$1,234.56")).toBeInTheDocument();
  });
});

function setupUser(element: ReactElement) {
  return {
    ...render(element),
    user: userEvent.setup(),
  };
}
