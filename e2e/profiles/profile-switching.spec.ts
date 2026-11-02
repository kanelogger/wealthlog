import { test, expect, type BrowserContext, type Page } from "@playwright/test";

async function command(context: BrowserContext, name: string, data = {}, scope?: string) {
  const result = await context.request.post(`/api/v1/profiles/${name}`, {
    data,
    headers: scope ? { "x-wf-profile-scope": scope } : {},
  });
  expect(result.ok(), `Profile command ${name}: ${result.status()}`).toBeTruthy();
  return result.json();
}
async function settings(context: BrowserContext, scope: string, theme: string, font: string) {
  const result = await context.request.put("/api/v1/settings", {
    headers: { "x-wf-profile-scope": scope },
    data: { onboardingCompleted: true, theme, font, language: "en", syncEnabled: false },
  });
  expect(result.ok()).toBeTruthy();
}
async function ready(page: Page, name: string) {
  await expect(page.locator(".app-shell").first()).toBeVisible();
  await expect(page.getByRole("button", { name: `${name} 的个人资料菜单` })).toBeVisible();
}
async function switchProfile(page: Page, current: string, next: string) {
  await page.getByRole("button", { name: `${current} 的个人资料菜单` }).click();
  await page.getByRole("menuitem", { name: "切换个人资料", exact: true }).click();
  await expect(page.locator(".app-shell").first()).not.toBeVisible();
  await expect(page.getByRole("heading", { name: "谁在使用 Wealthlog？" })).toBeVisible();
  await page.screenshot({
    path: `/tmp/wealthfolio-profile-chooser-${current === "Profile B" ? "dark" : "light"}.png`,
  });
  await page.getByRole("button", { name: next, exact: true }).click();
  await ready(page, next);
}

