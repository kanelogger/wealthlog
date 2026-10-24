import { test, expect } from "@playwright/test";

test("profile settings omit avatar configuration and preserve existing profile data", async ({
  page,
  request,
}) => {
  const command = async (name: string, data = {}, scope?: string) => {
    const response = await request.post(`/api/v1/profiles/${name}`, {
      data,
      headers: scope ? { "x-wf-profile-scope": scope } : {},
    });
    expect(response.ok()).toBeTruthy();
    return response.json();
  };
  const state = await command("get_profile_state");
  const profile = state.profiles[0] ?? (await command("create_profile", { name: "Personal" }));
  const grant = await command("unlock_profile", { profileId: profile.id });
  await command(
    "update_profile",
    { name: "Personal", avatarId: "clay-pebble-animated" },
    grant.scopeId,
  );
  const settings = await request.put("/api/v1/settings", {
    headers: { "x-wf-profile-scope": grant.scopeId },
    data: { onboardingCompleted: true },
  });
  expect(settings.ok()).toBeTruthy();
  await page.goto("/");
  await page.getByRole("button", { name: "Personal 的个人资料菜单" }).click();
  await page.getByRole("menuitem", { name: "个人资料设置" }).click();
  await expect(page.getByLabel("名称", { exact: true })).toHaveValue("Personal");
  await expect(page.getByRole("group", { name: "头像风格" })).toHaveCount(0);
  await expect(page.locator("form .profile-lock-avatar")).toHaveCount(0);
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.getByLabel("名称", { exact: true })).toBeVisible();
  await page.getByLabel("名称", { exact: true }).fill("Renamed");
  await page.getByRole("button", { name: /^(?:保存(?:更改)?)$/ }).click();
  await expect(page.locator(".app-shell").first()).toBeVisible();
  const updated = await command("get_profile_state");
  expect(updated.profiles.find((entry: { id: string }) => entry.id === profile.id)).toMatchObject({
    name: "Renamed",
    avatarId: "clay-pebble-animated",
  });
});
