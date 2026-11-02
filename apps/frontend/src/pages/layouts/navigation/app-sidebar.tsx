import { isWeb } from "@/adapters";
import { isAppleDevice } from "@/lib/device-utils";
import { useAuth } from "@/context/auth-context";
import { ProfileMenu } from "@/features/profiles/profile-menu";
import { cn } from "@/lib/utils";
import { Button } from "@wealthfolio/ui/components/ui/button";
import { Icons } from "@wealthfolio/ui/components/ui/icons";
import { Separator } from "@wealthfolio/ui/components/ui/separator";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Link, useLocation } from "react-router-dom";
import { type NavLink, type NavigationProps, isPathActive } from "./app-navigation";
import { resolveNavigationIcon } from "./navigation-icons";

interface AppSidebarProps {
  navigation: NavigationProps;
}

const modKey = isAppleDevice() ? "⌘" : "Ctrl";

export function AppSidebar({ navigation }: AppSidebarProps) {
  const { t } = useTranslation();
  const [collapsed, setCollapsed] = useState(false);
  const { logout, requiresAuth } = useAuth();

  return (
    <div
      className={cn({
        "studio-sidebar-shell hidden h-full border-r pt-12 transition-[width] duration-200 ease-out md:flex md:flex-shrink-0 md:overflow-hidden": true,
        "md:w-sidebar": !collapsed,
        "md:w-sidebar-collapsed": collapsed,
      })}
      data-tauri-drag-region="true"
    >
      <div className="z-20 w-full md:flex">
        <div className="flex w-full flex-col">
          <div className="flex min-h-0 w-full flex-1 flex-col">
            <div data-tauri-drag-region="true" className="min-h-0 flex-1 overflow-y-auto">
              <nav
                data-tauri-drag-region="true"
                aria-label={t("common:layout.sidebar")}
                className="flex shrink-0 flex-col gap-1 p-3"
              >
                <div
                  data-tauri-drag-region="true"
                  className="draggable flex items-center justify-center pb-7"
                >
                  <Link to="/" aria-label="Wealthlog" className="block shrink-0">
                    <img
                      className="h-9 w-9 bg-transparent object-contain"
                      aria-hidden="true"
                      src="/logo.png"
                    />
                  </Link>

                  <span
                    className={cn(
                      "text-foreground ml-2 font-sans text-lg font-semibold tracking-tight transition-opacity duration-150",
                      {
                        "sr-only opacity-0": collapsed,
                        "block opacity-100": !collapsed,
                      },
                    )}
                  >
                    Wealthlog
                  </span>
                </div>

                <Button
                  type="button"
                  variant="ghost"
                  onClick={() => {
                    // Trigger the launcher by dispatching Cmd/Ctrl+K
                    const event = new KeyboardEvent("keydown", {
                      key: "k",
                      code: "KeyK",
                      keyCode: 75,
                      which: 75,
                      metaKey: true,
                      ctrlKey: true,
                      bubbles: true,
                      cancelable: true,
                    });
                    document.dispatchEvent(event);
                  }}
                  className={cn(
                    "text-foreground [&_svg]:size-4! mb-4 h-10 transition-colors duration-150",
                    collapsed
                      ? "justify-center rounded-md"
                      : "bg-muted/60 hover:bg-muted justify-start rounded-md px-3 shadow-none",
                  )}
                  title={t("common:layout.search_shortcut", { shortcut: `${modKey}+K` })}
                >
                  <span aria-hidden="true">
                    <Icons.Search2 className="h-5 w-5 opacity-60" />
                  </span>
                  <span
                    className={cn({
                      "text-muted-foreground ml-2 flex-1 text-left text-sm transition-opacity delay-100 duration-300 ease-in-out": true,
                      "sr-only opacity-0": collapsed,
                      "block opacity-100": !collapsed,
                    })}
                  >
                    {t("common:layout.search")}
                  </span>
                  {!collapsed && (
                    <kbd className="bg-background text-muted-foreground pointer-events-none inline-flex h-5 select-none items-center gap-1 rounded border px-1.5 font-mono text-[10px] font-medium opacity-100">
                      <span className="text-xs">{modKey}</span>K
                    </kbd>
                  )}
                </Button>

                {navigation?.primary?.map((item) => (
                  <NavItem key={item.title} item={item} collapsed={collapsed} />
                ))}
              </nav>
            </div>

            <div className="flex shrink-0 flex-col gap-1 p-3">
              {navigation?.secondary?.map((item) => (
                <NavItem key={item.title} item={item} collapsed={collapsed} />
              ))}
              {isWeb && requiresAuth && (
                <Button
                  type="button"
                  variant="ghost"
                  onClick={logout}
                  className={cn(
                    "text-foreground [&_svg]:size-5! mb-1 h-12 rounded-md transition-all duration-300",
                    collapsed ? "justify-center" : "justify-start",
                  )}
                  title={t("common:layout.logout")}
                >
                  <span aria-hidden="true">
                    <Icons.LogOut className="h-5 w-5" />
                  </span>
                  <span
                    className={cn({
                      "ml-2 transition-opacity delay-100 duration-300 ease-in-out": true,
                      "sr-only opacity-0": collapsed,
                      "block opacity-100": !collapsed,
                    })}
                  >
                    {t("common:layout.logout")}
                  </span>
                </Button>
              )}
              <Separator className="mt-0" />
              <div className="flex justify-end">
                <Button
                  title={t("common:layout.toggle_sidebar")}
                  variant="ghost"
                  onClick={() => setCollapsed(!collapsed)}
                  className="text-muted-foreground [&_svg]:size-5! cursor-pointer rounded-md hover:bg-transparent"
                  aria-label={
                    collapsed
                      ? t("common:layout.expand_sidebar")
                      : t("common:layout.collapse_sidebar")
                  }
                >
                  <Icons.PanelLeftOpen
                    size={18}
                    className={`h-5 w-5 transition-transform duration-500 ease-in-out ${!collapsed ? "rotate-180" : ""}`}
                    aria-label={
                      collapsed
                        ? t("common:layout.expand_sidebar")
                        : t("common:layout.collapse_sidebar")
                    }
                  />
                </Button>
              </div>
              <div className="flex justify-center pt-1">
                <ProfileMenu collapsed={collapsed} />
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

interface NavItemProps {
  item: NavLink;
  collapsed: boolean;
  className?: string;
  onClick?: () => void;
}

function NavItem({ item, collapsed, className, ...props }: NavItemProps) {
  const location = useLocation();
  const isActive = isPathActive(location.pathname, item.href);

  return (
    <Button
      key={item.title}
      variant={isActive ? "secondary" : "ghost"}
      asChild
      className={cn(
        "studio-sidebar-link [&_svg]:size-[18px]! mb-1 h-11 rounded-md transition-colors duration-150",
        isActive
          ? "bg-sidebar-primary text-sidebar-primary-foreground hover:bg-sidebar-primary/90 hover:text-sidebar-primary-foreground"
          : "text-sidebar-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
        collapsed ? "justify-center" : "justify-start",
        className,
      )}
    >
      <Link
        key={item.title}
        to={item.href}
        title={item.title}
        aria-current={isActive ? "page" : undefined}
        {...props}
      >
        <span aria-hidden="true">{resolveNavigationIcon(item.icon, "h-5 w-5")}</span>

        <span
          className={cn({
            "ml-2 transition-opacity delay-100 duration-300 ease-in-out": true,
            "sr-only opacity-0": collapsed,
            "block opacity-100": !collapsed,
          })}
        >
          {item.title}
        </span>
      </Link>
    </Button>
  );
}
