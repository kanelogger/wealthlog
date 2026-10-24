import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { profileChangesChannel } from "./api";
import { ProfileShell } from "./profile-shell";
import { ProfileMenu } from "./profile-menu";
import { MobileProfileMenu } from "./mobile-profile-menu";
const mocks = vi.hoisted(() => ({
  isWeb: true,
  changed: () => {},
  ready: () => {},
  databaseChanged: () => {},
  command: vi.fn(),
  admitted: vi.fn(() => true),
  reload: vi.fn(),
  listen: vi.fn(async () => async () => {}),
}));
vi.mock("@/lib/reload-application", () => ({ reloadApplication: mocks.reload }));
vi.mock("@/adapters", () => ({
  get isWeb() {
    return mocks.isWeb;
  },
  listenPortfolioUpdateStart: mocks.listen,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (_event: string, callback: () => void) => {
    if (_event === "profile-session-changed") mocks.changed = callback;
    if (_event === "app:ready") mocks.ready = callback;
    if (_event === "database-state-changed") mocks.databaseChanged = callback;
    return () => {};
  }),
}));
vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  profileCommand: mocks.command,
  profileChangesChannel: new EventTarget(),
}));
vi.mock("./auth-bridge", () => ({ isNativeAuthPending: () => false }));
vi.mock("./session", () => ({
  installProfileSession: mocks.admitted,
  hasProfileSession: () => false,
  profileScope: () => "scope",
  revokeProfileSession: () => window.dispatchEvent(new Event("wealthfolio:profile-locked")),
}));
const profile = { id: "a", name: "Personal", avatarId: "clay-pebble-animated", lockEnabled: false };
const unlocked = {
  profiles: [profile],
  session: { profileId: "a", scopeId: "scope" },
  starting: false,
};
const mount = () =>
  render(
    <QueryClientProvider client={new QueryClient()}>
      <ProfileShell>
        <ProfileMenu collapsed />
        <div>Private portfolio</div>
      </ProfileShell>
    </QueryClientProvider>,
  );
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  Reflect.deleteProperty(navigator, "clipboard");
});
beforeEach(() => {
  vi.stubGlobal(
    "Image",
    class {
      src = "";
      decode() {
        return Promise.resolve();
      }
    },
  );
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  vi.clearAllMocks();
  mocks.isWeb = true;
  mocks.command.mockResolvedValue(unlocked);
});
it.each(["锁定 Wealthlog", "切换个人资料"])(
  "unmounts financial providers immediately on %s from the profile menu",
  async (action) => {
    if (action === "锁定 Wealthlog")
      mocks.command.mockResolvedValue({
        ...unlocked,
        profiles: [{ ...profile, lockEnabled: true }],
      });
    else
      mocks.command.mockResolvedValue({
        ...unlocked,
        profiles: [profile, { ...profile, id: "b", name: "Family" }],
      });
    mount();
    expect(await screen.findByText("Private portfolio")).toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole("button", { name: "Personal 的个人资料菜单" }), {
      key: "Enter",
    });
    const item = await screen.findByRole("menuitem", { name: action });
    mocks.command.mockResolvedValue({ ...unlocked, session: null });
    fireEvent.click(item);
    expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
    expect(mocks.command).toHaveBeenCalledWith("lock_profile");
    expect(
      await screen.findByRole("heading", {
        name: "谁在使用 Wealthlog？",
      }),
    ).toBeInTheDocument();
  },
);
it.each([true, false])(
  "opens an unprotected profile directly and keeps settings available (web: %s)",
  async (isWeb) => {
    mocks.isWeb = isWeb;
    mount();
    expect(await screen.findByText("Private portfolio")).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "谁在使用 Wealthlog？" }),
    ).not.toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole("button", { name: "Personal 的个人资料菜单" }), {
      key: "Enter",
    });
    expect(await screen.findByRole("menuitem", { name: "个人资料设置" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "添加个人资料" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "切换个人资料" })).not.toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "锁定 Wealthlog" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("menuitem", { name: "个人资料设置" }));
    fireEvent.click(await screen.findByRole("button", { name: "启用密码" }));
    expect(screen.getByLabelText("新密码")).toBeInTheDocument();
    expect(screen.queryByLabelText("当前密码或恢复代码")).not.toBeInTheDocument();
  },
);
it.each([
  "PROFILE_UNAVAILABLE: The profile registry is missing. Restore profiles.json from a backup.",
  "PROFILE_UNAVAILABLE: The profile registry is invalid.",
])("keeps registry startup failures recoverable: %s", async (startupError) => {
  mocks.isWeb = false;
  const failed = { profiles: [], session: null, starting: false, startupError };
  mocks.command.mockResolvedValue(failed);
  mount();
  expect(await screen.findByText("无法打开 Wealthlog")).toBeInTheDocument();
  expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "创建个人资料" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByText("技术详情"));
  expect(screen.getByText(startupError)).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "打开数据文件夹" }));
  await waitFor(() => expect(mocks.command).toHaveBeenCalledWith("open_profile_data_folder"));
  expect(mocks.reload).not.toHaveBeenCalled();
  await waitFor(() => expect(screen.getByRole("button", { name: "重试" })).toBeEnabled());
  mocks.command.mockImplementation(async (command: string) => {
    if (command === "retry_profile_startup") throw new Error(startupError);
    return failed;
  });
  fireEvent.click(screen.getByRole("button", { name: "重试" }));
  expect(
    await screen.findByText("无法完成此步骤，请重试。"),
  ).toBeInTheDocument();
  expect(mocks.reload).not.toHaveBeenCalled();
  mocks.command.mockImplementation(async (command: string) =>
    command === "get_profile_state" ? unlocked : null,
  );
  fireEvent.click(screen.getByRole("button", { name: "重试" }));
  await waitFor(() => expect(mocks.reload).toHaveBeenCalledOnce());
});
it.each([false, true])(
  "requires confirmation before starting fresh and handles failure: %s",
  async (fails) => {
    mocks.isWeb = false;
    const failed = {
      profiles: [],
      session: null,
      starting: false,
      startupError: "PROFILE_UNAVAILABLE: The profile registry is missing.",
    };
    const empty = { ...failed, startupError: null };
    mocks.command.mockResolvedValue(failed);
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "设置新的个人资料" }));
    expect(
      screen.getByRole("heading", { name: "使用空白个人资料开始？" }),
    ).toBeInTheDocument();
    expect(mocks.command).not.toHaveBeenCalledWith("start_new_profile_setup");
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(mocks.command).not.toHaveBeenCalledWith("start_new_profile_setup");
    fireEvent.click(screen.getByRole("button", { name: "设置新的个人资料" }));
    mocks.command.mockImplementation((command: string) => {
      if (command === "start_new_profile_setup") {
        return fails
          ? Promise.reject(new Error("Cannot preserve registry files"))
          : Promise.resolve(empty);
      }
      return Promise.resolve(fails ? failed : empty);
    });
    fireEvent.click(screen.getByRole("button", { name: "继续设置" }));
    await waitFor(() => expect(mocks.command).toHaveBeenCalledWith("start_new_profile_setup"));
    if (fails) {
      expect(await screen.findByRole("alert")).toHaveTextContent(
        "无法完成此步骤，请重试。",
      );
      expect(screen.getByRole("button", { name: "继续设置" })).toBeEnabled();
    } else {
      expect(await screen.findByLabelText("名称")).toHaveValue("");
      expect(screen.getByRole("button", { name: "启用密码" })).toBeInTheDocument();
      expect(
        screen.queryByRole("heading", { name: "无法打开 Wealthlog" }),
      ).not.toBeInTheDocument();
    }
    expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
  },
);

