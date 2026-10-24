import { Icons } from "@wealthfolio/ui";
import React from "react";

export function resolveNavigationIcon(icon: React.ReactNode, className: string) {
  if (!icon) {
    return <Icons.ChevronRight className={className} />;
  }

  // Navigation items pass React elements directly.
  if (React.isValidElement<{ className?: string }>(icon)) {
    return icon.props.className ? icon : React.cloneElement(icon, { className });
  }

  if (typeof icon === "function") {
    const IconComponent = icon as React.ComponentType<{ className?: string }>;
    return <IconComponent className={className} />;
  }

  return <Icons.ChevronRight className={className} />;
}
