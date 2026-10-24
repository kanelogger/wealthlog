import { fireEvent, render, screen, waitFor } from "@/test/render";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import settings from "@/i18n/locales/zh/settings.json";
import { LocalBackups } from "./local-backups";

const adapters = vi.hoisted(() => ({
  backupDatabase: vi.fn(),
  deleteDatabaseBackup: vi.fn(),
  listDatabaseBackups: vi.fn(),
  openDatabaseBackupFolder: vi.fn(),
  isTauri: true,
}));
vi.mock("@/adapters", () => ({
  backupDatabase: adapters.backupDatabase,
  deleteDatabaseBackup: adapters.deleteDatabaseBackup,
  isTauri: true,
  listDatabaseBackups: adapters.listDatabaseBackups,
  openDatabaseBackupFolder: adapters.openDatabaseBackupFolder,
}));
vi.mock("./backup-export-dialog", () => ({
  BackupExportDialog: ({ filename }: { filename: string }) => <p>Export {filename}</p>,
}));

function mount() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return {
    client,
    ...render(
      <QueryClientProvider client={client}>
        <LocalBackups />
      </QueryClientProvider>,
    ),
  };
}

describe("LocalBackups", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    adapters.listDatabaseBackups.mockResolvedValue([
      {
        filename: "portfolio-2026-10-13.db",
        sizeBytes: 2048,
        modifiedAt: "2026-10-13T10:00:00Z",
        protection: "encrypted",
        reason: "manual",
      },
    ]);
  });

  it("creates, exports and deletes database copies without exposing restore", async () => {
    adapters.backupDatabase.mockResolvedValue({ filename: "portfolio-2026-10-13.db" });
    adapters.deleteDatabaseBackup.mockResolvedValue(undefined);
    mount();

    expect(await screen.findByText("portfolio-2026-10-13.db")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /restore/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /import/i })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: settings.backup_now }));
    await waitFor(() => expect(adapters.backupDatabase).toHaveBeenCalledOnce());

    fireEvent.click(
      screen.getByRole("button", {
        name: settings.backup_export_item.replace("{{filename}}", "portfolio-2026-10-13.db"),
      }),
    );
    expect(screen.getByText("Export portfolio-2026-10-13.db")).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", {
        name: `${settings.common_delete} portfolio-2026-10-13.db`,
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: settings.common_delete }));
    await waitFor(() => expect(adapters.deleteDatabaseBackup).toHaveBeenCalledWith("portfolio-2026-10-13.db"));
  });
});