it("does not expose a protected profile while startup authorization is pending", async () => {
  mocks.command.mockResolvedValue({
    profiles: [{ ...profile, lockEnabled: true }],
    session: null,
    starting: false,
  });
  mount();
  await screen.findByRole("heading", { name: "谁在使用 Wealthlog？" });
  expect(screen.queryByLabelText("密码")).not.toBeInTheDocument();
  expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Personal" }));
  expect(screen.getByLabelText("密码")).toHaveAttribute("type", "password");
  fireEvent.click(screen.getByRole("button", { name: "显示密码" }));
  expect(screen.getByLabelText("密码")).toHaveAttribute("type", "text");
  fireEvent.click(screen.getByRole("button", { name: "隐藏密码" }));
  expect(screen.getByLabelText("密码")).toHaveAttribute("type", "password");
});
it("prompts for the existing password when a migrated profile has a stale unlocked hint", async () => {
  mocks.command.mockImplementation(async (command: string, input?: { proof?: string }) => {
    if (command === "unlock_profile") {
      if (!input?.proof) throw new Error("PROFILE_LOCKED: Unlock this profile to continue.");
      if (input.proof === "wrong password") throw new Error("PROFILE_PASSWORD_INVALID: incorrect");
      return unlocked.session;
    }
    return { ...unlocked, session: null };
  });
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "Personal" }));
  const password = await screen.findByLabelText("密码");
  expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
  expect(mocks.reload).not.toHaveBeenCalled();
  // A state read restores the stale registry hint before the user submits their password.
  act(
    () =>
      void profileChangesChannel?.dispatchEvent(new MessageEvent("message", { data: "changed" })),
  );
  await waitFor(() =>
    expect(
      mocks.command.mock.calls.filter(([name]) => name === "get_profile_state").length,
    ).toBeGreaterThanOrEqual(2),
  );
  fireEvent.change(password, { target: { value: "wrong password" } });
  fireEvent.submit(password.closest("form")!);
  await waitFor(() =>
    expect(screen.getByLabelText("密码")).toHaveAttribute("aria-invalid", "true"),
  );
  expect(screen.getByLabelText("密码")).toHaveFocus();
  expect(mocks.reload).not.toHaveBeenCalled();
  fireEvent.change(password, { target: { value: "existing password" } });
  fireEvent.submit(password.closest("form")!);
  await waitFor(() =>
    expect(mocks.command).toHaveBeenCalledWith("unlock_profile", {
      profileId: "a",
      proof: "existing password",
    }),
  );
  await waitFor(() => expect(mocks.reload).toHaveBeenCalled());
});
it("keeps the new recovery code visible after password setup revokes the session", async () => {
  mount();
  fireEvent.keyDown(await screen.findByRole("button", { name: "Personal 的个人资料菜单" }), {
    key: "Enter",
  });
  fireEvent.click(await screen.findByRole("menuitem", { name: "个人资料设置" }));
  fireEvent.click(screen.getByRole("button", { name: "启用密码" }));
  fireEvent.change(screen.getByLabelText("新密码"), {
    target: { value: "my passphrase 🔒" },
  });
  fireEvent.change(screen.getByLabelText("再次输入密码"), {
    target: { value: "my passphrase 🔒" },
  });
  mocks.command.mockImplementation(async (command: string) => {
    if (command === "set_profile_password") return "TEST-RECOVERY-CODE";
    if (command === "get_profile_state") return { ...unlocked, session: null };
    return null;
  });
  await act(async () =>
    fireEvent.click(screen.getByRole("button", { name: /^(?:保存(?:更改)?)$/ })),
  );
  await waitFor(() => expect(screen.getByText("TEST-RECOVERY-CODE")).toBeInTheDocument());
  expect(screen.getByRole("button", { name: "我已保存恢复代码" })).toBeInTheDocument();
  expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}
async function menuAction(name: string) {
  fireEvent.keyDown(await screen.findByRole("button", { name: "Personal 的个人资料菜单" }), {
    key: "Enter",
  });
  fireEvent.click(await screen.findByRole("menuitem", { name }));
}
it("does not show the chooser while the initial profile state is unresolved", async () => {
  const pending = deferred<typeof unlocked>();
  mocks.command.mockReturnValue(pending.promise);
  mount();
  expect(screen.getByRole("status")).toHaveTextContent("正在打开 Wealthlog…");
  expect(screen.getByRole("status")).toHaveClass("sr-only");
  expect(screen.getByRole("img", { name: "Wealthlog" })).toHaveAttribute("src", "/logo.png");
  expect(screen.queryByRole("heading", { name: "谁在使用 Wealthlog？" })).not.toBeInTheDocument();
  expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
  await act(async () => pending.resolve(unlocked));
  expect(await screen.findByText("Private portfolio")).toBeInTheDocument();
});
it("waits for teardown before allowing profile selection", async () => {
  const closing = deferred<void>();
  mocks.command.mockResolvedValue({
    ...unlocked,
    profiles: [profile, { ...profile, id: "b", name: "Family" }],
  });
  mount();
  await screen.findByText("Private portfolio");
  mocks.command.mockImplementation((command) =>
    command === "lock_profile" ? closing.promise : Promise.resolve({ ...unlocked, session: null }),
  );
  await menuAction("切换个人资料");
  expect(screen.getByRole("status")).toHaveTextContent("正在锁定 Wealthlog…");
  expect(screen.queryByRole("button", { name: "Personal" })).not.toBeInTheDocument();
  await act(async () => closing.resolve());
  fireEvent.click(await screen.findByRole("button", { name: "Personal" }));
  await waitFor(() => expect(mocks.reload).toHaveBeenCalledWith({ dashboard: true }));
});
it("keeps failed teardown covered and lets the user retry", async () => {
  mocks.command.mockResolvedValue({
    ...unlocked,
    profiles: [profile, { ...profile, id: "b", name: "Family" }],
  });
  mount();
  await screen.findByText("Private portfolio");
  mocks.command.mockImplementation((command) =>
    command === "lock_profile"
      ? Promise.reject(new Error("Teardown failed"))
      : Promise.resolve(unlocked),
  );
  await menuAction("切换个人资料");
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "出现了问题。请重试。",
  );
  expect(screen.getByRole("status")).toHaveTextContent("无法完成锁定");
  expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Personal" })).not.toBeInTheDocument();
  mocks.command.mockResolvedValue({ ...unlocked, session: null });
  fireEvent.click(screen.getByRole("button", { name: "重试" }));
  expect(
    await screen.findByRole("heading", { name: "谁在使用 Wealthlog？" }),
  ).toBeInTheDocument();
});
it("opens an unprotected profile from the explicitly requested profile picker", async () => {
  mocks.command.mockResolvedValue({
    ...unlocked,
    profiles: [profile, { ...profile, id: "b", name: "Family" }],
  });
  mount();
  await screen.findByText("Private portfolio");
  mocks.command.mockResolvedValue({ ...unlocked, session: null });
  await menuAction("切换个人资料");
  expect(
    await screen.findByRole("heading", { name: "谁在使用 Wealthlog？" }),
  ).toBeInTheDocument();
  expect(mocks.reload).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Personal" }));
  await waitFor(() => expect(mocks.reload).toHaveBeenCalledWith({ dashboard: true }));
});
it("ignores a status read from before a switch started", async () => {
  const old = deferred<typeof unlocked>();
  mocks.command.mockResolvedValue({
    ...unlocked,
    profiles: [profile, { ...profile, id: "b", name: "Family" }],
  });
  mount();
  await screen.findByText("Private portfolio");
  // Hold a real background read, then invalidate it with an explicit switch.
  mocks.command.mockImplementation((command) =>
    command === "get_profile_state" ? old.promise : Promise.resolve(null),
  );
  mocks.command.mockClear();
  act(
    () =>
      void profileChangesChannel?.dispatchEvent(new MessageEvent("message", { data: "changed" })),
  );
  await waitFor(() => expect(mocks.command).toHaveBeenCalledWith("get_profile_state"));
  await menuAction("切换个人资料");
  await screen.findByRole("heading", { name: "谁在使用 Wealthlog？" });
  await act(async () => old.resolve(unlocked));
  expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
});

