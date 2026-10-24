import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { listAllocationTargets } from "@/adapters";
import { useAllocationTargets } from "./use-allocation-targets";

vi.mock("@/adapters", () => ({
  listAllocationTargets: vi.fn(),
}));

const mockListAllocationTargets = vi.mocked(listAllocationTargets);

function wrapper({ children }: { children: ReactNode }) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });

  return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
}

describe("useAllocationTargets", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("returns a stable empty target list when the query fails", async () => {
    mockListAllocationTargets.mockRejectedValue(new Error("list failed"));

    const { result, rerender } = renderHook(() => useAllocationTargets(), { wrapper });
    const emptyTargets = result.current.targets;

    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(result.current.targets).toBe(emptyTargets);

    rerender();
    expect(result.current.targets).toBe(emptyTargets);
  });
});
