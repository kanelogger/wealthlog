import type { SpendingSettings } from "@/features/spending/types";
import type {
  CategorizationRule,
  NewCategorizationRule,
  UpdateCategorizationRule,
} from "@/features/spending/types/rule";

import { invoke, logger } from "./platform";

export const isSpendingEnabled = async (): Promise<boolean> => {
  try {
    const settings = await invoke<SpendingSettings>("get_spending_settings");
    return settings.enabled;
  } catch (e) {
    logger.error("Error fetching spending settings.");
    throw e;
  }
};

export const listCategorizationRules = async (): Promise<CategorizationRule[]> => {
  try {
    return await invoke<CategorizationRule[]>("list_categorization_rules");
  } catch (e) {
    logger.error("Error listing categorization rules.");
    throw e;
  }
};

export const createCategorizationRule = async (
  rule: NewCategorizationRule,
): Promise<CategorizationRule> => {
  try {
    return await invoke<CategorizationRule>("create_categorization_rule", { rule });
  } catch (e) {
    logger.error("Error creating categorization rule.");
    throw e;
  }
};

export const updateCategorizationRule = async (
  id: string,
  patch: UpdateCategorizationRule,
): Promise<CategorizationRule> => {
  try {
    return await invoke<CategorizationRule>("update_categorization_rule", { id, patch });
  } catch (e) {
    logger.error("Error updating categorization rule.");
    throw e;
  }
};

export const deleteCategorizationRule = async (id: string): Promise<void> => {
  try {
    await invoke<void>("delete_categorization_rule", { id });
  } catch (e) {
    logger.error("Error deleting categorization rule.");
    throw e;
  }
};

export const rerunCategorizationRules = async (onlyUncategorized: boolean): Promise<number> => {
  try {
    return await invoke<number>("rerun_categorization_rules", { onlyUncategorized });
  } catch (e) {
    logger.error("Error re-running categorization rules.");
    throw e;
  }
};