it("centers the selected profile and lets users go back without retaining credentials", async () => {
  const second = { id: "b", name: "Family", avatarId: "clay-fluff-animated", lockEnabled: true };
  mocks.command.mockResolvedValue({
    profiles: [{ ...profile, lockEnabled: true }, second],
    session: null,
    starting: false,
  });
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "Personal" }));
  fireEvent.change(screen.getByLabelText("密码"), { target: { value: "123456" } });
  expect(screen.getByRole("button", { name: "Personal" })).toHaveAttribute("aria-pressed", "true");
  fireEvent.click(screen.getByRole("button", { name: "返回" }));
  fireEvent.click(await screen.findByRole("button", { name: "Family" }));
  expect(screen.getByLabelText("密码")).toHaveValue("");
  await waitFor(() =>
    expect(screen.queryByRole("button", { name: "Personal" })).not.toBeInTheDocument(),
  );
  expect(screen.queryByText("password required")).not.toBeInTheDocument();
  expect(screen.queryByText("Open profile")).not.toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("密码"), { target: { value: "654321" } });
  fireEvent.click(screen.getByRole("button", { name: "解锁" }));
  await waitFor(() =>
    expect(mocks.command).toHaveBeenCalledWith("unlock_profile", {
      profileId: "b",
      proof: "654321",
    }),
  );
});

it("does not send another profile's password when selecting an unprotected profile", async () => {
  mocks.command.mockResolvedValue({
    profiles: [
      { ...profile, lockEnabled: true },
      {
        id: "b",
        name: "Family",
        avatarId: "clay-fluff-animated",
        lockEnabled: false,
      },
    ],
    session: null,
    starting: false,
  });
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "Personal" }));
  fireEvent.change(screen.getByLabelText("密码"), { target: { value: "123456" } });
  fireEvent.click(screen.getByRole("button", { name: "返回" }));
  fireEvent.click(await screen.findByRole("button", { name: "Family" }));
  await waitFor(() =>
    expect(mocks.command).toHaveBeenCalledWith("unlock_profile", {
      profileId: "b",
      proof: null,
    }),
  );
});

it("keeps password entry hidden after locking until the selected profile is clicked", async () => {
  const protectedState = { ...unlocked, profiles: [{ ...profile, lockEnabled: true }] };
  mocks.command.mockResolvedValue(protectedState);
  mount();
  await screen.findByText("Private portfolio");
  mocks.command.mockResolvedValue({ ...protectedState, session: null });
  await menuAction("锁定 Wealthlog");
  const selected = await screen.findByRole("button", { name: "Personal" });
  expect(selected).toHaveAttribute("aria-pressed", "true");
  expect(screen.queryByLabelText("密码")).not.toBeInTheDocument();
  fireEvent.click(selected);
  expect(screen.getByLabelText("密码")).toHaveAttribute("type", "password");
});

it("keeps password verification on the lock screen and allows retry after an incorrect password", async () => {
  const locked = { ...unlocked, profiles: [{ ...profile, lockEnabled: true }], session: null };
  mocks.command.mockResolvedValue(locked);
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "Personal" }));
  const input = screen.getByLabelText("密码");
  fireEvent.change(input, { target: { value: "111111" } });
  const pending = deferred<void>();
  mocks.command.mockImplementation((command) =>
    command === "unlock_profile" ? pending.promise : Promise.resolve(locked),
  );
  fireEvent.click(screen.getByRole("button", { name: "解锁" }));
  expect(input).toBeInTheDocument();
  expect(input).toBeDisabled();
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
  await waitFor(() =>
    expect(mocks.command).toHaveBeenCalledWith("unlock_profile", {
      profileId: "a",
      proof: "111111",
    }),
  );
  expect(input.closest("main")).toHaveClass("profile-centering");
  await act(async () =>
    pending.reject(
      new Error("PROFILE_PASSWORD_INVALID: The password or recovery code is incorrect."),
    ),
  );
  expect(screen.getByLabelText("密码")).toBe(input);
  expect(input).toHaveAttribute("aria-invalid", "true");
  expect(input).toHaveFocus();
  expect(screen.getByRole("alert")).not.toHaveClass("sr-only");
  expect(screen.getByRole("alert")).toHaveTextContent("密码不正确。请重试。");
  expect(screen.queryByText(/PROFILE_PASSWORD_INVALID/)).not.toBeInTheDocument();
  expect(mocks.reload).not.toHaveBeenCalled();
  fireEvent.change(input, { target: { value: "123456" } });
  expect(input).toHaveAttribute("aria-invalid", "false");
  mocks.command.mockResolvedValue(unlocked.session);
  fireEvent.click(screen.getByRole("button", { name: "解锁" }));
  await waitFor(() => expect(mocks.reload).toHaveBeenCalled());
  expect(screen.getByRole("status")).toHaveTextContent("正在打开 Personal…");
});

it("edits profile details without avatar controls and preserves the stored avatar", async () => {
  mount();
  await menuAction("个人资料设置");
  expect(screen.queryByRole("group", { name: "头像风格" })).not.toBeInTheDocument();
  expect(document.querySelector("form .profile-lock-avatar")).toBeNull();
  fireEvent.change(screen.getByLabelText("名称"), { target: { value: "Renamed" } });
  fireEvent.click(screen.getByRole("button", { name: /^(?:保存(?:更改)?)$/ }));
  await waitFor(() =>
    expect(mocks.command).toHaveBeenCalledWith(
      "update_profile",
      { name: "Renamed", avatarId: profile.avatarId },
      true,
    ),
  );
});

it("only edits the password when its management action is expanded", async () => {
  mount();
  await menuAction("个人资料设置");
  expect(screen.queryByLabelText("新密码")).not.toBeInTheDocument();
  const toggle = screen.getByRole("button", { name: "启用密码" });
  fireEvent.click(toggle);
  fireEvent.change(screen.getByLabelText("新密码"), {
    target: { value: "my passphrase 🔒" },
  });
  fireEvent.change(screen.getByLabelText("再次输入密码"), {
    target: { value: "my passphrase 🔒" },
  });
  fireEvent.click(screen.getByRole("button", { name: "禁用密码" }));
  expect(screen.queryByLabelText("新密码")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: /^(?:保存(?:更改)?)$/ }));
  await waitFor(() => expect(mocks.reload).toHaveBeenCalled());
  expect(mocks.command.mock.calls.some(([command]) => command === "set_profile_password")).toBe(
    false,
  );
});

