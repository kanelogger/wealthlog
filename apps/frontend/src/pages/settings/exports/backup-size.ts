import type { NumberFormatting } from "@wealthfolio/ui";

export function formatBackupSize(
  bytes: number,
  formatDecimal: NumberFormatting["formatDecimal"],
): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  const exponent = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = formatDecimal(bytes / 1024 ** exponent, {
    maximumFractionDigits: exponent === 0 ? 0 : 1,
  });
  return `${value} ${units[exponent]}`;
}
