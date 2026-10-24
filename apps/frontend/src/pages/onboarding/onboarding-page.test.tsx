import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { forwardRef, useImperativeHandle } from "react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import OnboardingPage from "./onboarding-page";

const mocks = vi.hoisted(() => ({
  updateSettings: vi.fn(),
  saveAppearance: vi.fn<() => Promise<void>>(),
}));
vi.mock("@/hooks/use-settings", () => ({
  useSettings: () => ({ data: { onboardingCompleted: false } }),
}));
vi.mock("@/lib/settings-provider", () => ({
  useSettingsContext: () => ({ updateSettings: mocks.updateSettings }),
}));
vi.mock("@/hooks/use-platform", () => ({ usePlatform: () => ({ isMobile: false }) }));
vi.mock("./onboarding-step1", () => ({ OnboardingStep1: () => <div>Welcome</div> }));
vi.mock("./onboarding-step2", () => ({
  OnboardingStep2: forwardRef(function MockPreferences({ onNext }: { onNext: () => void }, ref) {
    useImperativeHandle(ref, () => ({ submitForm: onNext }));
    return <div>Preferences</div>;
  }),
}));
vi.mock("./onboarding-appearance", () => ({
  OnboardingAppearance: forwardRef(function MockAppearance(
    { onNext }: { onNext: () => void },
    ref,
  ) {
    useImperativeHandle(ref, () => ({
      submitForm: () =>
        mocks
          .saveAppearance()
          .then(onNext)
          .catch(() => undefined),
    }));
    return <div>Appearance</div>;
  }),
}));

beforeEach(() => {
  mocks.updateSettings.mockReset();
  mocks.saveAppearance.mockReset().mockResolvedValue(undefined);
});

function reachAppearance() {
  render(
    <MemoryRouter>
      <OnboardingPage />
    </MemoryRouter>,
  );
  fireEvent.click(screen.getByTestId("onboarding-continue-button"));
  fireEvent.click(screen.getByTestId("onboarding-continue-button"));
}

describe("local onboarding", () => {
  it("finishes after saving appearance, without a cloud subscription step", async () => {
    reachAppearance();
    expect(mocks.updateSettings).not.toHaveBeenCalled();
    fireEvent.click(screen.getByTestId("onboarding-finish-button"));
    await waitFor(() =>
      expect(mocks.updateSettings).toHaveBeenCalledWith({ onboardingCompleted: true }),
    );
    expect(mocks.saveAppearance).toHaveBeenCalledOnce();
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
  });

  it("does not finish when saving appearance fails", async () => {
    mocks.saveAppearance.mockRejectedValue(new Error("save failed"));
    reachAppearance();
    fireEvent.click(screen.getByTestId("onboarding-finish-button"));
    await waitFor(() => expect(mocks.saveAppearance).toHaveBeenCalledOnce());
    expect(mocks.updateSettings).not.toHaveBeenCalled();
  });
});
