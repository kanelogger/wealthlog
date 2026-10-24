import { useState } from "react";

import { useAssetLogoOverride } from "@/lib/asset-logo-registry";
import { parseOccSymbol } from "@/lib/occ-symbol";
import { cn } from "@/lib/utils";
import { Avatar, AvatarFallback, AvatarImage, getCashAvatarLabel } from "@wealthfolio/ui";

interface TickerAvatarProps {
  symbol: string;
  /** Enables an asset-scoped custom-logo override lookup. */
  assetId?: string | null;
  /** Explicit image to show first (e.g. a not-yet-saved preview). */
  src?: string;
  exchangeMic?: string | null;
  instrumentType?: string | null;
  className?: string;
  imageClassName?: string;
}

const getFallbackAvatarLabel = (symbol: string): string => symbol.slice(0, 4);

export const TickerAvatar = ({
  symbol,
  assetId,
  src,
  className = "size-8",
  imageClassName = "object-cover p-0",
}: TickerAvatarProps) => {
  // For OCC option symbols (e.g. "AAPL250321C00150000"), use the underlying ticker for logo
  const parsed = symbol ? parseOccSymbol(symbol) : null;
  const logoSymbol = parsed ? parsed.underlying : symbol;

  // Extract the base symbol (before any dot, hyphen, or colon) for fallback
  const baseSymbol = logoSymbol ? logoSymbol.split(/[.:-]/)[0].toUpperCase() : "";
  const fullSymbol = logoSymbol ? logoSymbol.toUpperCase() : "";

  const cashAvatarLabel = getCashAvatarLabel(fullSymbol);
  const override = useAssetLogoOverride(cashAvatarLabel ? {} : { assetId, symbol: fullSymbol });
  const customSrc = src ?? override.dataUri;

  // Use the exact preview source so changing between equally sized images resets a failed load.
  const customKey = src ? `src:${src}` : override.dataUri ? (override.ref?.sha256 ?? "") : "";
  const chainKey = customKey;
  const fallbackAvatarLabel = baseSymbol ? getFallbackAvatarLabel(baseSymbol) : "•";

  // Reset the failed-image state when the custom source changes.
  const [failed, setFailed] = useState({ chainKey, index: 0 });
  const candidateIndex = failed.chainKey === chainKey ? failed.index : 0;
  const logoUrl = candidateIndex === 0 ? customSrc : "";
  const logoSource = logoUrl ? "custom" : "initials";

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
    <Avatar className={className} data-logo-source={logoSource}>
      <AvatarImage
        src={logoUrl}
        alt={fullSymbol}
        className={imageClassName}
        onLoadingStatusChange={(status) => {
          if (status === "error" && logoUrl) {
            setFailed({ chainKey, index: 1 });
          }
        }}
      />
      <AvatarFallback className="bg-primary/80 dark:bg-primary/20 font-medium text-white">
        <span
          className={cn(
            "px-0.5 leading-none",
            fallbackAvatarLabel.length >= 4 ? "text-[10px]" : "text-xs",
          )}
          title={fullSymbol}
        >
          {fallbackAvatarLabel}
        </span>
      </AvatarFallback>
    </Avatar>
  );
};
