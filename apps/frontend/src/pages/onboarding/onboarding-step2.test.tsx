import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { OnboardingStep2 } from "./onboarding-step2";

const mocks = vi.hoisted(() => ({
  settings: { baseCurrency: "", timezone: "" } as { baseCurrency: string; timezone: string },
  updateSettings: vi.fn(() => Promise.resolve()),
}));

vi.mock("@/lib/settings-provider", () => ({
  useSettingsContext: () => ({
    settings: mocks.settings,
    updateSettings: mocks.updateSettings,
  }),
}));

describe("OnboardingStep2", () => {
  it("preselects CNY for a new ledger and does not offer a language control", () => {
    mocks.settings = { baseCurrency: "", timezone: "Asia/Shanghai" };
    render(<OnboardingStep2 onNext={vi.fn()} onValidityChange={vi.fn()} />);

    expect(screen.getByTestId("currency-cny-button")).toHaveClass("border-primary");
    expect(screen.queryByTestId("language-en-button")).not.toBeInTheDocument();
    expect(screen.queryByTestId("onboarding-formatting-locale")).not.toBeInTheDocument();
  });

  it("keeps an existing ledger currency", () => {
    mocks.settings = { baseCurrency: "USD", timezone: "Asia/Shanghai" };
    render(<OnboardingStep2 onNext={vi.fn()} onValidityChange={vi.fn()} />);

    expect(screen.getByTestId("currency-usd-button")).toHaveClass("border-primary");
    expect(screen.getByTestId("currency-cny-button")).not.toHaveClass("border-primary");
  });
});
