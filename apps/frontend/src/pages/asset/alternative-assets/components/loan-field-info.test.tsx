import { render, screen } from "@/test/render";
import { expect, it } from "vitest";
import { LoanFieldInfo } from "./loan-field-info";

it("names the info button after its field", () => {
  render(<LoanFieldInfo label="付款账户">Help</LoanFieldInfo>);
  expect(screen.getByRole("button", { name: "关于 付款账户 的更多信息" })).toBeInTheDocument();
});
