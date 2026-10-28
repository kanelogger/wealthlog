import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { localizeEventSummary, getEventTypeLabel } from "../lib/localized-labels";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { QueryKeys } from "@/lib/query-keys";

import { invalidateSpendingCaches } from "../lib/invalidation";
import {
  createEvent,
  createEventType,
  deleteEvent,
  deleteEventType,
  getEventSpendingSummaries,
  listEventTypes,
  listEvents,
  updateEvent,
  updateEventType,
} from "../adapters/events";
import type {
  EventSpendingSummary,
  EventType,
  NewEventType,
  NewSpendingEvent,
  SpendingEvent,
  UpdateSpendingEvent,
} from "../types/event";

// Event types and the full events list rarely change — keep them around so
// multi-page navigation doesn't refetch unnecessarily.
const STALE_TIME = 60_000;

export function useEventTypes() {
  const { t } = useTranslation();
  const select = useCallback(
    (data: EventType[]) => data.map((row) => ({ ...row, name: getEventTypeLabel(row, t) })),
    [t],
  );
  return useQuery<EventType[], Error>({
    queryKey: [QueryKeys.SPENDING_EVENT_TYPES],
    queryFn: listEventTypes,
    select,
    staleTime: STALE_TIME,
  });
}

export function useSpendingEvents() {
  return useQuery<SpendingEvent[], Error>({
    queryKey: [QueryKeys.SPENDING_EVENTS],
    queryFn: listEvents,
    staleTime: STALE_TIME,
  });
}

export function useEventSpendingSummaries(request: { startDate: string; endDate: string }) {
  const { t } = useTranslation();
  const select = useCallback(
    (data: EventSpendingSummary[]) => data.map((row) => localizeEventSummary(row, t)),
    [t],
  );
  return useQuery<EventSpendingSummary[], Error>({
    queryKey: [QueryKeys.SPENDING_EVENTS, "summaries", request],
    queryFn: () => getEventSpendingSummaries(request),
    select,
    staleTime: STALE_TIME,
  });
}

export function useEventTypeMutations() {
  const qc = useQueryClient();
  const { t } = useTranslation();
  const invalidate = () => {
    qc.invalidateQueries({ queryKey: [QueryKeys.SPENDING_EVENT_TYPES] });
    invalidateSpendingCaches(qc);
  };

  const create = useMutation({
    mutationFn: (t: NewEventType) => createEventType(t),
    onSuccess: () => {
      invalidate();
      toast.success("事件类型已创建。");
    },
    onError: () => toast.error("创建事件类型失败。"),
  });
  const update = useMutation({
    mutationFn: ({
      id,
      patch,
    }: {
      id: string;
      patch: { name?: string; color?: string | null };
    }) => {
      const raw = qc
        .getQueryData<EventType[]>([QueryKeys.SPENDING_EVENT_TYPES])
        ?.find((row) => row.id === id);
      const name = raw && patch.name === getEventTypeLabel(raw, t) ? raw.name : patch.name;
      return updateEventType(id, { ...patch, ...(patch.name !== undefined ? { name } : {}) });
    },
    onSuccess: () => {
      invalidate();
      toast.success("事件类型已更新。");
    },
    onError: () => toast.error("更新事件类型失败。"),
  });
  const remove = useMutation({
    mutationFn: (id: string) => deleteEventType(id),
    onSuccess: () => {
      invalidate();
      toast.success("事件类型已删除。");
    },
    onError: () => toast.error("删除事件类型失败。"),
  });
  return { create, update, remove };
}

export function useSpendingEventMutations() {
  const qc = useQueryClient();
  const invalidate = () => invalidateSpendingCaches(qc);

  const create = useMutation({
    mutationFn: (e: NewSpendingEvent) => createEvent(e),
    onSuccess: () => {
      invalidate();
      toast.success("事件已创建。");
    },
    onError: () => toast.error("创建事件失败。"),
  });
  const update = useMutation({
    mutationFn: ({ id, patch }: { id: string; patch: UpdateSpendingEvent }) =>
      updateEvent(id, patch),
    onSuccess: () => {
      invalidate();
      toast.success("事件已更新。");
    },
    onError: () => toast.error("更新事件失败。"),
  });
  const remove = useMutation({
    mutationFn: (id: string) => deleteEvent(id),
    onSuccess: () => {
      invalidate();
      toast.success("事件已删除。");
    },
    onError: () => toast.error("删除事件失败。"),
  });
  return { create, update, remove };
}
