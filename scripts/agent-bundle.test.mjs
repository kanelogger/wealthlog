import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile, readFile, readdir, cp, chmod, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  buildTarget,
  cliBuildArgs,
  fileDigests,
  generateBundle,
  skillDigest,
  verifyAppBundle,
} from "./agent-bundle.mjs";

test("native target rejects universal and cross architecture builds", () => {
  assert.equal(buildTarget([], "arm64"), "aarch64-apple-darwin");
  assert.equal(buildTarget(["--target=x86_64-apple-darwin"], "x64"), "x86_64-apple-darwin");
  assert.throws(() => buildTarget(["--target", "universal-apple-darwin"], "arm64"));
  assert.throws(() => buildTarget(["--target", "x86_64-apple-darwin"], "arm64"));
});

test("CLI build inherits offline mode without affecting normal builds", () => {
  const args = ["build", "--", "--offline"];
  assert.ok(cliBuildArgs(args).includes("--offline"));
  assert.ok(!cliBuildArgs(["build"]).includes("--offline"));
  assert.ok(cliBuildArgs(args).includes("--locked"));
  assert.equal(cliBuildArgs(args).at(-2), "target/cli");
});

test("failed staging is cleaned without removing another build's directory", async () => {
  const dir = await mkdtemp(join(tmpdir(), "wealthlog-bundle-staging-"));
  try {
    const cli = join(dir, "wealthlog");
    await writeFile(cli, "cli", { mode: 0o755 });
    const source = join(dir, "incomplete");
    await mkdir(source);
    await writeFile(join(source, "SKILL.md"), "skill");
    const destination = join(dir, "build");
    await mkdir(join(destination, "payload-another-build"), { recursive: true });
    await assert.rejects(
      generateBundle({
        cli,
        skillSource: source,
        destination,
        version: "0.0.3",
        arch: "arm64",
        run: async (command) => ({ stdout: command === "lipo" ? "arm64" : "wealthlog 0.0.3" }),
      }),
      /Missing skill resource/,
    );
    assert.deepEqual(await readdir(destination), ["payload-another-build"]);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("bundle staging, Helpers mapping, version, architecture, integrity and read-only verification", async () => {
  const dir = await mkdtemp(join(tmpdir(), "wealthlog-bundle-"));
  try {
    const cli = join(dir, "wealthlog");
    await writeFile(cli, "cli", { mode: 0o755 });
    let version = "wealthlog 0.0.3";
    let arch = "arm64";
    const run = async (command) => ({
      stdout: command === "lipo" ? arch : command === "plutil" ? "0.0.3" : version,
    });
    const args = {
      cli,
      skillSource: new URL("../skills/wealthlog/", import.meta.url),
      destination: join(dir, "build"),
      version: "0.0.3",
      arch: "arm64",
      run,
    };
    const bundle = await generateBundle(args);
    const config = JSON.parse(await readFile(bundle.configPath, "utf8"));
    assert.equal(config.bundle.macOS.files["Helpers/wealthlog"], cli);
    assert.equal(config.bundle.resources[bundle.skill], "skills/wealthlog");
    assert.equal((await readFile(join(bundle.skill, "VERSION"), "utf8")).trim(), "0.0.3");
    assert.equal(skillDigest(bundle.manifest.skillFiles), bundle.manifest.skillSha256);
    const app = join(dir, "Wealthlog.app");
    const contents = join(app, "Contents");
    await mkdir(join(contents, "Helpers"), { recursive: true });
    await mkdir(join(contents, "Resources/skills"), { recursive: true });
    await cp(cli, join(contents, "Helpers/wealthlog"));
    await cp(bundle.skill, join(contents, "Resources/skills/wealthlog"), { recursive: true });
    const manifestFile = join(contents, "Resources/agent-bundle.json");
    await writeFile(manifestFile, JSON.stringify(bundle.manifest));
    const before = await fileDigests(contents);
    await verifyAppBundle(app, bundle.manifest, run);
    assert.deepEqual(await fileDigests(contents), before);
    await chmod(join(contents, "Helpers/wealthlog"), 0o644);
    await assert.rejects(verifyAppBundle(app, bundle.manifest, run), /executable/);
    await chmod(join(contents, "Helpers/wealthlog"), 0o755);
    version = "wealthlog 0.0.2";
    await assert.rejects(generateBundle(args), /version differs/);
    version = "wealthlog 0.0.3";
    arch = "x86_64";
    await assert.rejects(generateBundle(args), /architecture differs/);
    arch = "arm64";
    await writeFile(join(contents, "Resources/skills/wealthlog/SKILL.md"), "changed");
    await assert.rejects(verifyAppBundle(app, bundle.manifest, run), /contents or version differ/);
    await bundle.cleanup();
    await assert.rejects(readFile(bundle.configPath), { code: "ENOENT" });
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