it("requires proof and removes an existing password only when disabling is saved", async () => {
  const protectedState = { ...unlocked, profiles: [{ ...profile, lockEnabled: true }] };
  mocks.command.mockResolvedValue(protectedState);
  mount();
  await menuAction("个人资料设置");
  fireEvent.click(screen.getByRole("button", { name: "禁用密码" }));
  expect(screen.queryByLabelText("新密码")).not.toBeInTheDocument();
  expect(screen.getByLabelText("当前密码或恢复代码")).toBeRequired();
  expect(mocks.command.mock.calls.some(([command]) => command === "set_profile_password")).toBe(
    false,
  );
  fireEvent.change(screen.getByLabelText("当前密码或恢复代码"), {
    target: { value: "123456" },
  });
  mocks.command.mockImplementation((command) =>
    Promise.resolve(command === "get_profile_state" ? protectedState : null),
  );
  fireEvent.click(screen.getByRole("button", { name: /^(?:保存(?:更改)?)$/ }));
  await waitFor(() =>
    expect(mocks.command).toHaveBeenCalledWith(
      "set_profile_password",
      { proof: "123456", password: null },
      true,
    ),
  );
  await waitFor(() => expect(mocks.reload).toHaveBeenCalledWith({ dashboard: false }));
});

it.each(["setup", "change", "recovery"])(
  "requires matching passwords before any %s mutation",
  async (flow) => {
    const protectedState = {
      ...unlocked,
      profiles: [{ ...profile, lockEnabled: flow !== "setup" }],
      session: flow === "recovery" ? null : unlocked.session,
    };
    mocks.command.mockResolvedValue(protectedState);
    mount();
    if (flow === "recovery") {
      fireEvent.click(await screen.findByRole("button", { name: "Personal" }));
      fireEvent.click(screen.getByRole("button", { name: "忘记密码？" }));
      fireEvent.change(screen.getByLabelText("恢复代码"), {
        target: { value: "RECOVERY-CODE" },
      });
    } else {
      await menuAction("个人资料设置");
      if (flow === "setup")
        fireEvent.click(screen.getByRole("button", { name: "启用密码" }));
      else
        fireEvent.change(screen.getByLabelText("当前密码或恢复代码"), {
          target: { value: "123456" },
        });
    }
    const password = "  My passphrase é🔒  ";
    fireEvent.change(screen.getByLabelText("新密码"), { target: { value: password } });
    fireEvent.change(screen.getByLabelText("再次输入密码"), {
      target: { value: "different password" },
    });
    mocks.command.mockClear();
    fireEvent.click(screen.getByRole("button", { name: /^(?:保存(?:更改)?|重置密码)$/ }));
    expect(await screen.findByText("两次输入的密码不一致。")).toBeInTheDocument();
    expect(screen.getByLabelText("再次输入密码")).toHaveFocus();
    expect(mocks.command).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText("再次输入密码"), { target: { value: password } });
    expect(screen.queryByText("两次输入的密码不一致。")).not.toBeInTheDocument();
    mocks.command.mockImplementation((command) => {
      if (command === "set_profile_password" || command === "recover_profile_password")
        return Promise.resolve("NEW-RECOVERY-CODE");
      if (command === "get_profile_state")
        return Promise.resolve({ ...protectedState, session: null });
      return Promise.resolve(null);
    });
    fireEvent.click(screen.getByRole("button", { name: /^(?:保存(?:更改)?|重置密码)$/ }));
    await screen.findByText("NEW-RECOVERY-CODE");
    if (flow === "recovery") {
      expect(mocks.command).toHaveBeenCalledWith("recover_profile_password", {
        profileId: "a",
        recoveryCode: "RECOVERY-CODE",
        password,
      });
    } else {
      expect(mocks.command).toHaveBeenCalledWith(
        "set_profile_password",
        {
          proof: flow === "setup" ? null : "123456",
          password,
        },
        true,
      );
    }
  },
);

it("removes a password with current proof and reopens without a profile picker", async () => {
  mocks.command.mockResolvedValue({ ...unlocked, profiles: [{ ...profile, lockEnabled: true }] });
  mount();
  await menuAction("个人资料设置");
  fireEvent.change(screen.getByLabelText("新密码"), {
    target: { value: "discard this password" },
  });
  fireEvent.change(screen.getByLabelText("再次输入密码"), {
    target: { value: "different password" },
  });
  fireEvent.click(screen.getByRole("button", { name: "禁用密码" }));
  expect(screen.queryByLabelText("再次输入密码")).not.toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("当前密码或恢复代码"), {
    target: { value: "old password" },
  });
  mocks.command.mockResolvedValue(null);
  fireEvent.click(screen.getByRole("button", { name: /^(?:保存(?:更改)?)$/ }));
  await waitFor(() =>
    expect(mocks.command).toHaveBeenCalledWith(
      "set_profile_password",
      {
        proof: "old password",
        password: null,
      },
      true,
    ),
  );
  await waitFor(() => expect(mocks.reload).toHaveBeenCalledWith({ dashboard: false }));
  expect(mocks.command).toHaveBeenCalledWith("unlock_profile", { profileId: "a", proof: null });
  expect(screen.queryByRole("heading", { name: "谁在使用 Wealthlog？" })).not.toBeInTheDocument();
});

it("deletes the last profile and returns to an empty profile picker", async () => {
  localStorage.setItem("profile:a:chart", "private preference");
  localStorage.setItem("profile:b:chart", "keep");
  mount();
  fireEvent.keyDown(await screen.findByRole("button", { name: "Personal 的个人资料菜单" }), {
    key: "Enter",
  });
  fireEvent.click(await screen.findByRole("menuitem", { name: "个人资料设置" }));
  fireEvent.click(screen.getByRole("button", { name: "删除个人资料" }));
  fireEvent.change(screen.getByLabelText("输入 Personal 以确认"), {
    target: { value: "Personal" },
  });
  mocks.command.mockImplementation(async (command: string) => {
    if (command === "get_profile_state") return { profiles: [], session: null, starting: false };
    return null;
  });
  const buttons = screen.getAllByRole("button", { name: "删除个人资料" });
  fireEvent.click(buttons[buttons.length - 1]);
  expect(await screen.findByRole("button", { name: "创建个人资料" })).toBeInTheDocument();
  expect(localStorage.getItem("profile:a:chart")).toBeNull();
  expect(localStorage.getItem("profile:b:chart")).toBe("keep");
  expect(mocks.command).toHaveBeenCalledWith(
    "delete_profile",
    { profileId: "a", confirmation: "Personal", proof: "" },
    true,
  );
  expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
});

