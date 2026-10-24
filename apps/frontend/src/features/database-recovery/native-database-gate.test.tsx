import { act, fireEvent, render, screen, waitFor } from "@/test/render";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import copy from "@/i18n/locales/zh/common.json";
import { NativeDatabaseGate } from "./native-database-gate";

const mocks = vi.hoisted(() => ({
  status: vi.fn(),
  changed: () => {},
  stop: vi.fn(),
  listen: vi.fn(),
  retry: vi.fn(),
  reload: vi.fn(),
  isWeb: false,
  resetLogos: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("@/lib/reload-application", () => ({ reloadApplication: mocks.reload }));
vi.mock("@/lib/asset-logo-registry", () => ({ assetLogoRegistry: { reset: mocks.resetLogos } }));
vi.mock("@/adapters", () => ({
  get isWeb() {
    return mocks.isWeb;
  },
}));
vi.mock("@/adapters/tauri/events", () => ({
  DATABASE_STATE_CHANGED: "database-state-changed",
}));
vi.mock("@/adapters/tauri/settings", () => ({
  getDatabaseStartupStatus: mocks.status,
  retryDatabaseStartup: mocks.retry,
}));
const ready = { ready: true, maintenance: false, error: null, generation: "1" };
const failed = { ...ready, ready: false, error: "Missing database key" };

beforeEach(() => {
  vi.resetAllMocks();
  mocks.isWeb = false;
  mocks.status.mockResolvedValue(failed);
  mocks.retry.mockResolvedValue(undefined);
  mocks.listen.mockImplementation(async (_event: string, callback: () => void) => {
    mocks.changed = callback;
    return mocks.stop;
  });
});

function mount(client = new QueryClient({ defaultOptions: { queries: { retry: false } } })) {
  return render(
    <QueryClientProvider client={client}>
      <NativeDatabaseGate>
        <p>Portfolio mounted</p>
      </NativeDatabaseGate>
    </QueryClientProvider>,
  );
}

it("blocks financial children during startup and opens only after fresh readiness", async () => {
  mocks.status.mockResolvedValueOnce({ ...ready, ready: false }).mockResolvedValue(ready);
  mount();
  await waitFor(() => expect(mocks.status).toHaveBeenCalledOnce());
  expect(screen.getByRole("status")).toHaveTextContent("正在打开 Wealthlog…");
  expect(screen.queryByText("Portfolio mounted")).not.toBeInTheDocument();
  await act(async () => mocks.changed());
  await screen.findByText("Portfolio mounted");
});

it("prioritizes maintenance over a stale ready flag or startup error", async () => {
  mocks.status.mockResolvedValue({ ...failed, ready: true, maintenance: true });
  mount();
  await waitFor(() => expect(mocks.status).toHaveBeenCalledOnce());
  expect(screen.getByRole("status")).toBeInTheDocument();
  expect(screen.queryByText(failed.error)).not.toBeInTheDocument();
  expect(screen.queryByText("Portfolio mounted")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: copy.retry })).not.toBeInTheDocument();
});

it("keeps financial children blocked on a status read failure", async () => {
  mocks.status.mockRejectedValue(new Error("Status unavailable"));
  mount();
  await screen.findByText("Status unavailable");
  expect(screen.queryByText("Portfolio mounted")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: copy.retry })).toBeEnabled();
});

it("reloads after startup retry before reading the replacement generation", async () => {
  mount();
  await screen.findByText(failed.error);
  mocks.retry.mockImplementation(async () => {
    mocks.status.mockRejectedValue(new Error("PROFILE_STALE"));
    mocks.changed();
  });
  fireEvent.click(screen.getByRole("button", { name: copy.retry }));
  await waitFor(() => expect(mocks.reload).toHaveBeenCalledOnce());
  expect(mocks.retry).toHaveBeenCalledOnce();
  expect(mocks.status).toHaveBeenCalledOnce();
  expect(screen.queryByText("Portfolio mounted")).not.toBeInTheDocument();
});

it("shows retry failures and leaves retry reachable", async () => {
  mocks.retry.mockRejectedValue(new Error("Database still owned"));
  mount();
  await screen.findByText(failed.error);
  fireEvent.click(screen.getByRole("button", { name: copy.retry }));
  await screen.findByText("Database still owned");
  expect(screen.queryByText("Portfolio mounted")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: copy.retry })).toBeEnabled();
});

