import { spawn } from "node:child_process";
import { once } from "node:events";
import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { setTimeout } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { prepE2eEnv } from "./prep-e2e.mjs";

const DEV_SERVER_URL = process.env.WF_E2E_BASE_URL || "http://localhost:1420";
const REPO_ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const QUOTE_FIXTURE_DIR = join(REPO_ROOT, "e2e", "fixtures", "quotes");
const DEFAULT_RUST_LOG = ["error", "wealthfolio_core::quotes::sync=warn"].join(",");
const cliArgs = process.argv.slice(2);
const shouldUseUi = cliArgs.includes("--ui");

const buildHealthUrl = (base, path = "/") =>
  new URL(path, `${base.replace(/\/$/, "")}/`).toString();

const readBackendUrl = async () => {
  if (process.env.WF_E2E_BACKEND_URL) {
    return process.env.WF_E2E_BACKEND_URL;
  }

  const content = await readFile(join(REPO_ROOT, ".env.web"), "utf8");
  const listenAddress = content.match(/^WF_LISTEN_ADDR=(.+)$/m)?.[1]?.trim();
  if (!listenAddress) {
    throw new Error("WF_LISTEN_ADDR is required in .env.web for E2E readiness checks");
  }

  return listenAddress.includes("://") ? listenAddress : `http://${listenAddress}`;
};

const waitForServer = async (url, serverProcess, { timeout = 60_000, interval = 500 } = {}) => {
  const deadline = Date.now() + timeout;
  const healthUrl = buildHealthUrl(url);

  while (Date.now() < deadline) {
    if (serverProcess.exitCode !== null) {
      throw new Error(`Dev server exited prematurely with code ${serverProcess.exitCode}`);
    }

    try {
      const response = await fetch(healthUrl, { method: "GET" });
      if (response.ok) {
        return;
      }
    } catch (error) {
      // continue until service responds
    }

    await setTimeout(interval);
  }

  throw new Error(`Timed out waiting for ${url}`);
};

const spawnCommand = (command, args, extraEnv = {}, options = {}) =>
  spawn(command, args, {
    stdio: "inherit",
    env: { ...process.env, ...extraEnv },
    ...options,
  });

const devServerPort = (() => {
  try {
    return new URL(DEV_SERVER_URL).port || "1420";
  } catch {
    return "1420";
  }
})();

const runPlaywrightTests = (extraArgs = []) =>
  new Promise((resolve, reject) => {
    const tests = spawnCommand("pnpm", ["exec", "playwright", "test", ...extraArgs]);
    tests.once("error", reject);
    tests.once("exit", (code) => {
      if (code === 0) {
        resolve();
      } else {
        reject(new Error(`Playwright exited with code ${code}`));
      }
    });
  });

const run = async () => {
  await prepE2eEnv();
  const backendUrl = await readBackendUrl();

  // A dedicated process group lets cleanup stop pnpm wrappers and every child server.
  const grouped = process.platform !== "win32";
  const devServer = spawnCommand(
    "pnpm",
    ["run", "dev:web"],
    {
      WEALTHFOLIO_E2E: "1",
      WEALTHFOLIO_FIXTURE_DIR: QUOTE_FIXTURE_DIR,
      WEALTHFOLIO_FIXTURE_AS_OF: process.env.WEALTHFOLIO_FIXTURE_AS_OF || "2026-05-12",
      RUST_LOG: process.env.WF_E2E_RUST_LOG || DEFAULT_RUST_LOG,
      VITE_DEV_PORT: devServerPort,
    },
    { detached: grouped },
  );

  const cleanup = async () => {
    if (devServer.exitCode === null && !devServer.killed) {
      const exited = once(devServer, "exit");
      if (grouped) {
        process.kill(-devServer.pid, "SIGTERM");
      } else {
        devServer.kill("SIGTERM");
      }
      await exited;
    }
  };

  const handleSignal = () => {
    cleanup().catch(() => {});
  };

  process.once("SIGINT", handleSignal);
  process.once("SIGTERM", handleSignal);
  process.once("exit", handleSignal);

  try {
    // Wait for both frontend and backend to be ready
    console.log("Waiting for frontend server...");
    await waitForServer(DEV_SERVER_URL, devServer);
    console.log("Frontend ready. Waiting for backend server...");
    await waitForServer(backendUrl, devServer, { timeout: 120_000, interval: 1000 });
    console.log("Backend ready. Starting tests...");
    await runPlaywrightTests(cliArgs);
  } finally {
    await cleanup();
  }
};

run().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
