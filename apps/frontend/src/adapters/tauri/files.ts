// File Dialogs
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  type FileHandle,
  open as openFile,
  startAccessingSecurityScopedResource,
  stopAccessingSecurityScopedResource,
} from "@tauri-apps/plugin-fs";

import { invoke } from "./core";

const fileExtension = (fileName: string): string | null => {
  const extension = fileName.split(".").pop();
  return extension && extension !== fileName ? extension : null;
};

const toBase64 = (bytes: Uint8Array): string => {
  let binary = "";
  const chunkSize = 0x8000;

  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    const chunk = bytes.subarray(offset, offset + chunkSize);
    binary += String.fromCharCode(...chunk);
  }

  return btoa(binary);
};

const describeError = (error: unknown): string => {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === "string") {
    return error;
  }
  try {
    return JSON.stringify(error);
  } catch {
    return String(error);
  }
};

const writeAll = async (file: FileHandle, data: Uint8Array): Promise<void> => {
  let offset = 0;
  while (offset < data.byteLength) {
    const written = await file.write(data.subarray(offset));
    if (written <= 0) {
      throw new Error("write returned without writing data");
    }
    offset += written;
  }
};

const copyPendingExport = async (relativePath: string, toPath: string): Promise<void> => {
  const destination = await openFile(toPath, { write: true, create: true, truncate: true });
  try {
    let offset = 0;
    while (true) {
      const encoded = await invoke<string>("profile_transfer_file", {
        relativePath,
        operation: "read",
        offset,
      });
      const chunk = Uint8Array.from(atob(encoded), (c) => c.charCodeAt(0));
      if (chunk.length === 0) break;
      await writeAll(destination, chunk);
      offset += chunk.length;
    }
  } finally {
    await destination.close();
  }
};

export const openFolderDialog = async (): Promise<string | null> => {
  return open({ directory: true });
};

export const openFileSaveDialog = async (
  fileContent: string | Blob | Uint8Array,
  fileName: string,
): Promise<boolean> => {
  if (typeof fileContent === "string") {
    return invoke<boolean>("save_text_file_with_dialog", {
      fileName,
      content: fileContent,
    });
  }

  let contentToSave: Uint8Array;
  if (fileContent instanceof Blob) {
    const arrayBuffer = await fileContent.arrayBuffer();
    contentToSave = new Uint8Array(arrayBuffer);
  } else {
    contentToSave = fileContent;
  }

  const contentBase64 = toBase64(contentToSave);

  return invoke<boolean>("save_file_with_dialog", {
    fileName,
    contentBase64,
  });
};

export const saveAppDataFileViaPicker = async (
  relativePath: string,
  fileName: string,
): Promise<boolean> => {
  if (!/^pending-exports\/[^/\\]+\/[^/\\]+$/.test(relativePath)) {
    throw new Error("Only pending export files can be saved with the native file picker");
  }

  let filePath: string | null = null;
  const pendingDir = relativePath.slice(0, relativePath.lastIndexOf("/"));
  try {
    try {
      const extension = fileExtension(fileName);
      filePath = await save({
        defaultPath: fileName,
        filters: extension
          ? [
              {
                name: extension === "db" ? "SQLite Database" : `${extension.toUpperCase()} File`,
                extensions: [extension],
              },
            ]
          : undefined,
      });
    } catch (error) {
      throw new Error(`save picker failed: ${describeError(error)}`);
    }

    if (filePath === null) {
      return false;
    }

    let didStartScopedAccess = false;
    try {
      await startAccessingSecurityScopedResource(filePath);
      didStartScopedAccess = true;
      await copyPendingExport(relativePath, filePath);
    } catch (error) {
      throw new Error(
        `copyFile failed from ${relativePath} to ${filePath}: ${describeError(error)}`,
      );
    } finally {
      if (didStartScopedAccess) {
        await stopAccessingSecurityScopedResource(filePath).catch(() => undefined);
      }
    }
    return true;
  } finally {
    await removeAppDataPath(pendingDir);
  }
};

export const removeAppDataPath = async (relativePath: string): Promise<void> => {
  await invoke("profile_transfer_file", { relativePath, operation: "remove" }).catch(
    () => undefined,
  );
};

// ============================================================================
// Shell & Browser
// ============================================================================

export const openUrlInBrowser = async (url: string): Promise<void> => {
  await invoke("open_external_url", { url });
};
