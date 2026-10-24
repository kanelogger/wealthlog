// Activity Commands
import type {
  Activity,
  ActivityBulkMutationRequest,
  ActivityBulkMutationResult,
  ActivityCreate,
  ActivityDetails,
  ActivitySearchResponse,
  ActivityUpdate,
  InternalTransferPairRequest,
  InternalTransferPairResponse,
  TransferMatchCandidate,
  TransferMatchCandidateRequest,
} from "@/lib/types";

import { invoke, logger } from "./platform";

interface ActivityFilters {
  accountIds?: string | string[];
  activityTypes?: string | string[];
  symbol?: string;
  needsReview?: boolean;
  dateFrom?: string; // YYYY-MM-DD format
  dateTo?: string; // YYYY-MM-DD format
  instrumentTypes?: string | string[];
  activityIds?: string | string[];
}

interface ActivitySort {
  id: string;
  desc?: boolean;
}

function normalizeStringArray(input?: string | string[]): string[] | undefined {
  if (!input) return undefined;
  if (Array.isArray(input)) {
    return input.length > 0 ? input : undefined;
  }
  return input.length > 0 ? [input] : undefined;
}

function serializeActivityMetadata<T extends ActivityCreate | ActivityUpdate>(activity: T): T {
  if (activity.metadata === undefined || typeof activity.metadata === "string") {
    return activity;
  }

  return {
    ...activity,
    metadata: JSON.stringify(activity.metadata),
  };
}

export const getActivities = async (accountId?: string): Promise<ActivityDetails[]> => {
  try {
    const response = await searchActivities(
      0,
      Number.MAX_SAFE_INTEGER,
      accountId ? { accountIds: [accountId] } : {},
      "",
      { id: "date", desc: true },
    );
    return response.data;
  } catch (err) {
    logger.error("Error fetching all activities.");
    throw err;
  }
};

export const searchActivities = async (
  page: number,
  pageSize: number,
  filters: ActivityFilters,
  searchKeyword: string,
  sort?: ActivitySort,
): Promise<ActivitySearchResponse> => {
  const accountIdFilter = normalizeStringArray(filters?.accountIds);
  const activityTypeFilter = normalizeStringArray(filters?.activityTypes);
  const instrumentTypeFilter = normalizeStringArray(filters?.instrumentTypes);
  const activityIdFilter = normalizeStringArray(filters?.activityIds);
  const assetIdKeywordRaw = filters?.symbol ?? searchKeyword;
  const assetIdKeyword = assetIdKeywordRaw?.trim() ? assetIdKeywordRaw.trim() : undefined;
  const sortOption = sort?.id
    ? { id: sort.id, desc: sort.desc ?? false }
    : { id: "date", desc: true };
  const needsReviewFilter = filters?.needsReview;
  const dateFrom = filters?.dateFrom;
  const dateTo = filters?.dateTo;

  try {
    return await invoke<ActivitySearchResponse>("search_activities", {
      page,
      pageSize,
      accountIdFilter,
      activityTypeFilter,
      assetIdKeyword,
      sort: sortOption,
      needsReviewFilter,
      dateFrom,
      dateTo,
      instrumentTypeFilter,
      activityIdFilter,
    });
  } catch (err) {
    logger.error("Error fetching activities.");
    throw err;
  }
};

export const createActivity = async (activity: ActivityCreate): Promise<Activity> => {
  try {
    return await invoke<Activity>("create_activity", {
      activity: serializeActivityMetadata(activity),
    });
  } catch (err) {
    logger.error("Error creating activity.");
    throw err;
  }
};

export const updateActivity = async (activity: ActivityUpdate): Promise<Activity> => {
  try {
    return await invoke<Activity>("update_activity", {
      activity: serializeActivityMetadata(activity),
    });
  } catch (err) {
    logger.error("Error updating activity.");
    throw err;
  }
};

