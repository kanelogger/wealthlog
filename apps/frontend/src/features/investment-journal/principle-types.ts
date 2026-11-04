import type { ReviewBody, ReviewFacts } from "./review-types";
import type { Origin } from "./types";

export interface PrincipleBody {
  schemaVersion: 1;
  name: string;
  content: string;
  applicableConditions: string | null;
  exceptions: string | null;
  revisionReason: string | null;
  sourceReviewId: string | null;
  sourceReference: string | null;
  scenarioTags: string[];
  contentOrigin: Origin | "UNSPECIFIED";
}

export interface PrincipleTarget {
  principleId: string;
  draftRevisionId: string;
  expectedVersion: number;
}
export interface PrincipleSummary {
  principleId: string;
  name: string;
  draftName?: string | null;
  currentRevisionId: string | null;
  currentRevisionNumber?: number | null;
  draftRevisionId: string | null;
  isArchived: boolean;
  version: number;
}
export interface PrincipleSource {
  sourceReview: {
    id: string;
    revisionNumber: number;
    statusAtCapture: "DRAFT" | "SUBMITTED" | "DISCARDED";
    observationCutoff: string;
    observationTimezone: string;
    body: ReviewBody;
    factsSnapshot: ReviewFacts | null;
  } | null;
  sourceReference: string | null;
}
export interface PrincipleRevision {
  id: string;
  revisionNumber: number;
  status: "DRAFT" | "EFFECTIVE" | "DISCARDED";
  body: PrincipleBody;
  sourceContext: PrincipleSource | null;
  publishedAt: string | null;
  publishedActorKind: "GUI_SESSION" | "PROGRAM" | null;
}
export interface PrincipleDetail extends PrincipleSummary {
  revisions: PrincipleRevision[];
}
export interface PrinciplePreview {
  ledgerId: string;
  target: PrincipleTarget;
  revisionNumber: number;
  body: PrincipleBody;
  expectedContentHash: string;
  expectedSourceContextHash: string;
  sourceContext: PrincipleSource;
}
export interface PrincipleSaveResult {
  principleId: string;
  draftRevisionId: string;
  version: number;
  revisionNumber: number;
}
export function blankPrinciple(sourceReviewId?: string): PrincipleBody {
  return {
    schemaVersion: 1,
    name: "",
    content: "",
    applicableConditions: null,
    exceptions: null,
    revisionReason: null,
    sourceReviewId: sourceReviewId ?? null,
    sourceReference: null,
    scenarioTags: [],
    contentOrigin: "UNSPECIFIED",
  };
}
