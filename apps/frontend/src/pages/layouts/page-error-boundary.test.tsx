import { fireEvent, render, screen } from "@testing-library/react";
import { Link, MemoryRouter, Route, Routes, useSearchParams } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PageErrorBoundary } from "./page-error-boundary";

function Broken(): never {
  throw new Error("page failed");
}

/** A page whose `?tab=broken` view fails, as a query-driven view can. */
function Page({ name }: { name: string }) {
  const [params] = useSearchParams();
  if (params.get("tab") === "broken") throw new Error("view failed");
  return <p>{name} page</p>;
}

const app = (path: string) =>
  render(
    <MemoryRouter initialEntries={[path]}>
      <nav>
        Sidebar <Link to="/holdings">Holdings link</Link>
      </nav>
      <PageErrorBoundary>
        <Routes>
          <Route path="/" element={<Page name="活动" />} />
          <Route path="/activities" element={<Page name="活动" />} />
          <Route path="/health" element={<Broken />} />
          <Route path="/holdings" element={<Page name="持仓" />} />
        </Routes>
      </PageErrorBoundary>
    </MemoryRouter>,
  );

describe("a page that fails to render", () => {
  beforeEach(() => {
    // React reports the caught error; the boundary handles it.
    vi.spyOn(console, "error").mockImplementation(() => undefined);
  });
  afterEach(() => vi.restoreAllMocks());

  it("leaves navigation usable and offers a way back to the dashboard", () => {
    app("/health");
    expect(screen.getByRole("alert")).toHaveTextContent("出现问题");
    expect(screen.getByText(/Sidebar/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "前往活动" }));
    // The next route starts afresh.
    expect(screen.getByText("活动 page")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("recovers when navigation changes only the query", () => {
    app("/holdings?tab=broken");
    expect(screen.getByRole("alert")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("link", { name: "Holdings link" }));
    expect(screen.getByText("持仓 page")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("offers the plain dashboard from a dashboard view that fails", () => {
    app("/?tab=broken");
    fireEvent.click(screen.getByRole("button", { name: "前往活动" }));
    expect(screen.getByText("活动 page")).toBeInTheDocument();
  });

  it("offers no way to the dashboard from the dashboard itself", () => {
    app("/activities?tab=broken");
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "前往活动" })).toBeInTheDocument();
  });
});
