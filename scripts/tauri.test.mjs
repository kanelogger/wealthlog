import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import { withDevelopmentConfig } from "./tauri.mjs";

const config = resolve("apps/tauri/tauri.dev.conf.json");

test("development identity is applied after custom configs, including release-mode dev", () => {
  for (const args of [
    ["dev"],
    ["-v", "dev"],
    ["--verbose", "dev"],
    ["-vv", "--verbose", "dev"],
    ["dev", "--release"],
    ["dev", "--config", '{"identifier":"com.teymz.wealthfolio"}'],
    ["dev", "--config=custom.json"],
    ["dev", "-c", "custom.json", "--config", "another.json"],
  ]) {
    assert.deepEqual(withDevelopmentConfig(args), [...args, "--config", config]);
  }
});

test("development config is not forwarded to Cargo or the application", () => {
  assert.deepEqual(withDevelopmentConfig(["dev", "--", "--", "--config", "app.json"]), [
    "dev",
    "--config",
    config,
    "--",
    "--",
    "--config",
    "app.json",
  ]);
});

test("build and help commands retain their explicitly selected identity", () => {
  for (const args of [
    ["build"],
    ["build", "--debug"],
    ["build", "--debug", "--config", config],
    ["--help"],
  ]) {
    assert.deepEqual(withDevelopmentConfig(args), args);
  }
});
