// Pending entries (待确认收支) — shared by the desktop and web adapters.
import type {
  PendingActivity,
  PendingActivityCreate,
  PendingActivityUpdate,
  PendingConfirmResult,
} from "@/lib/types";

import { invoke, logger } from "./platform";

export const listPendingActivities = async (
  includeResolved = false,
): Promise<PendingActivity[]> => {
  try {
    return await invoke<PendingActivity[]>("list_pending_activities", { includeResolved });
  } catch (err) {
    logger.error("Error listing pending activities.");
    throw err;
  }
};

export const createPendingActivity = async (
  pending: PendingActivityCreate,
): Promise<PendingActivity> => {
  try {
    return await invoke<PendingActivity>("create_pending_activity", { pending });
  } catch (err) {
    logger.error("Error creating pending activity.");
    throw err;
  }
};

export const updatePendingActivity = async (
  id: string,
  pending: PendingActivityUpdate,
): Promise<PendingActivity> => {
  try {
    return await invoke<PendingActivity>("update_pending_activity", { id, pending });
  } catch (err) {
    logger.error("Error updating pending activity.");
    throw err;
  }
};

export const confirmPendingActivity = async (
  id: string,
  expectedVersion: number,
): Promise<PendingConfirmResult> => {
  try {
    return await invoke<PendingConfirmResult>("confirm_pending_activity", {
      args: { id, expectedVersion },
    });
  } catch (err) {
    logger.error("Error confirming pending activity.");
    throw err;
  }
};

export const rejectPendingActivity = async (
  id: string,
  expectedVersion: number,
): Promise<PendingActivity> => {
  try {
    return await invoke<PendingActivity>("reject_pending_activity", {
      args: { id, expectedVersion },
    });
  } catch (err) {
    logger.error("Error rejecting pending activity.");
    throw err;
  }
};
