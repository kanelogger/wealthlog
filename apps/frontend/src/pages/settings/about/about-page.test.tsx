import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it } from "vitest";
import { version } from "../../../../../../package.json";
import AboutPage from "./about-page";

describe("AboutPage", () => {
  it("offers offline legal files and the public source address", () => {
    render(
      <MemoryRouter>
        <AboutPage />
      </MemoryRouter>,
    );

    expect(screen.getByText(new RegExp(`Wealthlog ${version}`))).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "阅读完整许可证" })).toHaveAttribute(
      "href",
      "/legal/LICENSE.txt",
    );
    expect(screen.getByRole("link", { name: "第三方声明" })).toHaveAttribute(
      "href",
      "/legal/THIRD_PARTY_NOTICES.txt",
    );
    expect(screen.getByRole("link", { name: "获取源码" })).toHaveAttribute(
      "href",
      "https://github.com/kanelogger/wealthlog",
    );
  });
});
