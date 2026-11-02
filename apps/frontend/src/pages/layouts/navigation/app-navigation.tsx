import { Icons } from "@wealthfolio/ui/components/ui/icons";
import { useMemo, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

export interface NavLink {
  id?: string;
  title: string;
  href: string;
  icon?: ReactNode;
  keywords?: string[];
  label?: string; // Optional descriptive label for launcher/search
}

export interface NavigationProps {
  primary: NavLink[];
  secondary?: NavLink[];
}

type TFunction = ReturnType<typeof useTranslation>["t"];

function buildStaticNavigation(t: TFunction): NavigationProps {
  return {
    primary: [
      {
        icon: <Icons.Activity className="size-6" />,
        title: t("common:activities"),
        href: "/activities",
        keywords: ["transactions", "trades", "history"],
        label: t("common:nav.label_activities"),
      },
      {
        icon: <Icons.Holdings className="size-6" />,
        title: t("common:holdings"),
        href: "/holdings",
        keywords: ["Holdings", "portfolio", "assets", "positions", "stocks"],
        label: t("common:nav.label_holdings"),
      },
    ],
    secondary: [
      {
        icon: <Icons.Settings className="size-6" />,
        title: t("common:settings"),
        href: "/settings",
        keywords: ["preferences", "config", "configuration"],
      },
    ],
  };
}

export function useNavigation() {
  const { t } = useTranslation();
  return useMemo(() => buildStaticNavigation(t), [t]);
}

export function isPathActive(pathname: string, href: string): boolean {
  if (!href) {
    return false;
  }

  const ensureLeadingSlash = href.startsWith("/") ? href : `/${href}`;
  const normalize = (value: string) => {
    if (value.length > 1 && value.endsWith("/")) {
      return value.slice(0, -1);
    }
    return value;
  };

  const normalizedHref = normalize(ensureLeadingSlash);
  const normalizedPath = normalize(pathname);

  if (normalizedHref === "/") {
    return normalizedPath === "/";
  }

  return normalizedPath === normalizedHref || normalizedPath.startsWith(`${normalizedHref}/`);
}
