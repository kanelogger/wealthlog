import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const checker = fileURLToPath(new URL("./check-translations.mjs", import.meta.url));

function check(t, catalogs, sources) {
  const root = mkdtempSync(path.join(tmpdir(), "wealthfolio-i18n-"));
  const catalog = path.join(root, "catalog");
  const source = path.join(root, "source");
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(catalog);
  mkdirSync(source);
  for (const [name, messages] of Object.entries(catalogs)) {
    writeFileSync(path.join(catalog, `${name}.json`), JSON.stringify(messages));
  }
  for (const [name, text] of Object.entries(sources)) {
    writeFileSync(path.join(source, name), text);
  }
  const result = spawnSync(process.execPath, [checker, catalog, source], { encoding: "utf8" });
  assert.ifError(result.error);
  return result;
}

test("rejects a literal key that is absent from the Chinese catalog", (t) => {
  const result = check(
    t,
    { common: { save: "保存" } },
    { "page.tsx": `export const label = t("common:missing");` },
  );
  assert.equal(result.status, 1);
  assert.match(result.stdout, /common:missing: missing/);
});

test("accepts a plural stem when the Chinese other form exists", (t) => {
  const result = check(
    t,
    { common: { items_other: "{{count}} 项" } },
    { "page.tsx": `t("common:items", { count });` },
  );
  assert.equal(result.status, 0, result.stdout + result.stderr);
});

test("rejects empty text and broken interpolation", (t) => {
  const result = check(
    t,
    { common: { empty: "  ", broken: "你好 {{name" } },
    { "page.tsx": `t("common:empty");` },
  );
  assert.equal(result.status, 1);
  assert.match(result.stdout, /empty: empty text/);
  assert.match(result.stdout, /broken: interpolation mismatch/);
});

test("ignores dynamic keys and test files", (t) => {
  const result = check(
    t,
    { holdings: { cash: "现金" } },
    {
      "page.tsx": "t(`holdings:${kind}`);",
      "page.test.tsx": `t("holdings:missing");`,
    },
  );
  assert.equal(result.status, 0, result.stdout + result.stderr);
});
