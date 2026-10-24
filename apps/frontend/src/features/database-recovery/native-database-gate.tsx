import { isWeb } from "@/adapters";
import { DATABASE_STATE_CHANGED } from "@/adapters/tauri/events";
import {
  getDatabaseStartupStatus,
  retryDatabaseStartup,
  type DatabaseStartupStatus,
} from "@/adapters/tauri/settings";
import { StartupError } from "@/components/startup-error";
import { StartupScreen } from "@/components/startup-screen";
import { assetLogoRegistry } from "@/lib/asset-logo-registry";
import { reloadApplication } from "@/lib/reload-application";
import { listen } from "@tauri-apps/api/event";
import { useQueryClient } from "@tanstack/react-query";
import { Fragment, useEffect, useRef, useState, type ReactNode } from "react";

const STARTUP_QUERY_KEY = ["database-startup-status"];

export function NativeDatabaseGate({ children }: { children: ReactNode }) {
  const queryClient = useQueryClient();
  const [status, setStatus] = useState<DatabaseStartupStatus | null>(null);
  const [error, setError] = useState<Error | null>(null);
  const [isRetrying, setIsRetrying] = useState(false);
  const [subscriptionFailed, setSubscriptionFailed] = useState(false);
  const retryingRef = useRef(false);

  useEffect(() => {
    if (isWeb) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    let refreshing: Promise<void> | null = null;
    let pending = false;
    let generation = queryClient.getQueryData<DatabaseStartupStatus>(STARTUP_QUERY_KEY)?.generation;

    const refresh = (): Promise<void> => {
      if (retryingRef.current) return Promise.resolve();
      pending = true;
      if (refreshing) return refreshing;
      refreshing = (async () => {
        while (pending && !disposed && !retryingRef.current) {
          pending = false;
          try {
            const next = await getDatabaseStartupStatus();
            if (disposed || retryingRef.current) return;
            if (next.ready && !next.maintenance) {
              if (generation !== undefined && next.generation !== generation) {
                await queryClient.cancelQueries();
                if (disposed || retryingRef.current) return;
                queryClient.clear();
                assetLogoRegistry.reset();
              }
              generation = next.generation;
            }
            queryClient.setQueryData(STARTUP_QUERY_KEY, next);
            setError(null);
            setStatus(next);
          } catch (cause) {
            if (disposed || retryingRef.current) return;
            setStatus(null);
            setError(cause instanceof Error ? cause : new Error(String(cause)));
          }
        }
      })().finally(() => {
        refreshing = null;
      });
      return refreshing;
    };

    void listen(DATABASE_STATE_CHANGED, () => {
      void refresh();
    })
      .then((stop) => {
        if (disposed) {
          stop();
          return;
        }
        unlisten = stop;
        void refresh();
      })
      .catch((cause) => {
        if (disposed) return;
        setSubscriptionFailed(true);
        setError(cause instanceof Error ? cause : new Error(String(cause)));
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [queryClient]);

  const retry = async () => {
    if (subscriptionFailed) {
      reloadApplication();
      return;
    }
    retryingRef.current = true;
    setIsRetrying(true);
    try {
      await retryDatabaseStartup();
      reloadApplication();
    } catch (cause) {
      setError(cause instanceof Error ? cause : new Error(String(cause)));
      retryingRef.current = false;
      setIsRetrying(false);
    }
  };

  if (isWeb) return children;
  if (status?.maintenance) return <StartupScreen />;
  if (error || status?.error) {
    return (
      <StartupError
        error={error ?? new Error(status?.error ?? "Database startup failed")}
        isRetrying={isRetrying}
        onRetry={() => {
          void retry();
        }}
      />
    );
  }
  if (status?.ready) return <Fragment key={status.generation}>{children}</Fragment>;
  return <StartupScreen />;
}
