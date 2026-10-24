import { useSettingsContext } from "@/lib/settings-provider";
import { zodResolver } from "@hookform/resolvers/zod";
import { Icons } from "@wealthfolio/ui";
import { Card, CardContent } from "@wealthfolio/ui/components/ui/card";
import {
  Form,
  FormControl,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from "@wealthfolio/ui/components/ui/form";
import { Input } from "@wealthfolio/ui/components/ui/input";
import { worldCurrencies } from "@wealthfolio/ui/lib/currencies";
import type { TFunction } from "i18next";
import { forwardRef, useEffect, useImperativeHandle, useMemo, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import * as z from "zod";

function createOnboardingSettingsSchema(t: TFunction) {
  return z.object({
    baseCurrency: z
      .string({ required_error: t("onboarding:steps.preferences.currencyRequired") })
      .min(1, t("onboarding:steps.preferences.currencyRequired")),
    timezone: z
      .string({ required_error: t("onboarding:steps.preferences.timezoneRequired") })
      .min(1, t("onboarding:steps.preferences.timezoneRequired")),
  });
}

type OnboardingSettingsSchema = ReturnType<typeof createOnboardingSettingsSchema>;

const NEW_LEDGER_CURRENCY = "CNY";
const popularCurrencies = [NEW_LEDGER_CURRENCY, "USD", "CAD", "EUR"];

const TIMEZONE_FALLBACKS = [
  "UTC",
  "America/New_York",
  "America/Chicago",
  "America/Denver",
  "America/Los_Angeles",
  "America/Toronto",
  "Europe/London",
  "Europe/Paris",
  "Europe/Berlin",
  "Asia/Tokyo",
  "Asia/Singapore",
  "Australia/Sydney",
];

function getSupportedTimezones(): string[] {
  const supportedValuesOf = (
    Intl as unknown as { supportedValuesOf?: (key: "timeZone") => string[] }
  ).supportedValuesOf;
  const rawValues: string[] =
    typeof supportedValuesOf === "function" ? supportedValuesOf("timeZone") : TIMEZONE_FALLBACKS;
  const merged = rawValues.includes("UTC") ? rawValues : ["UTC", ...rawValues];
  return Array.from(new Set(merged)).sort((a, b) => a.localeCompare(b));
}

function detectBrowserTimezone(): string {
  try {
    const detected = Intl.DateTimeFormat().resolvedOptions().timeZone?.trim();
    if (detected) {
      new Intl.DateTimeFormat("en-US", { timeZone: detected }).format(new Date());
      return detected;
    }
  } catch {
    // ignore
  }
  return "UTC";
}

function formatTimezoneLabel(tz: string): string {
  const parts = tz.split("/");
  return parts[parts.length - 1].replace(/_/g, " ");
}

const popularTimezones = [
  "America/New_York",
  "America/Chicago",
  "America/Los_Angeles",
  "Europe/London",
];

type OnboardingSettingsValues = z.infer<OnboardingSettingsSchema>;

export interface OnboardingStep2Handle {
  submitForm: () => void;
}

interface OnboardingStep2Props {
  onNext: () => void;
  onValidityChange: (isValid: boolean) => void;
}

export const OnboardingStep2 = forwardRef<OnboardingStep2Handle, OnboardingStep2Props>(
  ({ onNext, onValidityChange }, ref) => {
    const { t } = useTranslation();
    const { settings, updateSettings } = useSettingsContext();
    const onboardingSettingsSchema = useMemo(() => createOnboardingSettingsSchema(t), [t]);
    const [initialValuesSet, setInitialValuesSet] = useState(false);
    const [showCurrencySearch, setShowCurrencySearch] = useState(false);
    const [currencySearch, setCurrencySearch] = useState("");
    const [showTimezoneSearch, setShowTimezoneSearch] = useState(false);
    const [timezoneSearch, setTimezoneSearch] = useState("");

    const form = useForm<OnboardingSettingsValues>({
      resolver: zodResolver(onboardingSettingsSchema),
    });

    const filteredCurrencies = worldCurrencies.filter(
      (curr) =>
        curr.value.toLowerCase().includes(currencySearch.toLowerCase()) ||
        curr.label.toLowerCase().includes(currencySearch.toLowerCase()),
    );

    const currentCurrency = form.watch("baseCurrency");

    const currencyOptions =
      popularCurrencies.includes(currentCurrency) || !currentCurrency
        ? popularCurrencies
        : [...popularCurrencies.slice(0, -1), currentCurrency];

    function handleCurrencySelect(currencyCode: string) {
      form.setValue("baseCurrency", currencyCode, { shouldValidate: true, shouldDirty: true });
      setShowCurrencySearch(false);
      setCurrencySearch("");
    }

    const allTimezones = useMemo(() => getSupportedTimezones(), []);
    const detectedTimezone = useMemo(() => detectBrowserTimezone(), []);
    const currentTimezone = form.watch("timezone");
    const filteredTimezones = allTimezones.filter((tz) =>
      tz.toLowerCase().includes(timezoneSearch.toLowerCase()),
    );

    const timezoneOptions = useMemo(() => {
      const base = popularTimezones.includes(detectedTimezone)
        ? [detectedTimezone, ...popularTimezones.filter((tz) => tz !== detectedTimezone)]
        : [detectedTimezone, ...popularTimezones.slice(0, -1)];
      if (
        currentTimezone &&
        currentTimezone !== detectedTimezone &&
        !base.includes(currentTimezone)
      ) {
        return [...base.slice(0, -1), currentTimezone];
      }
      return base;
    }, [detectedTimezone, currentTimezone]);

    function handleTimezoneSelect(timezone: string) {
      form.setValue("timezone", timezone, { shouldValidate: true, shouldDirty: true });
      setShowTimezoneSearch(false);
      setTimezoneSearch("");
    }

    useEffect(() => {
      onValidityChange(form.formState.isValid);
    }, [form.formState.isValid, onValidityChange]);

    useImperativeHandle(ref, () => ({
      submitForm() {
        form.handleSubmit(onSubmit)();
      },
    }));

    useEffect(() => {
      if (!initialValuesSet) {
        const defaultCurrency = settings?.baseCurrency || NEW_LEDGER_CURRENCY;
        const defaultTimezone = settings?.timezone || detectBrowserTimezone();
        form.reset(
          { baseCurrency: defaultCurrency, timezone: defaultTimezone },
          { keepDefaultValues: false },
        );
        void form.trigger();
        setInitialValuesSet(true);
      }
    }, [form, settings, initialValuesSet]);

    async function onSubmit(data: OnboardingSettingsValues) {
      try {
        await updateSettings({
          baseCurrency: data.baseCurrency,
          timezone: data.timezone,
        });
        onNext();
      } catch (error) {
        console.error("Failed to save onboarding settings:", error);
      }
    }

    return (
      <>
        <div className="w-full max-w-4xl space-y-4">
          <div className="text-center">
            <p className="text-muted-foreground">{t("onboarding:steps.preferences.subtitle")}</p>
          </div>
          <Card className="border-none bg-transparent shadow-none">
            <CardContent className="p-0 sm:p-4">
              <Form {...form}>
                <form
                  onSubmit={form.handleSubmit(onSubmit)}
                  className="grid gap-3 sm:gap-4 md:grid-cols-2"
                >
                  <FormField
                    control={form.control}
                    name="baseCurrency"
                    render={({ field }) => (
                      <FormItem className="border-border/70 bg-muted/10 min-w-0 rounded-xl border p-4">
                        <div className="mb-3 flex items-center gap-2.5">
                          <div className="bg-muted rounded-md p-1.5">
                            <Icons.DollarSign className="text-muted-foreground size-4" />
                          </div>
                          <FormLabel className="text-base font-semibold">
                            {t("onboarding:steps.preferences.currencyLabel")}
                          </FormLabel>
                        </div>
                        <FormControl>
                          <div className="flex flex-wrap gap-2">
                            {currencyOptions.map((curr) => (
                              <button
                                key={curr}
                                type="button"
                                data-testid={`currency-${curr.toLowerCase()}-button`}
                                onClick={() => field.onChange(curr)}
                                className={`inline-flex min-h-10 items-center rounded-md border px-3 py-2 text-sm font-medium transition-colors ${
                                  field.value === curr
                                    ? "border-primary bg-primary/10 text-primary"
                                    : "border-border bg-background hover:border-primary/50 hover:bg-accent"
                                }`}
                              >
                                {curr}
                              </button>
                            ))}
                            <button
                              type="button"
                              onClick={() => setShowCurrencySearch(true)}
                              className="border-border bg-background hover:border-primary/50 hover:bg-accent ring-offset-background focus-visible:ring-ring inline-flex min-h-10 cursor-pointer items-center justify-center gap-1.5 whitespace-nowrap rounded-md border px-3 py-2 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-2"
                            >
                              <Icons.Search className="size-4 shrink-0" />
                              {t("onboarding:steps.preferences.other")}
                            </button>
                          </div>
                        </FormControl>
                        <FormMessage />
                      </FormItem>
                    )}
                  />

                  <FormField
                    control={form.control}
                    name="timezone"
                    render={({ field }) => (
                      <FormItem className="border-border/70 bg-muted/10 min-w-0 rounded-xl border p-4">
                        <div className="mb-3 flex items-center gap-2.5">
                          <div className="bg-muted rounded-md p-1.5">
                            <Icons.Clock className="text-muted-foreground size-4" />
                          </div>
                          <FormLabel className="text-base font-semibold">
                            {t("onboarding:steps.preferences.timezoneLabel")}
                          </FormLabel>
                        </div>
                        <FormControl>
                          <div className="flex flex-wrap gap-2">
                            {timezoneOptions.map((tz) => (
                              <button
                                key={tz}
                                type="button"
                                data-testid={`timezone-${tz.toLowerCase().replace(/\//g, "-")}-button`}
                                onClick={() => field.onChange(tz)}
                                className={`inline-flex min-h-10 max-w-full items-center rounded-md border px-3 py-2 text-sm font-medium leading-tight transition-colors ${
                                  field.value === tz
                                    ? "border-primary bg-primary/10 text-primary"
                                    : "border-border bg-background hover:border-primary/50 hover:bg-accent"
                                }`}
                              >
                                {formatTimezoneLabel(tz)}
                              </button>
                            ))}
                            <button
                              type="button"
                              onClick={() => setShowTimezoneSearch(true)}
                              className="border-border bg-background hover:border-primary/50 hover:bg-accent ring-offset-background focus-visible:ring-ring inline-flex min-h-10 cursor-pointer items-center justify-center gap-1.5 whitespace-nowrap rounded-md border px-3 py-2 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-2"
                            >
                              <Icons.Search className="size-4 shrink-0" />
                              {t("onboarding:steps.preferences.other")}
                            </button>
                          </div>
                        </FormControl>
                        <FormMessage />
                      </FormItem>
                    )}
                  />
                </form>
              </Form>
            </CardContent>
          </Card>
        </div>

        {showCurrencySearch && (
          <div className="bg-background/80 animate-in fade-in fixed inset-0 z-50 flex items-center justify-center p-4 backdrop-blur-sm duration-200">
            <Card className="w-full max-w-md border shadow-lg">
              <div className="p-6">
                <div className="mb-4 flex items-center justify-between">
                  <h3 className="text-xl font-bold">
                    {t("onboarding:steps.preferences.selectCurrency")}
                  </h3>
                  <button
                    onClick={() => {
                      setShowCurrencySearch(false);
                      setCurrencySearch("");
                    }}
                    className="hover:bg-accent rounded-lg p-2 transition-colors"
                  >
                    <Icons.Close className="h-5 w-5" />
                  </button>
                </div>

                <div className="relative mb-4">
                  <Icons.Search className="text-muted-foreground absolute left-3 top-1/2 h-5 w-5 -translate-y-1/2 transform" />
                  <Input
                    type="text"
                    placeholder={t("onboarding:steps.preferences.searchCurrenciesPlaceholder")}
                    value={currencySearch}
                    onChange={(e) => setCurrencySearch(e.target.value)}
                    className="pl-10"
                    autoFocus
                  />
                </div>

                <div className="max-h-96 space-y-1 overflow-y-auto pr-2">
                  {filteredCurrencies.map((curr) => (
                    <button
                      key={curr.value}
                      onClick={() => handleCurrencySelect(curr.value)}
                      className={`flex w-full items-center justify-between rounded-lg p-3 transition-all ${
                        currentCurrency === curr.value ? "bg-primary/10" : "hover:bg-accent"
                      }`}
                    >
                      <div className="flex items-center gap-3">
                        <div className="text-left">
                          <div className="font-semibold">{curr.value}</div>
                          <div className="text-muted-foreground text-sm">{curr.label}</div>
                        </div>
                      </div>
                      {currentCurrency === curr.value && (
                        <Icons.CheckCircle className="text-primary h-5 w-5" />
                      )}
                    </button>
                  ))}
                  {filteredCurrencies.length === 0 && (
                    <div className="text-muted-foreground py-8 text-center">
                      {t("onboarding:steps.preferences.noCurrenciesFound")}
                    </div>
                  )}
                </div>
              </div>
            </Card>
          </div>
        )}

        {showTimezoneSearch && (
          <div className="bg-background/80 animate-in fade-in fixed inset-0 z-50 flex items-center justify-center p-4 backdrop-blur-sm duration-200">
            <Card className="w-full max-w-md border shadow-lg">
              <div className="p-6">
                <div className="mb-4 flex items-center justify-between">
                  <h3 className="text-xl font-bold">
                    {t("onboarding:steps.preferences.selectTimezone")}
                  </h3>
                  <button
                    onClick={() => {
                      setShowTimezoneSearch(false);
                      setTimezoneSearch("");
                    }}
                    className="hover:bg-accent rounded-lg p-2 transition-colors"
                  >
                    <Icons.Close className="h-5 w-5" />
                  </button>
                </div>

                <div className="relative mb-4">
                  <Icons.Search className="text-muted-foreground absolute left-3 top-1/2 h-5 w-5 -translate-y-1/2 transform" />
                  <Input
                    type="text"
                    placeholder={t("onboarding:steps.preferences.searchTimezonesPlaceholder")}
                    value={timezoneSearch}
                    onChange={(e) => setTimezoneSearch(e.target.value)}
                    className="pl-10"
                    autoFocus
                  />
                </div>

                <div className="max-h-96 space-y-1 overflow-y-auto pr-2">
                  {filteredTimezones.map((tz) => (
                    <button
                      key={tz}
                      onClick={() => handleTimezoneSelect(tz)}
                      className={`flex w-full items-center justify-between rounded-lg p-3 transition-all ${
                        currentTimezone === tz ? "bg-primary/10" : "hover:bg-accent"
                      }`}
                    >
                      <div className="flex items-center gap-3">
                        <div className="text-left">
                          <div className="font-semibold">{formatTimezoneLabel(tz)}</div>
                          <div className="text-muted-foreground text-sm">{tz}</div>
                        </div>
                      </div>
                      {currentTimezone === tz && (
                        <Icons.CheckCircle className="text-primary h-5 w-5" />
                      )}
                    </button>
                  ))}
                  {filteredTimezones.length === 0 && (
                    <div className="text-muted-foreground py-8 text-center">
                      {t("onboarding:steps.preferences.noTimezonesFound")}
                    </div>
                  )}
                </div>
              </div>
            </Card>
          </div>
        )}
      </>
    );
  },
);

OnboardingStep2.displayName = "OnboardingStep2";
