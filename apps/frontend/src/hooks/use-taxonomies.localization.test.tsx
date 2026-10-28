import { updateCategory } from "@/adapters";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@/test/render";
import type { ReactNode } from "react";
import { expect, it, vi } from "vitest";
import { useTaxonomies, useTaxonomy, useUpdateCategory } from "./use-taxonomies";
import { QueryKeys } from "@/lib/query-keys";
import type { Taxonomy, TaxonomyCategory } from "@/lib/types";

const taxonomy: Taxonomy = {
  id: "instrument_type",
  name: "Instrument Type",
  isSystem: true,
  isSingleSelect: true,
  color: "#4385be",
  sortOrder: 1,
  scope: "asset",
  createdAt: "",
  updatedAt: "",
};
const category: TaxonomyCategory = {
  id: "STOCK_PREFERRED",
  taxonomyId: taxonomy.id,
  name: "Preferred Stock",
  key: "STOCK_PREFERRED",
  color: "#4385be",
  sortOrder: 1,
  createdAt: "",
  updatedAt: "",
};
vi.mock("@/adapters", () => ({
  updateCategory: vi.fn((row: TaxonomyCategory) => Promise.resolve(row)),
  getTaxonomies: vi.fn(() => Promise.resolve([taxonomy])),
  getTaxonomy: vi.fn(() => Promise.resolve({ taxonomy, categories: [category] })),
}));

it("localizes display observers while keeping cached data and editor data raw", async () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  const { result } = renderHook(
    () => ({
      list: useTaxonomies({ scope: "asset" }),
      display: useTaxonomy(taxonomy.id),
      editor: useTaxonomy(taxonomy.id, { localize: false }),
    }),
    { wrapper },
  );
  await waitFor(() => expect(result.current.display.data?.categories[0].name).toBe("优先股"));
  expect(result.current.list.data?.[0].name).toBe("金融工具类型");
  expect(result.current.editor.data?.categories[0].name).toBe("Preferred Stock");
  expect(result.current.editor.data?.taxonomy.name).toBe("Instrument Type");
  expect(client.getQueryData(QueryKeys.taxonomy(taxonomy.id))).toEqual({
    taxonomy,
    categories: [category],
  });
  expect(client.getQueryData([QueryKeys.TAXONOMIES])).toEqual([taxonomy]);
});

it("preserves stored names on unrelated edits while accepting explicit renames", async () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  client.setQueryData(QueryKeys.taxonomy(taxonomy.id), { taxonomy, categories: [category] });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  const { result } = renderHook(() => useUpdateCategory(), { wrapper });
  await act(async () => {
    await result.current.mutateAsync({ ...category, name: "优先股", color: "#123456" });
  });
  expect(updateCategory).toHaveBeenLastCalledWith({ ...category, color: "#123456" });
  await act(async () => {
    await result.current.mutateAsync({ ...category, name: "My preferred stocks" });
  });
  expect(updateCategory).toHaveBeenLastCalledWith({ ...category, name: "My preferred stocks" });
});
