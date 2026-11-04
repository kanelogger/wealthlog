import type { ContextCapture, DecisionBody, Origin } from "./types";
import type { Execution, ExecutionSource } from "./execution-work-types";

export interface ReviewEvidence {
  informationKind: "FACT" | "HYPOTHESIS" | "UNKNOWN";
  content: string;
  source: string | null;
}
export interface PrincipleAssessment {
  principleRevisionId: string | null;
  disposition: "KEEP" | "REVISE" | "RETIRE" | "NEW" | "UNDECIDED";
  newPrincipleRevisionId: string | null;
  reason: string | null;
}
export interface ReviewBody {
  schemaVersion: 1;
  judgment: string;
  executionAssessment: string;
  deviationReason: string | null;
  outcomeEvidence: ReviewEvidence[];
  otherExplanations: string | null;
  openQuestions: string | null;
  scenarioTags: string[];
  principles: PrincipleAssessment[];
  contentOrigin: Origin | "UNSPECIFIED";
}
export interface ReviewFacts {
  query: { decisionRevisionId: string; observationCutoff: string; observationTimezone: string };
  quality: "COMPLETE" | "PARTIAL" | "UNAVAILABLE";
  sourceVersion: string;
  issues: { code: string }[];
  subjectMarket: {
    quote: {
      price: string;
      currency: string;
      source: string;
      sourceTimestamp: string;
      stale: boolean;
    } | null;
  } | null;
  observations: {
    decision: {
      revisionNumber: number;
      body: DecisionBody;
      contextAtConfirmation: ContextCapture | null;
      confirmedAt: string;
    };
    executions: {
      observationScope: "IN_SCOPE" | "AFTER_CUTOFF" | "UNDETERMINED";
      eventDate: string | null;
      link: {
        id: string;
        sourceVersion: string;
        unlinkedAt: string | null;
        snapshotAtLink: { execution: Execution };
      };
      currentSource: ExecutionSource;
      sourceAcknowledged: boolean;
    }[];
    principles: {
      id?: string;
      revisionId?: string;
      name?: string;
      statusAtCapture?: string;
      body?: {
        name: string;
        content: string;
        applicableConditions: string | null;
        exceptions: string | null;
      };
      revisionNumber?: number;
    }[];
  };
}
export interface ReviewRecord {
  id: string;
  rootId: string;
  revisionNumber: number;
  status: "DRAFT" | "SUBMITTED" | "DISCARDED";
  observationCutoff: string;
  observationTimezone: string;
  version: number;
  body: ReviewBody;
  factsSnapshot: ReviewFacts | null;
  submittedAt: string | null;
  submittedActorKind: "GUI_SESSION" | "PROGRAM" | null;
}
export interface ReviewDraftTarget {
  reviewId: string;
  expectedVersion: number;
}
export interface ReviewSaveResult {
  reviewId: string;
  rootId: string;
  revisionNumber: number;
  version: number;
}
export interface ReviewPreview {
  ledgerId: string;
  target: ReviewDraftTarget;
  body: ReviewBody;
  facts: ReviewFacts;
  expectedContentHash: string;
  expectedFactsHash: string;
}
export interface ReviewPrinciple {
  principleId: string;
  name: string;
  currentRevisionId: string | null;
  currentRevisionNumber: number | null;
  draftRevisionId: string | null;
  draftName: string | null;
}
export function newReviewBody(): ReviewBody {
  return {
    schemaVersion: 1,
    judgment: "",
    executionAssessment: "",
    deviationReason: null,
    outcomeEvidence: [],
    otherExplanations: null,
    openQuestions: null,
    scenarioTags: [],
    principles: [],
    contentOrigin: "UNSPECIFIED",
  };
}
