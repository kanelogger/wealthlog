import { getCashAvatarLabel } from "../../lib/ticker-logo";
import { cn } from "../../lib/utils";
import { Avatar, AvatarFallback } from "../ui/avatar";

interface TickerAvatarProps {
  symbol: string;
  exchangeMic?: string | null;
  instrumentType?: string | null;
  className?: string;
}

export const TickerAvatar = ({ symbol, className = "size-8" }: TickerAvatarProps) => {
  // Extract the base symbol (before any dot, hyphen, or colon) for fallback
  const baseSymbol = symbol ? symbol.split(/[.:-]/)[0].toUpperCase() : "";
  const fullSymbol = symbol ? symbol.toUpperCase() : "";
  const fallbackAvatarLabel = baseSymbol ? baseSymbol.slice(0, 4) : "•";

  const cashAvatarLabel = getCashAvatarLabel(fullSymbol);

  if (cashAvatarLabel) {
    return (
      <Avatar key="cash" className={cn("font-semibold", className)}>
        <AvatarFallback className="bg-primary/80 dark:bg-primary/20 text-xs font-semibold text-white">
          <span className="p-1" title={fullSymbol}>
            {cashAvatarLabel}
          </span>
        </AvatarFallback>
      </Avatar>
    );
  }

  return (
    <Avatar className={className}>
      <AvatarFallback className="bg-primary/80 dark:bg-primary/20 font-medium text-white">
        <span
          className={cn("px-0.5 leading-none", fallbackAvatarLabel.length >= 4 ? "text-[10px]" : "text-xs")}
          title={fullSymbol}
        >
          {fallbackAvatarLabel}
        </span>
      </AvatarFallback>
    </Avatar>
  );
};
