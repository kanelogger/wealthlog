import { profileFetch, profileScope } from "@/features/profiles/session";
// Web adapter - Settings and App Info Commands

import { notifyUnauthorized } from "@/lib/auth-token";
import type { Settings } from "@/lib/types";
import type { AppInfo, PlatformInfo } from "../types";
import { API_PREFIX, invoke, logger } from "./core";

// ============================================================================
// Settings Commands
// ============================================================================

export const getSettings = async (): Promise<Settings> => {
  try {
    return await invoke<Settings>("get_settings");
  } catch (error) {
    logger.error("Error fetching settings.");
    throw error;
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

export interface DatabaseBackup {
  filename: string;
  sizeBytes: number;
  modifiedAt: string;
  protection: "encrypted" | "unencrypted" | "unavailable";
  reason: "manual" | "before-restore" | "before-maintenance" | "before-migration" | "legacy";
}

export const backupDatabase = async (): Promise<{ filename: string }> => {
  try {
    return await invoke<{ filename: string }>("backup_database");
  } catch (error) {
    logger.error("Error backing up database.");
    throw error;
  }
};

export const listDatabaseBackups = async (): Promise<DatabaseBackup[]> => {
  try {
    return await invoke<DatabaseBackup[]>("list_database_backups");
  } catch (error) {
    logger.error("Error listing database backups.");
    throw error;
  }
};

export const deleteDatabaseBackup = async (filename: string): Promise<void> => {
  try {
    await invoke<void>("delete_database_backup", { filename });
  } catch (error) {
    logger.error("Error deleting database backup.");
    throw error;
  }
};

export const getDatabaseBackupDownloadUrl = (filename: string): string =>
  `${API_PREFIX}/utilities/database/backups/${encodeURIComponent(filename)}/download?profileScope=${encodeURIComponent(profileScope())}`;

export const exportDatabaseBackup = async (
  filename: string,
  password: string | null,
  unencrypted: boolean,
  signal?: AbortSignal,
): Promise<boolean> => {
  if (
    !unencrypted &&
    window.location.protocol !== "https:" &&
    !["localhost", "127.0.0.1", "[::1]"].includes(window.location.hostname)
  ) {
    throw new Error("Use HTTPS to export a password-protected backup.");
  }
  const response = await profileFetch(
    `${API_PREFIX}/utilities/database/backups/${encodeURIComponent(filename)}/export`,
    {
      method: "POST",
      credentials: "same-origin",
      headers: { "Content-Type": "application/json", "X-Wealthfolio-Backup": "1" },
      body: JSON.stringify({ password, unencrypted }),
      signal,
    },
  );
  if (response.status === 401) {
    notifyUnauthorized();
  }
  const result = await response
    .json()
    .catch(() => ({ message: "Backup export timed out or failed" }));
  if (!response.ok) throw new Error(result.message || "Backup export failed");
  const link = document.createElement("a");
  link.href = `${API_PREFIX}/utilities/database/exports/${encodeURIComponent(result.id)}?profileScope=${encodeURIComponent(profileScope())}`;
  link.download = result.filename;
  document.body.append(link);
  link.click();
  link.remove();
  return true;
};

export interface DatabaseEncryptionStatus {
  enabled: boolean;
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

export const setDatabaseEncryptionEnabled = (_enabled: boolean): Promise<void> =>
  Promise.reject(
    new Error(
      "Server database encryption is converted with `wealthfolio-server db encrypt` " +
        "and required with WF_DB_REQUIRE_ENCRYPTION",
    ),
  );

// ============================================================================
// App Commands
// ============================================================================

export const getAppInfo = async (): Promise<AppInfo> => {
  try {
    return await invoke<AppInfo>("get_app_info");
  } catch (err) {
    logger.error("Error fetching app info");
    console.error(err);
    return {
      version: "",
      dbPath: "",
      logsDir: "",
    };
  }
};

export const getPlatform = (): Promise<PlatformInfo> => {
  // Web environment - detect from user agent
  const userAgent = typeof window !== "undefined" ? window.navigator.userAgent.toLowerCase() : "";
  const platform =
    typeof window !== "undefined" ? window.navigator.platform?.toLowerCase() || "" : "";

  // Check for mobile devices
  const isMobileUA = /android|webos|iphone|ipad|ipod|blackberry|iemobile|opera mini/i.test(
    userAgent,
  );
  const isTablet = /ipad|tablet|playbook|silk/i.test(userAgent);

  // Detect OS
  let os = "unknown";
  if (/iphone|ipad|ipod/.test(userAgent)) {
    os = "ios";
  } else if (userAgent.includes("android")) {
    os = "android";
  } else if (/mac|darwin/.test(platform) || userAgent.includes("macintosh")) {
    os = "macos";
  } else if (platform.includes("win") || userAgent.includes("windows")) {
    os = "windows";
  } else if (platform.includes("linux") || userAgent.includes("linux")) {
    os = "linux";
  }

  const is_mobile = isMobileUA || isTablet;

  return Promise.resolve({
    os,
    is_mobile,
    is_desktop: !is_mobile,
    is_tauri: false,
  });
};

export const openDatabaseBackupFolder = (): Promise<void> =>
  Promise.reject(new Error("Backup folders can only be opened on desktop"));