it("can create a protected profile and shows recovery before unlocking it", async () => {
  const locked = { ...unlocked, session: null };
  const created = { id: "b", name: "Family", avatarId: "default", lockEnabled: true };
  mocks.command.mockImplementation((command) =>
    Promise.resolve(
      command === "create_profile" ? { ...created, recoveryCode: "ABCD-1234" } : locked,
    ),
  );
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "添加个人资料" }));
  fireEvent.change(screen.getByLabelText("名称"), { target: { value: "Family" } });
  expect(screen.queryByRole("group", { name: "全部头像" })).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "启用密码" }));
  fireEvent.change(screen.getByLabelText("新密码"), { target: { value: "123" } });
  fireEvent.change(screen.getByLabelText("再次输入密码"), { target: { value: "123" } });
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  const passwordField = screen.getByLabelText("新密码");
  const lengthError = screen.getByRole("alert");
  expect(lengthError).toHaveTextContent("密码长度应为 4–128 个字符。");
  expect(lengthError).toHaveAttribute("id", "profile-password-error");
  expect(passwordField).toHaveAttribute("aria-describedby", lengthError.id);
  expect(passwordField).toHaveAttribute("aria-invalid", "true");
  expect(passwordField).toHaveFocus();
  fireEvent.change(screen.getByLabelText("新密码"), { target: { value: "1234" } });
  expect(screen.queryByText("密码长度应为 4–128 个字符。")).not.toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("再次输入密码"), {
    target: { value: "different password" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  expect(screen.getByText("两次输入的密码不一致。")).toBeInTheDocument();
  expect(mocks.command).not.toHaveBeenCalledWith("create_profile", expect.anything());
  fireEvent.change(screen.getByLabelText("再次输入密码"), {
    target: { value: "1234" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  expect(await screen.findByText("ABCD-1234")).toBeInTheDocument();
  const copy = vi
    .fn()
    .mockRejectedValueOnce(new Error("clipboard unavailable"))
    .mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: copy } });
  fireEvent.click(screen.getByRole("button", { name: "复制恢复代码" }));
  expect(
    await screen.findByText("无法复制代码。请选中代码并手动复制。"),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "复制恢复代码" }));
  expect(await screen.findByText("已复制到剪贴板")).toBeInTheDocument();
  expect(copy).toHaveBeenLastCalledWith("ABCD-1234");

  expect(mocks.command).toHaveBeenCalledWith("create_profile", {
    name: "Family",
    avatarId: "default",
    password: "1234",
  });
  expect(mocks.command).not.toHaveBeenCalledWith("unlock_profile", expect.anything());
  expect(mocks.reload).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "我已保存恢复代码" }));
  await waitFor(() =>
    expect(mocks.command).toHaveBeenCalledWith("unlock_profile", {
      profileId: "b",
      proof: "1234",
    }),
  );
  await waitFor(() => expect(mocks.reload).toHaveBeenCalled());
});

it("keeps password setup optional when creating a profile", async () => {
  const locked = { ...unlocked, session: null };
  mocks.command.mockImplementation((command) =>
    Promise.resolve(
      command === "create_profile"
        ? { id: "b", name: "Family", avatarId: "default", lockEnabled: false, recoveryCode: null }
        : locked,
    ),
  );
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "添加个人资料" }));
  expect(screen.queryByRole("group", { name: "头像风格" })).not.toBeInTheDocument();
  expect(document.querySelector("form .profile-lock-avatar")).toBeNull();
  fireEvent.change(screen.getByLabelText("名称"), { target: { value: "Family" } });
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  await waitFor(() => expect(mocks.reload).toHaveBeenCalled());
  expect(mocks.command).toHaveBeenCalledWith("create_profile", {
    name: "Family",
    avatarId: "default",
    password: null,
  });
  expect(mocks.command).toHaveBeenCalledWith("unlock_profile", { profileId: "b", proof: null });
});

it("shows recovery-code errors inline and returns to the selected profile unlock", async () => {
  const locked = { ...unlocked, session: null, profiles: [{ ...profile, lockEnabled: true }] };
  mocks.command.mockImplementation((command) =>
    command === "recover_profile_password"
      ? Promise.reject(new Error("PROFILE_PASSWORD_INVALID: invalid recovery"))
      : Promise.resolve(locked),
  );
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "Personal" }));
  fireEvent.click(screen.getByRole("button", { name: "忘记密码？" }));
  expect(screen.getByRole("heading", { name: "重置您的密码" })).toBeInTheDocument();
  const code = screen.getByLabelText("恢复代码");
  expect(code).toHaveFocus();
  fireEvent.change(code, { target: { value: "incorrect" } });
  fireEvent.change(screen.getByLabelText("新密码"), { target: { value: "1234" } });
  fireEvent.change(screen.getByLabelText("再次输入密码"), { target: { value: "1234" } });
  fireEvent.click(screen.getByRole("button", { name: "重置密码" }));
  expect(
    await screen.findByText("此恢复代码不正确。请检查后重试。"),
  ).toBeInTheDocument();
  expect(code).toHaveAttribute("aria-describedby", "profile-recovery-error");
  fireEvent.change(code, { target: { value: "replacement" } });
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "返回解锁" }));
  expect(screen.getByLabelText("密码")).toHaveValue("");
  expect(screen.getByRole("button", { name: "解锁" })).toBeInTheDocument();
});

it("offers a prominent create action when there are no profiles", async () => {
  mocks.command.mockResolvedValue({ profiles: [], session: null, starting: false });
  mount();
  const title = await screen.findByRole("heading", { name: "创建您的第一个个人资料" });
  expect(title).not.toHaveClass("sr-only");
  const create = screen.getByRole("button", { name: "创建个人资料" });
  expect(create).not.toHaveClass("profile-lock-add");
  fireEvent.click(create);
  expect(screen.getByRole("heading", { name: "创建个人资料" })).toBeInTheDocument();
  expect(screen.getByLabelText("名称")).toBeInTheDocument();
});

it("keeps the native profile open when switching to another app", async () => {
  mocks.isWeb = false;
  mount();
  expect(await screen.findByText("Private portfolio")).toBeInTheDocument();
  const hidden = vi.spyOn(document, "hidden", "get").mockReturnValue(true);
  try {
    fireEvent(document, new Event("visibilitychange"));
    expect(mocks.command).not.toHaveBeenCalledWith("lock_profile");
    expect(screen.getByText("Private portfolio")).toBeInTheDocument();
  } finally {
    hidden.mockRestore();
  }
});

it("does not poll native profile state after startup settles", async () => {
  vi.useFakeTimers();
  mocks.isWeb = false;
  mount();
  await act(async () => {});
  expect(screen.getByText("Private portfolio")).toBeInTheDocument();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(
    mocks.command.mock.calls.filter(([command]) => command === "get_profile_state"),
  ).toHaveLength(1);
});

it("reloads after native session subscription failure without admitting financial content", async () => {
  mocks.isWeb = false;
  const { listen } = await import("@tauri-apps/api/event");
  vi.mocked(listen).mockRejectedValueOnce(new Error("Session subscription failed"));
  mount();
  const retry = await screen.findByRole("button", { name: "重试" });
  expect(listen).toHaveBeenCalledWith("profile-session-changed", expect.any(Function));
  expect(mocks.command).not.toHaveBeenCalled();
  expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
  fireEvent.click(retry);
  await waitFor(() => expect(mocks.reload).toHaveBeenCalledTimes(1));
  expect(mocks.command).not.toHaveBeenCalled();
  expect(mocks.admitted).not.toHaveBeenCalled();
  expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
});

it("uses native startup and lock events without polling", async () => {
  vi.useFakeTimers();
  mocks.isWeb = false;
  mocks.command.mockResolvedValueOnce({ profiles: [profile], session: null, starting: true });
  mount();
  await act(async () => {});
  expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(10_000);
  });
  expect(mocks.command).toHaveBeenCalledTimes(1);
  await act(async () => mocks.ready());
  expect(screen.getByText("Private portfolio")).toBeInTheDocument();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(
    mocks.command.mock.calls.filter(([command]) => command === "get_profile_state"),
  ).toHaveLength(2);
  mocks.command.mockResolvedValue({ ...unlocked, session: null });
  await act(async () => {
    mocks.changed();
  });
  expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "谁在使用 Wealthlog？" })).toBeInTheDocument();
});

