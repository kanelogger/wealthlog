import type { Resource } from "./types";
export interface CorrectionLine {
  purposeId: string;
  resourceKey: string;
  delta: string;
}

export function resourceIdentity(resource: Resource) {
  return JSON.stringify(
    resource.kind === "CASH"
      ? [resource.kind, resource.accountId, resource.currency]
      : [resource.kind, resource.accountId, resource.assetId],
  );
}

export const blankLine = { purposeId: "", resourceKey: "", delta: "" };