it("does not admit cached readiness and clears financial caches on generation replacement", async () => {
  const client = new QueryClient();
  client.setQueryData(["database-startup-status"], ready);
  client.setQueryData(["holdings"], [{ id: "old-data" }]);
  mocks.status.mockResolvedValue({ ...ready, generation: "2" });
  mount(client);
  expect(screen.queryByText("Portfolio mounted")).not.toBeInTheDocument();
  await screen.findByText("Portfolio mounted");
  expect(client.getQueryData(["holdings"])).toBeUndefined();
  expect(mocks.resetLogos).toHaveBeenCalledOnce();
  expect(client.getQueryData(["database-startup-status"])).toEqual({ ...ready, generation: "2" });
});

it("preserves financial caches when the generation is unchanged", async () => {
  const client = new QueryClient();
  client.setQueryData(["database-startup-status"], ready);
  client.setQueryData(["holdings"], [{ id: "current-data" }]);
  mocks.status.mockResolvedValue(ready);
  mount(client);
  await screen.findByText("Portfolio mounted");
  await act(async () => mocks.changed());
  expect(client.getQueryData(["holdings"])).toEqual([{ id: "current-data" }]);
});

it("unmounts financial children during maintenance and continues observing startup failure", async () => {
  mocks.status.mockResolvedValue(ready);
  mount();
  await screen.findByText("Portfolio mounted");
  mocks.status.mockResolvedValue({ ...ready, ready: false, maintenance: true });
  await act(async () => mocks.changed());
  await waitFor(() => expect(screen.queryByText("Portfolio mounted")).not.toBeInTheDocument());
  mocks.status.mockResolvedValue(failed);
  await act(async () => mocks.changed());
  await screen.findByText(failed.error);
  expect(mocks.stop).not.toHaveBeenCalled();
});

it("rereads a transition received while a status request is pending", async () => {
  const { promise, resolve } = Promise.withResolvers<typeof ready>();
  mocks.status.mockReturnValueOnce(promise).mockResolvedValue(ready);
  mount();
  await waitFor(() => expect(mocks.status).toHaveBeenCalledOnce());
  await act(async () => {
    mocks.changed();
    resolve({ ...ready, ready: false, maintenance: true });
  });
  await screen.findByText("Portfolio mounted");
  expect(mocks.status).toHaveBeenCalledTimes(2);
});

it("does not refetch startup on focus and unsubscribes after readiness", async () => {
  mocks.status.mockResolvedValue(ready);
  const view = mount();
  await screen.findByText("Portfolio mounted");
  await act(async () => {
    window.dispatchEvent(new Event("focus"));
  });
  expect(mocks.status).toHaveBeenCalledOnce();
  view.unmount();
  expect(mocks.stop).toHaveBeenCalledOnce();
});

it("unsubscribes on unmount and ignores late reads", async () => {
  const { promise, resolve } = Promise.withResolvers<typeof ready>();
  mocks.status.mockReturnValue(promise);
  const client = new QueryClient();
  const view = mount(client);
  await waitFor(() => expect(mocks.status).toHaveBeenCalledOnce());
  view.unmount();
  await act(async () => {
    resolve(ready);
  });
  expect(mocks.stop).toHaveBeenCalledOnce();
  expect(client.getQueryData(["database-startup-status"])).toBeUndefined();
});

it("cleans a subscription that finishes after unmount", async () => {
  const { promise, resolve } = Promise.withResolvers<() => void>();
  mocks.listen.mockReturnValue(promise);
  const view = mount();
  view.unmount();
  await act(async () => {
    resolve(mocks.stop);
  });
  expect(mocks.stop).toHaveBeenCalledOnce();
  expect(mocks.status).not.toHaveBeenCalled();
});

it("reloads on subscription failure without issuing a database retry", async () => {
  mocks.listen.mockRejectedValue(new Error("Event subscription unavailable"));
  mount();
  await screen.findByText("Event subscription unavailable");
  fireEvent.click(screen.getByRole("button", { name: copy.retry }));
  expect(mocks.reload).toHaveBeenCalledOnce();
  expect(mocks.retry).not.toHaveBeenCalled();
});

it("admits the web runtime without calling native startup APIs", () => {
  mocks.isWeb = true;
  mount();
  expect(screen.getByText("Portfolio mounted")).toBeInTheDocument();
  expect(mocks.status).not.toHaveBeenCalled();
  expect(mocks.listen).not.toHaveBeenCalled();
});
