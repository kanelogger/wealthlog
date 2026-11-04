import type { InvestmentJournalCommand } from "../investment-journal-commands.generated";
import { invoke, isTauri } from "./platform";

export type { InvestmentJournalCommand } from "../investment-journal-commands.generated";

export interface InvestmentJournalFailure {
  code: string;
  message: string;
  retryable: boolean;
}

export class InvestmentJournalError extends Error implements InvestmentJournalFailure {
  constructor(
    public readonly code: string,
    message: string,
    public readonly retryable = false,
  ) {
    super(message);
    this.name = "InvestmentJournalError";
  }
}

/** Native session admission and field validation stay on the shared business boundary.
 * Financial strings, displayed previews and requestKeys are forwarded unchanged.
 * A caller must retain a frozen write after an unknown outcome and replay its key.
 */
export async function executeInvestmentJournal<T>(
  command: InvestmentJournalCommand,
  args: Record<string, unknown> = {},
): Promise<T> {
  if (!isTauri) {
    throw new InvestmentJournalError("desktop_only", "Investment journal requires the desktop app");
  }
  try {
    return await invoke<T>("execute_investment_journal", { command, args });
  } catch (error) {
    if (error && typeof error === "object" && "code" in error && "message" in error) {
      const failure = error as InvestmentJournalFailure;
      throw new InvestmentJournalError(
        String(failure.code),
        String(failure.message),
        failure.retryable === true,
      );
    }
    throw new InvestmentJournalError("unknown_result", String(error));
  }
}
