import { ErrorBoundary } from "@wealthfolio/ui";
import type { ReactNode } from "react";
import { useLocation, useNavigate } from "react-router-dom";

/**
 * Keeps the app usable when a page fails to render: the error shows in the page
 * area, navigation stays available, and any navigation, even one that changes
 * only the query, clears the error.
 */
export function PageErrorBoundary({ children }: { children: ReactNode }) {
  const { key, pathname, search } = useLocation();
  const navigate = useNavigate();
  const onHome = (pathname === "/" || pathname === "/activities") && !search;
  return (
    <ErrorBoundary
      variant="page"
      resetKey={key}
      onGoHome={onHome ? undefined : () => navigate("/activities")}
    >
      {children}
    </ErrorBoundary>
  );
}
