// Settings Commands
import type { Settings } from "@/lib/types";
import type { AppInfo, PlatformInfo } from "../types";

import { invoke, logger, tauriInvoke } from "./core";
import { removeAppDataPath, saveAppDataFileViaPicker } from "./files";

export const getSettings = async (): Promise<Settings> => {
  try {
    return await invoke<Settings>("get_settings");
  } catch (err) {
    logger.error("Error fetching settings.");
    throw err;
  }
};

export const updateSettings = async (settingsUpdate: Partial<Settings>): Promise<Settings> => {
  try {
    return await invoke<Settings>("update_settings", { settingsUpdate });
  } catch (error) {
    logger.error("Error updating settings.");
    throw error;
  }
};

export const backupDatabase = async (): Promise<{ filename: string }> => {
  try {
    const filename = await invoke<string>("backup_database");
    return { filename };
  } catch (error) {
    logger.error("Error backing up database.");
    throw error;
  }
};

export interface DatabaseBackup {
  filename: string;
  sizeBytes: number;
  modifiedAt: string;
  protection: "encrypted" | "unencrypted" | "unavailable";
  reason: "manual" | "before-restore" | "before-maintenance" | "before-migration" | "legacy";
}

export const listDatabaseBackups = (): Promise<DatabaseBackup[]> =>
  invoke<DatabaseBackup[]>("list_database_backups");

export const deleteDatabaseBackup = (filename: string): Promise<void> =>
  invoke<void>("delete_database_backup", { filename });

export const exportDatabaseBackup = async (
  filename: string,
  password: string | null,
  unencrypted: boolean,
  signal?: AbortSignal,
): Promise<boolean> => {
  if (signal?.aborted) return false;
  const output = await tauriInvoke<PendingExport>("export_database_backup", {
    filename,
    password,
    unencrypted,
  });
  if (signal?.aborted) {
    await removeAppDataPath(output.relativePath.slice(0, output.relativePath.lastIndexOf("/")));
    return false;
  }
  return saveAppDataFileViaPicker(output.relativePath, output.filename);
};

export const getDatabaseBackupDownloadUrl = (_filename: string): string => {
  throw new Error("Server backup downloads are only supported in web mode");
};

export interface PendingExport {
  relativePath: string;
  filename: string;
}

export interface DatabaseEncryptionStatus {
  /** Whether the database file is encrypted right now. */
  enabled: boolean;
  /** Whether this platform can toggle encryption at all. */
  supported: boolean;
}

export const getDatabaseEncryptionStatus = async (): Promise<DatabaseEncryptionStatus> => {
  try {
    return await invoke<DatabaseEncryptionStatus>("get_database_encryption_status");
  } catch (error) {
    logger.error("Error reading database encryption status.");
    throw error;
  }
};

/**
 * Converts the database between encrypted and plaintext.
 *
 * On desktop the app restarts as soon as the new database is verified, so this
 * promise never resolves on success — callers should show a restarting state
 * rather than waiting for it.
 */
export const setDatabaseEncryptionEnabled = async (enabled: boolean): Promise<void> => {
  try {
    await tauriInvoke<void>("set_database_encryption_enabled", { enabled });
  } catch (error) {
    logger.error("Error changing database encryption.");
    throw error;
  }
};

// ============================================================================
// App Commands
// ============================================================================

export const getAppInfo = async (): Promise<AppInfo> => {
  try {
    return await invoke<AppInfo>("get_app_info");
  } catch (err) {
    logger.error("Error fetching app info");
    throw err;
  }
};

// ============================================================================
// Platform Commands
// ============================================================================

export const getPlatform = async (): Promise<PlatformInfo> => {
  return invoke<PlatformInfo>("get_platform");
};

export interface DatabaseStartupStatus {
  generation: string | null;
  ready: boolean;
  maintenance: boolean;
  error: string | null;
}

export const getDatabaseStartupStatus = (): Promise<DatabaseStartupStatus> =>
  invoke("get_database_startup_status");

export const retryDatabaseStartup = (): Promise<void> => tauriInvoke("retry_database_startup");

export const openDatabaseBackupFolder = (): Promise<void> => invoke("open_database_backup_folder");
