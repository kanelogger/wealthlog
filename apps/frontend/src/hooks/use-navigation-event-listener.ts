import { listenNavigateToRoute } from "@/adapters";
import { useEffect } from "react";
import { useNavigate } from "react-router-dom";

const useNavigationEventListener = () => {
  const navigate = useNavigate();

  useEffect(() => {
    let cleanup: (() => void) | undefined;
    let disposed = false;

    const setupNavigationListener = async () => {
      const handleNavigateToRoute = (event: { payload: { route: string } }) => {
        const { route } = event.payload;
        navigate(route);
      };

      const unlisten = await listenNavigateToRoute(handleNavigateToRoute);
      return unlisten;
    };

    setupNavigationListener()
      .then((unlistenFn) => {
        if (disposed) unlistenFn();
        else cleanup = unlistenFn;
      })
      .catch((error) => {
        console.error("Failed to setup navigation event listener:", error);
      });

    return () => {
      disposed = true;
      cleanup?.();
    };
  }, [navigate]);

  return null;
};

export default useNavigationEventListener;
