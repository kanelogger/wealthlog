import { render, screen } from "@/test/render";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it } from "vitest";

import { MetricDisplay } from "./metric-display";

describe("MetricDisplay", () => {
  it("links unavailable metric reasons to the Health Center", () => {
    const reason =
      "TWR unavailable for 2025-11-02 because an external flow amount or transfer boundary is unknown.";

    render(
      <MemoryRouter>
        <MetricDisplay
          label="时间加权收益率"
          infoText="Time-weighted return"
          emptyReason={reason}
        />
      </MemoryRouter>,
    );

    const link = screen.getByRole("link", {
      name: "为 时间加权收益率 问题打开健康中心",
    });

    expect(link).toHaveAttribute("href", "/health");
    expect(link).toHaveAttribute("title", reason);
    expect(link).toHaveTextContent(reason);
  });
});
