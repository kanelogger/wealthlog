import "@testing-library/jest-dom";
import { cleanup } from "@testing-library/react";
import { afterEach, expect, vi } from "vitest";
import * as matchers from "@testing-library/jest-dom/matchers";

import i18n from "@/i18n/i18n";
import { consumeMissingKeys } from "@/i18n/missing-keys";

void i18n;

expect.extend(matchers);

afterEach(() => {
  cleanup();
  const missing = consumeMissingKeys();
  expect(missing, `Missing translation keys:\n${missing.join("\n")}`).toEqual([]);
});

if (typeof window !== "undefined") {
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  });
}
