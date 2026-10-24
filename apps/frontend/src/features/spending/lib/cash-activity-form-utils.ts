import { isCreditCardAccountType } from "./constants";

interface ResolveCashActivitySubtypeInput {
  activityType: string;
  accountType?: string;
  existingActivityType?: string | null;
  existingSubtype?: string | null;
}

export function resolveCashActivitySubtype({
  activityType,
  accountType,
  existingActivityType,
  existingSubtype,
}: ResolveCashActivitySubtypeInput): string | null {
  if (activityType !== "CREDIT" || isCreditCardAccountType(accountType)) {
    return null;
  }

  if (existingActivityType === "CREDIT") {
    return existingSubtype ?? null;
  }

  return "REIMBURSEMENT";
}

export function cashActivityFlowMetadata(
  activityType: string,
  subtype?: string | null,
  existingMetadata?: Record<string, unknown>,
): Record<string, unknown> | undefined {
  const normalizedSubtype = subtype?.trim().toUpperCase();
  const supportsCreditBoundary = activityType === "CREDIT";

  const existingFlow = existingMetadata?.flow;
  const hasFlowObject =
    typeof existingFlow === "object" && existingFlow !== null && !Array.isArray(existingFlow);
  const flow = hasFlowObject ? { ...(existingFlow as Record<string, unknown>) } : {};
  const hasExplicitBoundary = typeof flow.is_external === "boolean";

  if (supportsCreditBoundary && hasExplicitBoundary) {
    return {
      ...existingMetadata,
      flow,
    };
  }

  if (activityType === "CREDIT" && normalizedSubtype === "REIMBURSEMENT") {
    return {
      ...existingMetadata,
      flow: { ...flow, is_external: true },
    };
  }

  if (!hasFlowObject || !("is_external" in flow)) {
    return undefined;
  }

  delete flow.is_external;
  const metadata = { ...existingMetadata };
  if (Object.keys(flow).length > 0) {
    metadata.flow = flow;
  } else {
    delete metadata.flow;
  }
  return metadata;
}
