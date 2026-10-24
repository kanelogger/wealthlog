import { BrowserRouter, Navigate, Route, Routes } from "react-router-dom";

import useNavigationEventListener from "@/hooks/use-navigation-event-listener";
import ActivityManagerPage from "@/pages/activity/activity-manager-page";
import ActivityPage from "@/pages/activity/activity-page";
import AssetsPage from "@/pages/asset/assets-page";
import PortfolioPage from "@/pages/dashboard/portfolio-page";
import HoldingsPage from "@/pages/holdings/holdings-page";
import IncomePage from "@/pages/income/income-page";
import PortfolioInsightsPage from "@/pages/insights/portfolio-insights";
import { AppLayout } from "@/pages/layouts/app-layout";
import { OnboardingLayout } from "@/pages/layouts/onboarding-layout";
import NotFoundPage from "@/pages/not-found";
import PerformancePage from "@/pages/performance/performance-page";
import SettingsAccountsPage from "@/pages/settings/accounts/accounts-page";
import SettingsAppearancePage from "@/pages/settings/appearance/appearance-page";
import PortfoliosPage from "@/pages/settings/portfolios/portfolios-page";
import SettingsLayout from "@/pages/settings/settings-layout";
import AboutPage from "@/pages/settings/about/about-page";
import SpendingBudgetPage from "./features/spending/pages/spending-budget-page";
import SpendingInsightsPage from "./features/spending/pages/spending-insights-page";
import AccountPage from "./pages/account/account-page";
import AssetProfilePage from "./pages/asset/asset-profile-page";
import HealthPage from "./pages/health/health-page";
import OnboardingPage from "./pages/onboarding/onboarding-page";
import AgentAccessPage from "./pages/settings/agent-access/agent-access-page";
import ExportSettingsPage from "./pages/settings/exports/exports-page";
import GeneralSettingsPage from "./pages/settings/general/general-page";
import MarketDataSettingsPage from "./pages/settings/market-data/market-data-settings";
import SpendingSettingsCategoriesPage from "./pages/settings/spending/categories/spending-categories-page";
import SpendingSettingsEventsPage from "./pages/settings/spending/events/spending-events-page";
import SpendingSettingsRulesPage from "./pages/settings/spending/rules/spending-rules-page";
import SpendingSetupPage from "./pages/settings/spending/setup/spending-setup-page";
import SpendingSettingsPage from "./pages/settings/spending/spending-settings-page";
import TaxonomiesPage from "./pages/settings/taxonomies/taxonomies-page";

function NavigationEventBridge() {
  useNavigationEventListener();
  return null;
}

export function AppRoutes() {
  return (
    <BrowserRouter>
      <NavigationEventBridge />
      <Routes>
        {/* Onboarding with dedicated layout */}
        <Route path="/onboarding" element={<OnboardingLayout />}>
          <Route index element={<OnboardingPage />} />
        </Route>

        {/* Main app with sidebar */}
        <Route path="/" element={<AppLayout />}>
          <Route index element={<PortfolioPage />} />
          <Route path="dashboard" element={<PortfolioPage />} />
          <Route path="activities" element={<ActivityPage />} />
          <Route path="activities/manage" element={<ActivityManagerPage />} />
          <Route path="holdings" element={<HoldingsPage />} />
          <Route path="holdings/:assetId" element={<AssetProfilePage />} />

          <Route path="accounts/:id" element={<AccountPage />} />
          <Route path="income" element={<IncomePage />} />
          <Route path="performance" element={<PerformancePage />} />
          <Route path="insights" element={<PortfolioInsightsPage />} />
          <Route path="health" element={<HealthPage />} />
          <Route
            path="allocation-targets"
            element={<Navigate to="/insights?tab=overview" replace />}
          />
          <Route path="spending/insights" element={<SpendingInsightsPage />} />
          <Route path="spending/budget" element={<SpendingBudgetPage />} />
          <Route path="settings" element={<SettingsLayout />}>
            <Route index element={<GeneralSettingsPage />} />
            <Route path="general" element={<GeneralSettingsPage />} />
            <Route path="accounts" element={<SettingsAccountsPage />} />
            <Route path="portfolios" element={<PortfoliosPage />} />
            <Route path="appearance" element={<SettingsAppearancePage />} />
            <Route path="exports" element={<ExportSettingsPage />} />
            <Route path="spending" element={<SpendingSettingsPage />} />
            <Route path="spending/categories" element={<SpendingSettingsCategoriesPage />} />
            <Route path="spending/events" element={<SpendingSettingsEventsPage />} />
            <Route path="spending/rules" element={<SpendingSettingsRulesPage />} />
            <Route path="spending/setup" element={<SpendingSetupPage />} />
            <Route path="market-data" element={<MarketDataSettingsPage />} />
            <Route path="securities" element={<AssetsPage />} />
            <Route path="taxonomies" element={<TaxonomiesPage />} />
            <Route path="agent-access" element={<AgentAccessPage />} />
            <Route path="about" element={<AboutPage />} />
          </Route>
          <Route path="*" element={<NotFoundPage />} />
        </Route>
      </Routes>
    </BrowserRouter>
  );
}
