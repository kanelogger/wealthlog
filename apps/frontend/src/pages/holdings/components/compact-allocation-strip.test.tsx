import { render, screen } from "@/test/render";
import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";
import { CompactAllocationStrip } from "./compact-allocation-strip";

it("uses risk IDs to retain the correct percentages after labels are translated", async () => {
  const onSegmentClick = vi.fn();
  render(
    <CompactAllocationStrip
      title="风险分布"
      variant="risk-composition"
      onSegmentClick={onSegmentClick}
      allocation={{
        taxonomyId: "risk_category",
        taxonomyName: "风险等级",
        color: "#111111",
        categories: [
          {
            categoryId: "LOW",
            categoryName: "低风险",
            color: "#111111",
            value: 60,
            percentage: 60,
          },
          {
            categoryId: "HIGH",
            categoryName: "高风险",
            color: "#222222",
            value: 40,
            percentage: 40,
          },
        ],
      }}
    />,
  );
  const low = screen.getByRole("button", { name: /低.*60/ });
  const high = screen.getByRole("button", { name: /高.*40/ });
  expect(low).toBeInTheDocument();
  expect(high).toBeInTheDocument();
  await userEvent.setup().click(low);
  expect(onSegmentClick).toHaveBeenCalledWith("LOW", "低风险");
});
