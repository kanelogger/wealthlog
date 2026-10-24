import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { prepareAgentBundle, verifyAppBundle } from "./agent-bundle.mjs";

export function withDevelopmentConfig(args) {
  const command = args.find((arg) => arg !== "--verbose" && !/^-v+$/.test(arg));
  if (command !== "dev") return args;

  // Apply identity last so unrelated custom configs cannot restore the production
  // data directory. Keep the option before arguments forwarded to Cargo/the app.
  const separator = args.indexOf("--");
  const end = separator === -1 ? args.length : separator;
  return [
    ...args.slice(0, end),
    "--config",
    resolve("apps/tauri/tauri.dev.conf.json"),
    ...args.slice(end),
  ];
}

export async function runTauri(args) {
  const require = createRequire(import.meta.url);
  const command = args.find((arg) => arg !== "--verbose" && !/^-v+$/.test(arg));
  const packageAgentTools =
    process.platform === "darwin" &&
    command === "build" &&
    !args.includes("--help") &&
    !args.includes("-h") &&
    !args.includes("--no-bundle");
  const bundle = packageAgentTools ? await prepareAgentBundle(args) : null;
  let configured = withDevelopmentConfig(args);
  if (bundle) {
    const separator = configured.indexOf("--");
    const end = separator === -1 ? configured.length : separator;
    configured = [
      ...configured.slice(0, end),
      "--config",
      bundle.configPath,
      ...configured.slice(end),
    ];
  }
  const child = spawn(
    process.execPath,
    [require.resolve("@tauri-apps/cli/tauri.js"), ...configured],
    { env: process.env, stdio: "inherit" },
  );

  child.on("error", async (error) => {
    console.error(`Failed to start Tauri CLI: ${error.message}`);
    process.exitCode = 1;
    try {
      await bundle?.cleanup();
    } catch (cleanupError) {
      console.error(`Failed to clean agent bundle staging: ${cleanupError.message}`);
    }
  });

  child.on("exit", async (code, signal) => {
    try {
      if (code === 0 && bundle) {
        await verifyAppBundle(bundle.app, bundle.manifest);
        console.log("Agent bundle verified: Helpers CLI, version, architecture and skill digests");
      }
      process.exitCode = code ?? (signal ? 1 : 0);
    } catch (error) {
      console.error(`Agent bundle verification failed: ${error.message}`);
      process.exitCode = 1;
    } finally {
      if (bundle) {
        try {
          await bundle.cleanup();
        } catch (error) {
          console.error(`Failed to clean agent bundle staging: ${error.message}`);
          process.exitCode = 1;
        }
      }
    }
  });
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  runTauri(process.argv.slice(2)).catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
