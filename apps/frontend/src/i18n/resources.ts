import account from "./locales/zh/account.json" with { type: "json" };
import activity from "./locales/zh/activity.json" with { type: "json" };
import allocation from "./locales/zh/allocation.json" with { type: "json" };
import asset from "./locales/zh/asset.json" with { type: "json" };
import auth from "./locales/zh/auth.json" with { type: "json" };
import common from "./locales/zh/common.json" with { type: "json" };
import dashboard from "./locales/zh/dashboard.json" with { type: "json" };
import health from "./locales/zh/health.json" with { type: "json" };
import holdings from "./locales/zh/holdings.json" with { type: "json" };
import income from "./locales/zh/income.json" with { type: "json" };
import insights from "./locales/zh/insights.json" with { type: "json" };
import onboarding from "./locales/zh/onboarding.json" with { type: "json" };
import performance from "./locales/zh/performance.json" with { type: "json" };
import settings from "./locales/zh/settings.json" with { type: "json" };
import spending from "./locales/zh/spending.json" with { type: "json" };
import ui from "./locales/zh/ui.json" with { type: "json" };
import { NAMESPACES, UI_LANGUAGE, type Namespace } from "./locales";

export const resources = {
  [UI_LANGUAGE]: {
    account,
    activity,
    allocation,
    asset,
    auth,
    common,
    dashboard,
    health,
    holdings,
    income,
    insights,
    onboarding,
    performance,
    settings,
    spending,
    ui,
  },
} as const;

const resourceNamespaces = Object.keys(resources[UI_LANGUAGE]) as Namespace[];

if (
  resourceNamespaces.length !== NAMESPACES.length ||
  NAMESPACES.some((namespace) => !resourceNamespaces.includes(namespace))
) {
  throw new Error("Chinese translation resources do not match the namespace list");
}
