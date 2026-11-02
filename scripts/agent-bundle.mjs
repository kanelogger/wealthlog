import { spawn } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { cp, lstat, mkdir, readFile, readdir, rm, stat, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { promisify } from "node:util";
import { execFile } from "node:child_process";
import { fileURLToPath } from "node:url";

const exec = promisify(execFile);
const root = fileURLToPath(new URL("../", import.meta.url));
const required = [
  "SKILL.md",
  "VERSION",
  "references/connection.md",
  "references/operations.md",
  "references/portfolio.md",
  "scripts/wealthlog_date.py",
  "scripts/wealthlog_snapshot.py",
  "scripts/wealthlog_portfolio.py",
];
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

export function skillDigest(files) {
  return sha256(
    Object.keys(files)
      .sort()
      .map((name) => `${name}\0${files[name]}\n`)
      .join(""),
  );
}

export async function fileDigests(directory, prefix = "") {
  const result = {};
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const name = prefix + entry.name;
    if (entry.isDirectory()) {
      Object.assign(result, await fileDigests(join(directory, entry.name), name + "/"));
    } else if (entry.isFile()) {
      result[name] = sha256(await readFile(join(directory, entry.name)));
    } else {
      throw new Error(`Unsupported resource object: ${name}`);
    }
  }
  return Object.fromEntries(
    Object.entries(result).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)),
  );
}

export function nativeTarget(arch = process.arch) {
  if (arch === "arm64") return "aarch64-apple-darwin";
  if (arch === "x64") return "x86_64-apple-darwin";
  throw new Error(`Unsupported native macOS architecture: ${arch}`);
}

export function buildTarget(args, arch = process.arch) {
  const i = args.indexOf("--target");
  const target =
    (i === -1 ? undefined : args[i + 1]) ??
    args.find((a) => a.startsWith("--target="))?.slice("--target=".length) ??
    nativeTarget(arch);
  if (target !== nativeTarget(arch)) {
    throw new Error("Agent bundles support only the native macOS architecture");
  }
  return target;
}

export async function checkCli(cli, version, arch, run = exec) {
  if (!(await lstat(cli)).isFile() || !((await stat(cli)).mode & 0o111)) {
    throw new Error("Bundled CLI must be an executable regular file");
  }
  const { stdout: cliVersion } = await run(cli, ["--version"]);
  if (cliVersion.trim() !== `wealthlog ${version}`) {
    throw new Error(`CLI version differs from Tauri version ${version}`);
  }
  const { stdout: architectures } = await run("lipo", ["-archs", cli]);
  if (architectures.trim() !== arch) {
    throw new Error(`CLI architecture differs from ${arch}`);
  }
}

export async function generateBundle({ cli, skillSource, destination, version, arch, run = exec }) {
  await checkCli(cli, version, arch, run);
  // A fresh staging directory prevents removed skill files leaking into builds.
  const staging = join(destination, `payload-${randomUUID()}`);
  const cleanup = () => rm(staging, { recursive: true, force: true });
  try {
    const skill = join(staging, "skills/wealthlog");
    await mkdir(staging, { recursive: true });
    await cp(skillSource, skill, { recursive: true, dereference: false });
    await writeFile(join(skill, "VERSION"), `${version}\n`);
    const files = await fileDigests(skill);
    for (const file of required) {
      if (!files[file]) throw new Error(`Missing skill resource: ${file}`);
    }
    const manifest = {
      formatVersion: 1,
      version,
      arch,
      cliSha256: sha256(await readFile(cli)),
      skillSha256: skillDigest(files),
      skillFiles: files,
    };
    const manifestPath = join(staging, "agent-bundle.json");
    await writeFile(manifestPath, JSON.stringify(manifest, null, 2) + "\n");
    const config = {
      bundle: {
        resources: {
          [skill]: "skills/wealthlog",
          [manifestPath]: "agent-bundle.json",
        },
        macOS: { files: { "Helpers/wealthlog": cli } },
      },
    };
    const configPath = join(staging, "tauri.agent-bundle.json");
    await writeFile(configPath, JSON.stringify(config, null, 2) + "\n");
    return { configPath, manifest, skill, cli, cleanup };
  } catch (error) {
    await cleanup();
    throw error;
  }
}

async function runCargo(args) {
  await new Promise((done, fail) => {
    const child = spawn("cargo", args, { cwd: root, stdio: "inherit" });
    child.on("error", fail);
    child.on("exit", (code) => (code === 0 ? done() : fail(new Error(`CLI build exited ${code}`))));
  });
}

export function cliBuildArgs(args) {
  return [
    "build",
    "-p",
    "wealthlog-cli",
    "--release",
    "--locked",
    "--target-dir",
    "target/cli",
    ...(args.includes("--offline") ? ["--offline"] : []),
  ];
}

export async function prepareAgentBundle(args) {
  const target = buildTarget(args);
  const config = JSON.parse(await readFile(join(root, "apps/tauri/tauri.conf.json"), "utf8"));
  await runCargo(cliBuildArgs(args));
  const bundle = await generateBundle({
    cli: join(root, "target/cli/release/wealthlog"),
    skillSource: join(root, "skills/wealthlog"),
    destination: join(root, "target/agent-bundle"),
    version: config.version,
    arch: process.arch === "arm64" ? "arm64" : "x86_64",
  });
  const profileIndex = args.indexOf("--profile");
  const profile =
    (profileIndex === -1 ? undefined : args[profileIndex + 1]) ??
    args.find((a) => a.startsWith("--profile="))?.slice(10) ??
    (args.includes("--debug") ? "debug" : "release");
  const explicitTarget = args.some((a) => a === "--target" || a.startsWith("--target="));
  const targetDir = resolve(process.env.CARGO_TARGET_DIR ?? join(root, "target"));
  return {
    ...bundle,
    app: join(
      targetDir,
      ...(explicitTarget ? [target] : []),
      profile,
      "bundle/macos/Wealthlog.app",
    ),
  };
}

export async function verifyAppBundle(app, expected, run = exec) {
  const contents = join(app, "Contents");
  const { stdout: appVersion } = await run("plutil", [
    "-extract",
    "CFBundleShortVersionString",
    "raw",
    "-o",
    "-",
    join(contents, "Info.plist"),
  ]);
  if (appVersion.trim() !== expected.version) {
    throw new Error("Packaged application version differs from the CLI");
  }
  const cli = join(contents, "Helpers/wealthlog");
  const manifest = JSON.parse(
    await readFile(join(contents, "Resources/agent-bundle.json"), "utf8"),
  );
  if (JSON.stringify(manifest) !== JSON.stringify(expected)) {
    throw new Error("Packaged manifest differs from build manifest");
  }
  await checkCli(cli, expected.version, expected.arch, run);
  if (sha256(await readFile(cli)) !== manifest.cliSha256) {
    throw new Error("Packaged CLI digest mismatch");
  }
  const files = await fileDigests(join(contents, "Resources/skills/wealthlog"));
  if (
    JSON.stringify(files) !== JSON.stringify(manifest.skillFiles) ||
    skillDigest(files) !== manifest.skillSha256 ||
    (await readFile(join(contents, "Resources/skills/wealthlog/VERSION"), "utf8")).trim() !==
      expected.version
  ) {
    throw new Error("Packaged skill contents or version differ");
  }
}
