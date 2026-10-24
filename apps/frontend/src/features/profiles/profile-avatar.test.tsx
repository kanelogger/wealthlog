import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { DEFAULT_PROFILE_AVATAR, PROFILE_AVATARS, ProfileAvatar } from "./profile-avatar";

describe("profile avatars", () => {
  it("preserves every backend avatar ID while rendering the neutral fallback", () => {
    const source = readFileSync(resolve("../../crates/core/src/profiles/mod.rs"), "utf8");
    const catalog = source.split("pub const PROFILE_AVATARS: &[&str] = &[")[1].split("];")[0];
    const defaultId = /pub const DEFAULT_PROFILE_AVATAR: &str = "([^"]+)"/.exec(source)![1];
    expect(defaultId).toBe(DEFAULT_PROFILE_AVATAR);
    const ids = [defaultId, ...[...catalog.matchAll(/"([^"]+)"/g)].map((match) => match[1])];
    expect([...ids].sort()).toEqual([...PROFILE_AVATARS].sort());
    const fallback = renderToStaticMarkup(<ProfileAvatar id={DEFAULT_PROFILE_AVATAR} />);
    for (const id of ids) {
      expect(PROFILE_AVATARS, id).toContain(id);
      const markup = renderToStaticMarkup(<ProfileAvatar id={id} />);
      expect(markup).toBe(fallback);
      expect(markup).toContain("<svg");
      expect(markup).not.toContain("/avatars/");
    }
  });

  it("uses the neutral default for unknown IDs", () => {
    expect(renderToStaticMarkup(<ProfileAvatar id="../../missing" />)).toBe(
      renderToStaticMarkup(<ProfileAvatar id={DEFAULT_PROFILE_AVATAR} />),
    );
  });
});