test("real A → B → A reloads, different appearance, cross-tab route reset, and mobile lock", async ({
  page,
  context,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const initial = await command(context, "get_profile_state");
  const a = initial.profiles[0];
  let grant = initial.session ?? (await command(context, "unlock_profile", { profileId: a.id }));
  await command(
    context,
    "update_profile",
    { name: "Profile A", avatarId: "clay-pebble-animated" },
    grant.scopeId,
  );
  await settings(context, grant.scopeId, "light", "font-sans");
  const b = await command(context, "create_profile", {
    name: "Profile B",
    avatarId: "clay-fluff-animated",
  });
  grant = await command(context, "unlock_profile", { profileId: b.id });
  await settings(context, grant.scopeId, "dark", "font-serif");
  await command(context, "unlock_profile", { profileId: a.id });
  await page.goto("/activities");
  await ready(page, "Profile A");
  const observer = await context.newPage();
  await observer.goto("/settings/accounts");
  await ready(observer, "Profile A");
  await switchProfile(page, "Profile A", "Profile B");
  await expect(page).toHaveURL(/\/activities$/);
  await expect(page.locator("html")).toHaveClass(/dark/);
  await expect(page.locator("body")).toHaveClass(/font-serif/);
  await ready(observer, "Profile B");
  await expect(observer).toHaveURL(/\/activities$/);
  await observer.close();
  await switchProfile(page, "Profile B", "Profile A");
  await expect(page.locator("html")).toHaveClass(/light/);
  await expect(page.locator("body")).toHaveClass(/font-sans/);

  const active = await command(context, "get_profile_state");
  await command(
    context,
    "set_profile_password",
    { proof: null, password: "mobile passphrase" },
    active.session.scopeId,
  );
  await command(context, "unlock_profile", { profileId: a.id, proof: "mobile passphrase" });
  await page.reload();
  await ready(page, "Profile A");
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole("button", { name: "更多选项" }).click();
  await page.getByRole("button", { name: "Profile A 的个人资料菜单" }).click();
  await expect(page.getByRole("menu")).not.toBeVisible();
  await expect(page.getByRole("dialog")).toHaveCount(1);
  await expect(page.getByRole("button", { name: "Profile A", exact: true })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect(page.getByRole("button", { name: "Profile B", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "添加个人资料", exact: true })).toBeVisible();
  await page.screenshot({ path: "/tmp/wealthfolio-mobile-profile-menu.png" });
  await page.getByRole("button", { name: "返回", exact: true }).click();
  await expect(page.getByRole("button", { name: "Profile A 的个人资料菜单" })).toBeFocused();
  await page.getByRole("button", { name: "Profile A 的个人资料菜单" }).click();
  await page.getByRole("button", { name: "关闭菜单" }).click();
  await page.getByRole("button", { name: "更多选项" }).click();
  await expect(page.getByRole("heading", { name: "更多", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Profile A 的个人资料菜单" }).click();
  await page.getByRole("button", { name: "锁定 Wealthlog" }).click();
  await expect(page.locator(".app-shell").first()).not.toBeVisible();
  await expect(page.getByRole("heading", { name: "谁在使用 Wealthlog？" })).toBeVisible();
  await expect(page.getByRole("dialog")).not.toBeVisible();
  await page.screenshot({ path: "/tmp/wealthfolio-profile-lock-mobile.png" });
  await page.getByRole("button", { name: "Profile A", exact: true }).click();
  await page.getByLabel("密码", { exact: true }).fill("mobile passphrase");
  await page.getByRole("button", { name: "解锁", exact: true }).click();
  await expect(page.locator(".app-shell").first()).toBeVisible();
  await expect(page.getByRole("button", { name: "更多选项" })).toBeVisible();
  await page.getByRole("button", { name: "更多选项" }).click();
  await page.getByRole("button", { name: "Profile A 的个人资料菜单" }).click();
  await page.getByRole("button", { name: "Profile B", exact: true }).click();
  await expect(page.locator("body")).toHaveClass(/font-serif/);
  await page.getByRole("button", { name: "更多选项" }).click();
  await page.getByRole("button", { name: "Profile B 的个人资料菜单" }).click();
  await page.getByRole("button", { name: "Profile A", exact: true }).click();
  await page.getByLabel("密码", { exact: true }).fill("mobile passphrase");
  await page.getByRole("button", { name: "解锁", exact: true }).click();
  await expect(page.locator("body")).toHaveClass(/font-sans/);
  expect(errors).toEqual([]);
  const unlocked = await command(context, "get_profile_state");
  await command(
    context,
    "set_profile_password",
    { proof: "mobile passphrase", password: null },
    unlocked.session.scopeId,
  );
});

test("incorrect password stays on the chooser and can be retried", async ({ page, context }) => {
  const state = await command(context, "get_profile_state");
  const profile =
    state.profiles.find((item: { id: string }) => item.id === state.session?.profileId) ??
    state.profiles[0];
  const grant =
    state.session ?? (await command(context, "unlock_profile", { profileId: profile.id }));
  await settings(context, grant.scopeId, "light", "font-sans");
  await command(
    context,
    "set_profile_password",
    { proof: null, password: "correct passphrase" },
    grant.scopeId,
  );
  await command(context, "lock_profile", { preserveAuth: false });
  await page.goto("/");
  await expect(page.getByLabel("密码", { exact: true })).not.toBeVisible();
  await page.getByRole("button", { name: profile.name, exact: true }).click();
  const password = page.getByLabel("密码", { exact: true });
  await password.fill("111111");
  await page.getByRole("button", { name: "解锁", exact: true }).click();
  await expect(password).toHaveAttribute("aria-invalid", "true");
  await expect(password).toBeFocused();
  await expect(page.getByText(/PROFILE_PASSWORD_INVALID/)).not.toBeVisible();
  await page.screenshot({ path: "/tmp/wealthfolio-invalid-password.png" });
  await password.fill("correct passphrase");
  await page.getByRole("button", { name: "解锁", exact: true }).click();
  await ready(page, profile.name);
});

test("profile settings keeps name and password controls usable on desktop and mobile", async ({
  page,
  context,
}) => {
  const state = await command(context, "get_profile_state");
  const profile = state.profiles.find((item: { lockEnabled: boolean }) => !item.lockEnabled);
  const grant = await command(context, "unlock_profile", { profileId: profile.id });
  await settings(context, grant.scopeId, "light", "font-sans");
  await page.goto("/");
  await ready(page, profile.name);
  await page.getByRole("button", { name: `${profile.name} 的个人资料菜单` }).click();
  await page.getByRole("menuitem", { name: "个人资料设置" }).click();
  const profileForm = page.locator("form").filter({
    has: page.getByLabel("名称", { exact: true }),
  });
  await expect(profileForm).toHaveCount(1);
  await expect(page.getByRole("group", { name: "全部头像" })).toHaveCount(0);
  await expect(page.getByRole("group", { name: "头像风格" })).toHaveCount(0);
  await expect(profileForm.locator(".profile-lock-avatar")).toHaveCount(0);
  await expect(page.getByLabel("名称", { exact: true })).toHaveValue(profile.name);
  await page.getByRole("button", { name: "启用密码" }).click();
  const nameBounds = await page.getByLabel("名称", { exact: true }).boundingBox();
  const passwordBounds = await page.getByLabel("新密码", { exact: true }).boundingBox();
  expect(nameBounds).not.toBeNull();
  expect(passwordBounds).not.toBeNull();
  expect(passwordBounds!.x).toBe(nameBounds!.x);
  expect(passwordBounds!.width).toBe(nameBounds!.width);
  const confirmation = page.getByLabel("再次输入密码", { exact: true });
  const confirmationBounds = await confirmation.boundingBox();
  expect(confirmationBounds).not.toBeNull();
  expect(confirmationBounds!.x).toBe(passwordBounds!.x);
  expect(confirmationBounds!.width).toBe(passwordBounds!.width);
  const save = page.getByRole("button", { name: "保存更改", exact: true });
  await expect(save).toHaveCount(1);
  await expect(profileForm.getByRole("button", { name: "保存更改", exact: true })).toHaveCount(1);
  const saveBounds = await save.boundingBox();
  const formBounds = await profileForm.boundingBox();
  expect(saveBounds).not.toBeNull();
  expect(formBounds).not.toBeNull();
  expect(saveBounds!.x).toBeGreaterThanOrEqual(formBounds!.x);
  expect(saveBounds!.x + saveBounds!.width).toBeLessThanOrEqual(formBounds!.x + formBounds!.width);
  expect(saveBounds!.y).toBeGreaterThan(confirmationBounds!.y + confirmationBounds!.height);
  expect(saveBounds!.y + saveBounds!.height).toBeLessThanOrEqual(
    formBounds!.y + formBounds!.height,
  );
  await page.getByLabel("新密码", { exact: true }).fill("a new passphrase 🔒");
  await confirmation.fill("does not match");
  await page.getByRole("button", { name: "保存更改", exact: true }).click();
  await expect(page.getByText("两次输入的密码不一致。")).toBeVisible();
  await expect(confirmation).toBeFocused();
  await confirmation.fill("a new passphrase 🔒");
  await expect(page.getByText("两次输入的密码不一致。")).not.toBeVisible();
  await page.screenshot({ path: "/tmp/wealthfolio-profile-settings-desktop.png", fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.getByLabel("名称", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "保存更改", exact: true }).scrollIntoViewIfNeeded();
  await page.screenshot({ path: "/tmp/wealthfolio-profile-settings-mobile.png", fullPage: true });
  await settings(context, grant.scopeId, "dark", "font-sans");
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.reload();
  await ready(page, profile.name);
  await page.getByRole("button", { name: `${profile.name} 的个人资料菜单` }).click();
  await page.getByRole("menuitem", { name: "个人资料设置" }).click();
  await expect(page.locator("html")).toHaveClass(/dark/);
  await expect(page.getByLabel("名称", { exact: true })).toBeVisible();
  await page.screenshot({ path: "/tmp/wealthfolio-profile-settings-dark.png", fullPage: true });
});

test("sets, changes, and recovers a password with confirmation", async ({ page, context }) => {
  const profile = await command(context, "create_profile", {
    name: "Password profile",
    avatarId: "clay-bot-animated",
  });
  const grant = await command(context, "unlock_profile", { profileId: profile.id });
  await settings(context, grant.scopeId, "light", "font-sans");
  await page.goto("/");
  await ready(page, profile.name);
  let previousPassword = "";
  let recoveryCode = "";
  for (const flow of ["setup", "change", "recovery"]) {
    await page.getByRole("button", { name: `${profile.name} 的个人资料菜单` }).click();
    if (flow === "recovery") {
      await page.getByRole("menuitem", { name: "锁定 Wealthlog" }).click();
      await page.getByRole("button", { name: profile.name, exact: true }).click();
      await page.getByRole("button", { name: "忘记密码？" }).click();
      await page.getByLabel("恢复代码", { exact: true }).fill(recoveryCode);
    } else {
      await page.getByRole("menuitem", { name: "个人资料设置" }).click();
      if (flow === "setup") await page.getByRole("button", { name: "启用密码" }).click();
      else await page.getByLabel("当前密码或恢复代码", { exact: true }).fill(previousPassword);
    }
    const password = ` ${flow} passphrase é🔒 `;
    await page.getByLabel("新密码", { exact: true }).fill(password);
    await page.getByLabel("再次输入密码", { exact: true }).fill("mismatched password");
    await page
      .getByRole("button", {
        name: flow === "recovery" ? "重置密码" : "保存更改",
        exact: true,
      })
      .click();
    await expect(page.getByText("两次输入的密码不一致。")).toBeVisible();
    await page.getByLabel("再次输入密码", { exact: true }).fill(password);
    await page
      .getByRole("button", {
        name: flow === "recovery" ? "重置密码" : "保存更改",
        exact: true,
      })
      .click();
    await expect(page.getByRole("heading", { name: "保存您的恢复代码" })).toBeVisible();
    const nextRecoveryCode = (await page.locator("code").textContent())!;
    expect(nextRecoveryCode).not.toBe(recoveryCode);
    recoveryCode = nextRecoveryCode;
    await page.getByRole("button", { name: "我已保存恢复代码" }).click();
    await page.getByRole("button", { name: profile.name, exact: true }).click();
    await page.getByLabel("密码", { exact: true }).fill(password);
    await page.getByRole("button", { name: "解锁", exact: true }).click();
    await ready(page, profile.name);
    previousPassword = password;
  }
});