const profileStateReads = () =>
  mocks.command.mock.calls.filter(([command]) => command === "get_profile_state").length;

it.each([true, false])("never polls web profile state (active: %s)", async (active) => {
  vi.useFakeTimers();
  mocks.command.mockResolvedValue({ ...unlocked, session: active ? unlocked.session : null });
  mount();
  await act(async () => {});
  await act(async () => {
    await vi.advanceTimersByTimeAsync(300_000);
  });
  expect(profileStateReads()).toBe(1);
});

it("refreshes only when visibility changes to visible", async () => {
  const visibility = vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
  try {
    mount();
    await act(async () => {});
    await act(async () => document.dispatchEvent(new Event("visibilitychange")));
    expect(profileStateReads()).toBe(1);
    visibility.mockReturnValue("visible");
    await act(async () => document.dispatchEvent(new Event("visibilitychange")));
    expect(profileStateReads()).toBe(2);
  } finally {
    visibility.mockRestore();
  }
});

it.each([
  ["web session admitted", true, true, 1],
  ["web session rejected", true, false, 0],
  ["desktop session", false, true, 0],
])(
  "keeps the profile event stream open on every route (%s)",
  async (_name, isWeb, admitted, calls) => {
    mocks.isWeb = isWeb;
    mocks.admitted.mockReturnValue(admitted);
    mocks.command.mockResolvedValue(unlocked);
    mocks.listen.mockClear();
    mount();
    await waitFor(() => expect(profileStateReads()).toBeGreaterThanOrEqual(1));
    await act(async () => {});
    expect(mocks.listen).toHaveBeenCalledTimes(calls);
    if (calls) expect(mocks.listen).toHaveBeenCalledWith(expect.any(Function));
    mocks.admitted.mockReturnValue(true);
  },
);

it("closes locally without locking the server when another tab already closed the session", async () => {
  mount();
  await screen.findByText("Private portfolio");
  mocks.command.mockClear();
  mocks.command.mockResolvedValue({ ...unlocked, session: null });
  await act(async () => {
    profileChangesChannel?.dispatchEvent(new MessageEvent("message", { data: "changed" }));
  });
  expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "谁在使用 Wealthlog？" })).toBeInTheDocument();
  expect(mocks.command).not.toHaveBeenCalledWith("lock_profile");
});

it("keeps another tab's unlock queued behind a read that observed its lock", async () => {
  mount();
  await screen.findByText("Private portfolio");
  const closed = deferred<Omit<typeof unlocked, "session"> & { session: null }>();
  mocks.command.mockClear();
  mocks.command.mockReturnValueOnce(closed.promise).mockResolvedValue(unlocked);
  // The other tab's lock and unlock both arrive while the first read is in flight.
  await act(async () => {
    profileChangesChannel?.dispatchEvent(new MessageEvent("message", { data: "changed" }));
    profileChangesChannel?.dispatchEvent(new MessageEvent("message", { data: "changed" }));
  });
  await act(async () => closed.resolve({ ...unlocked, session: null }));
  await waitFor(() => expect(profileStateReads()).toBeGreaterThanOrEqual(2));
  expect(mocks.command).not.toHaveBeenCalledWith("lock_profile");
  expect(await screen.findByText("Private portfolio")).toBeInTheDocument();
});

it("follows another tab's unlock from the lock screen and unsubscribes on unmount", async () => {
  mocks.command.mockResolvedValue({ ...unlocked, session: null });
  const view = mount();
  await act(async () => {});
  expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
  mocks.command.mockResolvedValue(unlocked);
  await act(async () => {
    profileChangesChannel?.dispatchEvent(new MessageEvent("message", { data: "changed" }));
  });
  expect(profileStateReads()).toBe(2);
  expect(screen.getByText("Private portfolio")).toBeInTheDocument();
  view.unmount();
  profileChangesChannel?.dispatchEvent(new MessageEvent("message", { data: "changed" }));
  expect(profileStateReads()).toBe(2);
});

it("re-reads a cross-tab unlock received during an in-flight locked-state read", async () => {
  const pending = deferred<Omit<typeof unlocked, "session"> & { session: null }>();
  mocks.command.mockReturnValueOnce(pending.promise);
  mount();
  await act(async () => {
    profileChangesChannel?.dispatchEvent(new MessageEvent("message", { data: "changed" }));
    profileChangesChannel?.dispatchEvent(new MessageEvent("message", { data: "changed" }));
  });
  expect(profileStateReads()).toBe(1);
  await act(async () => pending.resolve({ ...unlocked, session: null }));
  expect(profileStateReads()).toBe(2);
  expect(screen.getByText("Private portfolio")).toBeInTheDocument();
});

it.each(["online", "wealthfolio:event-stream-error"])(
  "re-reads web profile state on %s",
  async (event) => {
    vi.useFakeTimers();
    mount();
    await act(async () => {});
    expect(profileStateReads()).toBe(1);
    await act(async () => {
      window.dispatchEvent(new Event(event));
    });
    expect(profileStateReads()).toBe(2);
  },
);

it.each([true, false])(
  "keeps an open web session visible through an outage without locking (protected: %s)",
  async (lockEnabled) => {
    const active = { ...unlocked, profiles: [{ ...profile, lockEnabled }] };
    mocks.command.mockResolvedValue(active);
    mount();
    await screen.findByText("Private portfolio");
    mocks.command.mockRejectedValue(new TypeError("Failed to fetch"));
    await act(async () => window.dispatchEvent(new Event("wealthfolio:event-stream-error")));
    expect(screen.getByText("Private portfolio")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "重试" })).not.toBeInTheDocument();
    expect(mocks.command).not.toHaveBeenCalledWith("lock_profile");
    mocks.command.mockResolvedValue(active);
    await act(async () => window.dispatchEvent(new Event("online")));
    expect(await screen.findByText("Private portfolio")).toBeInTheDocument();
    expect(mocks.admitted).toHaveBeenLastCalledWith(active.session, undefined);
    expect(mocks.reload).not.toHaveBeenCalled();
  },
);

it("keeps an expired profile closed after reconnect without issuing another lock", async () => {
  const protectedState = { ...unlocked, profiles: [{ ...profile, lockEnabled: true }] };
  mocks.command.mockResolvedValue(protectedState);
  mount();
  await screen.findByText("Private portfolio");
  mocks.command.mockRejectedValue(new TypeError("Failed to fetch"));
  await act(async () => window.dispatchEvent(new Event("wealthfolio:event-stream-error")));
  mocks.command.mockResolvedValue({ ...protectedState, session: null });
  await act(async () => window.dispatchEvent(new Event("online")));
  expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
  expect(
    await screen.findByRole("heading", { name: "谁在使用 Wealthlog？" }),
  ).toBeInTheDocument();
  expect(mocks.command).not.toHaveBeenCalledWith("lock_profile");
});

it("offers sign-in recovery when an external authentication proxy interrupts profile verification", async () => {
  mocks.command.mockRejectedValue(new Error("PROFILE_AUTH_REQUIRED"));
  mount();
  const signIn = await screen.findByRole("button", { name: "重新登录" });
  expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
  fireEvent.click(signIn);
  expect(mocks.reload).toHaveBeenCalledOnce();
  expect(mocks.command).not.toHaveBeenCalledWith("lock_profile");
});

