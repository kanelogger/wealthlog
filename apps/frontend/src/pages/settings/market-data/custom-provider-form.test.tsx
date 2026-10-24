import { fireEvent, render, screen, waitFor } from "@/test/render";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { Asset } from "@/lib/types";
import type { CustomProviderWithSources, NewCustomProvider } from "@/lib/types/custom-provider";
import { CustomProviderForm } from "./custom-provider-form";

const createProvider = vi.fn();
const updateProvider = vi.fn();
const testSource = vi.fn();

// jsdom lacks ResizeObserver, which the Radix radio group measures with.
if (typeof ResizeObserver === "undefined") {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as typeof ResizeObserver;
}

function setInputValue(input: HTMLElement, value: string) {
  fireEvent.change(input, { target: { value } });
}

vi.mock("@wealthfolio/ui", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@wealthfolio/ui")>()),
  Dialog: ({ open, children }: { open: boolean; children: ReactNode }) =>
    open ? <div>{children}</div> : null,
  DialogContent: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  DialogDescription: ({ children, ...props }: { children: ReactNode }) => (
    <p {...props}>{children}</p>
  ),
  DialogTitle: ({ children, ...props }: { children: ReactNode }) => <h2 {...props}>{children}</h2>,
}));

vi.mock("@/adapters", () => ({
  openUrlInBrowser: vi.fn(),
}));

vi.mock("@/hooks/use-custom-providers", () => ({
  useCreateCustomProvider: () => ({
    mutate: createProvider,
    isPending: false,
  }),
  useUpdateCustomProvider: () => ({
    mutate: updateProvider,
    isPending: false,
  }),
  useTestCustomProviderSource: () => ({
    mutate: testSource,
    isPending: false,
  }),
}));

vi.mock("@/lib/settings-provider", () => ({
  useSettingsContext: () => ({
    settings: { timezone: "UTC" },
  }),
}));

