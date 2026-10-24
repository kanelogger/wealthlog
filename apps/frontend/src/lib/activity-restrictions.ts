import type { Account } from "@/lib/types";
import { ActivityType, ACTIVITY_TYPES, isLiabilityAccountType } from "@/lib/constants";

/**
 * Picker-friendly activity type that maps TRANSFER_IN/OUT to a single TRANSFER option.
 * Used by ActivityTypePicker component.
 */
const PICKER_TRANSFER_TYPE = "TRANSFER";

/**
 * All activity types in picker-compatible format.
 * Includes TRANSFER as UI alias for TRANSFER_IN/TRANSFER_OUT.
 */
const ALL_PICKER_TYPES: readonly string[] = [...ACTIVITY_TYPES, PICKER_TRANSFER_TYPE];

/**
 * Activity types allowed for manual HOLDINGS tracking mode accounts.
 * These are income/cash activities that don't affect positions directly,
 * and splits, which are facts about the asset: a split carries the account's
 * snapshot quantities across it.
 */
const HOLDINGS_MODE_ALLOWED_TYPES: readonly string[] = [
  ActivityType.DEPOSIT,
  ActivityType.WITHDRAWAL,
  ActivityType.FEE,
  ActivityType.TAX,
  ActivityType.INTEREST,
  ActivityType.DIVIDEND, // Income activity - doesn't change positions
  ActivityType.SPLIT,
];

/**
 * Returns the list of activity types allowed for manual entry on the given account.
 * Returns picker-compatible types (includes "TRANSFER" alias).
 *
 * - No account selected: all types
 * - TRANSACTIONS mode: all types
 * - HOLDINGS mode: income/cash activities and splits
 */
export function getAllowedActivityTypes(account: Account | undefined): readonly string[] {
  if (!account) {
    return ALL_PICKER_TYPES;
  }

  const { trackingMode } = account;

  if (isLiabilityAccountType(account.accountType)) {
    return [];
  }

  if (trackingMode === "HOLDINGS") {
    return HOLDINGS_MODE_ALLOWED_TYPES;
  }

  // TRANSACTIONS or NOT_SET - all types allowed
  return ALL_PICKER_TYPES;
}

/**
 * Returns true if the account supports the given activity type.
 */
export function accountSupportsActivityType(account: Account, activityType: string): boolean {
  return getAllowedActivityTypes(account).includes(activityType);
}

/**
 * Returns true if the account supports adding holdings directly.
 * Historical provider metadata does not restrict manual entry after sync removal.
 */
export function canAddHoldings(account: Account | undefined): boolean {
  if (!account) {
    return false;
  }
  if (isLiabilityAccountType(account.accountType)) {
    return false;
  }
  return account.trackingMode === "HOLDINGS";
}

export type ActivityRestrictionLevel = "none" | "limited" | "blocked";

/**
 * Returns the restriction level for an account's activity entry.
 */
export function getActivityRestrictionLevel(
  account: Account | undefined,
): ActivityRestrictionLevel {
  if (!account) {
    return "none";
  }

  if (isLiabilityAccountType(account.accountType)) {
    return "blocked";
  }

  if (account.trackingMode === "HOLDINGS") {
    return "limited";
  }

  return "none";
}

/**
 * Returns true if an account with the given restriction level supports the activity type.
 */
export function restrictionAllowsType(
  restrictionLevel: ActivityRestrictionLevel | undefined,
  activityType: string,
): boolean {
  if (!restrictionLevel || restrictionLevel === "none") return true;
  if (restrictionLevel === "blocked") return false;
  // "limited" — only HOLDINGS_MODE_ALLOWED_TYPES
  return HOLDINGS_MODE_ALLOWED_TYPES.includes(activityType);
}
