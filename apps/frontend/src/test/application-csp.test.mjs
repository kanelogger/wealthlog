// @vitest-environment node
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const html = readFileSync(new URL("../../index.html", import.meta.url), "utf8");
const serverSource = readFileSync(new URL("../../../server/src/api.rs", import.meta.url), "utf8");

describe("application startup CSP", () => {
  it("allows the host's inline theme initializer by its exact server hash", () => {
    const policy = serverSource.match(/const SERVER_CSP: &str = "([^"]+)";/)?.[1];
    expect(policy).toBeDefined();
    const scriptSources = policy
      ?.split(";")
      .find((directive) => directive.trim().startsWith("script-src "))
      ?.trim()
      .split(/\s+/)
      .slice(1);
    const inlineScripts = [...html.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/g)].filter(
      ([, attributes]) => !/\bsrc\s*=/.test(attributes),
    );
    expect(inlineScripts.length).toBeGreaterThan(0);
    for (const [, , body] of inlineScripts) {
      const hash = createHash("sha256").update(body).digest("base64");
      expect(scriptSources).toContain(`'sha256-${hash}'`);
    }
    expect(scriptSources).not.toContain("'unsafe-inline'");
  });

  it("loads splash styles externally so Tauri preserves runtime styles", () => {
    const renderedHtml = html.replace(/<!--[\s\S]*?-->/g, "");
    expect(renderedHtml).not.toMatch(/<style\b/i);
    expect(html).toContain('<link rel="stylesheet" href="/splash.css" />');
  });
});
