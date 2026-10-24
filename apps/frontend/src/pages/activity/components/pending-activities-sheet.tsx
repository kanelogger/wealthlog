import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import {
  confirmPendingActivity,
  getAccounts,
  listPendingActivities,
  rejectPendingActivity,
  updatePendingActivity,
} from "@/adapters";
import { QueryKeys } from "@/lib/query-keys";
import type { Account, PendingActivity } from "@/lib/types";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Badge,
  Button,
  Icons,
  Input,
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
  Skeleton,
} from "@wealthfolio/ui";

interface PendingActivitiesSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCommitted?: () => void;
}

interface PendingDraft {
  amount?: string;
  accountId?: string;
  activityDate?: string;
  currency?: string;
  notes?: string;
}

/**
 * Review surface for pending income/expense entries (待确认收支).
 *
 * Entries may arrive incomplete from an Agent; the user fills the missing
 * fields here, then confirms or rejects. Confirming saves the edits and
 * creates exactly one posted activity, using the entry version for conflict
 * detection.
 */
export function PendingActivitiesSheet({
  open,
  onOpenChange,
  onCommitted,
}: PendingActivitiesSheetProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [busyId, setBusyId] = useState<string | null>(null);
  const [drafts, setDrafts] = useState<Record<string, PendingDraft>>({});
  /** Version bumped by a save, so a failed confirm can be retried directly. */
  const [versions, setVersions] = useState<Record<string, number>>({});

  const pendingQuery = useQuery({
    queryKey: [QueryKeys.PENDING_ACTIVITIES],
    queryFn: () => listPendingActivities(false),
    enabled: open,
  });
  const accountsQuery = useQuery({ queryKey: [QueryKeys.ACCOUNTS], queryFn: () => getAccounts() });
  const accounts: Account[] = accountsQuery.data ?? [];
  const spendingAccounts = useMemo(
    () =>
      accounts.filter(
        (account) =>
          (account.accountType === "CASH" || account.accountType === "CREDIT_CARD") &&
          !account.isArchived,
      ),
    [accounts],
  );
  const accountName = useMemo(() => {
    const map = new Map<string, string>();
    for (const account of accounts) map.set(account.id, account.name);
    return map;
  }, [accounts]);

  useEffect(() => {
    if (!open) {
      setDrafts({});
      setVersions({});
    }
  }, [open]);

  const refresh = async () => {
    await queryClient.invalidateQueries({ queryKey: [QueryKeys.PENDING_ACTIVITIES] });
    await queryClient.invalidateQueries({ queryKey: [QueryKeys.ACTIVITIES] });
    onCommitted?.();
  };

  const patchDraft = (id: string, patch: PendingDraft) => {
    setDrafts((prev) => ({ ...prev, [id]: { ...prev[id], ...patch } }));
  };

  const confirm = async (entry: PendingActivity) => {
    const draft = drafts[entry.id] ?? {};
    const accountId = draft.accountId ?? entry.accountId ?? "";
    const activityDate = draft.activityDate ?? entry.activityDate ?? "";
    const currency = draft.currency ?? entry.currency ?? "";
    const amount = draft.amount ?? entry.amount ?? "";
    if (!accountId || !activityDate || !currency || !amount) {
      toast.error(t("activity:pending.incomplete"));
      return;
    }
    setBusyId(entry.id);
    const version = versions[entry.id] ?? entry.version;
    try {
      // Save the reviewed fields first; the confirm then uses the returned
      // entry version so an intervening edit cannot be silently overwritten.
      const saved = await updatePendingActivity(entry.id, {
        expectedVersion: version,
        accountId,
        activityDate,
        currency,
        amount,
        notes: draft.notes ?? entry.notes ?? undefined,
      });
      setVersions((prev) => ({ ...prev, [entry.id]: saved.version }));
      await confirmPendingActivity(entry.id, saved.version);
      await refresh();
    } catch (error) {
      toast.error((error as Error).message ?? String(error));
      // A failed confirm may have left the cached row behind the server
      // version; re-read it and drop the local bump so the retry is fresh.
      setVersions((prev) => {
        const next = { ...prev };
        delete next[entry.id];
        return next;
      });
      await queryClient.invalidateQueries({ queryKey: [QueryKeys.PENDING_ACTIVITIES] });
    } finally {
      setBusyId(null);
    }
  };

  const reject = async (entry: PendingActivity) => {
    setBusyId(entry.id);
    const version = versions[entry.id] ?? entry.version;
    try {
      await rejectPendingActivity(entry.id, version);
      await refresh();
    } catch (error) {
      toast.error((error as Error).message ?? String(error));
    } finally {
      setBusyId(null);
    }
  };

  const entries = pendingQuery.data ?? [];

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent className="flex w-full flex-col sm:max-w-xl">
        <SheetHeader>
          <SheetTitle>{t("activity:pending.title")}</SheetTitle>
          <SheetDescription>{t("activity:pending.description")}</SheetDescription>
        </SheetHeader>

        <div className="flex-1 space-y-3 overflow-y-auto py-4">
          {pendingQuery.isLoading && (
            <div className="space-y-2">
              <Skeleton className="h-16 w-full" />
              <Skeleton className="h-16 w-full" />
            </div>
          )}
          {!pendingQuery.isLoading && entries.length === 0 && (
            <p className="text-muted-foreground text-sm">{t("activity:pending.empty")}</p>
          )}
          {entries.map((entry) => {
            const draft = drafts[entry.id] ?? {};
            const accountId = draft.accountId ?? entry.accountId ?? "";
            const activityDate = (draft.activityDate ?? entry.activityDate ?? "").slice(0, 10);
            const currency = draft.currency ?? entry.currency ?? "";
            const amount = draft.amount ?? entry.amount ?? "";
            const notes = draft.notes ?? entry.notes ?? "";
            const missing = !accountId || !activityDate || !currency || !amount;
            return (
              <div key={entry.id} className="space-y-2 rounded-md border p-3">
                <div className="flex items-center justify-between gap-2">
                  <div className="flex items-center gap-2">
                    <Badge variant={entry.activityType === "DEPOSIT" ? "success" : "secondary"}>
                      {t(`activity:types.${entry.activityType.toLowerCase()}`, {
                        defaultValue: entry.activityType,
                      })}
                    </Badge>
                    <span className="text-muted-foreground text-xs">
                      {activityDate || t("activity:pending.missing_date")}
                    </span>
                  </div>
                  <span className="text-muted-foreground text-xs">
                    {entry.confirmedBy
                      ? t("activity:pending.confirmed_by", { actor: entry.confirmedBy })
                      : t("activity:pending.created_by", { actor: entry.createdBy })}
                  </span>
                </div>

                <div className="grid gap-2 sm:grid-cols-2">
                  <label className="text-sm">
                    <span className="text-muted-foreground">{t("activity:pending.account")}</span>
                    <select
                      aria-label={t("activity:pending.account")}
                      className="border-input bg-background mt-1 h-8 w-full rounded-md border px-2 text-sm"
                      value={accountId}
                      onChange={(event) => patchDraft(entry.id, { accountId: event.target.value })}
                    >
                      <option value="">{t("activity:pending.missing")}</option>
                      {spendingAccounts.map((account) => (
                        <option key={account.id} value={account.id}>
                          {account.name} ({account.currency})
                        </option>
                      ))}
                    </select>
                  </label>
                  <label className="text-sm">
                    <span className="text-muted-foreground">{t("activity:pending.amount")}</span>
                    <Input
                      aria-label={t("activity:pending.amount")}
                      className="mt-1 h-8"
                      inputMode="decimal"
                      value={amount}
                      onChange={(event) => patchDraft(entry.id, { amount: event.target.value })}
                    />
                  </label>
                  <label className="text-sm">
                    <span className="text-muted-foreground">{t("activity:pending.date")}</span>
                    <Input
                      aria-label={t("activity:pending.date")}
                      className="mt-1 h-8"
                      type="date"
                      value={activityDate}
                      onChange={(event) =>
                        patchDraft(entry.id, { activityDate: event.target.value })
                      }
                    />
                  </label>
                  <label className="text-sm">
                    <span className="text-muted-foreground">{t("activity:pending.currency")}</span>
                    <Input
                      aria-label={t("activity:pending.currency")}
                      className="mt-1 h-8"
                      maxLength={3}
                      value={currency}
                      onChange={(event) =>
                        patchDraft(entry.id, { currency: event.target.value.toUpperCase() })
                      }
                    />
                  </label>
                </div>
                <label className="block text-sm">
                  <span className="text-muted-foreground">{t("activity:pending.notes")}</span>
                  <Input
                    aria-label={t("activity:pending.notes")}
                    className="mt-1 h-8"
                    value={notes}
                    onChange={(event) => patchDraft(entry.id, { notes: event.target.value })}
                  />
                </label>
                {entry.accountId && accountName.get(entry.accountId) && (
                  <p className="text-muted-foreground text-xs">
                    {t("activity:pending.source_account", {
                      account: accountName.get(entry.accountId),
                    })}
                  </p>
                )}
                {missing && (
                  <p className="text-warning text-xs">{t("activity:pending.incomplete")}</p>
                )}

                <div className="flex gap-2 pt-1">
                  <Button
                    size="sm"
                    disabled={busyId === entry.id || missing}
                    onClick={() => void confirm(entry)}
                  >
                    <Icons.Check className="mr-1 h-4 w-4" />
                    {t("activity:pending.confirm")}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={busyId === entry.id}
                    onClick={() => void reject(entry)}
                  >
                    <Icons.X className="mr-1 h-4 w-4" />
                    {t("activity:pending.reject")}
                  </Button>
                </div>
              </div>
            );
          })}
        </div>

        <SheetFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("common:close")}
          </Button>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
}
