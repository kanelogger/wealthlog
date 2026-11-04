import {
  Children,
  cloneElement,
  isValidElement,
  useId,
  type InputHTMLAttributes,
  type ReactNode,
} from "react";
import { Button, Icons } from "@wealthfolio/ui";
import { Input } from "@wealthfolio/ui/components/ui/input";
import { useBalancePrivacy } from "@/hooks/use-balance-privacy";
import { InvestmentJournalError } from "@/adapters";
import { useJournalLocale, type JournalTextKey } from "./locale-context";

export const selectClass =
  "border-input bg-background text-foreground focus-visible:ring-ring h-10 w-full min-w-0 rounded-md border px-3 text-sm outline-none focus-visible:ring-2 disabled:opacity-60";
export const textAreaClass =
  "border-input bg-background text-foreground placeholder:text-muted-foreground focus-visible:ring-ring min-h-24 w-full rounded-md border p-3 text-sm outline-none focus-visible:ring-2 disabled:opacity-60";

export function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: (id: string) => ReactNode;
}) {
  const id = useId();
  const hintId = `${id}-hint`;
  function describe(node: ReactNode): ReactNode {
    if (!isValidElement<{ id?: string; children?: ReactNode; "aria-describedby"?: string }>(node))
      return node;
    if (node.props.id === id)
      return cloneElement(node, {
        "aria-describedby": [node.props["aria-describedby"], hintId].filter(Boolean).join(" "),
      });
    return node.props.children
      ? cloneElement(node, {}, Children.map(node.props.children, describe))
      : node;
  }
  return (
    <div className="min-w-0 space-y-2">
      <label className="block text-sm font-medium" htmlFor={id}>
        {label}
      </label>
      {hint ? describe(children(id)) : children(id)}
      {hint && (
        <p id={hintId} className="text-muted-foreground max-w-prose text-sm leading-relaxed">
          {hint}
        </p>
      )}
    </div>
  );
}
export function ExactValue({
  value,
  currency,
  className = "",
}: {
  value: string | null | undefined;
  currency?: string;
  className?: string;
}) {
  const { isBalanceHidden } = useBalancePrivacy();
  const { t } = useJournalLocale();
  if (isBalanceHidden)
    return (
      <span className={className} aria-label={t("hidden")}>
        ••••
      </span>
    );
  if (value == null) return <span className={className}>{t("unavailable")}</span>;
  const [integer, fraction] = value.split(".");
  const shown = integer.replace(/\B(?=(\d{3})+(?!\d))/g, ",") + (fraction ? `.${fraction}` : "");
  return (
    <span className={`break-all tabular-nums ${className}`}>
      {shown}
      {currency ? (
        <span className="ml-1 inline-block whitespace-nowrap text-xs font-normal">{currency}</span>
      ) : null}
    </span>
  );
}
export function ExactInput(
  props: Omit<InputHTMLAttributes<HTMLInputElement>, "type" | "value" | "defaultValue"> & {
    value?: string;
  },
) {
  const { isBalanceHidden } = useBalancePrivacy();
  const { t } = useJournalLocale();
  return (
    <Input
      {...props}
      type="text"
      inputMode="decimal"
      value={isBalanceHidden ? "" : (props.value ?? "")}
      title={isBalanceHidden ? undefined : props.title}
      aria-valuenow={isBalanceHidden ? undefined : props["aria-valuenow"]}
      aria-valuetext={isBalanceHidden ? undefined : props["aria-valuetext"]}
      readOnly={isBalanceHidden || props.readOnly}
      placeholder={isBalanceHidden ? t("hidden") : props.placeholder}
      autoComplete="off"
    />
  );
}
export function JournalPrivacyButton() {
  const { isBalanceHidden, toggleBalanceVisibility } = useBalancePrivacy();
  const { t } = useJournalLocale();
  return (
    <Button
      variant="outline"
      size="icon"
      aria-label={t(isBalanceHidden ? "show_values" : "hide_values")}
      onClick={toggleBalanceVisibility}
    >
      {isBalanceHidden ? <Icons.Eye className="size-4" /> : <Icons.EyeOff className="size-4" />}
    </Button>
  );
}
export function JournalErrorNotice({
  error,
  replay,
  busy,
}: {
  error: unknown;
  replay?: () => void;
  busy?: boolean;
}) {
  const { t } = useJournalLocale();
  const code = error instanceof InvestmentJournalError ? error.code : "unknown_result";
  const keys: Record<string, JournalTextKey> = {
    invalid_input: "error_invalid",
    state_conflict: "error_conflict",
    request_conflict: "error_conflict",
    scope_denied: "error_permission",
    resource_unavailable: "error_unavailable",
    insufficient_resource: "error_invalid",
    negative_allocation: "error_invalid",
    already_applied: "error_already_applied",
  };
  return (
    <div role="alert" className="border-destructive/50 space-y-3 rounded-xl border p-4">
      <p className="font-medium">{t("error_title")}</p>
      <p className="text-sm leading-relaxed">{t(keys[code] ?? "error_unknown")}</p>
      {replay && (
        <Button variant="outline" disabled={busy} onClick={replay}>
          {t("replay")}
        </Button>
      )}
    </div>
  );
}
