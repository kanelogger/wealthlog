import { render, screen } from "@testing-library/react";
import { FormattingProvider, GainAmount, GainPercent } from "@wealthfolio/ui";
import { describe, expect, it } from "vitest";

describe("localized financial components", () => {
  it("preserves Wealthlog quote-unit symbols in gain amounts", () => {
    render(
      <FormattingProvider locale="CN" uiLocale="zh">
        <GainAmount value={12.34} currency="GBp" />
      </FormattingProvider>,
    );
    expect(screen.getByText("+12.34p")).toBeInTheDocument();
  });

  it("formats an animated percent with China separators", () => {
    const { container } = render(
      <FormattingProvider locale="zh-CN" uiLocale="zh">
        <GainPercent value={0.125} animated />
      </FormattingProvider>,
    );
    expect(container.textContent).toMatch(/^\+12\.50%$/);
  });
});
