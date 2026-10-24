import { StartupBoundary } from "@/components/startup-boundary";
import { StartupScreen } from "@/components/startup-screen";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./globals.css";
// Initialize i18next before the app renders. The active language is applied
// from the stored user setting by the settings provider.
import "./i18n/i18n";

if (import.meta.env.DEV) {
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    {/* Suspense boundary for i18next lazy-loaded translation resources: the auth
        layer reads translations before any route-level boundary, so a top-level
        boundary prevents a cold-load suspend from blanking the app. */}
    <StartupBoundary>
      <React.Suspense fallback={<StartupScreen />}>
        <App />
      </React.Suspense>
    </StartupBoundary>
  </React.StrictMode>,
);
