import { render, screen } from "@/test/render";
import { expect, it, vi } from "vitest";
import { ModelPresetPicker, BUILT_IN_PRESETS } from "./model-preset-picker";

it("shows Chinese built-in template names, descriptions, risks and categories without holdings", () => {
  render(
    <ModelPresetPicker
      taxonomyId="asset_classes"
      selected={null}
      onSelect={vi.fn()}
      currentCategories={[]}
    />,
  );
  expect(screen.getByRole("button", { name: /均衡型 60 \/ 40/ })).toHaveTextContent(
    "权益类资产 60% / 固定收益 40%",
  );
  expect(screen.getByText("经典股债组合")).toBeInTheDocument();
  expect(screen.getByText("均衡")).toBeInTheDocument();
  expect(screen.queryByText("Conservative")).not.toBeInTheDocument();
  for (const preset of BUILT_IN_PRESETS) {
    expect(preset.name).toMatch(/[\u3400-\u9fff]/);
    expect(preset.description).toMatch(/[\u3400-\u9fff]/);
  }
});