it("shows cooldown feedback below the unlock button and keeps the password form available", async () => {
  const locked = { ...unlocked, profiles: [{ ...profile, lockEnabled: true }], session: null };
  mocks.command.mockImplementation((command) =>
    command === "unlock_profile"
      ? Promise.reject(new Error('"PROFILE_COOLDOWN: Try again in 30 seconds."'))
      : Promise.resolve(locked),
  );
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "Personal" }));
  const input = screen.getByLabelText("密码");
  fireEvent.change(input, { target: { value: "wrong password" } });
  const button = screen.getByRole("button", { name: "解锁" });
  fireEvent.click(button);
  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent("尝试次数过多。请在 30 秒后重试。");
  expect(alert.closest("form")).toBe(input.closest("form"));
  expect(button.compareDocumentPosition(alert) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(input).toHaveAccessibleDescription("尝试次数过多。请在 30 秒后重试。");
  expect(screen.queryByText(/PROFILE_COOLDOWN/)).not.toBeInTheDocument();
  fireEvent.change(input, { target: { value: "another password" } });
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

it("uses safe form feedback when creating a profile fails unexpectedly", async () => {
  mocks.command.mockImplementation((command) =>
    command === "create_profile"
      ? Promise.reject(new Error("database /private/internal/profile.db unavailable"))
      : Promise.resolve({ ...unlocked, profiles: [], session: null }),
  );
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "创建个人资料" }));
  fireEvent.change(screen.getByLabelText("名称"), { target: { value: "Test" } });
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent("出现了问题。请重试。");
  expect(alert.closest("form")).not.toBeNull();
  expect(screen.queryByText(/private\/internal/)).not.toBeInTheDocument();
});

it("rereads native startup when readiness arrives during the initial request", async () => {
  mocks.isWeb = false;
  const initial = deferred<unknown>();
  mocks.command.mockReturnValueOnce(initial.promise);
  mount();
  await waitFor(() => expect(mocks.command).toHaveBeenCalledOnce());
  await act(async () => {
    mocks.ready();
    initial.resolve({ profiles: [profile], session: null, starting: true });
  });
  expect(await screen.findByText("Private portfolio")).toBeInTheDocument();
  expect(mocks.command).toHaveBeenCalledTimes(2);
});

it("refreshes native admission after database maintenance while content is covered", async () => {
  mocks.isWeb = false;
  mount();
  await screen.findByText("Private portfolio");
  // A scoped read was rejected during teardown. Backend still holds the old
  // session until maintenance completes and it can issue its replacement.
  mocks.admitted.mockReturnValueOnce(false);
  await act(async () => mocks.changed());
  expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
  const replacement = { profileId: "a", scopeId: "rebuilt" };
  mocks.command.mockResolvedValue({ ...unlocked, session: replacement });
  await act(async () => mocks.databaseChanged());
  expect(mocks.admitted).toHaveBeenLastCalledWith(replacement, undefined);
  expect(screen.getByText("Private portfolio")).toBeInTheDocument();
});

it.each([true, false])("adds a second profile directly from the menu (web: %s)", async (isWeb) => {
  mocks.isWeb = isWeb;
  mount();
  await screen.findByText("Private portfolio");
  fireEvent.keyDown(screen.getByRole("button", { name: "Personal 的个人资料菜单" }), {
    key: "Enter",
  });
  fireEvent.click(await screen.findByRole("menuitem", { name: "添加个人资料" }));
  expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
  expect(mocks.command).toHaveBeenCalledWith("lock_profile");
  expect(await screen.findByRole("heading", { name: "创建个人资料" })).toBeInTheDocument();
  expect(screen.queryByRole("heading", { name: "谁在使用 Wealthlog？" })).not.toBeInTheDocument();
});
it("does not open profile creation when closing the current profile fails", async () => {
  mount();
  await screen.findByText("Private portfolio");
  fireEvent.keyDown(screen.getByRole("button", { name: "Personal 的个人资料菜单" }), {
    key: "Enter",
  });
  const add = await screen.findByRole("menuitem", { name: "添加个人资料" });
  mocks.command.mockRejectedValue(new Error("Unable to close profile"));
  fireEvent.click(add);
  expect(await screen.findByText("无法完成锁定")).toBeInTheDocument();
  expect(screen.queryByRole("heading", { name: "创建个人资料" })).not.toBeInTheDocument();
});

it.each([true, false])(
  "mobile profile selection waits for closing (success: %s)",
  async (closeSucceeds) => {
    const family = { ...profile, id: "b", name: "Family" };
    let finishClose!: () => void;
    const closing = new Promise<void>((resolve, reject) => {
      finishClose = () => (closeSucceeds ? resolve() : reject(new Error("Close failed")));
    });
    mocks.command.mockImplementation(async (command: string) => {
      if (command === "lock_profile") return closing;
      return { ...unlocked, profiles: [profile, family] };
    });
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ProfileShell>
          <MobileProfileMenu onAction={() => {}} />
          <div>Private portfolio</div>
        </ProfileShell>
      </QueryClientProvider>,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Family" }));
    expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
    expect(mocks.command.mock.calls.some(([name]) => name === "unlock_profile")).toBe(false);
    await act(async () => finishClose());
    if (closeSucceeds) {
      await waitFor(() =>
        expect(mocks.command).toHaveBeenCalledWith("unlock_profile", {
          profileId: "b",
          proof: null,
        }),
      );
      await waitFor(() => expect(mocks.reload).toHaveBeenCalledWith({ dashboard: true }));
    } else {
      expect(mocks.command.mock.calls.some(([name]) => name === "unlock_profile")).toBe(false);
      expect(mocks.reload).not.toHaveBeenCalled();
    }
  },
);

it("shows a retryable web runtime failure without mounting financial content", async () => {
  mocks.command.mockRejectedValue(new Error("PROFILE_STARTUP_FAILED: private database detail"));
  mount();
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "服务器无法打开此配置文件。请重试，或请服务器管理员检查启动错误。",
  );
  expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
  expect(screen.queryByText(/private database detail/)).not.toBeInTheDocument();
  mocks.command.mockResolvedValue(unlocked);
  fireEvent.click(screen.getByRole("button", { name: /^重试$/ }));
  expect(await screen.findByText("Private portfolio")).toBeInTheDocument();
  expect(mocks.command).not.toHaveBeenCalledWith("lock_profile");
});

it("offers a fresh-page retry when a revoked web session is still returned by the server", async () => {
  mount();
  await screen.findByText("Private portfolio");
  mocks.admitted.mockReturnValue(false);
  try {
    await act(async () => window.dispatchEvent(new Event("wealthfolio:profile-locked")));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "您的配置文件会话已中断。请重试以重新打开。",
    );
    expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
    expect(mocks.reload).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: /^重试$/ }));
    expect(mocks.reload).toHaveBeenCalledOnce();
    expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
    expect(mocks.command).not.toHaveBeenCalledWith("unlock_profile", expect.anything());
  } finally {
    mocks.admitted.mockReturnValue(true);
  }
});

// Invoke the registered DOM listener with trusted activity; synthetic DOM dispatches
// intentionally cannot extend a real profile's idle deadline.
async function activityListener(lockEnabled: boolean) {
  const registered = vi.spyOn(window, "addEventListener");
  mocks.command.mockResolvedValue({ ...unlocked, profiles: [{ ...profile, lockEnabled }] });
  mount();
  await act(async () => {});
  const listener = registered.mock.calls.find(([event]) => event === "pointerdown")?.[1];
  registered.mockRestore();
  return listener as ((event: Event) => void) | undefined;
}

