import { beforeEach, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  cleanup: vi.fn(),
  save: vi.fn(),
}));
vi.mock("./core", () => ({
  invoke: mocks.invoke,
  tauriInvoke: mocks.invoke,
  logger: { error: vi.fn() },
}));
vi.mock("./files", () => ({
  removeAppDataPath: mocks.cleanup,
  saveAppDataFileViaPicker: mocks.save,
}));

import { exportDatabaseBackup } from "./settings";

beforeEach(() => {
  vi.resetAllMocks();
  mocks.invoke.mockResolvedValue(undefined);
});

it.each([true, false])("reports native picker completion accurately: %s", async (saved) => {
  mocks.invoke.mockResolvedValueOnce({
    relativePath: "pending-exports/test/backup.wfbackup",
    filename: "backup.wfbackup",
  });
  mocks.save.mockResolvedValueOnce(saved);
  const password = "  keep these spaces  ";
  expect(await exportDatabaseBackup("selected.db", password, false)).toBe(saved);
  expect(mocks.invoke).toHaveBeenCalledWith("export_database_backup", {
    filename: "selected.db",
    password,
    unencrypted: false,
  });
  expect(mocks.save).toHaveBeenCalledWith(
    "pending-exports/test/backup.wfbackup",
    "backup.wfbackup",
  );
});

it("cleans a cancelled native export without opening the save picker", async () => {
  let finish!: (output: unknown) => void;
  mocks.invoke.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const controller = new AbortController();
  const result = exportDatabaseBackup("selected.db", null, true, controller.signal);
  controller.abort();
  finish({ relativePath: "pending-exports/test/backup.db", filename: "backup.db" });
  expect(await result).toBe(false);
  expect(mocks.cleanup).toHaveBeenCalledWith("pending-exports/test");
  expect(mocks.save).not.toHaveBeenCalled();
});
