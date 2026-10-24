import { render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TickerAvatar as SharedTickerAvatar } from "@wealthfolio/ui";
import { getAssetLogo } from "@/adapters";
import { assetLogoRegistry } from "@/lib/asset-logo-registry";
import type { AssetLogoSummary } from "@/lib/types";
import { TickerAvatar } from "./ticker-avatar";

vi.mock("@/adapters", () => ({ getAssetLogo: vi.fn().mockResolvedValue(null) }));

const failingSources = new Set<string>();
const attemptedSources: string[] = [];

class FakeImage {
  private listeners = new Map<string, Set<() => void>>();
  private currentSrc = "";
  complete = false;
  naturalWidth = 0;

  get src() {
    return this.currentSrc;
  }

  set src(value: string) {
    this.currentSrc = value;
    attemptedSources.push(value);
    queueMicrotask(() => {
      if (this.currentSrc !== value) return;
      for (const listener of this.listeners.get(failingSources.has(value) ? "error" : "load") ??
        []) {
        listener();
      }
    });
  }

  addEventListener(type: string, listener: () => void) {
    const set = this.listeners.get(type) ?? new Set();
    set.add(listener);
    this.listeners.set(type, set);
  }

  removeEventListener(type: string, listener: () => void) {
    this.listeners.get(type)?.delete(listener);
  }
}

function summary(assetId: string, displayCode: string): AssetLogoSummary {
  return {
    assetId,
    displayCode,
    sha256: `sha-${assetId}`,
    updatedAt: "2026-09-02T00:00:00Z",
  };
}

const dataUriFor = (assetId: string) => `data:image/png;base64,${assetId}`;

function primeOverride(assetId: string, displayCode: string) {
  assetLogoRegistry.prime(`sha-${assetId}`, dataUriFor(assetId));
  return summary(assetId, displayCode);
}

describe("TickerAvatar", () => {
  beforeEach(() => vi.stubGlobal("Image", FakeImage));
  afterEach(() => {
    vi.unstubAllGlobals();
    failingSources.clear();
    attemptedSources.length = 0;
    assetLogoRegistry.reset();
  });

  it.each([
    ["CASH:USD", "$"],
    ["CASH:CAD", "C$"],
    ["$cash-eur", "EUR"],
  ])("renders %s as a currency label without image requests", (symbol, label) => {
    const view = render(<TickerAvatar symbol={symbol} />);
    expect(view.getByTitle(symbol.toUpperCase())).toHaveTextContent(label);
    expect(view.container.querySelector("img")).toBeNull();
    expect(attemptedSources).toEqual([]);
  });

  it.each([
    ["AAPL", undefined, undefined, "AAPL"],
    ["SHOP", "XTSE", undefined, "SHOP"],
    ["ACT", undefined, "CRYPTO", "ACT"],
    ["CASH.TO", undefined, undefined, "CASH"],
    ["ABCDE", undefined, undefined, "ABCD"],
  ])(
    "shows text for %s without bundled logo requests",
    (symbol, exchangeMic, instrumentType, label) => {
      const view = render(
        <TickerAvatar symbol={symbol} exchangeMic={exchangeMic} instrumentType={instrumentType} />,
      );
      expect(view.getByTitle(symbol.toUpperCase())).toHaveTextContent(label);
      expect(view.container.querySelector("img")).toBeNull();
      expect(attemptedSources).toEqual([]);
    },
  );

  it("uses the asset-scoped custom image and falls back to text if it fails", async () => {
    assetLogoRegistry.setIndex([primeOverride("a1", "AAPL")]);
    failingSources.add(dataUriFor("a1"));
    const view = render(<TickerAvatar symbol="AAPL" assetId="a1" />);
    await waitFor(() => {
      expect(attemptedSources).toEqual([dataUriFor("a1")]);
      expect(view.container.querySelector("img")).toBeNull();
    });
    expect(view.getByTitle("AAPL")).toHaveTextContent("AAPL");
    expect(attemptedSources).toEqual([dataUriFor("a1")]);
  });

  it("uses the asset id rather than a different asset's symbol match", async () => {
    assetLogoRegistry.setIndex([primeOverride("a1", "AAPL"), primeOverride("a2", "MSFT")]);
    const view = render(<TickerAvatar symbol="AAPL" assetId="a2" />);
    await waitFor(() =>
      expect(view.container.querySelector("img")).toHaveAttribute("src", dataUriFor("a2")),
    );
  });

  it("does not show another asset's custom image when its id has no override", () => {
    assetLogoRegistry.setIndex([primeOverride("a1", "AAPL")]);
    const view = render(<TickerAvatar symbol="AAPL" assetId="a2" />);
    expect(view.container.querySelector("img")).toBeNull();
    expect(view.getByTitle("AAPL")).toHaveTextContent("AAPL");
  });

  it("prefers an explicit preview image", async () => {
    assetLogoRegistry.setIndex([primeOverride("a1", "AAPL")]);
    const view = render(
      <TickerAvatar symbol="AAPL" assetId="a1" src="data:image/png;base64,preview" />,
    );
    await waitFor(() =>
      expect(view.container.querySelector("img")).toHaveAttribute(
        "src",
        "data:image/png;base64,preview",
      ),
    );
  });

  it("retries when a failed preview is replaced by an equally sized image", async () => {
    failingSources.add("data:image/png;base64,one");
    const view = render(<TickerAvatar symbol="AAPL" src="data:image/png;base64,one" />);
    await waitFor(() => {
      expect(attemptedSources).toContain("data:image/png;base64,one");
      expect(view.container.querySelector("img")).toBeNull();
    });
    view.rerender(<TickerAvatar symbol="AAPL" src="data:image/png;base64,two" />);
    await waitFor(() =>
      expect(view.container.querySelector("img")).toHaveAttribute(
        "src",
        "data:image/png;base64,two",
      ),
    );
  });
});

describe.each([
  ["app", TickerAvatar],
  ["shared", SharedTickerAvatar],
] as const)("%s avatar", (_name, AvatarComponent) => {
  afterEach(() => {
    vi.unstubAllGlobals();
    attemptedSources.length = 0;
  });

  it("keeps cash labels when switching from and to a security", () => {
    vi.stubGlobal("Image", FakeImage);
    const view = render(<AvatarComponent symbol="CASH.TO" />);
    expect(view.getByTitle("CASH.TO")).toHaveTextContent("CASH");
    expect(view.container.querySelector("img")).toBeNull();
    view.rerender(<AvatarComponent symbol="$CASH" />);
    expect(view.getByTitle("$CASH")).toHaveTextContent("$");
    view.rerender(<AvatarComponent symbol="AAPL" />);
    expect(view.getByTitle("AAPL")).toHaveTextContent("AAPL");
    expect(attemptedSources).toEqual([]);
  });
});

it("does not fetch a custom override for the app cash avatar", () => {
  vi.mocked(getAssetLogo).mockClear();
  assetLogoRegistry.setIndex([summary("cash", "$CASH")]);
  try {
    const view = render(<TickerAvatar symbol="$CASH" assetId="cash" src="preview.png" />);
    expect(view.getByTitle("$CASH")).toHaveTextContent("$");
    expect(view.container.querySelector("img")).toBeNull();
    expect(getAssetLogo).not.toHaveBeenCalled();
  } finally {
    assetLogoRegistry.reset();
  }
});
