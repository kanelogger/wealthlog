import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  backupDatabase,
  deleteDatabaseBackup,
  isTauri,
  listDatabaseBackups,
  openDatabaseBackupFolder,
  type DatabaseBackup,
} from "@/adapters";
import { QueryKeys } from "@/lib/query-keys";
import { Button, Icons, useDateFormatting, useNumberFormatting } from "@wealthfolio/ui";
import { toast } from "@wealthfolio/ui/components/ui/use-toast";
import { BackupExportDialog } from "./backup-export-dialog";
import { formatBackupSize } from "./backup-size";
import {
  BackupAlert,
  BackupCardHeader,
  BackupDeleteDialog,
  BackupEmpty,
  BackupRow,
  BackupRowsSkeleton,
  PendingIcon,
} from "./backup-ui";

export function LocalBackups() {
  const { t } = useTranslation();
  const dates = useDateFormatting();
  const { formatDecimal } = useNumberFormatting();
  const queryClient = useQueryClient();
  const [exporting, setExporting] = useState<DatabaseBackup | null>(null);
  const [deleting, setDeleting] = useState<DatabaseBackup | null>(null);
  const [folderError, setFolderError] = useState<unknown>(null);
  const backups = useQuery({
    queryKey: [QueryKeys.DATABASE_BACKUPS],
    queryFn: listDatabaseBackups,
  });
  const createBackup = useMutation({
    mutationFn: () => backupDatabase(),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: [QueryKeys.DATABASE_BACKUPS] });
      toast({ title: t("settings:backup_saved"), variant: "success" });
    },
  });
  const deleteBackup = useMutation({
    mutationFn: (filename: string) => deleteDatabaseBackup(filename),
    onSuccess: async () => {
      setDeleting(null);
      await queryClient.invalidateQueries({ queryKey: [QueryKeys.DATABASE_BACKUPS] });
    },
  });
  const busy = createBackup.isPending || deleteBackup.isPending;

  return (
    <section className="space-y-4 rounded-lg border p-4" aria-label={t("settings:backup_title")}>
      <BackupCardHeader
        icon={<Icons.Database />}
        title={t("settings:backup_title")}
        subtitle={
          <p className="text-muted-foreground text-sm">{t("settings:backup_portable_help")}</p>
        }
        action={
          <Button disabled={busy} onClick={() => createBackup.mutate()}>
            <PendingIcon
              pending={createBackup.isPending}
              icon={<Icons.Database className="size-4" />}
            />
            {t(createBackup.isPending ? "settings:backup_creating_short" : "settings:backup_now")}
          </Button>
        }
        menu={
          <div className="flex items-center gap-1">
            {isTauri && (
              <Button
                variant="ghost"
                size="icon"
                aria-label={t("settings:backup_open_folder")}
                onClick={() => {
                  setFolderError(null);
                  void openDatabaseBackupFolder().catch(setFolderError);
                }}
              >
                <Icons.FolderOpen className="size-4" />
              </Button>
            )}
            <Button
              variant="ghost"
              size="icon"
              disabled={busy || backups.isFetching}
              aria-label={t("common:refresh")}
              onClick={() => {
                void backups.refetch();
              }}
            >
              <Icons.RefreshCw className="size-4" />
            </Button>
          </div>
        }
      />
      {createBackup.error && <BackupAlert error={{ cause: createBackup.error }} />}
      {folderError != null && <BackupAlert error={{ cause: folderError }} />}
      {backups.error && <BackupAlert error={{ cause: backups.error }} />}
      {backups.isPending ? (
        <BackupRowsSkeleton />
      ) : backups.data?.length ? (
        <ul className="divide-y" aria-label={t("settings:backup_saved_heading")}>
          {backups.data.map((backup) => (
            <BackupRow
              key={backup.filename}
              title={backup.filename}
              meta={[
                dates.formatDateTime(backup.modifiedAt),
                formatBackupSize(backup.sizeBytes, formatDecimal),
              ]}
              details={[t(`settings:backup_protection_${backup.protection}`)]}
              actions={
                <>
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={busy || backup.protection === "unavailable"}
                    aria-label={t("settings:backup_export_item", { filename: backup.filename })}
                    onClick={() => setExporting(backup)}
                  >
                    <Icons.Download className="size-4" />
                    {t("common:export")}
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    disabled={busy}
                    aria-label={`${t("common:delete")} ${backup.filename}`}
                    onClick={() => {
                      deleteBackup.reset();
                      setDeleting(backup);
                    }}
                  >
                    <Icons.Trash2 className="size-4" />
                  </Button>
                </>
              }
            />
          ))}
        </ul>
      ) : (
        <BackupEmpty
          title={t("settings:backup_empty_title")}
          description={t("settings:backup_empty_description")}
        />
      )}
      {exporting && (
        <BackupExportDialog filename={exporting.filename} onClose={() => setExporting(null)} />
      )}
      {deleting && (
        <BackupDeleteDialog
          open
          title={t("settings:backup_delete_title")}
          description={t("settings:backup_delete_snapshot", { filename: deleting.filename })}
          pending={deleteBackup.isPending}
          pendingLabel={t("settings:backup_deleting")}
          error={
            deleteBackup.error ? <BackupAlert error={{ cause: deleteBackup.error }} /> : undefined
          }
          onConfirm={() => deleteBackup.mutate(deleting.filename)}
          onCancel={() => setDeleting(null)}
        />
      )}
    </section>
  );
}
