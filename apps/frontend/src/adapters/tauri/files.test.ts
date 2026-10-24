import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { saveAppDataFileViaPicker } from "./files";
const mocks = vi.hoisted(() => ({
  open: vi.fn(),
  save: vi.fn(),
  openFile: vi.fn(),
  invoke: vi.fn(),
  start: vi.fn(),
  stop: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: mocks.open, save: mocks.save }));
vi.mock("@tauri-apps/plugin-fs", () => ({
  BaseDirectory: { AppData: "AppData" },
  open: mocks.openFile,
  startAccessingSecurityScopedResource: mocks.start,
  stopAccessingSecurityScopedResource: mocks.stop,
}));
vi.mock("./core", () => ({ invoke: mocks.invoke }));
const id = "e8a3d0a8-7b46-4d41-a455-2608d510b521";
const exported = `pending-exports/${id}/accounts.csv`;
beforeEach(() => {
  vi.resetAllMocks();
  mocks.save.mockResolvedValue("/picked/accounts.csv");
  mocks.start.mockResolvedValue(undefined);
  mocks.stop.mockResolvedValue(undefined);
  mocks.invoke.mockImplementation(async (_command, payload) =>
    payload.operation === "read"
      ? payload.offset === 0
        ? btoa(String.fromCharCode(1, 2, 3))
        : ""
      : undefined,
  );
});
afterEach(() => vi.unstubAllGlobals());
describe("profile file transfers", () => {
  it("reads private exports through admitted IPC and opens only the picked destination", async () => {
    const file = {
      write: vi.fn(async (data: Uint8Array) => data.length),
      close: vi.fn().mockResolvedValue(undefined),
    };
    mocks.openFile.mockResolvedValue(file);
    expect(await saveAppDataFileViaPicker(exported, "accounts.csv")).toBe(true);
    expect(mocks.openFile).toHaveBeenCalledTimes(1);
    expect(mocks.openFile).toHaveBeenCalledWith("/picked/accounts.csv", {
      write: true,
      create: true,
      truncate: true,
    });
    expect(mocks.invoke).toHaveBeenCalledWith("profile_transfer_file", {
      relativePath: exported,
      operation: "read",
      offset: 0,
    });
    expect(file.write).toHaveBeenCalledWith(new Uint8Array([1, 2, 3]));
    expect(file.close).toHaveBeenCalled();
    expect(mocks.invoke).toHaveBeenLastCalledWith("profile_transfer_file", {
      relativePath: `pending-exports/${id}`,
      operation: "remove",
    });
  });
  it("rejects paths outside pending exports before opening a picker", async () => {
    await expect(saveAppDataFileViaPicker("../private.db", "private.db")).rejects.toThrow(
      "Only pending export",
    );
    expect(mocks.save).not.toHaveBeenCalled();
    expect(mocks.openFile).not.toHaveBeenCalled();
  });
  it("cleans private staging when the save picker is cancelled", async () => {
    mocks.save.mockResolvedValue(null);
    expect(await saveAppDataFileViaPicker(exported, "accounts.csv")).toBe(false);
    expect(mocks.openFile).not.toHaveBeenCalled();
    expect(mocks.invoke).toHaveBeenCalledWith("profile_transfer_file", {
      relativePath: `pending-exports/${id}`,
      operation: "remove",
    });
  });
});