export const saveActivities = async (
  request: ActivityBulkMutationRequest,
): Promise<ActivityBulkMutationResult> => {
  const payload: ActivityBulkMutationRequest = {
    creates: (request.creates ?? []).map(serializeActivityMetadata),
    updates: (request.updates ?? []).map(serializeActivityMetadata),
    deleteIds: request.deleteIds ?? [],
  };
  try {
    return await invoke<ActivityBulkMutationResult>("save_activities", { request: payload });
  } catch (err) {
    logger.error("Error saving activities.");
    throw err;
  }
};

export const deleteActivity = async (activityId: string): Promise<Activity> => {
  try {
    return await invoke<Activity>("delete_activity", { activityId });
  } catch (err) {
    logger.error("Error deleting activity.");
    throw err;
  }
};

export const getTransferPairForActivity = async (
  activityId: string,
): Promise<InternalTransferPairResponse | null> => {
  try {
    return await invoke<InternalTransferPairResponse | null>("get_transfer_pair_for_activity", {
      activityId,
    });
  } catch (err) {
    logger.error("Error fetching transfer pair.");
    throw err;
  }
};

export const findTransferMatchCandidates = async (
  request: TransferMatchCandidateRequest,
): Promise<TransferMatchCandidate[]> => {
  try {
    return await invoke<TransferMatchCandidate[]>("find_transfer_match_candidates", {
      request,
    });
  } catch (err) {
    logger.error("Error finding transfer match candidates.");
    throw err;
  }
};

export const saveInternalTransferPair = async (
  request: InternalTransferPairRequest,
): Promise<InternalTransferPairResponse> => {
  const payload: InternalTransferPairRequest = {
    ...request,
    activityDate:
      request.activityDate instanceof Date
        ? request.activityDate.toISOString()
        : request.activityDate,
  };
  try {
    return await invoke<InternalTransferPairResponse>("save_internal_transfer_pair", {
      request: payload,
    });
  } catch (err) {
    logger.error("Error saving internal transfer pair.");
    throw err;
  }
};

export const linkTransferActivities = async (
  activityAId: string,
  activityBId: string,
): Promise<[Activity, Activity]> => {
  try {
    return await invoke<[Activity, Activity]>("link_transfer_activities", {
      activityAId,
      activityBId,
    });
  } catch (err) {
    logger.error("Error linking transfer activities.");
    throw err;
  }
};

export const unlinkTransferActivities = async (
  activityAId: string,
  activityBId: string,
): Promise<[Activity, Activity]> => {
  try {
    return await invoke<[Activity, Activity]>("unlink_transfer_activities", {
      activityAId,
      activityBId,
    });
  } catch (err) {
    logger.error("Error unlinking transfer activities.");
    throw err;
  }
};

// ---------------------------------------------------------------------------
// Unified business interface: ordinary income/expense
// ---------------------------------------------------------------------------

export const queryOrdinaryActivities = async (query: {
  page?: number;
  pageSize?: number;
  accountIds?: string[];
  dateFrom?: string;
  dateTo?: string;
}): Promise<import("@/lib/types").OrdinaryActivityPage> => {
  try {
    return await invoke("query_ordinary_activities", { query });
  } catch (err) {
    logger.error("Error querying ordinary activities.");
    throw err;
  }
};

export const createOrdinaryActivity = async (
  input: import("@/lib/types").OrdinaryActivityCreate,
): Promise<import("@/lib/types").OrdinaryActivityResult> => {
  try {
    return await invoke("create_ordinary_activity", { input });
  } catch (err) {
    logger.error("Error creating ordinary activity.");
    throw err;
  }
};

export const updateOrdinaryActivity = async (
  input: import("@/lib/types").OrdinaryActivityUpdate,
): Promise<import("@/lib/types").OrdinaryActivityResult> => {
  try {
    return await invoke("update_ordinary_activity", { input });
  } catch (err) {
    logger.error("Error updating ordinary activity.");
    throw err;
  }
};