describe("CustomProviderForm", () => {
  beforeEach(() => {
    createProvider.mockReset();
    updateProvider.mockReset();
    testSource.mockReset();
  });

  it("keeps latest and historical source values separate while switching tabs", async () => {
    const user = userEvent.setup();

    render(<CustomProviderForm open onOpenChange={vi.fn()} />);

    await user.click(screen.getByRole("button", { name: /两者/ }));

    const urlInput = () => screen.getByLabelText(/URL 模板/);
    const pricePathInput = () => screen.getByPlaceholderText("$.data.price");

    setInputValue(urlInput(), "https://latest.example.com/price/{SYMBOL}");
    setInputValue(pricePathInput(), "$.price");

    await user.click(screen.getByRole("button", { name: /历史/ }));

    setInputValue(urlInput(), "https://history.example.com/prices/{SYMBOL}");
    setInputValue(pricePathInput(), "$[*].adj_close");

    await user.click(screen.getByRole("button", { name: /最新价格/ }));
    expect(urlInput()).toHaveValue("https://latest.example.com/price/{SYMBOL}");
    expect(pricePathInput()).toHaveValue("$.price");

    await user.click(screen.getByRole("button", { name: /历史/ }));
    expect(urlInput()).toHaveValue("https://history.example.com/prices/{SYMBOL}");
    expect(pricePathInput()).toHaveValue("$[*].adj_close");

    const createButton = screen.getByRole("button", { name: /创建提供商/ });
    fireEvent.submit(createButton.closest("form")!);

    await waitFor(() => expect(createProvider).toHaveBeenCalledTimes(1));
    const payload = createProvider.mock.calls[0][0] as NewCustomProvider;

    expect(payload.sources).toEqual([
      expect.objectContaining({
        kind: "latest",
        url: "https://latest.example.com/price/{SYMBOL}",
        pricePath: "$.price",
      }),
      expect.objectContaining({
        kind: "historical",
        url: "https://history.example.com/prices/{SYMBOL}",
        pricePath: "$[*].adj_close",
      }),
    ]);
  }, 10_000);

  it("shows body-only identity inputs and passes them to the source tester", async () => {
    const user = userEvent.setup();

    render(<CustomProviderForm open onOpenChange={vi.fn()} />);

    setInputValue(screen.getByLabelText(/URL 模板/), "https://example.test/quotes");
    await user.selectOptions(screen.getByLabelText(/HTTP 方法/), "POST");
    setInputValue(screen.getByLabelText(/请求正文/), '{"isin":"{ISIN}","mic":"{MIC}"}');

    setInputValue(screen.getByPlaceholderText("例如 AAPL"), "AAPL");
    setInputValue(screen.getByPlaceholderText("US0378331005"), "US5949181045");
    setInputValue(screen.getByPlaceholderText("XLON"), "XNAS");
    await user.click(screen.getByRole("button", { name: /^获取$/ }));

    expect(testSource).toHaveBeenCalledWith(
      expect.objectContaining({
        url: "https://example.test/quotes",
        body: '{"isin":"{ISIN}","mic":"{MIC}"}',
        symbol: "AAPL",
        isin: "US5949181045",
        mic: "XNAS",
      }),
      expect.any(Object),
    );
  });

  it("resets POST state when applying a quick-start template", async () => {
    const user = userEvent.setup();

    render(<CustomProviderForm open onOpenChange={vi.fn()} />);

    await user.selectOptions(screen.getByLabelText(/HTTP 方法/), "POST");
    setInputValue(screen.getByLabelText(/请求正文/), '{"symbol":"{SYMBOL}"}');

    await user.click(screen.getByRole("button", { name: /coingecko/i }));

    expect(screen.getByLabelText(/HTTP 方法/)).toHaveValue("GET");
    expect(screen.queryByLabelText(/请求正文/)).not.toBeInTheDocument();
  });

  it("clears the request body when switching between GET and POST", async () => {
    const user = userEvent.setup();

    render(<CustomProviderForm open onOpenChange={vi.fn()} />);

    const method = screen.getByLabelText(/HTTP 方法/);
    await user.selectOptions(method, "POST");
    setInputValue(screen.getByLabelText(/请求正文/), '{"symbol":"{SYMBOL}"}');

    await user.selectOptions(method, "GET");
    expect(screen.queryByLabelText(/请求正文/)).not.toBeInTheDocument();

    await user.selectOptions(method, "POST");
    expect(screen.getByLabelText(/请求正文/)).toHaveValue("");
  });

  function fillLatestSource(url: string) {
    setInputValue(screen.getByLabelText(/URL 模板/), url);
    setInputValue(screen.getByPlaceholderText("$.data.price"), "$.price");
  }

  async function submittedPayload() {
    const createButton = screen.getByRole("button", { name: /创建提供商/ });
    fireEvent.submit(createButton.closest("form")!);
    await waitFor(() => expect(createProvider).toHaveBeenCalledTimes(1));
    return createProvider.mock.calls[0][0] as NewCustomProvider;
  }

  it("creates providers that serve only the securities assigned to them", async () => {
    render(<CustomProviderForm open onOpenChange={vi.fn()} />);
    fillLatestSource("https://funds.example.com/nav/{ISIN}");

    expect(screen.getByRole("radio", { name: /仅限分配给它的资产/ })).toBeChecked();
    expect(screen.queryByLabelText(/备用提供商顺序/)).not.toBeInTheDocument();
    expect((await submittedPayload()).useAsFallback).toBe(false);
  });

  it("lets keyboard users switch the usage with arrow keys", async () => {
    const user = userEvent.setup();
    render(<CustomProviderForm open onOpenChange={vi.fn()} />);

    screen.getByRole("radio", { name: /仅限分配给它的资产/ }).focus();
    // Radix moves focus on the next tick and selects only while the arrow key is
    // still down, so hold it the way a real key press does.
    await user.keyboard("{ArrowDown>}");
    await waitFor(() =>
      expect(screen.getByRole("radio", { name: /同时作为其他资产的备用提供商/ })).toBeChecked(),
    );
    await user.keyboard("{/ArrowDown}");

    expect(screen.getByLabelText(/备用提供商顺序/)).toBeInTheDocument();
  });

  it("sends fallback use and its order when chosen", async () => {
    const user = userEvent.setup();
    render(<CustomProviderForm open onOpenChange={vi.fn()} />);
    fillLatestSource("https://funds.example.com/nav/{ISIN}");

    await user.click(screen.getByRole("radio", { name: /同时作为其他资产的备用提供商/ }));
    setInputValue(screen.getByLabelText(/备用提供商顺序/), "7");

    expect(screen.queryByText(/需要包含 \{SYMBOL\} 或 \{ISIN\}/)).not.toBeInTheDocument();
    const payload = await submittedPayload();
    expect(payload.useAsFallback).toBe(true);
    expect(payload.priority).toBe(7);
  });

  it("explains that fallback use needs an identity placeholder", async () => {
    const user = userEvent.setup();
    render(<CustomProviderForm open onOpenChange={vi.fn()} />);
    fillLatestSource("https://funds.example.com/nav?fund=abc");

    await user.click(screen.getByRole("radio", { name: /同时作为其他资产的备用提供商/ }));

    expect(screen.getByText(/需要包含 \{SYMBOL\} 或 \{ISIN\}/)).toBeInTheDocument();
  });

  it("lists the securities using the provider and warns about mapping-only ones", async () => {
    const user = userEvent.setup();
    const provider: CustomProviderWithSources = {
      id: "fund",
      name: "Fund",
      description: "",
      enabled: true,
      priority: 50,
      useAsFallback: false,
      sources: [
        {
          id: "fund:latest",
          providerId: "fund",
          kind: "latest",
          format: "json",
          url: "https://funds.example.com/nav/{ISIN}",
          pricePath: "$.price",
        },
      ],
    };
    const usage = {
      assigned: [{ id: "fund-a", displayCode: "FUNDA" } as Asset],
      mappedOnly: [{ id: "aapl", displayCode: "AAPL" } as Asset],
      leftover: [],
    };

    render(<CustomProviderForm open onOpenChange={vi.fn()} provider={provider} usage={usage} />);

    expect(screen.getByText("已分配（1）")).toBeInTheDocument();
    expect(screen.getByText("FUNDA")).toBeInTheDocument();
    expect(screen.getByText("仅映射了代码（1）")).toBeInTheDocument();
    expect(screen.getByText("AAPL")).toBeInTheDocument();
    expect(screen.getByText(/因此不会使用此提供商/)).toBeInTheDocument();

    await user.click(screen.getByRole("radio", { name: /同时作为其他资产的备用提供商/ }));
    expect(screen.queryByText(/因此不会使用此提供商/)).not.toBeInTheDocument();
  });
});