it.each([true, false])(
  "does not register activity updates for an unprotected profile (web: %s)",
  async (isWeb) => {
    mocks.isWeb = isWeb;
    expect(await activityListener(false)).toBeUndefined();
    expect(mocks.command.mock.calls.some(([command]) => command === "profile_activity")).toBe(
      false,
    );
  },
);

it.each([true, false])(
  "throttles protected-profile activity to thirty seconds (web: %s)",
  async (isWeb) => {
    vi.useFakeTimers();
    mocks.isWeb = isWeb;
    const activity = (await activityListener(true))!;
    const trusted = { isTrusted: true } as Event;
    await act(async () => activity(trusted));
    await act(async () => activity(trusted));
    const updates = () =>
      mocks.command.mock.calls.filter(([command]) => command === "profile_activity");
    expect(updates()).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(29_999);
    await act(async () => activity(trusted));
    expect(updates()).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    await act(async () => activity(trusted));
    expect(updates()).toHaveLength(2);
  },
);

it("leaves failed activity updates alone without another state request or revoking a grant", async () => {
  const activity = (await activityListener(true))!;
  mocks.command.mockImplementation((command) =>
    command === "profile_activity"
      ? Promise.reject(new TypeError("Failed to fetch"))
      : Promise.resolve({ ...unlocked, profiles: [{ ...profile, lockEnabled: true }] }),
  );
  await act(async () => activity({ isTrusted: true } as Event));
  expect(screen.getByText("Private portfolio")).toBeInTheDocument();
  expect(
    mocks.command.mock.calls.filter(([command]) => command === "get_profile_state"),
  ).toHaveLength(1);
  expect(mocks.command).not.toHaveBeenCalledWith("lock_profile");
  expect(mocks.reload).not.toHaveBeenCalled();
});

it("does not accept a late activity rejection after a lock has begun", async () => {
  const activity = (await activityListener(true))!;
  const pending = deferred<unknown>();
  mocks.command.mockImplementation((command) =>
    command === "profile_activity" ? pending.promise : Promise.resolve(unlocked),
  );
  await act(async () => activity({ isTrusted: true } as Event));
  await act(async () => window.dispatchEvent(new Event("wealthfolio:profile-locked")));
  const count = mocks.command.mock.calls.length;
  await act(async () => pending.reject(new Error("PROFILE_LOCKED")));
  expect(mocks.command.mock.calls).toHaveLength(count);
});

it("preserves a never-admitted unlock form when visibility refresh confirms no session", async () => {
  mocks.command.mockResolvedValue({
    ...unlocked,
    profiles: [{ ...profile, lockEnabled: true }],
    session: null,
  });
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "Personal" }));
  fireEvent.change(screen.getByLabelText("密码"), { target: { value: "unfinished password" } });
  await act(async () => document.dispatchEvent(new Event("visibilitychange")));
  expect(screen.getByLabelText("密码")).toHaveValue("unfinished password");
  expect(mocks.command).not.toHaveBeenCalledWith("lock_profile");
});

it("resumes after an SSE reconnect without relying on a browser online event", async () => {
  mount();
  await screen.findByText("Private portfolio");
  mocks.command.mockRejectedValue(new Error("upstream unavailable"));
  await act(async () => window.dispatchEvent(new Event("wealthfolio:event-stream-error")));
  expect(screen.getByText("Private portfolio")).toBeInTheDocument();
  mocks.command.mockResolvedValue(unlocked);
  await act(async () => window.dispatchEvent(new Event("wealthfolio:event-stream-reconnected")));
  expect(screen.getByText("Private portfolio")).toBeInTheDocument();
  expect(mocks.reload).not.toHaveBeenCalled();
  expect(mocks.command).not.toHaveBeenCalledWith("lock_profile");
});

it.each(["锁定 Wealthlog", "切换个人资料"])(
  "keeps content hidden when %s fails offline",
  async (action) => {
    mocks.command.mockResolvedValue({
      ...unlocked,
      profiles: [
        { ...profile, lockEnabled: true },
        { ...profile, id: "b", name: "Family" },
      ],
    });
    mount();
    await screen.findByText("Private portfolio");
    fireEvent.keyDown(screen.getByRole("button", { name: "Personal 的个人资料菜单" }), {
      key: "Enter",
    });
    const item = await screen.findByRole("menuitem", { name: action });
    mocks.command.mockRejectedValue(new TypeError("Failed to fetch"));
    fireEvent.click(item);
    expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
    await screen.findByRole("button", { name: "重试" });
    await act(async () => window.dispatchEvent(new Event("online")));
    expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
  },
);

it("keeps startup closed when the initial profile check fails", async () => {
  mocks.command.mockRejectedValue(new TypeError("Failed to fetch"));
  mount();
  await screen.findByRole("button", { name: "重试" });
  expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
});

it.each([false, true])(
  "preserves the selected profile and unlock input through a connection failure (switching: %s)",
  async (switching) => {
    const profiles = [
      { ...profile, lockEnabled: true },
      { ...profile, id: "b", name: "Family", lockEnabled: true },
    ];
    const locked = { ...unlocked, profiles, session: null };
    mocks.command.mockResolvedValue(switching ? { ...unlocked, profiles } : locked);
    mount();
    if (switching) {
      await screen.findByText("Private portfolio");
      fireEvent.keyDown(screen.getByRole("button", { name: "Personal 的个人资料菜单" }), {
        key: "Enter",
      });
      mocks.command.mockResolvedValue(locked);
      fireEvent.click(await screen.findByRole("menuitem", { name: "切换个人资料" }));
    }
    fireEvent.click(await screen.findByRole("button", { name: "Family" }));
    const input = screen.getByLabelText("密码");
    fireEvent.change(input, { target: { value: "unfinished password" } });
    const callsBeforeOutage = mocks.command.mock.calls.length;
    mocks.command.mockRejectedValue(new TypeError("Failed to fetch"));
    await act(async () => window.dispatchEvent(new Event("wealthfolio:event-stream-error")));
    expect(screen.getByLabelText("密码")).toBe(input);
    expect(input).toHaveValue("unfinished password");
    expect(screen.queryByText("Private portfolio")).not.toBeInTheDocument();
    expect(
      mocks.command.mock.calls
        .slice(callsBeforeOutage)
        .every(([name]) => name === "get_profile_state"),
    ).toBe(true);
    mocks.command.mockResolvedValue(locked);
    await act(async () => window.dispatchEvent(new Event("online")));
    expect(screen.getByLabelText("密码")).toBe(input);
    expect(input).toHaveValue("unfinished password");
    fireEvent.submit(input.closest("form")!);
    await waitFor(() =>
      expect(mocks.command).toHaveBeenCalledWith("unlock_profile", {
        profileId: "b",
        proof: "unfinished password",
      }),
    );
  },
);

it("does not send state requests when going offline or receiving an offline stream error", async () => {
  mount();
  await screen.findByText("Private portfolio");
  const before = profileStateReads();
  vi.spyOn(navigator, "onLine", "get").mockReturnValue(false);
  try {
    await act(async () => {
      window.dispatchEvent(new Event("offline"));
      window.dispatchEvent(new Event("wealthfolio:event-stream-error"));
    });
    expect(profileStateReads()).toBe(before);
    expect(screen.getByText("Private portfolio")).toBeInTheDocument();
  } finally {
    vi.restoreAllMocks();
  }
});
